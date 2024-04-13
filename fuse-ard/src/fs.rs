use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    ffi::OsStr,
    fs::File,
    hash::{Hash, Hasher},
    io::{BufReader, Read, Seek},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::Result;
use ardain::{ArdReader, ArhFileSystem, DirEntry, DirNode, FileMeta};
use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};
use libc::{ENOENT, ENOTDIR};

pub struct ArhFuseSystem {
    fs: ArhFileSystem,
    ard_file: ArdReader<BufReader<File>>,
    inode_cache: HashMap<u64, (String, u64)>,
}

const TTL: Duration = Duration::from_secs(1);
const INODE_ROOT: u64 = 1;

impl ArhFuseSystem {
    pub fn load(arh: impl Read + Seek, ard: File) -> Result<Self> {
        let fs = ArhFileSystem::load(arh)?;
        Ok(Self {
            fs,
            inode_cache: HashMap::default(),
            ard_file: ArdReader::new(BufReader::new(ard)),
        })
    }

    fn get_inode(&mut self, full_path: String) -> u64 {
        if full_path == "/" {
            return INODE_ROOT;
        }
        let mut hash = DefaultHasher::new();
        full_path.hash(&mut hash);
        let hash = hash.finish();
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
}

impl Filesystem for ArhFuseSystem {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let base = if parent == INODE_ROOT {
            ""
        } else {
            let Some(parent) = self.get_path(parent) else {
                reply.error(ENOENT);
                return;
            };
            parent
        };
        let name = name.to_str().expect("TODO");
        let name = format!("{base}/{name}");
        let ino = self.get_inode(name.clone());
        if let Some(dir) = self.fs.get_dir(&name) {
            reply.entry(&TTL, &make_dir_attr(dir, ino), 0);
            return;
        }
        if let Some(file) = self.fs.get_file_info(&name) {
            reply.entry(&TTL, &make_file_attr(file, ino), 0);
            return;
        }
        reply.error(ENOENT);
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        let Some(name) = self.get_path(ino) else {
            reply.error(ENOENT);
            return;
        };
        if let Some(dir) = self.fs.get_dir(&name) {
            reply.attr(&TTL, &make_dir_attr(dir, ino));
            return;
        }
        if let Some(file) = self.fs.get_file_info(&name) {
            reply.attr(&TTL, &make_file_attr(file, ino));
            return;
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let Some(dir) = self.get_path(ino).and_then(|path| self.fs.get_dir(path)) else {
            reply.error(ENOENT);
            return;
        };

        let DirEntry::Directory { children } = &dir.entry else {
            reply.error(ENOTDIR);
            return;
        };

        let mut entries = vec![
            (1, FileType::Directory, "."),
            (1, FileType::Directory, ".."),
        ];

        entries.extend(children.iter().map(|node| {
            (
                2,
                match node.entry {
                    DirEntry::File => FileType::RegularFile,
                    DirEntry::Directory { .. } => FileType::Directory,
                },
                node.name.as_str(),
            )
        }));

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            // i + 1 means the index of the next entry
            if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
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
            .and_then(|path| self.fs.get_file_info(path))
        else {
            reply.error(ENOENT);
            return;
        };
        assert!(offset >= 0);
        let data = self
            .ard_file
            .entry(file)
            .skip_take(offset as u64, size.into())
            .read()
            .unwrap();
        reply.data(&data);
    }

    fn forget(&mut self, _req: &Request, ino: u64, nlookup: u64) {
        let cnt = if let Some((_, cnt)) = self.inode_cache.get_mut(&ino) {
            *cnt = cnt.saturating_sub(nlookup);
            *cnt
        } else {
            return;
        };
        if cnt == 0 {
            self.inode_cache.remove(&ino);
        }
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
