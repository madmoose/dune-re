//! VGA driver primitives, mirroring the DOS segment `segvga`.
//!
//! Function names follow the disassembly's `vga_*` / `transition_*` naming so the
//! mapping is grep-able. Free functions take `&mut GameState` because most ops
//! touch the framebuffer, screen, and palette together.
//!
//! Transitions are foreground operations that loop one palette step at a time.
//! Between steps they call `state.present_transition_frame()`, which emits a
//! frame to the display thread and paces one frame interval — but does NOT run
//! frame tasks, exactly like the DOS engine's vsync wait at `loc_segvga_02572`
//! (frame tasks resume only in the post-transition wait loops).
//!
//! Palette values are stored as 6-bit (0–63) DAC values, matching the DOS
//! VGA hardware. The display thread up-scales to 8-bit via
//! `Palette::get_rgb888`. The fade kernels therefore step in 6-bit space —
//! 0x3a uses step=3 cycles=22 (3*22=66 covers 0–63) and 0x36 uses step=1
//! cycles=64 (1*64=64 covers 0–63).

//! The segvga primitives apply `state.y_offset` (DOS `fb_base_ofs`) to the
//! destination y of everything they draw into `state.framebuffer`, as the DOS
//! driver does; the seg000-side sprite helpers in sprite_bank.rs do the same
//! before calling the blitter. The framebuffer→screen copy
//! (`GameState::gfx_copy_whole_framebuf_to_screen`, seg000:c4cd) is a plain
//! memcpy that does NOT apply the offset — matching DOS's `vga_copy_screen_2`.

pub mod blit;
pub mod globe_renderer;
pub mod map_renderer;
pub mod zoom;

use std::mem::swap;

use crate::{
    Color, CursorShapeId, FbId, Font, FrameBuffer, GameState, Rect, cursor_shape,
    font::{TextSize, glyph_height},
};

// = segvga:0a68 vga_save_palette_to_fade_target — snapshot current palette.
pub fn vga_save_palette_to_fade_target(state: &mut GameState) {
    state.palette_fade_target = state.palette.clone();
}

// = segvga:0a76 vga_swap_palettes — atomic swap palette ↔ palette_to_transition_from.
pub fn vga_swap_palettes(state: &mut GameState) {
    swap(&mut state.palette, &mut state.palette_fade_target);
}

// = segvga:0b0c palette_flush
pub fn palette_flush(state: &mut GameState) {
    state.screen_pal = state.palette.clone();
}

// = segvga:0bdc palette_cycle_water — rotate the 64 water entries of the live
// palette (palette_cache + 128*3 = cs:073f, entries 128..191) left by one and
// write them straight to the DAC (dac_write bx = 0x80, cx = 0x40), bypassing
// the dirty-flag flush. blit_water_ripple runs it once per pass.
pub fn palette_cycle_water(state: &mut GameState) {
    // = segvga:0bea..0bf5 lodsw + [si] hold entry 128; 63 entries slide down
    //   one slot; the held entry lands in slot 191.
    state.palette.as_mut_slice()[128..192].rotate_left(1);
    // = segvga:0bf7..0bfd dac_write — the DAC takes the 64 entries now.
    for i in 128..192 {
        state.screen_pal.set(i, state.palette.get(i));
    }
}

// = segvga:0c06 vga_set_fb_row — set `fb_base_ofs` (the per-blit destination y
// offset added by segvga blit primitives). In our model this is
// `state.y_offset`; the gfx-level blit helpers below read it and apply it
// to the destination y of every draw into `state.framebuffer`.
pub fn vga_set_fb_row(state: &mut GameState, row: u16) {
    state.y_offset = row;
}

// = segvga:1888 vga_draw_cursor — composite a 16x16 cursor shape onto `screen` at
// (x, y), saving the pixels it overwrites so vga_restore_cursor can put them back.
// DOS draws straight to A000 and stashes the background at A000:FA00; the port
// draws the front buffer (`screen`) and stashes into `cursor_save`.
pub fn vga_draw_cursor(state: &mut GameState, id: CursorShapeId, x: u16, y: u16) {
    let shape = cursor_shape(id);
    // = segvga:1889/1890 subtract the hotspot, clamping each axis to 0.
    let x = x.saturating_sub(shape.hotspot_x);
    let y = y.saturating_sub(shape.hotspot_y);
    // = segvga:1896 height = 16, clipped to the bottom edge (y + 16 <= 200).
    let h = if y <= 0xb8 {
        16
    } else {
        200u16.saturating_sub(y)
    };
    // = segvga:18ac width = min(16, 320 - x) — clipped to the right edge.
    let w = 320u16.saturating_sub(x).min(16);
    // = segvga:18a4 calc_fb_offset: di = min(y, 199)*320 + x + fb_base_ofs.
    let fb_pos = (y.min(199) as usize + state.y_offset as usize) * 320 + x as usize;

    // `cursor_save` is moved out so `screen` (another field) can be borrowed
    // mutably for the read-then-write in the same loop.
    let mut save = std::mem::take(&mut state.cursor_save);
    save.clear();
    let screen = state.screen.pixels_mut();
    // = segvga:18d8 row loop; segvga:18e5 pixel loop (the DOS pair/odd split is
    // flattened to one pixel per step).
    for row in 0..h as usize {
        let and = shape.and_mask[row];
        let or = shape.or_mask[row];
        let row_off = fb_pos + row * 320;
        for col in 0..w {
            let off = row_off + col as usize;
            // = segvga:18e5 save the background pixel before overwriting it.
            save.push(screen[off]);
            let bit = 0x8000u16 >> col;
            // = segvga:18ee AND set -> keep the background (transparent pixel).
            if and & bit == 0 {
                // = segvga:18f6/18fc OR bit picks colour 0x0f, else black.
                screen[off] = if or & bit != 0 { 0x0f } else { 0x00 };
            }
        }
    }
    state.cursor_save = save;
    // = segvga:18ba..18d3 record the geometry vga_restore_cursor replays.
    state.cursor_save_pos = fb_pos;
    state.cursor_save_w = w;
    state.cursor_save_h = h;
}

// = segvga:1940 vga_restore_cursor — write the saved background back over the
// last cursor footprint, erasing the pointer before it is redrawn elsewhere.
pub fn vga_restore_cursor(state: &mut GameState) {
    let w = state.cursor_save_w as usize;
    let h = state.cursor_save_h as usize;
    let pos = state.cursor_save_pos;
    let save = std::mem::take(&mut state.cursor_save);
    let screen = state.screen.pixels_mut();
    // = segvga:1962 per-row rep movsb from the contiguous save area.
    for row in 0..h {
        let row_off = pos + row * 320;
        let k = row * w;
        screen[row_off..row_off + w].copy_from_slice(&save[k..k + w]);
    }
    state.cursor_save = save;
}

// = segvga:1979 vga_clear_rect — vga_fill_rect with colour 0 (the byte before
// the fill entry that seeds al).
pub fn vga_clear_rect(state: &mut GameState, dest: FbId, x0: u16, y0: u16, x1: u16, y1: u16) {
    vga_fill_rect(state, dest, x0, y0, x1, y1, 0);
}

// = segvga:197b vga_fill_rect (gfx_vtable_vga_fill_rect, seg001:38dd). Fill the
// half-open rect [x0,x1) × [y0,y1) of the framebuffer `dest` with `color`,
// applying fb_base_ofs (y_offset) like every other segvga primitive. `dest`
// is the DOS caller's es load: [_word_2D08A_framebuffer_active_seg]
// (state.active_fb()) at every site except the nav-panel background fill
// (seg000:d74f), which loads [_word_2D088_screen_buffer_seg]
// (state.screen_buffer).
pub fn vga_fill_rect(
    state: &mut GameState,
    dest: FbId,
    x0: u16,
    y0: u16,
    x1: u16,
    y1: u16,
    color: u8,
) {
    let yoff = state.y_offset;
    let fb = state.fb_mut(dest);
    let w = fb.w();
    let h = fb.h();
    for y in y0..y1 {
        let py = y + yoff;
        if py >= h {
            break;
        }
        for x in x0..x1.min(w) {
            fb.set(x, py, color);
        }
    }
}

// = segvga:19c9 vga_fb_copy_rect (j_vga_fb_copy_rect, segvga:015d; the
// gfx_vtable slot at seg001:3931) — copy a width x height rect within the
// framebuffer `dest` from (src_col, src_row) to (dst_col, dst_row), row by
// row (rep movsb at stride 320), both offsets through calc_fb_offset
// (fb_base_ofs / y_offset applied). The port bounds-checks the rows and
// columns DOS trusts its callers for.
#[allow(clippy::too_many_arguments)]
pub fn vga_fb_copy_rect(
    state: &mut GameState,
    dest: FbId,
    src_col: i16,
    src_row: i16,
    width: i16,
    height: i16,
    dst_col: i16,
    dst_row: i16,
) {
    let yoff = state.y_offset as i16;
    let fb = state.fb_mut(dest);
    let (w, h) = (fb.w() as i16, fb.h() as i16);
    for row in 0..height.max(0) {
        let sy = src_row + yoff + row;
        let dy = dst_row + yoff + row;
        if !(0..h).contains(&sy) || !(0..h).contains(&dy) {
            continue;
        }
        for col in 0..width.max(0) {
            let sx = src_col + col;
            let dx = dst_col + col;
            if (0..w).contains(&sx) && (0..w).contains(&dx) {
                let p = fb.get(sx as u16, sy as u16);
                fb.set(dx as u16, dy as u16, p);
            }
        }
    }
}

// = segvga:19f7 vga_clear_screen — clear the active framebuffer to color 0.
pub fn vga_clear_screen(state: &mut GameState) {
    state.active_fb_mut().clear();
}

// = segvga:1a07 vga_draw_line / segvga:1adc bresenham_line (gfx_vtable_vga_draw_line, seg001:3901) — draw
// a line from (x0, y0) to (x1, y1) in `color` through the 16-bit `pattern`:
// the pattern rotates left one bit per step and a pixel plots only on a set
// bit, each plot clipped to the half-open `clip` rect. `dest` is the DOS
// caller's es load. fb_base_ofs (y_offset) applies like every segvga
// primitive. Three DOS shapes, all kept:
// - Δy == 0 (segvga:1a3a): a horizontal run from the left end, |Δx|+1
//   pixels, the row's clip checked once, per-pixel x clip;
// - Δx == 0 (segvga:1a86): a vertical run from the top end, |Δy|+1 rows,
//   the start row clamped into 0..200 (segvga:1a98..1aa2), the column's
//   clip checked once, per-pixel y clip;
// - the general Bresenham (segvga:1afb): max(|Δx|, |Δy|) steps from the
//   start with err seeded at major/2, plotting the stepped positions —
//   the start pixel is never drawn (DOS steps before plotting) — with the
//   full rect clip per pixel.
#[allow(clippy::too_many_arguments)]
pub(crate) fn vga_draw_line(
    state: &mut GameState,
    dest: FbId,
    x0: i16,
    y0: i16,
    x1: i16,
    y1: i16,
    color: u8,
    pattern: u16,
    clip: Rect,
) {
    // The port guards the framebuffer bounds where DOS's calc_fb_offset
    // clamps the row to 199; the clip tests keep the two equivalent.
    fn put(fb: &mut FrameBuffer, x: i16, y: i16, color: u8) {
        if x >= 0 && (x as u16) < fb.w() && y >= 0 && (y as u16) < fb.h() {
            fb.set(x as u16, y as u16, color);
        }
    }
    let yoff = state.y_offset as i16;
    let fb = state.fb_mut(dest);
    let mut pat = pattern;
    // = segvga:1a6d/1abf/1b4b rol the pattern one bit; the rolled-out MSB
    //   gates the plot.
    let mut bit = || {
        let b = pat & 0x8000 != 0;
        pat = pat.rotate_left(1);
        b
    };
    // = segvga:1a24..1a2a the deltas.
    let dx = x1.wrapping_sub(x0);
    let dy = y1.wrapping_sub(y0);
    // = segvga:1ade/1ae2 Δy == 0: the horizontal run.
    if dy == 0 {
        // = segvga:1a56..1a5e the row must lie in the clip band.
        if y0 < clip.y0 || y0 >= clip.y1 {
            return;
        }
        // = segvga:1a3f..1a4f walk from the left end, |Δx|+1 pixels.
        let sx = if dx < 0 { x0 + dx } else { x0 };
        for i in 0..=dx.unsigned_abs() as i16 {
            let x = sx + i;
            // = segvga:1a6d..1a7d pattern bit + the x clip.
            if bit() && x >= clip.x0 && x < clip.x1 {
                put(fb, x, y0 + yoff, color);
            }
        }
        return;
    }
    // = segvga:1ae5..1aec the y step.
    let ystep: i16 = if dy < 0 { -1 } else { 1 };
    // = segvga:1aee/1af0 Δx == 0: the vertical run.
    if dx == 0 {
        // = segvga:1a86..1a96 from the top end.
        let mut count = dy.unsigned_abs() as i16;
        let mut y = if ystep < 0 { y0 - count } else { y0 };
        // = segvga:1a98..1aa2 clamp the start row into 0..200.
        if y >= 0xc8 {
            return;
        }
        if y < 0 {
            count += y;
            y = 0;
        }
        // = segvga:1aa9..1ab0 the column must lie in the clip band.
        if x0 < clip.x0 || x0 >= clip.x1 {
            return;
        }
        // = segvga:1ab5..1ad7 count+1 rows down.
        for i in 0..=count.max(-1) {
            // = segvga:1abf..1ad0 pattern bit + the y clip.
            let ry = y + i;
            if bit() && ry >= clip.y0 && ry < clip.y1 {
                put(fb, x0, ry + yoff, color);
            }
        }
        return;
    }
    // = segvga:1af2..1af9 the x step.
    let xstep: i16 = if dx < 0 { -1 } else { 1 };
    let adx = dx.unsigned_abs();
    let ady = dy.unsigned_abs();
    // = segvga:1afb..1b16 the step pairs: the minor step moves along the
    //   major axis only; the major (diagonal) step moves both.
    let (minor_step, major, minor) = if adx > ady {
        ((xstep, 0i16), adx, ady)
    } else {
        ((0i16, ystep), ady, adx)
    };
    // = segvga:1b19..1b1d err seeds at major/2.
    let mut err = major >> 1;
    let (mut x, mut y) = (x0, y0);
    for _ in 0..major {
        // = segvga:1b1f..1b44 the Bresenham step.
        err = err.wrapping_add(minor);
        let (sx, sy) = if err >= major {
            err -= major;
            (xstep, ystep)
        } else {
            minor_step
        };
        x = x.wrapping_add(sx);
        y = y.wrapping_add(sy);
        // = segvga:1b4b..1b71 pattern bit + the full rect clip.
        if bit() && x >= clip.x0 && x < clip.x1 && y >= clip.y0 && y < clip.y1 {
            put(fb, x, y + yoff, color);
        }
    }
}

// = segvga:1b8c vga_copy_rect_ds / segvga:1b8e vga_copy_rect (gfx_vtable_vga_copy_rect_ds; the inner blit
// behind seg000:c446 copy_rect_fb2_to_fb1). Copy the half-open rect
// `[x0,x1) × [y0,y1)` from `src` to `dst` pixel-for-pixel. The rect is
// clamped to each buffer's bounds; partial overlap is honoured (rows past
// either buffer's height are skipped, columns past width are clipped). The
// two buffers must share the same width for the copy to make sense, but the
// function does not assert it — DOS framebuffers are always 320 wide.
pub fn vga_copy_rect(dst: &mut FrameBuffer, src: &FrameBuffer, rect: Rect) {
    let dst_w = dst.w() as i16;
    let dst_h = dst.h() as i16;
    let src_w = src.w() as i16;
    let src_h = src.h() as i16;
    let x0 = rect.x0.max(0).min(dst_w).min(src_w) as usize;
    let x1 = rect.x1.max(0).min(dst_w).min(src_w) as usize;
    let y0 = rect.y0.max(0).min(dst_h).min(src_h) as usize;
    let y1 = rect.y1.max(0).min(dst_h).min(src_h) as usize;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let dst_stride = dst.w() as usize;
    let src_stride = src.w() as usize;
    for y in y0..y1 {
        let dst_off = y * dst_stride;
        let src_off = y * src_stride;
        dst.pixels_mut()[dst_off + x0..dst_off + x1]
            .copy_from_slice(&src.pixels()[src_off + x0..src_off + x1]);
    }
}

