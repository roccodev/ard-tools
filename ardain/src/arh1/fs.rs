use std::num::NonZeroU64;

use crate::{
    arh1::ext::ArhExtSection,
    compat::{CompatArh, CompatArhRef},
    error::{Error, Result},
    opts::ArhOptions,
    path::ArhPath,
    private::ArhAccessPrivate,
    ArhAccess, DirEntry, DirNode, FileEntry,
};

use super::{Arh1, Arh1Entry, DictNode, FileFlag};

impl Arh1 {
    fn get_file_id(&self, path: &ArhPath) -> Option<(u32, i32)> {
        let nodes = &self.path_dictionary();
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
        let (remaining, file_id) = self.strings().get_str_part_id(string_offset as usize);

        (remaining == path).then_some((file_id.into(), cur.0))
    }

    fn internal_create_file(&mut self, path: &ArhPath) -> Result<&mut Arh1Entry> {
        if self.get_file_entry(path).is_some() {
            return Err(Error::FsAlreadyExists);
        }

        // Follow existing paths
        let (last, mut last_parent, mut path) = {
            let nodes = &self.path_dictionary().nodes;
            let mut cur = (0, &nodes[0]);
            let mut path = path.as_str();
            let mut last_parent = 0;

            while !cur.1.is_leaf() {
                if path.is_empty() {
                    // Whole `path` consumed but there are still nodes to traverse.
                    // This means that there is another file with a name that extends it.
                    return Err(Error::FsFileNameExtended);
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

            let (old_str, old_file) = self.strings().get_str_part_id(string_offset as usize);
            let old_str = old_str.to_string();
            let mut old_str = old_str.as_str();
            let mut node_block = self.path_dictionary().node(previous).next();
            let mut last = final_node.0;
            // We take a clone here because some branches might fail, and failure is only detected
            // after modifying part of it. We correctly throw errors but we don't want to leave
            // the file system in an inconsistent state.
            let mut path_dict = self.path_dictionary().clone();

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
                    node_block = path_dict.allocate_new_block(last);
                    next = node_block ^ path.as_bytes()[0] as i32;
                    *path_dict.node_mut(next) = DictNode::Occupied {
                        previous: last,
                        next: 0xBADD,
                    };
                }
                last = next;
                old_str = &old_str[1..];
                path = &path[1..];
            }

            if path.is_empty() || old_str.is_empty() {
                return Err(Error::FsFileNameExtended);
            }

            // Found a level where the two strings differ. Make a block for them, copy the leaf node
            // to it and pass it on.
            let next_block = path_dict.allocate_new_block(last);
            path_dict.node_mut(last).attach_next(next_block);

            let id = self.strings_mut().push(&old_str[1..], old_file);
            let idx = next_block ^ old_str.as_bytes()[0] as i32;
            *path_dict.node_mut(idx) = DictNode::Leaf {
                previous: last,
                string_offset: id,
            };

            let final_idx = next_block ^ path.as_bytes()[0] as i32;
            final_node = (final_idx, *path_dict.node(final_idx));
            last_parent = last;
            path = &path[1..];

            *self.path_dictionary_mut() = path_dict;
        }

        // We need to diverge from the existing path. If the next expected node is free,
        // we occupy it with the rest of the name. Otherwise, we must move the previous node
        // alongside all its children to a new location that lets us add the new node.
        if !final_node.1.is_free() {
            let idx = self.path_dictionary_mut().allocate_new_block(final_node.0)
                ^ path.as_bytes()[0] as i32;
            last_parent = final_node.0;
            final_node = (idx, *self.path_dictionary().node(idx));
            path = &path[1..];
        }

        // `final_node` is now a free node.
        let id = self.file_table.push_entry(
            Arh1Entry::new_invalid(),
            self.arh_ext_section
                .as_mut()
                .map(ArhExtSection::recycle_bin_mut),
        );
        let str_offset = self.strings_mut().push(path, id);
        *self.path_dictionary_mut().node_mut(final_node.0) = DictNode::Leaf {
            previous: last_parent,
            string_offset: str_offset,
        };

        Ok(self.file_table.get_meta_mut(id).unwrap())
    }
}

impl ArhAccessPrivate for Arh1 {
    fn post_read(&mut self) {}

