use std::{
    fs::File,
    io::{BufReader, Cursor},
};

use ardain::{
    file_alloc::{ArdFileAllocator, CompressionStrategy},
    ArdReader, ArdWriter, ArhFileSystem,
};

#[test]
fn read_write() {
    let ard_path = "tests/res/bf3_dlc04.ard";
    let mut arh = load_arh();
    let mut ard_file = File::open(ard_path).unwrap();
    let mut reader = ArdReader::new(BufReader::new(&mut ard_file));

    let mut buf = Cursor::new(std::fs::read(ard_path).unwrap());
    let mut writer = ArdWriter::new(&mut buf);

    let btl_bdat = arh.get_file_info("/bdat/btl.bdat").unwrap().id;
    let new_file = arh.create_file("/test_file").unwrap().id;
    let mut allocator = ArdFileAllocator::new(&mut arh, &mut writer);
    allocator
        .write_new_file(new_file, &[0, 1, 2, 3, 4, 5], CompressionStrategy::None)
        .unwrap();
    allocator
        .replace_file(
            btl_bdat,
            &[100, 101, 102, 103, 104, 105],
            CompressionStrategy::None,
        )
        .unwrap();

    buf.set_position(0);
    let bdat_read_back = ArdReader::new(&mut buf)
        .entry(arh.get_file_info("/bdat/btl.bdat").unwrap())
        .read()
        .unwrap();
    buf.set_position(0);
    let new_read_back = ArdReader::new(&mut buf)
        .entry(arh.get_file_info("/test_file").unwrap())
        .read()
        .unwrap();
    assert_eq!(&new_read_back, &[0, 1, 2, 3, 4, 5]);
    assert_eq!(&bdat_read_back, &[100, 101, 102, 103, 104, 105]);
}

fn load_arh() -> ArhFileSystem {
    ArhFileSystem::load(File::open("tests/res/bf3_dlc04.arh").unwrap()).unwrap()
}