impl Font {
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

// = segvga:1c46 vga_grab_rect (gfx_vtable_vga_grab_rect). Copy the half-open
// rect `[x0,x1) × [y0,y1)` out of `src` into a freshly packed row-major buffer
// (destination stride = rect width, source stride = src width). DOS reads the
// framebuffer at stride 320 and writes the buffer tightly; the buffer is sized
// width × height so vga_put_rect lays it back down at the same coordinates.
pub fn vga_grab_rect(src: &FrameBuffer, rect: Rect) -> Vec<u8> {
    let x0 = rect.x0 as usize;
    let x1 = rect.x1 as usize;
    let y0 = rect.y0 as usize;
    let y1 = rect.y1 as usize;
    let w = x1 - x0;
    let src_stride = src.w() as usize;
    let mut buf = Vec::with_capacity(w * (y1 - y0));
    for y in y0..y1 {
        let off = y * src_stride;
        buf.extend_from_slice(&src.pixels()[off + x0..off + x1]);
    }
    buf
}

// = segvga:1c76 vga_put_rect (gfx_vtable_vga_put_rect). Complement of
// vga_grab_rect: copy a tightly packed `buf` (stride = rect width) into the
// half-open rect `[x0,x1) × [y0,y1)` of `dst`.
pub fn vga_put_rect(dst: &mut FrameBuffer, buf: &[u8], rect: Rect) {
    let x0 = rect.x0 as usize;
    let x1 = rect.x1 as usize;
    let y0 = rect.y0 as usize;
    let y1 = rect.y1 as usize;
    let w = x1 - x0;
    let dst_stride = dst.w() as usize;
    for (row, y) in (y0..y1).enumerate() {
        let off = y * dst_stride;
        let src = &buf[row * w..row * w + w];
        dst.pixels_mut()[off + x0..off + x1].copy_from_slice(src);
    }
}

// = segvga:2384 map_globe_edge_insets — per-row edge insets for map rows at
// |latitude| 0x46..0x57: the number of pixels vga_blit_shaded trims from each
// side of the row, giving the windowed map its curved-globe top/bottom edges.
const MAP_GLOBE_EDGE_INSETS: [u8; 18] = [
    0x02, 0x04, 0x06, 0x08, 0x0b, 0x0e, 0x10, 0x13, 0x16, 0x19, 0x1c, 0x20, 0x24, 0x29, 0x2f, 0x36,
    0x41, 0x4b,
];

// = segvga:23eb vga_blit_shaded / segvga:2413 map_row_blit_shaded / segvga:2396 map_row_edge_inset / segvga:23d7 map_row_shade_right_edge
// (gfx_vtable_vga_blit_shaded) — blit the map
// row buffer (`rows`, stride 0xc8, one raw map cell per byte) to the active
// framebuffer at (x0, y0), remapping every cell from palette bank 0 to bank 1
// (pixel = (cell & 0x0f) + 0x10). `top_lat` is the top row's latitude: rows at
// |latitude| >= 0x46 are trimmed by map_globe_edge_insets on both sides —
// black up to four shade bytes 0x1c,0x19 / 0x18,0x17 toward the map — for the
// curved globe edge. DOS blits bottom-to-top; row order does not matter here.
pub fn vga_blit_shaded(
    state: &mut GameState,
    rows: &[u8],
    width: usize,
    height: usize,
    x0: i16,
    y0: i16,
    top_lat: i16,
) {
    let yoff = state.y_offset as usize;
    let fb = state.active_fb_mut();
    for (row, src) in rows.chunks_exact(0xc8).take(height).enumerate() {
        let y = y0 as usize + row + yoff;
        let mut x = x0 as usize;
        let mut put = |x: &mut usize, c: u8| {
            fb.set(*x as u16, y as u16, c);
            *x += 1;
        };
        // = segvga:2396 loc_segvga_02396 — the left edge inset for this row's
        //   latitude.
        let lat = (top_lat + row as i16).unsigned_abs() as usize;
        let inset = if lat >= 0x46 {
            MAP_GLOBE_EDGE_INSETS[(lat - 0x46).min(MAP_GLOBE_EDGE_INSETS.len() - 1)] as usize
        } else {
            0
        };
        // = segvga:23b2 sub cx,dx twice; jb loc_segvga_023cc — a row narrower
        //   than twice the inset is entirely off the globe: fill it black.
        if width < 2 * inset {
            for _ in 0..width {
                put(&mut x, 0x00);
            }
            continue;
        }
        // = segvga:23b6..23c7 the left edge: black up to the four shade bytes
        //   0x1c,0x19 (only when the inset is at least four) then 0x18,0x17.
        if inset > 0 {
            for _ in 0..inset.saturating_sub(4) {
                put(&mut x, 0x00);
            }
            if inset >= 4 {
                put(&mut x, 0x1c);
                put(&mut x, 0x19);
            }
            put(&mut x, 0x18);
            put(&mut x, 0x17);
        }
        // = segvga:2422..2434 the row kernel: and 0x0f, add 0x10.
        for &cell in &src[inset..width - inset] {
            put(&mut x, (cell & 0x0f) + 0x10);
        }
        // = segvga:23d7 loc_segvga_023d7 — the mirrored right edge: 0x17,0x18,
        //   then 0x19,0x1c and black when the inset is at least four.
        if inset > 0 {
            put(&mut x, 0x17);
            put(&mut x, 0x18);
            if inset >= 4 {
                put(&mut x, 0x19);
                put(&mut x, 0x1c);
                for _ in 0..inset - 4 {
                    put(&mut x, 0x00);
                }
            }
        }
    }
}

// = segvga:2441 vga_draw_landscape (gfx_vtable_vga_draw_landscape)
// = segvga:24ad landscape_row_bh0 — the plain row body
// = segvga:24e9 landscape_row_bh1 — the animated row body
// Render the map row buffer (`rows`, stride 0xc8) to the active framebuffer at (x0, y0)
// through the 256-entry palette-remap table `xlat`. Interior-only: a pixel is
// drawn only when it equals BOTH its right-hand neighbour and the pixel one
// row below (segvga:24bb..24c4; the last column, past the dec cx at
// segvga:24ba, compares the below neighbour only, segvga:24d1), so only the
// inside of each same-valued region takes its colour and the borders come out
// as the backdrop 0x70 — that is what gives the spice-density overlay its
// blobby fields. `top_lat` is the top row's latitude: rows at |latitude| >=
// 0x46 are trimmed on both sides by the same map_globe_edge_insets curve as
// vga_blit_shaded (loc_segvga_02396 / loc_segvga_023d7: black up to the four
// shade bytes 0x1c,0x19,0x18,0x17 toward the map, mirrored on the right).
//
// `bh` (= map_overlay_mode) picks the row body: 0 is the plain remap; any
// other value is the animated remap (loc_segvga_02487): a drawn pixel becomes
// 0x71 + (xlat & 3) unless the xlat is the backdrop 0x70, and between rows
// every non-0x70 xlat entry rotates left by 2 bits (segvga:2493..24a6), so
// the occupation layer's regions shimmer through 0x71..0x74 down the window.
// The DOS grayscale-mode DAC patch (segvga:245e) has no port equivalent.
#[allow(clippy::too_many_arguments)]
pub fn vga_draw_landscape(
    state: &mut GameState,
    rows: &[u8],
    width: usize,
    height: usize,
    x0: i16,
    y0: i16,
    top_lat: i16,
    xlat: &[u8; 256],
    bh: u8,
) {
    let yoff = state.y_offset as usize;
    let fb = state.active_fb_mut();
    // = ss:bp — the bh != 0 path rewrites the caller's table between rows.
    let mut xlat = *xlat;
    for row in 0..height {
        // = segvga:2493..24a6 the between-row rotation of the bh != 0 path
        //   (it runs after each row, so it first applies to row 1).
        if bh != 0 && row > 0 {
            for e in xlat.iter_mut() {
                if *e != 0x70 {
                    *e = e.rotate_left(2);
                }
            }
        }
        let y = (y0 as usize + row + yoff) as u16;
        let mut x = x0 as u16;
        let mut put = |x: &mut u16, c: u8| {
            fb.set(*x, y, c);
            *x += 1;
        };
        // = segvga:24b2 call loc_segvga_02396 — the edge inset for this
        //   row's latitude.
        let lat = (top_lat + row as i16).unsigned_abs() as usize;
        let inset = if lat >= 0x46 {
            MAP_GLOBE_EDGE_INSETS[(lat - 0x46).min(MAP_GLOBE_EDGE_INSETS.len() - 1)] as usize
        } else {
            0
        };
        // = segvga:23b2 sub cx,dx twice; jb loc_segvga_023cc — a row narrower
        //   than twice the inset is entirely off the globe: fill it black.
        if width < 2 * inset {
            for _ in 0..width {
                put(&mut x, 0);
            }
            continue;
        }
        // = segvga:23b6..23c7 the left edge: black up to the shade ramp.
        if inset > 0 {
            if inset >= 4 {
                for _ in 0..inset - 4 {
                    put(&mut x, 0);
                }
                put(&mut x, 0x1c);
                put(&mut x, 0x19);
            }
            put(&mut x, 0x18);
            put(&mut x, 0x17);
        }
        // = segvga:24bb..24dd the interior: width - 2*inset pixels from the
        //   source column `inset` on.
        let w_int = width - 2 * inset;
        for col in 0..w_int {
            let i = row * 0xc8 + inset + col;
            let cell = rows[i];
            // = segvga:24bc/24c0 cmp al,[si]; cmp al,[si+0c7h] — the right
            //   and below neighbours (the source cursor has already stepped
            //   past the pixel, so +0xc7 is one row down, same column); the
            //   last column (segvga:24d1) has no right neighbour.
            let right = rows.get(i + 1).copied().unwrap_or(0);
            let below = rows.get(i + 0xc8).copied().unwrap_or(0);
            let c = if (col == w_int - 1 || cell == right) && cell == below {
                // = segvga:24c6/24c7 xlat; stosb.
                let v = xlat[cell as usize];
                // = segvga:2503..2509 (bh != 0): 0x70 stays; anything else
                //   maps to 0x71 + (v & 3).
                if bh != 0 && v != 0x70 {
                    0x71 + (v & 3)
                } else {
                    v
                }
            } else {
                // = segvga:24cc mov al,ah (0x70) — the backdrop.
                0x70
            };
            put(&mut x, c);
        }
        // = segvga:24df call loc_segvga_023d7 — the right edge, mirrored.
        if inset > 0 {
            put(&mut x, 0x17);
            put(&mut x, 0x18);
            if inset >= 4 {
                put(&mut x, 0x19);
                put(&mut x, 0x1c);
                for _ in 0..inset - 4 {
                    put(&mut x, 0);
                }
            }
        }
    }
}

// = segvga:2596 transition_snapshot_screen_to_fb2 — vga_copy_screen from the
// transition's es (the visible screen, the OLD image) to its si (fb2): all
// 64000 bytes. The effects use the snapshot as their scratch source; fb2's
// prior content is lost, exactly as in DOS.
fn transition_snapshot_screen_to_fb2(state: &mut GameState) {
    let screen_snapshot = state.screen.pixels().to_vec();
    state
        .framebuffer_saved
        .pixels_mut()
        .copy_from_slice(&screen_snapshot);
}

// The midpoint hook type for `vga_transition`: `&dyn Fn` so call sites can
// pass a plain function or a capturing closure without allocation.
pub type TransitionMidpoint<'a> = Option<&'a dyn Fn(&mut GameState)>;

// = segvga:25e7 vga_transition — main transition dispatcher.
// Forces `code` even, wraps modulo 0x3e, and dispatches via the per-handler match below.
// Mirrors the `jmp word ptr transition_dispatch_table[bx]` at segvga:2616.
//
// `dx` is the caller's dx register. The vertical fold (code 0x04) reads its
// low byte to pick the script-traversal direction; the page turn (codes
// 0x0c/0x0e) reads its sign to pick the turn direction.
//
// `midpoint` is a port-only hook, absent in DOS: when set, it runs exactly
// once, at the effect's visual commit point — the instant before the new
// framebuffer's pixels first reach the visible screen. For the palette fades
// (0x36/0x38/0x3a) that is the black moment after the fade-out, before the
// framebuffer copy (so anything the callback draws into fb1 rides along);
// for the instant swap (0x30) it is the cut; for the progressive wipes it is
// the start of the effect, whose reveal begins with its first frame.
// Unimplemented codes fire it too, so a caller gets its one invocation
// regardless of effect.
pub fn vga_transition(state: &mut GameState, code: u16, dx: i16, midpoint: TransitionMidpoint) {
    let mut idx = code & 0xfe;
    while idx >= 0x3e {
        idx -= 0x3e;
    }
    // The fade/cut handlers fire `midpoint` themselves at their black moment
    // or cut; every other effect starts revealing fb1 immediately, so its
    // commit point is here, before the handler runs.
    if !matches!(idx, 0x30 | 0x36 | 0x38 | 0x3a)
        && let Some(midpoint) = midpoint
    {
        midpoint(state);
    }
    match idx {
        0x00 => transition_vertical_curtain(state, dx as u8),
        0x02 => transition_expanding_box(state),
        0x04 => transition_vertical_fold(state, dx as u8),
        0x06 => transition_mosaic_full(state),
        0x08 => transition_dissolve_lfsr_fast(state),
        0x0c | 0x0e => transition_page_turn(state, dx),
        0x10 => transition_dotted_columns(state),
        0x12 => transition_ripple_hold(state),
        0x22 | 0x24 => transition_scroll_push_down(state),
        0x26 => transition_mosaic_medium(state),
        0x28 => transition_mosaic_fine(state),
        0x2a => transition_spiral(state),
        0x2e => transition_mosaic_oneway(state),
        0x34 => transition_dotted_columns_tall(state),
        0x30 => transition_instant_swap(state, midpoint),
        0x36 => transition_fade_in_from_black(state, midpoint),
        0x38 => transition_fade_out_to_black(state, midpoint),
        0x3a => transition_fade_through_black(state, midpoint),
        0x3c => transition_dissolve_lfsr_slow(state),
        other => {
            println!("gfx: vga_transition unimpl code 0x{other:02x}");
        }
    }
}

// = segvga:2604 `mov cx, 98h` — the default handler arg (the 152-row game
// area). transition_dotted_columns_tall overrides it with 0xc8 (the full
// 200-row screen) at segvga:2dc0. Both are halved twice at segvga:2dc9 to give
// the dot-row group count, so the lattice touches `cx` rows (every 4th row, in
// 4-row groups).
const DOTTED_ROWS: usize = 152;

// = segvga:2604 cx = 0x98 — the 152-row game area every pass covers.
const MOSAIC_ROWS: usize = 152;

const DOTTED_ROWS_TALL: usize = 0xc8;

// = segvga:2628 loc_segvga_02628 (transition_dispatch_table entry 27) — code 0x36:
// fade in from black. Saves the new palette as the fade target, blacks
// out the live palette, flips the offscreen framebuffer onto the screen
// (safe while the palette is all-zero — nothing is visible), then steps
// the palette back up to the saved target. Parameters from segvga:264a:
// cx=0x60 (batch=96 bytes = 32 entries × 8 chunks), dx=320 (step=1
// cycles=64).
fn transition_fade_in_from_black(state: &mut GameState, midpoint: TransitionMidpoint) {
    const FADE_36_CYCLES: u8 = 64;
    const FADE_36_CHUNKS: u8 = 8;
    const FADE_36_PER_CHUNK: usize = 32;
    const FADE_36_STEP: u8 = 1;

    state.palette_fade_target = state.palette.clone();
    for i in 0..256 {
        state.palette.set(i, Color(0, 0, 0));
        state.screen_pal.set(i, Color(0, 0, 0));
    }
    // Port-only midpoint hook: the palette is black; fb1 commits next.
    if let Some(midpoint) = midpoint {
        midpoint(state);
    }
    state.gfx_copy_whole_framebuf_to_screen();

    run_fade_to_palette(
        state,
        FADE_36_CYCLES,
        FADE_36_CHUNKS,
        FADE_36_PER_CHUNK,
        FADE_36_STEP,
    );
}

// = segvga:264d loc_segvga_0264d inner step. Steps every component of every palette
// entry in `chunk_start..chunk_start+chunk_size` toward
// `palette_to_transition_from`, gated by `dl` (the outer-loop counter
// that counts DOWN from `cycles` to 1). The DOS kernel does
// `cmp ah, dl; jb skip; add [di], al` — step ONLY when `quotient >= dl`.
//
// With `quotient = (src - dst) / step + 1` (adjusted as below), the
// condition is true on later outer iterations as `dl` shrinks: the first
// few iterations skip (quotient small relative to dl), and as dl drops
// the entry starts stepping. Each step advances `dst` by `remainder`,
// which is `step` when `(src - dst) % step == 0` and the literal
// remainder otherwise. The total advancement over the full loop equals
// exactly `src - dst`, so the entry lands on `src` precisely.
// Per-component fade-up step. Returns the new dst component value after
// one outer iteration with descending counter `dl`. Extracted so the
// math is testable without standing up a `GameState`.
fn fade_step_component(src: u8, dst: u8, step_size: u8, dl: u8) -> u8 {
    let al = src.wrapping_sub(dst);
    if al == 0 {
        return dst;
    }
    let quotient_raw = al as u16 / step_size as u16;
    let remainder_raw = al as u16 % step_size as u16;
    // ah = quotient + 1; if remainder == 0 then ah -= 1, remainder = step.
    let (quotient, remainder) = if remainder_raw == 0 {
        (quotient_raw, step_size as u16)
    } else {
        (quotient_raw + 1, remainder_raw)
    };
    if quotient >= dl as u16 {
        dst.wrapping_add(remainder as u8)
    } else {
        dst
    }
}

fn fade_palette_to_palette_step(
    state: &mut GameState,
    chunk_start: usize,
    chunk_size: usize,
    step_size: u8,
    dl: u8,
) {
    let end = (chunk_start + chunk_size).min(256);
    for j in chunk_start..end {
        let dst_color = state.palette.get(j);
        let src_color = state.palette_fade_target.get(j);
        let new_color = Color(
            fade_step_component(src_color.0, dst_color.0, step_size, dl),
            fade_step_component(src_color.1, dst_color.1, step_size, dl),
            fade_step_component(src_color.2, dst_color.2, step_size, dl),
        );
        state.palette.set(j, new_color);
        state.screen_pal.set(j, new_color);
    }
}

// Run the fade-up kernel. The outer loop counts `dl` DOWN from `cycles`
// to 1 — matching the DOS `dec dl; jnz` semantics. The kernel's `cmp ah,
// dl; jb skip` then makes the early outer iterations no-op for most
// entries, with stepping kicking in as dl shrinks.
fn run_fade_to_palette(
    state: &mut GameState,
    cycles: u8,
    chunks: u8,
    per_chunk: usize,
    step_size: u8,
) {
    for dl in (1..=cycles).rev() {
        for chunk_idx in 0..chunks {
            let chunk_start = (chunk_idx as usize) * per_chunk;
            fade_palette_to_palette_step(state, chunk_start, per_chunk, step_size, dl);
            state.present_transition_frame();
        }
    }
}

// = segvga:26b0 transition_fade_out_to_black (transition_dispatch_table
// entry 28) — code 0x38: fade the visible palette out to black, swap the
// new framebuffer onto the screen while nothing is visible, then restore
// the palette so what follows shows in full colour immediately. Unlike
// 0x3a there is no fade back up — the restore is a snap (the DOS tail
// `jmp palette_flush`, reached in the port through the caller's
// update_screen_palette as well).
//
// Its only current caller is the CD intro's final stage (INTRO_SCRIPT
// stage 47, init = clear): the new framebuffer is black, so the effect
// reads as a plain fade-out that leaves the palette armed for whatever
// comes next.
//
// Parameters from segvga:26b3/26b6: ax=0x40 (4 chunks of 64 entries),
// dx=0x220 (step=2, cycles=32; 32 × 2 covers the full 6-bit DAC range).
fn transition_fade_out_to_black(state: &mut GameState, midpoint: TransitionMidpoint) {
    const FADE_38_CYCLES: u8 = 32;
    const FADE_38_CHUNKS: u8 = 4;
    const FADE_38_PER_CHUNK: usize = 64;
    const FADE_38_STEP: u8 = 2;

    // = segvga:26b0 call vga_copy_palette_to_fade_target — snapshot the
    // working palette (palette_cache) so the fade below can destroy it.
    state.palette_fade_target = state.palette.clone();

    // = segvga:26b9 call fade_palette_to_black (ax=0x40, dx=0x220).
    run_fade_to_black(
        state,
        FADE_38_CYCLES,
        FADE_38_CHUNKS,
        FADE_38_PER_CHUNK,
        FADE_38_STEP,
    );

    // Port-only midpoint hook: the black moment — fb1 commits next.
    if let Some(midpoint) = midpoint {
        midpoint(state);
    }

    // = segvga:26bc..26c7 restore the caller's ds/es and vga_copy_screen —
    // swap the new framebuffer in at the black moment.
    state.gfx_copy_whole_framebuf_to_screen();

    // = segvga:26ce..26d9 rep movsw palette_fade_target → palette_cache —
    // bring the snapshotted palette back into the working palette.
    state.palette = state.palette_fade_target.clone();
    // = segvga:26e0 jmp palette_flush — upload the restored entries.
    palette_flush(state);
}

// = segvga:26e3 fade_palette_to_black inner step. Subtracts `step_size` from every component
// of every palette entry in `chunk_start..chunk_start+chunk_size`,
// saturating at 0. Called `cycles` times per outer loop.
fn fade_palette_to_black_step(
    state: &mut GameState,
    chunk_start: usize,
    chunk_size: usize,
    step_size: u8,
) {
    let end = (chunk_start + chunk_size).min(256);
    for i in chunk_start..end {
        let c0 = state.palette.get(i);
        let c1 = Color(
            c0.0.saturating_sub(step_size),
            c0.1.saturating_sub(step_size),
            c0.2.saturating_sub(step_size),
        );
        state.palette.set(i, c1);
        state.screen_pal.set(i, c1);
    }
}

// Run the fade-out kernel (= fade_palette_to_black, segvga:26e3) for
// `cycles` outer iterations, processing `chunks` chunks of `per_chunk`
// palette entries each, and yielding one frame to the driver between
// chunks (= fade_vsync_wait, segvga:261d).
fn run_fade_to_black(
    state: &mut GameState,
    cycles: u8,
    chunks: u8,
    per_chunk: usize,
    step_size: u8,
) {
    for _ in 0..cycles {
        for chunk_idx in 0..chunks {
            let chunk_start = (chunk_idx as usize) * per_chunk;
            fade_palette_to_black_step(state, chunk_start, per_chunk, step_size);
            state.present_transition_frame();
        }
    }
}

// = segvga:272e loc_segvga_0272e (transition_dispatch_table entry 29) — code 0x3a:
// fade the current palette out to black, swap the framebuffer onto the
// screen while it's invisible, then fade up to the new palette.
//
// The new palette is in `palette` when called; the previous visible
// palette is in `palette_to_transition_from` (set by play_intro's
// pre-stage snapshot). The swap puts the OLD palette in `palette` (so
// the fade-to-black operates on what's on screen) and the NEW palette in
// `palette_to_transition_from` (the fade-in target). The framebuffer →
// screen flip happens at the "black moment" between the two fades so the
// audience never sees old bytes with new colours or vice versa.
//
// Parameters from segvga:2745 (fade-out) and segvga:2751 (fade-up):
// ax/cx=0xff (3 chunks of 85 entries), dx=0x316 (step=3 cycles=22).
fn transition_fade_through_black(state: &mut GameState, midpoint: TransitionMidpoint) {
    const FADE_3A_CYCLES: u8 = 22;
    const FADE_3A_CHUNKS: u8 = 3;
    const FADE_3A_PER_CHUNK: usize = 85;
    const FADE_3A_STEP: u8 = 3;

    vga_swap_palettes(state);

    run_fade_to_black(
        state,
        FADE_3A_CYCLES,
        FADE_3A_CHUNKS,
        FADE_3A_PER_CHUNK,
        FADE_3A_STEP,
    );

    // Port-only midpoint hook: the black moment — fb1 commits next.
    if let Some(midpoint) = midpoint {
        midpoint(state);
    }

    // Palette is now all-zero — safe to swap the screen contents under it.
    state.gfx_copy_whole_framebuf_to_screen();

    run_fade_to_palette(
        state,
        FADE_3A_CYCLES,
        FADE_3A_CHUNKS,
        FADE_3A_PER_CHUNK,
        FADE_3A_STEP,
    );
}

// = segvga:2757 loc_segvga_02757 (transition_dispatch_table entry 24) — code 0x30:
// palette flush + copy framebuffer to screen. No fade — an immediate cut.
// One frame-task tick is enough to let the driver emit the new screen.
fn transition_instant_swap(state: &mut GameState, midpoint: TransitionMidpoint) {
    // Port-only midpoint hook: the cut — fb1 commits to the screen next.
    if let Some(midpoint) = midpoint {
        midpoint(state);
    }
    state.gfx_copy_whole_framebuf_to_screen();
    palette_flush(state);
    state.present_transition_frame();
}

// = segvga:276c transition_tick (vga_effect_dispatch effect 0x0c). Advance the
// wipe-transition engine one step: redraw the wipe edge at the current
// transition_col/transition_frame, then step col += 8 / frame += 1 (wrapping
// back to col=8/frame=1 once col reaches 0x212) and return the NEW column.
//
// The only dune-rs caller is room_frame_task (GameState::tick_room), which steps
// this every 0x0c ticks in the cave/water rooms (location_and_room 0x0804): the
// "wipe" engine is reused as the expanding water ripple, and the returned column
// also times the cave water-drip sound. (The task never installs during the
// intro; see add_room_frame_task.)
pub fn transition_tick(state: &mut GameState) -> u16 {
    // = segvga:276c mov cx,[transition_col]; mov si,[transition_frame].
    let cx = state.transition_col;
    let si = state.transition_frame;
    // = segvga:2778 call transition_draw_edge — render this frame's ellipse band.
    // es = screen (the distorted buffer), ds = fb1 (the clean reference);
    // fb_base_ofs = the active blit row (vga_set_fb_row: y_offset*320).
    let fb_base = state.y_offset as usize * 320;
    transition_draw_edge(
        state.screen.pixels_mut(),
        state.framebuffer.pixels(),
        fb_base,
        cx,
        si,
    );

    // = segvga:277d add cx,8; segvga:2780 add si,1.
    let mut cx = cx + 8;
    let mut si = si + 1;
    // = segvga:2783 cmp cx,212h; jb keep; else reset col=8/frame=1.
    if cx >= 0x212 {
        cx = 8;
        si = 1;
    }
    // = segvga:278f/2794 store transition_col=cx, transition_frame=si.
    state.transition_col = cx;
    state.transition_frame = si;
    // = segvga:2799 retf — returns the new column in cx.
    cx
}

// = segvga:279a transition_ripple_hold (dispatch entry 9) — code 0x12: the
// water-ripple hold on the OLD screen. The screen is snapshotted into fb2 and
// ds = fb2, so the ripple kernel's clean reference is the old image and
// nothing of fb1 is revealed; the transition() tail's full copy paints the
// new image afterwards. The band sweeps col = 8..0x3c0 (drawn only while col
// < 0x1f4), three vsync_wait_5_ticks per step, erasing and restarting at the
// end of each sweep, until 0x12c0 ticks have passed.
fn transition_ripple_hold(state: &mut GameState) {
    // = segvga:279a call transition_snapshot_screen_to_fb2; 279d/279e ds = si.
    transition_snapshot_screen_to_fb2(state);
    let fb_base = state.y_offset as usize * 320;
    // = segvga:27a0/27a3 cx = 8, si = 1; 27a6/27a9 dx = [bp].
    let mut col: u16 = 8;
    let mut frame: u16 = 1;
    let start = state.game_ticks();
    loop {
        // = segvga:27ac cmp cx,1f4h; jnb — draw only while col < 500.
        if col < 0x1f4 {
            let GameState {
                screen,
                framebuffer_saved,
                ..
            } = state;
            transition_draw_edge(
                screen.pixels_mut(),
                framebuffer_saved.pixels(),
                fb_base,
                col,
                frame,
            );
        }
        // = segvga:27b9..27bf three vsync_wait_5_ticks — present and pace.
        let t = state.game_ticks();
        state.send_frame_to_display();
        state.sleep_ticks(t, 15);
        // = segvga:27c2/27c5 col += 8, frame += 1.
        col += 8;
        frame += 1;
        // = segvga:27c8..27d4 at col 0x3c0 erase the last band and restart.
        if col >= 0x3c0 {
            let GameState {
                screen,
                framebuffer_saved,
                ..
            } = state;
            transition_erase_edge(
                screen.pixels_mut(),
                framebuffer_saved.pixels(),
                fb_base,
                col,
                frame,
            );
            col = 8;
            frame = 1;
        }
        // = segvga:27d8..27e0 ax = [bp] - dx; cmp ax,12c0h; jb loop.
        if state.game_ticks().wrapping_sub(start) >= 0x12c0 {
            break;
        }
    }
}

// Which pixel operation the ellipse kernel applies along the band: DOS selects
// it by self-modifying the dispatch pointer at data_segvga_027e4 (0x2823 = draw,
// 0x2887 = erase) before calling transition_kernel.
#[derive(Copy, Clone, PartialEq, Eq)]
enum RippleOp {
    // = segvga:2823 loc_segvga_02823: smear the clean (fb1) water pixel into a 4x4 block.
    Draw,
    // = segvga:2887 loc_segvga_02887: restore the band's water pixels from the clean buffer.
    Erase,
}

// = segvga:27e6 transition_draw_edge — erase the previous edge (col-8, frame-1),
// then draw the leading edge at the current col/frame. Both edges run the same
// transition_kernel with dx=0xb0 (center x = 176), bx=0x5b (center y = 91).
// `screen` is the distorted buffer (es); `fb1` the clean reference (ds).
fn transition_draw_edge(screen: &mut [u8], fb1: &[u8], fb_base: usize, col: u16, frame: u16) {
    // = segvga:27e6 call transition_erase_edge (erases the trailing band first).
    transition_erase_edge(screen, fb1, fb_base, col, frame);
    // = segvga:27e9 patch SMC to 0x2823 (draw); 27f4 dx=0b0h; 27f7 bx=5bh.
    transition_kernel(screen, fb1, fb_base, RippleOp::Draw, col, frame, 0xb0, 0x5b);
}

// = segvga:2802 transition_erase_edge — erase the band 8 columns back (col-8,
// frame-1). At col==8 there is no previous band, so it is skipped (the DOS
// `sub cx,8; jz` guard).
fn transition_erase_edge(screen: &mut [u8], fb1: &[u8], fb_base: usize, col: u16, frame: u16) {
    // = segvga:2804 sub cx,8; jz loc_02820 — nothing to erase on the first step.
    if col == 8 {
        return;
    }
    // = segvga:280e patch SMC to 0x2887 (erase); 2815 dx=0b0h; 2818 bx=5bh.
    transition_kernel(
        screen,
        fb1,
        fb_base,
        RippleOp::Erase,
        col - 8,
        frame - 1,
        0xb0,
        0x5b,
    );
}

// = segvga:2823 ripple_draw_op / segvga:2887 ripple_erase_op — the per-point pixel operation.
// `row` is squashed toward the center line (water seen at an angle) and clipped
// to [0x47, 0x95); `col` is clipped to [0, 320). The 4x4 block at the
// resulting framebuffer offset is then drawn or erased.
fn ripple_edge_op(
    screen: &mut [u8],
    fb1: &[u8],
    fb_base: usize,
    op: RippleOp,
    col: i64,
    mut row: i64,
) {
    // = segvga:2824..2830 squash the upper half toward cy: if (0x5b - row) >= 0
    //   (row <= 0x5b) then row = 0x5b - (0x5b - row)/2.
    let t = 0x5b - row;
    if t >= 0 {
        row = 0x5b - (t >> 1);
    }
    // = segvga:2834..283a clip row to [0x47, 0x47+0x4e) via an unsigned compare.
    if ((row - 0x47) as u16) >= 0x4e {
        return;
    }
    // = segvga:283c cmp dx,140h; jnb skip — unsigned, so a negative (off-screen
    //   left) col wraps high and is clipped too.
    if (col as u16) >= 320 {
        return;
    }

    // = segvga:0c10 calc_fb_offset: di = min(row,199)*320 + col + fb_base_ofs.
    //   fb_base_ofs is the start of the active blit row (vga_set_fb_row: row*320).
    let di = (row.min(199) as usize) * 320 + col as usize + fb_base;

    // Defensive bound: the row/col clips above keep the cave (y_offset=24) inside
    // the 320x200 buffer; guard the 4x4 footprint so other offsets cannot panic.
    if di + 3 * 320 + 3 >= screen.len() {
        return;
    }

    match op {
        RippleOp::Draw => {
            // = segvga:2847..286c: the 4x4 screen block must be all "water"
            //   (every byte's bit 7 set) — AND the 16 bytes and test the sign.
            let mut acc: u8 = 0xff;
            for r in 0..4 {
                for c in 0..4 {
                    acc &= screen[di + r * 320 + c];
                }
            }
            // = segvga:286e jns skip — bit 7 clear means not all-water.
            if acc & 0x80 == 0 {
                return;
            }
            // = segvga:2870 al=[di] (the CLEAN fb1 pixel); 2872 cmp 0f0h; jnb skip.
            let color = fb1[di];
            if color >= 0xf0 {
                return;
            }
            // = segvga:2876..2881 write the 4x4 block (= color) into the screen.
            for r in 0..4 {
                for c in 0..4 {
                    screen[di + r * 320 + c] = color;
                }
            }
        }
        RippleOp::Erase => {
            // = segvga:28af..28e5: for each of the 4x4 block, restore the clean
            //   fb1 pixel wherever the screen pixel is still "water" (bit 7 set).
            for r in 0..4 {
                for c in 0..4 {
                    let off = di + r * 320 + c;
                    // = or al,al; jns skip — only restore bytes >= 0x80.
                    if screen[off] & 0x80 != 0 {
                        screen[off] = fb1[off];
                    }
                }
            }
        }
    }
}

// = segvga:28ec transition_kernel — rasterize one elliptical band of the wipe /
// water ripple. A midpoint-ellipse stepper centered at (`cx_center`, `cy`) with
// horizontal radius `col` and a per-frame vertical scale derived from `frame`;
// it plots the four mirror points of each ellipse step (round-robin, one mirror
// per plot, via the octant counter at data_segvga_029d3) through `op`.
//
// The 32-bit decision accumulators (`a`/`b`/`c`/`d`, the DOS [bp..bp+0eh] frame)
// are modelled as i64. The DOS branch flags map cleanly because each value
// stays in the signed 32-bit range: after a 32-bit `sub`, jnb (no borrow) ⟺
// result >= 0; after an `add` of a positive `b` to a negative `a`, jb (carry) ⟺
// result >= 0; after `b -= 512`, jns ⟺ b >= 0.
#[allow(clippy::too_many_arguments)]
fn transition_kernel(
    screen: &mut [u8],
    fb1: &[u8],
    fb_base: usize,
    op: RippleOp,
    col: u16,
    frame: u16,
    cx_center: i64,
    cy: i64,
) {
    let col = col as i64;
    let frame = frame as i64;

    // = segvga:28f1 data_segvga_029d3 = 0 — the octant round-robin counter.
    let mut octant: u8 = 0;

    // = segvga:2900..2920 set up the decision parameters.
    //   a = 512*(col+1)  (= [bp]),   b = 512*(col-1)  (= [bp+4])
    //   q = (256*col) / frame;  c = (q*q) >> 8  (= [bp+8]),  d = 2*c  (= [bp+0c])
    let mut a: i64 = (col << 9) + 0x200;
    let mut b: i64 = (col << 9) - 0x200;
    let q: i64 = if frame != 0 { (col << 8) / frame } else { 0 };
    let mut c: i64 = (q * q) >> 8;
    let d: i64 = c << 1;

    // = segvga:293c..2943 di = center+col (right x), dx = center-col (left x),
    //   bx = cy (lower y), si = cy (upper y).
    let mut di: i64 = cx_center + col; // right x
    let mut dx: i64 = cx_center - col; // left x
    let mut bx: i64 = cy; // lower y
    let mut si: i64 = cy; // upper y

    // A safety cap; DOS terminates via `b` going negative. Bounds the worst case
    // (col up to 0x212) well above the real iteration count.
    let mut guard = 1 << 16;

    // = segvga:295e..297d region 1 (y is the fast axis).
    loop {
        guard -= 1;
        if guard == 0 {
            return;
        }
        // = segvga:295e call plot; 2961 a -= c.
        ripple_plot(screen, fb1, fb_base, op, &mut octant, di, dx, bx, si);
        a -= c;
        if a >= 0 {
            // = segvga:296d jnb 2950: y-step only.
            c += d;
            bx += 1;
            si -= 1;
            continue;
        }
        // = segvga:296f inc dx; dec di (x-step inward); 2971 a += b.
        dx += 1;
        di -= 1;
        a += b;
        if a >= 0 {
            // = segvga:297d jb 2947: b -= 512, then the y-step at 2950.
            b -= 0x200;
            c += d;
            bx += 1;
            si -= 1;
            continue;
        }
        // = fall through to 297f: enter region 2.
        break;
    }

    // = segvga:297f b -= 512; c += d (no coordinate step).
    b -= 0x200;
    c += d;

    // = segvga:2994..29ca region 2 (x is the fast axis).
    loop {
        guard -= 1;
        if guard == 0 {
            return;
        }
        // = segvga:2994 call plot; 2997 inc dx; dec di (x-step); 2999 a += b.
        ripple_plot(screen, fb1, fb_base, op, &mut octant, di, dx, bx, si);
        dx += 1;
        di -= 1;
        a += b;
        if a >= 0 {
            // = segvga:29a5 carry: y-step, a -= c, c += d.
            bx += 1;
            si -= 1;
            a -= c;
            c += d;
        }
        // = segvga:29c1 b -= 512; jns 2994.
        b -= 0x200;
        if b >= 0 {
            continue;
        }
        // = segvga:29cc final plot, then return.
        ripple_plot(screen, fb1, fb_base, op, &mut octant, di, dx, bx, si);
        return;
    }
}

// = segvga:29d4 — plot one of the ellipse's four mirror points, cycling which
// one through the octant counter (counter & 3 selects the (col,row) pair from
// the left/right x in {dx,di} and the lower/upper y in {bx,si}).
#[allow(clippy::too_many_arguments)]
fn ripple_plot(
    screen: &mut [u8],
    fb1: &[u8],
    fb_base: usize,
    op: RippleOp,
    octant: &mut u8,
    di: i64,
    dx: i64,
    bx: i64,
    si: i64,
) {
    // = segvga:29d4 inc data_029d3; al = data_029d3 & 3.
    *octant = octant.wrapping_add(1);
    let (col, row) = match *octant & 3 {
        // = segvga:29f5 (==0): op(col=dx, row=bx).
        0 => (dx, bx),
        // = segvga:29fb (==1): xchg dx,di -> op(col=di, row=bx).
        1 => (di, bx),
        // = segvga:2a05 (==2): xchg bx,si -> op(col=dx, row=si).
        2 => (dx, si),
        // = segvga:29e7 (==3): xchg both -> op(col=di, row=si).
        _ => (di, si),
    };
    ripple_edge_op(screen, fb1, fb_base, op, col, row);
}

// = segvga:2a10 transition_dissolve_lfsr_slow (transition_dispatch_table entry
// 30) — code 0x3c: the pseudo-random pixel dissolve, 80 pixels per PIT tick.
// The whole game area is 32767 LFSR steps, so the wipe runs ~410 ticks (~2.05 s)
// — a slow speckled fade of the new image over the old. The desert collapse
// (desert_collapse_cutscene, seg000:0e77) reveals each of DEAD3.HNM's frames
// with it.
fn transition_dissolve_lfsr_slow(state: &mut GameState) {
    // = segvga:2a10 dx = 0x50 — 80 pixels per tick.
    dissolve_lfsr_body(state, 0x50);
}

// = segvga:2a15 transition_dissolve_lfsr_fast (entry 4) — code 0x08: the same
// dissolve at 150 pixels per tick, ~219 ticks (~1.09 s).
fn transition_dissolve_lfsr_fast(state: &mut GameState) {
    // = segvga:2a15 dx = 0x96 — 150 pixels per tick.
    dissolve_lfsr_body(state, 0x96);
}

// = segvga:2a18 dissolve_lfsr_body — the body both dissolve entries share.
// `batch` is their pixels-per-tick count (DOS dx).
//
// A 15-bit LFSR walks the game area in pseudo-random order, copying pixels from
// `framebuffer` (ds, the new image) to `screen` (es, the visible one) as it
// goes. Each step copies two: the offset the LFSR names, and that offset plus
// 0x7fff — the walk only reaches 1..0x7fff, so the companion covers the rest of
// the 48640 bytes. Offset 0, the one byte neither pass reaches, is copied at the
// end.
fn dissolve_lfsr_body(state: &mut GameState, batch: u16) {
    // = segvga:2a18/2a1c ax = 0x140 * cx with cx = 152 (set at segvga:2604) —
    //   the 320×152 game area.
    const AREA: usize = 320 * 152;
    let fb_base = state.y_offset as usize * state.screen.w() as usize;
    // = segvga:2a1f cx = 1 — the LFSR seed.
    let mut lfsr: u16 = 1;
    let mut left = batch;
    loop {
        // = segvga:2a26..2a2f si = di = cx + fb_base_ofs; movsb.
        let ofs = fb_base + lfsr as usize;
        let px = state.framebuffer.pixels()[ofs];
        state.screen.pixels_mut()[ofs] = px;
        // = segvga:2a35..2a45 add si,7ffeh (si is one past the copy) and repeat
        //   for that offset while it stays below ax. The compare is on the
        //   un-based offset, as in DOS.
        let far = lfsr as usize + 0x7fff;
        if far < AREA {
            let ofs = fb_base + far;
            let px = state.framebuffer.pixels()[ofs];
            state.screen.pixels_mut()[ofs] = px;
        }
        // = segvga:2a4a..2a4e shr cx,1; on the shifted-out bit xor ch,44h.
        let carry = lfsr & 1 != 0;
        lfsr >>= 1;
        if carry {
            lfsr ^= 0x4400;
        }
        // = segvga:2a51/2a54 the walk ends the step the LFSR returns to its
        //   seed — 32767 steps, every offset in 1..0x7fff visited once.
        if lfsr == 1 {
            break;
        }
        // = segvga:2a56/2a57 dec dx; jnz — otherwise carry on filling the batch.
        left -= 1;
        if left == 0 {
            left = batch;
            // = segvga:2a5a..2a5f `cmp bx,[bp]; jz` — spin until the PIT
            //   counter changes, i.e. one tick per batch (not the three of
            //   loc_segvga_02572).
            state.present_transition_frame_ticks(1);
        }
    }
    // = segvga:2a61 dissolve_lfsr_tail `xor si,si; mov di,si; movsb` — the
    //   byte the walk never reaches. DOS zeroes si/di WITHOUT fb_base_ofs, so
    //   at a nonzero fb_base_ofs it copies byte 0 of the buffer and the first
    //   visible pixel keeps its old value; mirrored here.
    let px = state.framebuffer.pixels()[0];
    state.screen.pixels_mut()[0] = px;
}

// = segvga:2a68 transition_expanding_box (transition_dispatch_table entry 1)
// — code 0x02: the expanding-box reveal (the globe exit into the map view,
// ui_transition_to_map_interface). 8×4-pixel blocks of the new fb1 image are
// copied to the screen in an outward rectangular spiral from (row 74, col
// 156): run lengths 1 right, 2 down, then per ring {n left, n up, n+1 right,
// n+1 down} with n stepping 2..0x26. Each run (loc_segvga_02ab0) spin-waits
// for the next PIT tick after its blocks, so a ring takes ~4 ticks.
fn transition_expanding_box(state: &mut GameState) {
    // = each block copy advances di 4 rows (+4*320); the ax post-adjust after
    // every block selects the walk direction: 0fb08h right (net +8), 0 down
    // (net +4 rows), 0faf8h left (net -8), 0f600h up (net -4 rows).
    const BLOCK_ADVANCE: isize = 4 * 320;
    const RIGHT: isize = 8 - BLOCK_ADVANCE;
    const DOWN: isize = 0;
    const LEFT: isize = -8 - BLOCK_ADVANCE;
    const UP: isize = -2 * BLOCK_ADVANCE;

    // = segvga:2a68..2a6e calc_fb_offset(row 0x4a, col 0x9c) — the centre
    // block, offset by fb_base_ofs.
    let w = state.screen.w() as isize;
    let mut di = (0x4a + state.y_offset as isize) * w + 0x9c;

    // = segvga:2ab0 loc_segvga_02ab0 — one run: `count` blocks in one direction, each an
    // 8-byte × 4-row fb1→screen copy (di += BLOCK_ADVANCE) followed by the
    // ax adjust, then one presented PIT-tick wait.
    fn run(state: &mut GameState, di: &mut isize, count: usize, adj: isize) {
        let start = state.game_ticks();
        for _ in 0..count {
            {
                let (scr, fb1) = (&mut state.screen, &state.framebuffer);
                let len = fb1.pixels().len() as isize;
                let w = scr.w() as isize;
                for row in 0..4 {
                    let o = *di + row * w;
                    if o >= 0 && o + 8 <= len {
                        let (o, dst) = (o as usize, scr.pixels_mut());
                        dst[o..o + 8].copy_from_slice(&fb1.pixels()[o..o + 8]);
                    }
                }
            }
            *di += BLOCK_ADVANCE + adj;
        }
        // = segvga:2ac9..2acd pop ax; cmp ax,[bp]; jz — present the run and
        // hold until the PIT counter moves on.
        state.send_frame_to_display();
        state.sleep_ticks(start, 1);
    }

    // = segvga:2a71..2a7d the centre 8×8: one block right-stepping, then two
    // stacked blocks below its right neighbour.
    run(state, &mut di, 1, RIGHT);
    run(state, &mut di, 2, DOWN);
    // = segvga:2a80..2aad the rings, n = 2..0x26. The odd-looking di nudges
    // between runs re-anchor the walk onto the next ring's corner.
    let mut n = 2;
    while n < 0x26 {
        di += LEFT;
        run(state, &mut di, n, LEFT);
        di -= BLOCK_ADVANCE - 8;
        run(state, &mut di, n, UP);
        di += BLOCK_ADVANCE + 8;
        n += 1;
        run(state, &mut di, n, RIGHT);
        di += BLOCK_ADVANCE - 8;
        run(state, &mut di, n, DOWN);
        n += 1;
    }
}

// ===== Book page-turn fold (segvga transition_page_turn) ===================

// = segvga:2ad1 transition_page_turn (dispatch entries [6]/[7], codes
// 0x0c/0x0e) — the book page-turn fold, used only by book_page_turn_present
// (seg000:b02c, transition 0x0e with the SN2 papers-ruffle). A 45° fold line
// sweeps across the game area; the fold triangle shows the turning page
// rotated 90° (pre-built into fb2 by page_turn_build_rotated_page, its paper
// colors shaded), and an 8-pixel band of the new image (fb1) is painted per
// frame where the fold just passed — the bands accumulate into the full new
// page behind the moving fold.
//
// The sign of `dx` (the DOS caller's dx register) picks the direction.
// Forward (dx >= 0, next page): the rotated page is built from the visible
// old image and the anchor sweeps from the bottom-right corner up the right
// edge, then left along the top row — the old page turns away. Backward
// (dx < 0, back a page / to the cover): the rotated page is built from the
// incoming fb1 image and the anchor runs the mirrored sweep — the new page
// folds open from the top-left.
fn transition_page_turn(state: &mut GameState, dx: i16) {
    // = segvga:2ad2 push cs; call palette_flush — the page palette is live
    // from the first frame.
    palette_flush(state);
    // = the calc_fb_offset fb_base_ofs term (the book screen runs at
    // y_offset = 0).
    let fb_base = state.y_offset as i32 * 320;
    if dx >= 0 {
        // = segvga:2ad9..2ae4 build the rotated page from es — the visible
        // old page.
        {
            let (fb2, screen) = state.fb_pair_mut(FbId::Saved, FbId::Screen);
            page_turn_build_rotated_page(fb2.pixels_mut(), screen.pixels());
        }
        // = segvga:2ae5..2b17 bx walks 0x90 down to -0x138 step -8. The
        // anchor: row = bx, column = 0x140 while bx >= 0 (up the right edge),
        // then row = 0, column = 0x140 + bx (left along the top row).
        let mut bx: i32 = 0x98;
        loop {
            // = segvga:2aea sub bx,8; segvga:2af1..2af5 the bx < 0 clamp.
            bx -= 8;
            let (row, col) = if bx < 0 { (0, 0x140 + bx) } else { (bx, 0x140) };
            // = segvga:2aff calc_fb_offset(row, col) (row <= 0x90, so its
            // 199-row clamp never fires).
            let anchor = fb_base + row * 320 + col;
            {
                let (screen, fb1) = state.fb_pair_mut(FbId::Screen, FbId::Fb1);
                page_turn_fill_forward_band(screen.pixels_mut(), fb1.pixels(), row, col, anchor);
            }
            {
                let (screen, fb2) = state.fb_pair_mut(FbId::Screen, FbId::Saved);
                page_turn_draw_fold(screen.pixels_mut(), fb2.pixels(), row, col, anchor);
            }
            // = segvga:2afa the PIT-tick wait + segvga:2b0f the vsync wait —
            // one paced frame per step.
            state.present_transition_frame();
            // = segvga:2b13 cmp bx,0fec8h; jg — the -0x138 step is the last.
            if bx <= -0x138 {
                break;
            }
        }
    } else {
        // = segvga:2b1a..2b25 build the rotated page from ds (fb1) — the
        // incoming new page.
        {
            let (fb2, fb1) = state.fb_pair_mut(FbId::Saved, FbId::Fb1);
            page_turn_build_rotated_page(fb2.pixels_mut(), fb1.pixels());
        }
        // = segvga:2b26..2b53 bx walks -0x138 up to 0x90 step 8 — the
        // mirrored sweep: right along the top row, then down the right edge.
        let mut bx: i32 = -0x138;
        while bx < 0x98 {
            // = segvga:2b30..2b36 the bx < 0 clamp.
            let (row, col) = if bx < 0 { (0, 0x140 + bx) } else { (bx, 0x140) };
            // = segvga:2b38 calc_fb_offset(row, col).
            let anchor = fb_base + row * 320 + col;
            {
                let (screen, fb1) = state.fb_pair_mut(FbId::Screen, FbId::Fb1);
                page_turn_fill_backward_band(screen.pixels_mut(), fb1.pixels(), row, anchor);
            }
            {
                let (screen, fb2) = state.fb_pair_mut(FbId::Screen, FbId::Saved);
                page_turn_draw_fold(screen.pixels_mut(), fb2.pixels(), row, col, anchor);
            }
            // = segvga:2b48 the vsync wait (the PIT-tick wait sits at 2afa on
            // this path too, before the draw).
            state.present_transition_frame();
            // = segvga:2b4c..2b53 add bx,8; cmp bx,98h; jl.
            bx += 8;
        }
    }
}

// = segvga:2b56 page_turn_draw_fold — draw the fold triangle at the anchor:
// a right triangle whose vertical left edge sits at the anchor column minus
// `ax` and whose hypotenuse lies on the fold diagonal (each row one pixel
// shorter). Each screen row samples one column of the rotated page (si +=
// 0xc8 per row), so the triangle shows the turning page's content rotated 90°.
fn page_turn_draw_fold(screen: &mut [u8], rotated: &[u8], row: i32, col: i32, anchor: i32) {
    // = segvga:2b57..2b71 si = (0x141 - col)*200 - 0x98 (+ 0x98-col when
    // col < 0x98) — the rotated-page column for screen x = col-2, starting at
    // the game-area bottom row.
    let mut si = (0x141 - col) * 200 - 0x98 + if col < 0x98 { 0x98 - col } else { 0 };
    // = segvga:2b73..2b79 ax = min(0x98 - row, col) — the triangle height and
    // its top-row width.
    let mut ax = (0x98 - row).min(col);
    // = segvga:2b80 sub di,ax — every row starts at the anchor column minus
    // the top width.
    let mut di = anchor - ax;
    // = segvga:2b82 the row loop: two half-steps per pass, alternating the
    // even (rep movsw) and odd (rep movsw + movsb) width handling.
    loop {
        for odd in [0, 1] {
            let n = (ax & !1) + odd;
            for k in 0..n {
                // = the rep movsw/movsb bytes. DOS reads a few bytes past the
                // 64000-byte rotated image on edge frames (segment-wrap
                // garbage at the triangle tip); we substitute 0.
                screen[(di + k) as usize] = rotated.get((si + k) as usize).copied().unwrap_or(0);
            }
            // = sub si/di by ax, then si += 0xc8 (next rotated column),
            // di += 0x140 (next screen row).
            si += n - ax + 0xc8;
            di += n - ax + 0x140;
            ax -= 1;
        }
        // = segvga:2ba9 dec ax; jg.
        if ax <= 0 {
            break;
        }
    }
}

// = segvga:2bac page_turn_fill_forward_band — forward turn: paint the 8-pixel
// diagonal band of the new page (fb1) revealed as the fold moved this frame.
// All copies go fb1 → screen at the same offset (DOS sets si = di).
fn page_turn_fill_forward_band(screen: &mut [u8], fb1: &[u8], row: i32, col: i32, anchor: i32) {
    let mut di = anchor;
    let copy = |screen: &mut [u8], di: i32, n: i32| {
        for k in 0..n {
            let ofs = (di + k) as usize;
            screen[ofs] = fb1[ofs];
        }
    };
    // = segvga:2bb1..2bbc ax = min(0x98 - row, col) band rows; bx < 0 marks
    // the diagonal running out the left edge before the game-area bottom.
    let mut ax = 0x98 - row;
    let bx = col - ax;
    if bx < 0 {
        ax += bx;
    }
    // = segvga:2bbe..2bdb column > 0x138 (the anchor still on the right
    // edge): an 8-row lead-in wedge of widths 0..7, stride 0x13f (down 1,
    // left 1), tapering the band in from the edge.
    if col > 0x138 {
        for w in 0..8 {
            copy(screen, di, w);
            di += 0x13f;
        }
        ax -= 8;
        if ax <= 0 {
            return;
        }
    }
    // = segvga:2bdd the main band: ax rows of 8 bytes just right of the fold
    // diagonal, stride 0x13f.
    for _ in 0..ax {
        copy(screen, di, 8);
        di += 0x13f;
    }
    // = segvga:2beb..2bff diagonal out the left edge: a tail wedge of widths
    // 8..1 straight down (stride 0x140).
    if bx < 0 {
        for w in (1..=8).rev() {
            copy(screen, di, w);
            di += 0x140;
        }
    }
}

// = segvga:2c02 page_turn_fill_backward_band — backward turn: paint the
// 8-pixel strip the fold triangle vacated as it moved this frame, from the
// new page (fb1 → screen at the same offset, DOS si = di).
fn page_turn_fill_backward_band(screen: &mut [u8], fb1: &[u8], row: i32, anchor: i32) {
    let mut di = anchor;
    let copy = |screen: &mut [u8], di: i32, n: i32| {
        for k in 0..n {
            let ofs = (di + k) as usize;
            screen[ofs] = fb1[ofs];
        }
    };
    // = segvga:2c07..2c0f anchor offset < 0xa0 (column < 160 on the top row):
    // the strip would start left of the screen — nothing to paint yet.
    if di < 0xa0 {
        return;
    }
    let mut ax;
    if row > 0 {
        // = segvga:2c1a..2c39 anchor on the right edge: an 8-row band of
        // 0xa0-row bytes ending at the anchor column, 8 rows above the
        // triangle top (di -= 0xa00) — repaints the rows the fold top passed
        // this frame. rep movsw copies the even part of the width.
        ax = 0xa0 - row;
        di -= ax + 0xa00;
        for _ in 0..8 {
            let n = ax & !1;
            copy(screen, di, n);
            di += n - ax + 0x140;
        }
        ax -= 8;
    } else {
        // = segvga:2c3e..2c40 anchor on the top row: the strip starts 160
        // pixels left of the anchor (8 left of the triangle's vertical edge)
        // and runs the full 0x98-row game-area height.
        ax = 0x98;
        di -= ax + 8;
    }
    // = segvga:2c43..2c4f the strip: ax rows of 8 bytes straight down
    // (stride 0x140) along the left of the triangle's vertical trailing edge.
    for _ in 0..ax {
        copy(screen, di, 8);
        di += 0x140;
    }
}

// = segvga:2c52 page_turn_build_rotated_page — build the rotated page image in
// the fb2 segment (es = the saved caller si, data_segvga_02535). The source
// page (ds, picked by the caller) is rotated 90°:
// dst[c*200 + r] = src[(199-r)*320 + (319-c)] — 320 columns right-to-left
// (si starts at 0xf9ff, the bottom-right pixel), each written bottom-to-top
// (4 rows per inner step, si -= 0x500). The fold triangle then reads screen
// rows as rotated-page columns with plain sequential copies.
fn page_turn_build_rotated_page(rotated: &mut [u8], src: &[u8]) {
    let mut di = 0;
    for c in 0..320 {
        let x = 319 - c;
        for r in 0..200 {
            let y = 199 - r;
            let mut p = src[y * 320 + x];
            // = segvga:2c6b/2c78/2c89/2c98 pixels in [0x60,0x62) get +2 —
            // the page paper colors remap to their darker shades, so the fold
            // shows the shaded back of the page.
            if (0x60..0x62).contains(&p) {
                p += 2;
            }
            rotated[di] = p;
            di += 1;
        }
    }
    // = segvga:2cac..2cc4 patch the spine out of the rotated image: bytes
    // 48..56 of column c-54 are copied into column c for c in 126..194 — in
    // screen terms, rows 144..151 at x 193 down to 126 take the flat paper
    // 54 pixels to their right.
    for i in 0..0x44 {
        let d = 0x62a0 + i * 0xc8;
        rotated.copy_within(d - 0x2a30..d - 0x2a30 + 8, d);
    }
}

// = segvga:2cca transition_vertical_curtain (transition_dispatch_table entry
// 0) — code 0x00: the vertical curtain over the 152-row game area (cx =
// 0x98; the effect addresses from offset 0, no fb_base_ofs). An 8-row
// colour-7 band rides the seam and each pass is paced ~5 PIT ticks
// (loc_segvga_0253d). Neither branch draws the final frame — the
// transition() tail's full copy paints the last rows.
//
// dl >= 0 (segvga:2cce): the new fb1 image slides DOWN into view from the
// top — pass k shows fb1's bottom k*8 rows at the top of the screen, band
// below them.
//
// dl < 0 (loc_segvga_02cfe — the GLOBE open's dx = 0ffffh): snapshot the
// live screen into fb2 (loc_segvga_02596), slide that old image UP off the
// screen, and reveal the new fb1 image in place below the rising band
// (8 fresh rows per pass).
fn transition_vertical_curtain(state: &mut GameState, dl: u8) {
    // = segvga:2cce/2d01 dx = 0x140 * cx(0x98) — the game-area byte count.
    const TOTAL: usize = 320 * 152;
    // = the 0xa00-byte (8-row) band and per-pass step.
    const BAND: usize = 320 * 8;

    if (dl as i8) >= 0 {
        // = segvga:2cd5 si = the full byte count: the first pass copies
        // nothing and only paints the band at the top.
        let mut si = TOTAL;
        loop {
            let start = state.game_ticks();
            let n = TOTAL - si;
            {
                let (scr, fb1) = (&mut state.screen, &state.framebuffer);
                let dst = scr.pixels_mut();
                // = segvga:2cda..2ce7 screen[0..n] = fb1[si..TOTAL].
                dst[..n].copy_from_slice(&fb1.pixels()[si..TOTAL]);
                // = segvga:2ce8..2cee the colour-7 band (0x500 stosw of
                // 0x0707).
                dst[n..n + BAND].fill(7);
            }
            // = segvga:2cf4 call loc_segvga_0253d — present and pace.
            state.send_frame_to_display();
            state.sleep_ticks(start, 5);
            // = segvga:2cf0/2cf7 si -= 0xa00; loop while si > 0xa00.
            si -= BAND;
            if si <= BAND {
                break;
            }
        }
    } else {
        // = segvga:2596 loc_segvga_02596 — snapshot the visible screen into fb2.
        transition_snapshot_screen_to_fb2(state);
        // = segvga:2d08 si = 0xa00, stepping up to the full byte count.
        let mut si = BAND;
        while si <= TOTAL {
            let start = state.game_ticks();
            let n = TOTAL - si;
            {
                let (scr, fb1, fb2) = (
                    &mut state.screen,
                    &state.framebuffer,
                    &state.framebuffer_saved,
                );
                let dst = scr.pixels_mut();
                // = segvga:2d0e..2d20 screen[0..n] = fb2[si..TOTAL] — the
                // old image shifted up by si bytes.
                dst[..n].copy_from_slice(&fb2.pixels()[si..TOTAL]);
                // = segvga:2d22..2d28 the colour-7 band at the seam.
                dst[n..n + BAND].fill(7);
                // = segvga:2d2a..2d33 the 8 freshly exposed rows of the new
                // image, in place below the band.
                let di = n + BAND;
                if di < TOTAL {
                    dst[di..di + BAND].copy_from_slice(&fb1.pixels()[di..di + BAND]);
                }
            }
            // = segvga:2d3a call loc_segvga_0253d — present and pace.
            state.send_frame_to_display();
            state.sleep_ticks(start, 5);
            // = segvga:2d36/2d3d si += 0xa00; loop while si <= the total.
            si += BAND;
        }
    }
}

// = segvga:2d44 transition_scroll_push_down (dispatch entries 17/18) — codes
// 0x22 / 0x24: the push-down scroll over the 152-row game area. The screen is
// snapshotted into fb2 and the buffer slots swapped (02535 = fb1, 02537 =
// fb2); each pass paints fb1[si..end] at the top of the area and fb2[base..si]
// below it, so the new image slides DOWN in from the top and pushes the old
// one off the bottom, one row per pass. Pass k waits until 6k ticks have
// elapsed (loc_segvga_02d9d); when the spin overshoots, the step doubles to
// two rows and the pass counter skips a slot. The ending's FINAL still enters
// with it (seg000:1522).
fn transition_scroll_push_down(state: &mut GameState) {
    const W: usize = 320;
    // = segvga:2604 cx = 0x98 — the game-area row count.
    const ROWS: usize = 152;
    // = segvga:2d44 bx = 0fec0h — one row up per pass.
    const ROW_STEP: usize = W;
    // = segvga:2d90..2d9b the per-pass tick budget.
    const TICKS_PER_PASS: u64 = 6;

    // = segvga:2d47 call transition_snapshot_screen_to_fb2.
    transition_snapshot_screen_to_fb2(state);
    // = segvga:2d51..2d5c calc_fb_offset(row = 152, col = 0): si = dx = the
    //   end of the game area.
    let fb_base = state.y_offset as usize * W;
    let end = fb_base + ROWS * W;
    // = segvga:2d5e call swap_transition_buffers — the copies below read the
    //   new image from fb1 and the old one from the fb2 snapshot.
    // = segvga:2d61 cx = [bp]; 2d4a data_segvga_0253b = 0.
    let start = state.game_ticks();
    let mut passes: u64 = 0;
    let mut si = end;
    loop {
        {
            let GameState {
                screen,
                framebuffer,
                framebuffer_saved,
                ..
            } = state;
            let dst = screen.pixels_mut();
            // = segvga:2d66..2d78 screen[base..base+n] = fb1[si..end].
            let n = end - si;
            dst[fb_base..fb_base + n].copy_from_slice(&framebuffer.pixels()[si..end]);
            // = segvga:2d7a..2d8c screen[base+n..end] = fb2[base..si].
            dst[fb_base + n..end].copy_from_slice(&framebuffer_saved.pixels()[fb_base..si]);
        }
        state.send_frame_to_display();
        // = segvga:2d90..2da4 passes += 1; spin until [bp] - cx >= passes*6.
        passes += 1;
        state.sleep_ticks(start, passes * TICKS_PER_PASS);
        // = segvga:2da6..2dad bx = -320; unless the spin ended exactly on the
        //   deadline (zf), double the step and count the missed slot.
        let mut step = ROW_STEP;
        if state.game_ticks().wrapping_sub(start) != passes * TICKS_PER_PASS {
            step *= 2;
            passes += 1;
        }
        // = segvga:2db2..2dbd si += bx; jb ret once si drops below
        //   fb_base_ofs (a wrap past 0 lands above dx and returns too).
        if si < fb_base + step {
            break;
        }
        si -= step;
    }
}

// = segvga:2dc0 transition_dotted_columns_tall (transition_dispatch_table entry
// 0x1a) — code 0x34: the dotted-column reveal over the full 200-row screen. It
// preloads cx = 0xc8 and falls through to the same body as
// transition_dotted_columns. This is the room re-enter / view-toggle reveal
// (ui_present_room_screen(0x34) from ui_enter_room_view / ui_toggle_room_view,
// seg000:1898), which composes the whole room screen into fb1 offscreen and
// dissolves it in. The throne room renders at fb_base_ofs = 0, so the 200-row
// lattice fills the screen buffer exactly.
fn transition_dotted_columns_tall(state: &mut GameState) {
    // The 200-row lattice fills the screen buffer exactly only from row 0
    // (fb_base_ofs = 0); a nonzero offset would run the dot grid past the
    // buffer. The room re-enter always arrives here with y_offset = 0, so warn
    // and force it rather than letting an out-of-bounds index panic.
    if state.y_offset != 0 {
        println!(
            "gfx: transition_dotted_columns_tall expects y_offset = 0, got {}; forcing 0",
            state.y_offset
        );
        state.y_offset = 0;
    }
    // = segvga:2dc0 cx = 0xc8 — the full 200-row screen.
    dotted_columns_reveal(state, DOTTED_ROWS_TALL >> 2);
}

// = segvga:2dc3 transition_dotted_columns (transition_dispatch_table entry
// 8) — code 0x10: stippled-column reveal. The visible old image (in
// `screen`) is dissolved to black through a 4×4 dot lattice, the palette is
// flipped at the all-black moment, then the new image (in `framebuffer`) is
// revealed through the same lattice. Used by INTRO_SCRIPT stage 12
// (intro_12_init) to cut from the desert-sky scene to the first frame of
// MTG1.HNM.
//
// The new palette is in `palette` on entry (hnm_load_first_frame / the still's
// init loaded it) while the screen still shows the old palette. DOS keeps the
// old palette in the DAC across pass 1 and only uploads the new one at the
// `call palette_flush` between passes; we mirror that by presenting pass 1 with
// `screen_pal` (the displayed palette) and restoring the live `palette` before
// pass 2.
fn transition_dotted_columns(state: &mut GameState) {
    // = segvga:2604 cx = 152 — the 152-row game area.
    dotted_columns_reveal(state, DOTTED_ROWS >> 2);
}

// One reveal pass over the dotted lattice. For every table entry, walk the
// `row_groups × DOTTED_COLS` dot grid anchored at `fb_base + ofs`
// and write each touched pixel, then wait one frame (= the segvga:2df0 /
// segvga:2e24 vsync wait `call loc_segvga_02572`) so the partially-filled
// screen is emitted. `reveal == false` blacks the pixel out (pass 1's
// `xor ax,ax; stosb`); `reveal == true` copies the framebuffer pixel at the
// same offset (pass 2's `mov si,di; movsb` from the source buffer).
fn run_dotted_pass(state: &mut GameState, fb_base: usize, row_groups: usize, reveal: bool) {
    for &ofs in &DOTTED_COLUMNS_OFFSETS {
        let base = fb_base + ofs;
        for group in 0..row_groups {
            let mut di = base + group * DOTTED_ROW_STRIDE;
            for _ in 0..DOTTED_COLS {
                let value = if reveal {
                    state.framebuffer.pixels()[di]
                } else {
                    0
                };
                state.screen.pixels_mut()[di] = value;
                di += DOTTED_COL_STRIDE;
            }
        }
        state.present_transition_frame();
    }
}

// The shared transition_dotted_columns body (= loc_segvga_02dc3): dissolve the
// visible old image to black through the 4×4 dot lattice (`row_groups` 4-row
// groups), flip the palette at the all-black midpoint, then reveal the new
// image (in `framebuffer`) through the same lattice.
//
// The new palette is in `palette` on entry while the screen still shows the old
// palette. DOS keeps the old palette in the DAC across pass 1 and only uploads
// the new one at the `call palette_flush` between passes; we mirror that by
// presenting pass 1 with `screen_pal` (the displayed palette) and flushing the
// live `palette` into it before pass 2. We rely on `screen_pal`, *not*
// palette_fade_target: intro_29_init repurposes palette_fade_target as the sky
// cross-fade target before this transition, so swapping it in would tint the
// dissolve with sky colours (the visible "jump to a wrong palette").
fn dotted_columns_reveal(state: &mut GameState, row_groups: usize) {
    let fb_base = state.y_offset as usize * state.screen.w() as usize;

    // Pass 1 dissolves the OLD image to black using the palette already on
    // screen (the DAC).
    run_dotted_pass(state, fb_base, row_groups, false);

    // = segvga:2df8 push cs; call palette_flush — make the new palette live
    // now that the screen is all-black (color 0, unaffected by the swap).
    palette_flush(state);

    // = segvga:2e01 second pass — copy the source framebuffer through the
    // same dot lattice to reveal the new image in the new palette.
    run_dotted_pass(state, fb_base, row_groups, true);
}

// = segvga:2ddf `mov dx, 50h` — 80 dots written across each row.
const DOTTED_COLS: usize = 0x50;

// = segvga:2de3 `add di, 3` after the stosb — stride 4, one dot per 4-wide
// block column (80 dots × 4 = a full 320-pixel row).
const DOTTED_COL_STRIDE: usize = 4;

// = segvga:2dea `add di, 500h` — advance 4 screen rows (4 × 320) between
// dot rows, so each table entry touches only every 4th row.
const DOTTED_ROW_STRIDE: usize = 0x500;

// = segvga:2e66 spiral_offset_table — the 65 framebuffer offsets that drive the
// spiral order, one per (col_phase, row_phase) cell of an 8×8 block. Each offset
// names the block-relative start pixel (`ofs % 320` = column phase 0..7,
// `ofs / 320` = row phase 0..7); walking every 8th column and every 8th row from
// it touches that one cell of every 8×8 block. The 64 cells are ordered as an
// inward spiral — the outer ring of the block first (bottom edge L→R, right edge
// bottom→top, top edge R→L, left edge top→bottom), then the next ring in, down
// to the centre — so all blocks fill in lockstep and the screen reads as a
// spiral dissolve. In DOS the table is terminated by a 0xffff sentinel (the
// forward walk's `js` exit); here the array length stands in for it. Entry 0x0140
// appears twice, so that cell is stamped twice (harmlessly) — 65 entries cover
// the 64-cell block.
const SPIRAL_OFFSETS: [usize; 65] = [
    0x08c0, 0x08c1, 0x08c2, 0x08c3, 0x08c4, 0x08c5, 0x08c6, 0x08c7, 0x0787, 0x0647, 0x0507, 0x03c7,
    0x0287, 0x0147, 0x0007, 0x0006, 0x0005, 0x0004, 0x0003, 0x0002, 0x0001, 0x0000, 0x0140, 0x0140,
    0x0280, 0x03c0, 0x0500, 0x0640, 0x0780, 0x0781, 0x0782, 0x0783, 0x0784, 0x0785, 0x0786, 0x0646,
    0x0506, 0x03c6, 0x0286, 0x0146, 0x0145, 0x0144, 0x0143, 0x0142, 0x0141, 0x0281, 0x03c1, 0x0501,
    0x0641, 0x0642, 0x0643, 0x0644, 0x0645, 0x0505, 0x03c5, 0x0285, 0x0284, 0x0283, 0x0282, 0x03c2,
    0x0502, 0x0503, 0x0504, 0x03c4, 0x03c3,
];

// = segvga:2eea transition_spiral (transition_dispatch_table entry 21) — code
// 0x2a: dissolve the visible old image to black through a per-8×8-block inward
// spiral, flip the palette at the all-black midpoint, then reveal the new image
// (composed in `framebuffer`) through the same spiral. ui_present_room_screen
// (0x2a) uses it for the WAIT FOR EVENING/MORNING re-present (seg000:0fac),
// dissolving the 152-row room view from the old time-of-day palette/scene to the
// new one; the HUD/panel below the game area is left to the trailing
// whole-framebuffer copy in `transition`.
//
// Mirrors transition_dotted_columns: the new palette is in `palette` on entry
// while the screen still shows the old palette, so pass 1 presents with the
// displayed `screen_pal` and palette_flush uploads the new one at the all-black
// midpoint before pass 2.
fn transition_spiral(state: &mut GameState) {
    // = segvga:01a3 fb_base_ofs — the game-area blit offset. The room game screen composes
    // from row 0 (y_offset = 0, as transition_dotted_columns_tall also assumes),
    // so the 152-row lattice (+ the offsets' 8-row reach) stays inside the 200-row
    // screen buffer.
    let fb_base = state.y_offset as usize * state.screen.w() as usize;
    // = segvga:2ef6 the black-out pass dissolves the OLD image to black using the
    // palette already on screen (the DAC).
    run_spiral_pass(state, fb_base, false);
    // = segvga:2f25 push cs; call palette_flush — make the new palette live now
    // that the game area is all-black (color 0, unaffected by the swap).
    palette_flush(state);
    // = segvga:2f28 the reveal pass copies the source framebuffer through the same
    // spiral to reveal the new image in the new palette.
    run_spiral_pass(state, fb_base, true);
}

const DUMP_FRAMES: bool = false;

// One pass of the spiral. For every table entry, stamp the
// `SPIRAL_ROW_GROUPS × SPIRAL_COLS` dot grid anchored at `fb_base + ofs`, then
// wait one frame (= the segvga:2f1e / segvga:2f4b vsync wait `call
// loc_segvga_02572`) so the partially-filled screen is emitted. `reveal ==
// false` blacks the pixel out (the black-out pass's `xor ax,ax; stosb`);
// `reveal == true` copies the source-framebuffer pixel (the reveal pass's `mov
// di,si; movsb` from `ds`). The black-out pass walks the table backward
// (segvga:2ef6 `sub si,2`, so it spirals outward from the centre) and the reveal
// forward (segvga:2f28 `lodsw`, spiralling inward); each pass stamps every cell.
fn run_spiral_pass(state: &mut GameState, fb_base: usize, reveal: bool) {
    let n = SPIRAL_OFFSETS.len();
    for i in 0..n {
        let ofs = if reveal {
            SPIRAL_OFFSETS[i]
        } else {
            SPIRAL_OFFSETS[n - 1 - i]
        };
        let base = fb_base + ofs;
        for group in 0..SPIRAL_ROW_GROUPS {
            let mut di = base + group * SPIRAL_ROW_STRIDE;
            for _ in 0..SPIRAL_COLS {
                let value = if reveal {
                    state.framebuffer.pixels()[di]
                } else {
                    0
                };
                state.screen.pixels_mut()[di] = value;
                di += SPIRAL_COL_STRIDE;
            }
        }
        state.present_transition_frame();
    }
}

// = segvga:2ef0 `shr cx,1` ×3 — the 152-row game area (cx = 0x98) in 8-row
// groups (152 / 8 = 19). The spiral touches every 8th row in 8-row groups.
const SPIRAL_ROW_GROUPS: usize = 152 >> 3;

// = segvga:2f0d `mov dx,28h` — 40 dots written across each row.
const SPIRAL_COLS: usize = 0x28;

// = segvga:2f11 `add di,7` after the stosb — stride 8, one dot per 8-wide block
// column (40 dots × 8 = a full 320-pixel row).
const SPIRAL_COL_STRIDE: usize = 8;

// = segvga:2f18 `add di,0a00h` — advance 8 screen rows (8 × 320) between dot
// rows, so each table entry touches only every 8th row.
const SPIRAL_ROW_STRIDE: usize = 0xa00;

// The framebuffer a mosaic pass samples: fb2 (the old-screen snapshot) for
// the pixelate half, fb1 (the new image) for the de-pixelate half.
#[derive(Clone, Copy)]
enum MosaicSource {
    Fb2,
    Fb1,
}

// = segvga:2f53 transition_mosaic_full (transition_dispatch_table entry 3) —
// code 0x06: the three-level mosaic. Snapshot the old screen into fb2, then
// re-stamp the game area as 2×2, 4×4 and 8×8 blocks of the snapshot (each
// level for 0x24 ticks, the block sample position cycling per frame so the
// mosaic shimmers), flush the new palette at the coarsest point, and
// de-pixelate the new image from fb1 through the same levels in reverse.
// The vision dream enters its VIS backdrop with it (present_vision_dream,
// seg000:2c47).
fn transition_mosaic_full(state: &mut GameState) {
    // = segvga:2f53 call loc_segvga_02596.
    transition_snapshot_screen_to_fb2(state);
    // = segvga:2f56/2f57 push ds; ds = si — the pixelate half reads fb2.
    mosaic_pass(state, MosaicSource::Fb2, 2, &MOSAIC_OFFSETS_2X2);
    mosaic_pass(state, MosaicSource::Fb2, 4, &MOSAIC_OFFSETS_4X4);
    mosaic_pass(state, MosaicSource::Fb2, 8, &MOSAIC_OFFSETS_8X8);
    // = segvga:2f62 push cs; call palette_flush.
    palette_flush(state);
    // = segvga:2f66 pop ds — the de-pixelate half reads the new image (fb1).
    mosaic_pass(state, MosaicSource::Fb1, 8, &MOSAIC_OFFSETS_8X8);
    mosaic_pass(state, MosaicSource::Fb1, 4, &MOSAIC_OFFSETS_4X4);
    mosaic_pass(state, MosaicSource::Fb1, 2, &MOSAIC_OFFSETS_2X2);
}

// = segvga:2f71 transition_mosaic_oneway (dispatch entry 23) — code 0x2e:
// coarse-to-fine mosaic reveal with no snapshot and no palette swap. ds stays
// the dispatcher's fb1, so all seven passes stamp the NEW image: 8x8 x3,
// 4x4 x2, 2x2 x2.
fn transition_mosaic_oneway(state: &mut GameState) {
    mosaic_pass(state, MosaicSource::Fb1, 8, &MOSAIC_OFFSETS_8X8);
    mosaic_pass(state, MosaicSource::Fb1, 8, &MOSAIC_OFFSETS_8X8);
    mosaic_pass(state, MosaicSource::Fb1, 8, &MOSAIC_OFFSETS_8X8);
    mosaic_pass(state, MosaicSource::Fb1, 4, &MOSAIC_OFFSETS_4X4);
    mosaic_pass(state, MosaicSource::Fb1, 4, &MOSAIC_OFFSETS_4X4);
    mosaic_pass(state, MosaicSource::Fb1, 2, &MOSAIC_OFFSETS_2X2);
    mosaic_pass(state, MosaicSource::Fb1, 2, &MOSAIC_OFFSETS_2X2);
}

// = segvga:2f87 transition_mosaic_medium (dispatch entry 19) — code 0x26:
// 2x2 then 4x4 on the old image, palette_flush, 4x4 then 2x2 on the new one.
fn transition_mosaic_medium(state: &mut GameState) {
    // = segvga:2f87 call transition_snapshot_screen_to_fb2; 2f8a/2f8b ds = si.
    transition_snapshot_screen_to_fb2(state);
    mosaic_pass(state, MosaicSource::Fb2, 2, &MOSAIC_OFFSETS_2X2);
    mosaic_pass(state, MosaicSource::Fb2, 4, &MOSAIC_OFFSETS_4X4);
    // = segvga:2f93 push cs; call palette_flush.
    palette_flush(state);
    // = segvga:2f97 pop ds — the de-pixelate half reads fb1.
    mosaic_pass(state, MosaicSource::Fb1, 4, &MOSAIC_OFFSETS_4X4);
    mosaic_pass(state, MosaicSource::Fb1, 2, &MOSAIC_OFFSETS_2X2);
}

// = segvga:2f9f transition_mosaic_fine (dispatch entry 20) — code 0x28: three
// 2x2 passes on the old image, palette_flush, one 2x2 pass on the new one.
fn transition_mosaic_fine(state: &mut GameState) {
    // = segvga:2f9f call transition_snapshot_screen_to_fb2; 2fa2/2fa3 ds = si.
    transition_snapshot_screen_to_fb2(state);
    mosaic_pass(state, MosaicSource::Fb2, 2, &MOSAIC_OFFSETS_2X2);
    mosaic_pass(state, MosaicSource::Fb2, 2, &MOSAIC_OFFSETS_2X2);
    mosaic_pass(state, MosaicSource::Fb2, 2, &MOSAIC_OFFSETS_2X2);
    // = segvga:2fae push cs; call palette_flush.
    palette_flush(state);
    // = segvga:2fb2 pop ds — the last pass reads fb1.
    mosaic_pass(state, MosaicSource::Fb1, 2, &MOSAIC_OFFSETS_2X2);
}

// = segvga:2fb7 mosaic_offsets_2x2 — the sample position (row * 320 + col
// within the block) each 2×2 mosaic frame reads, cycled in this order.
const MOSAIC_OFFSETS_2X2: [usize; 4] = [0x000, 0x141, 0x001, 0x140];

// = segvga:2fc1 mosaic_pass_2x2 / segvga:2ff9 mosaic_pass_4x4 / segvga:3031 mosaic_pass_8x8 — one mosaic
// level: bx = [bp] (the PIT counter at entry), then walk the offset table,
// stamping the whole game area once per entry, restarting the table at its
// 0xffff sentinel, until the stamp's tick check (segvga:3082, `[bp] - bx <
// 0x24`) clears the carry. DOS re-stamps as fast as the CPU allows; the
// port presents one frame per PIT tick.
fn mosaic_pass(state: &mut GameState, source: MosaicSource, size: usize, offsets: &[usize]) {
    // = segvga:30c6 / 3091 / 3048 di = fb_base_ofs.
    let fb_base = state.y_offset as usize * state.screen.w() as usize;
    let start = state.game_ticks();
    let mut i = 0;
    loop {
        let ofs = offsets[i];
        // = the `js` restart at the table's 0xffff sentinel.
        i = (i + 1) % offsets.len();
        {
            let GameState {
                screen,
                framebuffer,
                framebuffer_saved,
                ..
            } = state;
            let src = match source {
                MosaicSource::Fb2 => framebuffer_saved,
                MosaicSource::Fb1 => framebuffer,
            };
            mosaic_stamp(screen.pixels_mut(), src.pixels(), fb_base, size, ofs);
        }
        state.present_transition_frame_ticks(1);
        // = segvga:3082..308a `mov ax,[bp]; sub ax,bx; cmp ax,24h` — carry
        // (another table entry) while under 0x24 ticks.
        if state.game_ticks().wrapping_sub(start) >= MOSAIC_PASS_TICKS {
            break;
        }
    }
}

// = segvga:2fd7 — the 16 framebuffer offsets that drive the dotted-column
// reveal. Each is one (col, row) phase within a 4×4 pixel block:
// `ofs % 4` selects the column and `ofs / 320` (0..4) the row. Together
// they name every cell of a 4×4 block, but in a scrambled visit order so
// the dots appear to scatter in rather than march. In DOS the table is
// terminated by a 0xffff sentinel; here the array length stands in for it.
const DOTTED_COLUMNS_OFFSETS: [usize; 16] = [
    0x0141, 0x03c0, 0x0283, 0x0002, 0x0140, 0x03c2, 0x0000, 0x0281, 0x0003, 0x03c1, 0x0142, 0x03c3,
    0x0282, 0x0001, 0x0143, 0x0280,
];

// = segvga:2fd7 mosaic_offsets_4x4 — the 16 sample positions of a 4×4 block.
const MOSAIC_OFFSETS_4X4: [usize; 16] = [
    0x141, 0x3c0, 0x283, 0x002, 0x140, 0x3c2, 0x000, 0x281, 0x003, 0x3c1, 0x142, 0x3c3, 0x282,
    0x001, 0x143, 0x280,
];

// = segvga:300f mosaic_offsets_8x8 — 16 sample positions (rows 2..5, cols
// 0..5) of an 8×8 block.
const MOSAIC_OFFSETS_8X8: [usize; 16] = [
    0x3c3, 0x640, 0x505, 0x284, 0x3c0, 0x644, 0x280, 0x503, 0x285, 0x643, 0x3c4, 0x645, 0x504,
    0x283, 0x3c5, 0x502,
];

// = segvga:3087 `cmp ax,24h` — each mosaic pass runs for 0x24 PIT ticks.
const MOSAIC_PASS_TICKS: u64 = 0x24;

// = segvga:30c5 mosaic_stamp_2x2 / segvga:308c mosaic_stamp_4x4 / segvga:3047 mosaic_stamp_8x8 — stamp the
// 152-row game area as `size`×`size` blocks: every block takes the single
// source pixel at `ofs` (row * 320 + col) inside it. 2×2: `lodsb; inc si`
// then the pair written to di and di+320; 4×4 / 8×8: `lodsb; add si,3/7`
// then 4 / 8 rows of `stosw`s.
fn mosaic_stamp(dst: &mut [u8], src: &[u8], fb_base: usize, size: usize, ofs: usize) {
    const W: usize = 320;
    for by in 0..MOSAIC_ROWS / size {
        let row0 = fb_base + by * size * W;
        for bx in 0..W / size {
            let block = row0 + bx * size;
            let v = src.get(block + ofs).copied().unwrap_or(0);
            for r in 0..size {
                let d = block + r * W;
                if d + size <= dst.len() {
                    dst[d..d + size].fill(v);
                }
            }
        }
    }
}

// ===== Command/dialogue verb-panel fold (segvga panel_anim) ================
//
// The bottom command/verb panel is revealed with a vertical accordion fold —
// the OLD panel squishes toward its centre to a solid band, then the NEW panel
// (staged into fb1 while in_transition was armed by screen_overlay_request_transition) expands back out.
// This is segvga `panel_anim` (vga_effect_dispatch effect 0x18 =
// panel_anim_play_step), driven by play_pending_panel_fold which steps it 17 frames. It uses
// the SAME fold parameter table as transition_vertical_fold, scoped to the panel
// rect (= panel_anim_frame's hardcoded col=92, row=159, 136x41).

// The panel rect. The fold is centred on the row pair 178/179 (DOS bp=0xdedc /
// di=0xe01c) and reaches PANEL_HALF rows each way (DOS dx=0x14 = 20).
const PANEL_X0: u16 = 92;

const PANEL_W: u16 = 136;

const PANEL_UP: u16 = 178;

const PANEL_DN: u16 = 179;

const PANEL_HALF: u16 = 20;

// = segvga:30f2 panel_fold_table (al = rows copied, ah = rows skipped),
// indexed by frame 1..0x11; symmetric around frame 9 (fully collapsed). Same
// values as transition_vertical_fold's FOLD_LINES. play_pending_panel_fold plays frames
// 0x11..1: the closing half (> 9) squishes the old panel, frame 9 is the
// solid-fill midpoint, the opening half (< 9) expands the new (fb1) panel.
const PANEL_FOLD: [(u16, u16); 18] = [
    (0, 0),
    (17, 1),
    (7, 1),
    (4, 1),
    (2, 1),
    (3, 2),
    (4, 5),
    (1, 2),
    (1, 5),
    (0, 0),
    (1, 5),
    (1, 2),
    (4, 5),
    (3, 2),
    (2, 1),
    (4, 1),
    (7, 1),
    (17, 1),
];

// Which buffer the fold reads its source rows from. DOS holds it in `ds` and
// switches it at the cl == 9 midpoint (= segvga:3126 mov ds,[02537]).
#[derive(Clone, Copy)]
enum FoldSource {
    /// First half: the saved old screen, snapshotted into fb2 (= ds = [02535]).
    OldScreen,
    /// Second half: the new image composed in fb1 (= ds = [02537]).
    NewImage,
}

// = segvga:3130 transition_vertical_fold (dispatch entry [2], code 0x04) — the
// vertical centre-fold reveal used by the LOOK AT MIRROR still (seg000:0ea6
// look_at_mirror). It compresses the old screen toward the centre line until it
// collapses to a thin band (first half), uploads the new palette at the step-9
// midpoint, then expands the new image back out from the centre (second half) —
// reading as the room "folding" away to reveal the mirror. `dl` selects the
// table walk (segvga:3140..3147): dl >= 0 sets cx = 0xff11 (cl 0x11 -> 1,
// 17 steps); dl < 0 sets cx = 0x0101 (cl 1 -> 0x10, 16 steps). The table at
// segvga:30f2 is symmetric about its cl == 9 midpoint, so both directions
// play the same squish / clear / expand — the forward walk only stops one
// step short of the final (17, 1) expansion. Every current caller passes
// dx = 0.
fn transition_vertical_fold(state: &mut GameState, dl: u8) {
    // = segvga:3130 call loc_02596 — snapshot the visible screen into fb2
    state.framebuffer_saved.copy_from(&state.screen);

    const FOLD_LINES: [(u16, u16); 8] = [
        (17, 1),
        (7, 1),
        (4, 1),
        (2, 1),
        (3, 2),
        (4, 5),
        (1, 2),
        (1, 5),
    ];

    const FOLD_LINES_REV: [(u16, u16); 8] = [
        (1, 5),
        (1, 2),
        (4, 5),
        (3, 2),
        (2, 1),
        (4, 1),
        (7, 1),
        (17, 1),
    ];

    let mut step = 0;

    if DUMP_FRAMES {
        state
            .screen
            .write_ppm_scaled(
                &state.screen_pal,
                &format!("../transition-fold-{}.ppm", step),
            )
            .unwrap();
        step += 1;
    }

    // = segvga:3163..316a the forward walk (cl 1 -> 0x10) ends before the
    //   table's last entry; the reverse walk (cl 0x11 -> 1) plays all of it.
    let second_half: &[(u16, u16)] = if (dl as i8) < 0 {
        &FOLD_LINES_REV[..7]
    } else {
        &FOLD_LINES_REV
    };

    // First half (cl 0x11..0x0a, or 1..8): squish the OLD screen (snapshotted
    // into fb2) toward the centre line; clear_at_end runs the cl == 9
    // band-clear.
    transition_vertical_fold_part(state, &FOLD_LINES, &mut step, FoldSource::OldScreen, true);

    // = segvga:311a midpoint: the band-clear above (loc_0311a) erased the
    // collapsed old image; = segvga:3126 mov ds,[02537] switches the source to
    // fb1 (expressed by the NewImage half below reading fb1, not by copying it
    // into fb2); = segvga:312c palette_flush uploads the new palette while the
    // band is black. DOS presents this cl == 9 frame after the flush, so the
    // present comes here rather than inside clear_at_end.
    palette_flush(state);
    if DUMP_FRAMES {
        state
            .screen
            .write_ppm_scaled(
                &state.screen_pal,
                &format!("../transition-fold-{}.ppm", step),
            )
            .unwrap();
        step += 1;
    }
    state.present_transition_frame();

    // Second half (cl 8..1, or 10..0x10): expand the NEW image (fb1) back out
    // from the centre.
    transition_vertical_fold_part(state, second_half, &mut step, FoldSource::NewImage, false);

    // = segvga:316c retf — the fold draws only the centre band; the transition
    // wrapper's gfx_copy_whole_framebuf_to_screen lays down the final full fb1
    // image (covering the rows the fold never reaches), so no in-fold full-screen
    // copy is done here.
    if DUMP_FRAMES {
        state
            .screen
            .write_ppm_scaled(
                &state.screen_pal,
                &format!("../transition-fold-{}.ppm", step),
            )
            .unwrap();
    }
}

// = segvga:316d transition_fold_kernel — one frame of the fold: both halves
// step toward the centre line together.
fn transition_vertical_fold_part(
    state: &mut GameState,
    lines: &[(u16, u16)],
    step: &mut i32,
    source: FoldSource,
    clear_at_end: bool,
) {
    const MID_Y: u16 = 75;
    let mut last_dst_dy = MID_Y;

    fn copy_line(dst_fb: &mut FrameBuffer, dst_y: u16, src_fb: &FrameBuffer, src_y: u16) {
        for x in 0..320 {
            dst_fb.set(x, dst_y, src_fb.get(x, src_y));
        }
    }

    fn clear_line(dst_fb: &mut FrameBuffer, dst_y: u16) {
        for x in 0..320 {
            dst_fb.set(x, dst_y, 0);
        }
    }

    for (copy, skip) in lines.iter().copied() {
        // = ds — the fold source: fb2 (snapshotted old screen) in the first
        //   half, fb1 (the new image) after the midpoint source switch.
        let src_fb = match source {
            FoldSource::OldScreen => &state.framebuffer_saved,
            FoldSource::NewImage => &state.framebuffer,
        };
        let dst_fb = &mut state.screen;
        let mut src_dy = 0;
        let mut dst_dy = 0;

        'pass: loop {
            for _ in 0..copy {
                copy_line(dst_fb, MID_Y - dst_dy, src_fb, MID_Y - src_dy);
                copy_line(dst_fb, MID_Y + dst_dy + 1, src_fb, MID_Y + src_dy + 1);
                if MID_Y - src_dy == 0 {
                    break 'pass;
                }
                src_dy += 1;
                dst_dy += 1;
            }
            src_dy += skip;
            if src_dy >= MID_Y {
                break;
            }
        }

        // Blank the rows above/below the new fill that the previous pass had drawn.
        for y in dst_dy..=last_dst_dy {
            clear_line(dst_fb, MID_Y - y);
            clear_line(dst_fb, MID_Y + y + 1);
        }
        last_dst_dy = dst_dy;

        if DUMP_FRAMES {
            state
                .screen
                .write_ppm_scaled(
                    &state.screen_pal,
                    &format!("../transition-fold-{}.ppm", step),
                )
                .unwrap();
            *step += 1;
        }

        state.present_transition_frame();
    }

