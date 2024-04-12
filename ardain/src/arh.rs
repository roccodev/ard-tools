use std::{
    ffi::CStr,
    io::{self, Cursor, Read, Seek, SeekFrom},
    mem::size_of,
};

use binrw::{binread, BinRead, NullString};

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
    file_table: FileTable,
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
    node_table: PathDictionary,
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
    nodes: Vec<DictNode>,
}

#[derive(Debug, PartialEq, Clone)]
#[binread]
#[br(import { len: u32 })]
pub struct FileTable {
    #[br(args { count: usize::try_from(len).unwrap() })]
    files: Vec<FileMeta>,
}

#[derive(Debug, PartialEq, Clone)]
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
    _unk: u32,
    pub id: u32,
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
    fn get_str_part_id(&self, mut offset: usize) -> (&str, u32) {
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
}

fn test_lookup(arh: &Arh, name: &str) {
    let nodes = &arh.encrypted.node_table.nodes;
    let mut cur = (0usize, &nodes[0]);
    let mut i = 0;
    for (j, b) in name.bytes().enumerate() {
        i = j;
        if cur.1.next < 0 {
            break;
        }
        let next = (cur.1.next ^ b as i32) as usize;
        println!("{next} => {:?}", nodes[next]);
        assert_eq!(nodes[next].prev, cur.0 as i32);
        cur = (next, &nodes[next]);
    }
    println!("Found {cur:?}");
    println!(
        "Parts {:?} {:?}",
        arh.encrypted
            .string_table
            .get_str_part_id(-cur.1.next as usize),
        &name[i..]
    )
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use binrw::BinRead;

    use crate::arh::test_lookup;

    use super::Arh;

    #[test]
    pub fn a() {
        let mut f = File::open("/tmp/bf3.arh").unwrap();
        let f = Arh::read(&mut f).unwrap();
        //println!("{f:?}");
        test_lookup(&f, "/bdad/btl.bdat");
        panic!();
    }
}
