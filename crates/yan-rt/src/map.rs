//! `dict` for native code: entries of key then value slots in insertion order, found by the
//! canonical encoding of the key's slots ([`Enc`]), so equal keys (`Str` by content, `-0.0` as
//! `0.0`) are one entry and the order matches the interpreter's (17.3, R475).

use crate::keyhash::Enc;

use std::borrow::Cow;
use std::collections::HashMap;

/// How a key's slots encode, from the schema text the lowering builds:
/// `i<bits>`, `u<bits>`, `f`, `b`, `n` (unit), `s` (`Str`), `y` (`bytes`), `(..)` tuple,
/// `{name|..}` record, `<name|..;..>` sum (a tag slot, then every variant's fields).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Key {
    Int(u32, bool),
    Float,
    Bool,
    Unit,
    Str,
    /// A `Path` block: a `Str` block ordered by its parts.
    Path,
    /// A `WindowsPath` block: a `Str` block ordered, equal and hashed by its folded parts.
    WinPath,
    /// A `bytes` block (`yan_rt::bytes`).
    Bytes,
    Tuple(Vec<Key>),
    Rec(String, Vec<Key>),
    Sum(Vec<(String, Vec<Key>)>),
}

impl Key {
    fn width(&self) -> usize {
        match self {
            Key::Tuple(ks) | Key::Rec(_, ks) => ks.iter().map(Key::width).sum(),
            Key::Sum(vs) => {
                1 + vs
                    .iter()
                    .flat_map(|(_, ks)| ks)
                    .map(Key::width)
                    .sum::<usize>()
            }
            _ => 1,
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            Key::Int(..) => "int",
            Key::Float => "float",
            Key::Bool => "bool",
            Key::Unit => "value",
            Key::Str => "text",
            Key::Path | Key::WinPath => "text",
            Key::Bytes => "bytes",
            Key::Tuple(_) => "tuple",
            Key::Rec(..) | Key::Sum(_) => "record",
        }
    }

    /// Whether the elements `a` and `b` of equal layout are equal from slot `*at`, as `==` reads
    /// them: a NaN is not equal to anything.
    ///
    /// # Safety
    /// Every `Str` slot holds a live `Str` block.
    pub(crate) unsafe fn eq(&self, a: &[u64], b: &[u64], at: &mut usize) -> bool {
        match self.eq_in(a, b, at, &Raw) {
            Ok(eq) => eq,
            Err(e) => match e {},
        }
    }

    /// [`Key::eq`] with the blocks read through `blk`.
    pub fn eq_in<B: Blocks>(
        &self,
        a: &[u64],
        b: &[u64],
        at: &mut usize,
        blk: &B,
    ) -> Result<bool, B::Err> {
        Ok(match self {
            Key::Float => {
                let eq = f64::from_bits(a[*at]) == f64::from_bits(b[*at]);
                *at += 1;
                eq
            }
            Key::Tuple(ks) | Key::Rec(_, ks) => {
                let mut eq = true;
                for k in ks {
                    // Every field is read, so `at` ends past the element.
                    eq &= k.eq_in(a, b, at, blk)?;
                }
                eq
            }
            Key::Sum(vs) => {
                if a[*at] != b[*at] {
                    return Ok(false);
                }
                let tag = a[*at];
                *at += 1;
                let mut eq = true;
                for (i, (_, ks)) in vs.iter().enumerate() {
                    if i as u64 == tag {
                        for k in ks {
                            eq &= k.eq_in(a, b, at, blk)?;
                        }
                    } else {
                        *at += ks.iter().map(Key::width).sum::<usize>();
                    }
                }
                eq
            }
            _ => self.cmp_in(a, b, at, blk)?.is_eq(),
        })
    }

    /// Order the elements `a` and `b` of equal layout from slot `*at`, as `Ord` does.
    ///
    /// # Safety
    /// Every `Str` slot holds a live `Str` block.
    pub(crate) unsafe fn cmp(&self, a: &[u64], b: &[u64], at: &mut usize) -> std::cmp::Ordering {
        match self.cmp_in(a, b, at, &Raw) {
            Ok(o) => o,
            Err(e) => match e {},
        }
    }