    if clear_at_end {
        // = segvga:311a loc_0311a — the cl == 9 midpoint clears 0x12c0 words
        // (= 30 rows of the 320-wide screen) from the top fold extent [03118]
        // (di, the dest pointer just above the collapsed band). This erases the
        // squished old image so the second half expands the new image from black.
        // The caller flushes the palette and presents the resulting frame.
        let top = MID_Y - last_dst_dy;
        let dst_fb = &mut state.screen;
        for row in top..top + 30 {
            clear_line(dst_fb, row);
        }
    }
}

impl GameState {
    // = segvga:3200 vga_effect_dispatch / seg000:c0d5 blit_fb1_to_screen_effect — present fb1 to the visible screen
    // = segvga:3500 blit_water_ripple / segvga:356f fb_row_copy_shifted / segvga:3581 blit_zoom_shimmer / segvga:35c8 fb_blit_2x_scaled
    //   — the four blit-mode effects implemented in the arms below.
    // through the segvga vga_effect_dispatch vtable (effect = `al`). The full
    // dispatcher (vga_effect_dispatch, segvga:3200) reduces `effect` mod 0x1a and
    // jumps through blit_mode_dispatch_table (segvga:31e6) to one of 13 effects;
    // only the two the PALACE PLAN drives are wired here (every other effect —
    // transition_tick 0x0c, panel_anim 0x18, … — is invoked from its own ported
    // site). DOS scrolls live VGA memory, so the motion is visible as it runs;
    // the port renders each outer pass into `screen`, presents it, and paces one
    // PIT tick per pass (DOS has no explicit timer here — the scroll is paced
    // implicitly by CPU speed — so the 1-tick cadence is a port-side stand-in
    // that makes the reveal perceptible without pegging a core).
    pub(crate) fn blit_fb1_to_screen_effect(&mut self, effect: u8, rect: Rect) {
        match effect {
            // = segvga:33ca blit_mode_dispatch_table[8] (segvga:31e6)
            //   blit_scroll_rect_down: the open reveal. The source origin steps
            //   from y2-2 up to y1 (si -= 0x280 per pass), each pass redrawing a
            //   taller bottom-anchored window of fb1 at the rect top.
            0x10 => {
                let mut src_row = rect.y1 - 2;
                loop {
                    let start = self.game_ticks();
                    scroll_rect_down_pass(
                        &mut self.screen,
                        &self.framebuffer,
                        self.y_offset,
                        rect,
                        src_row,
                    );
                    self.send_frame_to_display();
                    self.sleep_ticks(start, 1);

                    // = jnb loc_033ef: the outer loop ends once the source origin
                    //   reaches the rect top (si -= 0x280 would borrow).
                    if src_row <= rect.y0 {
                        break;
                    }
                    src_row -= 2;
                }

                // = jmp vga_copy_rect: the final clean full-rect copy (identical
                //   to the last pass, mirroring the DOS tail jump).
                let yoff = self.y_offset as i16;
                let r = Rect {
                    x0: rect.x0,
                    y0: rect.y0 + yoff,
                    x1: rect.x1,
                    y1: rect.y1 + yoff,
                };
                vga_copy_rect(&mut self.screen, &self.framebuffer, r);
                self.send_frame_to_display();
            }

            // = segvga:3429 blit_mode_dispatch_table[9] (segvga:31e6)
            //   blit_scroll_rect_up: the close reveal. The block height bx steps
            //   down by six per pass (110, 104, …, 2, then a final 0 pass);
            //   blit_scroll_rect_up has no tail vga_copy_rect (its fill blocks
            //   lay down every row of fb1).
            0x12 => {
                let mut bx = (rect.y1 - rect.y0) - 6;
                loop {
                    let start = self.game_ticks();
                    scroll_rect_up_pass(
                        &mut self.screen,
                        &self.framebuffer,
                        self.y_offset,
                        rect,
                        bx,
                    );
                    self.send_frame_to_display();
                    self.sleep_ticks(start, 1);

                    // = bx -= 6; jnb loc_03445 / cmp bx,-6; mov bx,0; jnz — a
                    //   borrow that lands on -6 ends the loop; any other borrow
                    //   runs one last pass at bx = 0.
                    let next = bx - 6;
                    if next >= 0 {
                        bx = next;
                    } else if next == -6 {
                        break;
                    } else {
                        bx = 0;
                    }
                }
            }

            // = segvga:3581 blit_mode_dispatch_table[0] (segvga:31e6)
            //   blit_zoom_shimmer: blit the rect's interior from the clean
            //   fb1 source (ds, per the c0d6/c0da buffer setup) into the
            //   screen (es) at 2x scale around the rect top-left, cycling
            //   the 2x2 sub-pixel source offsets (zoom_tile_offsets,
            //   segvga:2fb7), until the caller's tick budget runs out (cx —
            //   the globe zoom box, globe_zoom_box_shimmer_step, passes 10).
            //   Every pass rewrites the whole interior from fb1, so anything
            //   drawn over the screen inside the rect (the previous zoom-box
            //   outline) is erased each pass.
            0x00 => {
                // = segvga:358b..359e half width/height; nothing on a flat
                //   rect.
                let half_w = ((rect.x1 - rect.x0) / 2) as usize;
                let half_h = ((rect.y1 - rect.y0) / 2) as usize;
                if half_w == 0 || half_h == 0 {
                    return;
                }
                let yoff = self.y_offset as usize;
                let w = self.screen.w() as usize;
                let origin = (rect.y0 as usize + yoff) * w + rect.x0 as usize;

                // = segvga:35a0 the entry tick, segvga:35bb..35c4 the loop
                //   until cx (10) ticks elapse.
                let start = self.game_ticks();
                let mut jitter = [0usize, 321, 1, 320].iter().copied().cycle();
                loop {
                    // = segvga:35c8 fb_blit_2x_scaled — lodsb from ds (fb1)
                    //   every other byte/row, stosw doubled into es (screen).
                    let off = jitter.next().unwrap();
                    let src = self.framebuffer.pixels();
                    let dst = self.screen.pixels_mut();
                    for j in 0..half_h {
                        let di = origin + 2 * j * w;
                        let si = di + off;
                        for i in 0..half_w {
                            let c = src[si + 2 * i];
                            dst[di + 2 * i] = c;
                            dst[di + 2 * i + 1] = c;
                            dst[di + w + 2 * i] = c;
                            dst[di + w + 2 * i + 1] = c;
                        }
                    }
                    self.send_frame_to_display();
                    // DOS repeats at CPU speed; pace one PIT tick per pass so
                    // the shimmer is perceptible without pegging a core.
                    self.sleep_ticks(self.game_ticks(), 1);
                    if self.game_ticks() - start >= 10 {
                        break;
                    }
                }
            }

            // = segvga:3500 blit_mode_dispatch_table[5] (segvga:31e6)
            //   blit_water_ripple — one pass per call (the vision-dream
            //   shimmer task fires it every 6 ticks): the rect's rows copy
            //   from fb1 with a per-row horizontal shift from the wave table
            //   (segvga:3487), the wave origin advancing one row per call
            //   (data_segvga_034fc). DOS smears the rows in place on the VGA
            //   surface; the port simplifies to a clean shifted copy from fb1,
            //   which reads the same rolling-wave distortion without
            //   accumulating smear.
            0x0a => {
                // = segvga:3500 call palette_cycle_water.
                palette_cycle_water(self);
                // = segvga:3487 wave_displacement_tbl — a ±5 px sine-like
                //   ramp, 116 rows per period.
                #[rustfmt::skip]
                const WAVE: [i16; 116] = [
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2,
                    3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5,
                    5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 3, 3, 3, 3,
                    3, 3, 2, 2, 2, 2, 2, 1, 1, 1, 1, 0, 0, 0, -1, -1,
                    -1, -2, -2, -2, -2, -3, -3, -3, -3, -3, -4, -4, -4, -4, -4, -4,
                    -5, -5, -5, -5, -5, -5, -5, -5, -5, -4, -4, -4, -4, -4, -4, -3,
                    -3, -3, -3, -3, -3, -2, -2, -2, -2, -2, -2, -2, -1, -1, -1, -1,
                    -1, -1, -1, -1,
                ];
                let phase = self.vision_shimmer_phase as usize;
                self.vision_shimmer_phase = self.vision_shimmer_phase.wrapping_add(1);
                let yoff = self.y_offset as i16;
                let w = self.screen.w() as i16;
                let src = self.framebuffer.pixels();
                let dst = self.screen.pixels_mut();
                for row in rect.y0..rect.y1 {
                    let shift = WAVE[(row as usize + phase) % WAVE.len()];
                    let y = (row + yoff) as usize;
                    for x in rect.x0..rect.x1 {
                        let sx = (x + shift).clamp(0, w - 1) as usize;
                        dst[y * w as usize + x as usize] = src[y * w as usize + sx];
                    }
                }
                self.send_frame_to_display();
            }

            // = the remaining vga_effect_dispatch effects are unported; this
            //   dispatcher only serves the PALACE PLAN and GLOBE effects.
            other => {
                eprintln!("blit_fb1_to_screen_effect: unhandled effect 0x{other:02x}");
            }
        }
    }
}

