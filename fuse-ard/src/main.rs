use std::fs::File;

use clap::{arg, Command};
use env_logger::Env;
use fs::ArhFuseSystem;
use fuser::MountOption;

mod fs;

fn main() {
    let cmd = Command::new("fuse-ard")
        .arg(arg!([mount_point] "where to mount the archive, e.g. /mnt/ard").required(true))
        .arg(arg!(--arh <FILE> "path to the .arh file").required(true))
        .arg(arg!(--ard <FILE> "path to the .ard file").required(true))
        .arg(arg!(-r --readonly "mount the archive as read-only"))
        .arg(arg!(-d --debug "enable FUSE debugging and debug logs"));
    let matches = cmd.get_matches();

    let debug = matches.get_flag("debug");
    env_logger::Builder::from_env(Env::default().default_filter_or(if debug {
        "debug"
    } else {
        "warn"
    }))
    .init();

    let arh = File::open(matches.get_one::<String>("arh").unwrap()).unwrap();
    let ard = File::open(matches.get_one::<String>("ard").unwrap()).unwrap();
    let fs = ArhFuseSystem::load(arh, ard).unwrap();

    let mount_point = matches.get_one::<String>("mount_point").unwrap();
    let mut opts = vec![
        MountOption::RO,
        MountOption::CUSTOM("kernel_cache".to_string()),
    ];
    if debug {
        opts.push(MountOption::CUSTOM("debug".to_string()));
    }
    fuser::mount2(fs, mount_point, &opts).unwrap();
}
