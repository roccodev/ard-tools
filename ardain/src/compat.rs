use std::{any::Any, mem::MaybeUninit};

use binrw::{BinRead, BinWrite};

use crate::{
    arh1::Arh1, arh2::Arh2, error::Result, opts::ArhOptions, path::ArhPath,
    private::ArhAccessPrivate, ArhAccess, FileEntry,
};

pub type CompatArhDyn = Box<dyn ArhAccess + Send + Sync>;

#[derive(BinRead, BinWrite)]
pub enum CompatArh {
    Arh1(Arh1),
    Arh2(Arh2),
}

#[derive(BinWrite)]
pub enum CompatArhRef<'a> {
    Arh1(&'a Arh1),
    Arh2(&'a Arh2),
}

impl CompatArh {
    pub fn erase(self) -> CompatArhDyn {
        match self {
            CompatArh::Arh1(arh) => Box::new(arh),
            CompatArh::Arh2(arh) => Box::new(arh),
        }
    }

    pub fn into_inner<A: ArhAccess + 'static>(mut self) -> std::result::Result<A, CompatArhDyn> {
        let inner_ref: &mut dyn Any = match &mut self {
            CompatArh::Arh1(arh) => arh,
            CompatArh::Arh2(arh) => arh,
        };
        if inner_ref.is::<A>() {
            let versioned = unsafe {
                // SAFETY: Manual mem::take, and there is no way to unwind at this point
                let out = std::ptr::read(inner_ref.downcast_mut().unwrap());
                // Keep the forget here so it's not removed accidentally
                std::mem::forget(self);
                out
            };
            Ok(versioned)
        } else {
            Err(self.erase())
        }
    }
}

impl ArhAccess for CompatArhDyn {}
impl ArhAccessPrivate for CompatArhDyn {
    fn post_read(&mut self) {
        (**self).post_read();
    }

    fn get_file_entry(&self, path: &ArhPath) -> Option<FileEntry> {
        (**self).get_file_entry(path)
    }

    fn init_dir_tree(&self, opts: &ArhOptions) -> crate::DirNode {
        (**self).init_dir_tree(opts)
    }

    fn create_file(&mut self, path: &ArhPath) -> Result<FileEntry> {
        (**self).create_file(path)
    }

    fn delete_file(&mut self, path: &ArhPath, opts: &ArhOptions) -> Result<()> {
        (**self).delete_file(path, opts)
    }

    fn hide_file(&mut self, path: &ArhPath, hidden: bool) -> Result<()> {
        (**self).hide_file(path, hidden)
    }

    fn rename_file(
        &mut self,
        from: &ArhPath,
        to: &ArhPath,
        opts: &ArhOptions,
        dir_tree: &mut crate::DirNode,
    ) -> Result<()> {
        (**self).rename_file(from, to, opts, dir_tree)
    }

    fn prepare_for_write(&mut self) {
        (**self).prepare_for_write();
    }

    fn into_compat(self: Box<Self>) -> CompatArh {
        (*self).into_compat()
    }

    fn as_compat(&self) -> CompatArhRef {
        (**self).as_compat()
    }
}