    /// [`Key::cmp`] with the blocks read through `blk`.
    pub fn cmp_in<B: Blocks>(
        &self,
        a: &[u64],
        b: &[u64],
        at: &mut usize,
        blk: &B,
    ) -> Result<std::cmp::Ordering, B::Err> {
        use std::cmp::Ordering::Equal;
        let (x, y) = (a[*at], b[*at]);
        let ord = match self {
            Key::Int(bits, signed) => {
                let sh = 64 - bits;
                let n = |v: u64| {
                    if *signed {
                        i128::from(((v << sh) as i64) >> sh)
                    } else {
                        i128::from((v << sh) >> sh)
                    }
                };
                n(x).cmp(&n(y))
            }
            Key::Float => f64::from_bits(x)
                .partial_cmp(&f64::from_bits(y))
                .unwrap_or(Equal),
            Key::Bool => (x as u8 != 0).cmp(&(y as u8 != 0)),
            Key::Unit => Equal,
            Key::Str => blk.text(x)?.cmp(&blk.text(y)?),
            Key::Path => {
                let (x, y) = (blk.text(x)?, blk.text(y)?);
                let parts = |p: &str| crate::path::Parsed::posix(p).parts();
                parts(&x).cmp(&parts(&y))
            }
            Key::WinPath => {
                let (x, y) = (blk.text(x)?, blk.text(y)?);
                let parts = |p: &str| crate::path::Parsed::windows(p).folded_parts();
                parts(&x).cmp(&parts(&y))
            }
            Key::Bytes => blk.bytes(x)?.cmp(&blk.bytes(y)?),
            Key::Tuple(ks) | Key::Rec(_, ks) => {
                for k in ks {
                    let o = k.cmp_in(a, b, at, blk)?;
                    if o != Equal {
                        return Ok(o);
                    }
                }
                return Ok(Equal);
            }
            Key::Sum(vs) => {
                if x != y {
                    return Ok(x.cmp(&y));
                }
                *at += 1;
                for (i, (_, ks)) in vs.iter().enumerate() {
                    if i as u64 == x {
                        for k in ks {
                            let o = k.cmp_in(a, b, at, blk)?;
                            if o != Equal {
                                return Ok(o);
                            }
                        }
                    } else {
                        *at += ks.iter().map(Key::width).sum::<usize>();
                    }
                }
                return Ok(Equal);
            }
        };
        *at += 1;
        Ok(ord)
    }

    /// Encode the slots `w[*at..]`, advancing `at` past them.
    fn enc<B: Blocks>(
        &self,
        w: &[u64],
        at: &mut usize,
        out: &mut Enc,
        b: &B,
    ) -> Result<(), B::Err> {
        let x = w[*at];
        match self {
            Key::Int(bits, signed) => {
                let sh = 64 - bits;
                let n = if *signed {
                    i128::from(((x << sh) as i64) >> sh)
                } else {
                    i128::from((x << sh) >> sh)
                };
                out.int(n);
            }
            Key::Float => out.float(f64::from_bits(x)),
            Key::Bool => out.bool(x as u8 != 0),
            Key::Unit => out.unit(),
            Key::Str | Key::Path => out.text(&b.text(x)?),
            Key::WinPath => out.win_path(&b.text(x)?.to_lowercase()),
            Key::Bytes => out.bytes(&b.bytes(x)?),
            Key::Tuple(ks) => {
                out.seq(ks.len());
                for k in ks {
                    k.enc(w, at, out, b)?;
                }
                return Ok(());
            }
            Key::Rec(name, ks) => {
                out.data(name);
                for k in ks {
                    k.enc(w, at, out, b)?;
                }
                return Ok(());
            }
            Key::Sum(vs) => {
                *at += 1;
                for (i, (name, ks)) in vs.iter().enumerate() {
                    if i as u64 == x {
                        out.data(name);
                        for k in ks {
                            k.enc(w, at, out, b)?;
                        }
                    } else {
                        *at += ks.iter().map(Key::width).sum::<usize>();
                    }
                }
                return Ok(());
            }
        }
        *at += 1;
        Ok(())
    }
}

/// Whether the element at `a` equals the one at `b`, both laid out as `schema` says.
///
/// # Safety
/// `schema` is a `Str` block; `a` and `b` point to elements of its layout with live `Str` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_key_eq(schema: *const String, a: *const u64, b: *const u64) -> u8 {
    // SAFETY: the caller's contract.
    unsafe {
        let key = parse(&mut (*schema).as_str());
        let w = key.width();
        u8::from(key.eq(
            std::slice::from_raw_parts(a, w),
            std::slice::from_raw_parts(b, w),
            &mut 0,
        ))
    }
}

