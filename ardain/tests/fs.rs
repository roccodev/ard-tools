use std::{collections::VecDeque, fs::File};

use ardain::{ArhFileSystem, DirEntry};

#[test]
fn check_initial_reachable() {
    let arh = load_arh();
    check_reachable(&arh)
}

#[test]
fn create_files() {
    let mut arh = load_arh();
    let files = [
        "/bdat/test.bdat2",
        "/bdat/test.bdat3",
        "/bdat/test.bdat4",
        "/bdat/endpath.1",
        "/bdat/endpath.2",
        "/bdat/endpath.48",
        "/root.txt",
        "/noext",
        "/a/very/long/directory/path/file.txt",
    ];
    for f in files {
        arh.create_file(f).unwrap();
        println!("Checking after adding {f}");
        check_reachable(&arh);
    }
}

fn check_reachable(arh: &ArhFileSystem) {
    let node = arh.get_dir("/").unwrap();
    let mut queue = VecDeque::new();
    queue.push_back((node, "".to_string()));
    while let Some((node, path)) = queue.pop_back() {
        match &node.entry {
            DirEntry::File => {
                let path = &format!("{path}/{}", node.name)[2..];
                assert!(
                    arh.get_file_info(path).is_some(),
                    "{path} did not match file infos"
                );
            }
            DirEntry::Directory { children } => {
                for child in children {
                    queue.push_back((child, format!("{path}/{}", node.name)));
                }
            }
        }
    }
}

fn load_arh() -> ArhFileSystem {
    ArhFileSystem::load(File::open("tests/res/bf3.arh").unwrap()).unwrap()
}
