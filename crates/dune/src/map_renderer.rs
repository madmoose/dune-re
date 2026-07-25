//! = segvga:1f4c vga_draw_map_zoomed — the SEE DUNE MAP full-planet renderer.
//!
//! Draws the whole planet as a flat map into the full-map window
//! (seg001:1482 full_map_view_rect, (4,4)-(316,148)): 36 latitude bands of 4
//! rows each, 312 px wide. Every band interpolates one raw MAP.HSQ row pair
//! (4 output px per map cell horizontally, 4 rows per band vertically with
//! error-spread row duplication) through the RESOURCE_GLOBDATA scratch
//! buffer, shades the curved west/east planet edges, and stores the band
//! into the framebuffer through an absolute destination cursor
//! (data_segvga_01cb2, seeded 0x504 = (4,4) — no fb_base_ofs, like the
//! globe renderer).

use crate::{FrameBuffer, fixed_point::FixedU16F16, tablat::Tablat};

const BANDS: u16 = 196;
const MAP_BAND_BEGIN: u16 = 5;
const MAP_BAND_END: u16 = 191;

const MAP_X_START: u16 = 4;
const MAP_Y_START: u16 = 4;
const MAP_WIDTH: u16 = 312;
const _MAP_HEIGHT: u16 = 144;

const LOG_GLOBDATA_ACCESS: bool = false;

// = the ss scratch the DOS routine works in (si = RESOURCE_GLOBDATA): the
// interpolated row pair at +140/+1740, the five 400-byte band lines at +316,
// and the packed output row at +160.
pub struct MapRenderer {
    buffer: Vec<u8>,
}

impl Default for MapRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl MapRenderer {
    pub fn new() -> Self {
        Self {
            buffer: vec![0; 4096],
        }
    }

    // = segvga:1f4c vga_draw_map_zoomed — draw the full map. `lat`/`lng` are
    // the zoomed globe centre (dx / the ax = lat - 0x12 top band on the DOS
    // side; the +80 below is that -0x12 plus the +98 tablat row bias). The
    // caller clamps `lat` to ±0x4b.
    pub fn draw(&mut self, fb: &mut FrameBuffer, map: &[u8], tablat: &Tablat, lat: i16, lng: u16) {
        let lat = (lat + 75 + 5) as u16;
        // = segvga:1f60 the 0x24-band loop counter (data_segvga_001b7); the
        // north/south walks (segvga:1fa1 / 2005) are folded into the tablat
        // row index here.
        for i in 0..36 {
            self.draw_band(fb, map, tablat, i, lat + i, lng);
        }
    }

    // = one iteration of the band loop (segvga:1fa1 north / 2005 south):
    // interpolate the band's top and bottom map rows, spread them over the
    // four band lines, shade the planet edges, and store the band.
    pub fn draw_band(
        &mut self,
        fb: &mut FrameBuffer,
        map: &[u8],
        tablat: &Tablat,
        i: u16,
        lat: u16,
        lng: u16,
    ) {
        self.buffer.fill(0);

        // = segvga:1f9e/1fb7 the band's top row into +140 (0x8c) and
        // = segvga:2002/2017 its bottom row into +1740 (0x6cc).
        self.interpolate_horizontal_line(map, tablat, lat, lng, 140);
        self.interpolate_horizontal_line(map, tablat, lat + 1, lng, 1740);
        self.interpolate_vertically(tablat, lat);
        self.post_process(lat);
        self.copy_to_framebuffer(fb, i);
    }

    fn read_globdata(&self, addr: usize) -> u8 {
        let v = self.buffer[addr];
        if LOG_GLOBDATA_ACCESS {
            println!("GLOBDATA[{}] -> {:02x}", addr, v);
        }
        v
    }

    fn write_globdata(&mut self, addr: usize, v: u8) {
        if LOG_GLOBDATA_ACCESS {
            println!("GLOBDATA[{}] <- {:02x}", addr, v);
        }
        self.buffer[addr] = v;
    }

