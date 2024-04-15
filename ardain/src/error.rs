use std::{io, num::TryFromIntError};

use xc3_lib::error::DecompressStreamError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Parse(#[from] binrw::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    SizeConvert(#[from] TryFromIntError),
    #[error("ARD entry decompression: {0}")]
    ArdDecompress(#[from] DecompressStreamError),
    #[error("FS: no such file or directory")]
    FsNoEntry,
    #[error("FS: an entry already exists with this name")]
    FsAlreadyExists,
    #[error("FS: extended file names are not supported (e.g. \"a.tar\", \"a.tar.gz\")")]
    FsFileNameExtended,
}
