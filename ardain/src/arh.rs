use std::{
    ffi::CStr,
    io::{self, Cursor, Read, Seek, SeekFrom},
    mem::size_of,
};

use binrw::{binread, BinRead};

const KEY_XOR: u32 = 0xF3F35353;

#[derive(Debug, PartialEq, Clone)]
#[binread]
#[brw(little, magic(b"arh1"))]
pub struct Arh {
    _str_table_len_dup: u32,
    _path_dict_rel_ptr: u32,
    offsets: ArhOffsets,
    key: u32,
    #[br(args { offsets, key })]
    encrypted: EncryptedSection,
    #[br(args { len: offsets.file_table_len }, seek_before = SeekFrom::Start(offsets.file_table_offset.into()))]
    pub file_table: FileTable,
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[binread]
struct ArhOffsets {
    str_table_offset: u32,
    str_table_len: u32,
    path_dict_offset: u32,
    path_dict_len: u32,
    file_table_offset: u32,
    file_table_len: u32,
}

#[derive(Debug, PartialEq, Clone)]
#[binread]
#[br(import {
    offsets: ArhOffsets,
    key: u32
})]
struct EncryptedSection {
    #[br(args { key, len: offsets.str_table_len }, seek_before = SeekFrom::Start(offsets.str_table_offset.into()))]
    string_table: StringTable,
    #[br(args { key, len: offsets.path_dict_len }, seek_before = SeekFrom::Start(offsets.path_dict_offset.into()))]
    path_dict: PathDictionary,
}

#[derive(Debug, PartialEq, Clone)]
#[binread]
#[br(import { len: u32, key: u32 })]
pub struct StringTable {
    #[br(args { count: len.try_into().unwrap() }, map_stream = |reader| EncryptedSection::decrypt(reader, len, key).expect("TODO"))]
    strings: Vec<u8>,
}

#[derive(Debug, PartialEq, Clone)]
#[binread]
#[br(import { len: u32, key: u32 })]
pub struct PathDictionary {
    #[br(args { count: usize::try_from(len).unwrap() / size_of::<RawDictNode>() }, map_stream = |reader| EncryptedSection::decrypt(reader, len, key).expect("TODO"))]
    pub nodes: Vec<DictNode>,
}

#[derive(Debug, PartialEq, Clone)]
#[binread]
#[br(import { len: u32 })]
pub struct FileTable {
    #[br(args { count: usize::try_from(len).unwrap() })]
    files: Vec<FileMeta>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[binread]
#[br(map = |raw: RawDictNode| raw.into())]
pub enum DictNode {
    /// Raw repr: previous < 0 and next < 0
    Free,
    /// Raw repr: previous < 0 and next >= 0
    Root { next: i32 },
    /// Raw repr: previous >= 0 and next >= 0
    Occupied { previous: i32, next: i32 },
    /// Raw repr: previous >= 0 and next < 0
    Leaf { previous: i32, string_offset: i32 },
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[binread]
pub struct RawDictNode {
    pub next: i32,
    pub prev: i32,
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[binread]
pub struct FileMeta {
    pub offset: u64,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub _unk: u32,
    pub id: u32,
}

impl Arh {
    pub fn strings(&self) -> &StringTable {
        &self.encrypted.string_table
    }

    pub fn strings_mut(&mut self) -> &mut StringTable {
        &mut self.encrypted.string_table
    }

    pub fn path_dictionary(&self) -> &PathDictionary {
        &self.encrypted.path_dict
    }

