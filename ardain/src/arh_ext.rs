//! Persistent data that makes working with ARD/ARH files easier

use std::mem::size_of;

use binrw::{BinRead, BinWrite};

use crate::{arh::Arh, FileMeta};

pub const BLOCK_SIZE_POW_DEFAULT: u16 = 9; // 512-byte blocks

#[derive(Debug, Clone, BinRead, BinWrite)]
#[brw(magic = b"arhx")]
pub struct ArhExtSection {
    pub section_size: u32,
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
    /// The size of each block, as a power of 2
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
            section_size: 0,
            allocated_blocks: BlockAllocTable::new(arh, block_size),
            file_meta_recycle_bin: FileRecycleBin::default(),
        }
    }

    pub(crate) fn calc_size(&mut self) {
        self.section_size = self
            .allocated_blocks
            .size_on_wire()
            .checked_add(self.file_meta_recycle_bin.size_on_wire())
            .and_then(|sz| sz.checked_add(size_of::<u32>()))
            .and_then(|sz| sz.try_into().ok())
            .expect("arhext size overflow");
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

    pub fn mark(&mut self, file: &FileMeta, occupied: bool) {
        let (file_start, file_end) = (file.offset, file.offset + u64::from(file.compressed_size));
        let mut start = file_start >> self.block_size_pow;
        let mut end = file_end >> self.block_size_pow;
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
            if occupied {
                self.blocks[item] |= 1 << in_item;
            } else {
                println!("Bit {in_item} ({item})");
                self.blocks[item] &= !(1 << in_item);
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

    fn size_on_wire(&self) -> usize {
        self.file_ids.len() * size_of::<u32>() + size_of::<u32>()
    }
}
