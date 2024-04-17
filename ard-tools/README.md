# Basic ARH/ARD utilities

This is a set of command-line tools to perform queries (list files, directories, etc.) and modifications (add, remove, move files, etc.) on ARH file systems.

## Building

Run this command from any directory in the workspace:
```
cargo build --release -p arh-tools
```

## Usage

```
Usage: ard-tools [OPTIONS] <COMMAND>

Commands:
  list    List all files in a directory [aliases: ls]
  remove  Remove files or directories [aliases: rm]

Options:
      --arh <IN_ARH>       Input .arh file, required for most commands
      --ard <IN_ARD>       Input .ard file (data archive)
      --out-arh <OUT_ARH>  Output .arh file, for commands that write data and metadata. If absent, the input .arh file will be overwritten!
  -h, --help               Print help
  -V, --version            Print version
```

## License

This tool is licensed under the GPLv3. See [COPYING](COPYING) for details.