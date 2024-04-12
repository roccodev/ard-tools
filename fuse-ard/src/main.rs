use std::{env, fs::File};

use fs::ArhFuseSystem;
use fuser::MountOption;

mod fs;

fn main() {
    env_logger::init();
    let arh_path = env::args().nth(1).unwrap();
    let mount_point = env::args().nth(2).unwrap();
    let arh = File::open(arh_path).unwrap();
    let fs = ArhFuseSystem::load(arh).unwrap();
    let opts = vec![MountOption::RO, MountOption::CUSTOM("debug".to_string())];
    fuser::mount2(fs, mount_point, &opts).unwrap();
}
