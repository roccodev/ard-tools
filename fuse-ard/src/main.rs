use std::fs::File;

use clap::{arg, Command};
use env_logger::Env;
use fs::ArhFuseSystem;
use fuser::MountOption;

mod error;
mod fs;

fn main() {
    let cmd = Command::new("fuse-ard")
        .arg(arg!([mount_point] "where to mount the archive, e.g. /mnt/ard").required(true))
        .arg(arg!(--arh <FILE> "path to the .arh file").required(true))
        .arg(arg!(--ard <FILE> "path to the .ard file. If absent, some operations won't be available. Note that the .ard file will always be overwritten unless --readonly is present!"))
        .arg(arg!(--arhout <FILE> "path to the .arh file to write modifications to. If absent, the main .arh file will be overwritten!"))
        .arg(arg!(-r --readonly "mount the archive as read-only"))
        .arg(arg!(-d --debug "enable FUSE debugging and debug logs"));
    let matches = cmd.get_matches();

    let debug = matches.get_flag("debug");
    env_logger::Builder::from_env(Env::default().default_filter_or(if debug {
        "debug"
    } else {
        "info"
    }))
    .init();

    let arh_path = matches.get_one::<String>("arh").unwrap();
    let arh = File::open(&arh_path).unwrap();
    let ard = matches
        .get_one::<String>("ard")
        .map(|path| File::open(path).unwrap());
    let out_arh = matches
        .get_one::<String>("arhout")
        .unwrap_or_else(|| &arh_path);
    let fs = ArhFuseSystem::load(arh, ard, out_arh).unwrap();

    let mount_point = matches.get_one::<String>("mount_point").unwrap();
    let mut opts = vec![
        MountOption::NoExec,
        MountOption::NoAtime,
        MountOption::CUSTOM("kernel_cache".to_string()),
    ];
    if debug {
        opts.push(MountOption::CUSTOM("debug".to_string()));
    }
    if matches.get_flag("readonly") {
        opts.push(MountOption::RO);
    }
    fuser::mount2(fs, mount_point, &opts).unwrap();
}
