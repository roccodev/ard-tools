use std::num::NonZeroU64;

use crate::{
    compat::{CompatArh, CompatArhRef},
    error::{Error, Result},
    opts::ArhOptions,
    path::ArhPath,
    private::ArhAccessPrivate,
    ArhAccess, DirEntry, DirNode, FileEntry,
};

use super::{Arh2, Arh2Entry};

impl ArhAccessPrivate for Arh2 {
    fn post_read(&mut self) {
        self.prepare_read();
    }

    fn get_file_entry(&self, path: &ArhPath) -> Option<FileEntry> {
        self.entries
            .binary_search_by_key(&self.get_file_hash(path), |e| e.file_name_hash)
            .ok()
            .map(|i| self.entries[i].into())
    }

    fn init_dir_tree(&self, opts: &ArhOptions) -> DirNode {
        let mut start = DirNode {
            name: "/".to_string(),
            entry: DirEntry::Directory {
                children: Vec::new(),
            },
        };
        for entry in self.entries() {
            start.insert_file_entry(entry.get_name(&opts.arh2_name_table).to_string());
        }
        start
    }

    fn create_file(&mut self, path: &ArhPath) -> Result<FileEntry> {
        if Self::is_name_reserved(path) {
            return Err(Error::FsReservedName);
        }
        let hash = self.get_file_hash(path);
        // Add file at the end
        let offset = self
            .entries
            .iter()
            .max_by_key(|f| f.calculated_offset)
            .map(|f| f.calculated_offset + u64::from(f.size_in_ard))
            .unwrap_or_default();
        let entry = Arh2Entry {
            file_name_hash: hash,
            size_in_ard: 0,
            extracted_size: 0,
            calculated_offset: offset,
        };
        self.insert_entry(entry)
            .then(|| entry.into())
            .ok_or(Error::FsAlreadyExists)
    }

    fn rename_file(
        &mut self,
        from: &ArhPath,
        to: &ArhPath,
        _opts: &ArhOptions,
        dir_tree: &mut DirNode,
    ) -> Result<()> {
        // Much simpler than ARH1, need to take care of reordering the list
        if self.get_file_entry(to).is_some() {
            return Err(Error::FsAlreadyExists);
        }
        if Self::is_name_reserved(to) {
            // Note: allow reserved names in `from`, also as a way to unhash a file name
            return Err(Error::FsReservedName);
        }
        let mut old = self
            .remove_entry(self.get_file_hash(from))
            .ok_or(Error::FsNoEntry)?;
        old.file_name_hash = self.get_file_hash(to);
        self.insert_entry(old);

        dir_tree.remove_file_entry(from);
        dir_tree.insert_file_entry(to.to_string());
        Ok(())
    }

    fn delete_file(&mut self, path: &ArhPath, _opts: &ArhOptions) -> Result<()> {
        self.remove_entry(self.get_file_hash(path))
            .map(|_| ())
            .ok_or(Error::FsNoEntry)
    }

    fn hide_file(&mut self, path: &ArhPath, hidden: bool) -> Result<()> {
        todo!()
    }

    fn prepare_for_write(&mut self) {
        self.prepare_write();
    }

    fn into_compat(self: Box<Self>) -> CompatArh {
        CompatArh::Arh2(*self)
    }

    fn as_compat(&self) -> CompatArhRef {
        CompatArhRef::Arh2(self)
    }
}

impl ArhAccess for Arh2 {}

impl From<Arh2Entry> for FileEntry {
    fn from(value: Arh2Entry) -> Self {
        Self {
            ard_offset: value.calculated_offset,
            ard_size: value.size_in_ard.into(),
            expanded_size: NonZeroU64::new(value.extracted_size.into()),
            unique_id: value.file_name_hash,
            xbc1_header: value.extracted_size != 0,
            hidden: false,
        }
    }
}