    // = segvga:206a/208f/20a7 `stc; adc al,al; add al,al; shl al,1; shl al,1`
    // — unpack a map cell's low-nibble height into the interpolation range:
    // ((cell & 0x0f) * 2 + 1) * 8. `offset` is signed: the run seed reads
    // `[si-1]`, the cell before the rotation offset, so an offset of 0 reads
    // the byte preceding the row (the previous row's last cell) exactly as
    // DOS's segment-relative pointer does.
    fn read_map_pixel(map: &[u8], map_base: usize, offset: isize) -> u8 {
        let b = map[map_base.wrapping_add_signed(offset)];

        (((b & 0x0f) << 1) + 1) << 3
    }

    // = segvga:2025 loc_segvga_02025 — interpolate one map row into the
    // scratch at `output`, 4 px per cell, rotated so longitude `lng` is the
    // row centre. Rows shorter than 88 cells (segvga:204b) are centred by
    // stepping `output` in; wider rows show an 88-cell window.
    fn interpolate_horizontal_line(
        &mut self,
        map: &[u8],
        tablat: &Tablat,
        y0: u16,
        lng: u16,
        output: u16,
    ) {
        let mut output = output;
        let len = tablat.len(y0);
        let map_lat_offset = tablat.offset(y0);

        // = segvga:2034 mul bx; the cell = the high word of lng * len, with
        // = segvga:203b rol/and — the sub-cell remainder's top 2 bits nudge
        // the output pen left for sub-cell scrolling.
        let lng_fp = FixedU16F16::from_u16_u16(0, lng);
        let rotation_offset = lng_fp * len;

        let subpixel_offset = (rotation_offset.0 >> 14) & 0b11;
        let mut rotation_offset = rotation_offset.int_part() as i32;

        output -= subpixel_offset as u16;

        let mut len2: i32;
        let mut len1 = len as i32;

        if len1 < 88 {
            // = segvga:204d..2051 centre a short row.
            output += 2 * (88 - len1) as u16;

            rotation_offset -= len1 / 2;
            if rotation_offset < 0 {
                rotation_offset += len1;
            }

            let len0 = len1;

            len1 -= rotation_offset;
            len2 = (len0 + 1) - len1;
        } else {
            // = segvga:2079..2087 the 88-cell window of a wide row.
            rotation_offset -= 88 / 2;
            if rotation_offset < 0 {
                rotation_offset += len1;
            }

            len1 -= rotation_offset;
            len2 = 88 + 1 - len1;
            if len2 < 0 {
                len2 = 0;
            }
        }

        let mut p0 =
            Self::read_map_pixel(map, map_lat_offset as usize, (rotation_offset - 1) as isize)
                as i32;

        // = segvga:209e.. the two run loops: from the rotation offset to the
        // row end, then the wrap-around from the row start.
        for i in 0..len1 {
            let p1 =
                Self::read_map_pixel(map, map_lat_offset as usize, (i + rotation_offset) as isize)
                    as i32;
            let d = (p1 - p0) / 4;

            for _ in 0..4 {
                self.write_globdata(output as usize, p0 as u8);
                output += 1;
                p0 += d;
            }
            p0 = p1;
        }

        for i in 0..len2 {
            let p1 = Self::read_map_pixel(map, map_lat_offset as usize, i as isize) as i32;
            let d = (p1 - p0) / 4;

            for _ in 0..4 {
                self.write_globdata(output as usize, p0 as u8);
                output += 1;
                p0 += d;
            }
            p0 = p1;
        }
    }

