use std::io::{Read, Seek, SeekFrom, Write};

use xc3_lib::xbc1::Xbc1;

use crate::error::Result;
use crate::FileMeta;

/// Provides easy access to entries in an ARD file.
pub struct ArdReader<R> {
    reader: R,
}

pub struct ArdWriter<W> {
    writer: W,
}

pub enum CompressionStrategy {
    /// Never compress entries.
    None,
    /// Use the default compression algorithm the game supports.
    Standard,
    /// Compress using all available methods, then pick the smallest result.
    Best,
}

pub struct EntryReader<R> {
    reader: R,
    offset: u64,
    entry_size: u64,
    compressed: bool,
}

pub struct OffsetReader<R> {
    entry: EntryReader<R>,
    offset: u64,
    max_size: Option<u64>,
}

impl<R: Read + Seek> ArdReader<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Returns a handle that can read a file entry.
    ///
    /// The file will be transparently decompressed if needed.
    pub fn entry(&mut self, file: &FileMeta) -> EntryReader<&mut R> {
        EntryReader {
            reader: &mut self.reader,
            offset: file.offset,
            compressed: file.uncompressed_size != 0,
            entry_size: file.compressed_size.into(),
        }
    }
}

impl<W: Write + Seek> ArdWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn write_entry(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.writer.seek(SeekFrom::Start(offset))?;
        self.writer.write_all(data)?;
        Ok(())
    }
}

impl<R: Read + Seek> EntryReader<R> {
    /// Reads the entry in full.
    pub fn read(&mut self) -> Result<Vec<u8>> {
        self.read_at(0, self.entry_size)
    }

    /// Wraps the reader to apply an offset and stop reading before the end of the file.
    pub fn skip_take(self, skip: u64, take: u64) -> OffsetReader<R> {
        OffsetReader {
            entry: self,
            offset: skip,
            max_size: Some(take),
        }
    }

    fn read_at(&mut self, offset_in_entry: u64, max_size: u64) -> Result<Vec<u8>> {
        self.reader.seek(SeekFrom::Start(self.offset))?;
        if self.compressed {
            let xbc1 = Xbc1::read(&mut self.reader)?;
            let buf = xbc1.decompress()?;
            let end = offset_in_entry
                .saturating_add(max_size)
                .min(xbc1.decompressed_size.into());
            Ok(buf[offset_in_entry.try_into()?..end.try_into()?].to_vec())
        } else {
            let size = self
                .entry_size
                .saturating_sub(offset_in_entry)
                .min(max_size);
            let mut buf = vec![0u8; size.try_into()?];
            let reader = &mut self.reader;
            reader.seek(SeekFrom::Current(offset_in_entry.try_into()?))?;
            reader.take(size.into()).read_exact(&mut buf)?;
            Ok(buf)
        }
    }
}

impl<R: Read + Seek> OffsetReader<R> {
    pub fn read(&mut self) -> Result<Vec<u8>> {
        self.entry
            .read_at(self.offset, self.max_size.unwrap_or(self.entry.entry_size))
    }
}
