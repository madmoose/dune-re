use std::io::{Cursor, Seek};

use bytes_ext::ReadBytesExt;

use crate::fixed_point::FixedU16F16;

#[derive(Copy, Clone, Debug, Default)]
pub struct TablatEntry {
    offset: u16,
    len: u16,
    fp: FixedU16F16, // Fixed point
}

#[derive(Debug)]
pub struct Tablat {
    table: [TablatEntry; 99],
}

impl Tablat {
    pub fn new(data: &[u8; 792]) -> Self {
        let mut table = [TablatEntry::default(); 99];

        let mut c = Cursor::new(data);
        for e in &mut table {
            let offset = c.read_be_u16().unwrap();
            let len = c.read_be_u16().unwrap();
            c.seek_relative(4).unwrap();

            e.offset = offset;
            e.len = len;
        }

        Tablat { table }
    }

    fn entry(&self, y: u16) -> &TablatEntry {
        if y < 99 {
            &self.table[(98 - y) as usize]
        } else {
            &self.table[(y - 98) as usize]
        }
    }

    pub fn offset(&self, y: u16) -> u16 {
        if y < 99 {
            0x62fc - self.entry(y).offset
        } else {
            0x62fc + self.entry(y).offset
        }
    }

    pub fn len(&self, y: u16) -> u16 {
        2 * self.entry(y).len
    }

    pub fn rotated_offset(&self, y: u16) -> FixedU16F16 {
        self.entry(y).fp
    }

    pub fn set_rotated_offset(&mut self, y: u16, fp: FixedU16F16) {
        self.table[if y < 99 {
            (98 - y) as usize
        } else {
            (y - 98) as usize
        }]
        .fp = fp;
    }

    pub fn yx_for_offset(&self, offset: u16) -> Option<(u16, u16)> {
        for y in 0..197 {
            let y_offset = self.offset(y);
            if offset >= y_offset && offset < y_offset + self.len(y) {
                return Some((y, offset - y_offset));
            }
        }
        None
    }
}
