use std::{
    fs::File,
    io::{BufReader, Cursor, Seek},
    path::Path,
    time::Instant,
};

use anyhow::{bail, Context, Result};
use ardain::{path::ArhPath, ArdReader, FileMeta};
use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::{
    current_num_threads, current_thread_index,
    iter::{IntoParallelIterator, ParallelIterator},
};

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

    // Extraction steps:
    // 1. Collect path skeleton
    // 2. Extract files

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

    // Sort paths by offset, leads to better access patterns for the underlying ARD file
    arh_paths.sort_by_cached_key(|path| fs.get_file_info(path).unwrap().offset);

    // Extract files
    let start = Instant::now();
    let progress = ProgressBar::new(arh_paths.len().try_into().unwrap()).with_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} ETA: {eta}",
        )
        .unwrap()
        .progress_chars("##-"),
    );

    // Open one fd per thread - try_for_each_init seems to call the init function more than once
    // per thread
    let thread_fds = (0..current_num_threads())
        .map(|i| {
            input
                .ard_file()
                .with_context(|| format!("failed to open ARD for thread {i}"))
        })
        .collect::<Result<Vec<_>>>()?;

    arh_paths.into_par_iter().try_for_each(|path| {
        let Some(file) = fs.get_file_info(&path) else {
            unreachable!()
        };
        let out_path = root_out.join(&path.as_str()[1..]);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dir {parent:?}"))?;
        }
        let mut ard_file = &thread_fds[current_thread_index().unwrap()];
        ard_file.rewind()?;
        ArdAccess::File(ard_file.try_clone()?)
            .copy_to(&out_path, file)
            .with_context(|| format!("failed to extract {path}"))?;
        progress.inc(1);
        Ok::<(), anyhow::Error>(())
    })?;
    progress.finish();
    let elapsed = start.elapsed();
    println!("Extraction completed in {} seconds.", elapsed.as_secs_f64());

    Ok(())
}

impl<'b> ArdAccess<'b> {
    fn copy_to(&self, out_path: &Path, file: &FileMeta) -> Result<()> {
        // Here one alternative for uncompressed files could be to use sendfile(2) between the
        // ard and output fds
        let buf = match self {
            ArdAccess::File(ard) => ArdReader::new(BufReader::new(ard)).entry(file).read(),
            ArdAccess::Mem(ard) => ArdReader::new(Cursor::new(ard)).entry(file).read(),
        }?;
        Ok(std::fs::write(out_path, buf)?)
    }
}
