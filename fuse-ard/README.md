# FUSE driver for .arh/.ard files

This is a [FUSE](https://www.kernel.org/doc/html/latest/filesystems/fuse.html) driver that adds support for mounting ARH and ARD files.

## Building

Run this command from any directory in the workspace:
```
cargo build --release -p fuse-ard
```

## Usage

The tool can be used to mount an ARH/ARD file pair as a FUSE file system.

The ARD file is only required to read data. If it is not provided, the file system can still be accessed to list files and directories.

```
Usage: fuse-ard [OPTIONS] --arh <FILE> <mount_point>

Arguments:
  <mount_point>  where to mount the archive, e.g. /mnt/ard

Options:
      --arh <FILE>  path to the .arh file
      --ard <FILE>  path to the .ard file. If absent, some operations won't be available.
  -r, --readonly    mount the archive as read-only
  -d, --debug       enable FUSE debugging and debug logs
  -h, --help        Print help
```

## License

This tool is licensed under the GPLv3. See [COPYING](COPYING) for details.