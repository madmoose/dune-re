use crate::{FbId, FrameBuffer, GameState};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextSize {
    #[default]
    Small,
    Large,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

pub struct Font {
    data: Box<[u8]>,
}

impl Font {
    pub fn new(data: &[u8]) -> Self {
        Self { data: data.into() }
    }

    /// Advance width of glyph `c` in `size` — the per-font width table at the
    /// head of the DNCHAR.BIN blob (data[c] for the tall font = seg001:0ceec,
    /// data[c + 0x80] for the small font = seg001:0cf6c).
    pub fn glyph_width(&self, c: u8, size: TextSize) -> u8 {
        match size {
            TextSize::Large => self.data[c as usize],
            TextSize::Small => self.data[c as usize + 0x80],
        }
    }

    // = segvga:1bf5 vga_draw_glyph — blit one 1bpp glyph (1 byte/row, MSB first)
    // at (x, y). `color` is the DOS colour word (bg << 8) | fg: set bits draw the
    // low byte (fg); clear bits draw the high byte (bg), except bg == 0 leaves
    // them transparent (the BH == 0 path). The glyph bitmaps follow the width
    // tables in DNCHAR.BIN: tall at 0x100 (9 rows/glyph), small at 0x580 (7 rows/
    // glyph). Returns the glyph's advance width.
    pub fn draw_glyph(
        &self,
        framebuffer: &mut FrameBuffer,
        x: u16,
        y: u16,
        c: u8,
        size: TextSize,
        color: u16,
    ) -> u16 {
        let fg = color as u8;
        let bg = (color >> 8) as u8;
        let mut glyph_ofs = match size {
            TextSize::Large => 0x100 + glyph_height(size) as usize * c as usize,
            TextSize::Small => 0x580 + glyph_height(size) as usize * c as usize,
        };
        let h = glyph_height(size) as u16;
        let w = self.glyph_width(c, size) as u16;

        for y in y..y + h {
            let mut mask = 0x80;
            for x in x..x + w {
                if self.data[glyph_ofs] & mask != 0 {
                    framebuffer.set(x, y, fg);
                } else if bg != 0 {
                    framebuffer.set(x, y, bg);
                }
                mask >>= 1;
            }
            glyph_ofs += 1;
        }
        w
    }
}

pub struct TextContext<'a> {
    font: &'a Font,
    framebuffer: &'a mut FrameBuffer,
}

impl<'a> TextContext<'a> {
    pub fn new(font: &'a Font, framebuffer: &'a mut FrameBuffer) -> Self {
        Self { font, framebuffer }
    }

    pub fn draw_text(&mut self, style: TextStyle, x: u16, y: u16, s: &str) {
        draw_text(self.font, self.framebuffer, style, x, y, s);
    }

    pub fn measure_text(&self, style: TextStyle, s: &str) -> u16 {
        style.measure_text(self.font, s)
    }
}

#[derive(Copy, Clone, Default)]
pub struct TextStyle {
    pub size: TextSize,
    pub color: u8,
    pub align: TextAlign,
}

impl TextStyle {
    pub fn new() -> Self {
        Self {
            size: TextSize::Small,
            color: 0,
            align: TextAlign::Left,
        }
    }

    pub fn size(mut self) -> Self {
        self.size = TextSize::Large;
        self
    }

    pub fn small(mut self) -> Self {
        self.size = TextSize::Small;
        self
    }

    pub fn large(mut self) -> Self {
        self.size = TextSize::Large;
        self
    }

    pub fn color(mut self, color: u8) -> Self {
        self.color = color;
        self
    }

    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    pub fn left(mut self) -> Self {
        self.align = TextAlign::Left;
        self
    }

    pub fn center(mut self) -> Self {
        self.align = TextAlign::Center;
        self
    }

    pub fn right(mut self) -> Self {
        self.align = TextAlign::Right;
        self
    }

