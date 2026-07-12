use std::{
    borrow::Cow,
    io::{Cursor, Seek},
};

use bytes_ext::ReadBytesExt;

use crate::{FrameBuffer, Palette, blit, hnm::frame_header::FrameHeader, hsq};

/// HNM video decoder
pub struct HnmDecoder<'a> {
    data: Cow<'a, [u8]>,
    header_size: u16,
    frame_offsets: Vec<u32>,
    buffer: Vec<u8>,
    next_frame: usize,
    last_sd_block: Option<Vec<u8>>,
}

impl<'a> HnmDecoder<'a> {
    /// Creates a new empty decoder
    pub fn new<B>(bytes: B, pal: &mut Palette) -> std::io::Result<Self>
    where
        B: Into<Cow<'a, [u8]>>,
    {
        let data = bytes.into();

        let mut r = Cursor::new(&data[..]);
        let header_size = r.read_le_u16()?;

        let pal_size = pal.apply_palette_update(&data[2..])?;
        r.seek_relative(pal_size as i64)?;

        let frame_table_pos = r.position();
        let frame_count = (header_size as u64 - frame_table_pos) / 4;

        let mut frame_offsets = Vec::with_capacity(frame_count as usize);

        for _ in 0..frame_count {
            let offset = r.read_le_u32()?;
            frame_offsets.push(offset);
        }

        Ok(Self {
            data,
            header_size,
            frame_offsets,
            buffer: Vec::new(),
            next_frame: 0,
            last_sd_block: None,
        })
    }

    /// Returns the number of frames in the loaded video
    pub fn frame_count(&self) -> usize {
        self.frame_offsets.len().saturating_sub(1)
    }

    /// Returns the current frame index
    pub fn current_frame(&self) -> usize {
        self.next_frame
    }

    /// Seeks to a specific frame
    ///
    /// # Arguments
    /// * `frame` - Frame index to seek to (0-based)
    ///
    /// # Returns
    /// `Ok(())` if the frame index is valid, `Err` otherwise
    pub fn seek_frame(&mut self, frame: usize) -> std::io::Result<()> {
        if frame >= self.frame_count() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Frame {} out of range (max {})",
                    frame,
                    self.frame_count() - 1
                ),
            ));
        }
        self.next_frame = frame;
        Ok(())
    }

    /// Resets playback to the first frame
    pub fn reset(&mut self) {
        self.next_frame = 0;
    }

    /// Returns true if all frames have been decoded
    pub fn is_complete(&self) -> bool {
        assert!(self.next_frame <= self.frame_count());
        self.next_frame == self.frame_count()
    }

    pub fn take_sd_block(&mut self) -> Option<Vec<u8>> {
        self.last_sd_block.take()
    }

    /// Decodes the next frame to the framebuffer and advances the frame counter.
    ///
    /// # Arguments
    /// * `framebuffer` - Target framebuffer to decode into
    /// * `pal` - Palette that may be updated by the frame
    /// * `fb_y_offset` - Logical-to-physical y offset applied to every blit
    ///   destination, mirroring DOS `fb_base_ofs` (set by `vga_set_fb_row`).
    ///   The HNM frame header's logical y is added to this value before the
    ///   blit so e.g. a full-screen frame at logical (0, 0) lands at fb
    ///   (0, fb_y_offset).
    ///
    /// # Returns
    /// `Ok(())` on success, or an error if decoding fails
    pub fn decode_frame(
        &mut self,
        framebuffer: &mut FrameBuffer,
        pal: &mut Palette,
        fb_y_offset: i16,
    ) -> std::io::Result<(u16, u16)> {
        assert!(!self.is_complete());

        let frame = self.next_frame;
        let frame_pos = self.header_size as usize + self.frame_offsets[frame] as usize;

        if frame_pos >= self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "Frame position {} exceeds data length {}",
                    frame_pos,
                    self.data.len()
                ),
            ));
        }

        let mut r = Cursor::new(&self.data[frame_pos..]);
        let frame_size = r.read_le_u16()?;

        let frame_end = frame_pos + frame_size as usize;
        if frame_end > self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "Frame end {} exceeds data length {}",
                    frame_end,
                    self.data.len()
                ),
            ));
        }

        let mut r = Cursor::new(&self.data[frame_pos + 2..frame_end]);

        const BLOCK_TYPE_SD: u16 = 0x7364;
        const BLOCK_TYPE_PL: u16 = 0x706C;

        let mut w = 0;
        let mut h = 0;

        loop {
            let block_type = r.read_be_u16()?;

            match block_type {
                BLOCK_TYPE_SD => {
                    let block_size = r.read_le_u16()?;
                    let mut sd_data = vec![0u8; block_size as usize - 4];
                    std::io::Read::read_exact(&mut r, &mut sd_data)?;
                    self.last_sd_block = Some(sd_data);
                }
                BLOCK_TYPE_PL => {
                    let block_size = r.read_le_u16()?;
                    let pal_data = &r.get_ref()[r.position() as usize..];

                    pal.apply_palette_update(pal_data)?;

                    r.seek_relative(block_size as i64 - 4)?;
                }
                _ => {
                    r.seek_relative(-2)?;
                    let frame_header = FrameHeader::new(&mut r)?;

                    if frame_header.is_empty() {
                        break;
                    }

                    w = frame_header.width();
                    h = frame_header.height();

                    if frame_header.is_compressed() {
                        r.seek_relative(6)?;
                        let mut w = Cursor::new(&mut self.buffer);
                        hsq::unhsq(r, &mut w)?;
                        r = Cursor::new(&self.buffer);
                    };

                    let (x, y) = if frame_header.is_full_frame() {
                        (0, 0)
                    } else {
                        (r.read_le_i16()?, r.read_le_i16()?)
                    };

                    let data = &r.get_ref()[r.position() as usize..];

                    blit::Blitter::new(data, framebuffer)
                        .at(x, y + fb_y_offset)
                        .size(w, h)
                        .rle(frame_header.is_rle())
                        .pal_offset(frame_header.mode())
                        .draw()?;

                    break;
                }
            }
        }

        self.next_frame += 1;

        // println!("decoder: ({w}, {h})");

        Ok((w, h))
    }
}
