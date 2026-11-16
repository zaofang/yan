//! The yan-rt entry points the machine models, over abstract memory. A `Str` block is one
//! allocation of its UTF-8 bytes. A `List` block is a handle allocation; its elements are
//! `words` 8-byte slots each in a data allocation that is replaced when the list grows.

use crate::mem::Byte;
use crate::{Machine, Stop, Val, unsupported};
use std::io::Write;
use yan_mir::RtFn;
use yan_rt::jsonslots::Slot;
use yan_rt::sched;

#[derive(Clone, Copy)]
pub struct List {
    words: usize,
    len: usize,
    cap: usize,
    data: u64,
    /// A deque's `maxlen`, or -1.
    limit: i64,
}

/// A `Map` block: its entries are `kw + vw` slots each in a data allocation that is replaced
/// when the map grows or shrinks. The key layout and order index are yan-rt's, as native uses.
pub struct Map {
    layout: yan_rt::map::Layout,
    kw: usize,
    vw: usize,
    index: yan_rt::map::KeyIndex,
    data: u64,
}

/// Resolves `Str` and `bytes` slots against the machine's memory.
struct MemBlocks<'a>(&'a Machine<'a>);

fn word_bytes(words: &[u64]) -> Vec<Byte> {
    words
        .iter()
        .flat_map(|w| w.to_le_bytes().map(Byte::Init))
        .collect()
}

fn words_of(bytes: &[Byte]) -> Vec<u64> {
    bytes
        .chunks(8)
        .map(|c| {
            let mut w = [0u8; 8];
            for (d, b) in w.iter_mut().zip(c) {
                if let Byte::Init(x) = b {
                    *d = *x;
                }
            }
            u64::from_le_bytes(w)
        })
        .collect()
}

fn time(hi: Val, lo: Val) -> i128 {
    yan_rt::timeabi::join(hi.int() as i64, lo.int() as i64)
}

