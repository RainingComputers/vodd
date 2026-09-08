const OFFSET_BITS: u32 = 44;
const SLOT_BITS: u32 = 16;
const SLOT_SHIFT: u32 = OFFSET_BITS;
const REGION_SHIFT: u32 = OFFSET_BITS + SLOT_BITS;
const OFFSET_MASK: u64 = (1 << OFFSET_BITS) - 1;
const SLOT_MASK: u64 = (1 << SLOT_BITS) - 1;
const MAX_SLOTS: usize = (1 << SLOT_BITS) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Global,
    Local,
    Mutable,
    Invocation,
    Builtin,
    Constant,
}

impl Region {
    fn bits(self) -> u64 {
        match self {
            Region::Global => 1,
            Region::Local => 2,
            Region::Mutable => 3,
            Region::Invocation => 4,
            Region::Builtin => 5,
            Region::Constant => 6,
        }
    }

    fn from_bits(bits: u64) -> Option<Region> {
        Some(match bits {
            1 => Region::Global,
            2 => Region::Local,
            3 => Region::Mutable,
            4 => Region::Invocation,
            5 => Region::Builtin,
            6 => Region::Constant,
            _ => return None,
        })
    }
}

pub fn encode(region: Option<Region>, slot: usize, offset: u64) -> u64 {
    (region.map_or(0, Region::bits) << REGION_SHIFT)
        | ((slot as u64 & SLOT_MASK) << SLOT_SHIFT)
        | (offset & OFFSET_MASK)
}

pub fn decode(address: u64) -> (Option<Region>, usize, u64) {
    let region = Region::from_bits(address >> REGION_SHIFT);
    let slot = ((address >> SLOT_SHIFT) & SLOT_MASK) as usize;

    (region, slot, address & OFFSET_MASK)
}

pub fn region(address: u64) -> Option<Region> {
    decode(address).0
}

fn offset(address: u64) -> u64 {
    decode(address).2
}

fn base(region: Option<Region>, slot: usize) -> u64 {
    encode(region, slot, 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    Null,
    Unmapped,
    Overrun,
}

#[derive(Debug, Clone)]
struct Entry<P, T> {
    size: u64,
    ptr: P,
    metadata: T,
}

#[derive(Debug, Clone)]
struct Segments<P, T> {
    region: Region,
    entries: Vec<Entry<P, T>>,
}

impl<P, T> Segments<P, T> {
    fn new(region: Region) -> Segments<P, T> {
        Segments { region, entries: Vec::new() }
    }

    fn push(&mut self, size: u64, ptr: P, metadata: T) -> Option<u64> {
        if self.entries.len() >= MAX_SLOTS {
            return None;
        }

        self.entries.push(Entry { size, ptr, metadata });

        Some(base(Some(self.region), self.entries.len()))
    }

    fn get(&self, address: u64, size: usize) -> Result<&Entry<P, T>, Invalid> {
        let (region, slot, offset) = decode(address);

        match region {
            None if slot == 0 => return Err(Invalid::Null),
            Some(region) if region == self.region && slot != 0 => {}
            _ => return Err(Invalid::Unmapped),
        }

        let entry = self.entries.get(slot - 1).ok_or(Invalid::Unmapped)?;

        if offset
            .checked_add(size as u64)
            .is_none_or(|end| end > entry.size)
        {
            return Err(Invalid::Overrun);
        }

        Ok(entry)
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn truncate(&mut self, slot: usize) {
        self.entries.truncate(slot);
    }
}

#[derive(Debug, Clone)]
pub struct Storage {
    segments: Segments<usize, ()>,
    bytes: Vec<u8>,
}

impl Storage {
    pub fn new(region: Region) -> Storage {
        Storage { segments: Segments::new(region), bytes: Vec::new() }
    }

    pub fn allocate(&mut self, size: usize, alignment: usize) -> Option<u64> {
        let ptr = match alignment {
            0 => self.bytes.len(),
            alignment => self.bytes.len().next_multiple_of(alignment),
        };

        let address = self.segments.push(size as u64, ptr, ())?;

        self.bytes.resize(ptr + size, 0);

        Some(address)
    }

    fn span(&self, address: u64, size: usize) -> Result<std::ops::Range<usize>, Invalid> {
        let ptr = self.segments.get(address, size)?.ptr;
        let within = ptr + offset(address) as usize;

        Ok(within..within + size)
    }

    pub fn read(&self, address: u64, size: usize) -> Result<&[u8], Invalid> {
        Ok(&self.bytes[self.span(address, size)?])
    }

    pub fn write(&mut self, address: u64, size: usize) -> Result<&mut [u8], Invalid> {
        let span = self.span(address, size)?;

        Ok(&mut self.bytes[span])
    }

    pub fn zero(&mut self) {
        self.bytes.fill(0);
    }

    pub fn capacity(&self) -> usize {
        self.bytes.len()
    }

    pub fn watermark(&self) -> usize {
        self.segments.len()
    }

    pub fn truncate(&mut self, watermark: usize) {
        if let Some(entry) = self.segments.entries.get(watermark) {
            self.bytes.truncate(entry.ptr);
        }

        self.segments.truncate(watermark);
    }
}

#[derive(Debug, Clone)]
pub struct UnsafeSharedRawPtrStorage<T> {
    segments: Segments<*mut u8, T>,
}

unsafe impl<T: Send> Send for UnsafeSharedRawPtrStorage<T> {}
unsafe impl<T: Sync> Sync for UnsafeSharedRawPtrStorage<T> {}

impl<T> UnsafeSharedRawPtrStorage<T> {
    pub fn new(region: Region) -> UnsafeSharedRawPtrStorage<T> {
        UnsafeSharedRawPtrStorage { segments: Segments::new(region) }
    }

    /// # Safety
    ///
    /// `base` must point to at least `size` valid bytes that stay allocated, and are not
    /// otherwise aliased, for as long as this storage lives.
    pub unsafe fn borrow(&mut self, base: *mut u8, size: u64, metadata: T) -> Option<u64> {
        self.segments.push(size, base, metadata)
    }

    pub fn read_with_metadata<R>(
        &self,
        address: u64,
        size: usize,
        read: impl FnOnce(&T, u64, &[u8]) -> R,
    ) -> Result<R, Invalid> {
        let entry = self.segments.get(address, size)?;
        let within = offset(address);
        let bytes = unsafe { core::slice::from_raw_parts(entry.ptr.add(within as usize), size) };

        Ok(read(&entry.metadata, within, bytes))
    }

    pub fn write_with_metadata<R>(
        &mut self,
        address: u64,
        size: usize,
        write: impl FnOnce(&T, u64, &mut [u8]) -> R,
    ) -> Result<R, Invalid> {
        let entry = self.segments.get(address, size)?;
        let within = offset(address);
        let destination =
            unsafe { core::slice::from_raw_parts_mut(entry.ptr.add(within as usize), size) };

        Ok(write(&entry.metadata, within, destination))
    }
}