    fn get_file_entry(&self, path: &ArhPath) -> Option<FileEntry> {
        self.get_file_id(path)
            .and_then(|id| self.file_table.get_meta(id.0))
            .copied()
            .map(Into::into)
    }

    fn init_dir_tree(&self, opts: &ArhOptions) -> DirNode {
        let mut start = DirNode {
            name: "/".to_string(),
            entry: DirEntry::Directory {
                children: Vec::new(),
            },
        };
        for (idx, node) in self.path_dictionary().nodes.iter().enumerate() {
            if !node.is_leaf() {
                continue;
            }
            start.insert_file_entry(self.path_dictionary().get_full_path(idx, self.strings()));
        }

        start
    }

    fn create_file(&mut self, path: &ArhPath) -> Result<FileEntry> {
        self.internal_create_file(path).copied().map(Into::into)
    }

    fn delete_file(&mut self, path: &ArhPath, opts: &ArhOptions) -> Result<()> {
        let (file_id, leaf_id) = self.get_file_id(path).ok_or(Error::FsNoEntry)?;

        // We must recursively free nodes. Consider this scenario:
        // Files "ab", "ac", "ad" are created, then removed. If nodes are not freed
        // recursively, then file "a" cannot be created because the common node was not freed
        self.path_dictionary_mut().free_node_recursive(leaf_id);

        // For the file entry, it's not as simple as it looks. While FileMeta has an ID field,
        // the game actually indexes into the file table instead of filtering by that field.
        // Because there is no longer a leaf pointing to that file node, we can zero out its
        // contents, and recycle it later.
        let file = self.file_table.delete_entry(file_id).unwrap();
        let ext = self.get_or_init_ext(opts);
        ext.allocated_blocks.mark(&file, false);
        ext.file_meta_recycle_bin.push(file_id);
        Ok(())
    }

    fn hide_file(&mut self, path: &ArhPath, hidden: bool) -> Result<()> {
        let (file_id, _) = self.get_file_id(path).ok_or(Error::FsNoEntry)?;
        self.file_table
            .get_meta_mut(file_id)
            .ok_or(Error::FsNoEntry)?
            .set_flag(FileFlag::Hidden, hidden);
        Ok(())
    }

    fn rename_file(
        &mut self,
        from: &ArhPath,
        to: &ArhPath,
        opts: &ArhOptions,
        dir_tree: &mut DirNode,
    ) -> Result<()> {
        let meta = self
            .get_file_id(from)
            .and_then(|id| self.file_table.get_meta(id.0))
            .copied()
            .ok_or(Error::FsNoEntry)?;
        // We need to delete the file first, because the new name might be in conflict with the old
        // file's name. For instance, some file managers first create a ".part" file which they then
        // rename to the regular file name without ".part". This type of file names is not supported
        // by the file system.
        self.delete_file(from, opts)?;
        dir_tree.remove_file_entry(from);
        let new_file = match self.internal_create_file(to) {
            Ok(f) => f,
            Err(e) => {
                // Re-create the old file if creating the new one fails.
                // This shouldn't fail as we just deleted it.
                self.internal_create_file(from).unwrap().clone_from(&meta);
                dir_tree.insert_file_entry(from.to_string());
                return Err(e);
            }
        };
        dir_tree.insert_file_entry(to.to_string());
        new_file.clone_from(&meta);
        Ok(())
    }

    fn prepare_for_write(&mut self) {
        self.prepare_write();
    }

    fn into_compat(self: Box<Self>) -> CompatArh {
        CompatArh::Arh1(*self)
    }

    fn as_compat(&self) -> CompatArhRef {
        CompatArhRef::Arh1(self)
    }
}

impl ArhAccess for Arh1 {}

impl From<Arh1Entry> for FileEntry {
    fn from(value: Arh1Entry) -> Self {
        Self {
            ard_offset: value.offset,
            ard_size: value.compressed_size.into(),
            expanded_size: NonZeroU64::new(value.uncompressed_size.into()),
            unique_id: value.id.into(),
            hidden: value.is_flag(FileFlag::Hidden),
            xbc1_header: value.is_flag(FileFlag::HasXbc1Header),
        }
    }
}
