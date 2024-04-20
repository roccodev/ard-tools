//! ARD file allocator

use std::io::{Seek, Write};

use xc3_lib::xbc1::{CompressionType, Xbc1};

use crate::{
    ard::ArdWriter, arh::FileTable, arh_ext::BlockAllocTable, error::Result, ArhFileSystem,
    FileFlag, FileMeta,
};

pub struct ArdFileAllocator<'a, 'w, W> {
    block_table: &'a mut BlockAllocTable,
    file_table: &'a mut FileTable,
    writer: &'w mut ArdWriter<W>,
}

pub enum CompressionStrategy {
    /// Never compress entries.
    None,
    /// Use the default compression algorithm the game supports.
    Standard,
    /// Compress using all available methods, then pick the smallest result.
    Best,
}

enum EntryFile<'a> {
    /// Stored verbatim
    Raw(&'a [u8]),
    /// Stored uncompressed, but within a XBC1 structure
    RawWrapped(&'a [u8]),
    /// Compressed and wrapped in a XBC1 structure
    Compressed(Box<[u8]>, CompressionMeta),
}

struct CompressionMeta {
    compression_type: CompressionType,
    uncompressed_len: u32,
    crc_hash: u32,
}

impl<'a, 'w, W: Write + Seek> ArdFileAllocator<'a, 'w, W> {
    pub fn new(arh: &'a mut ArhFileSystem, writer: &'w mut ArdWriter<W>) -> Self {
        arh.arh.get_or_init_ext(&arh.opts);
        Self {
            block_table: &mut arh.arh.arh_ext_section.as_mut().unwrap().allocated_blocks,
            file_table: &mut arh.arh.file_table,
            writer,
        }
    }

    /// Writes the file as a new entry.
    ///
    /// The allocator compresses the data in accordance with the
    /// compression strategy. It then tries to find free space in the archive,
    /// and writes the data to the file.
    pub fn write_new_file(
        &mut self,
        file_id: u32,
        data: &[u8],
        strategy: CompressionStrategy,
    ) -> Result<()> {
        let file = self
            .file_table
            .get_meta_mut(file_id)
            .expect("file not found");
        let data = Self::compress_data(data, strategy);
        let total_len: u64 = data.size_on_disk().try_into().unwrap();
        let offset = self.block_table.find_free_space(total_len);
        data.write(self.writer.entry(offset)?)?;
        Self::update_meta(self.block_table, &data, file, offset);
        Ok(())
    }

    /// Writes the file, replacing the entry pointed identified by `file_id`.
    ///
    /// This works like [`Self::write_new_file`], except it treats the file as
    /// empty, and frees the space occupied by the old file.
    pub fn replace_file(
        &mut self,
        file_id: u32,
        new_data: &[u8],
        strategy: CompressionStrategy,
    ) -> Result<()> {
        let file = self
            .file_table
            .get_meta_mut(file_id)
            .expect("file not found");
        let data = Self::compress_data(new_data, strategy);
        if data.size_on_disk() <= file.compressed_size.try_into().unwrap() {
            // If it fits, just write and update size
            data.write(self.writer.entry(file.offset)?)?;
            Self::update_meta(self.block_table, &data, file, file.offset);
            return Ok(());
        }
        let total_len: u64 = data.size_on_disk().try_into().unwrap();
        let offset = self.block_table.find_space_replace(file, total_len);
        data.write(self.writer.entry(offset)?)?;
        // First, mark the old file as unoccupied
        self.block_table.mark(file, false);
        // After updating the file entry, this will mark the new one as occupied
        // (no problem if they overlap)
        Self::update_meta(self.block_table, &data, file, offset);
        Ok(())
    }

    fn compress_data(data: &[u8], strategy: CompressionStrategy) -> EntryFile {
        // TODO: actually compress
        EntryFile::Raw(data)
    }

    fn update_meta(
        alloc_table: &mut BlockAllocTable,
        data: &EntryFile,
        meta: &mut FileMeta,
        offset: u64,
    ) {
        meta.offset = offset;
        let (has_xbc1, unc_size) = match data {
            EntryFile::Raw(_) => (false, 0),
            EntryFile::RawWrapped(_) => (true, 0),
            EntryFile::Compressed(_, meta) => (true, meta.uncompressed_len),
        };
        meta.set_flag(FileFlag::HasXbc1Header, has_xbc1);
        meta.uncompressed_size = unc_size;
        meta.compressed_size = data.size_on_disk().try_into().unwrap();
        alloc_table.mark(meta, true);
    }
}

impl<'a> EntryFile<'a> {
    pub fn write(&self, mut writer: impl Write + Seek) -> Result<()> {
        if let Self::Raw(data) = self {
            writer.write_all(data)?;
            return Ok(());
        }
        let xbc1 = match self {
            EntryFile::RawWrapped(data) => {
                Xbc1::from_decompressed(String::new(), data, CompressionType::Uncompressed)
                    .expect("TODO")
            }
            EntryFile::Compressed(data, meta) => Xbc1 {
                compression_type: meta.compression_type,
                decompressed_size: meta.uncompressed_len,
                compressed_size: data.len().try_into().unwrap(),
                decompressed_hash: meta.crc_hash,
                name: String::new(),
                compressed_stream: data.to_vec(),
            },
            EntryFile::Raw(_) => unreachable!(),
        };
        xbc1.write(&mut writer)?;
        Ok(())
    }

    pub fn size_on_disk(&self) -> usize {
        match self {
            EntryFile::Raw(data) => data.len(),
            EntryFile::RawWrapped(data) => data.len() + 0x30,
            EntryFile::Compressed(data, _) => data.len() + 0x30,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match self {
            EntryFile::Raw(buf) => buf,
            EntryFile::RawWrapped(buf) => buf,
            EntryFile::Compressed(buf, _) => buf,
        }
    }
}