    pub fn path_dictionary_mut(&mut self) -> &mut PathDictionary {
        &mut self.encrypted.path_dict
    }
}

impl EncryptedSection {
    fn decrypt<S: Read + Seek>(
        mut stream: S,
        len: u32,
        mut key: u32,
    ) -> io::Result<Cursor<Vec<u8>>> {
        let mut buf = vec![0u8; len.try_into().unwrap()];
        stream.read_exact(&mut buf)?;
        key ^= KEY_XOR;
        if key != 0 {
            for chunk in buf.chunks_exact_mut(4) {
                let [a, b, c, d] = chunk else { unreachable!() };
                let [x_a, x_b, x_c, x_d] = key.to_le_bytes();
                *a ^= x_a;
                *b ^= x_b;
                *c ^= x_c;
                *d ^= x_d;
            }
        }
        Ok(Cursor::new(buf))
    }
}

impl StringTable {
    pub fn get_str_part_id(&self, mut offset: usize) -> (&str, u32) {
        let st = CStr::from_bytes_until_nul(&self.strings[offset..])
            .unwrap()
            .to_str()
            .unwrap();
        offset += st.len() + 1;
        (
            st,
            u32::read_le(&mut Cursor::new(&self.strings[offset..])).unwrap(),
        )
    }

    pub fn push(&mut self, text: &str, id: u32) -> i32 {
        let offset = self
            .strings
            .len()
            .try_into()
            .expect("max string table offset reached");
        self.strings.extend_from_slice(text.as_bytes());
        self.strings.push(0);
        self.strings.extend_from_slice(&id.to_le_bytes());
        offset
    }
}

impl PathDictionary {
    pub fn get_full_path(&self, mut node_idx: usize, strings: &StringTable) -> String {
        let mut node = &self.nodes[node_idx];

        let DictNode::Leaf { string_offset, .. } = *node else {
            panic!("must start from a leaf node")
        };
        let mut path = strings
            .get_str_part_id(string_offset as usize)
            .0
            .to_string()
            .into_bytes();
        path.reverse();

        while let Some(prev) = node.get_previous() {
            let cur_idx = node_idx;
            node_idx = prev.try_into().unwrap();
            node = &self.nodes[node_idx];
            path.push((cur_idx as i32 ^ node.next()).try_into().unwrap());
        }

        path.reverse();
        String::from_utf8(path).unwrap()
    }

    /// Allocates a new node block (0x80 entries) and returns the first offset of the block as the
    /// base value to XOR from.
    ///
    /// ## Params
    ///
    /// * `previous_node`: the parent node. A new block is allocated and all its children are
    /// moved to that block. At the end, the `next` value in `previous_node` is updated accordingly.
    pub fn allocate_new_block(&mut self, previous_node: i32) -> usize {
        const BLOCK_SIZE: usize = 0x80;
        let mut offset = self.nodes.len();
        // Offset should be the center point wrt XOR with a value in [0, BLOCK_SIZE-1]. In other words,
        // `offset ^ x` should be in [nodes.len(), nodes.len()+BLOCK_SIZE-1] for all x in [0, BLOCK_SIZE-1].
        self.nodes.reserve_exact(BLOCK_SIZE);
        for _ in 0..BLOCK_SIZE {
            self.nodes.push(DictNode::Free);
        }

        // Copy old block.
        let next = self.nodes[previous_node as usize].next();
        for c in 0..BLOCK_SIZE as i32 {
            // Select the characters that are actually children
            let Some(node) = (next ^ c)
                .try_into()
                .ok()
                .and_then(|i: usize| self.nodes.get(i))
                .copied()
            else {
                continue;
            };
            if !node.is_child(previous_node) {
                continue;
            }

            // Then, copy the old child node to the new block

            let from_idx = next ^ c;
            let to_idx = offset ^ c as usize;
            println!(
                "[CHR {}] Copying {node:?} to {}",
                char::from_u32(c as u32).unwrap(),
                to_idx
            );
            self.nodes[to_idx] = node;

            // Lastly, fix the links to each child's children (the `previous` value on each
            // grandchild must match the child's index)
            if let Some(next) = node.get_next() {
                // Again, find the characters that make grandchild nodes
                for c in 0..BLOCK_SIZE as i32 {
                    if let Some(node) = (next ^ c)
                        .try_into()
                        .ok()
                        .and_then(|i: usize| self.nodes.get(i))
                        .copied()
                    {
                        if node.is_child(from_idx) {
                            // Fix the link
                            println!("Attaching PREV {} => {}", next ^ c as i32, to_idx);
                            self.nodes[(next ^ c) as usize].attach_previous(to_idx as i32);
                        }
                    }
                }
            }

            // Child was fully moved, replace the initial slot with a free node
            self.nodes[from_idx as usize] = DictNode::Free;
        }
        // At the end, fix back links for source node (see function docs)
        self.nodes[previous_node as usize].attach_next(offset as i32);
        offset
    }
}

impl FileTable {
    pub fn get_meta(&self, file_id: u32) -> Option<&FileMeta> {
        self.files
            .binary_search_by_key(&file_id, |f| f.id)
            .ok()
            .map(|id| &self.files[id])
    }

