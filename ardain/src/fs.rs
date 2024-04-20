use std::{
    collections::VecDeque,
    io::{Read, Seek, Write},
};

use binrw::{BinRead, BinResult, BinWrite};

use crate::{
    arh::{Arh, DictNode, FileMeta},
    arh_ext::ArhExtSection,
    error::{Error, Result},
    opts::ArhOptions,
    path::ArhPath,
};

pub struct ArhFileSystem {
    pub(crate) arh: Arh,
    pub(crate) opts: ArhOptions,
    // Not part of the ARH format, but we keep one to make enumerating and traversing directories
    // easier.
    dir_tree: DirNode,
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

impl ArhFileSystem {
    pub fn load(reader: impl Read + Seek) -> BinResult<Self> {
        Self::load_with_options(reader, ArhOptions::default())
    }

    pub fn load_with_options(mut reader: impl Read + Seek, options: ArhOptions) -> BinResult<Self> {
        let arh = Arh::read(&mut reader)?;
        Ok(Self {
            dir_tree: DirNode::build(&arh),
            opts: options,
            arh,
        })
    }

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

    pub fn get_file_info(&self, path: &ArhPath) -> Option<&FileMeta> {
        self.get_file_id(path)
            .and_then(|(id, _)| self.arh.file_table.get_meta(id))
    }

    pub fn get_file_info_mut(&mut self, path: &ArhPath) -> Option<&mut FileMeta> {
        self.get_file_id(path)
            .and_then(|(id, _)| self.arh.file_table.get_meta_mut(id))
    }