/// # Safety
/// As [`yan_rt_key_eq`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_key_cmp(
    schema: *const String,
    a: *const u64,
    b: *const u64,
) -> i64 {
    // SAFETY: the caller's contract.
    unsafe {
        let key = parse(&mut (*schema).as_str());
        let w = key.width();
        key.cmp(
            std::slice::from_raw_parts(a, w),
            std::slice::from_raw_parts(b, w),
            &mut 0,
        ) as i64
    }
}

pub(crate) fn parse(s: &mut &str) -> Key {
    let c = s.as_bytes()[0];
    *s = &s[1..];
    let digits = |s: &mut &str| {
        let n = s.bytes().take_while(u8::is_ascii_digit).count();
        let (d, rest) = s.split_at(n);
        *s = rest;
        d.parse().expect("bits")
    };
    let name = |s: &mut &str| {
        let (n, rest) = s.split_once('|').expect("a name");
        *s = rest;
        n.to_owned()
    };
    let until = |s: &mut &str, end: u8| {
        let mut ks = Vec::new();
        while s.as_bytes()[0] != end {
            ks.push(parse(s));
        }
        *s = &s[1..];
        ks
    };
    match c {
        b'i' => Key::Int(digits(s), true),
        b'u' => Key::Int(digits(s), false),
        b'f' => Key::Float,
        b'b' => Key::Bool,
        b'n' => Key::Unit,
        b's' => Key::Str,
        b'y' => Key::Bytes,
        b'p' => Key::Path,
        b'w' => Key::WinPath,
        b'(' => Key::Tuple(until(s, b')')),
        b'{' => {
            let n = name(s);
            Key::Rec(n, until(s, b'}'))
        }
        b'<' => {
            let mut vs = Vec::new();
            while s.as_bytes()[0] != b'>' {
                let n = name(s);
                vs.push((n, until(s, b';')));
            }
            *s = &s[1..];
            Key::Sum(vs)
        }
        _ => panic!("map key schema"),
    }
}

/// Reads the blocks a key's slots point to, so native code (raw pointers) and the machine
/// (abstract memory) encode keys with one routine.
pub trait Blocks {
    type Err;
    fn text(&self, block: u64) -> Result<Cow<'_, str>, Self::Err>;
    fn bytes(&self, block: u64) -> Result<Cow<'_, [u8]>, Self::Err>;
}

/// Blocks as native code holds them: slots are live pointers.
struct Raw;

impl Blocks for Raw {
    type Err = std::convert::Infallible;

    fn text(&self, block: u64) -> Result<Cow<'_, str>, Self::Err> {
        // SAFETY: a `Str` slot holds a live `Str` block.
        Ok(Cow::Borrowed(unsafe { &*(block as *const String) }))
    }

    fn bytes(&self, block: u64) -> Result<Cow<'_, [u8]>, Self::Err> {
        // SAFETY: as above, for `bytes` blocks.
        Ok(Cow::Borrowed(unsafe { &*(block as *const Vec<u8>) }))
    }
}

/// A map's key layout: the schema parsed once.
#[derive(Debug, Clone)]
pub struct Layout(Key);

impl Layout {
    pub fn parse(schema: &str) -> Layout {
        Layout(parse(&mut &*schema))
    }

    pub fn width(&self) -> usize {
        self.0.width()
    }

    /// Order the elements in slots `a` and `b`, as `Ord` does on the keys they hold.
    pub fn cmp<B: Blocks>(
        &self,
        a: &[u64],
        b: &[u64],
        blocks: &B,
    ) -> Result<std::cmp::Ordering, B::Err> {
        self.0.cmp_in(a, b, &mut 0, blocks)
    }

    /// Whether the elements in slots `a` and `b` are equal, as `==` reads them.
    pub fn eq<B: Blocks>(&self, a: &[u64], b: &[u64], blocks: &B) -> Result<bool, B::Err> {
        self.0.eq_in(a, b, &mut 0, blocks)
    }

    /// The canonical encoding of the key in slots `w`; equal keys encode equal.
    pub fn encode<B: Blocks>(&self, w: &[u64], blocks: &B) -> Result<Vec<u8>, B::Err> {
        let mut e = Enc::new();
        self.0.enc(w, &mut 0, &mut e, blocks)?;
        Ok(e.into_bytes())
    }
}

/// Entry positions by encoded key, in insertion order: removal closes the gap.
#[derive(Debug, Clone, Default)]
pub struct KeyIndex(HashMap<Vec<u8>, usize>);

