use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    ffi::OsStr,
    fs::File,
    hash::{Hash, Hasher},
    io::{BufReader, BufWriter, Read, Seek},
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use ardain::{error::Result, ArdReader, ArhFileSystem, DirEntry, DirNode, FileMeta};
use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry,
    ReplyOpen, ReplyWrite, Request,
};
use libc::{EBADFD, EEXIST, ENOENT, ENOTDIR, ENOTEMPTY, ENOTSUP, O_RDWR, O_WRONLY};
use log::debug;

use crate::{fuse_err, write::FileBuffers, StandardArdFile};

pub struct ArhFuseSystem {
    pub arh: ArhFileSystem,
    pub ard: Option<StandardArdFile>,
    inode_cache: HashMap<u64, (String, u64)>,
    out_arh: PathBuf,
    write_buffers: FileBuffers,
}

const TTL: Duration = Duration::from_secs(1);
const INODE_ROOT: u64 = 1;

impl ArhFuseSystem {
    pub fn load(
        arh: impl Read + Seek,
        ard: Option<StandardArdFile>,
        out_arh: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        let fs = ArhFileSystem::load(arh)?;
        Ok(Self {
            arh: fs,
            inode_cache: HashMap::default(),
            ard,
            out_arh: PathBuf::from(out_arh.as_ref()),
            write_buffers: FileBuffers::default(),
        })
    }

    fn get_inode_and_save(&mut self, full_path: String) -> u64 {
        let hash = Self::hash_name(&full_path);
        self.inode_cache
            .entry(hash)
            .and_modify(|e| e.1 += 1)
            .or_insert_with(|| (full_path, 1));
        hash
    }

    fn get_path(&self, inode: u64) -> Option<&str> {
        if inode == INODE_ROOT {
            return Some("/");
        }
        self.inode_cache.get(&inode).map(|s| s.0.as_str())
    }

    pub(crate) fn sync(&mut self, only_data: bool) -> Result<()> {
        if !only_data {
            self.arh
                .sync(BufWriter::new(File::create(&self.out_arh)?))?;
        }
        Ok(())
    }

    fn build_path(&self, parent_inode: u64, name: &OsStr) -> Option<String> {
        let base = if parent_inode == INODE_ROOT {
            ""
        } else {
            let Some(parent) = self.get_path(parent_inode) else {
                return None;
            };
            parent
        };
        let name = name.to_str()?;
        Some(format!("{base}/{name}"))
    }

    fn is_fuse_dir_empty(&self, path: &str) -> bool {
        let Some(dir) = self.arh.get_dir(path) else {
            return true;
        };
        let DirEntry::Directory { children } = &dir.entry else {
            unreachable!()
        };
        if children.is_empty() {
            return true;
        }
        children.len() == 1
            && children[0].name == ".fuse_ard_dir"
            && matches!(children[0].entry, DirEntry::File)
    }

    fn hash_name(name: &str) -> u64 {
        if name == "/" {
            return INODE_ROOT;
        }
        let mut hash = DefaultHasher::new();
        name.hash(&mut hash);
        let hash = hash.finish();
        hash
    }
}

impl Filesystem for ArhFuseSystem {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let Some(name) = self.build_path(parent, name) else {
            debug!("[LOOKUP] invalid parent inode {parent}");
            reply.error(ENOENT);
            return;
        };
        let ino = self.get_inode_and_save(name.clone()); // TODO this creates inodes for invalid files too
        if let Some(dir) = self.arh.get_dir(&name) {
            debug!("[LOOKUP:{name}] found directory with inode {ino}");
            reply.entry(&TTL, &make_dir_attr(dir, ino), 0);
            return;
        }
        if let Some(file) = self.arh.get_file_info(&name) {
            debug!("[LOOKUP:{name}] found file with inode {ino}");
            reply.entry(&TTL, &make_file_attr(file, ino), 0);
            return;
        }
        debug!("[LOOKUP:{name}] no match");
        reply.error(ENOENT);
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        let Some(name) = self.get_path(ino) else {
            debug!("[GETATTR:{ino}] inode unknown");
            reply.error(ENOENT);
            return;
        };
        if let Some(dir) = self.arh.get_dir(&name) {
            reply.attr(&TTL, &make_dir_attr(dir, ino));
            return;
        }
        if let Some(file) = self.arh.get_file_info(&name) {
            reply.attr(&TTL, &make_file_attr(file, ino));
            return;
        }
        debug!("[GETATTR:{name}] no match");
        reply.error(ENOENT);
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let Some(dir) = self.get_path(ino).and_then(|path| self.arh.get_dir(path)) else {
            debug!("[READDIR:{ino}] inode unknown");
            reply.error(ENOENT);
            return;
        };

