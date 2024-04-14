use std::io::{Read, Seek};

use binrw::{BinRead, BinResult};

use crate::{
    arh::{Arh, FileMeta},
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

        while cur.1.next >= 0 {
            if path.is_empty() {
                // If we've consumed the whole path, the file exists iff there are no more
                // nodes to be visited.
                if cur.0 as i32 == cur.1.prev {
                    break;
                }
                return None;
            }
            let next = (cur.1.next ^ path.as_bytes()[0] as i32) as usize;
            if nodes[next].prev != cur.0 as i32 {
                return None;
            }
            cur = (next, &nodes[next]);
            path = &path[1..];
            println!("{path:?} {cur:?}");
        }
        let (remaining, file_id) = self.arh.strings().get_str_part_id(-cur.1.next as usize);
        println!("{remaining:?} {path:?} {cur:?}");
        (remaining == path).then_some(file_id)
    }

    // Structural modifications

    pub fn create_file(&mut self, full_path: &str) -> Result<&mut FileMeta> {
        if self.get_file_info(full_path).is_some() {
            return Err(Error::FsAlreadyExists);
        }

        // Follow existing paths
        let (last, mut path, split) = {
            let nodes = &self.arh.path_dictionary().nodes;
            let mut cur = (0usize, &nodes[0]);
            let mut path = full_path;
            let mut split = false;

            while cur.1.next >= 0 {
                if path.is_empty() {
                    unreachable!("should have been reported as existing file");
                }
                let next = (cur.1.next ^ path.as_bytes()[0] as i32) as usize;
                if nodes[next].prev != cur.0 as i32 {
                    println!("Detected break {path:?} {cur:?} {next} {:?}", nodes[next]);
                    break;
                }
                cur = (next, &nodes[next]);
                path = &path[1..];
            }
            println!("Exited {path:?}");
            ((cur.0, *cur.1), path, split)
        };

        let meta = FileMeta {
            offset: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            _unk: 0,
            id: 0,
        };
        let id = self.arh.file_table.push_entry(meta);
        let mut node = last.0;
        println!("LAST: {last:?}");
        if last.1.next < 0 {
            // Need to make new entries for as long as the new path matches the old one
            let last_id = last.1.next;
            let mut last = last.0;
            let (old_str, old_id) = self.arh.strings().get_str_part_id(-last_id as usize);
            let old_str = old_str.to_string();
            let mut old_str = old_str.as_str();

            let dict = &mut self.arh.path_dictionary_mut();
            while !path.is_empty()
                && !old_str.is_empty()
                && old_str.as_bytes()[0] == path.as_bytes()[0]
            {
                let next_block = dict.allocate_new_block();
                println!(
                    "Allocated {next_block} for {}",
                    path.chars().next().unwrap()
                );
                let next = next_block ^ path.as_bytes()[0] as usize;
                let next_node = &mut dict.nodes[next];
                next_node.prev = node as i32;
                dict.nodes[node].next = next_block as i32;
                last = node;
                node = next;
                old_str = &old_str[1..];
                path = &path[1..];
            }
            println!(
                "Last is now {last} {node} ({}) ({})",
                self.arh.path_dictionary().nodes[last].next,
                self.arh.path_dictionary().nodes[node].next
            );
            let final_block = self.arh.path_dictionary_mut().allocate_new_block();
            if !old_str.is_empty() {
                // e.g. "file.txt1" (old), "file.txt2" (new)
                // they have "file.txt" in common, then we need to branch on "1" and "2".
                // "2" is taken care of by the rest of the function, we need to create an entry
                // for "1"
                let id = self.arh.strings_mut().push(&old_str[1..], old_id);
                let nodes = &mut self.arh.path_dictionary_mut().nodes;
                let idx = final_block ^ old_str.as_bytes()[0] as usize;
                nodes[idx].next = -id;
                nodes[idx].prev = node as i32;
                println!("ADDED next -{id} @ {idx}");
            }
            self.arh.path_dictionary_mut().nodes[node].next = final_block as i32;
            println!(
                "PATH: {path:?} OLD: {old_str:?} {last} ({}) {node} ({})",
                self.arh.path_dictionary().nodes[last].next,
                self.arh.path_dictionary().nodes[node].next,
            );
            let idx = final_block ^ path.as_bytes()[0] as usize;
            self.arh.path_dictionary_mut().nodes[idx].prev = node as i32;
            node = idx;
            path = &path[1..];
        }
        //let next = self.arh.path_dictionary_mut().allocate_new_block();
        //self.arh.path_dictionary_mut().nodes[node].next = next;
        println!(
            "Mknod {id} {path} {node:?} {:?}",
            self.arh.path_dictionary().nodes[node]
        );
        let str_offset = self.arh.strings_mut().push(path, id);
        //let idx = node ^ path.as_bytes()[0] as usize;
        self.arh.path_dictionary_mut().nodes[node].next = -str_offset;
        println!("{node}.next <= -{str_offset}");
        //self.arh.path_dictionary_mut().nodes[node].prev = -str_offset;

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
            if node.next >= 0 || node.prev < 0 {
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