// = segvga:3280 panel_solid_fill — the fully-collapsed (frame 9) look: 16 rows of
// 0xfe, an 8-row 0xf2/0x08 checkerboard hinge, then 16 rows of 0xfe.
fn panel_solid_fill(dst: &mut FrameBuffer) {
    let mut y = 159u16;
    for _ in 0..16 {
        panel_fill_row(dst, y, 0xfe);
        y += 1;
    }
    // = segvga:3298 ax=0xf208 (bytes 0x08,0xf2); xchg al,ah each row.
    let (mut b0, mut b1) = (0x08u8, 0xf2u8);
    for _ in 0..8 {
        for dx in 0..PANEL_W {
            dst.set(PANEL_X0 + dx, y, if dx % 2 == 0 { b0 } else { b1 });
        }
        std::mem::swap(&mut b0, &mut b1);
        y += 1;
    }
    for _ in 0..16 {
        panel_fill_row(dst, y, 0xfe);
        y += 1;
    }
}

fn panel_copy_row(dst: &mut FrameBuffer, dy: u16, src: &FrameBuffer, sy: u16) {
    for dx in 0..PANEL_W {
        let x = PANEL_X0 + dx;
        dst.set(x, dy, src.get(x, sy));
    }
}

fn panel_fill_row(dst: &mut FrameBuffer, dy: u16, color: u8) {
    for dx in 0..PANEL_W {
        dst.set(PANEL_X0 + dx, dy, color);
    }
}

