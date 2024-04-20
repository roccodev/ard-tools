//! Error -> libc errno conversion

use ardain::error::Error;
use libc::{c_int, EEXIST, EINVAL, EIO, ENOENT};
use log::{error, warn};

pub trait LibcError {
    fn errno(&self) -> c_int;

    /// Perform actions like logging, or even panicking if the error can't be recovered from
    fn handle(&self) {}
}

#[macro_export]
macro_rules! fuse_err {
    ($res:expr, $reply:expr) => {{
        match $res {
            Ok(x) => x,
            Err(e) => {
                use $crate::error::LibcError;
                ::log::debug!("fuse_err caught Err({e:?}) - {e}");
                e.handle();
                $reply.error(e.errno());
                return;
            }
        }
    }};
}

impl LibcError for Error {
    fn errno(&self) -> c_int {
        match self {
            Error::FsNoEntry => ENOENT,
            Error::FsAlreadyExists => EEXIST,
            Error::FsFileNameExtended => EINVAL,
            _ => EIO,
        }
    }

    fn handle(&self) {
        match self {
            e @ Error::FsFileNameExtended => warn!("{e}"),
            e if e.errno() == EIO => error!("{e}"),
            _ => {}
        }
    }
}

impl LibcError for anyhow::Error {
    fn errno(&self) -> c_int {
        EIO
    }

    fn handle(&self) {
        error!("{}", self)
    }
}
