//! `dict` for native code: entries of key then value slots in insertion order, found by the
//! canonical encoding of the key's slots ([`Enc`]), so equal keys (`Str` by content, `-0.0` as
//! `0.0`) are one entry and the order matches the interpreter's (17.3, R475).

use crate::keyhash::Enc;

use std::collections::HashMap;

/// How a key's slots encode, from the schema text the lowering builds:
/// `i<bits>`, `u<bits>`, `f`, `b`, `n` (unit), `s` (`Str`), `(..)` tuple,
/// `{name|..}` record, `<name|..;..>` sum (a tag slot, then every variant's fields).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Key {
    Int(u32, bool),
    Float,
    Bool,
    Unit,
    Str,
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
            Key::Tuple(_) => "tuple",
            Key::Rec(..) | Key::Sum(_) => "record",
        }
    }

    /// Order the elements `a` and `b` of equal layout from slot `*at`, as `Ord` does.
    ///
    /// # Safety
    /// Every `Str` slot holds a live `Str` block.
    pub(crate) unsafe fn cmp(&self, a: &[u64], b: &[u64], at: &mut usize) -> std::cmp::Ordering {
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
            // SAFETY: the caller's contract.
            Key::Str => unsafe { (*(x as *const String)).cmp(&*(y as *const String)) },
            Key::Tuple(ks) | Key::Rec(_, ks) => {
                for k in ks {
                    // SAFETY: forwarded.
                    let o = unsafe { k.cmp(a, b, at) };
                    if o != Equal {
                        return o;
                    }
                }
                return Equal;
            }
            Key::Sum(vs) => {
                if x != y {
                    return x.cmp(&y);
                }
                *at += 1;
                for (i, (_, ks)) in vs.iter().enumerate() {
                    if i as u64 == x {
                        for k in ks {
                            // SAFETY: forwarded.
                            let o = unsafe { k.cmp(a, b, at) };
                            if o != Equal {
                                return o;
                            }
                        }
                    } else {
                        *at += ks.iter().map(Key::width).sum::<usize>();
                    }
                }
                return Equal;
            }
        };
        *at += 1;
        ord
    }

    /// Encode the slots `w[*at..]`, advancing `at` past them.
    ///
    /// # Safety
    /// A `Str` slot holds a live `Str` block.
    unsafe fn enc(&self, w: &[u64], at: &mut usize, out: &mut Enc) {
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
            // SAFETY: the caller's contract.
            Key::Str => out.text(unsafe { &*(x as *const String) }),
            Key::Tuple(ks) => {
                out.seq(ks.len());
                for k in ks {
                    // SAFETY: forwarded.
                    unsafe { k.enc(w, at, out) };
                }
                return;
            }
            Key::Rec(name, ks) => {
                out.data(name);
                for k in ks {
                    // SAFETY: forwarded.
                    unsafe { k.enc(w, at, out) };
                }
                return;
            }
            Key::Sum(vs) => {
                *at += 1;
                for (i, (name, ks)) in vs.iter().enumerate() {
                    if i as u64 == x {
                        out.data(name);
                        for k in ks {
                            // SAFETY: forwarded.
                            unsafe { k.enc(w, at, out) };
                        }
                    } else {
                        *at += ks.iter().map(Key::width).sum::<usize>();
                    }
                }
                return;
            }
        }
        *at += 1;
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

pub struct Map {
    key: Key,
    kw: usize,
    vw: usize,
    /// Entries of `kw + vw` slots, in insertion order.
    data: Vec<u64>,
    index: HashMap<Vec<u8>, usize>,
}

impl Map {
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
            index: HashMap::new(),
        }))
    }

    /// Each entry's key then value slots, in order.
    pub(crate) fn entries(&self) -> impl Iterator<Item = &[u64]> {
        self.data.chunks(self.stride())
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
        let mut e = Enc::new();
        // SAFETY: as above.
        unsafe { self.key.enc(w, &mut 0, &mut e) };
        e.into_bytes()
    }

    fn entry(&mut self, i: usize) -> *mut u64 {
        let s = self.stride();
        self.data[i * s..].as_mut_ptr()
    }

    /// # Safety
    /// As [`Map::encode`].
    unsafe fn find(&self, key: *const u64) -> Option<usize> {
        // SAFETY: forwarded.
        self.index.get(&unsafe { self.encode(key) }).copied()
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
    let i = m.len();
    m.index.insert(enc, i);
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
    for j in m.index.values_mut() {
        if *j > i {
            *j -= 1;
        }
    }
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