    // = segvga:2123 (northern hemisphere) / segvga:2153 (southern) — spread
    // the interpolated top/bottom rows over the band's four 400-byte lines
    // (segvga:213d the 0x190 line stride, segvga:2164 the 0xb0 = 176-column
    // loop), duplicating columns with the 8-bit error accumulators seeded
    // +0x80 (segvga:21ad..21b4) as the line length grows toward the equator.
    fn interpolate_vertically(&mut self, tablat: &Tablat, y0: u16) {
        let mut l0_len = (tablat.len(y0) / 2) as u32;
        let mut l4_len = (tablat.len(y0 + 1) / 2) as u32;

        let south_hemisphere = y0 >= BANDS / 2;
        if south_hemisphere {
            (l0_len, l4_len) = (l4_len, l0_len);
        }

        assert!(l4_len >= l0_len);

        // First interpolate from the center to the right.
        {
            let mut l0: usize = 320 - 4;
            let mut l1 = l0 + 400;
            let mut l2 = l1 + 400;
            let mut l3 = l2 + 400;
            let mut l4 = l3 + 400;

            // Ensure that we interpolate from the short edge to the long edge.
            if south_hemisphere {
                (l0, l1, l2, l3, l4) = (l4, l3, l2, l1, l0);
            }

            let line_len_delta = l4_len - l0_len;
            let err = ((line_len_delta << 16) / l0_len) as u16;

            let half = err / 2;
            let quarter = err / 4;

            let err_1 = ((quarter + 0x80) >> 8) as u8;
            let err_2 = ((half + 0x80) >> 8) as u8;
            let err_3 = ((half + quarter + 0x80) >> 8) as u8;
            let err_4 = err;

            let mut err_acc_1 = err_1;
            let mut err_acc_2 = err_2;
            let mut err_acc_3 = err_3;
            let mut err_acc_4 = err_4;

            for _ in 0..176 {
                let v0 = self.read_globdata(l0);
                l0 += 1;

                let v4 = self.read_globdata(l4);
                l4 += 1;

                let d = ((v4 as i16 - v0 as i16) / 4) as i8;

                /* LINE 1 */
                let v1 = v0.strict_add_signed(d);
                self.write_globdata(l1, v1);
                l1 += 1;

                let (new_err_acc, did_overflow) = err_acc_1.overflowing_add(err_1);
                err_acc_1 = new_err_acc;
                if did_overflow {
                    self.write_globdata(l1, v1);
                    l1 += 1;
                }

                /* LINE 2 */
                let v2 = v1.strict_add_signed(d);
                self.write_globdata(l2, v2);
                l2 += 1;

                let (new_err_acc, did_overflow) = err_acc_2.overflowing_add(err_2);
                err_acc_2 = new_err_acc;
                if did_overflow {
                    self.write_globdata(l2, v2);
                    l2 += 1;
                }

                /* LINE 3 */
                let v3 = v2.strict_add_signed(d);
                self.write_globdata(l3, v3);
                l3 += 1;

                let (new_err_acc, did_overflow) = err_acc_3.overflowing_add(err_3);
                err_acc_3 = new_err_acc;
                if did_overflow {
                    self.write_globdata(l3, v3);
                    l3 += 1;
                }

                let (new_err_acc, did_overflow) = err_acc_4.overflowing_add(err);
                err_acc_4 = new_err_acc;
                if did_overflow {
                    l4 += 1;
                }
            }
        }

        // Secondly interpolate from the center to the left.
        {
            let mut l0: usize = 320 - 4 - 1;
            let mut l1 = l0 + 400;
            let mut l2 = l1 + 400;
            let mut l3 = l2 + 400;
            let mut l4 = l3 + 400;

            // Ensure that we interpolate from the short edge to the long edge.
            if south_hemisphere {
                (l0, l1, l2, l3, l4) = (l4, l3, l2, l1, l0);
            }

            let line_len_delta = l4_len - l0_len;
            let err = ((line_len_delta << 16) / l0_len) as u16;

            let half = err / 2;
            let quarter = err / 4;

            let err_1 = ((quarter + 0x80) >> 8) as u8;
            let err_2 = ((half + 0x80) >> 8) as u8;
            let err_3 = ((half + quarter + 0x80) >> 8) as u8;
            let err_4 = err;

            let mut err_acc_1 = err_1;
            let mut err_acc_2 = err_2;
            let mut err_acc_3 = err_3;
            let mut err_acc_4 = err_4;

            for _ in 0..176 {
                let v0 = self.read_globdata(l0);
                l0 -= 1;

                let v4 = self.read_globdata(l4);
                l4 -= 1;

                let d = ((v4 as i16 - v0 as i16) / 4) as i8;

                /* LINE 1 */
                let v1 = v0.strict_add_signed(d);
                self.write_globdata(l1, v1);
                l1 -= 1;

                let (new_err_acc, did_overflow) = err_acc_1.overflowing_add(err_1);
                err_acc_1 = new_err_acc;
                if did_overflow {
                    self.write_globdata(l1, v1);
                    l1 -= 1;
                }

                /* LINE 2 */
                let v2 = v1.strict_add_signed(d);
                self.write_globdata(l2, v2);
                l2 -= 1;

                let (new_err_acc, did_overflow) = err_acc_2.overflowing_add(err_2);
                err_acc_2 = new_err_acc;
                if did_overflow {
                    self.write_globdata(l2, v2);
                    l2 -= 1;
                }

                /* LINE 3 */
                let v3 = v2.strict_add_signed(d);
                self.write_globdata(l3, v3);
                l3 -= 1;

                let (new_err_acc, did_overflow) = err_acc_3.overflowing_add(err_3);
                err_acc_3 = new_err_acc;
                if did_overflow {
                    self.write_globdata(l3, v3);
                    l3 -= 1;
                }

                let (new_err_acc, did_overflow) = err_acc_4.overflowing_add(err);
                err_acc_4 = new_err_acc;
                if did_overflow {
                    l4 -= 1;
                }
            }
        }
    }

