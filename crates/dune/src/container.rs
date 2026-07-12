use std::io::Cursor;

use bytes_ext::ReadBytesExt;

#[allow(unused)]
pub fn entry_count(blob: &[u8]) -> u16 {
    let mut c = Cursor::new(blob);
    let first_entry_offset = c
        .read_le_u16()
        .expect("container::entry_count: failed to read first entry offset");

    first_entry_offset / 2
}

pub fn entry_offset(blob: &[u8], index: u16) -> u16 {
    let count = entry_count(blob);

    assert!(
        index < count,
        "container::entry_ofs: invalid entry index ({index} >= {count})"
    );

    let mut c = Cursor::new(blob);
    c.set_position(2 * index as u64);

    c.read_le_u16()
        .expect("container::entry_ofs: failed to read entry offset")
}

fn entry_end(blob: &[u8], index: u16) -> u16 {
    let count = entry_count(blob);

    assert!(
        index < count,
        "container::entry_end: invalid entry index ({index} >= {count})"
    );

    let mut c = Cursor::new(blob);

    if index < count - 1 {
        c.set_position(2 * (index as u64 + 1));
        c.read_le_u16()
            .expect("container::entry_end: failed to read next entry offset")
    } else {
        blob.len() as u16
    }
}

pub fn entry_byte_range(blob: &[u8], index: u16) -> (u16, u16) {
    let count = entry_count(blob);

    assert!(
        index < count,
        "container::entry_byte_range: invalid entry index ({index} >= {count})"
    );

    let ofs = entry_offset(blob, index);
    let end = entry_end(blob, index);

    (ofs, end)
}

pub fn entry(blob: &[u8], index: u16) -> &[u8] {
    let (ofs, end) = entry_byte_range(blob, index);

    &blob[ofs as usize..end as usize]
}

pub fn read_u8(blob: &[u8], ofs: u16) -> u8 {
    blob[ofs as usize]
}

pub fn read_le_u16(blob: &[u8], ofs: u16) -> u16 {
    let mut c = Cursor::new(blob);
    c.set_position(ofs as u64);

    c.read_le_u16()
        .expect("container::word: failed to read word")
}
