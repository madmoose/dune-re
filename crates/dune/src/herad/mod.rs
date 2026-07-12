pub mod dnadl;

use std::io::{Cursor, Read, Result};

use bytes_ext::ReadBytesExt;
pub use dnadl::HeradADL;

pub enum Format {
    SoundBlaster,
    Adlib,
    AdlibGold,
    MT32,
}

#[derive(Debug)]
pub struct File {
    pub instruments_offset: u16,
    pub track_offsets: [u16; 21],
    pub loop_start: u16,
    pub loop_end: u16,
    pub loop_count: u16,
    pub speed: u16,
    pub adlib_gold_regs: Option<[u8; 32]>,
}

impl File {
    pub fn new(buf: &[u8]) -> Result<Self> {
        let mut r = Cursor::new(buf);

        let instruments_offset = r.read_le_u16()?;

        let mut track_offsets = [0u16; 21];
        for track_offset in &mut track_offsets {
            *track_offset = r.read_le_u16()?;
            if *track_offset != 0 {
                *track_offset += 2;
            }
        }

        let loop_start = r.read_le_u16()?;
        let loop_end = r.read_le_u16()?;
        let loop_count = r.read_le_u16()?;
        let speed = r.read_le_u16()?;

        let first_track_offset = track_offsets[0] as u64 + 2;
        let adlib_gold_regs = if first_track_offset >= r.position() + 32 {
            let mut adlib_gold_regs = [0u8; 32];
            r.read_exact(&mut adlib_gold_regs)?;
            Some(adlib_gold_regs)
        } else {
            None
        };

        Ok(File {
            instruments_offset,
            track_offsets,
            loop_start,
            loop_end,
            loop_count,
            speed,
            adlib_gold_regs,
        })
    }
}
