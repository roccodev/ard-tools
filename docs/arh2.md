# ARH2 file format

**Note**: This documents the ARH2 format, used in Xenoblade X DE. Older games use
the legacy version of the format, documented [here](arh1.md).

ARH files accompany ARD files and define the structure for the latter's internal file system.

## Format

| Field | Type | Notes |
| ----- | ---- | ----- |
| Magic | 4 bytes | "arh2" |
| File count | u32 |  |
| File alignment | u32 | Must be a power of 2 |
| Extended section pointer | u32 | **Only if ExtSection is present** (see below), padding otherwise |
| File entries | List of FileEntry | Size = File count. Sorted by ARD offset |

### FileEntry

| Field | Type | Notes |
| ----- | ---- | ----- |
| Path hash | u64 | Hash of absolute path (lowercase, with leading /, XXH64, seed 0) |
| Size on ARD | u32 | Space occupied in the ARD |
| Expanded size | u32 | If compressed, size of the uncompressed file. 0 if uncompressed |

### ExtSection

This isn't part of the official format, rather it is useful to the tools in this repository. It is ignored by the game.

| Field | Type | Notes |
| ----- | ---- | ----- |
| Magic | 4 bytes | "arhy" |
| Block allocation table | | see the [ARH1 docs](arh1.md#block-allocation-table) |
| Hidden files list | | see below |

**Hidden files list**

Some tools in this repository allow "hiding" files from the game, instead of deleting them, which prevents the game from loading them but still reserves space in the filesystem.

ARH2 has no native support for this, but it is achieved by setting the file path hash to 0, and the expanded size to point to an entry in this list. The entries in this list serve as backups for the original data.

| Field | Type | Notes |
| ----- | ---- | ----- |
| File count | u32 | |
| **Next structure is repeated for each file** | | |
| Original path hash | u64 | |
| Original expanded size | u32 |

## File paths

Unlike the previous format, ARH2 does not store file names in the file system, only their hashes.

The hasher used is [XXH64](https://github.com/Cyan4973/xxHash). The hash input is the file's absolute path, which includes the leading slash. For files with mixed-case names, the lowercase path is used.

Example hashes:
```
/bdat/common.bdat      = 4FEC95D41839AD42
/script/jp/com_wait.sb = BF084CF8F46AA0AD
/param/devxml/Doll.bin = 7143898E5701EB50 (/param/devxml/doll.bin)
```

## Calculating file offsets

File offset is notably missing from the ARH data, but it is because it can be derived from the size of the previous entries. Importantly, **file entries are stored in the same order as they appear in the ARD.**

An example algorithm is the following:

```cpp
struct ArhEntry {
    uint64_t hash;
    uint32_t ard_size;
    uint32_t exp_size;

    uint64_t offset; // This will be calculated
};

struct Arh2 {
    uint32_t ard_alignment; // Power of 2
    std::vector<ArhEntry> entries;
};

void calc_offsets(Arh2& arh) {
    uint64_t offset = 0;
    for (auto& entry : arh.entries) {
        entry.offset = offset;
        // Align size to `ard_alignment`
        uint32_t aligned_size = (arh.ard_alignment - 1) 
                    + entry.ard_size 
                    & -arh.ard_alignment;
        offset += aligned_size;
    }
}
```