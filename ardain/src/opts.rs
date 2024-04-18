use crate::arh_ext;

#[derive(Clone)]
pub struct ArhOptions {
    /// The size of a single block (bytes, power of 2) in the block allocation table.
    ///
    /// Lower values result in higher space efficiency in the ARD file (especially if there
    /// are many small files), but also increase the size of the ARH file.
    ///
    /// Defaults to [`arh_ext::BLOCK_SIZE_POW_DEFAULT`]
    pub ext_block_size_pow: u16,
    /// If `true`, when loading a file with an existing block table, the table will be
    /// regenerated if its block size is different than `ext_block_size_pow`.
    ///
    /// Defaults to `false`
    pub ext_force_block_size: bool,
}

impl Default for ArhOptions {
    fn default() -> Self {
        Self {
            ext_block_size_pow: arh_ext::BLOCK_SIZE_POW_DEFAULT,
            ext_force_block_size: false,
        }
    }
}
