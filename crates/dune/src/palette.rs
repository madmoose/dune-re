use std::io::{Cursor, Seek};

use bytes_ext::ReadBytesExt;

use crate::Color;

#[derive(Debug, Clone)]
pub struct Palette([Color; 256]);

pub fn scale_6bit_to_8bit(c: u8) -> u8 {
    (255 * (c as u16) / 63) as u8
}

pub fn _scale_8bit_to_6bit(c: u8) -> u8 {
    (63 * (c as u16) / 255) as u8
}

impl Palette {
    pub fn new() -> Self {
        Self([Color::default(); 256])
    }

    pub fn clear(&mut self) {
        for i in 0..256 {
            self.set(i, Color::default());
        }
    }

    pub fn get(&self, i: usize) -> Color {
        self.0[i]
    }

    pub fn get_rgb888(&self, i: usize) -> Color {
        let c = self.0[i];

        Color(
            scale_6bit_to_8bit(c.0),
            scale_6bit_to_8bit(c.1),
            scale_6bit_to_8bit(c.2),
        )
    }

    pub fn set(&mut self, i: usize, rgb: Color) {
        self.0[i] = rgb;
    }

    pub fn set_all(&mut self, pal: &[u8; 768]) {
        for i in 0..256 {
            self.set(i, Color(pal[3 * i + 0], pal[3 * i + 1], pal[3 * i + 2]))
        }
    }

    pub fn set_all_from_rgb666(&mut self, pal: &[u8; 768]) {
        for i in 0..256 {
            let r = ((pal[3 * i + 0] as u32) * 63 / 255) as u8;
            let g = ((pal[3 * i + 1] as u32) * 63 / 255) as u8;
            let b = ((pal[3 * i + 2] as u32) * 63 / 255) as u8;
            self.set(i, Color(r, g, b));
        }
    }

    pub fn as_slice(&self) -> &[Color; 256] {
        &self.0
    }

    pub fn as_mut_slice(&mut self) -> &mut [Color; 256] {
        &mut self.0
    }

    pub fn apply_palette_update(&mut self, data: &[u8]) -> Result<u64, std::io::Error> {
        let mut r = Cursor::new(data);

        loop {
            let read_u8 = r.read_u8();
            let index = read_u8? as usize;
            let mut count = r.read_u8()? as usize;

            // = seg000:c1bf `cmp ax, 100h; jnz ...; add si, 3` — the chunk
            // (index 0, count 1) is skipped, not applied. lodsw reads the pair
            // little-endian as ax = index | (count << 8), so 0x0100 means
            // index==0, count==1. Sprite sheets carry a placeholder colour here
            // (EQUI's is grey 22,22,22); skipping it preserves the global black
            // at palette index 0, which is what the cleared-framebuffer borders
            // (rows 0..23 and 176..199) display.
            if index == 0 && count == 1 {
                r.seek_relative(3)?;
                continue;
            }
            if index == 0xff && count == 0xff {
                break;
            }
            if count == 0 {
                count = 256;
            }

            for i in 0..count {
                let cr = r.read_u8()?;
                let cg = r.read_u8()?;
                let cb = r.read_u8()?;

                if index + i <= 255 {
                    self.set(index + i, Color(cr, cg, cb));
                }
            }
        }

        loop {
            match r.read_u8() {
                Ok(0xff) => {}
                Ok(_) => {
                    r.seek_relative(-1)?;
                    break;
                }
                Err(_) => {
                    break;
                }
            }
        }

        Ok(r.position())
    }

    pub fn find_closest_color(&self, color: Color) -> u8 {
        let mut best_index = 0;
        let mut best_distance = u32::MAX;

        for (i, &c) in self.0.iter().enumerate() {
            let dr = c.0 as i16 - color.0 as i16;
            let dg = c.1 as i16 - color.1 as i16;
            let db = c.2 as i16 - color.2 as i16;

            let distance = (dr * dr + dg * dg + db * db) as u32;

            if distance < best_distance {
                best_distance = distance;
                best_index = i;
            }
        }

        best_index as u8
    }

    pub fn copy_from(&mut self, other: &Self) {
        self.0.copy_from_slice(&other.0);
    }

    /// Write the pallette as a 128x128 ppm, each color a 8x8 block.
    pub fn write_png_grid<P: AsRef<std::path::Path>>(&self, path: P) -> std::io::Result<()> {
        use std::{fs::File, io::BufWriter};

        const BLOCK: usize = 8;
        const GRID: usize = 16;
        const DIM: usize = GRID * BLOCK; // 128

        let mut rgba = vec![0u8; DIM * DIM * 4];
        for y in 0..DIM {
            for x in 0..DIM {
                let color_index = (y / BLOCK) * GRID + (x / BLOCK);
                let color = self.get_rgb888(color_index);
                let o = 4 * (y * DIM + x);
                rgba[o] = color.0;
                rgba[o + 1] = color.1;
                rgba[o + 2] = color.2;
                rgba[o + 3] = 255;
            }
        }

        let file = File::create(path.as_ref())?;
        let buf = &mut BufWriter::new(file);
        let mut encoder = png::Encoder::new(buf, DIM as u32, DIM as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgba)?;

        Ok(())
    }

    pub fn write_as_ppm(&self, filename: &str) {
        use std::{fs::File, io::Write};

        let mut file = File::create(filename).expect("Failed to create file");

        writeln!(file, "P6 256 256 255").expect("Failed to write header");

        for block_y in 0..16 {
            for _ in 0..16 {
                for block_x in 0..16 {
                    let color_index = block_y * 16 + block_x;
                    let color = self.get_rgb888(color_index);

                    for _ in 0..16 {
                        file.write_all(&[color.0, color.1, color.2])
                            .expect("Failed to write pixel data");
                    }
                }
            }
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}
