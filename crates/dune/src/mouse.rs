//! The in-game main loop and the mouse-pointer plumbing it drives.
//!
//! Ported from `game_loop` (seg000:d815) — `start`'s final `call` — and the
//! mouse routines it calls each pass: `get_mouse_pos_etc` (seg000:df1e),
//! `redraw_mouse` (seg000:dc20) and `get_mouse_cursor_image_addr` (seg000:dc6a).
//! The cursor compositing itself lives in `gfx` (the segvga `vga_draw_cursor` /
//! `vga_restore_cursor` primitives).
//!

use std::sync::{Arc, Mutex};

use crate::{GameState, gfx};

/// Where the cursor sprite gets composited.
///
/// * `Baked` mirrors DOS: `vga_draw_cursor` / `vga_restore_cursor` mutate the
///   game framebuffer on the game thread and the cursor rides along with
///   every presented frame. Sampling lag is one game tick + however long
///   the present pipeline takes.
/// * `Overlay` skips the framebuffer mutation and instead publishes the
///   cursor `(shape, hidden)` state for the present thread, which samples
///   the latest mouse position from `SharedInput` and composites the
///   sprite on the GPU at present time. Latency drops to roughly one vsync.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorMode {
    #[default]
    Baked,
    Overlay,
}

/// The cursor shape/visibility the GPU overlay should draw, published by
/// the game thread and read by the present thread. Position is sampled
/// separately from `SharedInput` so the present path picks up the freshest
/// pointer move every vsync.
#[derive(Clone, Copy, Debug)]
pub struct CursorOverlay {
    pub shape: CursorShapeId,
    pub hidden: bool,
}

impl Default for CursorOverlay {
    fn default() -> Self {
        Self {
            shape: CursorShapeId::Arrow,
            hidden: true,
        }
    }
}

#[derive(Clone, Default)]
pub struct SharedCursor {
    inner: Arc<Mutex<CursorOverlay>>,
}

impl SharedCursor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> CursorOverlay {
        *self.inner.lock().unwrap()
    }

    pub fn publish(&self, overlay: CursorOverlay) {
        *self.inner.lock().unwrap() = overlay;
    }
}

// = a cursor shape as vga_draw_cursor consumes it: a hotspot (subtracted from the
// pointer position) and 16 rows of AND/OR mask. Each pixel is processed MSB-first
// (bit 15 = leftmost column): an AND bit keeps the background (transparent); AND
// clear with OR set draws colour 0x0f; AND clear with OR clear draws black (0).
pub struct CursorShape {
    pub hotspot_x: u16,
    pub hotspot_y: u16,
    pub and_mask: [u16; 16],
    pub or_mask: [u16; 16],
}

// = seg001:cursor_image_ptr targets — the cursor shapes vga_draw_cursor renders.
// get_mouse_cursor_image_addr (seg000:dc6a) picks between the arrow, the busy
// hand (seg001:25c8) and the four map-edge travel arrows (up/right/down/left
// at seg001:260c/2650/2694/26d8) by hover region against the mouse_nav_rect
// hot-zone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorShapeId {
    Arrow,
    Hand,
    Up,
    Right,
    Down,
    Left,
}

// = seg001:cursor_shape_arrow — the default arrow, hotspot (0, 0).
pub const CURSOR_ARROW: CursorShape = CursorShape {
    hotspot_x: 0,
    hotspot_y: 0,
    and_mask: [
        0b0011111111111111,
        0b0001111111111111,
        0b0000111111111111,
        0b0000011111111111,
        0b0000001111111111,
        0b0000000111111111,
        0b0000000011111111,
        0b0000000001111111,
        0b0000000000111111,
        0b0000000000111111,
        0b0000000111111111,
        0b0001000011111111,
        0b0011000011111111,
        0b1111100001111111,
        0b1111100001111111,
        0b1111110001111111,
    ],
    or_mask: [
        0b0000000000000000,
        0b0100000000000000,
        0b0110000000000000,
        0b0111000000000000,
        0b0111100000000000,
        0b0111110000000000,
        0b0111111000000000,
        0b0111111100000000,
        0b0111111110000000,
        0b0111110000000000,
        0b0110110000000000,
        0b0100011000000000,
        0b0000011000000000,
        0b0000001100000000,
        0b0000001100000000,
        0b0000000000000000,
    ],
};