impl KeyIndex {
    pub fn find(&self, enc: &[u8]) -> Option<usize> {
        self.0.get(enc).copied()
    }

    /// Adds `enc` as the last entry and returns its position.
    pub fn push(&mut self, enc: Vec<u8>) -> usize {
        let i = self.0.len();
        self.0.insert(enc, i);
        i
    }

    /// Removes `enc`, returning its former position.
    pub fn remove(&mut self, enc: &[u8]) -> Option<usize> {
        let i = self.0.remove(enc)?;
        for j in self.0.values_mut() {
            if *j > i {
                *j -= 1;
            }
        }
        Some(i)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub struct Map {
    key: Key,
    kw: usize,
    vw: usize,
    /// Entries of `kw + vw` slots, in insertion order.
    data: Vec<u64>,
    index: KeyIndex,
}

impl Map {
    /// The entries' slots, `kw + vw` each, in insertion order.
    pub(crate) fn slots(&self) -> &[u64] {
        &self.data
    }

    /// A new block: `kw`-slot keys encoded by `schema`, `vw`-slot values.
    pub(crate) fn new(schema: &str, kw: usize, vw: usize) -> *mut Map {
        let mut s = schema;
        let key = parse(&mut s);
        debug_assert_eq!(key.width(), kw);
        crate::heap::block_taken();
        Box::into_raw(Box::new(Map {
            key,
            kw,
            vw,
            data: Vec::new(),
            index: KeyIndex::default(),
        }))
    }

    fn stride(&self) -> usize {
        self.kw + self.vw
    }

    fn len(&self) -> usize {
        self.index.len()
    }

    /// # Safety
    /// `key` points to `kw` slots of a live key.
    unsafe fn encode(&self, key: *const u64) -> Vec<u8> {
        // SAFETY: the caller's contract.
        let w = unsafe { std::slice::from_raw_parts(key, self.kw) };
        match self.key_enc(w) {
            Ok(e) => e,
            Err(never) => match never {},
        }
    }

    fn entry(&mut self, i: usize) -> *mut u64 {
        let s = self.stride();
        self.data[i * s..].as_mut_ptr()
    }

    /// # Safety
    /// As [`Map::encode`].
    unsafe fn find(&self, key: *const u64) -> Option<usize> {
        // SAFETY: forwarded.
        self.index.find(&unsafe { self.encode(key) })
    }

    fn key_enc(&self, w: &[u64]) -> Result<Vec<u8>, std::convert::Infallible> {
        let mut e = Enc::new();
        self.key.enc(w, &mut 0, &mut e, &Raw)?;
        Ok(e.into_bytes())
    }
}

/// A dict of `kw`-slot keys encoded by `schema` and `vw`-slot values.
///
/// # Safety
/// `schema` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_map_new(kw: u64, vw: u64, schema: *const String) -> *mut Map {
    // SAFETY: the caller's contract.
    Map::new(unsafe { &*schema }, kw as usize, vw as usize)
}

/// The value slots of `key`'s entry, or null.
///
/// # Safety
/// `m` is a `Map` block; `key` points to its key slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_map_find(m: *mut Map, key: *const u64) -> *mut u64 {
    // SAFETY: the caller's contract.
    let m = unsafe { &mut *m };
    // SAFETY: as above.
    match unsafe { m.find(key) } {
        Some(i) => m.entry(i).wrapping_add(m.kw),
        None => std::ptr::null_mut(),
    }
}

/// The value slots of `key`'s entry; a `bounds` panic with `key_hash` and `key_type` when there
/// is none (R884).
///
/// # Safety
/// As [`yan_rt_map_find`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_map_index(m: *mut Map, key: *const u64) -> *mut u64 {
    // SAFETY: the caller's contract.
    let p = unsafe { yan_rt_map_find(m, key) };
    if p.is_null() {
        // SAFETY: as above.
        let (enc, ty) = unsafe { ((*m).encode(key), (*m).key.type_name()) };
        let k = "bounds";
        // SAFETY: static strings and a live buffer.
        unsafe {
            crate::abi::yan_rt_panic_key(
                k.as_ptr(),
                k.len(),
                enc.as_ptr(),
                enc.len(),
                ty.as_ptr(),
                ty.len(),
            )
        }
    }
    p
}

