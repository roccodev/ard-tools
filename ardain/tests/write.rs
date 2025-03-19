use std::{fs::File, io::Cursor};

use ardain::{
    file_alloc::{ArdFileAllocator, CompressionStrategy},
    path::ArhPath,
    ArdReader, ArdWriter, Arh1FileSystem,
};
use xc3_lib::xbc1::CompressionType;

#[test]
fn read_write() {
    let ard_path = "tests/res/bf3_dlc04.ard";
    let mut arh = load_arh();

    let mut buf = Cursor::new(std::fs::read(ard_path).unwrap());
    let mut writer = ArdWriter::new(&mut buf);

    let btl_path = ArhPath::normalize("/bdat/btl.bdat").unwrap();
    let new_path = ArhPath::normalize("test_file").unwrap();

    let btl_bdat: u32 = arh
        .get_file_info(&btl_path)
        .unwrap()
        .unique_id
        .try_into()
        .unwrap();
    let new_file: u32 = arh
        .create_file(&new_path)
        .unwrap()
        .unique_id
        .try_into()
        .unwrap();
    let mut allocator = ArdFileAllocator::new(&mut arh, &mut writer);
    allocator
        .write_new_file(
            new_file,
            &[0, 1, 2, 3, 4, 5],
            CompressionStrategy::Standard(CompressionType::Zlib),
        )
        .unwrap();
    allocator
        .replace_file(
            btl_bdat,
            &[100, 101, 102, 103, 104, 105],
            CompressionStrategy::Standard(CompressionType::Zstd),
        )
        .unwrap();

    buf.set_position(0);
    let bdat_read_back = ArdReader::new(&mut buf)
        .entry(&arh.get_file_info(&btl_path).unwrap())
        .read()
        .unwrap();
    buf.set_position(0);
    let new_read_back = ArdReader::new(&mut buf)
        .entry(&arh.get_file_info(&new_path).unwrap())
        .read()
        .unwrap();
    assert_eq!(&new_read_back, &[0, 1, 2, 3, 4, 5]);
    assert_eq!(&bdat_read_back, &[100, 101, 102, 103, 104, 105]);
}

fn load_arh() -> Arh1FileSystem {
    Arh1FileSystem::load(File::open("tests/res/bf3_dlc04.arh").unwrap()).unwrap()
}
