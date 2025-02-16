use std::{
    fs::File,
    io::{BufReader, BufWriter},
};

use anyhow::{anyhow, Result};
use ardain::{path::ArhPath, ArhFileSystem};
use clap::{command, Args, Parser, Subcommand};

mod extract;
mod ls;
mod rm;

#[derive(Parser)]
#[command(
    author,
    version,
    about,
    arg_required_else_help = true,
    subcommand_required = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[clap(flatten)]
    input: InputData,
}

#[derive(Args)]
struct InputData {
    /// Input .arh file, required for most commands
    #[arg(long = "arh", global = true)]
    in_arh: Option<String>,
    /// Input .ard file (data archive)
    #[arg(long = "ard", global = true)]
    in_ard: Option<String>,
    /// Output .arh file, for commands that write data and metadata. If absent, the input
    /// .arh file will be overwritten!
    #[arg(long = "out-arh", global = true)]
    out_arh: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all files in a directory
    #[clap(visible_alias = "ls")]
    List(ls::ListArgs),
    /// Remove files or directories
    #[clap(visible_alias = "rm")]
    Remove(rm::RemoveArgs),
    #[clap(visible_alias = "x")]
    Extract(extract::ExtractArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::List(args)) => ls::run(&cli.input, args),
        Some(Commands::Remove(args)) => rm::run(&cli.input, args),
        Some(Commands::Extract(args)) => extract::run(&cli.input, args),
        _ => Ok(()),
    }
}

impl InputData {
    pub fn load_fs(&self) -> Result<ArhFileSystem> {
        match &self.in_arh {
            Some(path) => Ok(ArhFileSystem::load(BufReader::new(File::open(path)?))?),
            None => Err(anyhow!("input .arh must be passed in as --arh")),
        }
    }

    pub fn write_fs(&self, fs: &mut ArhFileSystem) -> Result<()> {
        match self.out_arh.as_ref().or(self.in_arh.as_ref()) {
            Some(path) => Ok(fs.sync(BufWriter::new(File::create(path)?))?),
            None => Err(anyhow!("input .arh must be passed in as --arh")),
        }
    }

    pub fn ard_file(&self) -> Result<File> {
        match &self.in_ard {
            Some(path) => Ok(File::open(path)?),
            None => Err(anyhow!("input .ard must be passed in as --ard")),
        }
    }
}

pub(crate) fn parse_path(s: &str) -> Result<ArhPath> {
    Ok(ArhPath::normalize(s)?)
}