/// Add an entry for `key`, which is not in the dict: its slots are copied in, and the entry's
/// value slots, valid until the dict next changes, are returned for the caller to fill.
///
/// # Safety
/// `m` is a `Map` block that nothing else borrows; `key` points to its key slots, which the
/// dict owns from now on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_map_push(m: *mut Map, key: *const u64) -> *mut u64 {
    // SAFETY: the caller's contract.
    let m = unsafe { &mut *m };
    // SAFETY: as above.
    let enc = unsafe { m.encode(key) };
    let i = m.index.push(enc);
    // SAFETY: as above.
    m.data
        .extend_from_slice(unsafe { std::slice::from_raw_parts(key, m.kw) });
    m.data.resize(m.data.len() + m.vw, 0);
    m.entry(i).wrapping_add(m.kw)
}

/// Remove `key`'s entry, keeping the order of the rest: its key then value slots go to `out`
/// and the result is 1; 0 when there is none.
///
/// # Safety
/// As [`yan_rt_map_push`]; `out` points to `kw + vw` writable slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_map_remove(m: *mut Map, key: *const u64, out: *mut u64) -> u64 {
    // SAFETY: the caller's contract.
    let m = unsafe { &mut *m };
    // SAFETY: as above.
    let enc = unsafe { m.encode(key) };
    let Some(i) = m.index.remove(&enc) else {
        return 0;
    };
    let s = m.stride();
    // SAFETY: as above.
    let out = unsafe { std::slice::from_raw_parts_mut(out, s) };
    out.copy_from_slice(&m.data[i * s..(i + 1) * s]);
    m.data.drain(i * s..(i + 1) * s);
    1
}

/// # Safety
/// `m` is a `Map` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_map_len(m: *const Map) -> i64 {
    // SAFETY: the caller's contract.
    unsafe { (*m).len() as i64 }
}

/// The key then value slots of the `i`th entry in order; `i` is below the length.
///
/// # Safety
/// `m` is a `Map` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_map_at(m: *mut Map, i: i64) -> *mut u64 {
    // SAFETY: the caller's contract.
    unsafe { (*m).entry(i as usize) }
}

/// A copy of the slots; the caller copies the blocks they hold.
///
/// # Safety
/// `m` is a `Map` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_map_copy(m: *const Map) -> *mut Map {
    // SAFETY: the caller's contract.
    let m = unsafe { &*m };
    crate::heap::block_taken();
    Box::into_raw(Box::new(Map {
        key: m.key.clone(),
        kw: m.kw,
        vw: m.vw,
        data: m.data.clone(),
        index: m.index.clone(),
    }))
}

/// Free a `Map` block, not the blocks its slots hold.
///
/// # Safety
/// `m` is a `Map` block that nothing uses afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_map_free(m: *mut Map) {
    crate::heap::block_freed();
    // SAFETY: the caller's contract; the block came from `Box::into_raw`.
    drop(unsafe { Box::from_raw(m) });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_parses_nested_keys() {
        let k = parse(&mut "({P|i64s}<Some|f;None|;>b)");
        assert_eq!(
            k,
            Key::Tuple(vec![
                Key::Rec("P".into(), vec![Key::Int(64, true), Key::Str]),
                Key::Sum(vec![
                    ("Some".into(), vec![Key::Float]),
                    ("None".into(), vec![])
                ]),
                Key::Bool,
            ])
        );
        assert_eq!(k.width(), 5);
    }

    #[test]
    fn insertion_order_survives_removal() {
        let schema = "i64".to_owned();
        // SAFETY: live blocks and slots throughout.
        unsafe {
            let m = yan_rt_map_new(1, 1, &schema);
            for k in [3u64, 1, 2] {
                *yan_rt_map_push(m, &k) = k * 10;
            }
            let mut out = [0u64; 2];
            assert_eq!(yan_rt_map_remove(m, &1, out.as_mut_ptr()), 1);
            assert_eq!(out, [1, 10]);
            assert_eq!(yan_rt_map_len(m), 2);
            assert_eq!(*yan_rt_map_at(m, 1), 2);
            assert_eq!(*yan_rt_map_find(m, &2), 20);
            assert!(yan_rt_map_find(m, &1).is_null());
            yan_rt_map_free(m);
        }
    }

    #[test]
    fn negative_zero_is_one_key() {
        let schema = "f".to_owned();
        // SAFETY: as above.
        unsafe {
            let m = yan_rt_map_new(1, 1, &schema);
            *yan_rt_map_push(m, &0f64.to_bits()) = 7;
            assert_eq!(*yan_rt_map_find(m, &(-0f64).to_bits()), 7);
            yan_rt_map_free(m);
        }
    }
}