// = seg001:25c8 cursor_shape_hand — busy/working hand, hotspot (1, 0).
pub const CURSOR_HAND: CursorShape = CursorShape {
    hotspot_x: 1,
    hotspot_y: 0,
    and_mask: [
        0b1100111111111111,
        0b1000001111111111,
        0b1000000111111111,
        0b1110000001111111,
        0b1111000000111111,
        0b1100000000000111,
        0b1100000000000011,
        0b1000000000000011,
        0b0000000000000001,
        0b0000000000000001,
        0b1000000000000000,
        0b1100000000000000,
        0b1110000000000000,
        0b1111000000000000,
        0b1111110000000000,
        0b1111111100000000,
    ],
    or_mask: [
        0b0000000000000000,
        0b0011000000000000,
        0b0001110000000000,
        0b0000011000000000,
        0b0000001110000000,
        0b0000110100000000,
        0b0001011011111000,
        0b0001100111011000,
        0b0110110000111100,
        0b0011000010101100,
        0b0000001110111100,
        0b0001111111011110,
        0b0000111111111110,
        0b0000001110111110,
        0b0000000001111110,
        0b0000000001111110,
    ],
};

// = seg001:260c cursor_shape_up — upward travel arrow, hotspot (4, 0).
pub const CURSOR_UP: CursorShape = CursorShape {
    hotspot_x: 4,
    hotspot_y: 0,
    and_mask: [
        0b1111101111111111,
        0b1111000111111111,
        0b1110000011111111,
        0b1100000001111111,
        0b1000000000111111,
        0b0000000000011111,
        0b0000000000011111,
        0b1110000011111111,
        0b1110000011111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
    ],
    or_mask: [
        0b0000000000000000,
        0b0000010000000000,
        0b0000111000000000,
        0b0001111100000000,
        0b0011111110000000,
        0b0111111111000000,
        0b0000111000000000,
        0b0000111000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
    ],
};

// = seg001:2650 cursor_shape_right — rightward travel arrow, hotspot (4, 2).
pub const CURSOR_RIGHT: CursorShape = CursorShape {
    hotspot_x: 4,
    hotspot_y: 2,
    and_mask: [
        0b1100111111111111,
        0b1100011111111111,
        0b1100001111111111,
        0b0000000111111111,
        0b0000000011111111,
        0b0000000001111111,
        0b0000000011111111,
        0b0000000111111111,
        0b1100001111111111,
        0b1100011111111111,
        0b1100111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
    ],
    or_mask: [
        0b0000000000000000,
        0b0001000000000000,
        0b0001100000000000,
        0b0001110000000000,
        0b0111111000000000,
        0b0111111100000000,
        0b0111111000000000,
        0b0001110000000000,
        0b0001100000000000,
        0b0001000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
    ],
};

// = seg001:2694 cursor_shape_down — downward travel arrow, hotspot (4, 0).
pub const CURSOR_DOWN: CursorShape = CursorShape {
    hotspot_x: 4,
    hotspot_y: 0,
    and_mask: [
        0b1110000011111111,
        0b1110000011111111,
        0b0000000000011111,
        0b0000000000011111,
        0b1000000000111111,
        0b1100000001111111,
        0b1110000011111111,
        0b1111000111111111,
        0b1111101111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
    ],
    or_mask: [
        0b0000000000000000,
        0b0000111000000000,
        0b0000111000000000,
        0b0111111111000000,
        0b0011111110000000,
        0b0001111100000000,
        0b0000111000000000,
        0b0000010000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
    ],
};

// = seg001:26d8 cursor_shape_left — leftward travel arrow, hotspot (5, 2).
pub const CURSOR_LEFT: CursorShape = CursorShape {
    hotspot_x: 5,
    hotspot_y: 2,
    and_mask: [
        0b1111100111111111,
        0b1111000111111111,
        0b1110000111111111,
        0b1100000001111111,
        0b1000000001111111,
        0b0000000001111111,
        0b1000000001111111,
        0b1100000001111111,
        0b1110000111111111,
        0b1111000111111111,
        0b1111100111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
        0b1111111111111111,
    ],
    or_mask: [
        0b0000000000000000,
        0b0000010000000000,
        0b0000110000000000,
        0b0001110000000000,
        0b0011111100000000,
        0b0111111100000000,
        0b0011111100000000,
        0b0001110000000000,
        0b0000110000000000,
        0b0000010000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
        0b0000000000000000,
    ],
};