    pub fn measure_text(&self, font: &Font, s: &str) -> u16 {
        s.chars()
            .map(|c| font.glyph_width(c as u8, self.size) as u16)
            .sum()
    }
}

pub fn draw_text(
    font: &Font,
    framebuffer: &mut FrameBuffer,
    style: TextStyle,
    x: u16,
    y: u16,
    s: &str,
) {
    let w = style.measure_text(font, s);

    let mut x = match style.align {
        TextAlign::Left => x,
        TextAlign::Center => x - w / 2,
        TextAlign::Right => x - w,
    };

    for c in s.chars() {
        // The standalone path draws transparent (bg = 0, i.e. the colour word's
        // high byte is clear).
        x += font.draw_glyph(framebuffer, x, y, c as u8, style.size, style.color as u16);
    }
}

fn glyph_height(size: TextSize) -> u8 {
    match size {
        TextSize::Large => 9,
        TextSize::Small => 7,
    }
}

/// DOS font-draw state: the seg001 pen/color/selected-font globals that the
/// `font_*` routines maintain (seg001:2ccdc.. positions, seg001:d094 colour,
/// seg001:219c8 selected glyph func). Driven by the `impl GameState` wrappers
/// below, which render through [`Font`].
#[derive(Clone, Copy, Default)]
pub struct FontState {
    // = _word_2CCDC/_word_2CCDE — the current pen position.
    pub x: u16,
    pub y: u16,
    // = _word_2CCE0/_word_2CCE2 — the line-start position (restored on newline).
    pub start_x: u16,
    pub start_y: u16,
    // = the font_draw_fg_color/font_draw_bg_color pair at seg001:dbe4 — the
    // (font_draw_bg_color << 8) | font_draw_fg_color colour word.
    pub color: u16,
    // = which glyph func _off_219C8 points at: tall (d096) or small (d12f).
    pub size: TextSize,
}

impl GameState {
    // = seg000:d04e font_set_draw_position — set both the pen and the line-start
    // to (x, y).
    pub fn font_set_draw_position(&mut self, x: u16, y: u16) {
        self.font_state.x = x;
        self.font_state.y = y;
        self.font_state.start_x = x;
        self.font_state.start_y = y;
    }

    // = seg000:d05f font_get_draw_position — return the current pen (x, y).
    pub fn font_get_draw_position(&self) -> (u16, u16) {
        (self.font_state.x, self.font_state.y)
    }

    // = seg000:d068 font_select_tall_font — select the 9-row font.
    pub fn font_select_tall_font(&mut self) {
        self.font_state.size = TextSize::Large;
    }

    // = seg000:d075 font_select_small_font — select the 7-row font.
    pub fn font_select_small_font(&mut self) {
        self.font_state.size = TextSize::Small;
    }

    // = seg000:d096 / d12f font_draw_glyph_func — draw glyph `c` at the pen into
    // the active framebuffer with the current colour, then advance the pen x by
    // the glyph width. (DOS picks tall/small via the _off_219C8 pointer; the
    // port reads font_state.size.)
    pub fn font_draw_glyph(&mut self, c: u8) {
        let st = self.font_state;
        // Disjoint field borrows: &self.font and &mut the active framebuffer.
        let fb = match self.active_fb() {
            FbId::Screen => &mut self.screen,
            FbId::Fb1 => &mut self.framebuffer,
            FbId::Saved => &mut self.framebuffer_saved,
        };
        let w = self.font.draw_glyph(fb, st.x, st.y, c, st.size, st.color);
        self.font_state.x += w;
    }

