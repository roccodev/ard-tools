use std::{io, num::TryFromIntError};

use xc3_lib::error::{CreateXbc1Error, DecompressStreamError};

use crate::path::InvalidPathError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Path(#[from] InvalidPathError),
    #[error(transparent)]
    Parse(#[from] binrw::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    SizeConvert(#[from] TryFromIntError),
    #[error("ARD entry decompression: {0}, corrupted ARD entry?")]
    ArdDecompress(#[from] DecompressStreamError),
    #[error("FS: no such file or directory")]
    FsNoEntry,
    #[error("FS: an entry already exists with this name")]
    FsAlreadyExists,
    #[error("FS (ARH1): extended file names are not supported (e.g. \"a.tar\", \"a.tar.gz\")")]
    FsFileNameExtended,
    #[error("FS: part of the file name is not available for use")]
    FsReservedName,
}

impl From<CreateXbc1Error> for Error {
    fn from(value: CreateXbc1Error) -> Self {
        match value {
            CreateXbc1Error::Io(e) => Self::Io(e),
        }
    }
}
