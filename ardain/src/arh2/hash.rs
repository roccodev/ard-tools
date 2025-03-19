use std::{collections::HashMap, hash::BuildHasherDefault, io::BufRead};

use twox_hash::XxHash64;

use crate::error::Result;

#[derive(Clone, Default)]
pub struct Arh2NameTable {
    // It should be faster than the std hasher, so might as well use it
    hash_lookup: HashMap<u64, Box<str>, BuildHasherDefault<XxHash64>>,
}

impl Arh2NameTable {
    pub fn read(reader: impl BufRead) -> Result<Arh2NameTable> {
        let mut table = HashMap::default();
        for line in reader.lines() {
            let line = line?.to_ascii_lowercase();
            let hash = hash_exact_path(&line);
            table.insert(hash, line.into_boxed_str());
        }
        Ok(Arh2NameTable { hash_lookup: table })
    }

    pub fn get(&self, hash: u64) -> Option<&str> {
        self.hash_lookup.get(&hash).map(|s| &**s)
    }
}

pub fn hash_path(abs_path: &str) -> u64 {
    if abs_path.chars().all(|c| c.is_ascii_lowercase()) {
        hash_exact_path(abs_path)
    } else {
        hash_exact_path(&abs_path.to_ascii_lowercase())
    }
}

/// Faster version of [`hash_path`] if it is known that the input is lowercase.
pub fn hash_exact_path(abs_path: &str) -> u64 {
    XxHash64::oneshot(0, abs_path.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::arh2::hash::{hash_path, Arh2NameTable};

    #[test]
    fn results() {
        assert_eq!(hash_path("/bdat/common.bdat"), 0x4FEC95D41839AD42_u64);
        assert_eq!(hash_path("/script/jp/com_wait.sb"), 0xBF084CF8F46AA0AD_u64);
        assert_eq!(hash_path("/param/devxml/doll.bin"), 0x7143898E5701EB50_u64);
        assert_eq!(hash_path("/param/devxml/Doll.bin"), 0x7143898E5701EB50_u64);
    }

    #[test]
    fn table() {
        let buf = Cursor::new("/bdat/common.bdat\n/script/jp/com_wait.sb\n/param/devxml/Doll.bin");
        let table = Arh2NameTable::read(buf).unwrap();

        assert_eq!(table.get(0x4FEC95D41839AD42_u64), Some("/bdat/common.bdat"));
        assert_eq!(
            table.get(0xBF084CF8F46AA0AD_u64),
            Some("/script/jp/com_wait.sb")
        );
        assert_eq!(
            table.get(0x7143898E5701EB50_u64),
            Some("/param/devxml/doll.bin")
        );
    }
}
