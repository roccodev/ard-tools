use std::io::{Read, Seek};

use binrw::{BinRead, BinResult};

use crate::{
    arh::{Arh, DictNode, FileMeta},
    error::{Error, Result},
};

pub struct ArhFileSystem {
    arh: Arh,
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
    pub fn load(mut reader: impl Read + Seek) -> BinResult<Self> {
        let arh = Arh::read(&mut reader)?;
        Ok(Self {
            dir_tree: DirNode::build(&arh),
            arh,
        })
    }

    // Node queries

    pub fn exists(&self, path: &str) -> bool {
        self.get_file_info(path).is_some()
    }

    pub fn get_file_info(&self, path: &str) -> Option<&FileMeta> {
        self.get_file_id(path)
            .and_then(|id| self.arh.file_table.get_meta(id))
    }

    pub fn get_file_info_mut(&mut self, path: &str) -> Option<&mut FileMeta> {
        self.get_file_id(path)
            .and_then(|id| self.arh.file_table.get_meta_mut(id))
    }

    pub fn get_dir(&self, path: &str) -> Option<&DirNode> {
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

    fn get_file_id(&self, mut path: &str) -> Option<u32> {
        let nodes = &self.arh.path_dictionary().nodes;
        let mut cur = (0usize, &nodes[0]);

        while !cur.1.is_leaf() {
            if path.is_empty() {
                // If we've consumed the whole path, the file exists iff there are no more
                // nodes to be visited.
                if cur.1.is_child(cur.0 as i32) {
                    break;
                }
                return None;
            }
            println!("Visiting {cur:?} {path:?}");
            let next = cur.1.next_after_chr(path.as_bytes()[0]) as usize;
            if !nodes[next].is_child(cur.0 as i32) {
                println!("NAC ({next}, {:?}) {cur:?}", nodes[next]);
                return None;
            }
            cur = (next, &nodes[next]);
            path = &path[1..];
        }
        println!("Ended at {cur:?}");
        let DictNode::Leaf { string_offset, .. } = *cur.1 else {
            return None;
        };
        let (remaining, file_id) = self.arh.strings().get_str_part_id(string_offset as usize);
        println!("STRCK: {remaining:?} {path:?}");

        (remaining == path).then_some(file_id)
    }

    // Structural modifications

    pub fn create_file(&mut self, full_path: &str) -> Result<&mut FileMeta> {
        if self.get_file_info(full_path).is_some() {
            return Err(Error::FsAlreadyExists);
        }

        // Follow existing paths
        let (last, mut last_parent, mut path) = {
            let nodes = &self.arh.path_dictionary().nodes;
            let mut cur = (0usize, &nodes[0]);
            let mut path = full_path;
            let mut last_parent = 0;

            while !cur.1.is_leaf() {
                if path.is_empty() {
                    unreachable!("should have been reported as existing file");
                }
                let next = cur.1.next_after_chr(path.as_bytes()[0]) as usize;
                if !nodes[next].is_child(cur.0 as i32) {
                    println!("Detected break {path:?} {cur:?} {next} {:?}", nodes[next]);
                    break;
                }
                last_parent = cur.0;
                cur = (next, &nodes[next]);
                path = &path[1..];
            }
            println!("Exited {path:?} CUR: {cur:?} LP: {last_parent}");
            ((cur.0, *cur.1), last_parent, path)
        };

        // We need to diverge from the existing path. If the next expected node is free,
        // we occupy it with the rest of the name. Otherwise, we must move the previous node
        // alongside all its children to a new location that lets us add the new node.
        let mut final_node = last;

        if !final_node.1.is_free() {
            let idx = self
                .arh
                .path_dictionary_mut()
                .allocate_new_block(final_node.0 as i32)
                ^ path.as_bytes()[0] as usize;
            println!("Final node not free, allocated {idx}");
            last_parent = final_node.0;
            final_node = (idx, self.arh.path_dictionary().nodes[idx as usize]);
            path = &path[1..];
        }

        // `final_node` is now a free node.

        let meta = FileMeta {
            offset: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            _unk: 0,
            id: 0,
        };
        let id = self.arh.file_table.push_entry(meta);
        let str_offset = self.arh.strings_mut().push(path, id);
        println!("{final_node:?} <= Leaf {last_parent} -{str_offset}");
        self.arh.path_dictionary_mut().nodes[final_node.0] = DictNode::Leaf {
            previous: last_parent as i32,
            string_offset: str_offset,
        };

        // Update directory tree
        self.dir_tree.insert_file_entry(full_path.to_string());
        Ok(self.arh.file_table.get_meta_mut(id).unwrap())
    }

    pub fn delete(&mut self, path: &str) -> Result<()> {
        todo!()
    }

    pub fn rename(&mut self, path: &str, new_path: &str) -> Result<()> {
        let meta = self.get_file_info(path).copied().ok_or(Error::FsNoEntry)?;
        self.delete(path)?;
        self.create_file(new_path)?.clone_from(&meta);
        Ok(())
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

    fn insert_file_entry(&mut self, path: String) {
        assert!(path.starts_with("/"), "path must start at the root");
        let mut node = self;
        let parts = path.split("/").collect::<Vec<_>>();
        for (comp_idx, comp) in parts[1..].iter().enumerate() {
            let next_node = {
                let DirEntry::Directory { ref mut children } = node.entry else {
                    continue;
                };
                let name = comp.to_string();
                match children.binary_search_by_key(&&name, |c| &c.name) {
                    Ok(i) => {
                        // File/Subdirectory already present, proceed from there
                        &mut children[i]
                    }
                    Err(i) => {
                        // Need to create file or subdirectory
                        let dir_node = DirNode {
                            name,
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
}
