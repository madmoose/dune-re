#![allow(clippy::new_without_default)]

use crate::{FrameBuffer, fixed_point::FixedU16F16, tablat::Tablat};

const BANDS: u16 = 196;
const MAP_BAND_BEGIN: u16 = 5;
const MAP_BAND_END: u16 = 191;

const MAP_X_START: u16 = 4;
const MAP_Y_START: u16 = 4;
const MAP_WIDTH: u16 = 312;
const _MAP_HEIGHT: u16 = 144;

const LOG_GLOBDATA_ACCESS: bool = false;

pub struct MapRenderer {
    map: [u8; 50681],
    tablat: Tablat,
    buffer: Vec<u8>,
}

impl MapRenderer {
    pub fn new(map: &[u8; 50681], tablat: &[u8; 792]) -> Self {
        let tablat = Tablat::new(tablat);

        Self {
            map: *map,
            tablat,
            buffer: vec![0; 4096],
        }
    }

    pub fn draw(&mut self, fb: &mut FrameBuffer, lat: i16, lng: u16) {
        let lat = (lat + 75 + 5) as u16;
        for i in 0..36 {
            self.draw_band(fb, i, lat + i, lng);
        }
    }

    pub fn draw_band(&mut self, fb: &mut FrameBuffer, i: u16, lat: u16, lng: u16) {
        self.buffer.fill(0);

        self.interpolate_horizontal_line(lat, lng, 140);
        self.interpolate_horizontal_line(lat + 1, lng, 1740);
        self.interpolate_vertically(lat);
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

    fn read_map_pixel(&mut self, map_base: usize, offset: usize) -> u8 {
        let b = self.map[map_base + offset];

        (((b & 0x0f) << 1) + 1) << 3
    }

    fn interpolate_horizontal_line(&mut self, y0: u16, lng: u16, output: u16) {
        let mut output = output;
        let len = self.tablat.len(y0);
        let map_lat_offset = self.tablat.offset(y0);

        let lng_fp = FixedU16F16::from_u16_u16(0, lng);
        let rotation_offset = lng_fp * len;

        let subpixel_offset = (rotation_offset.0 >> 14) & 0b11;
        let mut rotation_offset = rotation_offset.int_part() as i32;

        output -= subpixel_offset as u16;

        let mut len2: i32;
        let mut len1 = len as i32;

        if len1 < 88 {
            output += 2 * (88 - len1) as u16;

            rotation_offset -= len1 / 2;
            if rotation_offset < 0 {
                rotation_offset += len1;
            }

            let len0 = len1;

            len1 -= rotation_offset;
            len2 = (len0 + 1) - len1;
        } else {
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
            self.read_map_pixel(map_lat_offset as usize, (rotation_offset - 1) as usize) as i32;

        for i in 0..len1 {
            let p1 =
                self.read_map_pixel(map_lat_offset as usize, (i + rotation_offset) as usize) as i32;
            let d = (p1 - p0) / 4;

            for _ in 0..4 {
                self.write_globdata(output as usize, p0 as u8);
                output += 1;
                p0 += d;
            }
            p0 = p1;
        }

        for i in 0..len2 {
            let p1 = self.read_map_pixel(map_lat_offset as usize, i as usize) as i32;
            let d = (p1 - p0) / 4;

            for _ in 0..4 {
                self.write_globdata(output as usize, p0 as u8);
                output += 1;
                p0 += d;
            }
            p0 = p1;
        }
    }

    fn interpolate_vertically(&mut self, y0: u16) {
        let mut l0_len = (self.tablat.len(y0) / 2) as u32;
        let mut l4_len = (self.tablat.len(y0 + 1) / 2) as u32;

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
