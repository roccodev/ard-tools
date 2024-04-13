use std::io::{Read, Seek};

use binrw::{BinRead, BinResult};

use crate::arh::{Arh, FileMeta};

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

    pub fn exists(&self, path: &str) -> bool {
        self.get_file_info(path).is_some()
    }

    pub fn get_file_info(&self, mut path: &str) -> Option<&FileMeta> {
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
        }
        let (remaining, file_id) = self.arh.strings().get_str_part_id(-cur.1.next as usize);
        (remaining == path)
            .then_some(())
            .and_then(|_| self.arh.file_table.get_meta(file_id))
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
