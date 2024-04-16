use std::{collections::VecDeque, fs::File, io::Cursor};

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
        "/bdat/test.bdat50",
        "/bdat/endpath.1",
        "/bdat/endpath.2",
        "/bdat/endpath.48",
        "/root.txt",
        "/noext",
        "/a/very/long/directory/path/file.txt",
    ];
    for f in files {
        arh.create_file(f).unwrap();
        check_and_read_back(&mut arh, |arh| {
            println!("Checking after adding {f}");
            check_reachable(&arh);
        });
    }
}

#[test]
#[should_panic = "FsFileNameExtended"]
fn create_error_extended() {
    let mut arh = load_arh();
    let files = ["/file.tar", "/file.tar.gz"];
    for f in files {
        arh.create_file(f).unwrap();
        check_and_read_back(&mut arh, |arh| {
            println!("Checking after adding {f}");
            check_reachable(&arh);
        });
    }
}

#[test]
#[should_panic = "FsFileNameExtended"]
fn create_error_into_extended() {
    let mut arh = load_arh();
    arh.create_file("/bdat/fld.bd").unwrap();
    check_and_read_back(&mut arh, |arh| check_reachable(&arh));
}

#[test]
fn delete_files() {
    let mut arh = load_arh();
    let files = [
        "/bdat/btl.bdat",
        "/bdat/fld.bdat",
        "/chr/tex/nx/m/fe85e8cc.wismt",
        "/map/ma66a.wismhd",
        "/map/ma66a.wismda",
        "/data_sheet/data_sheet.bin",
    ];
    let create_and_delete = [
        "/bdat/test.bdat2",
        "/bdat/test.bdat3",
        "/bdat/test.bdat4",
        "/bdat/test.bdat50",
        "/bdat/btl.bdat",
        "/in_root",
    ];
    for f in files {
        arh.delete_file(f).unwrap();
        check_and_read_back(&mut arh, |arh| {
            println!("Checking that {f} is no longer reachable");
            assert!(!arh.is_file(f));
            println!("Checking reachable after removing {f}");
            check_reachable(&arh);
        });
    }
    for f in create_and_delete {
        arh.create_file(f).unwrap();
        check_and_read_back(&mut arh, |arh| {
            println!("Checking that {f} is now reachable");
            assert!(arh.is_file(f));
            println!("Checking reachable after adding {f}");
            check_reachable(&arh);
        });
    }
    for f in create_and_delete.iter().rev() {
        arh.delete_file(f).unwrap();
        check_and_read_back(&mut arh, |arh| {
            println!("Checking that {f} is no longer reachable");
            assert!(!arh.is_file(f));
            println!("Checking reachable after removing {f}");
            check_reachable(&arh);
        });
    }
}

#[test]
fn rename_files() {
    let mut arh = load_arh();
    let files = [
        "/bdat/btl.bdat",
        "/bdat/fld.bdat",
        "/chr/tex/nx/m/fe85e8cc.wismt",
        "/map/ma66a.wismhd",
        "/map/ma66a.wismda",
        "/data_sheet/data_sheet.bin",
    ];
    for f in files {
        // Rename each file to the reverse of its components
        // (e.g. "/bdat/btl.bdat" -> "/tadb/tadb.ltb")
        let reverse_path = format!(
            "/{}",
            f.split("/")
                .map(|s| {
                    s.chars()
                        .rev()
                        .chain(std::iter::once('/'))
                        .collect::<String>()
                })
                .collect::<String>()
        );
        let reverse_path = &reverse_path[..reverse_path.len() - 1];
        println!("Checking that {f} was reachable");
        let meta = *arh.get_file_info(f).unwrap();
        arh.rename_file(f, &reverse_path).unwrap();
        check_and_read_back(&mut arh, |arh| {
            println!("Checking that {f} is no longer reachable");
            assert!(!arh.is_file(f));
            println!("Checking that {reverse_path} is now reachable");
            let new_meta = *arh.get_file_info(reverse_path).unwrap();
            assert_eq!(meta, new_meta);
            println!("Checking reachable after renaming {f}");
            check_reachable(&arh);
        });
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
                assert!(arh.is_file(path), "{path} does not exist");
            }
            DirEntry::Directory { children } => {
                for child in children {
                    queue.push_back((child, format!("{path}/{}", node.name)));
                }
            }
        }
    }
}

fn check_and_read_back(arh: &mut ArhFileSystem, check_fn: impl Fn(&mut ArhFileSystem)) {
    check_fn(arh);
    let mut out_arh = Cursor::new(Vec::new());
    arh.sync(&mut out_arh).expect("arh write");
    out_arh.set_position(0);
    let mut new_arh = ArhFileSystem::load(out_arh).expect("arh read back");
    check_fn(&mut new_arh);
}

fn load_arh() -> ArhFileSystem {
    ArhFileSystem::load(File::open("tests/res/bf3.arh").unwrap()).unwrap()
}
