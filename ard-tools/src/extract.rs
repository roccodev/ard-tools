use std::{
    collections::VecDeque,
    fs::File,
    io::{BufReader, Cursor, Read, Seek},
    os::unix::fs::MetadataExt,
    path::Path,
    time::Instant,
};

use anyhow::{anyhow, bail, Context, Result};
use ardain::{path::ArhPath, ArdReader, DirEntry, FileMeta};
use clap::Args;
use indicatif::ProgressBar;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::InputData;

#[derive(Args)]
pub struct ExtractArgs {
    #[arg(long = "out", short)]
    out_dir: String,
    #[arg(value_parser = crate::parse_path)]
    from_paths: Vec<ArhPath>,
}

enum ArdAccess<'b> {
    File(File),
    Mem(&'b [u8]),
}

pub fn run(input: &InputData, args: ExtractArgs) -> Result<()> {
    let fs = input.load_fs()?;
    let root_out = Path::new(&args.out_dir);
    let mut ard_file = input.ard_file()?;

    // Extraction steps:
    // 1. Collect path skeleton
    // 2. Extract files

    let start = Instant::now();

    // let mut buf = Vec::new();
    // buf.try_reserve_exact(ard_file.metadata()?.size().try_into()?)?;
    // BufReader::with_capacity(ard_file).read_to_end(&mut buf)?;
    // let buf = std::fs::read(input.in_ard.as_ref().unwrap())?;
    let elapsed = start.elapsed();
    println!(
        "File reading completed in {} seconds.",
        elapsed.as_secs_f64()
    );

    let mut arh_paths = vec![];

    for path in args.from_paths {
        if fs.is_file(&path) {
            arh_paths.push(path);
        } else if let Some(dir) = fs.get_dir(&path) {
            arh_paths.extend(dir.children_paths().into_iter().map(|s| path.join(&s)));
        } else {
            bail!("File {path} was not found");
        }
    }

    // Extract files
    let start = Instant::now();
    let progress = ProgressBar::new(arh_paths.len().try_into().unwrap());
    arh_paths.into_par_iter().try_for_each_init(
        || Some(input.ard_file().unwrap()),
        // || (),
        |ard_file, path| {
            let Some(file) = fs.get_file_info(&path) else {
                unreachable!()
            };
            let out_path = root_out.join(&path.as_str()[1..]);
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create parent dir {parent:?}"))?;
            }
            let mut ard_f = ard_file.take().unwrap();
            ard_f.rewind()?;
            ArdAccess::File(ard_f.try_clone()?)
                // ArdAccess::Mem(&buf)
                .copy_to(&out_path, file)
                .with_context(|| format!("failed to extract {path}"))?;
            ard_file.replace(ard_f);
            progress.inc(1);
            Ok::<(), anyhow::Error>(())
        },
    )?;
    progress.finish();
    let elapsed = start.elapsed();
    println!("Extraction completed in {} seconds.", elapsed.as_secs_f64());

    Ok(())
}

impl<'b> ArdAccess<'b> {
    fn copy_to(&self, out_path: &Path, file: &FileMeta) -> Result<()> {
        let buf = match self {
            ArdAccess::File(ard) => ArdReader::new(BufReader::new(ard)).entry(file).read(),
            ArdAccess::Mem(ard) => ArdReader::new(Cursor::new(ard)).entry(file).read(),
        }?;
        Ok(std::fs::write(out_path, buf)?)
    }
}
