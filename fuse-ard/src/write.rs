//! Temporary buffers to hold data before it's ready to be written.
//!
//! Files stored in ARD files are potentially compressed, so we can't write them in chunks.
//! We hold onto their data until the user calls `close` or `fsync`.

use std::io::Write;

use anyhow::Result;
use ardain::{
    file_alloc::{ArdFileAllocator, CompressionStrategy},
    path::ArhPath,
    ArhFileSystem,
};
use log::warn;

use crate::StandardArdFile;

#[derive(Default)]
pub struct FileBuffers {
    open_files: Vec<FileBuffer>,
}

pub struct FileBuffer {
    path: ArhPath,
    operations: Vec<Operation>,
}

enum Operation {
    Truncate { new_size: u64 },
    Write { offset: u64, data: Box<[u8]> },
}

impl FileBuffers {
    pub fn open(&mut self, path: ArhPath) -> u64 {
        match self.open_files.binary_search_by_key(&&path, |f| &f.path) {
            Ok(i) => i.try_into().unwrap(),
            Err(i) => {
                self.open_files.insert(
                    i,
                    FileBuffer {
                        path: path,
                        operations: Vec::new(),
                    },
                );
                i.try_into().unwrap()
            }
        }
    }

    pub fn release(&mut self, fd: u64) {
        let index: usize = fd.try_into().unwrap();
        if index < self.open_files.len() {
            self.open_files.remove(index);
        }
    }

    pub fn get_handle(&mut self, fd: u64) -> Option<&mut FileBuffer> {
        self.open_files.get_mut(usize::try_from(fd).ok()?)
    }

    pub fn flush_all(&mut self, arh: &mut ArhFileSystem, ard: &mut StandardArdFile) -> Result<()> {
        for file in &mut self.open_files {
            file.flush(arh, ard)?;
        }
        Ok(())
    }
}

impl FileBuffer {
    pub fn write(&mut self, offset: i64, data: &[u8]) {
        self.operations.push(Operation::Write {
            data: data.to_vec().into_boxed_slice(),
            offset: offset.try_into().unwrap(),
        })
    }

    pub fn flush(&mut self, arh: &mut ArhFileSystem, ard: &mut StandardArdFile) -> Result<()> {
        // Read the file, apply changes, then write back
        let Some(meta) = arh.get_file_info(&self.path).copied() else {
            // Likely deleted but didn't call `close`
            warn!(
                "[flush] dangling file descriptor (forgot to close()?): {}",
                self.path
            );
            return Ok(());
        };
        let mut buf = ard.reader.entry(&meta).read()?;
        for op in self.operations.drain(..) {
            op.run(&mut buf)?;
        }
        // TODO strategy
        ArdFileAllocator::new(arh, &mut ard.writer).replace_file(
            meta.id,
            &buf,
            CompressionStrategy::None,
        )?;
        // Make sure arh modifications get saved to disk
        ard.writer.get_mut().flush()?;
        Ok(())
    }

    pub fn truncate(&mut self, new_size: u64) {
        self.operations.push(Operation::Truncate { new_size });
    }
}

impl Operation {
    fn run(&self, buffer: &mut Vec<u8>) -> Result<()> {
        match self {
            Operation::Truncate { new_size } => buffer.resize(usize::try_from(*new_size)?, 0),
            Operation::Write { offset, data } => {
                let mut offset = usize::try_from(*offset)?;
                let end = offset + data.len();
                let max_len = buffer.len();
                if offset < max_len {
                    let first_area = &mut buffer[offset..end.min(max_len)];
                    first_area.copy_from_slice(&data[..first_area.len()]);
                    offset += first_area.len();
                    if offset < end {
                        buffer.extend_from_slice(&data[offset..]);
                    }
                } else {
                    buffer.extend_from_slice(&data);
                }
            }
        }
        Ok(())
    }
}
