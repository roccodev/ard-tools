use anyhow::{anyhow, Result};
use ardain::{path::ArhPath, DirEntry, FileFlag};
use clap::Args;

use crate::InputData;

#[derive(Args)]
pub struct RemoveArgs {
    /// The file or directory to remove
    #[arg(value_parser = crate::parse_path)]
    path: ArhPath,
    /// Remove all contents of each directory, including subdirectories. (Required to remove
    /// non-empty directories)
    #[arg(short, long)]
    recursive: bool,
    /// Mark the files as hidden instead of deleting them from the archive. The game will still
    /// treat them as deleted.
    #[arg(short, long)]
    soft: bool,
}

pub fn run(input: &InputData, args: RemoveArgs) -> Result<()> {
    let mut fs = input.load_fs()?;
    if fs.is_file(&args.path) {
        if args.soft {
            fs.get_file_info_mut(&args.path)
                .unwrap()
                .set_flag(FileFlag::Hidden, true);
        } else {
            fs.delete_file(&args.path)?;
        }
    } else if fs.is_dir(&args.path) {
        let dir = fs.get_dir(&args.path).unwrap();
        let DirEntry::Directory { children } = &dir.entry else {
            unreachable!()
        };
        if !args.soft && !args.recursive && !children.is_empty() {
            return Err(anyhow!(
                "refusing to delete non-empty directory: use --recursive to empty it first"
            ));
        }
        if args.recursive || args.soft {
            for path in dir.children_paths() {
                if args.soft {
                    fs.get_file_info_mut(&args.path.join(&path))
                        .unwrap()
                        .set_flag(FileFlag::Hidden, true);
                } else {
                    fs.delete_file(&args.path.join(&path))?;
                }
            }
        }
        fs.delete_empty_dir(&args.path)?;
    } else {
        return Err(anyhow!("no such file or directory"));
    }
    input.write_fs(&mut fs)?;
    Ok(())
}