        let DirEntry::Directory { children } = &dir.entry else {
            reply.error(ENOTDIR);
            return;
        };

        let mut entries = vec![
            (1, Self::hash_name(".") as i64, FileType::Directory, "."),
            (1, Self::hash_name("..") as i64, FileType::Directory, ".."),
        ];

        entries.extend(children.iter().map(|node| {
            (
                2,
                Self::hash_name(&node.name) as i64,
                match node.entry {
                    DirEntry::File => FileType::RegularFile,
                    DirEntry::Directory { .. } => FileType::Directory,
                },
                node.name.as_str(),
            )
        }));

        // See readdir(2), we need to skip over the already sent entries
        let ofs_pos = entries
            .iter()
            .position(|e| e.1 == offset)
            .map(|pos| pos as isize)
            .unwrap_or(-1);
        let ofs_skip = (ofs_pos + 1) as usize;

        for entry in entries.into_iter().skip(ofs_skip) {
            if reply.add(entry.0, entry.1, entry.2, entry.3) {
                break;
            }
        }

        reply.ok();
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let Some(file) = self
            .get_path(ino)
            .and_then(|path| self.arh.get_file_info(path))
        else {
            debug!("[READ:{ino}] inode unknown");
            reply.error(ENOENT);
            return;
        };
        assert!(offset >= 0);
        let Some(ard) = self.ard.as_mut() else {
            reply.error(ENOTSUP);
            return;
        };
        let data = ard
            .reader
            .entry(file)
            .skip_take(offset as u64, size.into())
            .read()
            .unwrap();
        reply.data(&data);
    }

    fn forget(&mut self, _req: &Request, ino: u64, nlookup: u64) {
        let cnt = if let Some((_, cnt)) = self.inode_cache.get_mut(&ino) {
            debug!("[FORGET] Decrementing inode count for {ino} (cnt -= {nlookup})");
            *cnt = cnt.saturating_sub(nlookup);
            *cnt
        } else {
            return;
        };
        if cnt == 0 {
            debug!("[FORGET] Forgetting {ino} (cnt = 0)");
            self.inode_cache.remove(&ino);
        }
    }

    fn mknod(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        let Some(name) = self.build_path(parent, name) else {
            debug!("[MKNOD] invalid parent inode {parent}");
            reply.error(ENOENT);
            return;
        };
        let inode = self.get_inode_and_save(name.clone());
        let meta = fuse_err!(self.arh.create_file(&name), reply);
        reply.entry(&TTL, &make_file_attr(&meta, inode), 0);
    }

    fn mkdir(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let Some(name) = self.build_path(parent, name) else {
            debug!("[MKDIR] invalid parent inode {parent}");
            reply.error(ENOENT);
            return;
        };
        if self.arh.exists(&name) {
            debug!("[MKDIR] entry already exists {name}");
            reply.error(EEXIST);
            return;
        }
        // The ARH format has no concept of directories, we create a hidden file to generate
        // the directory structure. Directories are automatically deleted when they are empty.
        let placeholder = format!("{name}/.fuse_ard_dir");
        fuse_err!(self.arh.create_file(&placeholder), reply);
        let inode = self.get_inode_and_save(placeholder);
        let dir = self.arh.get_dir(&name).unwrap();
        reply.entry(&TTL, &make_dir_attr(dir, inode), 0);
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let Some(name) = self.build_path(parent, name) else {
            debug!("[UNLINK] invalid parent inode {parent}");
            reply.error(ENOENT);
            return;
        };
        fuse_err!(self.arh.delete_file(&name), reply);
        reply.ok();
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let Some(name) = self.build_path(parent, name) else {
            debug!("[RMDIR] invalid parent inode {parent}");
            reply.error(ENOENT);
            return;
        };
        if !self.is_fuse_dir_empty(&name) {
            debug!("[RMDIR] dir {name} is not empty");
            reply.error(ENOTEMPTY);
            return;
        }
        // Recursive deletion is handled by the caller.
        // We delete the hidden file we made if we created the directory
        self.arh.delete_file(&format!("{name}/.fuse_ard_dir")).ok();
        fuse_err!(self.arh.delete_empty_dir(&name), reply);
        reply.ok();
    }

    fn rename(
        &mut self,
        _req: &Request,
        old_parent: u64,
        old_name: &OsStr,
        new_parent: u64,
        new_name: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        let Some(old_name) = self.build_path(old_parent, old_name) else {
            debug!("[RENAME] invalid parent inode {old_parent}");
            reply.error(ENOENT);
            return;
        };
        let Some(new_name) = self.build_path(new_parent, new_name) else {
            debug!("[RENAME] invalid parent inode {new_parent}");
            reply.error(ENOENT);
            return;
        };
        if self.arh.get_dir(&old_name).is_some() {
            fuse_err!(self.arh.rename_dir(&old_name, &new_name), reply);
            reply.ok();
            return;
        }
        if self.arh.get_file_info(&old_name).is_some() {
            fuse_err!(self.arh.rename_file(&old_name, &new_name), reply);
            reply.ok();
            return;
        }
        debug!("[RENAME] no match {old_parent}");
    }

    fn open(&mut self, _req: &Request, ino: u64, flags: i32, reply: ReplyOpen) {
        if flags & O_RDWR != 0 || flags & O_WRONLY != 0 {
            // We only care about writable fds
            let Some(path) = self
                .get_path(ino)
                .and_then(|path| self.arh.get_file_info(path).map(|_| path))
            else {
                debug!("[OPEN.W:{ino}] inode unknown");
                reply.error(ENOENT);
                return;
            };
            let fd = self.write_buffers.open(path.to_string());
            reply.opened(fd, 0);
            return;
        }
        reply.opened(ino, 0)
    }

    fn write(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        let Some(buf) = self.write_buffers.get_handle(fh) else {
            debug!("[WRITE:{ino},{fh}] bad descriptor");
            reply.error(EBADFD);
            return;
        };
        buf.write(offset, data);
        reply.written(data.len().try_into().unwrap());
    }

    fn flush(&mut self, _req: &Request, _ino: u64, fh: u64, _owner: u64, reply: ReplyEmpty) {
        let Some(buf) = self.write_buffers.get_handle(fh) else {
            // Silently ignore (we only care about writable FDs getting close()d)
            reply.ok();
            return;
        };
        let Some(ard) = self.ard.as_mut() else {
            reply.error(ENOTSUP);
            return;
        };
        fuse_err!(buf.flush(&mut self.arh, ard), reply);
        reply.ok();
    }

    fn fsync(&mut self, _req: &Request, _ino: u64, _fh: u64, only_data: bool, reply: ReplyEmpty) {
        fuse_err!(self.sync(only_data), reply);
        reply.ok();
    }

    fn fsyncdir(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        only_data: bool,
        reply: ReplyEmpty,
    ) {
        fuse_err!(self.sync(only_data), reply);
        reply.ok();
    }

    fn release(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        self.write_buffers.release(fh);
        reply.ok();
    }

    fn destroy(&mut self) {
        if let Some(ard) = self.ard.as_mut() {
            self.write_buffers
                .flush_all(&mut self.arh, ard)
                .expect("could not sync write buffers, data may be lost");
        }
        self.sync(false)
            .expect("could not sync file system, data may be lost");
    }
}

fn make_dir_attr(dir: &DirNode, inode: u64) -> FileAttr {
    FileAttr {
        ino: inode,
        size: 0,
        blocks: 0,
        atime: UNIX_EPOCH,
        mtime: UNIX_EPOCH,
        ctime: UNIX_EPOCH,
        crtime: UNIX_EPOCH,
        kind: FileType::Directory,
        perm: 0o705,
        nlink: 2,
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 0,
        flags: 0,
    }
}

fn make_file_attr(file: &FileMeta, inode: u64) -> FileAttr {
    let mut sz = file.uncompressed_size.into();
    if sz == 0 && file.compressed_size != 48 {
        sz = file.compressed_size.into();
    }
    FileAttr {
        ino: inode,
        size: sz,
        blocks: 0,
        atime: UNIX_EPOCH,
        mtime: UNIX_EPOCH,
        ctime: UNIX_EPOCH,
        crtime: UNIX_EPOCH,
        kind: FileType::RegularFile,
        perm: 0o700,
        nlink: 0,
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 0,
        flags: 0,
    }
}
