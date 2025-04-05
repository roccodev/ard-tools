use std::{
    any::Any,
    collections::VecDeque,
    io::{Read, Seek, Write},
    num::NonZeroU64,
};

use binrw::{BinRead, BinWrite};

use crate::{
    arh1::Arh1,
    arh2::Arh2,
    compat::{CompatArh, CompatArhDyn},
    error::{Error, Result},
    opts::ArhOptions,
    path::ArhPath,
    FileFlag,
};

pub struct ArhFileSystem<A> {
    pub(crate) arh_new: A,
    pub(crate) opts: ArhOptions,
    // Not part of the ARH format, but we keep one to make enumerating and traversing directories
    // easier.
    dir_tree: DirNode,
}

pub type ArhCompatFileSystem = ArhFileSystem<CompatArhDyn>;
pub type Arh1FileSystem = ArhFileSystem<Arh1>;
pub type Arh2FileSystem = ArhFileSystem<Arh2>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileEntry {
    /// The offset of the file in the ARD
    pub ard_offset: u64,
    /// The size of the file, in terms of space occupied in the ARD
    pub ard_size: u64,
    /// If the file is compressed, this is the uncompressed file size
    pub expanded_size: Option<NonZeroU64>,
    /// An ID unique to the file path
    pub unique_id: u64,

    // Flags
    pub(crate) xbc1_header: bool,
    pub(crate) hidden: bool,
}

#[derive(Debug)]
pub struct DirNode {
    pub name: String,
    pub entry: DirEntry,
}

#[derive(Debug)]
pub enum DirEntry {
    File,
    Directory { children: Vec<DirNode> },
}

pub(crate) mod private {
    use super::{DirNode, FileEntry};
    use crate::{
        compat::{CompatArh, CompatArhRef},
        error::Result,
        opts::ArhOptions,
        path::ArhPath,
    };

    pub trait ArhAccessPrivate {
        fn post_read(&mut self);
        fn get_file_entry(&self, path: &ArhPath) -> Option<FileEntry>;
        fn init_dir_tree(&self, opts: &ArhOptions) -> DirNode;

        fn create_file(&mut self, path: &ArhPath) -> Result<FileEntry>;
        fn delete_file(&mut self, path: &ArhPath, opts: &ArhOptions) -> Result<()>;
        fn hide_file(&mut self, path: &ArhPath, hidden: bool) -> Result<()>;
        fn rename_file(
            &mut self,
            from: &ArhPath,
            to: &ArhPath,
            opts: &ArhOptions,
            dir_tree: &mut DirNode,
        ) -> Result<()>;
        fn prepare_for_write(&mut self);

        fn into_compat(self: Box<Self>) -> CompatArh;
        fn as_compat(&self) -> CompatArhRef;
    }
}

pub trait ArhAccess: private::ArhAccessPrivate + Any {}

impl<A: ArhAccess + BinRead> ArhFileSystem<A>
where
    for<'a> <A as BinRead>::Args<'a>: Default,
{
    pub fn load(reader: impl Read + Seek) -> Result<Self> {
        Self::load_with_options(reader, ArhOptions::default())
    }

    pub fn load_with_options(mut reader: impl Read + Seek, options: ArhOptions) -> Result<Self> {
        let mut arh = A::read_le(&mut reader)?;
        arh.post_read();
        Ok(Self {
            dir_tree: arh.init_dir_tree(&options),
            opts: options,
            arh_new: arh,
        })
    }

    pub fn into_compat(self) -> ArhCompatFileSystem
    where
        A: Send + Sync + 'static,
    {
        ArhFileSystem {
            dir_tree: self.dir_tree,
            opts: self.opts,
            arh_new: Box::new(self.arh_new),
        }
    }
}

impl ArhFileSystem<CompatArhDyn> {
    pub fn load(reader: impl Read + Seek) -> Result<Self> {
        Self::load_with_options(reader, ArhOptions::default())
    }

    pub fn load_with_options(mut reader: impl Read + Seek, options: ArhOptions) -> Result<Self> {
        let mut arh = CompatArh::read_le(&mut reader)?.erase();
        arh.post_read();
        Ok(Self {
            dir_tree: arh.init_dir_tree(&options),
            opts: options,
            arh_new: arh,
        })
    }

    pub fn into_v1(self) -> std::result::Result<Arh1FileSystem, Self> {
        self.into_versioned::<Arh1>()
    }

    pub fn into_v2(self) -> std::result::Result<Arh2FileSystem, Self> {
        self.into_versioned::<Arh2>()
    }

    pub fn is_v1(&self) -> bool {
        self.is_versioned::<Arh1>()
    }

    pub fn is_v2(&self) -> bool {
        self.is_versioned::<Arh2>()
    }

