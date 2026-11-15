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

impl Machine<'_> {
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
            other => return unsupported(format!("runtime function {other:?} in the machine")),
        })
    }
}