// Copy the whole panel rect (rows 159..199, incl. the bottom border) between two
// buffers. = the vga_copy_rect(col=92,row=159,136x41) calls that back up the old
// panel (play_step frame 0x11) and lay down the final new panel (frame 1).
fn panel_copy_rect(dst: &mut FrameBuffer, src: &FrameBuffer) {
    for y in 159..200 {
        panel_copy_row(dst, y, src, y);
    }
}

// = segvga:32c1 panel_anim_frame — one panel animation frame from the fold table.
// One fold frame: squish `src`'s panel toward the centre — copy `al` rows then
// skip `ah` source rows, repeating outward from the centre pair (178/179) — and
// fill the vacated edge rows with the panel-closed colour (0xfe). = segvga:32c1
// panel_anim_frame, scoped to the panel rect. Mirrors transition_vertical_fold's
// squish but repaints the whole panel each frame instead of clearing the delta.
fn panel_fold_squish(dst: &mut FrameBuffer, src: &FrameBuffer, al: u16, ah: u16) {
    let mut src_d = 0u16;
    let mut dst_d = 0u16;
    'pass: loop {
        for _ in 0..al {
            panel_copy_row(dst, PANEL_UP - dst_d, src, PANEL_UP - src_d);
            panel_copy_row(dst, PANEL_DN + dst_d, src, PANEL_DN + src_d);
            // = `if MID - src_dy == 0`: the source reached the panel edge row.
            if src_d == PANEL_HALF - 1 {
                break 'pass;
            }
            src_d += 1;
            dst_d += 1;
        }
        src_d += ah;
        if src_d >= PANEL_HALF {
            break;
        }
    }
    // = segvga:3330 fill the vacated edge rows with the panel-closed colour.
    for d in dst_d..PANEL_HALF {
        panel_fill_row(dst, PANEL_UP - d, 0xfe);
        panel_fill_row(dst, PANEL_DN + d, 0xfe);
    }
}

