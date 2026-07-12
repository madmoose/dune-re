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
//!   - `loc_0c8c1` (seg000:c8c1): one zoom step. The source rectangle is
//!     centred on a per-scene focal point — top-left = focal − half-extent,
//!     clamped ≥ 0 — then the step holds for `data_0dbe6` (= 6) timer ticks.
//!
//!   - `loc_0c868` (seg000:c868): the sequencer. The "scene id" is the talking
//!     head id (`[22a6h]` = `_word_21756_talking_head_id`); it indexes the
//!     focal-point table (seg001:27b6). A (0,0) focal point, an id ≥ 0x11, or a
//!     voice already playing skips the zoom. The step sequence is a list of
//!     signed bytes: a positive value is a scale step, −1 is a long pause on
//!     the current (close-up) frame, 0 terminates. With `[227dh] != 0` (its
//!     intro value, 1) the full sequence seg001:2792 is used; otherwise one of
//!     two shorter sequences is chosen at random. After the sequence the scene
//!     is redrawn 1:1 (loc_0c4dd).

use crate::GameState;

// = seg001:27b6 per-scene zoom focal points (col, row), indexed by the talking
// head id. These line up with the 17 talking-head characters (LETO=0, JESS=1,
// …, CHAN=7, …). A (0, 0) entry means "no zoom for this character".
#[rustfmt::skip]
const ZOOM_FOCAL_POINTS: [(i16, i16); 17] = [
    (0x4c, 0x2f), (0x4b, 0x49), (0x00, 0x00), (0x53, 0x25),
    (0x4c, 0x3e), (0x53, 0x3e), (0x4d, 0x4e), (0x58, 0x3f), // [7] = Chani
    (0x47, 0x41), (0x56, 0x1b), (0x69, 0x5b), (0x00, 0x00),
    (0x4a, 0x29), (0x00, 0x00), (0x5e, 0x57), (0x00, 0x00),
    (0x00, 0x00),
];

// = seg001:279a per-scale source-rect half-extents (col, row), indexed by the
// scale selector 1..7. The source rect is centred on the focal point, so its
// top-left corner is `focal − half_extent`. Each pair is (src_w/2, src_h/2) for
// that scale's kernel. Index 0 is unused (0 terminates a sequence).
#[rustfmt::skip]
const ZOOM_HALF_EXTENTS: [(i16, i16); 8] = [
    (  0,  0), // [0] unused
    (140, 66), // [1] 8/7
    (120, 57), // [2] 4/3
    (106, 50), // [3] 3/2
    ( 80, 38), // [4] 2×
    ( 53, 25), // [5] 3×
    ( 40, 19), // [6] 4×
    ( 20,  9), // [7] 8×
];

// = the zoom step sequences. Positive = scale step; -1 = a long pause on the
// current frame; the trailing 0 terminator is dropped here (the loop ends at
// slice end). The intro uses ZOOM_SEQ_FULL because [227dh] is 1.
const ZOOM_SEQ_FULL: [i8; 7] = [6, -1, 5, 4, 3, 2, 1]; // = seg001:2792
const ZOOM_SEQ_RAND_A: [i8; 4] = [5, -1, 4, 3]; // = seg001:2789
const ZOOM_SEQ_RAND_B: [i8; 3] = [4, -1, 3]; // = seg001:278e

// = data_0dbe6 (set to 6 at seg000:0790): the minimum number of timer ticks
// each zoom step is held (the loc_0c8ed frame-rate gate). game_ticks() is the
// port's PIT counter equivalent.
const ZOOM_STEP_TICKS: u64 = 6;

// = wait_a_bit(0x12c) at seg000:c8aa — the pause held on a -1 sequence entry.
const ZOOM_PAUSE_TICKS: u64 = 300;

// = the output rectangle every kernel produces: 320×152, written from
// fb_base_ofs (the game-area top).
const ZOOM_OUT_W: usize = 320;
const ZOOM_OUT_H: usize = 152;

// = (numerator, denominator) of the zoom factor for scale selectors 1..7. The
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

    // = calc_fb_offset: clamp the base row to 199, then index the source.
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

impl GameState {
    // = seg000:c8c1 loc_0c8c1 — render one zoom step. Centre the `scale`-sized
    // source rect on `focal` (top-left = focal − half_extent, clamped ≥ 0),
    // blit it to the screen, then hold for ZOOM_STEP_TICKS.
    fn zoom_reveal_step(&mut self, focal: (i16, i16), scale: u8) {
        let (hx, hy) = ZOOM_HALF_EXTENTS[scale as usize];
        // = sub dx,[si+2796h] / sub bx,[si+2798h], each clamped ≥ 0.
        let col = (focal.0 - hx).max(0);
        let row = (focal.1 - hy).max(0);

        vga_zoom_screen(self, col, row, scale);
        self.send_frame_to_display();

        // = loc_0c8ed: spin until at least data_0dbe6 (6) ticks have elapsed.
        let start = self.game_ticks();
        self.sleep_ticks(start, ZOOM_STEP_TICKS);
    }

    // = seg000:c868 loc_0c868 — the cinematic zoom-in reveal of the current
    // talking-head scene. Runs synchronously (no frame tasks) before the head
    // starts talking; the static composited frame in fb1 is the source.
    pub fn scene_zoom_in_reveal(&mut self) {
        // = call is_voc_pcm_playing; jnz ret — don't zoom over a playing voice.
        let Some(scene) = self
            .talking_head
            .as_ref()
            .filter(|h| !h.speaking)
            .map(|h| h.talking_head_id as usize)
        else {
            return;
        };

        // = mov si,[22a6h]; cmp si,11h; jnb ret — scene id = talking head id.
        if scene >= 0x11 {
            return;
        }

        // = mov dx,[si+27b6h]; mov bx,[si+27b8h]; or ax; jz ret — (0,0) = none.
        let focal = ZOOM_FOCAL_POINTS[scene];
        if focal == (0, 0) {
            return;
        }

        // = select the step sequence. [227dh] is 1 in the intro, so the full
        // pull-back sequence is used; the [227dh]==0 branch (a random short
        // sequence) is preserved for the in-game callers.
        let seq: &[i8] = if zoom_uses_full_sequence() {
            &ZOOM_SEQ_FULL
        } else if self.rand_masked(1) == 0 {
            &ZOOM_SEQ_RAND_A
        } else {
            &ZOOM_SEQ_RAND_B
        };

        // = loc_0c8a3: lodsb; or al,al; jz end; jns step; (negative) pause.
        for &step in seq {
            if step == 0 {
                break;
            } else if step < 0 {
                // = mov ax,12ch; call wait_a_bit — hold the close-up.
                let start = self.game_ticks();
                self.send_frame_to_display();
                self.sleep_ticks(start, ZOOM_PAUSE_TICKS);
            } else {
                self.zoom_reveal_step(focal, step as u8);
            }
        }

        // = loc_0c8bd: call loc_0c4dd — final 1:1 reveal of the whole scene.
        self.gfx_copy_whole_framebuf_to_screen();
        self.send_frame_to_display();
    }
}

// = cmp byte ptr [227dh], 0 — [227dh] (seg001:227d) is statically 1 and never
// written, so the full sequence is always selected. Factored out so the
// random-sequence branch above stays exercised/visible.
fn zoom_uses_full_sequence() -> bool {
    true
}
