//! The yan-rt entry points the machine models, over abstract memory. A `Str` block is one
//! allocation of its UTF-8 bytes. A `List` block is a handle allocation; its elements are
//! `words` 8-byte slots each in a data allocation that is replaced when the list grows.

use crate::mem::Byte;
use crate::{Machine, Stop, Val, unsupported};
use std::io::Write;
use yan_mir::RtFn;

#[derive(Clone, Copy)]
pub struct List {
    words: usize,
    len: usize,
    cap: usize,
    data: u64,
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
        let old = self.list(l)?.data;
        let len = slots.len().checked_div(8 * words).unwrap_or(0);
        let data = self.mem.alloc(slots.len());
        self.mem.write(data, &slots);
        self.mem.free(old);
        *self.list(l)? = List {
            words,
            len,
            cap: len,
            data,
        };
        Ok(())
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
            },
        );
        Val::I(i128::from(h))
    }

    /// Words handed back by pointer, as native does with a thread-local. The block is reused and
    /// is not a leak.
    fn scratch_words(&mut self, words: &[i64]) -> Val {
        let at = *self.scratch.get_or_insert_with(|| self.mem.alloc(8 * 3));
        let bytes: Vec<Byte> = words
            .iter()
            .flat_map(|w| w.to_le_bytes().map(Byte::Init))
            .collect();
        self.mem.write(at, &bytes).expect("scratch block is live");
        Val::I(i128::from(at))
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

    fn element(&mut self, l: Val, i: i128) -> Result<Val, Stop> {
        let list = self.list(l)?;
        if !(0..list.len as i128).contains(&i) {
            return Err(Stop::Panic("bounds"));
        }
        Ok(Val::I(i128::from(list.data) + i * 8 * list.words as i128))
    }

    pub(crate) fn rt(&mut self, f: RtFn, a: &[Val]) -> Result<Val, Stop> {
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
            RtFn::ListNew => {
                let h = self.mem.alloc(8);
                let data = self.mem.alloc(0);
                self.lists.insert(
                    h,
                    List {
                        words: a[0].int() as usize,
                        len: 0,
                        cap: 0,
                        data,
                    },
                );
                Val::I(i128::from(h))
            }
            RtFn::ListPush => {
                let h = a[0].addr();
                let List {
                    words,
                    len,
                    cap,
                    data,
                } = *self.list(a[0])?;
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
                };
                Val::I(i128::from(at))
            }
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
                    return unsupported("civil operation `cli.parse` in the machine");
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
            other => return unsupported(format!("runtime function {other:?} in the machine")),
        })
    }
}