// = segvga:3223 panel_anim_play_all [not needed] — the blocking variant
// (blit_mode_dispatch_table entry 10, effect 0x14) that loops cl = 17..1 over
// the same frame routine in one call; no DOS caller passes effect 0x14.
// = segvga:3382 panel_anim_play_step (blit_fb1_to_screen_effect al=0x18): render ONE
// command-panel fold frame (`frame` = the DOS cl, 0x11..1) straight to the visible
// screen. play_pending_panel_fold drives this once per loop pass. The verb panel was
// staged into fb1 (in_transition routed draw_command_menu_item there); the closing
// half (frame > 9) squishes the backed-up old panel away, frame 9 is the solid
// hinge, and the opening half (< 9) expands the new fb1 panel out.
//
// frame 0x11 (first pass) backs the on-screen panel up into fb2 before squishing it,
// and frame 1 (last pass) lays down the clean new panel from fb1.
pub fn panel_anim_play_step(state: &mut GameState, frame: u16) {
    if frame == 0x11 {
        // = segvga:3387 (cx==0x11): back up the current on-screen panel into fb2
        //   so the closing half squishes it.
        panel_copy_rect(&mut state.framebuffer_saved, &state.screen);
    }

    if frame == 9 {
        // = segvga:32c4 the cl==9 special case: the fully-collapsed band.
        panel_solid_fill(&mut state.screen);
    } else if frame == 1 {
        // = segvga:33ba (cx==1): lay down the full new panel from fb1.
        panel_copy_rect(&mut state.screen, &state.framebuffer);
    } else {
        let (al, ah) = PANEL_FOLD[frame as usize];
        // = segvga:3382 panel_anim_play_step `cmp cl,9; jb`: the closing half (frame > 9) squishes the
        //   old panel snapshot (fb2); the opening half reads fb1.
        if frame > 9 {
            panel_fold_squish(&mut state.screen, &state.framebuffer_saved, al, ah);
        } else {
            panel_fold_squish(&mut state.screen, &state.framebuffer, al, ah);
        }
    }
}

