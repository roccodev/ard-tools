use anyhow::{anyhow, Result};
use ardain::{path::ArhPath, ArhFileSystem, DirEntry, FileFlag};
use clap::{ArgGroup, Args};

use crate::InputData;

#[derive(Args)]
#[clap(group(
    ArgGroup::new("my-group")
        .required(false)
        .args(&["soft", "restore"]),
))]
pub struct RemoveArgs {
    /// The files or directories to remove
    #[arg(value_parser = crate::parse_path)]
    paths: Vec<ArhPath>,
    /// Remove all contents of each directory, including subdirectories. (Required to remove
    /// non-empty directories)
    #[arg(short, long)]
    recursive: bool,
    /// Mark the files as hidden instead of deleting them from the archive. The game will still
    /// treat them as deleted. Always operates recursively.
    #[arg(short, long)]
    soft: bool,
    /// Remove the hidden flag on files that were previously removed with --soft. Always
    /// operates recursively.
    #[arg(short = 'z', long)]
    restore: bool,
}

pub fn run(input: &InputData, args: RemoveArgs) -> Result<()> {
    let mut fs = input.load_fs()?;
    for path in &args.paths {
        if args.soft {
            set_hidden_flag(&mut fs, path, true)?;
        } else if args.restore {
            set_hidden_flag(&mut fs, path, false)?;
        } else {
            delete(&mut fs, &args, path)?;
        }
    }
    input.write_fs(&mut fs)?;
    Ok(())
}

fn delete(fs: &mut ArhFileSystem, args: &RemoveArgs, path: &ArhPath) -> Result<()> {
    if fs.is_file(path) {
        fs.delete_file(path)?;
    } else if fs.is_dir(path) {
        let dir = fs.get_dir(path).unwrap();
        let DirEntry::Directory { children } = &dir.entry else {
            unreachable!()
        };
        if !args.recursive && !children.is_empty() {
            return Err(anyhow!(
                "refusing to delete non-empty directory {path}: use --recursive to empty it first"
            ));
        }
        if args.recursive {
            for child in dir.children_paths() {
                fs.delete_file(&path.join(&child))?;
            }
        }
        fs.delete_empty_dir(path)?;
    } else {
        return Err(anyhow!("{path}: no such file or directory"));
    }
    Ok(())
}

fn set_hidden_flag(fs: &mut ArhFileSystem, path: &ArhPath, hidden: bool) -> Result<()> {
    if fs.is_file(path) {
        fs.get_file_info_mut(path)
            .unwrap()
            .set_flag(FileFlag::Hidden, true);
    } else if fs.is_dir(path) {
        let dir = fs.get_dir(path).unwrap();
        for child in dir.children_paths() {
            let meta = fs.get_file_info_mut(&path.join(&child)).unwrap();
            meta.set_flag(FileFlag::Hidden, hidden);
        }
    } else {
        return Err(anyhow!("{path}: no such file or directory"));
    }
    Ok(())
}