pub fn cursor_shape(id: CursorShapeId) -> &'static CursorShape {
    match id {
        CursorShapeId::Arrow => &CURSOR_ARROW,
        CursorShapeId::Hand => &CURSOR_HAND,
        CursorShapeId::Up => &CURSOR_UP,
        CursorShapeId::Right => &CURSOR_RIGHT,
        CursorShapeId::Down => &CURSOR_DOWN,
        CursorShapeId::Left => &CURSOR_LEFT,
    }
}

impl GameState {
    // = seg000:df1e get_mouse_pos_etc — latch the pointer position for this pass.
    // Minimal port: copy the shared InputState (the host already maps the window
    // cursor into 320x200 game coordinates) into mouse_pos_x/y. DOS instead reads
    // INT 33,3 and shifts by the mickey scalers (_word_21A30..), then runs the
    // joystick path (loc_0dd10) and the per-person idle/click scan (loc_0df56);
    // mouse_stuff (seg000:db4c) button edge-detection is also TODO.
    pub(crate) fn get_mouse_pos_etc(&mut self) {
        // = seg000:df1e call pause_if_p_key_pressed — honour the P-key pause.
        self.pause_if_p_key_pressed();
        let input = self.input.lock().unwrap();
        self.mouse_pos_x = input.mouse_x;
        self.mouse_pos_y = input.mouse_y;
    }

    // = seg000:dc20 redraw_mouse — composite the cursor at its current position,
    // erasing it from the previous one first. Returns whether the screen changed
    // (the port presents only then). The cursor image, position and hide flag are
    // double-buffered exactly as DOS does so an unmoved pointer redraws nothing.
    //
    // In `CursorMode::Overlay` the cursor is composited on the GPU at present
    // time, so the framebuffer is left untouched and this returns `false` —
    // game-tick cursor motion no longer drives a re-present. The shape and
    // hide state are published to `SharedCursor` for the present thread.
    pub(crate) fn redraw_mouse(&mut self) -> bool {
        // = seg000:dc27/dc2b dx = mouse_pos_x (X), bx = mouse_pos_y (Y).
        let x = self.mouse_pos_x;
        let y = self.mouse_pos_y;
        // = seg000:dc2f call get_mouse_cursor_image_addr — the shape for this pass.
        let new_image = self.get_mouse_cursor_image();
        // = seg000:dc34 xchg bp,[cursor_image_ptr] — bp becomes the old shape.
        let old_image = self.cursor_image.replace(new_image);
        // = seg000:dc3a xchg al,[cursor_hide_counter] — read it, then clear to 0.
        let hide = std::mem::take(&mut self.cursor_hide_counter);

        if self.cursor_mode == CursorMode::Overlay {
            // Tell the present thread what shape to draw and whether it's
            // hidden; the freshest position is sampled there from
            // `SharedInput` instead of being routed through the game tick.
            self.shared_cursor.publish(CursorOverlay {
                shape: new_image,
                hidden: hide < 0,
            });
            // Track the "drawn" position as if we had drawn it so any other
            // consumer of mouse_draw_pos_* sees consistent state.
            self.mouse_draw_pos_x = x;
            self.mouse_draw_pos_y = y;
            return false;
        }

        // = seg000:dc3e or al,al; js loc_0dc56 — hidden last pass: skip the
        // restore and just draw.
        if hide >= 0 {
            // = seg000:dc42..dc50 unchanged position and shape -> nothing to do.
            if old_image == Some(new_image)
                && x == self.mouse_draw_pos_x
                && y == self.mouse_draw_pos_y
            {
                return false;
            }
            // = seg000:dc52 call vga_restore_cursor — repaint the old background.
            gfx::vga_restore_cursor(self);
        }
        // = seg000:dc56/dc5a record where the cursor is now drawn.
        self.mouse_draw_pos_x = x;
        self.mouse_draw_pos_y = y;
        // = seg000:dc5e call vga_draw_cursor.
        gfx::vga_draw_cursor(self, new_image, x, y);
        true
    }

