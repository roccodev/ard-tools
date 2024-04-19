//! Persistent data that makes working with ARD/ARH files easier

use std::mem::size_of;

use binrw::{BinRead, BinWrite};

use crate::{arh::Arh, FileMeta};

pub const BLOCK_SIZE_POW_DEFAULT: u16 = 9; // 512-byte blocks

#[derive(Debug, Clone, BinRead, BinWrite)]
#[brw(magic = b"arhx")]
pub struct ArhExtSection {
    pub allocated_blocks: BlockAllocTable,
    pub file_meta_recycle_bin: FileRecycleBin,
}

#[derive(Debug, Clone, Copy, BinRead, BinWrite)]
#[brw(magic = b"arhx")]
pub struct ArhExtOffsets {
    pub section_offset: u32,
}

/// File block allocation table
///
/// To make finding the right place to allocate a file easier, we divide the available space
/// in the ARD file into blocks of fixed size.
#[derive(Debug, Clone, BinRead, BinWrite)]
pub struct BlockAllocTable {
    /// The size of each block, as an exponent base 2
    pub block_size_pow: u16,
    block_arr_count: u64,
    #[br(args { count: block_arr_count.try_into().unwrap() })]
    blocks: Vec<u64>,
}

#[derive(Debug, Clone, BinRead, BinWrite, Default)]
pub struct FileRecycleBin {
    len: u32,
    #[br(args { count: len.try_into().unwrap() })]
    file_ids: Vec<u32>,
}

impl ArhExtSection {
    pub fn new(arh: &Arh, block_size: u16) -> Self {
        Self {
            allocated_blocks: BlockAllocTable::new(arh, block_size),
            file_meta_recycle_bin: FileRecycleBin::default(),
        }
    }

    pub fn recycle_bin(&self) -> &FileRecycleBin {
        &self.file_meta_recycle_bin
    }

    pub fn recycle_bin_mut(&mut self) -> &mut FileRecycleBin {
        &mut self.file_meta_recycle_bin
    }

    pub(crate) fn calc_size(&mut self) -> u32 {
        self.allocated_blocks
            .size_on_wire()
            .checked_add(self.file_meta_recycle_bin.size_on_wire())
            .and_then(|sz| sz.checked_add(size_of::<u32>()))
            .and_then(|sz| sz.try_into().ok())
            .expect("arhext size overflow")
    }
}

impl BlockAllocTable {
    fn new(arh: &Arh, block_size_pow: u16) -> Self {
        let mut res = Self {
            block_size_pow,
            block_arr_count: 0,
            blocks: Vec::new(),
        };
        for file in arh.file_table.files() {
            res.mark(file, true);
        }
        res
    }

    /// Returns the starting offset for an area with at least `desired_size` free bytes.
    ///
    /// The returned area is not guaranteed to be the one that comes first, nor must it be
    /// the one with the minimum size.
    pub fn find_free_space(&self, desired_size: u64) -> u64 {
        self.find_free_space_inner(desired_size, |_, i| i)
    }

    /// Treats the area occupied by `old_file` as empty, and returns the starting offset for an
    /// area with at least `desired_size` free bytes.
    pub fn find_space_replace(&self, old_file: &FileMeta, desired_size: u64) -> u64 {
        if old_file.compressed_size == 0 {
            return self.find_free_space(desired_size);
        }
        if desired_size <= old_file.compressed_size.into() {
            // Nothing to do, can reuse old space
            return old_file.offset;
        }
        let file_start_block = old_file.offset.div_ceil(1 << self.block_size_pow);
        let file_end_block = (old_file.offset + u64::from(old_file.compressed_size))
            .div_ceil(1 << self.block_size_pow)
            .saturating_sub(1);
        const BITS: u64 = u64::BITS as u64;
        let patch = |i, slot| {
            let first_block = u64::try_from(i).unwrap() * BITS;
            let last_block = first_block + BITS - 1;
            if first_block >= file_start_block && last_block <= file_end_block {
                // Completely free
                return 0;
            }
            if file_start_block >= first_block && file_start_block <= last_block {
                // [.....FFFFF] 0-9
                // First       Last
                return slot & !((1 << (file_start_block - first_block)) - 1);
            }
            if file_end_block >= first_block && file_end_block <= last_block {
                // [FFFF------]
                // First       Last
                return slot & ((1 << (last_block - file_end_block)) - 1);
            }
            slot
        };
        self.find_free_space_inner(desired_size, patch)
    }

    /// Used for the previous two functions
    ///
    /// The patch function can patch specific block slots, for example to make some blocks
    /// temporarily available.
    fn find_free_space_inner(
        &self,
        desired_size: u64,
        patch_fn: impl Fn(usize, u64) -> u64,
    ) -> u64 {
        const BITS: u64 = u64::BITS as u64;
        let desired_blocks = desired_size.div_ceil(1 << self.block_size_pow);

        let mut carry: u64 = 0;
        let mut start_block = 0;
        for (i, slot) in self.blocks.iter().copied().enumerate() {
            let slot = patch_fn(i, slot);
            let first_block = u64::try_from(i).unwrap() * BITS;
            let mut trailing = 0;
            // All occupied
            if slot != u64::MAX {
                if slot == 0 {
                    // All 0 => all 64 blocks are free
                    carry += BITS;
                    if carry >= desired_blocks {
                        return start_block * (1 << self.block_size_pow);
                    }
                    continue;
                }
                let leading = u64::from(slot.leading_zeros());
                trailing = u64::from(slot.trailing_zeros());
                if carry + leading >= desired_blocks {
                    // Case 1: carried over + leading free blocks
                    return start_block * (1 << self.block_size_pow);
                }
                if desired_blocks <= BITS - leading - trailing {
                    // Case 2: free blocks in the middle of a slot
                    let n_slot = !slot;
                    let mut mask = (1 << desired_blocks) - 1;
                    while mask & (1 << 63) == 0 {
                        if n_slot & mask == mask {
                            return (first_block + u64::from(mask.leading_zeros()))
                                * (1 << self.block_size_pow);
                        }
                        mask <<= 1;
                    }
                }
            }
            // Carry over trailing free blocks for case 1
            carry = trailing;
            start_block = first_block + BITS - carry;
        }
        // No free space
        let last = self.blocks.last().copied().unwrap_or_default();
        let first_free_block =
            u64::try_from(self.blocks.len()).unwrap() * BITS - u64::from(last.trailing_zeros());
        first_free_block * (1 << self.block_size_pow)
    }

