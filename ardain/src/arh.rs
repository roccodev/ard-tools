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
    #[br(args { count: usize::try_from(len).unwrap() / size_of::<DictNode>() }, map_stream = |reader| EncryptedSection::decrypt(reader, len, key).expect("TODO"))]
    pub nodes: Vec<DictNode>,
}

#[derive(Debug, PartialEq, Clone)]
#[binread]
#[br(import { len: u32 })]
pub struct FileTable {
    #[br(args { count: usize::try_from(len).unwrap() })]
    files: Vec<FileMeta>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[binread]
pub struct DictNode {
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
        assert!(node.next < 0, "must start from a leaf node");

        let mut path = strings
            .get_str_part_id(-node.next as usize)
            .0
            .to_string()
            .into_bytes();
        path.reverse();

        while node.next != 0 {
            let cur_idx = node_idx;
            node_idx = node.prev.try_into().unwrap();
            node = &self.nodes[node_idx];
            path.push((cur_idx as i32 ^ node.next).try_into().unwrap());
        }

        path.reverse();
        String::from_utf8(path).unwrap()
    }

    /// Allocates a new node block (0x80 entries) and returns the first offset of the block.
    pub fn allocate_new_block(&mut self) -> usize {
        let offset = self.nodes.len();
        self.nodes.reserve_exact(0x80);
        for _ in 0..0x80 {
            self.nodes.push(DictNode { next: 0, prev: 0 });
        }
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