// = segvga:33ca blit_scroll_rect_down — render one outer pass of the
// downward scroll reveal. The DOS routine walks an outer loop whose visible
// window grows two rows at a time (bx = 2, 4, 6, …) while the source origin
// climbs two rows at a time from the rect bottom (si -= 0x280 per pass); each
// pass redraws the top of the rect from a bottom-anchored window of `src`, so
// the content appears to scroll down into view. This helper is one such pass:
// for `src_row` in the rect's row range (the DOS source origin, stepping from
// y1-2 down to y0), copy the rect's columns from source rows [src_row, y1)
// into `dst` rows [y0, y0 + (y1-src_row)). fb_base_ofs (`y_offset`) is added to
// every row, exactly as calc_fb_offset (segvga:0c10) does. The caller (the
// blit_fb1_to_screen_effect dispatcher) drives the outer loop and presents
// after each pass, since the port renders into a buffer rather than live VGA.
pub fn scroll_rect_down_pass(
    dst: &mut FrameBuffer,
    src: &FrameBuffer,
    y_offset: u16,
    rect: Rect,
    src_row: i16,
) {
    let stride = dst.w() as usize;
    let x0 = rect.x0 as usize;
    let x1 = rect.x1 as usize;
    let yoff = y_offset as usize;
    // = bx = y2 - src_row: the count of rows copied this pass.
    let n = (rect.y1 - src_row) as usize;
    let dpix = dst.pixels_mut();
    let spix = src.pixels();
    for i in 0..n {
        // = rep movsw of one rect-wide row, then si/di advance one scanline.
        let s = (yoff + src_row as usize + i) * stride;
        let d = (yoff + rect.y0 as usize + i) * stride;
        dpix[d + x0..d + x1].copy_from_slice(&spix[s + x0..s + x1]);
    }
}

// = segvga:3429 blit_scroll_rect_up — render one outer pass of the upward
// scroll reveal. Each DOS pass first scrolls the on-screen rect up by six rows
// over its top `bx` rows (es:di <- es:[di+0x780], ds = es = screen), then lays
// six fresh rows of `src` at the bottom of that scrolled region (ds = fb1);
// across passes `bx` shrinks by six (110, 104, …, 2, 0) so `src` scrolls up
// into view from the bottom. fb_base_ofs (`y_offset`) is applied like
// calc_fb_offset. The caller drives the outer loop and presents per pass.
pub fn scroll_rect_up_pass(
    dst: &mut FrameBuffer,
    src: &FrameBuffer,
    y_offset: u16,
    rect: Rect,
    bx: i16,
) {
    let stride = dst.w() as usize;
    let x0 = rect.x0 as usize;
    let x1 = rect.x1 as usize;
    let yoff = y_offset as usize;
    let y0 = yoff + rect.y0 as usize;
    let bx = bx as usize;
    // = seg000:3452 loc_03452: scroll the top `bx` rows up by six (row j <- row j+6). Top-
    //   to-bottom is safe — each source row j+6 is read before it is later
    //   overwritten at step j+6.
    {
        let dpix = dst.pixels_mut();
        for j in 0..bx {
            let d = (y0 + j) * stride;
            let s = (y0 + j + 6) * stride;
            dpix.copy_within(s + x0..s + x1, d + x0);
        }
    }
    // = seg000:3467 loc_03467: fill the six rows below the scrolled region from `src` at
    //   the same offset (ds = fb1, si = di).
    let dpix = dst.pixels_mut();
    let spix = src.pixels();
    for k in 0..6 {
        let d = (y0 + bx + k) * stride;
        dpix[d + x0..d + x1].copy_from_slice(&spix[d + x0..d + x1]);
    }
}

impl GameState {
    // = segvga:3602 xor_bracket_zoom_to_panel / segvga:372d vga_xor_box_20 — the troop-contact popup's open
    // effect (vga_effect_dispatch al=2): a 20x20 XOR box stepping from the
    // troop icon to the panel centre (8 frames), then the corner brackets
    // expanding from a centred 20x20 out to the panel rect (8 frames). Each
    // phase runs twice with identical frames — the XOR draws accumulate over
    // the first pass and the second pass erases them — pacing one present
    // interval (loc_segvga_02572) per frame.
    pub(crate) fn xor_bracket_zoom_to_panel(&mut self, panel: Rect, icon_pos: (i16, i16)) {
        // The animation is a foreground timing effect; headless runs skip it.
        if self.is_headless() {
            return;
        }
        // = segvga:3603..361e the target: the icon point, x clamped to
        //   [0, 300], y to >= 0.
        let target = (icon_pos.0.clamp(0, 0x12c), icon_pos.1.max(0));
        self.xor_bracket_anim_setup(panel, target);
        // = segvga:362a..3652 the box trail: 2 passes x 8 frames from the
        //   target, advancing by the trail step after each frame's box
        //   (vga_xor_box_20 = the outline at fixed size 20).
        let (dx, dy) = self.xor_bracket_anim_move_step;
        for _ in 0..2 {
            let (mut x, mut y) = target;
            for _ in 0..8 {
                vga_xor_rect_outline_inner(self, x, y, 0x14, 0x14);
                self.present_transition_frame();
                x += dx;
                y += dy;
            }
        }
        // = segvga:3654..36ac the brackets: 2 passes x 8 frames from a centred
        //   20x20, growing by the expand step after each frame; every frame
        //   drawn is latched in data_035fa..03600 for the close to shrink
        //   back from.
        let (ex, ey) = self.xor_bracket_anim_expand_step;
        for _ in 0..2 {
            let (mut x, mut y) = self.xor_bracket_anim_center;
            let (mut w, mut h) = (0x14, 0x14);
            for _ in 0..8 {
                self.xor_bracket_anim_shape = (x, y, w, h);
                self.xor_corner_brackets(x, y, w, h);
                self.present_transition_frame();
                x -= ex;
                w += 2 * ex;
                y -= ey;
                h += 2 * ey;
            }
        }
    }
}

