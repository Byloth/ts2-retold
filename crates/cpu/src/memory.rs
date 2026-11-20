//! Sparse 32-bit guest address space.

const PAGE_BITS: u32 = 12;
pub const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_COUNT: usize = 1 << (32 - PAGE_BITS);

type Page = Box<[u8; PAGE_SIZE]>;

/// What happens when the guest touches a page that was never mapped.
#[derive(Clone, Copy)]
pub enum Unmapped {
    /// The access fails.
    Fault,
    /// The page appears, filled by this function of its page number. Used by
    /// differential tests, where any address must be readable and both sides
    /// must see the same bytes.
    Fill(fn(u32, &mut [u8; PAGE_SIZE])),
}

pub struct Memory {
    pages: Vec<Option<Page>>,
    /// Numbers of the pages that exist, in creation order.
    mapped: Vec<u32>,
    unmapped: Unmapped,
    /// When set, every guest write is appended as `(address, length)`.
    pub write_log: Option<Vec<(u32, u8)>>,
}

impl Memory {
    pub fn new(unmapped: Unmapped) -> Memory {
        Memory {
            pages: (0..PAGE_COUNT).map(|_| None).collect(),
            mapped: Vec::new(),
            unmapped,
            write_log: None,
        }
    }

    /// Maps zero-filled pages covering `start..start + len`.
    pub fn map(&mut self, start: u32, len: u32) {
        if len == 0 {
            return;
        }
        let first = start >> PAGE_BITS;
        let last = (start as u64 + len as u64 - 1) as u32 >> PAGE_BITS;
        for page in first..=last {
            if self.pages[page as usize].is_none() {
                self.pages[page as usize] = Some(Box::new([0; PAGE_SIZE]));
                self.mapped.push(page);
            }
        }
    }

    /// Unmaps everything.
    pub fn clear(&mut self) {
        for page in self.mapped.drain(..) {
            self.pages[page as usize] = None;
        }
    }

    pub fn is_mapped(&self, address: u32) -> bool {
        self.pages[(address >> PAGE_BITS) as usize].is_some()
    }

    #[inline]
    fn page(&mut self, number: u32) -> Option<&mut [u8; PAGE_SIZE]> {
        let slot = &mut self.pages[number as usize];
        if slot.is_none() {
            let Unmapped::Fill(fill) = self.unmapped else {
                return None;
            };
            let mut page = Box::new([0; PAGE_SIZE]);
            fill(number, &mut page);
            *slot = Some(page);
            self.mapped.push(number);
        }
        slot.as_deref_mut()
    }

    #[inline]
    pub fn read_u8(&mut self, address: u32) -> Option<u8> {
        let page = self.page(address >> PAGE_BITS)?;
        Some(page[address as usize & (PAGE_SIZE - 1)])
    }

    #[inline]
    fn write_u8_raw(&mut self, address: u32, value: u8) -> Option<()> {
        let page = self.page(address >> PAGE_BITS)?;
        page[address as usize & (PAGE_SIZE - 1)] = value;
        Some(())
    }

    /// Reads `len` (1, 2 or 4) bytes as a zero-extended little-endian value.
    #[inline]
    pub fn read(&mut self, address: u32, len: u32) -> Option<u32> {
        let offset = address as usize & (PAGE_SIZE - 1);
        if offset + len as usize <= PAGE_SIZE {
            let page = self.page(address >> PAGE_BITS)?;
            let mut bytes = [0u8; 4];
            bytes[..len as usize].copy_from_slice(&page[offset..offset + len as usize]);
            Some(u32::from_le_bytes(bytes))
        } else {
            let mut value = 0;
            for i in 0..len {
                value |= (self.read_u8(address.wrapping_add(i))? as u32) << (8 * i);
            }
            Some(value)
        }
    }

    /// Writes the low `len` (1, 2 or 4) bytes of `value`.
    #[inline]
    pub fn write(&mut self, address: u32, len: u32, value: u32) -> Option<()> {
        // Check both ends first so that a faulting write changes nothing.
        self.page(address >> PAGE_BITS)?;
        self.page(address.wrapping_add(len - 1) >> PAGE_BITS)?;
        for i in 0..len {
            self.write_u8_raw(address.wrapping_add(i), (value >> (8 * i)) as u8)?;
        }
        if let Some(log) = &mut self.write_log {
            log.push((address, len as u8));
        }
        Some(())
    }

    pub fn read_u64(&mut self, address: u32) -> Option<u64> {
        let low = self.read(address, 4)? as u64;
        let high = self.read(address.wrapping_add(4), 4)? as u64;
        Some(low | high << 32)
    }