    fn is_versioned<A: ArhAccess>(&self) -> bool {
        let arh_ref = &*self.arh_new as &dyn Any;
        arh_ref.is::<A>()
    }

    fn into_versioned<A: ArhAccess + 'static>(self) -> std::result::Result<ArhFileSystem<A>, Self> {
        let arh_new_ref = &*self.arh_new as &dyn Any;
        if arh_new_ref.is::<A>() {
            let arh_new = self.arh_new as Box<dyn Any>;
            Ok(ArhFileSystem {
                dir_tree: self.dir_tree,
                opts: self.opts,
                arh_new: *arh_new.downcast::<A>().unwrap(),
            })
        } else {
            Err(self)
        }
    }
}

impl<A: ArhAccess> ArhFileSystem<A> {
    /// Returns the size of a single block, in bytes.
    ///
    /// This can be changed by loading the file system using [`Self::load_with_options`].
    pub fn block_size(&self) -> u32 {
        1 << self.opts.ext_block_size_pow
    }

    // Node queries

    pub fn is_file(&self, path: &ArhPath) -> bool {
        self.get_file_info(path).is_some()
    }

    pub fn is_dir(&self, path: &ArhPath) -> bool {
        self.get_dir(path).is_some()
    }

    pub fn exists(&self, path: &ArhPath) -> bool {
        self.is_dir(path) || self.is_file(path)
    }

    pub fn get_file_info(&self, path: &ArhPath) -> Option<FileEntry> {
        self.arh_new.get_file_entry(path)
    }

    pub fn get_dir(&self, path: &ArhPath) -> Option<&DirNode> {
        if path.is_empty() {
            return None;
        }
        let parts = path.split('/').collect::<Vec<_>>();
        let mut node = &self.dir_tree;
        for part in &parts[1..] {
            if part.is_empty() {
                // Ignore leading, trailing, and adjacent slashes
                continue;
            }
            let DirEntry::Directory { ref children } = node.entry else {
                return None;
            };

            let child = children
                .binary_search_by_key(part, |c| c.name.as_str())
                .ok()?;
            node = &children[child]
        }
        matches!(node.entry, DirEntry::Directory { .. }).then_some(node)
    }

    // Structural modifications

    pub fn create_file(&mut self, full_path: &ArhPath) -> Result<FileEntry> {
        if self.get_file_info(full_path).is_some() {
            return Err(Error::FsAlreadyExists);
        }
        let entry = self.arh_new.create_file(full_path)?;
        self.dir_tree.insert_file_entry(full_path.to_string());
        Ok(entry)
    }

    pub fn delete_file(&mut self, path: &ArhPath) -> Result<()> {
        self.arh_new.delete_file(path, &self.opts)?;
        self.dir_tree.remove_file_entry(path);
        Ok(())
    }

    pub fn set_hidden_flag(&mut self, path: &ArhPath, hidden: bool) -> Result<()> {
        self.arh_new.hide_file(path, hidden)
    }

    /// Deletes an empty directory.
    ///
    /// This only updates the in-memory directory tree, it has no effect on the underlying
    /// file system, as the ARH format has no concept of directories.
    pub fn delete_empty_dir(&mut self, path: &ArhPath) -> Result<()> {
        self.dir_tree.remove_empty_dir(path);
        Ok(())
    }

    /// Renames a file. This also supports moving across directories.
    ///
    /// No data in the ARD file has to actually be moved, this operation only affects the file
    /// system.
    ///
    /// This operation is atomic. If it fails, the file system will be in the same (visible)
    /// state as before it was attempted.
    pub fn rename_file(&mut self, path: &ArhPath, new_path: &ArhPath) -> Result<()> {
        self.arh_new
            .rename_file(path, new_path, &self.opts, &mut self.dir_tree)
    }

    /// Renames a directory, recursively moving its children.
    ///
    /// No data in the ARD file has to actually be moved, this operation only affects the file
    /// system.
    pub fn rename_dir(&mut self, path: &ArhPath, new_path: &ArhPath) -> Result<()> {
        let dir = self.get_dir(path).ok_or(Error::FsNoEntry)?;
        let relative_paths = dir.children_paths();
        for (i, child) in relative_paths.iter().enumerate() {
            let child = &child[1..];
            if let Err(e) = self.rename_file(&path.join(child), &new_path.join(child)) {
                // Attempt rollback and panic if any operation fails.
                // This is currently implemented by renaming back the files for which the operation
                // succeeded. Another possibility is to save the state of the file system before
                // the operation.
                for child in &relative_paths[..i] {
                    self.rename_file(&new_path.join(child), &path.join(child))
                        .unwrap();
                }
                return Err(e);
            }
        }
        self.dir_tree.remove_empty_dir(path);
        Ok(())
    }
}