impl Machine<'_> {
    /// A list's slot width and its elements' bytes.
    fn slots(&mut self, l: Val) -> Result<(usize, Vec<Byte>), Stop> {
        let List {
            words, len, data, ..
        } = *self.list(l)?;
        let bytes = self.mem.read(data, 8 * words * len).unwrap_or_default();
        Ok((words, bytes))
    }

    /// Replaces a list's elements with `slots`, in a fresh data block.
    fn set_slots(&mut self, l: Val, slots: Vec<Byte>) -> Result<(), Stop> {
        let words = self.list(l)?.words;
        let (old, limit) = (self.list(l)?.data, self.list(l)?.limit);
        let len = slots.len().checked_div(8 * words).unwrap_or(0);
        let data = self.mem.alloc(slots.len());
        self.mem.write(data, &slots);
        self.mem.free(old);
        *self.list(l)? = List {
            words,
            len,
            cap: len,
            data,
            limit,
        };
        Ok(())
    }

    fn map(&mut self, v: Val) -> Result<&mut Map, Stop> {
        self.maps
            .get_mut(&v.addr())
            .ok_or_else(|| Stop::Fault(format!("{:#x} is not a Map block", v.addr())))
    }

    /// `n` slots at `addr`; uninitialised bytes read as zero.
    fn words(&self, addr: u64, n: usize) -> Result<Vec<u64>, Stop> {
        let bytes = self
            .mem
            .read(addr, 8 * n)
            .ok_or_else(|| Stop::Fault(format!("slots at {addr:#x} are not allocated")))?;
        Ok(words_of(&bytes))
    }

    /// The words of a map's entries, keys and values in order.
    fn map_words(&mut self, m: Val) -> Result<Vec<u64>, Stop> {
        Ok(words_of(&self.entries(m)?))
    }

    /// The texts of a list of `Str` blocks.
    fn strs(&mut self, l: Val) -> Result<Vec<String>, Stop> {
        self.list_words(l)?
            .into_iter()
            .map(|w| self.str_of(w))
            .collect()
    }

    fn encode_key(&mut self, m: Val, key: Val) -> Result<Vec<u8>, Stop> {
        let kw = self.map(m)?.kw;
        let w = self.words(key.addr(), kw)?;
        let layout = self.map(m)?.layout.clone();
        layout.encode(&w, &MemBlocks(self))
    }

    /// Replaces a map's data block with `slots`.
    fn set_entries(&mut self, m: Val, slots: Vec<Byte>) -> Result<u64, Stop> {
        let old = self.map(m)?.data;
        let data = self.mem.alloc(slots.len());
        self.mem.write(data, &slots);
        self.mem.free(old);
        self.map(m)?.data = data;
        Ok(data)
    }

    fn entries(&mut self, m: Val) -> Result<Vec<Byte>, Stop> {
        let Map {
            kw,
            vw,
            index,
            data,
            ..
        } = self.map(m)?;
        let n = 8 * (*kw + *vw) * index.len();
        let data = *data;
        Ok(self.mem.read(data, n).unwrap_or_default())
    }

    fn new_list(&mut self, words: usize, slots: Vec<Byte>) -> Val {
        let h = self.mem.alloc(8);
        let data = self.mem.alloc(slots.len());
        self.mem.write(data, &slots);
        let len = slots.len().checked_div(8 * words).unwrap_or(0);
        self.lists.insert(
            h,
            List {
                words,
                len,
                cap: len,
                data,
                limit: -1,
            },
        );
        Val::I(i128::from(h))
    }

    /// Words handed back by pointer, as native does with a thread-local. The block is reused and
    /// is not a leak.
    fn scratch_words(&mut self, words: &[i64]) -> Val {
        let at = *self.scratch.get_or_insert_with(|| self.mem.alloc(8 * 3));
        let words: Vec<u64> = words.iter().map(|w| *w as u64).collect();
        self.mem
            .write(at, &word_bytes(&words))
            .expect("scratch block is live");
        Val::I(i128::from(at))
    }

    /// A list's slots, flattened.
    fn list_words(&mut self, l: Val) -> Result<Vec<u64>, Stop> {
        let List {
            words, len, data, ..
        } = *self.list(l)?;
        self.words(data, words * len)
    }

    /// An actor's state slots at `state`.
    fn state_words(&self, actor: u64, state: u64) -> Result<Vec<u64>, Stop> {
        self.words(state, yan_rt::actor::words(actor))
    }

    fn write_words(&mut self, addr: u64, words: &[u64]) -> Result<(), Stop> {
        if words.is_empty() {
            return Ok(());
        }
        self.mem
            .write(addr, &word_bytes(words))
            .ok_or_else(|| self.bad_access("write", addr))
    }

    /// The blocks of `slots`, a decoded value that owns none yet, and the words that hold them.
    fn materialize(&mut self, slots: Vec<Slot>) -> Result<Vec<u64>, Stop> {
        slots
            .into_iter()
            .map(|s| match s {
                Slot::Word(w) => Ok(w),
                Slot::Str(s) => Ok(self.new_str(&s)),
                Slot::List(words, xs) => {
                    let data = self.materialize(xs)?;
                    Ok(self.new_list(words, word_bytes(&data)).addr())
                }
                Slot::Map {
                    schema,
                    kw,
                    vw,
                    entries,
                } => {
                    let layout = yan_rt::map::Layout::parse(&schema);
                    let mut index = yan_rt::map::KeyIndex::default();
                    let mut data = Vec::new();
                    for (k, v) in entries {
                        let (k, v) = (self.materialize(k)?, self.materialize(v)?);
                        index.push(layout.encode(&k, &MemBlocks(self))?);
                        data.extend(k);
                        data.extend(v);
                    }
                    let at = self.mem.alloc(8 * data.len());
                    self.write_words(at, &data)?;
                    let h = self.mem.alloc(8);
                    self.maps.insert(
                        h,
                        Map {
                            layout,
                            kw,
                            vw,
                            index,
                            data: at,
                        },
                    );
                    Ok(h)
                }
            })
            .collect()
    }

    /// Runs what actors dropped: a message's thunk drops its arguments, then its two blocks and
    /// start closures' captures are freed.
    pub(crate) fn dispose(&mut self) -> Result<(), Stop> {
        use yan_rt::actor::Disposal;
        loop {
            let queued = yan_rt::actor::take_disposals();
            if queued.is_empty() {
                return Ok(());
            }
            for d in queued {
                let blocks = match d {
                    Disposal::Env(env) => vec![env],
                    Disposal::Message([thunk, _, env, args]) => {
                        let id = yan_mir::FuncId::try_from(i128::from(thunk) - crate::FUNC_TAG)
                            .map_err(|_| {
                                Stop::Fault(format!("thunk {thunk:#x} is not a function"))
                            })?;
                        self.call(
                            id,
                            vec![Val::I(0), Val::I(i128::from(env)), Val::I(i128::from(args))],
                        )?;
                        vec![env, args]
                    }
                };
                for b in blocks.into_iter().filter(|b| *b != 0) {
                    self.free_block(b)?;
                }
            }
        }
    }

    /// Freeing a block twice is `ub(8)` under `--ub`.
    fn free_block(&mut self, addr: u64) -> Result<(), Stop> {
        if !self.mem.free(addr) && self.ub {
            return Err(Stop::Ub(8, format!("block {addr:#x} is dropped twice")));
        }
        Ok(())
    }

    pub(crate) fn new_str(&mut self, s: &str) -> u64 {
        let a = self.mem.alloc(s.len());
        let bytes: Vec<Byte> = s.bytes().map(Byte::Init).collect();
        self.mem.write(a, &bytes).expect("fresh allocation");
        a
    }

    fn str_of(&self, addr: u64) -> Result<String, Stop> {
        let n = self
            .mem
            .len(addr)
            .ok_or_else(|| Stop::Fault(format!("Str block {addr:#x} is not allocated")))?;
        let bytes = self.mem.read(addr, n).expect("inside its allocation");
        let raw: Vec<u8> = bytes
            .into_iter()
            .map(|b| if let Byte::Init(x) = b { x } else { 0 })
            .collect();
        String::from_utf8(raw).map_err(|_| Stop::Fault(format!("Str block {addr:#x} is not UTF-8")))
    }

    fn list(&mut self, v: Val) -> Result<&mut List, Stop> {
        self.lists
            .get_mut(&v.addr())
            .ok_or_else(|| Stop::Fault(format!("{:#x} is not a List block", v.addr())))
    }

    fn new_bytes(&mut self, b: Vec<u8>) -> Val {
        let h = self.mem.alloc(8);
        self.bytes.insert(h, b);
        Val::I(i128::from(h))
    }

    fn bytes_ref(&self, v: Val) -> Result<&Vec<u8>, Stop> {
        self.bytes
            .get(&v.addr())
            .ok_or_else(|| Stop::Fault(format!("{:#x} is not a bytes block", v.addr())))
    }

    fn bytes_mut(&mut self, v: Val) -> Result<&mut Vec<u8>, Stop> {
        self.bytes
            .get_mut(&v.addr())
            .ok_or_else(|| Stop::Fault(format!("{:#x} is not a bytes block", v.addr())))
    }

    fn element(&mut self, l: Val, i: i128) -> Result<Val, Stop> {
        let list = self.list(l)?;
        if !(0..list.len as i128).contains(&i) {
            return Err(Stop::Panic("bounds"));
        }
        Ok(Val::I(i128::from(list.data) + i * 8 * list.words as i128))
    }

    fn cli_parse(&mut self, args: Val, schema: Val, desc: Val) -> Result<Val, Stop> {
        let (words, len, data) = {
            let l = self.list(args)?;
            (l.words, l.len, l.data)
        };
        let args = self
            .words(data, words * len)?
            .into_iter()
            .map(|w| self.str_of(w))
            .collect::<Result<Vec<_>, _>>()?;
        let (schema, desc) = (self.str_of(schema.addr())?, self.str_of(desc.addr())?);
        let mut heap = MachineBlocks {
            m: self,
            fault: None,
        };
        let done = yan_rt::civil::cli_parse(&args, &schema, &desc, &mut heap);
        if let Some(e) = heap.fault {
            return Err(e);
        }
        let (status, out) = yan_rt::civil::outcome(done, |_| unreachable!("cli.parse has no Text"));
        self.civil_out = out;
        Ok(Val::I(i128::from(status)))
    }

    fn list_new(&mut self, words: usize) -> u64 {
        let h = self.mem.alloc(8);
        let data = self.mem.alloc(0);
        self.lists.insert(
            h,
            List {
                words,
                len: 0,
                cap: 0,
                data,
                limit: -1,
            },
        );
        h
    }

    /// Appends a zeroed element to the list `l` and returns its address.
    fn list_push(&mut self, l: Val) -> Result<u64, Stop> {
        let h = l.addr();
        let List {
            words,
            len,
            cap,
            data,
            limit,
        } = *self.list(l)?;
        let (mut data, mut cap) = (data, cap);
        if len == cap {
            cap = (cap * 2).max(4);
            let grown = self.mem.alloc(8 * words * cap);
            if let Some(bytes) = self.mem.read(data, 8 * words * len) {
                self.mem.write(grown, &bytes);
            }
            self.mem.free(data);
            data = grown;
        }
        let at = data + (8 * words * len) as u64;
        self.mem.write(at, &vec![Byte::Init(0); 8 * words]);
        *self.lists.get_mut(&h).expect("looked up above") = List {
            words,
            len: len + 1,
            cap,
            data,
            limit,
        };
        Ok(at)
    }

    pub(crate) fn rt(&mut self, f: RtFn, a: &[Val]) -> Result<Val, Stop> {
        let r = self.rt_call(f, a);
        self.deliver_stopped();
        r
    }

    /// Completes the `Stopped(Crashed)` messages yan-rt queued: their error text is a machine `Str`.
    pub(crate) fn deliver_stopped(&mut self) {
        for (ch, text) in yan_rt::actor::take_stopped() {
            let s = self.new_str(&text);
            yan_rt::actor::deliver_stopped(ch, s);
        }
    }

    fn rt_call(&mut self, f: RtFn, a: &[Val]) -> Result<Val, Stop> {
        Ok(match f {
            RtFn::Print => {
                let s = self.str_of(a[0].addr())?;
                let mut out = std::io::stdout().lock();
                writeln!(out, "{s}")
                    .and_then(|()| out.flush())
                    .map_err(|e| Stop::Fault(e.to_string()))?;
                Val::U
            }
            RtFn::FailMain => return Err(Stop::Fail(self.str_of(a[0].addr())?, a[1].int() as i64)),
            RtFn::ShowI64 => Val::I(i128::from(self.new_str(&(a[0].int() as i64).to_string()))),
            RtFn::ShowU64 => Val::I(i128::from(self.new_str(&(a[0].int() as u64).to_string()))),
            RtFn::ShowF64 => Val::I(i128::from(
                self.new_str(&yan_rt::fmt::float_repr(a[0].float())),
            )),
            RtFn::ShowBool => Val::I(i128::from(self.new_str(if a[0].truth() {
                "True"
            } else {
                "False"
            }))),
            RtFn::Concat => {
                let s = self.str_of(a[0].addr())? + &self.str_of(a[1].addr())?;
                Val::I(i128::from(self.new_str(&s)))
            }
            RtFn::Repr => {
                let s = yan_rt::fmt::repr(&self.str_of(a[0].addr())?);
                Val::I(i128::from(self.new_str(&s)))
            }
            RtFn::BytesOfStr => {
                let b = self.str_of(a[0].addr())?.into_bytes();
                self.new_bytes(b)
            }
            RtFn::BytesFromList => {
                let b = self.list_words(a[0])?.iter().map(|&w| w as u8).collect();
                self.new_bytes(b)
            }
            RtFn::BytesFree => {
                let h = a[0].addr();
                self.bytes.remove(&h);
                self.free_block(h)?;
                Val::U
            }
            RtFn::BytesCopy => {
                let b = self.bytes_ref(a[0])?.clone();
                self.new_bytes(b)
            }
            RtFn::BytesSet => {
                let b = self.bytes_mut(a[0])?;
                let at = yan_rt::bytes::index_of(a[1].int() as i64, b.len())
                    .ok_or(Stop::Panic("bounds"))?;
                b[at] = a[2].int() as u8;
                Val::U
            }
            RtFn::BytesPush => {
                self.bytes_mut(a[0])?.push(a[1].int() as u8);
                Val::U
            }
            RtFn::BytesExtend => {
                let x = self.bytes_ref(a[1])?.clone();
                self.bytes_mut(a[0])?.extend_from_slice(&x);
                Val::U
            }
            RtFn::BytesPop => Val::I(self.bytes_mut(a[0])?.pop().map_or(-1, i128::from)),
            RtFn::BytesClear => {
                self.bytes_mut(a[0])?.clear();
                Val::U
            }
            RtFn::BytesLen => Val::I(self.bytes_ref(a[0])?.len() as i128),
            RtFn::BytesConcat => {
                let b = [self.bytes_ref(a[0])?.as_slice(), self.bytes_ref(a[1])?].concat();
                self.new_bytes(b)
            }
            RtFn::BytesToList => {
                let ws: Vec<u64> = self.bytes_ref(a[0])?.iter().map(|&c| c.into()).collect();
                self.new_list(1, word_bytes(&ws))
            }
            RtFn::BytesStarts => Val::B(self.bytes_ref(a[0])?.starts_with(self.bytes_ref(a[1])?)),
            RtFn::BytesEnds => Val::B(self.bytes_ref(a[0])?.ends_with(self.bytes_ref(a[1])?)),
            RtFn::BytesHex => {
                let s = yan_rt::bytes::hex(self.bytes_ref(a[0])?);
                Val::I(i128::from(self.new_str(&s)))
            }
            RtFn::BytesLossy => {
                let s = String::from_utf8_lossy(self.bytes_ref(a[0])?).into_owned();
                Val::I(i128::from(self.new_str(&s)))
            }
            RtFn::BytesRepr => {
                let s = yan_rt::bytes::repr(self.bytes_ref(a[0])?);
                Val::I(i128::from(self.new_str(&s)))
            }
            RtFn::BytesIndex => {
                let b = self.bytes_ref(a[0])?;
                let at = yan_rt::bytes::index_of(a[1].int() as i64, b.len())
                    .ok_or(Stop::Panic("bounds"))?;
                let c = i64::from(b[at]);
                self.scratch_words(&[c])
            }
            RtFn::BytesAt => Val::I(i128::from(yan_rt::bytes::at(
                self.bytes_ref(a[0])?,
                a[1].int() as i64,
            ))),
            RtFn::BytesFind => Val::I(i128::from(yan_rt::bytes::find(
                self.bytes_ref(a[0])?,
                self.bytes_ref(a[1])?,
            ))),
            RtFn::BytesSlice => {
                let b = self.bytes_ref(a[0])?;
                let s = yan_rt::bytes::slice(b, a[1].int() as i64, a[2].int() as i64)
                    .ok_or(Stop::Panic("bounds"))?
                    .to_vec();
                self.new_bytes(s)
            }
            RtFn::CharCount => Val::I(self.str_of(a[0].addr())?.chars().count() as i128),
            RtFn::StrRepeat => {
                let r = self.str_of(a[0].addr())?.repeat(a[1].int().max(0) as usize);
                Val::I(i128::from(self.new_str(&r)))
            }
            RtFn::ParseInt => match yan_rt::text::parse_int(&self.str_of(a[0].addr())?) {
                Some(v) => {
                    self.write_words(a[1].addr(), &[v as u64])?;
                    Val::B(true)
                }
                None => Val::B(false),
            },
            RtFn::CharStr => {
                let c = char::from_u32(a[0].int() as u32).unwrap_or('\u{fffd}');
                Val::I(i128::from(self.new_str(&c.to_string())))
            }
            RtFn::StrChars => {
                let cs: Vec<u64> = yan_rt::chars::code_points(&self.str_of(a[0].addr())?).collect();
                self.new_list(1, word_bytes(&cs))
            }
            RtFn::KeyCmp => {
                let layout = yan_rt::map::Layout::parse(&self.str_of(a[0].addr())?);
                let w = layout.width();
                let (x, y) = (self.words(a[1].addr(), w)?, self.words(a[2].addr(), w)?);
                Val::I(layout.cmp(&x, &y, &MemBlocks(self))? as i128)
            }
            RtFn::KeyEq => {
                let layout = yan_rt::map::Layout::parse(&self.str_of(a[0].addr())?);
                let w = layout.width();
                let (x, y) = (self.words(a[1].addr(), w)?, self.words(a[2].addr(), w)?);
                Val::B(layout.eq(&x, &y, &MemBlocks(self))?)
            }
            RtFn::Utf8Check => Val::I(
                std::str::from_utf8(self.bytes_ref(a[0])?)
                    .map_or_else(|e| e.valid_up_to() as i128, |_| -1),
            ),
            RtFn::PathNew => {
                let p = yan_rt::path::Parsed::new(a[0].int() != 0, &self.str_of(a[1].addr())?);
                Val::I(i128::from(self.new_str(&p.text())))
            }
            RtFn::PathOp => {
                let s = yan_rt::path::op(
                    a[0].int() != 0,
                    a[1].int() as i64,
                    &self.str_of(a[2].addr())?,
                );
                Val::I(i128::from(self.new_str(&s)))
            }
            RtFn::PathWith => {
                let (p, n) = (self.str_of(a[2].addr())?, self.str_of(a[3].addr())?);
                match yan_rt::path::with(a[0].int() != 0, a[1].int() != 0, &p, &n) {
                    Some(s) => Val::I(i128::from(self.new_str(&s))),
                    None => return Err(Stop::Panic("value")),
                }
            }
            RtFn::PathJoin => {
                let (p, q) = (self.str_of(a[1].addr())?, self.str_of(a[2].addr())?);
                let s = yan_rt::path::join(a[0].int() != 0, &p, &q);
                Val::I(i128::from(self.new_str(&s)))
            }
            RtFn::PathIsAbsolute => Val::B(yan_rt::path::is_absolute(
                a[0].int() != 0,
                &self.str_of(a[1].addr())?,
            )),
            RtFn::PathStartsWith => {
                let (p, b) = (self.str_of(a[1].addr())?, self.str_of(a[2].addr())?);
                Val::B(yan_rt::path::starts_with(a[0].int() != 0, &p, &b))
            }
            RtFn::PathRelativeTo => {
                let (p, b) = (self.str_of(a[1].addr())?, self.str_of(a[2].addr())?);
                match yan_rt::path::relative_to(a[0].int() != 0, &p, &b) {
                    Some(s) => Val::I(i128::from(self.new_str(&s))),
                    None => Val::I(0),
                }
            }
            RtFn::PathParts => {
                let parts = yan_rt::path::parts(a[0].int() != 0, &self.str_of(a[1].addr())?);
                let ws: Vec<u64> = parts.iter().map(|p| self.new_str(p)).collect();
                self.new_list(1, word_bytes(&ws))
            }
            RtFn::PathWinPart => {
                let s = yan_rt::path::win_part(a[0].int() as i64, &self.str_of(a[1].addr())?);
                Val::I(i128::from(self.new_str(&s)))
            }
            RtFn::PathToPosix => match yan_rt::path::to_posix(&self.str_of(a[0].addr())?) {
                Some(s) => Val::I(i128::from(self.new_str(&s))),
                None => Val::I(0),
            },
            RtFn::WinPathEq => {
                let (x, y) = (self.str_of(a[0].addr())?, self.str_of(a[1].addr())?);
                Val::B(yan_rt::path::win_eq(&x, &y))
            }
            RtFn::StrStarts => Val::B(
                self.str_of(a[0].addr())?
                    .starts_with(&self.str_of(a[1].addr())?),
            ),
            RtFn::RandSeeded => Val::I(i128::from(yan_rt::rand::seeded(
                a[0].int() as u64,
                a[1].int() == 1,
            ))),
            RtFn::RandNext => Val::I(i128::from(yan_rt::rand::next(a[0].int() as i64) as i64)),
            RtFn::RandF64 => Val::F(yan_rt::rand::f64_from(
                yan_rt::rand::next(a[0].int() as i64),
            )),
            RtFn::RandBelow => {
                let h = a[0].int() as i64;
                Val::I(i128::from(
                    yan_rt::rand::below(a[1].int() as u64, || yan_rt::rand::next(h)) as i64,
                ))
            }
            RtFn::Math1 => Val::F(yan_rt::math::MATH1[a[0].int() as usize](a[1].float())),
            RtFn::Math2 => Val::F(yan_rt::math::MATH2[a[0].int() as usize](
                a[1].float(),
                a[2].float(),
            )),
            RtFn::CsvParse => {
                let (src, delim) = (self.str_of(a[1].addr())?, self.str_of(a[2].addr())?);
                let schema = self.str_of(a[3].addr())?;
                let out = self.materialize(yan_rt::csvslots::parse(&src, &delim, &schema))?;
                self.write_words(a[0].addr(), &out)?;
                Val::U
            }
            RtFn::StrSlice => {
                let b = |k: usize| (a[k].int() as i64, a[k + 1].int() as i64);
                let s = self.str_of(a[0].addr())?;
                let out =
                    yan_rt::slice::str_slice(&s, b(1), b(3), b(5)).ok_or(Stop::Panic("value"))?;
                Val::I(i128::from(self.new_str(&out)))
            }
            RtFn::ReadLine => match yan_rt::abi::read_line() {
                Some(l) => Val::I(i128::from(self.new_str(&l))),
                None => Val::I(0),
            },
            RtFn::ParseFloat => match yan_rt::text::parse_float(&self.str_of(a[0].addr())?) {
                Some(v) => {
                    self.write_words(a[1].addr(), &[v.to_bits()])?;
                    Val::B(true)
                }
                None => Val::B(false),
            },
            RtFn::HexCheck => Val::I(
                yan_rt::bytes::hex_decode(&self.str_of(a[0].addr())?)
                    .map_or_else(|at| at as i128, |_| -1),
            ),
            RtFn::HexDecode => {
                let b = yan_rt::bytes::hex_decode(&self.str_of(a[0].addr())?).unwrap_or_default();
                self.new_bytes(b)
            }
            RtFn::AbsI64 => Val::I(i128::from(
                (a[0].int() as i64)
                    .checked_abs()
                    .ok_or(Stop::Panic("overflow"))?,
            )),
            RtFn::CsvDump => {
                let (words, len, data) = {
                    let l = self.list(a[0])?;
                    (l.words, l.len, l.data)
                };
                let rows = self.words(data, words * len)?;
                let (delim, schema) = (self.str_of(a[1].addr())?, self.str_of(a[2].addr())?);
                let fault = std::cell::RefCell::new(None);
                let text = |w: u64| {
                    self.str_of(w).unwrap_or_else(|e| {
                        fault.borrow_mut().get_or_insert(e);
                        String::new()
                    })
                };
                let out = yan_rt::csvslots::dump(&rows, &delim, &schema, &text);
                if let Some(e) = fault.into_inner() {
                    return Err(e);
                }
                let out = out.ok_or(Stop::Panic("value"))?;
                Val::I(i128::from(self.new_str(&out)))
            }
            RtFn::HeapSort => {
                let layout = yan_rt::map::Layout::parse(&self.str_of(a[1].addr())?);
                let (w, data) = {
                    let l = self.list(a[0])?;
                    (l.words, l.data)
                };
                let flat = self.list_words(a[0])?;
                let mut order: Vec<&[u64]> = flat.chunks(w.max(1)).collect();
                let mut err = None;
                order.sort_by(|x, y| {
                    layout.cmp(x, y, &MemBlocks(self)).unwrap_or_else(|e| {
                        err = Some(e);
                        std::cmp::Ordering::Equal
                    })
                });
                if let Some(e) = err {
                    return Err(e);
                }
                let sorted: Vec<u64> = order.concat();
                self.write_words(data, &sorted)?;
                Val::U
            }
            RtFn::StrEq => Val::B(self.str_of(a[0].addr())? == self.str_of(a[1].addr())?),
            RtFn::StrFree => {
                // literals are static (R561)
                if !self.strs.contains(&Some(a[0].addr())) {
                    self.free_block(a[0].addr())?;
                }
                Val::U
            }
            RtFn::Alloc => Val::I(i128::from(self.mem.alloc(8 * (a[0].int() as usize + 1)))),
            RtFn::EnvFree => {
                self.free_block(a[0].addr())?;
                Val::U
            }
            RtFn::ListNew => Val::I(i128::from(self.list_new(a[0].int() as usize))),
            RtFn::ListPush => Val::I(i128::from(self.list_push(a[0])?)),
            RtFn::ListLen => Val::I(self.list(a[0])?.len as i128),
            RtFn::ListData => Val::I(i128::from(self.list(a[0])?.data)),
            RtFn::ListAt => self.element(a[0], a[1].int())?,
            RtFn::ListFree => {
                let h = a[0].addr();
                if let Some(l) = self.lists.remove(&h) {
                    self.mem.free(l.data);
                }
                self.free_block(h)?;
                Val::U
            }
            RtFn::PtrExpose => {
                self.exposed.insert(a[0].addr(), a[1].int());
                Val::U
            }
            RtFn::PtrExposed => Val::I(self.exposed.get(&a[0].addr()).copied().unwrap_or(0)),
            RtFn::ListCopy => {
                let (words, slots) = self.slots(a[0])?;
                self.new_list(words, slots)
            }
            RtFn::ListReverse => {
                let (words, mut slots) = self.slots(a[0])?;
                let mut elems: Vec<&[Byte]> = slots.chunks(8 * words.max(1)).collect();
                elems.reverse();
                slots = elems.concat();
                self.set_slots(a[0], slots)?;
                Val::U
            }
            RtFn::ListInsert => {
                let (words, mut slots) = self.slots(a[0])?;
                let n = slots.len().checked_div(8 * words).unwrap_or(0);
                let at = yan_rt::list::insert_at(a[1].int(), n) * 8 * words;
                slots.splice(at..at, vec![Byte::Init(0); 8 * words]);
                self.set_slots(a[0], slots)?;
                Val::I(i128::from(self.list(a[0])?.data) + at as i128)
            }
            RtFn::ListRemove => {
                let (words, mut slots) = self.slots(a[0])?;
                let at = a[1].int() as usize * 8 * words;
                slots.drain(at..at + 8 * words);
                self.set_slots(a[0], slots)?;
                Val::U
            }
            RtFn::ListSliceExact => {
                let (words, slots) = self.slots(a[0])?;
                let n = slots.len().checked_div(8 * words).unwrap_or(0) as i128;
                let (i, j) = (a[1].int(), a[2].int());
                if i < 0 || i > j || j > n {
                    return Err(Stop::Panic("bounds"));
                }
                let w = 8 * words as i128;
                self.new_list(words, slots[(i * w) as usize..(j * w) as usize].to_vec())
            }
            RtFn::DqLimit => {
                self.list(a[0])?.limit = a[1].int() as i64;
                Val::U
            }
            RtFn::DqMaxlen => Val::I(i128::from(self.list(a[0])?.limit)),
            RtFn::DqPush => {
                let (w, limit) = (self.list(a[0])?.words, self.list(a[0])?.limit);
                let mut data = self.list_words(a[0])?;
                let x = self.words(a[2].addr(), w)?;
                let evicted = yan_rt::dq::push(&mut data, w, limit, a[1].int() != 0, &x);
                self.set_slots(a[0], word_bytes(&data))?;
                match evicted {
                    Some(e) => {
                        self.write_words(a[3].addr(), &e)?;
                        Val::I(1)
                    }
                    None => Val::I(0),
                }
            }
            RtFn::DqPop => {
                let w = self.list(a[0])?.words;
                let mut data = self.list_words(a[0])?;
                match yan_rt::dq::pop(&mut data, w, a[1].int() != 0) {
                    Some(e) => {
                        self.set_slots(a[0], word_bytes(&data))?;
                        self.write_words(a[2].addr(), &e)?;
                        Val::I(1)
                    }
                    None => Val::I(0),
                }
            }
            RtFn::DqExtend => {
                let (w, limit) = (self.list(a[0])?.words, self.list(a[0])?.limit);
                let mut data = self.list_words(a[0])?;
                let src = self.list_words(a[1])?;
                let out = yan_rt::dq::extend(&mut data, w, limit, &src);
                self.set_slots(a[0], word_bytes(&data))?;
                let h = a[1].addr();
                if let Some(l) = self.lists.remove(&h) {
                    self.mem.free(l.data);
                }
                self.free_block(h)?;
                self.new_list(w, word_bytes(&out))
            }
            RtFn::DqRotate => {
                let w = self.list(a[0])?.words;
                let mut data = self.list_words(a[0])?;
                yan_rt::dq::rotate(&mut data, w, a[1].int() as i64);
                self.set_slots(a[0], word_bytes(&data))?;
                Val::U
            }
            RtFn::DqTake => {
                let (w, data) = (self.list(a[0])?.words, self.list_words(a[0])?);
                self.set_slots(a[0], Vec::new())?;
                self.new_list(w, word_bytes(&data))
            }
            RtFn::DqContains => {
                let w = self.list(a[0])?.words;
                let data = self.list_words(a[0])?;
                let x = self.words(a[2].addr(), w)?;
                let schema = self.str_of(a[1].addr())?;
                let layout = yan_rt::map::Layout::parse(&schema);
                let want = layout.encode(&x, &MemBlocks(self))?;
                let mut found = false;
                if w > 0 {
                    for e in data.chunks(w) {
                        found |= layout.encode(e, &MemBlocks(self))? == want;
                    }
                }
                Val::I(i128::from(found))
            }
            RtFn::DqSuffix => {
                let limit = self.list(a[0])?.limit;
                Val::I(i128::from(self.new_str(&yan_rt::dq::suffix(limit))))
            }
            RtFn::MapNew => {
                let schema = self.str_of(a[2].addr())?;
                let h = self.mem.alloc(8);
                let data = self.mem.alloc(0);
                let layout = yan_rt::map::Layout::parse(&schema);
                self.maps.insert(
                    h,
                    Map {
                        layout,
                        kw: a[0].int() as usize,
                        vw: a[1].int() as usize,
                        index: yan_rt::map::KeyIndex::default(),
                        data,
                    },
                );
                Val::I(i128::from(h))
            }
            RtFn::MapFind | RtFn::MapIndex => {
                let enc = self.encode_key(a[0], a[1])?;
                let m = self.map(a[0])?;
                match m.index.find(&enc) {
                    Some(i) => {
                        Val::I(i128::from(m.data) + (8 * (m.kw + m.vw) * i + 8 * m.kw) as i128)
                    }
                    None if f == RtFn::MapIndex => return Err(Stop::Panic("bounds")),
                    None => Val::I(0),
                }
            }
            RtFn::MapPush => {
                let enc = self.encode_key(a[0], a[1])?;
                let (kw, vw) = {
                    let m = self.map(a[0])?;
                    (m.kw, m.vw)
                };
                let key = self.mem.read(a[1].addr(), 8 * kw).unwrap_or_default();
                let mut slots = self.entries(a[0])?;
                slots.extend(key);
                slots.extend(vec![Byte::Init(0); 8 * vw]);
                let i = self.map(a[0])?.index.push(enc);
                let data = self.set_entries(a[0], slots)?;
                Val::I(i128::from(data) + (8 * (kw + vw) * i + 8 * kw) as i128)
            }
            RtFn::MapRemove => {
                let enc = self.encode_key(a[0], a[1])?;
                let Some(i) = self.map(a[0])?.index.remove(&enc) else {
                    return Ok(Val::I(0));
                };
                let m = self.map(a[0])?;
                let stride = 8 * (m.kw + m.vw);
                // The index has already shrunk by one.
                let (data, n) = (m.data, m.index.len() + 1);
                let mut slots = self.mem.read(data, stride * n).unwrap_or_default();
                let gone: Vec<Byte> = slots.drain(i * stride..(i + 1) * stride).collect();
                self.mem.write(a[2].addr(), &gone);
                self.set_entries(a[0], slots)?;
                Val::I(1)
            }
            RtFn::MapLen => Val::I(self.map(a[0])?.index.len() as i128),
            RtFn::MapAt => {
                let m = self.map(a[0])?;
                if !(0..m.index.len() as i128).contains(&a[1].int()) {
                    return Err(Stop::Fault("map entry out of range".into()));
                }
                Val::I(i128::from(m.data) + 8 * (m.kw + m.vw) as i128 * a[1].int())
            }
            RtFn::MapCopy => {
                let slots = self.entries(a[0])?;
                let m = self.map(a[0])?;
                let (layout, kw, vw, index) = (m.layout.clone(), m.kw, m.vw, m.index.clone());
                let h = self.mem.alloc(8);
                let data = self.mem.alloc(slots.len());
                self.mem.write(data, &slots);
                self.maps.insert(
                    h,
                    Map {
                        layout,
                        kw,
                        vw,
                        index,
                        data,
                    },
                );
                Val::I(i128::from(h))
            }
            RtFn::MapFree => {
                let h = a[0].addr();
                if let Some(m) = self.maps.remove(&h) {
                    self.mem.free(m.data);
                }
                self.free_block(h)?;
                Val::U
            }
            RtFn::TimeOp => {
                let r =
                    yan_rt::timeabi::time_op(a[0].int() as i64, time(a[1], a[2]), time(a[3], a[4]))
                        .map_err(Stop::Panic)?;
                self.scratch_words(&yan_rt::timeabi::time_words(r))
            }
            RtFn::TimeF64 => Val::F(
                yan_rt::timeabi::time_f64(a[0].int() as i64, time(a[1], a[2]), time(a[3], a[4]))
                    .map_err(Stop::Panic)?,
            ),
            RtFn::TimeShow => Val::I(i128::from(self.new_str(&yan_rt::timeabi::time_show(
                a[0].int() as i64,
                a[1].int() as i64,
                a[2].int() as i64,
            )))),
            RtFn::Civil => {
                let op = self.str_of(a[0].addr())?;
                if op == "cli.parse" {
                    return self.cli_parse(a[1], a[2], a[3]);
                }
                let words: Vec<i64> = a[1..].iter().map(|v| v.int() as i64).collect();
                let fault = std::cell::RefCell::new(None);
                let done = yan_rt::civil::run(&op, &words, &|w| {
                    self.str_of(w as u64).unwrap_or_else(|e| {
                        fault.borrow_mut().get_or_insert(e);
                        String::new()
                    })
                });
                if let Some(e) = fault.into_inner() {
                    return Err(e);
                }
                let (status, out) = yan_rt::civil::outcome(done, |s| self.new_str(s) as i64);
                self.civil_out = out;
                Val::I(i128::from(status))
            }
            RtFn::CivilOut => Val::I(i128::from(self.civil_out[a[0].int() as usize])),
            RtFn::CivilOutF => Val::F(f64::from_bits(self.civil_out[a[0].int() as usize] as u64)),
            RtFn::SimNew => {
                let plan = self.words(a[2].addr(), 8)?;
                let (data, len) = {
                    let cuts = self.list(Val::I(i128::from(plan[7])))?;
                    (cuts.data, cuts.len)
                };
                let cuts = self.words(data, 5 * len)?;
                let cuts = cuts
                    .chunks(5)
                    .map(|c| {
                        let m = self.map(Val::I(i128::from(c[4])))?;
                        let (data, n) = (m.data, m.index.len());
                        Ok(([c[0], c[1], c[2], c[3]], self.words(data, n)?))
                    })
                    .collect::<Result<Vec<_>, Stop>>()?;
                let plan: [u64; 7] = plan[..7].try_into().expect("7 slots");
                let sim = yan_rt::cluster::sim_new(
                    a[0].int() as i64,
                    a[1].int() as i64,
                    &plan,
                    &cuts,
                    yan_rt::civil::mono_now(),
                )
                .ok_or(Stop::Panic("contract"))?;
                Val::I(i128::from(yan_rt::cluster::register(sim)))
            }
            RtFn::SimBegin => Val::I(i128::from(yan_rt::cluster::yan_rt_sim_begin(
                a[0].addr(),
                a[1].addr(),
            ))),
            RtFn::ClusterNodes => {
                let ids: Vec<u64> = yan_rt::cluster::node_names()
                    .iter()
                    .map(|n| self.new_str(n))
                    .collect();
                self.new_list(1, word_bytes(&ids))
            }
            RtFn::ClusterSelfNode => {
                let n = yan_rt::cluster::self_node_name().ok_or_else(|| {
                    Stop::Unsupported("`cluster.self_node()` outside `with cluster`".into())
                })?;
                Val::I(i128::from(self.new_str(&n)))
            }
            RtFn::ClusterPlace => {
                let node = self.str_of(a[0].addr())?;
                if !yan_rt::cluster::place(&node) {
                    return Err(Stop::Panic("`On(node)` names a node outside this cluster"));
                }
                Val::U
            }
            RtFn::ClusterLeave => {
                yan_rt::cluster::yan_rt_cluster_leave();
                Val::U
            }
            RtFn::ClusterReach => {
                yan_rt::cluster::yan_rt_cluster_reach(a[0].addr());
                Val::U
            }
            RtFn::ClusterFaultSend => {
                yan_rt::cluster::yan_rt_cluster_fault_send(a[0].addr());
                Val::U
            }
            RtFn::ClusterSendMismatch => {
                yan_rt::cluster::yan_rt_cluster_send_mismatch(a[0].int() as u64);
                Val::U
            }
            RtFn::ClusterAdmit => {
                yan_rt::cluster::yan_rt_cluster_admit(a[0].int() as u64);
                Val::U
            }
            RtFn::ClusterArrive => Val::I(i128::from(yan_rt::cluster::yan_rt_cluster_arrive(
                a[0].int() as u64,
            ))),
            RtFn::ClusterReplied => Val::B(yan_rt::cluster::yan_rt_cluster_replied() != 0),
            RtFn::ClusterMonitor => {
                yan_rt::cluster::yan_rt_cluster_monitor(a[0].addr(), a[1].addr());
                Val::U
            }
            RtFn::ClusterGroup => {
                let name = self.str_of(a[0].addr())?;
                Val::I(i128::from(yan_rt::cluster::group(&name)))
            }
            RtFn::GroupJoin => {
                yan_rt::cluster::yan_rt_group_join(a[0].addr(), a[1].addr());
                Val::U
            }
            RtFn::GroupLeave => {
                yan_rt::cluster::yan_rt_group_leave(a[0].addr(), a[1].addr());
                Val::U
            }
            RtFn::GroupMembers => {
                let ms = yan_rt::cluster::group_members(a[0].addr());
                self.new_list(1, word_bytes(&ms))
            }
            RtFn::SimCheck => {
                let (trace, machines) = (self.list_words(a[1])?, self.list_words(a[2])?);
                let mut texts = Vec::new();
                let mut text = |m: &Self, w: u64| -> Result<usize, Stop> {
                    texts.push(m.str_of(w)?);
                    Ok(texts.len() - 1)
                };
                let steps = trace
                    .chunks(2)
                    .map(|s| Ok([text(self, s[0])?, text(self, s[1])?]))
                    .collect::<Result<Vec<_>, Stop>>()?;
                let mut ms = Vec::new();
                for m in machines.chunks(3) {
                    let edges = self.list_words(Val::I(i128::from(m[2])))?;
                    let es = edges
                        .chunks(3)
                        .map(|e| Ok([text(self, e[0])?, text(self, e[1])?, text(self, e[2])?]))
                        .collect::<Result<Vec<_>, Stop>>()?;
                    ms.push((text(self, m[0])?, text(self, m[1])?, es));
                }
                let t = |k: usize| texts[k].as_str();
                let trace: Vec<[&str; 2]> = steps.iter().map(|s| [t(s[0]), t(s[1])]).collect();
                let machines: Vec<yan_rt::cluster::TraceMachine> = ms
                    .iter()
                    .map(|(a, s, es)| {
                        (
                            t(*a),
                            t(*s),
                            es.iter().map(|e| [t(e[0]), t(e[1]), t(e[2])]).collect(),
                        )
                    })
                    .collect();
                let (tag, at, parts) = yan_rt::cluster::check_trace(&trace, &machines);
                let parts = parts.map(str::to_owned);
                let verdict = if tag == 0 {
                    [0; 5]
                } else {
                    let [actor, event, state] = parts.map(|p| self.new_str(&p));
                    [1, at, actor, event, state]
                };
                self.write_words(a[0].addr(), &verdict)?;
                Val::U
            }
            RtFn::WireRoundtrip => {
                let sc = yan_rt::wireslots::schemas(&self.str_of(a[2].addr())?);
                let w = self.words(a[1].addr(), yan_rt::wireslots::root_width(&sc))?;
                let (slots, why) = yan_rt::wireslots::roundtrip_in(&MemBlocks(self), &sc, &w)?;
                if why.is_some() && a[3].int() != 0 {
                    return Err(Stop::Panic("wire"));
                }
                let out = self.materialize(slots)?;
                self.write_words(a[0].addr(), &out)?;
                Val::I(i128::from(why.is_some()))
            }
            RtFn::JsonParse => {
                let (src, schema) = (self.str_of(a[1].addr())?, self.str_of(a[2].addr())?);
                let out = self.materialize(yan_rt::jsonslots::parse(&src, &schema))?;
                self.write_words(a[0].addr(), &out)?;
                Val::U
            }
            RtFn::JsonDump => {
                let schema = self.str_of(a[1].addr())?;
                let w = self.words(a[0].addr(), yan_rt::jsonslots::root_width(&schema))?;
                let Some(text) = yan_rt::jsonslots::dump_in(&MemBlocks(self), &w, &schema)? else {
                    return Err(Stop::Panic("value"));
                };
                Val::I(i128::from(self.new_str(&text)))
            }
            RtFn::JsonErrorShow => {
                let w = self.words(a[0].addr(), 14)?;
                let text = yan_rt::jsonslots::show_error_in(&MemBlocks(self), &w)?;
                Val::I(i128::from(self.new_str(&text)))
            }
            RtFn::RegexCheck => {
                let p = self.str_of(a[0].addr())?;
                Val::I(match yan_rt::regex::build(&p) {
                    Ok(_) => 0,
                    Err(reason) => i128::from(self.new_str(&reason)),
                })
            }
            RtFn::RegexEscape => {
                let s = self.str_of(a[0].addr())?;
                Val::I(i128::from(self.new_str(&yan_rt::regex::escape(&s))))
            }
            RtFn::RegexSub => {
                let (p, repl, s) = (
                    self.str_of(a[0].addr())?,
                    self.str_of(a[1].addr())?,
                    self.str_of(a[2].addr())?,
                );
                let r = yan_rt::regex::sub(
                    &yan_rt::regex::compiled(&p, yan_rt::regex::Anchor::None),
                    &repl,
                    &s,
                );
                Val::I(i128::from(self.new_str(&r)))
            }
            RtFn::RegexSplit => {
                let (p, s) = (self.str_of(a[0].addr())?, self.str_of(a[1].addr())?);
                let l = yan_rt::regexabi::split_slots(&p, &s);
                Val::I(i128::from(self.materialize(vec![l])?[0]))
            }
            RtFn::RegexFindAll => {
                let (p, s) = (self.str_of(a[0].addr())?, self.str_of(a[1].addr())?);
                let l = yan_rt::regexabi::find_all_slots(&p, &s);
                Val::I(i128::from(self.materialize(vec![l])?[0]))
            }
            RtFn::RegexFind => {
                let (p, s) = (self.str_of(a[0].addr())?, self.str_of(a[1].addr())?);
                match yan_rt::regexabi::find_slots(&p, &s, a[2].int() as i64) {
                    Some(slots) => {
                        let out = self.materialize(slots)?;
                        self.write_words(a[3].addr(), &out)?;
                        Val::I(1)
                    }
                    None => Val::I(0),
                }
            }
            RtFn::ZoneLookup => {
                let id = self.str_of(a[0].addr())?;
                Val::I(i128::from(yan_rt::envhandler::intern(&id).unwrap_or(0)))
            }
            RtFn::ZoneId => {
                let id = yan_rt::envhandler::zone_id(a[0].int() as i64);
                Val::I(i128::from(self.new_str(&id)))
            }
            RtFn::EnvFake => {
                let args = self.strs(a[0])?;
                let vars = self
                    .map_words(a[1])?
                    .chunks(2)
                    .map(|e| Ok((self.str_of(e[0])?, self.str_of(e[1])?)))
                    .collect::<Result<_, Stop>>()?;
                Val::I(i128::from(yan_rt::envhandler::fake(
                    args,
                    vars,
                    a[2].int() as i64,
                )))
            }
            RtFn::EnvOnly => {
                let vars = self.strs(a[1])?;
                Val::I(i128::from(yan_rt::envhandler::only(
                    a[0].int() as u64,
                    vars,
                )))
            }
            RtFn::EnvArgs => {
                let ws: Vec<u64> = yan_rt::envhandler::args(a[0].int() as u64)
                    .iter()
                    .map(|s| self.new_str(s))
                    .collect();
                self.new_list(1, word_bytes(&ws))
            }
            RtFn::EnvVar => {
                let name = self.str_of(a[1].addr())?;
                Val::I(match yan_rt::envhandler::var(a[0].int() as u64, &name) {
                    Some(v) => i128::from(self.new_str(&v)),
                    None => 0,
                })
            }
            RtFn::EnvVars => {
                let entries = yan_rt::envhandler::vars(a[0].int() as u64)
                    .into_iter()
                    .map(|(k, v)| (vec![Slot::Str(k)], vec![Slot::Str(v)]))
                    .collect();
                let m = Slot::Map {
                    schema: "s".into(),
                    kw: 1,
                    vw: 1,
                    entries,
                };
                Val::I(i128::from(self.materialize(vec![m])?[0]))
            }
            RtFn::EnvZone => Val::I(i128::from(yan_rt::envhandler::zone(a[0].int() as u64))),
            RtFn::ActorSpawn => {
                let state = self.words(a[0].addr(), a[1].int() as usize)?;
                let start = [a[5].addr(), a[6].addr(), a[7].addr()];
                let id = yan_rt::actor::spawn(
                    &state,
                    a[2].int() as i64,
                    a[3].addr(),
                    a[4].addr(),
                    start,
                )
                .map_err(Stop::Panic)?;
                Val::I(i128::from(id))
            }
            RtFn::ActorTake => {
                let (status, state) = yan_rt::actor::take(a[0].addr(), a[2].truth());
                self.write_words(a[1].addr(), &state)?;
                Val::I(i128::from(status))
            }
            RtFn::ActorRetake => {
                let (status, state) = yan_rt::actor::retake(a[0].addr(), a[1].addr(), a[3].truth());
                self.write_words(a[2].addr(), &state)?;
                Val::I(i128::from(status))
            }
            RtFn::ActorPut => {
                let state = self.state_words(a[0].addr(), a[1].addr())?;
                yan_rt::actor::put(a[0].addr(), &state);
                Val::U
            }
            RtFn::ActorLend => {
                let state = self.state_words(a[0].addr(), a[1].addr())?;
                Val::I(i128::from(yan_rt::actor::lend(a[0].addr(), &state)))
            }
            RtFn::ActorGive => {
                let state = self.state_words(a[0].addr(), a[1].addr())?;
                Val::I(i128::from(yan_rt::actor::give(a[0].addr(), &state)))
            }
            RtFn::ActorRestarted => {
                let state = self.state_words(a[0].addr(), a[1].addr())?;
                yan_rt::actor::restarted(a[0].addr(), &state);
                Val::U
            }
            RtFn::ActorSend => Val::I(i128::from(yan_rt::actor::yan_rt_actor_send(
                a[0].addr(),
                a[1].addr(),
                a[2].addr(),
                a[3].addr(),
                a[4].addr(),
                a[5].addr(),
            ))),
            RtFn::ActorArg => Val::I(i128::from(yan_rt::actor::yan_rt_actor_arg(a[0].addr()))),
            RtFn::ActorRan => {
                yan_rt::actor::yan_rt_actor_ran();
                Val::U
            }
            RtFn::ActorIdle => Val::B(yan_rt::actor::yan_rt_actor_idle() != 0),
            RtFn::ActorRestartNext => Val::B(yan_rt::actor::yan_rt_actor_restart_next() != 0),
            RtFn::ActorCrash => {
                let error = self.str_of(a[1].addr())?;
                yan_rt::actor::crashed(a[0].addr(), error)
                    .map_err(|_| Stop::Panic("supervisor"))?;
                Val::U
            }
            RtFn::ActorReason => {
                Val::I(i128::from(yan_rt::actor::yan_rt_actor_reason(a[0].addr())))
            }
            RtFn::ActorReasonText => Val::I(i128::from(
                self.new_str(&yan_rt::actor::reason_text(a[0].addr())),
            )),
            RtFn::SupBegin => Val::I(i128::from(yan_rt::actor::yan_rt_sup_begin(
                a[0].addr(),
                a[1].addr(),
                a[2].int() as i64,
                a[3].int() as i64,
            ))),
            RtFn::SupEnd => {
                yan_rt::actor::yan_rt_sup_end();
                Val::U
            }
            RtFn::SupEndTo => {
                yan_rt::actor::yan_rt_sup_end_to(a[0].addr());
                Val::U
            }
            RtFn::ParBegin if self.ub => {
                return unsupported("race checks across `par concurrent` tasks in the machine");
            }
            RtFn::ParBegin => Val::I(i128::from(sched::yan_rt_par_begin(
                a[0].addr(),
                a[1].addr(),
            ))),
            RtFn::ParNext => Val::I(i128::from(sched::yan_rt_par_next(a[0].addr()))),
            RtFn::ParTaskEnd => {
                sched::yan_rt_par_task_end(a[0].addr());
                Val::U
            }
            RtFn::ParExitKind => {
                sched::yan_rt_par_exit_kind(a[0].addr());
                Val::U
            }
            RtFn::ParCancel => Val::I(i128::from(sched::yan_rt_par_cancel(a[0].addr()))),
            RtFn::ParEnd => {
                sched::yan_rt_par_end(a[0].addr());
                Val::U
            }
            RtFn::SimPct => {
                sched::yan_rt_sim_pct(a[0].int() as i64, a[1].int() as i64, a[2].int() as i64);
                Val::U
            }
            RtFn::ChanNew => Val::I(i128::from(yan_rt::chan::yan_rt_chan_new(
                a[0].int() as u64,
                a[1].int() as u64,
            ) as u64)),
            RtFn::ChanSend | RtFn::ChanTrySend => {
                let ch = a[0].addr();
                let v = self.words(a[1].addr(), yan_rt::chan::words(ch as usize))?;
                let in_task = a[2].int() != 0;
                Val::I(i128::from(if f == RtFn::ChanSend {
                    yan_rt::chan::send(ch as usize, &v, in_task)
                } else {
                    yan_rt::chan::try_send(ch as usize, &v, in_task)
                }))
            }
            RtFn::ChanRecv | RtFn::ChanTryRecv => {
                let ch = a[0].addr() as usize;
                let in_task = a[2].int() != 0;
                let (status, v) = if f == RtFn::ChanRecv {
                    yan_rt::chan::recv(ch, in_task)
                } else {
                    yan_rt::chan::try_recv(ch, in_task)
                };
                self.write_words(a[1].addr(), &v)?;
                Val::I(i128::from(status))
            }
            RtFn::SelectWait => {
                let (keys, at) = (self.list_words(a[0])?, self.list_words(a[1])?);
                let (status, arm) = yan_rt::chan::select_wait(&keys, &at, a[3].int() != 0);
                if status == 0 {
                    self.write_words(a[2].addr(), &[arm])?;
                }
                Val::I(i128::from(status))
            }
            RtFn::ChanDrop | RtFn::ChanDropSender | RtFn::ChanClone | RtFn::ChanCloneSender => {
                use yan_rt::chan::End;
                let op = match f {
                    RtFn::ChanDrop => End::Drop,
                    RtFn::ChanDropSender => End::DropSender,
                    RtFn::ChanClone => End::Clone,
                    _ => End::CloneSender,
                };
                yan_rt::chan::end(a[0].addr() as usize, op);
                Val::U
            }
            other => return unsupported(format!("runtime function {other:?} in the machine")),
        })
    }
}

