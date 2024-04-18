use std::borrow::Cow;

use anyhow::{anyhow, Result};
use ardain::{path::ArhPath, DirEntry, FileFlag, FileMeta};
use clap::Args;

use crate::InputData;

#[derive(Args)]
pub struct ListArgs {
    working_directory: Option<ArhPath>,
    /// Only print file and directory names
    #[arg(short, long)]
    raw: bool,
}

#[derive(Default)]
struct Table<'a> {
    rows: Vec<Vec<Cow<'a, str>>>,
    lens: Vec<usize>,
}

pub fn run(input: &InputData, args: ListArgs) -> Result<()> {
    let fs = input.load_fs()?;
    let wd = args.working_directory.unwrap_or_default();

    let dir = fs
        .get_dir(&wd)
        .ok_or_else(|| anyhow!("directory not found"))?;
    let DirEntry::Directory { children } = &dir.entry else {
        unreachable!()
    };

    if !args.raw {
        println!("In {wd}:\n");
    }

    let mut dirs = 0;
    let mut files = 0;

    let mut table = Table::default();

    if !args.raw {
        table.push_row(vec!["Name", "Type", "Flags", "Size"]);
        table.push_row(vec!["----", "----", "-----", "----"]);
    }

    for child in children {
        match child.entry {
            DirEntry::File => {
                let file = fs.get_file_info(&format!("{wd}/{}", child.name)).unwrap();
                let file_size = file.actual_size();
                table.push_row::<Cow<_>>(vec![
                    child.name.as_str().into(),
                    "File".into(),
                    get_flags_display(file).into(),
                    format!("{file_size}").into(),
                ]);
                files += 1;
            }
            DirEntry::Directory { .. } => {
                table.push_row(vec![&child.name, "Directory", "", "--"]);
                dirs += 1;
            }
        }
    }

    table.print();

    if !args.raw {
        println!("\n{dirs} directories, {files} files");
    }

    Ok(())
}

fn get_flags_display(meta: &FileMeta) -> String {
    let mut res = String::new();
    if meta.is_flag(FileFlag::Hidden) {
        res.push('H');
    }
    if meta.is_flag(FileFlag::HasXbc1Header) {
        res.push('X');
    }
    res
}

impl<'a> Table<'a> {
    fn push_row<S: Into<Cow<'a, str>>>(&mut self, row: impl IntoIterator<Item = S>) {
        let row: Vec<_> = row.into_iter().map(Into::into).collect();
        for (i, cell) in row.iter().enumerate() {
            if i >= self.lens.len() {
                self.lens.push(cell.len());
            } else {
                self.lens[i] = cell.len().max(self.lens[i]);
            }
        }
        self.rows.push(row);
    }

    fn print(self) {
        for row in self.rows {
            for (i, cell) in row.into_iter().enumerate() {
                print!("{:<1$}  ", cell, self.lens[i]);
            }
            println!();
        }
    }
}
