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

//! The gfx module is the only module that reads or writes `state.y_offset`.
//! Graphics operations that draw into `state.framebuffer` go through helpers
//! here (`hnm_do_frame`, `hnm_load_first_frame`, `draw_sprite_on_framebuffer`),
//! which add the offset to the destination y the same way the DOS segvga
//! primitives auto-apply `fb_base_ofs`. The framebuffer→screen copy
//! (`gfx_copy_whole_framebuf_to_screen`) is a plain memcpy that does NOT
//! apply the offset — matching DOS's `vga_copy_screen_2`.

use std::mem::swap;

use crate::{
    Color, CursorShapeId, FrameBuffer, GameState, Rect, SpriteSheet, cursor_shape,
    draw_sprite_from_sheet, sprite_blitter,
};

// segvga:0b0c
pub fn palette_flush(state: &mut GameState) {
    state.screen_pal = state.palette.clone();
}

// = segvga:25e7 vga_transition — main transition dispatcher.
// Forces `code` even, wraps modulo 0x3e, and dispatches via the per-handler match below.
// Mirrors the `jmp word ptr transition_dispatch_table[bx]` at segvga:2616.
//
// `dl` is the caller's dx low byte; only the scripted-curtain handler (code
// 0x04) reads it, to pick the script-traversal direction.
pub fn vga_transition(state: &mut GameState, code: u16, dl: u8) {
    let mut idx = code & 0xfe;
    while idx >= 0x3e {
        idx -= 0x3e;
    }
    match idx {
        0x04 => transition_vertical_fold(state, dl),
        0x10 => transition_dotted_columns(state),
        0x34 => transition_dotted_columns_tall(state),
        0x30 => transition_instant_swap(state),
        0x36 => transition_fade_in_from_black(state),
        0x3a => transition_fade_through_black(state),
        other => {
            println!("gfx: vga_transition unimpl code 0x{other:02x}");
        }
    }
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

// Which pixel operation the ellipse kernel applies along the band: DOS selects
// it by self-modifying the dispatch pointer at data_segvga_027e4 (0x2823 = draw,
// 0x2887 = erase) before calling transition_kernel.
#[derive(Copy, Clone, PartialEq, Eq)]
enum RippleOp {
    // = loc_segvga_02823: smear the clean (fb1) water pixel into a 4x4 block.
    Draw,
    // = loc_segvga_02887: restore the band's water pixels from the clean buffer.
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

// = segvga:02823 (draw) / segvga:02887 (erase) — the per-point pixel operation.
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

// = segvga:0a68 vga_save_palette_to_fade_target — snapshot current palette.
pub fn vga_save_palette_to_fade_target(state: &mut GameState) {
    state.palette_fade_target = state.palette.clone();
}

// = segvga:0a76 vga_swap_palettes — atomic swap palette ↔ palette_to_transition_from.
pub fn vga_swap_palettes(state: &mut GameState) {
    swap(&mut state.palette, &mut state.palette_fade_target);
}

// = segvga:19f7 vga_clear_screen — clear the active framebuffer to color 0.
pub fn vga_clear_screen(state: &mut GameState) {
    state.active_fb_mut().clear();
}

// = segvga vga_set_fb_row — set `fb_base_ofs` (the per-blit destination y
// offset added by segvga blit primitives). In our model this is
// `state.y_offset`; the gfx-level blit helpers below read it and apply it
// to the destination y of every draw into `state.framebuffer`.
pub fn vga_set_fb_row(state: &mut GameState, row: u16) {
    state.y_offset = row;
}

// = loc_segvga_026e3 inner step. Subtracts `step_size` from every component
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

// = loc_segvga_0264d inner step. Steps every component of every palette
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

// Run the fade-out kernel for `cycles` outer iterations, processing
// `chunks` chunks of `per_chunk` palette entries each, and yielding one
// frame to the driver between chunks (= seg000:0261d / vsync wait).
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

// = loc_segvga_02757 (transition_dispatch_table entry 24) — code 0x30:
// palette flush + copy framebuffer to screen. No fade — an immediate cut.
// One frame-task tick is enough to let the driver emit the new screen.
fn transition_instant_swap(state: &mut GameState) {
    state.gfx_copy_whole_framebuf_to_screen();
    palette_flush(state);
    state.present_transition_frame();
}

// = loc_segvga_02628 (transition_dispatch_table entry 27) — code 0x36:
// fade in from black. Saves the new palette as the fade target, blacks
// out the live palette, flips the offscreen framebuffer onto the screen
// (safe while the palette is all-zero — nothing is visible), then steps
// the palette back up to the saved target. Parameters from segvga:264a:
// cx=0x60 (batch=96 bytes = 32 entries × 8 chunks), dx=320 (step=1
// cycles=64).
fn transition_fade_in_from_black(state: &mut GameState) {
    const FADE_36_CYCLES: u8 = 64;
    const FADE_36_CHUNKS: u8 = 8;
    const FADE_36_PER_CHUNK: usize = 32;
    const FADE_36_STEP: u8 = 1;

    state.palette_fade_target = state.palette.clone();
    for i in 0..256 {
        state.palette.set(i, Color(0, 0, 0));
        state.screen_pal.set(i, Color(0, 0, 0));
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

// = loc_segvga_0272e (transition_dispatch_table entry 29) — code 0x3a:
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
fn transition_fade_through_black(state: &mut GameState) {
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

// = segvga:2604 `mov cx, 98h` — the default handler arg (the 152-row game
// area). transition_dotted_columns_tall overrides it with 0xc8 (the full
// 200-row screen) at segvga:2dc0. Both are halved twice at segvga:2dc9 to give
// the dot-row group count, so the lattice touches `cx` rows (every 4th row, in
// 4-row groups).
const DOTTED_ROWS: usize = 0x98;
const DOTTED_ROWS_TALL: usize = 0xc8;
// = segvga:2ddf `mov dx, 50h` — 80 dots written across each row.
const DOTTED_COLS: usize = 0x50;
// = segvga:2de3 `add di, 3` after the stosb — stride 4, one dot per 4-wide
// block column (80 dots × 4 = a full 320-pixel row).
const DOTTED_COL_STRIDE: usize = 4;
// = segvga:2dea `add di, 500h` — advance 4 screen rows (4 × 320) between
// dot rows, so each table entry touches only every 4th row.
const DOTTED_ROW_STRIDE: usize = 0x500;

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
    // = segvga:2604 cx = 0x98 — the 152-row game area.
    dotted_columns_reveal(state, DOTTED_ROWS >> 2);
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

const DUMP_FRAMES: bool = false;

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
// reading as the room "folding" away to reveal the mirror. `dl` selects
// direction: dl >= 0 takes the reverse path; the forward path (dl < 0) is
// unimplemented, as every current caller sets dx = 0.
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

    if (dl as i8) < 0 {
        todo!();
    };

    // First half (cl 0x11..0x0a): squish the OLD screen (snapshotted into fb2)
    // toward the centre line; clear_at_end runs the cl == 9 band-clear.
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

    // Second half (cl 8..1): expand the NEW image (fb1) back out from the centre.
    transition_vertical_fold_part(
        state,
        &FOLD_LINES_REV,
        &mut step,
        FoldSource::NewImage,
        false,
    );

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

fn transition_vertical_fold_part(
    state: &mut GameState,
    lines: &[(u16, u16); 8],
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

// = segvga:30f2 panel-fold parameter table (al = rows copied, ah = rows skipped),
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

// = blit_fb1_to_screen_effect(al=0x18) -> panel_anim_play_step: render ONE
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
        // = play_step `cmp cl,9; jb`: the closing half (frame > 9) squishes the
        //   old panel snapshot (fb2); the opening half reads fb1.
        if frame > 9 {
            panel_fold_squish(&mut state.screen, &state.framebuffer_saved, al, ah);
        } else {
            panel_fold_squish(&mut state.screen, &state.framebuffer, al, ah);
        }
    }
}

// = seg000:c4cd gfx_copy_whole_framebuf_to_screen. Plain memcpy from fb1
// to the front buffer (`screen_buffer`) — does NOT apply `fb_base_ofs`
// (matching the DOS `vga_copy_screen_2` behaviour). The y-offset is applied
// to incoming draws, not to this outgoing copy.
//
// When `screen_buffer` is redirected to fb1 (inside
// gfx_call_bp_with_front_buffer_as_screen during a stage init), the copy is
// fb1 → fb1, i.e. a no-op — the visible screen is left untouched until the
// transition reveals fb1.
pub fn gfx_copy_whole_framebuf_to_screen(state: &mut GameState) {
    // Front buffer redirected to fb1: the copy would be fb1 → fb1.
    if state.front_buffer_is_fb1() {
        return;
    }
    state.screen.copy_from(&state.framebuffer);
}

// Draw a sprite into the active framebuffer at logical (x, y). The
// destination y is shifted by `state.y_offset` to mirror how DOS segvga
// blits auto-apply `fb_base_ofs`. Used by the intro stage 11 icon list.
pub fn draw_sprite_on_framebuffer(
    state: &mut GameState,
    sheet: &SpriteSheet,
    sprite_id: u16,
    x: i16,
    y: i16,
) -> std::io::Result<()> {
    let physical_y = y + state.y_offset as i16;
    draw_sprite_from_sheet(sheet, sprite_id, x, physical_y, state.active_fb_mut())
}

// As draw_sprite_on_framebuffer, but honouring the sprite's mirror flags. The
// icon-list entries carry these in the high bits of the sprite word (DOS
// `and ch,60h; or ah,ch` in draw_sprite_clobbering_bx_dx): 0x4000 = flip-x,
// 0x2000 = flip-y.
pub fn draw_sprite_on_framebuffer_flipped(
    state: &mut GameState,
    sheet: &SpriteSheet,
    sprite_id: u16,
    x: i16,
    y: i16,
    flip_x: bool,
    flip_y: bool,
) -> std::io::Result<()> {
    let physical_y = y + state.y_offset as i16;
    let Some(sprite) = sheet.get_sprite(sprite_id) else {
        return Ok(());
    };
    sprite_blitter(sprite, state.active_fb_mut())
        .at(x, physical_y)
        .flip_x(flip_x)
        .flip_y(flip_y)
        .draw()
}

pub fn vga_clear_rect(state: &mut GameState, x0: u16, y0: u16, x1: u16, y1: u16) {
    vga_fill_rect(state, x0, y0, x1, y1, 0);
}

// = segvga vga_copy_rect_ds (gfx_vtable_vga_copy_rect_ds; the inner blit
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

// = segvga:1c46 vga_grab_rect (gfx_vtable_vga_grab_rect). Copy the half-open
// rect `[x0,x1) × [y0,y1)` out of `src` into a freshly packed row-major buffer
// (destination stride = rect width, source stride = src width). DOS reads the
// framebuffer at stride 0x140 and writes the buffer tightly; the buffer is sized
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
    // = loc_03452: scroll the top `bx` rows up by six (row j <- row j+6). Top-
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
    // = loc_03467: fill the six rows below the scrolled region from `src` at
    //   the same offset (ds = fb1, si = di).
    let dpix = dst.pixels_mut();
    let spix = src.pixels();
    for k in 0..6 {
        let d = (y0 + bx + k) * stride;
        dpix[d + x0..d + x1].copy_from_slice(&spix[d + x0..d + x1]);
    }
}

// = segvga vga_fill_rect (gfx_vtable_vga_fill_rect, seg001:38dd). Fill the
// half-open rect [x0,x1) × [y0,y1) of the active framebuffer with `color`,
// applying fb_base_ofs (y_offset) like every other segvga primitive.
pub fn vga_fill_rect(state: &mut GameState, x0: u16, y0: u16, x1: u16, y1: u16, color: u8) {
    let yoff = state.y_offset;
    let fb = state.active_fb_mut();
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
