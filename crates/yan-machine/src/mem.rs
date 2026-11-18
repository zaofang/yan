//! 6.1 memory: allocations of bytes that are `Uninit` or `Init`, at distinct addresses.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Byte {
    Uninit,
    Init(u8),
    /// The value moved out (RFC 0087); a store makes the byte `Init` again.
    Moved,
}

pub struct Alloc {
    pub base: u64,
    pub size: usize,
    pub bytes: Vec<Byte>,
    pub live: bool,
    pub immutable: bool,
}

pub type AllocId = usize;

/// Allocation bases are 16-aligned and a gap apart, so an address one past an allocation's end
/// never falls inside another allocation.
#[derive(Default)]
pub struct Memory {
    allocs: Vec<Alloc>,
    next: u64,
}

const GAP: u64 = 16;

impl Memory {
    pub fn new() -> Memory {
        Memory {
            allocs: Vec::new(),
            next: 0x1_0000,
        }
    }

    pub fn alloc(&mut self, size: usize) -> u64 {
        let base = self.next;
        self.next = (base + size as u64 + GAP).next_multiple_of(16);
        self.allocs.push(Alloc {
            base,
            size,
            bytes: vec![Byte::Uninit; size],
            live: true,
            immutable: false,
        });
        base
    }

    pub fn find(&self, addr: u64) -> Option<AllocId> {
        let i = self
            .allocs
            .partition_point(|a| a.base <= addr)
            .checked_sub(1)?;
        let a = &self.allocs[i];
        (addr - a.base <= a.size as u64).then_some(i)
    }

    /// `false` when `addr` is in an allocation that is already freed.
    pub fn free(&mut self, addr: u64) -> bool {
        let Some(i) = self.find(addr) else {
            return true;
        };
        let a = &mut self.allocs[i];
        let was_live = a.live;
        a.live = false;
        a.bytes = Vec::new();
        was_live
    }

    /// Live allocations as `(base, size)`, excluding `except`.
    pub fn live(&self, except: impl Fn(u64) -> bool) -> Vec<(u64, usize)> {
        self.allocs
            .iter()
            .filter(|a| a.live && !except(a.base))
            .map(|a| (a.base, a.size))
            .collect()
    }

    /// Marks the allocation at `addr` immutable: later writes are `ub(7)` under `--ub`.
    pub fn freeze(&mut self, addr: u64) {
        if let Some(i) = self.find(addr) {
            self.allocs[i].immutable = true;
        }
    }

    pub fn is_immutable(&self, addr: u64) -> bool {
        self.find(addr).is_some_and(|i| self.allocs[i].immutable)
    }

    pub fn base(&self, id: AllocId) -> u64 {
        self.allocs[id].base
    }

    pub fn size(&self, id: AllocId) -> usize {
        self.allocs[id].size
    }

    /// Every initialised aligned word of every live allocation.
    pub fn words(&self) -> impl Iterator<Item = u64> + '_ {
        self.allocs.iter().flat_map(|a| {
            a.bytes.chunks_exact(8).filter_map(|c| {
                let mut w = [0u8; 8];
                for (d, b) in w.iter_mut().zip(c) {
                    match b {
                        Byte::Init(x) => *d = *x,
                        _ => return None,
                    }
                }
                Some(u64::from_le_bytes(w))
            })
        })
    }

    pub fn is_live(&self, id: AllocId) -> bool {
        self.allocs[id].live
    }

    pub fn len(&self, addr: u64) -> Option<usize> {
        self.find(addr).map(|i| self.allocs[i].size)
    }

    /// `n` bytes at `addr`; `None` when they are not inside one allocation.
    pub fn read(&self, addr: u64, n: usize) -> Option<Vec<Byte>> {
        let a = &self.allocs[self.find(addr)?];
        let at = (addr - a.base) as usize;
        a.bytes.get(at..at + n).map(<[Byte]>::to_vec)
    }

    /// Marks `n` bytes at `addr` moved-from; `None` when they are not inside one live allocation.
    pub fn mark_moved(&mut self, addr: u64, n: usize) -> Option<()> {
        let i = self.find(addr)?;
        let a = &mut self.allocs[i];
        let at = (addr - a.base) as usize;
        a.bytes.get_mut(at..at + n)?.fill(Byte::Moved);
        Some(())
    }

    pub fn write(&mut self, addr: u64, bytes: &[Byte]) -> Option<()> {
        let i = self.find(addr)?;
        let a = &mut self.allocs[i];
        let at = (addr - a.base) as usize;
        a.bytes
            .get_mut(at..at + bytes.len())?
            .copy_from_slice(bytes);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_lists_unfreed() {
        let mut m = Memory::new();
        let (a, b) = (m.alloc(8), m.alloc(3));
        m.free(a);
        assert_eq!(m.live(|_| false), vec![(b, 3)]);
        assert!(m.live(|x| x == b).is_empty());
    }
}