    // = seg000:d1bb font_draw_string — draw a glyph stream until the 0xff
    // terminator. 0x0d is a newline: reset x to the line start and advance y by
    // the font's line height (10 tall / 7 small). Bytes with the high bit set
    // render as 0x40.
    pub fn font_draw_string(&mut self, s: &[u8]) {
        for &b in s {
            match b {
                0xff => break,
                // = seg000:d1d1 carriage return: x = start_x, y += line height.
                0x0d => {
                    self.font_state.x = self.font_state.start_x;
                    let line_h = match self.font_state.size {
                        TextSize::Large => 0x0a,
                        TextSize::Small => 0x07,
                    };
                    self.font_state.start_y += line_h;
                    self.font_state.y += line_h;
                }
                // = seg000:d1c5 high-bit bytes render as 0x40.
                _ => self.font_draw_glyph(if b & 0x80 != 0 { 0x40 } else { b }),
            }
        }
    }

    // = seg000:e290 loc_0e290 — draw `n` as a 3-digit number at (x, y) via the
    // selected glyph func, blanking leading zeros to spaces.
    pub fn font_draw_number_right_aligned_at(&mut self, x: u16, y: u16, n: u16) {
        // = seg000:e290 call font_set_draw_position.
        self.font_set_draw_position(x, y);
        self.font_draw_number_right_aligned(n);
    }

    // = seg000:e297 — draw `n` as a 3-digit number via the  selected glyph func,
    // blanking leading zeros to spaces.
    pub fn font_draw_number_right_aligned(&mut self, n: u16) {
        // = seg000:e29b mov cx,64h; div cl — al = hundreds, ah = n % 100.
        let hundreds = (n / 100) as u8;
        let rem = (n % 100) as u8;
        // = seg000:e29d hundreds digit, blanked to a space when zero (sets the
        // leading-zero flag for the tens digit).
        let suppress = hundreds == 0;
        self.font_draw_glyph(if suppress { 0x20 } else { b'0' + hundreds });
        // = seg000:e2ad aam 0ah — split the remainder into tens and units.
        let tens = rem / 10;
        let units = rem % 10;
        // = seg000:e2b4 tens digit, blanked only while still in leading zeros.
        self.font_draw_glyph(if suppress && tens == 0 {
            0x20
        } else {
            b'0' + tens
        });
        // = seg000:e2c2 units digit (always drawn).
        self.font_draw_glyph(b'0' + units);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A minimal DNCHAR-layout blob: width tables at 0/0x80, glyphs at 0x100/0x580.
    fn test_font() -> Font {
        let mut data = vec![0u8; 0x900];
        let c = b'A' as usize;
        data[c] = 3; // tall width
        data[0x80 + c] = 4; // small width
        // tall glyph row 0 = 1010_0000 (cols 0 and 2 set, MSB first).
        data[0x100 + 9 * c] = 0b1010_0000;
        Font::new(&data)
    }

    #[test]
    fn glyph_width_reads_the_size_specific_table() {
        let font = test_font();
        assert_eq!(font.glyph_width(b'A', TextSize::Large), 3);
        assert_eq!(font.glyph_width(b'A', TextSize::Small), 4);
    }

    #[test]
    fn draw_glyph_is_msb_first_and_transparent_when_bg_zero() {
        let font = test_font();
        let mut fb = FrameBuffer::new(16, 16);
        // bg = 0 (transparent): only set bits paint fg = 5.
        let w = font.draw_glyph(&mut fb, 2, 3, b'A', TextSize::Large, 0x0005);
        assert_eq!(w, 3);
        assert_eq!(fb.get(2, 3), 5); // col 0 set
        assert_eq!(fb.get(3, 3), 0); // col 1 clear, left transparent
        assert_eq!(fb.get(4, 3), 5); // col 2 set
    }

    #[test]
    fn draw_glyph_paints_bg_for_clear_bits_when_bg_nonzero() {
        let font = test_font();
        let mut fb = FrameBuffer::new(16, 16);
        // color = (bg 7 << 8) | fg 5.
        font.draw_glyph(&mut fb, 0, 0, b'A', TextSize::Large, 0x0705);
        assert_eq!(fb.get(0, 0), 5); // set bit -> fg
        assert_eq!(fb.get(1, 0), 7); // clear bit -> bg
        assert_eq!(fb.get(2, 0), 5); // set bit -> fg
    }
}
