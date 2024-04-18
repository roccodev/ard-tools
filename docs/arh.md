# ARH file format

ARH files accompany ARD files and define the structure for the latter's internal file system.

This file system is optimized for queries and random file access; it does not have an easy way to list files or traverse the directory tree.

## Data types

### Header

| Field | Type | Notes |
| ----- | ---- | ----- |
| Magic | 4 bytes | "arh1" |
| *Unknown* | u32 | same value as string table size |
| Path dictionary entry count | u32 | Path dictionary size / 8 |
| String table pointer | u32 | absolute |
| String table size | u32 | |
| Path dictionary pointer | u32 | absolute |
| Path dictionary size | u32 | |
| File metadata table pointer | u32 | absolute |
| File count | u32 | |
| Encryption key | u32 | |
| Ext. magic | 4 bytes | "arhx", **determines whether extended section is present** (see below) |
| Extended section offset | u32 | absolute, **only if extended section is present** |
| Padding | | (total header size: 48 bytes)

### String table

Each 32-bit word in this section is XORed with `encryption key ^ 0xF3F35353`.

The string table is a sequence of pairs structured as follows:

| Field | Type | Notes |
| ----- | ---- | ----- |
| String part | nul-terminated ASCII string | |
| File ID | u32 | Matches ID in file metadata |

Entries in this table represent parts of a path name.

### Path dictionary

Each 32-bit word in this section is XORed with `encryption key ^ 0xF3F35353`.

Each entry in the path dictionary is structured as follows:

| Field | Type | Notes |
| ----- | ---- | ----- |
| Next part | i32 (signed) | If x < 0, then there is no next part but -x is an offset for the string table |
| Previous part | i32 (signed) | If x < 0, there is no previous part, -x is an offset for the string table |

The "next" field, if non-negative, points to a range of other entries based on the next character in the string: if x is the value of the field, the next dictionary entry is `x ^ c`, where `c` is the ASCII value of the character. (<= 0x7F)

After following the "next" field, it should be checked that the "previous" field of the next entry is the same as the original index.

The string table offset does not necessarily point to the start of a string. Instead, if a negative "next" field is encountered at index `i` in the original string, the string should be scanned starting from `i` and compared to the query until a nul-byte is reached. If the strings match, the file ID can then be acquired from the string table entry. An example snippet is included at the end of the document.

The first entry in the path dictionary should have `next = 0` and `previous < 0`.

### File metadata table

| Field | Type | Notes |
| ----- | ---- | ----- |
| Data offset | u64 | Offset in the .ard file |
| Compressed size | u32 |  |
| Uncompressed size | u32 |  |
| *Unknown* | u32 |  |
| ID | u32 | |

## Extended section

This isn't part of the official format, rather it is useful to the tools in this repository. It is ignored by the game.

### Section

| Field | Type | Notes |
| ----- | ---- | ----- |
| Magic | 4 bytes | "arhx" |
| Block allocation table | | see below |
| File recycle bin | | see below |

### Block allocation table

Used to find free space when allocating new files.

A lower block size allows for better packing (higher chances for smaller files to fit), but also increases the length of the block array, and thus the size of the ARH file.

| Field | Type | Notes |
| ----- | ---- | ----- |
| Block size | u16 | The size of a single block (bytes, exponent base 2) |
| Block array length | u64 | |
| Blocks | u64 * Block array length | Bit array of occupied blocks (1 = occupied, 0 = free) |

### File recycle bin

Deleted files are added to this set, so that their entry slot can be reused when adding new ones.

| Field | Type | Notes |
| ----- | ---- | ----- |
| File count | u32 | |
| File IDs | u32 * File count | in ascending order |

## Operations

### File lookup by path

```cpp
// name must start with /
int get_file_id(DictNode *dict, char *strings, char *name) {
    int len = strlen(name);

    int node = 0;
    int last = -1;
    while (dict[node]->next >= 0) {
        if (len == 0) {
            // If we've consumed the whole path, the file exists iff there are no more
            // nodes to visit.
            if node == dict[node]->prev {
                break;
            }
            return -1;
        }
        int next = dict[node]->next ^ (*name);
        if (dict[next]->prev != node) {
            // Wrong prefix or directory name
            return -1;
        }
        node = next;
        len--;
        name++;
    }

    // Compare the file name against the final part we found in the
    // string table
    int str_offset = -dict[node]->next;
    do {
        if (len-- <= 0 || *name != strings[str_offset]) {
            // Name mismatch
            return -1;
        }
        str_offset++;
        name++;
    } while (strings[str_offset] != 0);

    return * (int *) (strings + str_offset + 1);
}

```