    // = seg000:dbb2 call_restore_cursor — hide the cursor one nesting level so a
    // draw that lands under it paints clean background, not the cursor. Balanced
    // by draw_mouse, which re-shows it. Baked erases the cursor from the
    // framebuffer (vga_restore_cursor); Overlay/System publishes the hidden
    // state to the present thread so the click's hide is visible immediately
    // (not only at the next redraw_mouse). Both bracket every screen update that
    // can land under the cursor — including the game loop's per-click hide
    // (seg000:d8f4), which is what makes the cursor blink off on a HUD-arrow or
    // command click. No-op while composing a frame offscreen (front == fb1),
    // where the live cursor must not be touched.
    pub(crate) fn call_restore_cursor(&mut self) {
        if self.front_buffer_is_fb1() {
            return;
        }
        // = seg000:dbb3 al = cursor_hide_counter; dec, committing the decrement
        // only when the result is negative (so a visible 0 goes to -1 but a
        // positive over-shown count is left alone).
        let old = self.cursor_hide_counter;
        if old <= 0 {
            self.cursor_hide_counter = old - 1;
        }
        if self.cursor_mode == CursorMode::Baked {
            // = seg000:dbc0 or al,al; js — restore only when it was visible.
            if old >= 0 {
                gfx::vga_restore_cursor(self);
            }
        } else {
            self.publish_overlay_cursor();
        }
    }

    // = seg000:dbec draw_mouse — show the cursor one nesting level, restoring it
    // once the counter returns to 0. The mirror of call_restore_cursor. Baked
    // composites the cursor into the framebuffer; Overlay/System re-publishes
    // the (now shown) state. No-op while composing offscreen.
    pub(crate) fn draw_mouse(&mut self) {
        if self.front_buffer_is_fb1() {
            return;
        }
        // = seg000:dbec inc cursor_hide_counter.
        self.cursor_hide_counter = self.cursor_hide_counter.wrapping_add(1);
        // = seg000:dbf0 js loc_0dc1a — still negative: nested-hidden, draw nothing.
        if self.cursor_hide_counter < 0 {
            if self.cursor_mode != CursorMode::Baked {
                self.publish_overlay_cursor();
            }
            return;
        }
        // = seg000:dbf2 jnz loc_0dc1b — over-shown: undo the inc and return.
        if self.cursor_hide_counter > 0 {
            self.cursor_hide_counter -= 1;
            return;
        }
        // = seg000:dbf4 counter == 0: the cursor is fully shown again.
        if self.cursor_mode != CursorMode::Baked {
            self.publish_overlay_cursor();
            return;
        }
        // Baked: composite the cursor at mouse_pos with the last-selected shape.
        let x = self.mouse_pos_x;
        let y = self.mouse_pos_y;
        self.mouse_draw_pos_x = x;
        self.mouse_draw_pos_y = y;
        let image = match self.cursor_image {
            Some(image) => image,
            None => self.get_mouse_cursor_image(),
        };
        gfx::vga_draw_cursor(self, image, x, y);
    }

    // Publish the overlay cursor's current shape and hidden state to the
    // present thread. call_restore_cursor / draw_mouse call it so a hide (or
    // re-show) driven by an interaction reaches the compositor at once, without
    // waiting for the next redraw_mouse — the present thread samples the live
    // pointer position itself. Overlay/System only.
    fn publish_overlay_cursor(&mut self) {
        let shape = self
            .cursor_image
            .unwrap_or_else(|| self.get_mouse_cursor_image());
        self.shared_cursor.publish(CursorOverlay {
            shape,
            hidden: self.cursor_hide_counter < 0,
        });
    }