    // = segvga:22a0 loc_segvga_022a0 — shade the planet's west/east edges into
    // the polar bands: per band line, black out to the edge, then the four
    // shade bytes (cs:8ef) toward the map. The (width, inset) pairs come from
    // the per-band tables at cs:8f7 (north) / cs:92f (south).
    fn post_process(&mut self, y0: u16) {
        assert!(y0 >= MAP_BAND_BEGIN);
        assert!(y0 < MAP_BAND_END);

        let north_hemisphere = y0 < BANDS / 2;

        #[rustfmt::skip]
        const WS: [(u8, u8); 28] = [
            (138,  18),
            (119,  37),
            (107,  49),
            ( 97,  59),
            ( 89,  67),
            ( 81,  75),
            ( 74,  82),
            ( 68,  88),
            ( 63,  93),
            ( 57,  99),
            ( 52, 104),
            ( 48, 108),
            ( 44, 112),
            ( 39, 117),
            ( 36, 120),
            ( 32, 124),
            ( 28, 128),
            ( 25, 131),
            ( 22, 134),
            ( 18, 138),
            ( 16, 140),
            ( 13, 143),
            ( 11, 145),
            (  8, 148),
            (  5, 151),
            (  3, 153),
            (  2, 154),
            (  1, 155),
        ];

        for i in 0..4 {
            let ws_index = if north_hemisphere {
                4 * (y0 - MAP_BAND_BEGIN) as usize + i
            } else {
                4 * (MAP_BAND_END - y0 - 1) as usize + (3 - i)
            };

            if let Some(&(w, v)) = WS.get(ws_index) {
                let mut dst = 160 + 400 * i;

                // Fill in the left edge
                {
                    const LEFT_EDGE: [u8; 4] = [0xc0, 0x90, 0x80, 0x70];
                    if w > 4 {
                        for _ in 0..w - 4 {
                            self.write_globdata(dst, 0x00);
                            dst += 1;
                        }
                    }

                    let n = 4.min(w as usize);
                    for i in 0..n {
                        self.write_globdata(dst, LEFT_EDGE[4 - n + i]);
                        dst += 1;
                    }
                }

                dst += 2 * (v as usize);

                // Fill in the right edge
                {
                    const RIGHT_EDGE: [u8; 4] = [0x70, 0x80, 0x90, 0xc0];

                    for i in 0..4.min(w) {
                        self.write_globdata(dst, RIGHT_EDGE[i as usize]);
                        dst += 1;
                    }

                    if w > 4 {
                        for _ in 4..w {
                            self.write_globdata(dst, 0);
                            dst += 1;
                        }
                    }
                }
            }
        }
    }

    // = segvga:230a loc_segvga_0230a + segvga:2343 — store the band's four
    // lines into the framebuffer, remapping each interpolated height to the
    // map palette (pixel = (v >> 4) + 0x10, segvga:2353..235d). The DOS
    // destination cursor (data_segvga_01cb2) is absolute, seeded 0x504 =
    // (4,4) — no fb_base_ofs.
    fn copy_to_framebuffer(&mut self, fb: &mut FrameBuffer, i: u16) {
        for y in 0..4 {
            for x in 0..MAP_WIDTH {
                let src = y * 400 + (x + 160);
                let b = self.read_globdata(src as usize);
                let c = ((b >> 4) & 0x0f) + 0x10;
                fb.set(x + MAP_X_START, MAP_Y_START + (4 * i) + y, c);
            }
        }
    }
}