    pub fn write_u64(&mut self, address: u32, value: u64) -> Option<()> {
        self.page(address >> PAGE_BITS)?;
        self.page(address.wrapping_add(7) >> PAGE_BITS)?;
        self.write(address, 4, value as u32)?;
        self.write(address.wrapping_add(4), 4, (value >> 32) as u32)
    }

    /// Copies host bytes into guest memory without logging. Fails if any
    /// byte is unmapped; bytes before the failure may have been written.
    pub fn load(&mut self, address: u32, bytes: &[u8]) -> Option<()> {
        let mut done = 0;
        while done < bytes.len() {
            // One page at a time: whole runs of bytes are copied at once.
            let at = address.wrapping_add(done as u32);
            let offset = at as usize & (PAGE_SIZE - 1);
            let run = (PAGE_SIZE - offset).min(bytes.len() - done);
            let page = self.page(at >> PAGE_BITS)?;
            page[offset..offset + run].copy_from_slice(&bytes[done..done + run]);
            done += run;
        }
        Some(())
    }

    /// Copies guest bytes into `out`. Fails if any byte is unmapped.
    pub fn read_bytes(&mut self, address: u32, out: &mut [u8]) -> Option<()> {
        let mut done = 0;
        while done < out.len() {
            let at = address.wrapping_add(done as u32);
            let offset = at as usize & (PAGE_SIZE - 1);
            let run = (PAGE_SIZE - offset).min(out.len() - done);
            let page = self.page(at >> PAGE_BITS)?;
            out[done..done + run].copy_from_slice(&page[offset..offset + run]);
            done += run;
        }
        Some(())
    }

    /// Copies guest bytes out. Fails if any byte is unmapped.
    pub fn dump(&mut self, address: u32, len: u32) -> Option<Vec<u8>> {
        let mut bytes = vec![0; len as usize];
        self.read_bytes(address, &mut bytes)?;
        Some(bytes)
    }

    /// Reads a NUL-terminated string of at most `limit` bytes.
    pub fn c_string(&mut self, address: u32, limit: u32) -> Option<String> {
        let mut bytes = Vec::new();
        for i in 0..limit {
            match self.read_u8(address.wrapping_add(i))? {
                0 => break,
                byte => bytes.push(byte),
            }
        }
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faults_when_unmapped_and_reads_across_pages() {
        let mut memory = Memory::new(Unmapped::Fault);
        assert_eq!(memory.read(0x1000, 4), None);
        memory.map(0x1000, 0x2000);
        assert_eq!(memory.write(0x1FFE, 4, 0xAABB_CCDD), Some(()));
        assert_eq!(memory.read(0x1FFE, 4), Some(0xAABB_CCDD));
        assert_eq!(memory.read(0x2000, 2), Some(0xAABB));
        // A write straddling the end of the mapping changes nothing.
        assert_eq!(memory.write(0x2FFE, 4, 0x1122_3344), None);
        assert_eq!(memory.read(0x2FFE, 2), Some(0));
    }

    #[test]
    fn fill_policy_maps_on_demand_and_logs_writes() {
        fn fill(page: u32, bytes: &mut [u8; PAGE_SIZE]) {
            bytes.fill(page as u8);
        }
        let mut memory = Memory::new(Unmapped::Fill(fill));
        memory.write_log = Some(Vec::new());
        assert_eq!(memory.read(0x0001_2FFF, 2), Some(0x1312));
        memory.write(0x5000, 2, 0xBEEF).unwrap();
        memory.write_u64(0x6000, 1).unwrap();
        assert_eq!(memory.write_log.as_deref(), Some(&[(0x5000, 2), (0x6000, 4), (0x6004, 4)][..]));
    }

    #[test]
    fn bulk_copies_cross_page_boundaries_and_stop_at_unmapped_memory() {
        let mut memory = Memory::new(Unmapped::Fault);
        memory.map(0x1000, 0x3000);
        let data: Vec<u8> = (0..0x1F00u32).map(|i| (i * 7) as u8).collect();
        assert_eq!(memory.load(0x1F80, &data), Some(()));
        assert_eq!(memory.dump(0x1F80, data.len() as u32).as_deref(), Some(&data[..]));
        assert_eq!(memory.read_u8(0x2000), Some(data[0x80]));
        let mut out = [0u8; 4];
        assert_eq!(memory.read_bytes(0x3FFE, &mut out), None);
        assert_eq!(memory.load(0x3FFE, &[1, 2, 3, 4]), None);
        assert_eq!(memory.dump(0x1000, 0), Some(Vec::new()));
    }
}