    pub fn mark(&mut self, file: &FileMeta, occupied: bool) {
        let (file_start, file_end) = (file.offset, file.offset + u64::from(file.compressed_size));
        let mut start = file_start >> self.block_size_pow;
        let mut end = file_end.div_ceil(1 << self.block_size_pow);
        if !occupied {
            // We write files with sizes that are a multiple of the block size. If we are freeing
            // a file that only covers the start (or end) block partially, we must not mark the block
            // as freed because another file might also be there.
            if file_start % (1 << self.block_size_pow) != 0 {
                start += 1;
            }
            if file_end % (1 << self.block_size_pow) != 0 {
                end -= 1;
            }
        }
        for block in start..=end {
            let item = (block / 64) as usize;
            let in_item = block % 64;
            while item >= self.blocks.len() {
                self.blocks.push(0);
            }

            if occupied {
                self.blocks[item] |= 1 << (63 - in_item);
            } else {
                self.blocks[item] &= !(1 << (63 - in_item));
            }
        }
        self.block_arr_count = self.blocks.len().try_into().unwrap();
    }

    fn size_on_wire(&self) -> usize {
        self.blocks.len() * size_of::<u64>() + size_of::<u32>() + size_of::<u16>()
    }
}

impl FileRecycleBin {
    pub fn push(&mut self, file_id: u32) {
        if let Err(i) = self.file_ids.binary_search(&file_id) {
            self.file_ids.insert(i, file_id);
            self.len += 1;
        }
    }

    pub fn pop(&mut self) -> Option<u32> {
        self.len = self.len.saturating_sub(1);
        self.file_ids.pop()
    }

    fn size_on_wire(&self) -> usize {
        self.file_ids.len() * size_of::<u32>() + size_of::<u32>()
    }
}

#[cfg(test)]
mod tests {
    use crate::FileMeta;

    use super::BlockAllocTable;

    const BLOCK_POW: u16 = 9;
    const BLOCK_SIZE: u64 = 1 << BLOCK_POW;

    #[test]
    fn block_table_find() {
        // Case 1: leading free blocks
        let table = BlockAllocTable {
            block_size_pow: BLOCK_POW,
            block_arr_count: 0,
            // 4+64 free blocks, 192 occupied blocks
            blocks: vec![!0b1111, 0, u64::MAX, u64::MAX, u64::MAX],
        };
        assert_eq!(table.find_free_space(50 * BLOCK_SIZE), 60 * BLOCK_SIZE);
        assert_eq!(table.find_free_space(70 * BLOCK_SIZE), 320 * BLOCK_SIZE);

        // Case 1: trailing free blocks
        let table = BlockAllocTable {
            block_size_pow: BLOCK_POW,
            block_arr_count: 0,
            // 128 occupied blocks, 64+61 free blocks, 64 occupied blocks
            blocks: vec![u64::MAX, u64::MAX, 0, 0b111, u64::MAX],
        };
        assert_eq!(
            table.find_free_space((64 + 61) * BLOCK_SIZE),
            128 * BLOCK_SIZE
        );
        assert_eq!(
            table.find_free_space((64 + 62) * BLOCK_SIZE),
            320 * BLOCK_SIZE
        );

        // Case 2: free blocks in the middle
        let table = BlockAllocTable {
            block_size_pow: BLOCK_POW,
            block_arr_count: 0,
            blocks: vec![0b1110000110001100111111110111111111111111111111111111111111111111],
        };
        assert_eq!(table.find_free_space(2 * BLOCK_SIZE), 14 * BLOCK_SIZE);
        assert_eq!(table.find_free_space(3 * BLOCK_SIZE), 9 * BLOCK_SIZE);
        assert_eq!(table.find_free_space(4 * BLOCK_SIZE), 3 * BLOCK_SIZE);
    }

    #[test]
    fn block_table_find_replace() {
        let file = FileMeta::new_for_test(60 * BLOCK_SIZE, 68 * BLOCK_SIZE as u32);
        let table = BlockAllocTable {
            block_size_pow: BLOCK_POW,
            block_arr_count: 0,
            // 60 free blocks, 4+64 occupied blocks (occupied by `file`), 54 occupied blocks,
            // 10 free blocks, 128 occupied blocks
            blocks: vec![0b1111, u64::MAX, !0b1111111111, u64::MAX, u64::MAX],
        };
        assert_eq!(table.find_space_replace(&file, 100 * BLOCK_SIZE), 0);
        assert_eq!(
            table.find_space_replace(&file, 40 * BLOCK_SIZE),
            60 * BLOCK_SIZE
        );
        assert_eq!(
            table.find_space_replace(&file, 129 * BLOCK_SIZE),
            320 * BLOCK_SIZE
        );
    }
}
