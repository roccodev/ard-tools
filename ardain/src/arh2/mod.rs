use std::borrow::Cow;

use binrw::{BinRead, BinWrite};
use hash::Arh2NameTable;

use crate::path::ArhPath;

mod ext;
pub(crate) mod fs;
pub(crate) mod hash;

const RESERVED_PREFIX: &str = "/unknown+names/";

#[derive(Debug, Clone, BinRead, BinWrite)]
#[brw(little, magic(b"arh2"))]
pub struct Arh2 {
    // is there a better way?
    #[bw(map = |_: &u32| u32::try_from(entries.len()).expect("too many files"))]
    entry_count: u32,
    ard_file_align: u32,
    #[br(count = entry_count, align_before = 16)]
    #[bw(align_before = 16)]
    entries: Vec<Arh2Entry>,
}

#[derive(Debug, PartialEq, Clone, Copy, BinRead, BinWrite)]
pub struct Arh2Entry {
    pub file_name_hash: u64,
    size_in_ard: u32,
    extracted_size: u32,

    #[brw(ignore)]
    calculated_offset: u64,
}

impl Arh2 {
    pub fn entries(&self) -> &[Arh2Entry] {
        &self.entries
    }

    pub fn insert_entry(&mut self, entry: Arh2Entry) -> bool {
        if let Err(idx) = self
            .entries
            .binary_search_by_key(&entry.file_name_hash, |f| f.file_name_hash)
        {
            self.entries.insert(idx, entry);
            true
        } else {
            false
        }
    }

    pub fn remove_entry(&mut self, hash: u64) -> Option<Arh2Entry> {
        self.entries
            .binary_search_by_key(&hash, |f| f.file_name_hash)
            .ok()
            .map(|i| self.entries.remove(i))
    }

    pub fn get_file_hash(&self, path: &str) -> u64 {
        if let Some(hash) = path.strip_prefix(RESERVED_PREFIX) {
            u64::from_str_radix(hash, 16).unwrap_or_default()
        } else {
            hash::hash_path(path)
        }
    }

    fn prepare_read(&mut self) {
        let mut offset = 0;
        for entry in &mut self.entries {
            entry.calculated_offset = offset;
            offset += (entry.size_in_ard as u64).next_multiple_of(self.ard_file_align.into());
        }
        self.entries.sort_unstable_by_key(|f| f.file_name_hash);
    }

    fn prepare_write(&mut self) {
        // TODO: fix sizes if files were deleted
        self.entries.sort_unstable_by_key(|f| f.calculated_offset);
    }

    pub fn is_name_reserved(name: &ArhPath) -> bool {
        // ArhPath is already lowercase
        name.contains(RESERVED_PREFIX)
    }
}

impl Arh2Entry {
    pub fn get_name<'t>(&self, table: &'t Arh2NameTable) -> Cow<'t, str> {
        table
            .get(self.file_name_hash)
            .map(Cow::Borrowed)
            .unwrap_or_else(|| {
                Cow::Owned(format!("{}{:016X}", RESERVED_PREFIX, self.file_name_hash))
            })
    }
}
