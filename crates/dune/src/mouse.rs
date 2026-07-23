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

// = seg000:e65c/e65f — the startup pointer position. initialize_system warps
// the mouse to (237, 171) via warp_mouse_cursor (seg000:e662 -> seg000:db03),
// which stores mouse_pos_x/y and pushes the position into the INT 33 driver
// (set_mouse_pos, seg000:dae3). The port seeds InputState and GameState with
// the same position and warps the OS pointer to match at window creation.
pub const MOUSE_START_X: u16 = 237;
pub const MOUSE_START_Y: u16 = 171;

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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::{DatFile, InputState, recorder::Recorder};

    // The game-thread cursor state must self-heal across a character-click
    // conversation (the seg000:d8f4 click hide followed by common_dialogue's
    // unbalanced hides): the pass after the click redraws and presents.
    // Asset-gated: cargo test -p dune --bin dune -- --ignored cursor_
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn cursor_reappears_after_character_click_conversation() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let input = InputState::shared();
        let shared = SharedCursor::new();
        let mut game = crate::GameState::new_with_input_and_cursor(
            dat_file,
            tx,
            input.clone(),
            CursorMode::Baked,
            shared.clone(),
            std::sync::Arc::new(Recorder::new()),
        );
        game.set_headless();
        game.start(true);

        // Pointer in the game area, over a character-ish spot.
        input.lock().unwrap().on_mouse_move(160, 80);

        // One game_loop cursor pass: cursor baked into the screen.
        game.get_mouse_pos_etc();
        let drew = game.redraw_mouse();
        eprintln!(
            "pass1: drew={drew} counter={} save_h={}",
            game.cursor_hide_counter, game.cursor_save_h
        );

        // The click: seg000:d8f4 call_restore_cursor, then the person handler.
        game.call_restore_cursor();
        eprintln!("after click hide: counter={}", game.cursor_hide_counter);
        game.common_dialogue(0);
        eprintln!(
            "after common_dialogue: counter={} save_h={} draw_pos=({}, {})",
            game.cursor_hide_counter,
            game.cursor_save_h,
            game.mouse_draw_pos_x,
            game.mouse_draw_pos_y
        );

        // Next game_loop pass without any pointer motion.
        game.get_mouse_pos_etc();
        let drew = game.redraw_mouse();
        eprintln!(
            "pass2: drew={drew} counter={} save_h={}",
            game.cursor_hide_counter, game.cursor_save_h
        );
        assert!(
            drew,
            "redraw_mouse must redraw + present the cursor on the pass after the click"
        );
    }

    // Clicking a dialogue-panel line must never publish a frame with the
    // talking head missing or half-repainted. Guards two fixes:
    // subtitle_restore_prior re-renders the head over the restored backdrop
    // BEFORE presenting (= seg000:8cb8..8cc6, not deferred to the next idle
    // tick), and setup_talking_head's same-head early-out (= seg000:91bb)
    // keeps fb2 the clean head-less backdrop instead of re-saving a baked
    // head copy every line. Strip-mode subtitles restore the whole game area,
    // which is what exposed the blink.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn head_does_not_blink_on_line_click() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(8192);
        let input = InputState::shared();
        let shared = SharedCursor::new();
        let mut game = crate::GameState::new_with_input_and_cursor(
            dat_file,
            tx,
            input.clone(),
            CursorMode::Overlay,
            shared.clone(),
            std::sync::Arc::new(Recorder::new()),
        );
        game.cmd_args_memory |= 0x10;
        game.start(true);
        input.lock().unwrap().on_mouse_move(160, 165); // over the verb panel

        // Text-mode strip subtitles (voice_subtitle_mode 0): its
        // subtitle_restore_prior path restores the WHOLE game area — head
        // included — from fb2, which is what exposed the blink.
        game.voice_subtitle_mode = 0;

        game.call_restore_cursor();
        game.common_dialogue(0);

        // The true head-less backdrop: fb2 as saved at conversation start.
        let clean = game.framebuffer_saved.clone();

        let one_pass = |game: &mut crate::GameState| {
            game.ui_hud_companion_blink_task();
            game.process_frame_tasks();
            game.get_mouse_pos_etc();
            let _ = game.redraw_mouse();
            let handlers = game.active_mouse_handlers;
            let _ = game.highlight_hovered_text_action_item();
            (handlers.idle)(game);
            let start = game.game_ticks();
            game.sleep_ticks(start, 1);
        };
        for _ in 0..10 {
            one_pass(&mut game);
        }
        while rx.try_recv().is_ok() {}

        // Head-presence probe: pixels in the head rect differing from the
        // conversation-start backdrop — which never contains the head.
        let (hx0, hy0, hx1, hy1) = game.talking_head.as_ref().unwrap().rect;
        let yoff = game.y_offset;
        let head_present = move |fb: &crate::FrameBuffer, backdrop: &crate::FrameBuffer| {
            let mut diff = 0u32;
            for y in hy0 as u16..hy1 as u16 {
                for x in hx0 as u16..hx1 as u16 {
                    if fb.get(x, y + yoff) != backdrop.get(x, y + yoff) {
                        diff += 1;
                    }
                }
            }
            diff
        };

        // The line click, through the real game-loop button branch: pointer
        // over verb-panel slot 0 ("TALK TO ME", element rect (92,159)-(228,167)),
        // LMB press edge (= seg000:d8f4 call_restore_cursor + the d8fe press
        // dispatch), then the release edge next pass.
        input.lock().unwrap().on_mouse_move(120, 162);
        one_pass(&mut game);

        input.lock().unwrap().on_mouse_button(1);
        game.get_mouse_pos_etc();
        let ax = game.mouse_stuff();
        let _ = game.redraw_mouse();
        assert_eq!(ax & 0x0f, 5, "press edge expected");
        game.call_restore_cursor();
        game.game_loop_dispatch_lmb_press();

        input.lock().unwrap().on_mouse_button(0);
        game.get_mouse_pos_etc();
        let ax = game.mouse_stuff();
        let _ = game.redraw_mouse();
        assert_eq!(ax & 0x0f, 4, "release edge expected");
        game.call_restore_cursor();
        let handlers = game.active_mouse_handlers;
        if let Some(armed) = game.drag_armed_element.take() {
            game.dispatch_element_with_latch(armed);
        } else {
            (handlers.release)(&mut game);
        }
        // Run past the next idle-task tick (16-tick interval), where the
        // pre-fix deferred repaint used to bring the head back.
        for _ in 0..60 {
            one_pass(&mut game);
        }

        let mut states = Vec::new();
        while let Ok((fb, _pal)) = rx.try_recv() {
            states.push(head_present(&fb, &clean));
        }
        eprintln!("head-area diff-vs-clean per published frame: {states:?}");
        // The head was already up before the click (the warm-up drain consumed
        // its build-up frames), so every frame published by the click must
        // still carry it. The head differs from the head-less backdrop by
        // thousands of pixels; a small diff means it is gone or half-repainted.
        let missing = states.iter().filter(|&&d| d <= 5000).count();
        assert!(states.len() > 1, "the click must publish frames");
        assert!(
            missing == 0,
            "the head blinked out: {missing} head-less frame(s) published during the line click"
        );
    }

    // STOP TALKING in a companion conversation (travelling speaker with no
    // room anchor — the non-zoom cleanup branch, seg000:9825/982b) leaves the
    // talking head on screen with its idle task running, matching the
    // original: verified against DOS, a companion-bar dialogue ends with the
    // head lingering until some later action redraws the room. Only the
    // night attack tears it down there (the pushed loc_09840 continuation).
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn stop_talking_leaves_companion_talking_head_like_dos() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(8192);
        let input = InputState::shared();
        let shared = SharedCursor::new();
        let mut game = crate::GameState::new_with_input_and_cursor(
            dat_file,
            tx,
            input.clone(),
            CursorMode::Overlay,
            shared.clone(),
            std::sync::Arc::new(Recorder::new()),
        );
        game.cmd_args_memory |= 0x10;
        game.start(true);

        // Companion-style speaker: travelling (flags 0x40) and not standing in
        // the room (no zoom anchor), so the dialogue opens un-zoomed and the
        // cleanup takes the non-zoom travelling branch.
        game.room_persons[0].flags |= 0x40;
        game.character_screen_pos[0] = (0xffff, 0xffff);

        game.call_restore_cursor();
        game.common_dialogue(0);
        assert!(game.talking_head.is_some(), "the conversation shows a head");
        let clean = game.framebuffer_saved.clone();
        let (hx0, hy0, hx1, hy1) = game.talking_head.as_ref().unwrap().rect;
        let yoff = game.y_offset;

        let one_pass = |game: &mut crate::GameState| {
            game.ui_hud_companion_blink_task();
            game.process_frame_tasks();
            game.get_mouse_pos_etc();
            let _ = game.redraw_mouse();
            let handlers = game.active_mouse_handlers;
            let _ = game.highlight_hovered_text_action_item();
            (handlers.idle)(game);
            let start = game.game_ticks();
            game.sleep_ticks(start, 1);
        };
        for _ in 0..10 {
            one_pass(&mut game);
        }
        while rx.try_recv().is_ok() {}

        // Click STOP TALKING (verb-panel slot 3, y 183..191) through the real
        // game-loop button branch.
        input.lock().unwrap().on_mouse_move(120, 186);
        one_pass(&mut game);
        input.lock().unwrap().on_mouse_button(1);
        game.get_mouse_pos_etc();
        let ax = game.mouse_stuff();
        let _ = game.redraw_mouse();
        assert_eq!(ax & 0x0f, 5, "press edge expected");
        game.call_restore_cursor();
        game.game_loop_dispatch_lmb_press();
        input.lock().unwrap().on_mouse_button(0);
        game.get_mouse_pos_etc();
        let _ = game.mouse_stuff();
        let _ = game.redraw_mouse();
        game.call_restore_cursor();
        let handlers = game.active_mouse_handlers;
        (handlers.release)(&mut game);

        // Run well past the next idle-task tick: nothing may erase the head.
        for _ in 0..60 {
            one_pass(&mut game);
        }

        // Port note: the flattened element stack rebuilds the room records on
        // pop (build_persons_in_room_records, whose seg000:3090 head runs
        // reset_scene_lip_sync_state), so the TalkingHead struct is dropped
        // and the idle animation freezes — DOS keeps its records on the stack
        // and leaves the idle task running. The DOS-visible part is the
        // pixels: the head image must stay on screen until a later redraw.
        let mut last = None;
        while let Ok((fb, _pal)) = rx.try_recv() {
            last = Some(fb);
        }
        let last = last.expect("STOP TALKING publishes frames");
        let mut diff = 0u32;
        for y in hy0 as u16..hy1 as u16 {
            for x in hx0 as u16..hx1 as u16 {
                if last.get(x, y + yoff) != clean.get(x, y + yoff) {
                    diff += 1;
                }
            }
        }
        assert!(
            diff > 5000,
            "the head must still be on screen after STOP TALKING, only {diff} pixels differ"
        );
    }

    // In Overlay mode the published cursor state must settle shown and stay
    // shown across conversation passes (head idle + lip-sync tasks running) —
    // redraw_mouse publishes shown every pass, mirroring DOS's unconditional
    // draw at seg000:dc5e.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn overlay_cursor_state_during_conversation() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let input = InputState::shared();
        let shared = SharedCursor::new();
        let mut game = crate::GameState::new_with_input_and_cursor(
            dat_file,
            tx,
            input.clone(),
            CursorMode::Overlay,
            shared.clone(),
            std::sync::Arc::new(Recorder::new()),
        );
        game.set_headless();
        game.start(true);
        input.lock().unwrap().on_mouse_move(160, 80);

        // Click + conversation start.
        game.call_restore_cursor();
        game.common_dialogue(0);

        // Emulate full game_loop passes (idle path, no clicks): frame tasks
        // (head idle + voice lip sync), then the mouse pass, then the active
        // screen's idle handler. Track published hidden-state transitions.
        let mut last = shared.snapshot().hidden;
        let mut transitions = Vec::new();
        for pass in 0..400 {
            game.ui_hud_companion_blink_task();
            game.process_frame_tasks();
            game.get_mouse_pos_etc();
            let _ = game.redraw_mouse();
            let h1 = shared.snapshot().hidden;
            let handlers = game.active_mouse_handlers;
            let _ = game.highlight_hovered_text_action_item();
            (handlers.idle)(&mut game);
            let h2 = shared.snapshot().hidden;
            for h in [h1, h2] {
                if h != last {
                    transitions.push((pass, h));
                    last = h;
                }
            }
            let start = game.game_ticks();
            game.sleep_ticks(start, 1);
        }
        eprintln!("transitions (pass, hidden): {transitions:?}");
        eprintln!("final hidden: {}", shared.snapshot().hidden);
        assert!(
            !shared.snapshot().hidden,
            "published cursor state must settle shown during the conversation"
        );
    }

    // In Baked mode the cursor pixels must be present in the screen buffer at
    // the end of every conversation pass, even with the pointer overlapping
    // the talking head (restore_mouse_if_rect_intersects + the bracket-close
    // publish keep the erase/redraw balanced) — and every frame PUBLISHED
    // during the steady conversation must carry the cursor too
    // (send_frame_to_display skips publishing while a rect bracket has the
    // cursor lifted).
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn baked_cursor_survives_conversation_passes() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(8192);
        let input = InputState::shared();
        let shared = SharedCursor::new();
        let mut game = crate::GameState::new_with_input_and_cursor(
            dat_file,
            tx,
            input.clone(),
            CursorMode::Baked,
            shared.clone(),
            std::sync::Arc::new(Recorder::new()),
        );
        // Not headless: the frame sink must receive the real publish stream.
        // Music off, like set_headless (the MUSIC OFF cmd_args_memory bit).
        game.cmd_args_memory |= 0x10;
        game.start(true);
        input.lock().unwrap().on_mouse_move(160, 80);

        game.call_restore_cursor();
        game.common_dialogue(0);

        // Park the pointer in the middle of the talking head so its idle and
        // lip-sync dirty rects overlap the cursor and the rect bracket
        // (restore_mouse_if_rect_intersects) actually fires.
        let (hx0, hy0, hx1, hy1) = game.talking_head.as_ref().unwrap().rect;
        let (cx, cy) = (((hx0 + hx1) / 2) as u16, ((hy0 + hy1) / 2) as u16);
        input.lock().unwrap().on_mouse_move(cx, cy);

        // The arrow at (cx,cy): or_mask row 1 bit 14 paints (cx+1,cy+1) = 0x0f.
        let cursor_pixel =
            move |fb: &crate::FrameBuffer, yoff: u16| fb.get(cx + 1, cy + 1 + yoff) == 0x0f;
        let yoff = game.y_offset;

        let one_pass = |game: &mut crate::GameState| {
            game.ui_hud_companion_blink_task();
            game.process_frame_tasks();
            game.get_mouse_pos_etc();
            let _ = game.redraw_mouse();
            let handlers = game.active_mouse_handlers;
            let _ = game.highlight_hovered_text_action_item();
            (handlers.idle)(game);
            let start = game.game_ticks();
            game.sleep_ticks(start, 1);
        };

        // Two warm-up passes let redraw_mouse heal the click hide; frames
        // published up to here (conversation setup, fold) may legitimately
        // lack the cursor.
        one_pass(&mut game);
        one_pass(&mut game);
        while rx.try_recv().is_ok() {}

        let mut published = 0;
        let mut published_missing = 0;
        let mut screen_missing = 0;
        for _pass in 0..400 {
            one_pass(&mut game);
            if !cursor_pixel(&game.screen, game.y_offset) {
                screen_missing += 1;
            }
            while let Ok((fb, _pal)) = rx.try_recv() {
                published += 1;
                if !cursor_pixel(&fb, yoff) {
                    published_missing += 1;
                }
            }
        }
        eprintln!("published={published} missing_in_published={published_missing}");
        assert_eq!(
            screen_missing, 0,
            "the baked cursor must be in the screen buffer after every pass"
        );
        assert!(published > 0, "the conversation must publish frames");
        assert_eq!(
            published_missing, 0,
            "no published frame during the steady conversation may lack the cursor"
        );
    }
}

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
        // Port-only: consume a rect bracket left open across passes (DOS's
        // travel_trail_stamp_last lifts without a balancing draw) — the
        // unconditional draw below satisfies the owed re-show. DOS instead
        // lets the next draw_mouse_cursor_if_needed anywhere burn the flag on
        // an over-show; the port must clear it here because a negative flag
        // also suppresses send_frame_to_display.
        if self.mouse_cursor_restore_needed < 0 {
            self.mouse_cursor_restore_needed = 0;
        }

        if self.cursor_mode == CursorMode::Overlay {
            // Tell the present thread what shape to draw; the freshest position
            // is sampled there from `SharedInput` instead of being routed
            // through the game tick. Published shown unconditionally: DOS
            // always ends this routine with vga_draw_cursor (seg000:dc5e) — a
            // negative hide counter (seg000:dc40 js) only skips the restore,
            // never the draw — so after every pass the cursor is visible.
            self.shared_cursor.publish(CursorOverlay {
                shape: new_image,
                hidden: false,
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
        // = seg000:dbb3 al = cursor_hide_counter (the pre-decrement value used by
        //   the restore test below).
        let old = self.cursor_hide_counter;
        // = seg000:dbb6 dec [cursor_hide_counter]; dbba js keep / dbbc inc undo —
        //   decrement in place, but keep the result only when it is negative;
        //   otherwise inc it straight back, so a positive over-shown count is left
        //   unchanged. The `dec` wraps like the 8086 byte op, so at the -128 floor
        //   `dec` yields +127 (not negative) and the undo restores -128: the
        //   counter saturates there instead of overflowing.
        let dec = old.wrapping_sub(1);
        if dec < 0 {
            self.cursor_hide_counter = dec;
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
    // composites the cursor into the framebuffer; Overlay re-publishes
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

    // = seg000:db74 restore_mouse_if_rect_intersects — lift the cursor only
    // when `rect` overlaps the 16x16 cursor image as last drawn (mouse_draw_pos
    // minus the shape's hotspot). An already-hidden cursor is left alone. On
    // overlap, mouse_cursor_restore_needed goes negative so
    // draw_mouse_cursor_if_needed knows a re-show is owed, and the hide falls
    // through into call_restore_cursor (seg000:dbb2).
    pub(crate) fn restore_mouse_if_rect_intersects(&mut self, rect: crate::Rect) {
        // = seg000:db74 cmp [cursor_hide_counter],0; js ret.
        if self.cursor_hide_counter < 0 {
            return;
        }
        // = seg000:db7d..db8c the drawn cursor's top-left corner:
        // mouse_draw_pos minus the hotspot of the current shape.
        let shape = cursor_shape(self.cursor_image.unwrap_or(CursorShapeId::Arrow));
        let x = self.mouse_draw_pos_x.wrapping_sub(shape.hotspot_x) as i16;
        let y = self.mouse_draw_pos_y.wrapping_sub(shape.hotspot_y) as i16;
        // = seg000:db90..dba7 the 16x16 overlap test against (x0,y0,x1,y1).
        if x >= rect.x1 || y >= rect.y1 || x + 16 <= rect.x0 || y + 16 <= rect.y0 {
            return;
        }
        // = seg000:dbae dec [mouse_cursor_restore_needed] — flag the pending
        // re-show, then fall through into call_restore_cursor.
        self.mouse_cursor_restore_needed = self.mouse_cursor_restore_needed.wrapping_sub(1);
        self.call_restore_cursor();
    }

    // = seg000:db67 draw_mouse_cursor_if_needed — re-show the cursor only when
    // restore_mouse_if_rect_intersects lifted it (the flag is negative);
    // otherwise do nothing at all — no draw_mouse, no counter change.
    pub(crate) fn draw_mouse_cursor_if_needed(&mut self) {
        // = seg000:db67 cmp [mouse_cursor_restore_needed],0; jns ret.
        if self.mouse_cursor_restore_needed >= 0 {
            return;
        }
        // = seg000:db6e inc [mouse_cursor_restore_needed]; db72 jmp draw_mouse.
        self.mouse_cursor_restore_needed += 1;
        self.draw_mouse();
    }

    // = seg000:db67 draw_mouse_cursor_if_needed closing a bracket that
    // presented mid-bracket. Port-only presentation care: DOS's re-draw lands
    // straight on VGA and is instantly visible, but the port's present inside
    // the bracket already published the frame with the cursor erased — so when
    // the cursor really was lifted (Baked mode, cursor pixels live in the
    // framebuffer), publish once more so the visible frame carries the
    // re-drawn cursor instead of staying cursor-less until the next present.
    pub(crate) fn draw_mouse_cursor_if_needed_then_present(&mut self) {
        let lifted = self.mouse_cursor_restore_needed < 0;
        self.draw_mouse_cursor_if_needed();
        if lifted && self.cursor_mode == CursorMode::Baked && !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // Publish the overlay cursor's current shape and hidden state to the
    // present thread. call_restore_cursor / draw_mouse call it so a hide (or
    // re-show) driven by an interaction reaches the compositor at once, without
    // waiting for the next redraw_mouse — the present thread samples the live
    // pointer position itself. Overlay only.
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
