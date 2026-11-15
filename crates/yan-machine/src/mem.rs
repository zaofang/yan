//! 6.1 memory: allocations of bytes that are `Uninit` or `Init`, at distinct addresses.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Byte {
    Uninit,
    Init(u8),
}

pub struct Alloc {
    pub base: u64,
    pub size: usize,
    pub bytes: Vec<Byte>,
    pub live: bool,
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