    // = seg000:db4c mouse_stuff — read the live button state and the previously
    // latched state from `data_0dc34`, store the current state back so the next
    // call can compute edges, and return the combined word: bit0 = LMB down,
    // bit1 = RMB down, bit2 = LMB edge, bit3 = RMB edge (an edge is set on either
    // a press or release since the previous call). game_loop reads the returned
    // ax to dispatch idle / press / release / drag for either button.
    //
    // DOS layout:
    //   data_0dc34 (byte): current button state, refreshed by an INT 33 poll
    //   data_0dc35 (byte): previous button state, written here by mouse_stuff
    //
    // Port: live state comes from `InputState::mouse_buttons` (already polled
    // by the host event loop); the previous state lives in `prev_mouse_buttons`.
    // Returns the same ax as DOS so game_loop's dispatch reads it back unchanged.
    pub(crate) fn mouse_stuff(&mut self) -> u16 {
        // = seg000:db4c mov ax, [data_0dc34]. AL = live buttons, AH = previously
        //   latched buttons (set by the previous call's `mov [data_0dc35], al`).
        let live = self.input.lock().unwrap().mouse_buttons;
        let prev = self.prev_mouse_buttons;
        // = seg000:db4f and al,3 — keep only LMB | RMB.
        let cur = live & 3;
        // = seg000:db51 mov [data_0dc35], al — latch current for the next call's
        //   edge computation. The port stores into prev_mouse_buttons, which
        //   any_key_pressed also writes; both update sites store the same
        //   "current buttons masked to LMB|RMB", so they coexist.
        self.prev_mouse_buttons = cur;
        // = seg000:db54..db5a xor ah,al; add ah,ah; add ah,ah; or al,ah — the
        //   changed bits (bit0 LMB, bit1 RMB) shifted left by two and OR'd in, so
        //   the edges land in bits 2..3 above the live state in bits 0..1.
        let edges = cur ^ (prev & 3);
        let ax = (cur as u16) | ((edges as u16) << 2);
        // = seg000:db5e/db62 dx = mouse_pos_x; bx = mouse_pos_y. DOS returns
        //   them in registers; the port keeps them on GameState already.
        ax
    }

    // = seg000:dc6a get_mouse_cursor_image_addr — pick the cursor shape for the
    // pointer's current hover region.
    fn get_mouse_cursor_image(&self) -> CursorShapeId {
        // = seg000:dc6a cmp [settings_drag_target],0; dc6f bp = 25c8h
        // (cursor_shape_hand); dc72 jnz — while a mixer-panel slider or balance
        // knob handle is grabbed (data_028be != 0) the cursor is the busy hand. This
        // is the first, highest-priority check.
        if self.settings_drag_target != 0 {
            return CursorShapeId::Hand;
        }
        // = seg000:dc77 cmp [data_04723],0; jnz — the map-main-menu busy flag
        // also forces the hand; not modelled (nothing ported sets it).
        // = seg000:dc7e di = [mouse_nav_rect_ptr]; or di,di; jz — no
        // navigation hot-zone installed: the plain arrow.
        let Some(rect) = self.mouse_nav_rect else {
            return CursorShapeId::Arrow;
        };
        let x = self.mouse_pos_x as i16;
        let y = self.mouse_pos_y as i16;
        // = seg000:dc86 cmp bx,9bh; jge — below the game area (over the HUD)
        // the hot-zone does not apply.
        if y >= 0x9b {
            return CursorShapeId::Arrow;
        }
        // = seg000:dc8c call rect_contains; dc8f bp = cursor_shape_hand; jb —
        // strictly inside the hot-zone the cursor is the hand.
        if rect.contains_interior(x, y) {
            return CursorShapeId::Hand;
        }
        // = seg000:dc94..dc9c with y inside the zone's vertical span, the
        // pointer sits in a horizontal scroll band.
        if y >= rect.y0 && y < rect.y1 {
            // = seg000:dc9e..dca8 within 0x32 left of the zone: the left arrow.
            if (rect.x0.wrapping_sub(x) as u16) < 0x32 {
                return CursorShapeId::Left;
            }
            // = seg000:dcaa..dcb5 within 0x32 right of it: the right arrow.
            if (x.wrapping_sub(rect.x1) as u16) < 0x32 {
                return CursorShapeId::Right;
            }
        } else if x >= rect.x0 && x < rect.x1 {
            // = seg000:dcb9..dcc0 the vertical scroll bands.
            // = seg000:dcc2..dccd within 0x19 above the zone: the up arrow.
            if (rect.y0.wrapping_sub(y) as u16) < 0x19 {
                return CursorShapeId::Up;
            }
            // = seg000:dccf..dcda within 0x19 below it: the down arrow.
            if (y.wrapping_sub(rect.y1) as u16) < 0x19 {
                return CursorShapeId::Down;
            }
        }
        // = seg000:dcdc bp = cursor_shape_arrow.
        CursorShapeId::Arrow
    }
}