impl GameState {
    // = segvga:36b0 xor_bracket_anim_setup — stage the bracket-zoom animation
    // from the panel rect (es:di, the record's +0..+7) and the target point
    // (data_035f2/035f4): the origin of a 20x20 box centred on the panel
    // (data_035f6/035f8), the per-frame bracket expand step (half the panel
    // extent less 20, / 8 — data_035ee/035f0) and the per-frame trail step
    // from the target to that centre (/ 8, signed — data_035ea/035ec).
    pub(crate) fn xor_bracket_anim_setup(&mut self, panel: Rect, target: (i16, i16)) {
        // = segvga:36b0..36e7 per axis: ax = (extent - 20) / 2; centre origin
        //   = panel origin + ax; expand step = ax / 8.
        let half_w = (panel.x1 - panel.x0 - 0x14) >> 1;
        let half_h = (panel.y1 - panel.y0 - 0x14) >> 1;
        let center = (panel.x0 + half_w, panel.y0 + half_h);
        self.xor_bracket_anim_center = center;
        self.xor_bracket_anim_expand_step = (half_w >> 3, half_h >> 3);
        // = segvga:36eb..371e the trail step: (centre - target) / 8, the shift
        //   run on |value| with the sign restored (truncation toward zero).
        let step = |d: i16| (d.abs() >> 3) * d.signum();
        self.xor_bracket_anim_move_step = (step(center.0 - target.0), step(center.1 - target.1));
    }
}

// = segvga:3724 vga_xor_rect_outline (gfx_vtable_vga_xor_rect_outline,
// seg001:3905) — the far entry the seg000 callers reach: it decrements the
// width before the inner, so the caller's width is the visible pixel width
// (the inner walks x..x+w inclusive). Only the width; the height passes
// through undecremented.
pub(crate) fn vga_xor_rect_outline(state: &mut GameState, x: i16, y: i16, w: i16, h: i16) {
    // = segvga:3724 dec si.
    vga_xor_rect_outline_inner(state, x, y, w - 1, h);
}

// = segvga:3733 vga_xor_rect_outline_inner — XOR (with 0x0f) a one-pixel
// rect outline onto the visible screen. The corners come from (x, y) and
// (x + w, y + h), each clamped into 4..=0x13c horizontally and 4..=0x94
// vertically, then the cursor walks top row, right side, bottom row, left
// side. The bottom row lands one row above the clamped bottom corner (the
// DOS `sub cx,2` walk); a box under three rows tall draws top and bottom
// on the same row, cancelling its own XOR — both kept as DOS has them.
// Also the body of vga_xor_box_20 (segvga:372d), the fixed 20x20 wrapper
// the bracket-zoom trail steps with.
pub(crate) fn vga_xor_rect_outline_inner(state: &mut GameState, x: i16, y: i16, w: i16, h: i16) {
    let yoff = state.y_offset as i16;
    // = segvga:3733/3735 the far corner; segvga:3737..3771 the clamps.
    let x0 = x.clamp(4, 0x13c);
    let x1 = (x + w).clamp(4, 0x13c);
    let y0 = y.clamp(4, 0x94);
    let y1 = (y + h).clamp(4, 0x94);
    // = segvga:3773..3778 the walk counts: width + 1 across, height - 2 down.
    let w = x1 - x0 + 1;
    let h = y1 - y0 - 2;
    let screen = &mut state.screen;
    let mut toggle = |x: i16, y: i16| {
        let y = y + yoff;
        let c = screen.get(x as u16, y as u16);
        screen.set(x as u16, y as u16, c ^ 0x0f);
    };
    // = segvga:3784 the top row, left to right.
    for i in 0..w {
        toggle(x0 + i, y0);
    }
    // = segvga:378f the right side, then the step onto the bottom row.
    let mut by = y0;
    if h > 0 {
        for j in 1..=h {
            toggle(x1, y0 + j);
        }
        by = y0 + h + 1;
    }
    // = segvga:379d the bottom row, right to left.
    for i in 0..w {
        toggle(x1 - i, by);
    }
    // = segvga:37a8 the left side, bottom to top.
    if h > 0 {
        for j in 1..=h {
            toggle(x0, by - j);
        }
    }
}

impl GameState {
    // = segvga:37b1 vga_xor_corner_brackets — XOR-draw the corner
    // brackets at (x, y), size (w, h), into the visible screen with colour
    // 0x0f: a 10-pixel horizontal edge segment and a 9-pixel vertical spine
    // at each corner. DOS writes the framebuffer offsets unclamped; the port
    // skips off-screen pixels.
    pub(crate) fn xor_corner_brackets(&mut self, x: i16, y: i16, w: i16, h: i16) {
        let yoff = self.y_offset as i16;
        let scr = &mut self.screen;
        let mut toggle = |px: i16, py: i16| {
            let py = py + yoff;
            if (0..320).contains(&px) && (0..200).contains(&py) {
                let (px, py) = (px as u16, py as u16);
                scr.set(px, py, scr.get(px, py) ^ 0x0f);
            }
        };
        // = segvga:37ba/37cc the top and segvga:37fe/3810 the bottom edge
        //   segments (5 word XORs each = 10 pixels per corner).
        for i in 0..10 {
            toggle(x + i, y);
            toggle(x + w - 10 + i, y);
            toggle(x + i, y + h - 1);
            toggle(x + w - 10 + i, y + h - 1);
        }
        // = segvga:37d7/37f1 the right and segvga:381d/3837 the left spine
        //   segments (9 byte XORs each).
        for j in 1..10 {
            toggle(x + w - 1, y + j);
            toggle(x + w - 1, y + h - 1 - j);
            toggle(x, y + h - 1 - j);
            toggle(x, y + j);
        }
    }
}

impl GameState {
    // = segvga:3841 xor_bracket_zoom_from_panel — the troop-contact popup's close
    // effect (vga_effect_dispatch al=4), the open played backwards from the
    // state xor_bracket_zoom_to_panel staged: the brackets shrink from the last
    // latched shape back towards a centred 20x20 (2 passes x 8 frames, the
    // second pass erasing the first), then the box trail steps from the panel
    // centre back to the icon (2 passes x 8 frames).
    pub(crate) fn xor_bracket_zoom_from_panel(&mut self) {
        if self.is_headless() {
            return;
        }
        // = segvga:3847..388c the brackets, shrinking by the expand step
        //   after each frame.
        let (ex, ey) = self.xor_bracket_anim_expand_step;
        for _ in 0..2 {
            let (mut x, mut y, mut w, mut h) = self.xor_bracket_anim_shape;
            for _ in 0..8 {
                self.xor_corner_brackets(x, y, w, h);
                self.present_transition_frame();
                x += ex;
                w -= 2 * ex;
                y += ey;
                h -= 2 * ey;
            }
        }
        // = segvga:388e..38bc the trail, stepping back from the centre before
        //   each frame's box.
        let (dx, dy) = self.xor_bracket_anim_move_step;
        for _ in 0..2 {
            let (mut x, mut y) = self.xor_bracket_anim_center;
            for _ in 0..8 {
                x -= dx;
                y -= dy;
                vga_xor_rect_outline_inner(self, x, y, 0x14, 0x14);
                self.present_transition_frame();
            }
        }
    }
}

// = segvga:38d8 xor_rect_outline_advance / segvga:39bb xor_rect_outline_reverse
// (vga_effect_dispatch al=6 / al=8, run via run_vga_effect seg000:c0e8) — the
// panel outline scale animation: an XOR rect outline grows from the source
// record's origin `src` + (8, 8) to the panel rect `dst`, or shrinks back on
// the reverse (start at the panel corners, every step negated). 15 frames;
// each advances the top-left by (dst corner - start) / 16 and the extent by
// the dst extent / 16 (the sign restored around the shift, so the steps
// truncate toward zero), XOR-draws the outline, paces one frame interval
// (loc_segvga_02572), then XOR-draws again to erase it.
pub fn xor_rect_outline_anim(state: &mut GameState, src: (i16, i16), dst: Rect, reverse: bool) {
    // The animation is a foreground timing effect; headless runs skip it.
    if state.is_headless() {
        return;
    }
    // = segvga:38d8..38e4 the start point: the source origin + (8, 8).
    let (sx, sy) = (src.0 + 8, src.1 + 8);
    // = segvga:390c..3925 the per-frame extent steps.
    let pw = dst.x1 - dst.x0;
    let ph = dst.y1 - dst.y0;
    // = segvga:392a..395d the per-frame top-left steps.
    let toward = |from: i16, to: i16| {
        let d = to - from;
        if d < 0 { -((-d) >> 4) } else { d >> 4 }
    };
    let step_x = toward(sx, dst.x0);
    let step_y = toward(sy, dst.y0);
    // = segvga:3962..396e the advance starts at the bare source point, size
    //   0; the reverse (segvga:39bb) at the stored panel corners, size = its
    //   extent, with every step negated.
    let (mut x, mut y, mut w, mut h, dx, dy, dw, dh) = if reverse {
        (
            dst.x0,
            dst.y0,
            pw,
            ph,
            -step_x,
            -step_y,
            -(pw >> 4),
            -(ph >> 4),
        )
    } else {
        (sx, sy, 0, 0, step_x, step_y, pw >> 4, ph >> 4)
    };
    // = segvga:3970 xor_rect_outline_animate — the 15-frame loop. DOS XORs
    //   straight onto the visible screen, so the erase is seen at once; the
    //   port presents after the draw and publishes once more after the final
    //   erase on the reverse — the advance callers re-present the panel area
    //   themselves right after (copy_rect_fb1_to_screen sends the frame).
    for _ in 0..15 {
        x += dx;
        y += dy;
        w += dw;
        h += dh;
        vga_xor_rect_outline_inner(state, x, y, w, h);
        state.present_transition_frame();
        // = segvga:39ae the second XOR pass erases the outline.
        vga_xor_rect_outline_inner(state, x, y, w, h);
    }
    if reverse {
        state.send_frame_to_display();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A "water" reference buffer (= the clean fb1): every pixel has bit 7 set
    // and is < 0xf0, with a per-pixel gradient so the draw op's 4x4 smear (which
    // collapses a block to the top-left pixel's colour) is observable.
    fn water_buffer() -> Vec<u8> {
        (0..320 * 200).map(|i| 0x80 + (i % 0x70) as u8).collect()
    }

    const FB_BASE: usize = 24 * 320;

    #[test]
    fn ripple_draw_then_erase_round_trips() {
        // Drawing a band then erasing the SAME band must restore the buffer
        // exactly: the kernel is deterministic (octant counter starts at 0 both
        // times), draw writes only water-range colours, and erase restores every
        // water pixel from fb1 over the identical 4x4 footprints.
        let fb1 = water_buffer();
        // col = 8*frame, mirroring transition_tick's lock-step advance.
        for &(col, frame) in &[(0x10u16, 2u16), (0x18, 3), (0x40, 8), (0x80, 0x10)] {
            let mut screen = fb1.clone();
            transition_kernel(
                &mut screen,
                &fb1,
                FB_BASE,
                RippleOp::Draw,
                col,
                frame,
                0xb0,
                0x5b,
            );
            assert_ne!(
                screen, fb1,
                "draw col={col:#x} frame={frame} changed nothing"
            );
            transition_kernel(
                &mut screen,
                &fb1,
                FB_BASE,
                RippleOp::Erase,
                col,
                frame,
                0xb0,
                0x5b,
            );
            assert_eq!(
                screen, fb1,
                "draw+erase col={col:#x} frame={frame} did not round-trip"
            );
        }
    }

    #[test]
    fn ripple_draw_skips_non_water() {
        // The draw op only smears where the whole 4x4 screen block is "water"
        // (bit 7 set). A screen with no water must be left untouched.
        let fb1 = vec![0x88u8; 320 * 200];
        let mut screen = vec![0x00u8; 320 * 200];
        transition_kernel(
            &mut screen,
            &fb1,
            FB_BASE,
            RippleOp::Draw,
            0x40,
            8,
            0xb0,
            0x5b,
        );
        assert!(
            screen.iter().all(|&p| p == 0),
            "draw must skip regions whose bit 7 is clear",
        );
    }

    #[test]
    fn ripple_erase_edge_skips_first_column() {
        // = segvga:2804 sub cx,8; jz — at col==8 there is no trailing band yet.
        let fb1 = water_buffer();
        let mut screen = vec![0u8; 320 * 200];
        transition_erase_edge(&mut screen, &fb1, FB_BASE, 8, 1);
        assert!(
            screen.iter().all(|&p| p == 0),
            "erase at col=8 must be a no-op",
        );
    }

    fn run_fade_component(src: u8, dst_initial: u8, step_size: u8, cycles: u8) -> u8 {
        let mut dst = dst_initial;
        for dl in (1..=cycles).rev() {
            dst = fade_step_component(src, dst, step_size, dl);
        }
        dst
    }

    #[test]
    fn fade_3a_reaches_max_component() {
        // 0x3a parameters: step=3, cycles=22. A 6-bit palette value of 63
        // must land exactly on 63 after the full fade-up.
        assert_eq!(run_fade_component(63, 0, 3, 22), 63);
    }

    #[test]
    fn fade_3a_reaches_arbitrary_targets() {
        // Spot-check assorted targets across the 6-bit range.
        for src in [0u8, 1, 7, 23, 31, 47, 50, 62, 63] {
            assert_eq!(
                run_fade_component(src, 0, 3, 22),
                src,
                "fade-up to src={src} (step=3, cycles=22) didn't land",
            );
        }
    }

    #[test]
    fn fade_36_reaches_max_component() {
        // 0x36 parameters: step=1, cycles=64. Step=1 means each active
        // outer iteration advances by exactly 1.
        assert_eq!(run_fade_component(63, 0, 1, 64), 63);
    }

    #[test]
    fn fade_36_reaches_arbitrary_targets() {
        for src in [0u8, 1, 7, 23, 31, 47, 50, 62, 63] {
            assert_eq!(
                run_fade_component(src, 0, 1, 64),
                src,
                "fade-up to src={src} (step=1, cycles=64) didn't land",
            );
        }
    }

    #[test]
    fn dotted_lattice_tiles_active_area_exactly() {
        // The 16 table entries, each a 38×80 dot grid, must cover the active
        // 152-row × 320-col area (rows 24..176 with fb_base = 24×320) exactly
        // once — no gaps, no overlap. 16 × 38 × 80 == 152 × 320 == 48640.
        const W: usize = 320;
        const FB_BASE: usize = 24 * W;
        let mut coverage = vec![0u32; W * 200];

        for &ofs in &DOTTED_COLUMNS_OFFSETS {
            let base = FB_BASE + ofs;
            for group in 0..(DOTTED_ROWS >> 2) {
                let mut di = base + group * DOTTED_ROW_STRIDE;
                for _ in 0..DOTTED_COLS {
                    coverage[di] += 1;
                    di += DOTTED_COL_STRIDE;
                }
            }
        }

        let covered = coverage.iter().filter(|&&c| c != 0).count();
        assert_eq!(covered, 152 * W, "dot lattice didn't cover 152 full rows");
        for (i, &c) in coverage.iter().enumerate() {
            let row = i / W;
            if (24..176).contains(&row) {
                assert_eq!(c, 1, "pixel {i} (row {row}) covered {c} times, want 1");
            } else {
                assert_eq!(c, 0, "pixel {i} (row {row}) covered {c} times, want 0");
            }
        }
    }

    #[test]
    fn spiral_tiles_game_area_exactly() {
        // The 64 unique offsets each stamp a 19-group × 40-dot grid; together
        // they tile the 152-row × 320-col game area (fb_base = 0) exactly once —
        // 64 × 19 × 40 == 152 × 320 == 48640. The 65th table entry duplicates
        // 0x0140, so that one 760-pixel cell set is stamped a second time.
        const W: usize = 320;
        let mut coverage = vec![0u32; W * 200];

        for &ofs in &SPIRAL_OFFSETS {
            for group in 0..SPIRAL_ROW_GROUPS {
                let mut di = ofs + group * SPIRAL_ROW_STRIDE;
                for _ in 0..SPIRAL_COLS {
                    coverage[di] += 1;
                    di += SPIRAL_COL_STRIDE;
                }
            }
        }

        // Every pixel of the 152-row game area is covered; nothing below it is.
        for (i, &c) in coverage.iter().enumerate() {
            let row = i / W;
            if row < 152 {
                assert!(c >= 1, "pixel {i} (row {row}) not covered");
            } else {
                assert_eq!(c, 0, "pixel {i} (row {row}) covered {c} times, want 0");
            }
        }
        let covered = coverage.iter().filter(|&&c| c != 0).count();
        assert_eq!(covered, 152 * W, "spiral didn't cover 152 full rows");
        // The lone duplicate entry double-stamps exactly one 760-pixel cell set.
        let twice = coverage.iter().filter(|&&c| c == 2).count();
        assert_eq!(
            twice,
            SPIRAL_COLS * SPIRAL_ROW_GROUPS,
            "expected exactly one duplicated cell set"
        );
        assert!(
            coverage.iter().all(|&c| c <= 2),
            "no pixel should be stamped more than twice"
        );
    }

    #[test]
    fn fade_is_monotonic_nondecreasing() {
        // From any starting dst < src, dst should only ever advance toward
        // src, never overshoot or oscillate.
        let mut dst = 0u8;
        for dl in (1..=22u8).rev() {
            let next = fade_step_component(63, dst, 3, dl);
            assert!(next >= dst, "dst regressed: {dst} -> {next}");
            assert!(next <= 63, "dst overshot 63: {dst} -> {next}");
            dst = next;
        }
    }
}

#[cfg(test)]
mod mosaic_tests {
    use super::mosaic_stamp;

    // mosaic_stamp fills each block with its sampled pixel: an 8×8 block
    // takes the pixel at row 5 / col 5 (offset 0x645) of the source block.
    #[test]
    fn mosaic_stamp_fills_blocks_from_the_sampled_pixel() {
        let mut src = vec![0u8; 320 * 200];
        // Mark the (5, 5) pixel of the block at column 1, row-group 0, with
        // fb_base 320 * 10.
        let fb_base = 320 * 10;
        src[fb_base + 8 + 0x645] = 7;
        let mut dst = vec![1u8; 320 * 200];
        mosaic_stamp(&mut dst, &src, fb_base, 8, 0x645);
        for r in 0..8 {
            assert!(
                dst[fb_base + r * 320 + 8..fb_base + r * 320 + 16]
                    .iter()
                    .all(|&p| p == 7)
            );
            assert!(
                dst[fb_base + r * 320..fb_base + r * 320 + 8]
                    .iter()
                    .all(|&p| p == 0)
            );
        }
        // Rows outside the 152-row area are untouched.
        assert!(dst[fb_base + 152 * 320..].iter().all(|&p| p == 1));
        assert!(dst[..fb_base].iter().all(|&p| p == 1));
    }
}
