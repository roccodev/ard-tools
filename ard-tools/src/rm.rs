use anyhow::{anyhow, Result};
use ardain::{path::ArhPath, DirEntry};
use clap::Args;

use crate::InputData;

#[derive(Args)]
pub struct RemoveArgs {
    /// The file or directory to remove
    path: ArhPath,
    /// Remove all contents of each directory, including subdirectories. (Required to remove
    /// non-empty directories)
    #[arg(short, long)]
    recursive: bool,
}

pub fn run(input: &InputData, args: RemoveArgs) -> Result<()> {
    let mut fs = input.load_fs()?;
    if fs.is_file(&args.path) {
        fs.delete_file(&args.path)?;
    } else if fs.is_dir(&args.path) {
        let dir = fs.get_dir(&args.path).unwrap();
        let DirEntry::Directory { children } = &dir.entry else {
            unreachable!()
        };
        if !args.recursive && !children.is_empty() {
            return Err(anyhow!(
                "refusing to delete non-empty directory: use --recursive to empty it first"
            ));
        }
        if args.recursive {
            for path in dir.children_paths() {
                fs.delete_file(&format!("{}{path}", args.path))?;
            }
        }
        fs.delete_empty_dir(&args.path)?;
    } else {
        return Err(anyhow!("no such file or directory"));
    }
    input.write_fs(&mut fs)?;
    Ok(())
}