    pub fn get_dir(&self, path: &ArhPath) -> Option<&DirNode> {
        if path.is_empty() {
            return None;
        }
        let parts = path.split("/").collect::<Vec<_>>();
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

    /// Returns the file ID and leaf node ID for the given path.
    fn get_file_id(&self, path: &ArhPath) -> Option<(u32, i32)> {
        let nodes = &self.arh.path_dictionary();
        let mut cur = (0, nodes.node(0));
        let mut path = path.as_str();

        while !cur.1.is_leaf() {
            if path.is_empty() {
                // If we've consumed the whole path, the file exists iff there are no more
                // nodes to be visited.
                if cur.1.is_child(cur.0) {
                    break;
                }
                return None;
            }
            let next_id = cur.1.next_after_chr(path.as_bytes()[0]);
            let next = nodes.get_node(next_id)?;
            if !next.is_child(cur.0) {
                return None;
            }
            cur = (next_id, next);
            path = &path[1..];
        }
        let DictNode::Leaf { string_offset, .. } = *cur.1 else {
            return None;
        };
        let (remaining, file_id) = self.arh.strings().get_str_part_id(string_offset as usize);

        (remaining == path).then_some((file_id, cur.0))
    }

    // Structural modifications

    pub fn create_file(&mut self, full_path: &ArhPath) -> Result<&mut FileMeta> {
        if self.get_file_info(full_path).is_some() {
            return Err(Error::FsAlreadyExists);
        }

        // Follow existing paths
        let (last, mut last_parent, mut path) = {
            let nodes = &self.arh.path_dictionary().nodes;
            let mut cur = (0, &nodes[0]);
            let mut path = full_path.as_str();
            let mut last_parent = 0;

            while !cur.1.is_leaf() {
                if path.is_empty() {
                    unreachable!("should have been reported as existing file");
                }
                let next = cur.1.next_after_chr(path.as_bytes()[0]) as usize;
                if !nodes[next].is_child(cur.0 as i32) {
                    break;
                }
                last_parent = cur.0;
                cur = (next, &nodes[next]);
                path = &path[1..];
            }
            ((cur.0 as i32, *cur.1), last_parent as i32, path)
        };

        let mut final_node = last;

        if let DictNode::Leaf {
            string_offset,
            previous,
        } = final_node.1
        {
            // If the final common node is a leaf, we need to split the path.
            // Example: (-> denotes a XOR path)
            // "text.txt" (t->e->-x->"t.txt")
            // "text1.txt" (t->e->x->"???")
            // Expected result:
            // "text.txt" (t->-e->-x->t->".txt")
            // "text1.txt" (t->-e->-x->t->"1.txt")

            let (old_str, old_file) = self.arh.strings().get_str_part_id(string_offset as usize);
            let old_str = old_str.to_string();
            let mut old_str = old_str.as_str();
            let mut node_block = self.arh.path_dictionary().node(previous).next();
            let mut last = final_node.0 as i32;
            // We take a clone here because some branches might fail, and failure is only detected
            // after modifying part of it. We correctly throw errors but we don't want to leave
            // the file system in an inconsistent state.
            let mut path_dict = self.arh.path_dictionary().clone();

            while !path.is_empty()
                && !old_str.is_empty()
                && old_str.as_bytes()[0] == path.as_bytes()[0]
            {
                // Continue the XOR path while characters match
                let chr = old_str.as_bytes()[0] as i32;
                let node_idx = node_block ^ chr;
                let next_node = path_dict.node(node_idx);
                let next;
                if next_node.is_free() {
                    // Next node is free, occupy it
                    next = node_idx;
                    *path_dict.node_mut(next) = DictNode::Occupied {
                        previous: last,
                        next: 0xFEFE,
                    };
                    path_dict.node_mut(last).attach_next(node_block);
                } else {
                    // Otherwise, allocate a block
                    node_block = path_dict.allocate_new_block(last as i32) as i32;
                    next = node_block ^ path.as_bytes()[0] as i32;
                    *path_dict.node_mut(next) = DictNode::Occupied {
                        previous: last,
                        next: 0xBADD,
                    };
                }
                last = next as i32;
                old_str = &old_str[1..];
                path = &path[1..];
            }

            if path.is_empty() || old_str.is_empty() {
                return Err(Error::FsFileNameExtended);
            }

            // Found a level where the two strings differ. Make a block for them, copy the leaf node
            // to it and pass it on.
            let next_block = path_dict.allocate_new_block(last);
            path_dict.node_mut(last).attach_next(next_block as i32);

            let id = self.arh.strings_mut().push(&old_str[1..], old_file);
            let idx = next_block ^ old_str.as_bytes()[0] as i32;
            *path_dict.node_mut(idx) = DictNode::Leaf {
                previous: last,
                string_offset: id,
            };

            let final_idx = next_block ^ path.as_bytes()[0] as i32;
            final_node = (final_idx, *path_dict.node(final_idx));
            last_parent = last;
            path = &path[1..];

            *self.arh.path_dictionary_mut() = path_dict;
        }

        // We need to diverge from the existing path. If the next expected node is free,
        // we occupy it with the rest of the name. Otherwise, we must move the previous node
        // alongside all its children to a new location that lets us add the new node.
        if !final_node.1.is_free() {
            let idx = self
                .arh
                .path_dictionary_mut()
                .allocate_new_block(final_node.0 as i32)
                ^ path.as_bytes()[0] as i32;
            last_parent = final_node.0;
            final_node = (idx, *self.arh.path_dictionary().node(idx));
            path = &path[1..];
        }

        // `final_node` is now a free node.

        let Arh {
            file_table,
            arh_ext_section,
            ..
        } = &mut self.arh;
        let id = file_table.push_entry(
            FileMeta::new_invalid(),
            arh_ext_section.as_mut().map(ArhExtSection::recycle_bin_mut),
        );
        let str_offset = self.arh.strings_mut().push(path, id);
        *self.arh.path_dictionary_mut().node_mut(final_node.0) = DictNode::Leaf {
            previous: last_parent as i32,
            string_offset: str_offset,
        };

        // Update directory tree
        self.dir_tree.insert_file_entry(full_path.to_string());
        Ok(self.arh.file_table.get_meta_mut(id).unwrap())
    }

    pub fn delete_file(&mut self, path: &ArhPath) -> Result<()> {
        let (file_id, leaf_id) = self.get_file_id(path).ok_or(Error::FsNoEntry)?;

        // Probably not optimal (we potentially leave unused nodes dangling),
        // but we can just free the leaf node
        *self.arh.path_dictionary_mut().node_mut(leaf_id) = DictNode::Free;

        // For the file entry, it's not as simple as it looks. While FileMeta has an ID field,
        // the game actually indexes into the file table instead of filtering by that field.
        // Because there is no longer a leaf pointing to that file node, we can zero out its
        // contents, and recycle it later.
        let file = self.arh.file_table.delete_entry(file_id).unwrap();
        let ext = self.arh.get_or_init_ext(&self.opts);
        ext.allocated_blocks.mark(&file, false);
        ext.file_meta_recycle_bin.push(file_id);

        // Update directory tree
        self.dir_tree.remove_file_entry(path);
        Ok(())
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
        let meta = self.get_file_info(path).copied().ok_or(Error::FsNoEntry)?;
        // We need to delete the file first, because the new name might be in conflict with the old
        // file's name. For instance, some file managers first create a ".part" file which they then
        // rename to the regular file name without ".part". This type of file names is not supported
        // by the file system.
        self.delete_file(path)?;
        let new_file = match self.create_file(new_path) {
            Ok(f) => f,
            Err(e) => {
                // Re-create the old file if creating the new one fails.
                // This shouldn't fail as we just deleted it.
                self.create_file(path).unwrap().clone_from(&meta);
                return Err(e);
            }
        };
        new_file.clone_from(&meta);
        Ok(())
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

    /// Writes the updated version of the ARH file system to the given writer.
    pub fn sync(&mut self, mut writer: impl Write + Seek) -> Result<()> {
        self.arh.prepare_for_write();
        Ok(self.arh.write(&mut writer)?)
    }
}

impl DirNode {
    fn build(arh: &Arh) -> Self {
        let mut start = DirNode {
            name: "/".to_string(),
            entry: DirEntry::Directory {
                children: Vec::new(),
            },
        };
        for (idx, node) in arh.path_dictionary().nodes.iter().enumerate() {
            if !node.is_leaf() {
                continue;
            }
            start.insert_file_entry(arh.path_dictionary().get_full_path(idx, arh.strings()));
        }

        start
    }

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

    fn insert_file_entry(&mut self, path: String) {
        assert!(path.starts_with("/"), "path must start at the root");
        let mut node = self;
        let parts = path.split("/").collect::<Vec<_>>();
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

    fn remove_file_entry(&mut self, path: &str) {
        assert!(path.starts_with("/"), "path must start at the root");
        let parts = path.split("/").collect::<Vec<_>>();

        fn delete_node(node: &mut DirNode, parts: &[&str]) -> bool {
            let Some(part) = parts.first() else {
                return true;
            };
            if let DirEntry::Directory { ref mut children } = node.entry {
                if let Ok(i) = children.binary_search_by_key(part, |c| &c.name) {
                    let child = &mut children[i];
                    if matches!(child.entry, DirEntry::File) {
                        children.remove(i);
                    } else {
                        if !delete_node(&mut children[i], &parts[1..]) {
                            // Remove empty directories
                            //children.remove(i);
                        }
                    }
                    if children.is_empty() {
                        return false;
                    }
                }
            }
            true
        }

        delete_node(self, &parts[1..]);
    }

    fn remove_empty_dir(&mut self, path: &str) {
        assert!(path.starts_with("/"), "path must start at the root");
        let parts = path.split("/").collect::<Vec<_>>();
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