impl<A: ArhAccess + BinWrite> ArhFileSystem<A>
where
    for<'a> <A as BinWrite>::Args<'a>: Default,
{
    /// Writes the updated version of the ARH file system to the given writer.
    pub fn sync(&mut self, mut writer: impl Write + Seek) -> Result<()> {
        self.arh_new.prepare_for_write();
        self.arh_new.write_le(&mut writer)?;
        Ok(())
    }
}

impl ArhFileSystem<CompatArhDyn> {
    /// Writes the updated version of the ARH file system to the given writer.
    pub fn sync(&mut self, mut writer: impl Write + Seek) -> Result<()> {
        self.arh_new.prepare_for_write();
        self.arh_new.as_compat().write_le(&mut writer)?;
        Ok(())
    }
}

impl FileEntry {
    /// Returns the file's size after being extracted from the archive.
    ///
    /// For files that are stored uncompressed, the game expects `expanded_size` to be None,
    /// which can be confusing. This method always returns a non-zero size. (except for actually
    /// empty files)
    pub fn actual_size(&self) -> u64 {
        if let Some(exp_size) = self.expanded_size {
            exp_size.into()
        } else {
            self.ard_size
        }
    }

    pub fn is_flag(&self, flag: FileFlag) -> bool {
        match flag {
            FileFlag::Hidden => self.hidden,
            FileFlag::HasXbc1Header => self.xbc1_header,
        }
    }
}

impl DirNode {
    /// Returns the paths of all files and subdirectories (and their children), relative to
    /// this directory node.
    ///
    /// Paths start with a '/' character.
    pub fn children_paths(&self) -> Vec<String> {
        let children = match &self.entry {
            DirEntry::File => return vec![self.name.clone()],
            DirEntry::Directory { children } => children,
        };
        let mut paths = Vec::new();
        let mut stack = VecDeque::new();
        for child in children {
            stack.push_back((child, "".to_string()));
        }

        while let Some((node, path)) = stack.pop_back() {
            match &node.entry {
                DirEntry::File => {
                    paths.push(format!("{path}/{}", node.name));
                }
                DirEntry::Directory { children } => {
                    for child in children {
                        stack.push_back((child, format!("{path}/{}", node.name)));
                    }
                }
            }
        }

        paths
    }

    pub(crate) fn insert_file_entry(&mut self, path: String) {
        assert!(path.starts_with('/'), "path must start at the root");
        let mut node = self;
        let parts = path.split('/').collect::<Vec<_>>();
        for (comp_idx, comp) in parts[1..].iter().enumerate() {
            let next_node = {
                let DirEntry::Directory { ref mut children } = node.entry else {
                    continue;
                };
                match children.binary_search_by_key(comp, |c| &c.name) {
                    Ok(i) => {
                        // File/Subdirectory already present, proceed from there
                        &mut children[i]
                    }
                    Err(i) => {
                        // Need to create file or subdirectory
                        let dir_node = DirNode {
                            name: comp.to_string(),
                            entry: if comp_idx != parts.len() - 2 {
                                DirEntry::Directory {
                                    children: Vec::new(),
                                }
                            } else {
                                DirEntry::File
                            },
                        };
                        children.insert(i, dir_node);
                        &mut children[i]
                    }
                }
            };
            node = next_node;
        }
    }

    pub(crate) fn remove_file_entry(&mut self, path: &str) {
        assert!(path.starts_with('/'), "path must start at the root");
        let mut node = self;
        let parts = path.split('/').collect::<Vec<_>>();
        for comp in &parts[1..] {
            let next_node = {
                let DirEntry::Directory { ref mut children } = node.entry else {
                    continue;
                };
                if let Ok(i) = children.binary_search_by_key(comp, |c| &c.name) {
                    let child = &mut children[i];
                    if matches!(child.entry, DirEntry::File) {
                        children.remove(i);
                        break;
                    } else {
                        &mut children[i]
                    }
                } else {
                    break;
                }
            };
            node = next_node;
        }
    }

    fn remove_empty_dir(&mut self, path: &str) {
        assert!(path.starts_with('/'), "path must start at the root");
        let parts = path.split('/').collect::<Vec<_>>();
        let mut node = self;

        for (comp_idx, comp) in parts[1..].iter().enumerate() {
            let next_node = {
                let DirEntry::Directory { ref mut children } = node.entry else {
                    continue;
                };
                if let Ok(i) = children.binary_search_by_key(comp, |c| &c.name) {
                    if comp_idx == parts.len() - 2 {
                        children.remove(i);
                        return;
                    }
                    &mut children[i]
                } else {
                    return;
                }
            };
            node = next_node;
        }
    }
}
