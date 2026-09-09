//! DUNE.DAT container reader. Every resource the port opens is read whole
//! into a `Vec<u8>` from here, so DOS's fixed resource cache, its page-slot
//! allocator and the XMS/EMS drivers behind it have no counterpart; those
//! routines are listed in NOT_NEEDED.md.

use std::{
    fs::File,
    io::{BufReader, Cursor, ErrorKind, Read, Seek},
    path::Path,
};

use bytes_ext::ReadBytesExt;

use crate::hsq;

pub struct DatFile {
    reader: BufReader<File>,
    pub entries: Vec<DatEntry>,
}

#[derive(Debug)]
pub struct DatEntry {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

type Error = std::io::Error;

impl DatFile {
    // = seg000:e675 open_dune_dat / seg000:e741 read_dune_dat_toc — open
    // DUNE.DAT and read its table of contents (DOS reads the whole 64K TOC
    // block after seeking to 0): entry count, then per entry a 16-byte name,
    // u32 size, u32 offset and a flag byte. DOS also folds the entries into
    // its fixed resource index table; the port resolves entries by name.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<DatFile, Error> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        let entry_count = reader.read_le_u16()? as usize;
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let name = reader.read_fixed_str(16)?;
            let size = reader.read_le_u32()? as usize;
            let offset = reader.read_le_u32()? as usize;
            _ = reader.read_u8();

            if name.is_empty() {
                break;
            }

            entries.push(DatEntry { name, size, offset });
        }

        Ok(DatFile { reader, entries })
    }

    // = seg000:f244 read_resource_to_esdi / seg000:f229 open_res_or_file_or_die / seg000:f1fb open_res_or_file_with_name_in_dx_size_ax
    // — read one entry of DUNE.DAT whole. A name missing from the DAT falls
    // back to a loose file on disk there, and is an error here. The steps DOS
    // splits out:
    // = seg000:f314 get_res_index_in_ax_by_name_dssi / seg000:f3a7 res_locate_in_lookup_table
    //   — the name match and the DAT index lookup (`entries.iter().find`).
    // = seg000:f2a7 seek_dune_dat_to_res_dsdx / seg000:f2d6 seek_dune_dat_offset_dxax
    //   — the seek to the entry's offset.
    // = seg000:f2ea read_dune_dat_cx_to_esdi — the read of `size` bytes.
    pub fn read_raw(&mut self, name: &str) -> Result<Box<[u8]>, Error> {
        let entry = self
            .entries
            .iter()
            .find(|&e| e.name == name)
            .ok_or(Error::from(ErrorKind::NotFound))?;

        self.reader
            .seek(std::io::SeekFrom::Start(entry.offset as u64))?;

        let mut data = vec![0; entry.size];
        self.reader.read_exact(data.as_mut_slice())?;

        Ok(data.into())
    }

    // = seg000:f0d6 read_and_maybe_hsq / seg000:f0b9 open_resource_by_index_si_into_esdi
    // — read an entry and unpack it when its six-byte header says HSQ. DOS
    // reserves the bump heap around the read (alloc_check_cx_pages_available,
    // bump_allocate_bump_cx_bytes) and takes the name from the resource table
    // by index; the port's callers pass the name.
    pub fn read(&mut self, name: &str) -> Result<Box<[u8]>, Error> {
        let data = self.read_raw(name)?;

        let mut reader = Cursor::new(&data);
        let header = hsq::Header::from_reader(&mut reader)?;

        if !header.is_compressed() {
            return Ok(data);
        }

        if header.compressed_size() as usize != data.len() {
            println!("Packed length does not match resource size");
            return Ok(data);
        }

        let mut unpacked_data = vec![0; header.uncompressed_size() as usize];
        let mut writer = Cursor::new(&mut unpacked_data);

        hsq::unhsq(&data[6..], &mut writer).unwrap();
        Ok(unpacked_data.into())
    }
}