impl yan_rt::wireslots::Heap for MemBlocks<'_> {
    type Err = Stop;

    fn text(&self, block: u64) -> Result<String, Stop> {
        self.0.str_of(block)
    }

    fn list(&self, block: u64) -> Result<Vec<u64>, Stop> {
        let l = self
            .0
            .lists
            .get(&block)
            .ok_or_else(|| Stop::Fault(format!("{block:#x} is not a List block")))?;
        self.0.words(l.data, l.words * l.len)
    }

    fn map(&self, block: u64) -> Result<Vec<u64>, Stop> {
        let m = self
            .0
            .maps
            .get(&block)
            .ok_or_else(|| Stop::Fault(format!("{block:#x} is not a Map block")))?;
        self.0.words(m.data, (m.kw + m.vw) * m.index.len())
    }
}

impl yan_rt::map::Blocks for MemBlocks<'_> {
    type Err = Stop;

    fn text(&self, block: u64) -> Result<std::borrow::Cow<'_, str>, Stop> {
        self.0.str_of(block).map(Into::into)
    }

    fn bytes(&self, block: u64) -> Result<std::borrow::Cow<'_, [u8]>, Stop> {
        self.0
            .bytes_ref(Val::I(i128::from(block)))
            .map(|b| b.as_slice().into())
    }
}

/// `cli.parse`'s result blocks on the machine heap.
struct MachineBlocks<'a, 'b> {
    m: &'a mut Machine<'b>,
    fault: Option<Stop>,
}

impl yan_rt::civil::Blocks for MachineBlocks<'_, '_> {
    fn str(&mut self, s: &str) -> i64 {
        self.m.new_str(s) as i64
    }
    fn path(&mut self, s: &str) -> i64 {
        self.m.new_str(&yan_rt::path::Parsed::new(false, s).text()) as i64
    }
    fn list(&mut self, words: usize, elems: &[Vec<i64>]) -> i64 {
        let h = self.m.list_new(words);
        for slots in elems {
            let at = self.m.list_push(Val::I(i128::from(h))).expect("fresh list");
            let w: Vec<u64> = slots.iter().map(|&x| x as u64).collect();
            if let Err(e) = self.m.write_words(at, &w) {
                self.fault.get_or_insert(e);
            }
        }
        h as i64
    }
}
