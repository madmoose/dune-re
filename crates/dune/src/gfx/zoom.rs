//! Cinematic zoom-in reveal — the camera push used when a talking-head scene
//! appears (intro stage 21 "Chani in sietch", and the in-game dialogue path at
//! seg000:1b09). Faithful port of:
//!
//!   - `vga_zoom_screen` (segvga:3a14) + `calc_fb_offset` (segvga:0c10): the
//!     scaled blit primitive. It reads a sub-rectangle of the offscreen
//!     framebuffer (fb1) and writes a 320×152 nearest-neighbour upscale of it
//!     to the screen, starting at `fb_base_ofs`. The DOS code dispatches one of
//!     seven hand-unrolled blit kernels by a scale selector (1..7); each kernel
//!     is pure pixel replication, so it is exactly `dst[i] = src[i*den/num]` at
//!     the kernel's ratio. The selectors and ratios are:
//!     1 = 8/7 (zoom_kernel_8_7, segvga:3a25); 2 = 4/3 (zoom_kernel_4_3,
//!     segvga:3a69); 3 = 3/2 (zoom_kernel_3_2, segvga:3a9d); 4 = 2×
//!     (zoom_kernel_2x, segvga:3ad9); 5 = 3× (zoom_kernel_3x, segvga:3af6);
//!     6 = 4× (zoom_kernel_4x, segvga:3b46); 7 = 8× (zoom_kernel_8x,
//!     segvga:3b6d).
//!

use crate::GameState;

// = the output rectangle every kernel produces: 320×152, written from
// fb_base_ofs (the game-area top).
const ZOOM_OUT_W: usize = 320;
const ZOOM_OUT_H: usize = 152;

// (numerator, denominator) of the zoom factor for scale selectors 1..7. The
// source offset for output pixel d is `d * den / num`, i.e. nearest-neighbour
// down-sampling of the dest coordinate — exactly what each unrolled kernel does
// by pixel replication.
fn zoom_ratio(scale: u8) -> (usize, usize) {
    match scale {
        1 => (8, 7), // = zoom_kernel_8_7
        2 => (4, 3), // = zoom_kernel_4_3
        3 => (3, 2), // = zoom_kernel_3_2
        4 => (2, 1), // = zoom_kernel_2x
        5 => (3, 1), // = zoom_kernel_3x
        6 => (4, 1), // = zoom_kernel_4x
        7 => (8, 1), // = zoom_kernel_8x
        _ => (1, 1),
    }
}

// = segvga:3a14 vga_zoom_screen (+ segvga:0c10 calc_fb_offset). The scaled
// blit kernel itself, on raw pixel slices: read a `(col, row)`-anchored
// sub-rectangle of `src` (scaled up by `scale`) and write a 320×152 upscale to
// `dst`, both anchored at `fb_base_ofs` (= `y_offset` rows down). DOS picks the
// source (ds) and dest (es) framebuffers; the wrappers below bind them to the
// screen (the cinematic reveal) or to fb2 (the dialogue backdrop zoom).
fn zoom_blit(src: &[u8], dst: &mut [u8], y_offset: usize, col: i16, row: i16, scale: u8) {
    let (num, den) = zoom_ratio(scale);

    // = cs:[fb_base_ofs] — the game-area top, applied to both source and dest.
    let fb_base = y_offset * ZOOM_OUT_W;

    // = segvga:0c10 calc_fb_offset: clamp the base row to 199, then index the source.
    let row = row.clamp(0, 199) as usize;
    let col = col.max(0) as usize;
    let base_src = fb_base + row * ZOOM_OUT_W + col;

    for r in 0..ZOOM_OUT_H {
        let src_row = base_src + (r * den / num) * ZOOM_OUT_W;
        let dst_row = fb_base + r * ZOOM_OUT_W;
        for c in 0..ZOOM_OUT_W {
            let s = src_row + (c * den / num);
            let d = dst_row + c;
            if s < src.len() && d < dst.len() {
                dst[d] = src[s];
            }
        }
    }
}

// = segvga:3a14 vga_zoom_screen with es = screen, ds = fb1. The cinematic
// reveal (loc_0c868): zoom fb1's game area up onto the visible screen.
pub(crate) fn vga_zoom_screen(state: &mut GameState, col: i16, row: i16, scale: u8) {
    let y_offset = state.y_offset as usize;
    // ds:si = fb1, es:di = screen — disjoint fields, borrowed independently.
    let src = state.framebuffer.pixels();
    let dst = state.screen.pixels_mut();
    zoom_blit(src, dst, y_offset, col, row, scale);
}

// = segvga:3a14 vga_zoom_screen with es = fb2, ds = fb1 (the
// zoom_room_to_dialogue_speaker caller, seg000:3b43..3b4f): zoom fb1's game area
// up into fb2 (the saved framebuffer), which zoom_room_to_dialogue_speaker then
// copies back to fb1 as the dialogue backdrop.
pub(crate) fn vga_zoom_fb1_to_fb2(state: &mut GameState, col: i16, row: i16, scale: u8) {
    let y_offset = state.y_offset as usize;
    // ds:si = fb1 (framebuffer), es:di = fb2 (framebuffer_saved) — disjoint.
    let src = state.framebuffer.pixels();
    let dst = state.framebuffer_saved.pixels_mut();
    zoom_blit(src, dst, y_offset, col, row, scale);
}