    pub fn get_meta_mut(&mut self, file_id: u32) -> Option<&mut FileMeta> {
        self.files
            .binary_search_by_key(&file_id, |f| f.id)
            .ok()
            .map(|id| &mut self.files[id])
    }

    pub fn push_entry(&mut self, mut meta: FileMeta) -> u32 {
        let id = self.files.len().try_into().expect("dir tree limit");
        meta.id = id;
        self.files.push(meta);
        id
    }
}

impl DictNode {
    pub fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf { .. })
    }

    pub fn is_free(&self) -> bool {
        *self == Self::Free
    }

    pub fn previous(&self) -> i32 {
        self.get_previous().unwrap()
    }

    pub fn next(&self) -> i32 {
        self.get_next().unwrap()
    }

    pub fn next_after_chr(&self, ascii: u8) -> i32 {
        self.next() ^ ascii as i32
    }

    pub fn is_child(&self, parent: i32) -> bool {
        self.get_previous().is_some_and(|prev| prev == parent)
    }

    pub fn get_previous(&self) -> Option<i32> {
        match self {
            DictNode::Free | DictNode::Root { .. } => None,
            DictNode::Occupied { previous, .. } => Some(*previous),
            DictNode::Leaf { previous, .. } => Some(*previous),
        }
    }

    pub fn get_next(&self) -> Option<i32> {
        match self {
            DictNode::Occupied { next, .. } | DictNode::Root { next } => Some(*next),
            _ => None,
        }
    }

    fn attach_next(&mut self, next_node: i32) {
        match self {
            v @ DictNode::Free => *v = DictNode::Root { next: next_node },
            DictNode::Root { next } => *next = next_node,
            DictNode::Occupied { next, .. } => *next = next_node,
            DictNode::Leaf { .. } => panic!("cannot attach_next to leaf node"),
        }
    }

    fn attach_previous(&mut self, prev_node: i32) {
        match self {
            DictNode::Free => panic!("cannot attach_previous to free node"),
            DictNode::Root { next } => {
                *self = DictNode::Occupied {
                    previous: prev_node,
                    next: *next,
                }
            }
            DictNode::Occupied { previous, .. } | DictNode::Leaf { previous, .. } => {
                *previous = prev_node
            }
        }
    }
}

impl From<RawDictNode> for DictNode {
    fn from(value: RawDictNode) -> Self {
        match (value.prev, value.next) {
            (i32::MIN..=-1, i32::MIN..=-1) => Self::Free,
            (i32::MIN..=-1, 0..) => Self::Root { next: value.next },
            (0.., i32::MIN..=-1) => Self::Leaf {
                previous: value.prev,
                string_offset: -value.next,
            },
            (0.., 0..) => Self::Occupied {
                previous: value.prev,
                next: value.next,
            },
        }
    }
}

impl From<DictNode> for RawDictNode {
    fn from(value: DictNode) -> Self {
        match value {
            DictNode::Free => RawDictNode { next: -1, prev: -1 }, // Technically -id, shouldn't matter
            DictNode::Root { next } => RawDictNode { next, prev: -1 },
            DictNode::Occupied { previous, next } => RawDictNode {
                next,
                prev: previous,
            },
            DictNode::Leaf {
                previous,
                string_offset,
            } => RawDictNode {
                next: -string_offset,
                prev: previous,
            },
        }
    }
}
