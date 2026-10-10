//! RetroAchievements for Slot, over rcheevos.
//!
//! This crate owns the boundary with the C library: everything unsafe stays
//! here, and the rest of Slot sees ordinary Rust.
//!
//! An achievement does not read hardware addresses. It reads a flat space
//! RetroAchievements defines per console, and for the GBA that is the three
//! RAM regions laid end to end, in an order that is nothing like the hardware
//! map. Translating between the two means folding a `select` mask, pinning
//! `disconnect` bits and clearing high bits in a loop, and getting it wrong
//! produces an achievement that silently never triggers rather than an error.
//! rcheevos already does exactly what the server expects, so this crate hands
//! the problem to it rather than writing it again.

mod ffi;
mod http;

pub use ffi::DescriptorAbi;
pub use http::{Curl, Plan, Request, Response, ServerCall, CLIENT_ERROR, RETRYABLE_CLIENT_ERROR};

/// The console whose flat address space is wanted. rcheevos keeps a table per
/// console; the id is part of its API, not something we choose.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Console {
    GameBoyAdvance,
}

impl Console {
    fn id(self) -> u32 {
        match self {
            // RC_CONSOLE_GAMEBOY_ADVANCE
            Console::GameBoyAdvance => 5,
        }
    }
}

/// One region of the core's address space together with the live pointer into
/// the core's own memory.
///
/// slot-retro's `MemoryRegion` deliberately carries no pointer, because a
/// pointer into a core is only good on the thread that owns the core. This is
/// the same description with the pointer added back, for use where that
/// already holds.
#[derive(Copy, Clone, Debug)]
pub struct MappedRegion {
    pub flags: u64,
    pub ptr: *mut u8,
    pub offset: usize,
    pub start: usize,
    pub select: usize,
    pub disconnect: usize,
    pub len: usize,
}

/// Hand a region to rcheevos exactly as the core described it.
///
/// Every field is load-bearing somewhere. `select` and `disconnect` decide
/// mirrored addresses, and while the GBA happens never to need them (rcheevos
/// only ever asks it for three exact addresses, which match with or without a
/// mask), dropping them here would be a silent lie to the library for any map
/// that does.
fn descriptor(r: &MappedRegion) -> ffi::MemoryDescriptor {
    ffi::MemoryDescriptor {
        flags: r.flags,
        ptr: r.ptr.cast(),
        offset: r.offset,
        start: r.start,
        select: r.select,
        disconnect: r.disconnect,
        len: r.len,
        addrspace: std::ptr::null(),
    }
}

/// The flat address space achievements are evaluated against.
pub struct Memory {
    regions: Box<ffi::MemoryRegions>,
}

/// rcheevos calls this only when it was given no map, and it calls it without
/// checking for null, so a real function has to exist even when there is
/// nothing to report. Reporting nothing is what leaves `Memory::new` with no
/// valid region to return.
unsafe extern "C" fn no_core_memory(_id: u32, info: *mut ffi::CoreMemoryInfo) {
    if let Some(info) = info.as_mut() {
        info.data = std::ptr::null_mut();
        info.size = 0;
    }
}

impl Memory {
    /// Build the flat space from the map a core described, or `None` when
    /// nothing usable came of it.
    ///
    /// # Safety
    ///
    /// Every `ptr` in `map` must stay valid for its `len` bytes for as long as
    /// the returned `Memory` lives. rcheevos keeps the pointers, so a core
    /// that is unloaded or a buffer that moves leaves this reading freed
    /// memory.
    pub unsafe fn new(map: &[MappedRegion], console: Console) -> Option<Memory> {
        let descriptors: Vec<ffi::MemoryDescriptor> = map.iter().map(descriptor).collect();
        let mmap = ffi::MemoryMap {
            descriptors: descriptors.as_ptr(),
            num_descriptors: descriptors.len() as u32,
        };

        // Boxed because rcheevos is handed the address and keeps nothing else;
        // moving the struct afterwards would be fine, but the pointers inside
        // it are what the reads go through, so it stays put.
        let mut regions = Box::new(ffi::MemoryRegions::empty());
        let ok =
            ffi::rc_libretro_memory_init(regions.as_mut(), &mmap, no_core_memory, console.id());
        if ok == 0 {
            // init cleans up after itself when it answers no.
            return None;
        }
        Some(Memory { regions })
    }

    /// How many bytes the flat space holds. For the GBA this is 32K of
    /// internal work RAM, 256K of external work RAM and 64K of save RAM.
    pub fn len(&self) -> usize {
        self.regions.total_size
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Read from the flat space, answering how many bytes were actually read.
    /// A read is served from one region, so a run crossing a seam is short
    /// rather than stitched.
    pub fn read(&self, address: u32, out: &mut [u8]) -> usize {
        if out.is_empty() {
            return 0;
        }
        // SAFETY: the regions came from rc_libretro_memory_init and are still
        // owned here; out is valid for its own length.
        unsafe {
            ffi::rc_libretro_memory_read(
                self.regions.as_ref(),
                address,
                out.as_mut_ptr(),
                out.len() as u32,
            ) as usize
        }
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        // SAFETY: built by rc_libretro_memory_init and not freed before now.
        unsafe { ffi::rc_libretro_memory_destroy(self.regions.as_mut()) }
    }
}

/// The layout the C compiler gave the shim's `struct retro_memory_descriptor`,
/// so a test can hold it to the libretro ABI.
pub fn descriptor_abi() -> DescriptorAbi {
    let mut abi = DescriptorAbi::default();
    // SAFETY: the callee only writes the struct it is given.
    unsafe { ffi::slot_descriptor_abi(&mut abi) };
    abi
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_of_a_region_reaches_rcheevos_unaltered() {
        // rcheevos reads these straight out of the struct. A field quietly
        // zeroed here cannot be seen in GBA behaviour, because the GBA is
        // asked only for exact addresses, so nothing downstream would notice
        // until a map that does need masking arrives.
        let mut byte = 0u8;
        let r = MappedRegion {
            flags: 0x4,
            ptr: &mut byte,
            offset: 0x20,
            start: 0x0200_0000,
            select: 0xFF00_0000,
            disconnect: 0x00FF_0000,
            len: 0x4_0000,
        };
        let d = descriptor(&r);
        assert_eq!(d.flags, 0x4);
        assert_eq!(d.ptr, (&mut byte as *mut u8).cast());
        assert_eq!(d.offset, 0x20);
        assert_eq!(d.start, 0x0200_0000);
        assert_eq!(d.select, 0xFF00_0000, "the mask that decodes mirrors");
        assert_eq!(d.disconnect, 0x00FF_0000, "the pins that are not wired");
        assert_eq!(d.len, 0x4_0000);
        assert!(
            d.addrspace.is_null(),
            "libretro lets a descriptor name an address space; a core that \
             names one and a frontend that invents a name would disagree"
        );
    }

    #[test]
    fn the_gameboy_advance_is_the_console_id_retroachievements_assigned_it() {
        // RC_CONSOLE_GAMEBOY_ADVANCE. Pass 4 and rcheevos hands back the
        // GameBoy's regions, which are a different size at different
        // addresses.
        assert_eq!(Console::GameBoyAdvance.id(), 5);
    }
}
