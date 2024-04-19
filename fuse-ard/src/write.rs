//! Temporary buffers to hold data before it's ready to be written.
//!
//! Files stored in ARD files are potentially compressed, so we can't write them in chunks.
//! We hold onto their data until the user calls `close` or `fsync`.

use std::io::{Seek, Write};

use anyhow::Result;
use ardain::{
    file_alloc::{ArdFileAllocator, CompressionStrategy},
    ArhFileSystem,
};
use log::warn;

use crate::StandardArdFile;

#[derive(Default)]
pub struct FileBuffers {
    open_files: Vec<FileBuffer>,
}

pub struct FileBuffer {
    path: String,
    chunks: Vec<FileChunk>,
}

struct FileChunk {
    data: Vec<u8>,
    offset: u64,
}

impl FileBuffers {
    pub fn open(&mut self, path: String) -> u64 {
        match self.open_files.binary_search_by_key(&&path, |f| &f.path) {
            Ok(i) => i.try_into().unwrap(),
            Err(i) => {
                self.open_files.insert(
                    i,
                    FileBuffer {
                        path: path,
                        chunks: Vec::new(),
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
        self.chunks.push(FileChunk {
            data: data.to_vec(),
            offset: offset.try_into().unwrap(),
        })
    }

    pub fn flush(&mut self, arh: &mut ArhFileSystem, ard: &mut StandardArdFile) -> Result<()> {
        // Read the file, apply changes, then write back
        let Some(meta) = arh.get_file_info(&self.path).copied() else {
            // Likely deleted but didn't call `close`
            warn!(
                "dangling file descriptor (forgot to close()?): {}",
                self.path
            );
            return Ok(());
        };
        let mut buf = ard.reader.entry(&meta).read()?;
        for chunk in self.chunks.drain(..) {
            let FileChunk { data, offset } = chunk;
            let mut offset = usize::try_from(offset)?;
            let end = offset + data.len();
            let max_len = buf.len();
            if offset < max_len {
                let first_area = &mut buf[offset..end.min(max_len)];
                first_area.copy_from_slice(&data[..first_area.len()]);
                offset += first_area.len();
                if offset < end {
                    buf.extend_from_slice(&data[offset..]);
                }
            } else {
                buf.extend_from_slice(&data);
            }
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
}
