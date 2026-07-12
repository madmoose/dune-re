//! In-game UI / HUD setup and rendering.
//!
//! Ported from the seg000 game-UI routines reached from `start` after the
//! intro + credits (seg000:0024 call init_game_ui). `draw_all_ui_elements`
//! selects the ICONES bank and paints each entry of the UI-element list via
//! `draw_ui_element`, and `ui_hud_head_draw` overlays the character
//! portrait via the bank loader (`bank.rs`). Still outstanding: the voice-
//! language config (check_amr_or_eng_language) and dispatch of the per-element
//! click handlers (UiElement::func_ptr).

use crate::{GameState, gfx};

const UI_ELEMENT_CLEAR_FLAG: u16 = 0x40;
const UI_ELEMENT_SKIP_SPRITE_FLAG: u16 = 0x20;

/// One in-game HUD element. = a 14-byte (`dw`×7) record of the DOS
/// `_word_20F94_ui_elements` list (seg001:1ae4): a hit/draw rect, flags, a
/// sprite to draw, and the seg000 offset of its click handler.
#[derive(Clone, Copy)]
pub struct UiElement {
    /// Top-left / bottom-right of the element's rect (hit area + draw origin).
    pub x0: u16,
    pub y0: u16,
    pub x1: u16,
    pub y1: u16,
    /// 0x40 = draw via the `[38d9h]` primitive; 0x20 = skip the sprite draw;
    /// 0x80 = ... (clickable). Tested by draw_ui_element (seg000:d208/d218).
    pub flags: u16,
    /// ICONES sprite drawn at (x0, y0); -1 = none.
    pub sprite_id: i16,
    /// seg000 offset of the element's click handler (e.g. loc_00f66 = no-op).
    /// `dispatch_ui_click` matches on it after a hit-test, mirroring DOS's
    /// `call cs:[di+0xC]` (seg000:d8d4): the click dispatcher walks this list on
    /// a mouse hit, tests the 0x80 flag + hit rect, then invokes this offset.
    pub func_ptr: u16,
    pub handler: Option<ClickHandlerFn>,
}

type ClickHandlerFn = fn(state: &mut GameState);

const fn ui1(
    x0: u16,
    y0: u16,
    x1: u16,
    y1: u16,
    flags: u16,
    sprite_id: i16,
    func_ptr: u16,
) -> UiElement {
    UiElement {
        x0,
        y0,
        x1,
        y1,
        flags,
        sprite_id,
        func_ptr,
        handler: None,
    }
}

#[allow(clippy::too_many_arguments)]
const fn ui2(
    x0: u16,
    y0: u16,
    x1: u16,
    y1: u16,
    flags: u16,
    sprite_id: i16,
    func_ptr: u16,
    handler: Option<ClickHandlerFn>,
) -> UiElement {
    UiElement {
        x0,
        y0,
        x1,
        y1,
        flags,
        sprite_id,
        func_ptr,
        handler,
    }
}

/// = seg001:1ae4 _word_20F94_ui_elements — the static initial HUD layout (the
/// `dw 24` count prefix is implicit in the array length). GameState copies this
/// into a mutable field at startup; gameplay mutates entries in place (e.g. the
/// date/time digit sprite_ids of records 1–2, via seg000:d259). The trailing
/// comment on each row is the DOS click-handler label for its func_ptr.
#[rustfmt::skip]
pub const UI_ELEMENTS_INIT: [UiElement; 24] = [
    ui1( 22, 161,  68, 196, 0x0000, -1, 0xb8c6), //  0 loc_0b8c6
    ui2(  0, 152,   0, 152, 0x0000,  0, 0x0f66, None), //  1 loc_00f66 (date/time, mutated)
    ui2(228, 152, 300, 198, 0x0000,  3, 0x0f66, None), //  2 loc_00f66 (date/time, mutated)
    ui1( 24, 155,  69, 176, 0x0080, -1, 0xaed6), //  3 loc_0aed6
    ui2 (92, 152, 229, 159, 0x0000, 14, 0x0f66, None), //  4 loc_00f66
    ui2(  2, 154,   2, 154, 0x0000, 12, 0x0f66, None), //  5 loc_00f66
    ui2(317, 154, 317, 154, 0x0000, 12, 0x0f66, None), //  6 loc_00f66
    ui2( 92, 159, 228, 167, 0x0080, 27, 0xd443, Some(GameState::dispatch_command_menu_slot_0)), //  7 loc_0d443
    ui2( 92, 167, 228, 175, 0x0080, 27, 0xd43e, Some(GameState::dispatch_command_menu_slot_1)), //  8 loc_0d43e
    ui2( 92, 175, 228, 183, 0x0080, 27, 0xd439, Some(GameState::dispatch_command_menu_slot_2)), //  9 loc_0d439
    ui2( 92, 183, 228, 191, 0x0080, 27, 0xd434, Some(GameState::dispatch_command_menu_slot_3)), // 10 loc_0d434
    ui2( 92, 191, 228, 199, 0x0080, 27, 0xd42f, Some(GameState::dispatch_command_menu_slot_4)), // 11 loc_0d42f
    ui2(255, 162, 295, 192, 0x0000, -1, 0x0f66, None), // 12 loc_00f66
    ui2(269, 162, 279, 172, 0x0000, -1, 0x0f66, None), // 13 loc_00f66
    ui2(284, 172, 294, 182, 0x0000, -1, 0x0f66, None), // 14 loc_00f66
    ui2(269, 181, 279, 191, 0x0000, -1, 0x0f66, None), // 15 loc_00f66
    ui2(255, 172, 265, 182, 0x0000, -1, 0x0f66, None), // 16 loc_00f66
    ui2(  0,   0,   0,   0, 0x0000, -1, 0x0f66, None), // 17 loc_00f66
    ui2(  0,   0,   0,   0, 0x0000, -1, 0x0f66, None), // 18 loc_00f66
    ui1(  0,   0,   0,   0, 0x0000, -1, 0x945b), // 19 loc_0945b
    ui2(  0,   0, 320, 152, 0x0000, -1, 0x941d, Some(GameState::game_area_click)), // 20 loc_0941d
    ui2( 35, 182,  56, 196, 0x0080, 64, 0x9215, Some(GameState::callback_main_ui_element_21_22)), // 21 loc_09215
    ui2( 58, 182,  79, 196, 0x0080, 64, 0x9215, Some(GameState::callback_main_ui_element_21_22)), // 22 loc_09215
    ui1(  0,   4,  40,  46, 0x0000, -1, 0xb1ee), // 23 loc_0b1ee
];

/// = seg001:1c36 data_01c36 — the closed-book (normal room view) frieze-side
/// template: the (flags, sprite_id) applied to HUD records 0..4 by
/// `ui_set_and_draw_frieze_sides`. The sibling templates (open book 1c56, map
/// 1c66, globe 1c46) land with those views.
const FRIEZE_SIDES_CLOSED_BOOK: [(u16, i16); 4] = [(0, -1), (0, 0), (0, 3), (0x80, -1)];

/// = seg001:1e7e the date/time moon/sun coordinate table, indexed by the
/// time-of-day phase (game_time & 0xf). Each phase gives the screen position of
/// ICONES sprite 0x4a then sprite 0x4b; an x of 0 means that body is off screen.
#[rustfmt::skip]
const SUN_MOON_COORDS: [[(u16, u16); 2]; 16] = [
    [(  6, 187), ( 25, 186)],
    [(  6, 186), ( 26, 188)],
    [(  6, 185), (  0,   0)],
    [(  7, 183), (  0,   0)],
    [(  9, 182), (  0,   0)],
    [( 10, 181), (  0,   0)],
    [( 13, 181), (  0,   0)],
    [( 16, 181), (  0,   0)],
    [( 18, 182), (  0,   0)],
    [( 20, 183), (  0,   0)],
    [( 20, 185), (  0,   0)],
    [( 20, 186), (  8, 188)],
    [( 20, 187), (  9, 186)],
    [(  0,   0), ( 12, 183)],
    [(  0,   0), ( 17, 182)],
    [(  0,   0), ( 23, 183)],
];

/// = seg001:1c76 the room-view navigation panel template — the 6 records (HUD
/// records 12..18) of the bottom-right compass: a backing box (sprite 33) and
/// the N/E/S/W move-direction buttons (sprites 29..32, handlers ui_click_room_*)
/// plus the centre (sprite 36). `ui_setup_and_draw_nav_panel` copies it into place.
#[rustfmt::skip]
const NAV_PANEL_ROOM: [UiElement; 6] = [
    ui2(255, 162, 295, 192, 0x0000, 33, 0x0f66, None),                                 // 12 box
    ui2(269, 162, 279, 172, 0x0080, 29, 0x3f15, Some(GameState::ui_click_move_up)),    // 13 up
    ui2(284, 172, 294, 182, 0x0080, 30, 0x3f1a, Some(GameState::ui_click_move_right)), // 14 right
    ui2(269, 181, 279, 191, 0x0080, 31, 0x3f1f, Some(GameState::ui_click_move_down)),  // 15 down
    ui2(255, 172, 265, 182, 0x0080, 32, 0x3f24, Some(GameState::ui_click_move_left)),  // 16 left
    ui2(269, 173, 280, 181, 0x0080, 36, 0x18ee, Some(GameState::ui_draw_palace_plan)), // 17 centre
];

/// = seg001:1cca the alternate (ornithopter/travel) navigation panel template,
/// used when `data_046eb` is set.
#[rustfmt::skip]
const NAV_PANEL_ALT: [UiElement; 6] = [
    ui1(266, 171, 285, 184, 0x0080, 41, 0x5b05),
    ui1(267, 162, 284, 171, 0x4080, 37, 0x8829),
    ui1(285, 171, 297, 184, 0x4080, 38, 0x8824),
    ui1(267, 184, 284, 193, 0x4080, 39, 0x882e),
    ui1(254, 171, 266, 184, 0x4080, 40, 0x881f),
    ui2(266, 171, 284, 183, 0x0000, 53, 0x0f66, None),
];

/// = seg001:1d72 the map/book-mode navigation panel template, used when
/// `game_screen_mode_flags & 3` is set.
#[rustfmt::skip]
const NAV_PANEL_MAP: [UiElement; 6] = [
    ui2(262, 168, 263, 169, 0x0200, -1, 0x0f66, None),
    ui1(258, 172, 266, 182, 0x4080, 42, 0x4ad0),
    ui1(270, 170, 279, 182, 0x0080, 43, 0x4f09),
    ui1(283, 172, 291, 182, 0x4080, 44, 0x4ad7),
    ui2(  0,   0,   0,   0, 0x0000, -1, 0x0f66, None),
    ui2(  0,   0,   0,   0, 0x0000, -1, 0x0f66, None),
];

/// = seg001:1d1e the LOOK AT MIRROR navigation-panel template. Unlike the
/// move/travel/map panels it carries no compass sprites and no clickable
/// records (every sprite_id = -1, every handler = loc_00f66), so installing it
/// blanks the bottom-right compass for the mirror still. The full game-area
/// hotspot (record 20) that `callback_transition_look_at_mirror` arms handles
/// the look-away click instead.
#[rustfmt::skip]
pub(crate) const NAV_PANEL_MIRROR: [UiElement; 6] = [
    ui2(262, 168, 263, 169, 0x0200, -1, 0x0f66, None),
    ui2(258, 172, 266, 182, 0x0000, -1, 0x0f66, None),
    ui2(270, 170, 279, 182, 0x0000, -1, 0x0f66, None),
    ui2(283, 172, 291, 182, 0x0000, -1, 0x0f66, None),
    ui2(266, 186, 291, 191, 0x0000, -1, 0x0f66, None),
    ui2(  0,   0,   0,   0, 0x0000, -1, 0x0f66, None),
];

/// = the `MouseHandlers` record `active_mouse_handlers` (seg001:2570) points at:
/// the 7-word table of near-call handlers `game_loop`'s click/hover dispatch
/// invokes (`call word ptr [si]` idle, `[si+2]` LMB click, `[si+0ah]` LMB drag,
/// ...). `select_room_ui_table` (seg000:d95b) swaps which record it points at as
/// the active screen changes (room-screen, mixer panel, ...); the port models
/// the handlers it dispatches as a `&'static MouseHandlers` on `GameState`.
///
/// For the right button `game_loop` biases the record base by one word (`add
/// si,2` at seg000:d8b5), so the LMB click/release/drag dispatch sites `[si+2]`/
/// `[si+6]`/`[si+0ah]` re-read the RMB `rmb`/`rmb_release`/`rmb_drag` slots at
/// +4/+8/+0ch instead.
pub(crate) struct MouseHandlers {
    /// = `[si]` — the idle/hover handler, called when no button edge fired.
    pub idle: fn(&mut GameState),
    /// = `[si+2]` — the LMB press handler, called by the game loop
    /// (game_loop_dispatch_lmb_press) ONLY when the shared ui_element hit-test
    /// finds no clickable element under the cursor. Room: fn_0d917_noop (no-op);
    /// mixer: mixer_panel_lmb (loc_0a576), which closes the panel on a click
    /// outside its rect. The hit-test + element dispatch itself is not in here.
    pub lmb: fn(&mut GameState),
    /// = `[si+4]` — the RMB handler, called on a right-button press (`game_loop`
    /// selects this slot via the `rmb` flag in place of the `add si,2` bias).
    pub rmb: fn(&mut GameState),
    /// = `[si+6]` — the LMB-release handler, called on the button-up edge
    /// (= seg000:d955). The mixer table's slot clears its drag target; the room
    /// table's is fn_0d917_noop (a no-op).
    pub release: fn(&mut GameState),
    /// = `[si+8]` — the RMB-release handler, the right-button counterpart of
    /// `release` (reached as `[si+6]` under the `add si,2` bias). Both tables wire
    /// it to a no-op (room fn_0d917_noop, mixer loc_00f66).
    pub rmb_release: fn(&mut GameState),
    /// = `[si+0ah]` — the drag handler, called each pass the LMB is held
    /// without an edge and the pointer moved, with the (dx, dy) motion delta.
    pub drag: fn(&mut GameState, i16, i16),
    /// = `[si+0ch]` — the RMB-drag handler, the right-button counterpart of
    /// `drag` (reached as `[si+0ah]` under the `add si,2` bias). Both tables wire
    /// it to a no-op (room fn_0d917_noop, mixer loc_00f66).
    pub rmb_drag: fn(&mut GameState, i16, i16),
}

/// = the room-screen variant of the `data_02570` record (the one
/// `select_room_ui_table` selects for the in-game room view). Its LMB-press slot
/// is a no-op (fn_0d917_noop) — the HUD element hit-test + dispatch runs in the
/// game loop (game_loop_dispatch_lmb_press) for every screen, not here. The RMB
/// and release/drag slots are no-ops too (the room arms no drag target), and the
/// idle handler's body (loc_01ae7) is a stub.
pub(crate) static ROOM_MOUSE_HANDLERS: MouseHandlers = MouseHandlers {
    idle: GameState::room_mouse_idle,
    lmb: GameState::room_mouse_lmb,
    rmb: GameState::room_mouse_rmb,
    release: GameState::room_mouse_release,
    rmb_release: GameState::room_mouse_rmb_release,
    drag: GameState::room_mouse_drag,
    rmb_drag: GameState::room_mouse_rmb_drag,
};

/// = seg001:1ad6 settings_ui_mouse_handlers — the in-game mixer/settings panel's
/// `MouseHandlers` record. idle and RMB are the no-op loc_00f66; the LMB handler
/// is loc_0a576 (`mixer_panel_lmb`), which hit-tests the panel rect and closes
/// the panel on a miss, or dispatches to the sliders / button grid on a hit.
/// `open_mixer_panel` installs it via `active_mouse_handlers` (= loc_0d95e).
pub(crate) static MIXER_MOUSE_HANDLERS: MouseHandlers = MouseHandlers {
    idle: GameState::mixer_panel_idle,
    lmb: GameState::mixer_panel_lmb,
    rmb: GameState::mixer_panel_rmb,
    release: GameState::mixer_panel_release,
    rmb_release: GameState::mixer_panel_rmb_release,
    drag: GameState::mixer_panel_drag,
    rmb_drag: GameState::mixer_panel_rmb_drag,
};

impl GameState {
    // = seg000:0083 init_game_ui — configure the voice/subtitle language, then
    // draw the in-game HUD (falls through into draw_game_ui at seg000:0086).
    // Called once from start (seg000:0024) before the game loop.
    pub fn init_game_ui(&mut self) {
        // = seg000:0083 call check_amr_or_eng_language.
        self.check_amr_or_eng_language();
        // = seg000:0086 fall through into draw_game_ui.
        self.draw_game_ui();
    }

    // = seg000:0086 draw_game_ui — clear fb1, draw every HUD element offscreen,
    // then overlay the character head-and-shoulders portrait. Also entered
    // standalone from seg000:3768 to redraw the HUD.
    pub fn draw_game_ui(&mut self) {
        // = seg000:0086 set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = seg000:0089 gfx_clear_active_framebuffer.
        self.gfx_clear_active_framebuffer();
        // = seg000:008c
        self.gfx_call_bp_with_front_buffer_as_screen(|s| s.draw_all_ui_elements());
        // = seg000:0095 jmp ui_hud_head_draw.
        self.ui_hud_head_draw();
    }

    // = seg000:d1ef draw_all_ui_elements.
    fn draw_all_ui_elements(&mut self) {
        self.draw_ui_elements_list(0, self.ui_elements.len());
    }

    // = seg000:d1f2 draw_ui_elements_list.
    fn draw_ui_elements_list(&mut self, start: usize, count: usize) {
        // = seg000:d1f2 call open_icones_spritesheet.
        self.open_icones_spritesheet();
        // = seg000:d1f5 loop the records via draw_ui_element.
        for i in start..start + count {
            self.draw_ui_element(self.ui_elements[i]);
        }
    }

    // = seg000:d200 draw_ui_element — render one HUD element to the front buffer.
    fn draw_ui_element(&mut self, e: UiElement) {
        // = seg000:d200 push [framebuffer_active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();

        // = seg000:d208 test data_00008[si],40h — clear the element's rect first.
        if e.flags & UI_ELEMENT_CLEAR_FLAG != 0 {
            // = seg000:d213 call gfx_vtable_vga_clear_rect (rect = the record).
            gfx::vga_clear_rect(self, e.x0, e.y0, e.x1, e.y1);
        }

        // = seg000:d218 test data_00008[si],20h — 0x20 skips the sprite draw.
        if e.flags & UI_ELEMENT_SKIP_SPRITE_FLAG == 0 {
            // = seg000:d22b lodsw sprite_id; inc ax/jz skips -1.
            if e.sprite_id != -1 {
                // = seg000:d230 dx=x0, bx=y0, ax=sprite_id, draw_sprite_clobbering_bx_dx.
                self.draw_active_bank_sprite(e.sprite_id as u16, e.x0 as i16, e.y0 as i16);
            }
        }
        // = seg000:d234 pop [framebuffer_active_seg].
        self.active_fb = saved;
    }

    // = seg000:d239
    pub(crate) fn ui_hud_open_hands(&mut self) {
        self.ui_hud_animate_hands(2);
    }

    // = seg000:d23d
    pub(crate) fn ui_hud_close_hands(&mut self) {
        self.ui_hud_animate_hands(0);
    }

    // = seg000:d23f
    fn ui_hud_animate_hands(&mut self, target: i16) {
        // = seg000:d242 ax = ui_elements[1].sprite_id; div 3; cmp ch (target), ah
        //   (current % 3) — nothing to do when already there.
        let rem = self.ui_elements[1].sprite_id % 3;
        if rem == target {
            return;
        }
        // = seg000:d250 jnb -> +1 (target >= current%3), else neg -> -1.
        let dir = if target >= rem { 1 } else { -1 };
        // = seg000:d257..d27c the two unrolled steps (a wait_a_bit between them).
        for step in 0..2 {
            // = seg000:d259 add [si+0ah],ax (elem[1].sprite); d25c add [si+18h],ax
            //   (elem[2].sprite) — both hands step together.
            self.ui_elements[1].sprite_id += dir;
            self.ui_elements[2].sprite_id += dir;
            // = seg000:d262 draw_ui_elements_list_at_ds_si (cx=2 from si=elem[1]).
            self.draw_ui_elements_list(1, 2);
            // = seg000:d265 ui_draw_date_and_time_indicator — a no-op unless the
            //   left hand is fully closed (its guard is sprite_id == 0), so the
            //   date reappears only on the final step.
            self.ui_draw_date_and_time_indicator();
            // The sprites drew into the visible screen; present this frame.
            self.send_frame_to_display();
            // = seg000:d268 wait_a_bit(0xa) — 10 PIT ticks between the two steps
            //   (none after the second; the fold's vsync follows).
            if step == 0 {
                let start = self.game_ticks();
                self.sleep_ticks(start, 10);
            }
        }
    }

    // = seg000:1860 ui_enter_room_view — switch the display into the in-game room
    // view (then start the head portrait). Guard: returns immediately while
    // game_screen_mode_flags is nonzero (a non-room screen is active). Otherwise
    // folds any open head portrait down, then falls into ui_toggle_room_view.
    // Called once from start (seg000:002c) before the game loop.
    pub fn ui_enter_room_view(&mut self) {
        // = seg000:1860 cmp game_screen_mode_flags, 0; jnz ret.
        if self.game_screen_mode_flags != 0 {
            return;
        }
        // = seg000:1868 call ui_hud_head_animate_down_start.
        self.ui_hud_head_animate_down_start();
        // = seg000:186b fall into ui_toggle_room_view.
        self.ui_toggle_room_view();
    }

    // = seg000:186b ui_toggle_room_view — shared body (also entered from menu /
    // button handlers). Clears the mouse rect, then negs room_view_toggle: a
    // non-negative result shows the in-game room view, otherwise the globe/map
    // view (ui_show_globe_map_view).
    fn ui_toggle_room_view(&mut self) {
        // = seg000:186b call clear_some_mouse_rect.
        self.clear_some_mouse_rect();
        // = seg000:186e neg room_view_toggle; jns -> room view, else map view.
        self.room_view_toggle = (self.room_view_toggle as i8).wrapping_neg() as u8;
        if (self.room_view_toggle as i8) < 0 {
            // = seg000:1874 jmp ui_show_globe_map_view.
            self.ui_show_globe_map_view();
            return;
        }
        // = seg000:1877 loc_01877 — the enter-room path. Drain pending UI tasks,
        // reset the scene, restore the voice/subtitle mode from its default.
        self.dismiss_stacked_overlays();
        self.reset_room_scene_state();
        // = seg000:187d voice_subtitle_mode = voice_subtitle_mode_default.
        self.voice_subtitle_mode = self.voice_subtitle_mode_default;
        // = seg000:1883 remove the globe frame tasks.
        self.remove_globe_frame_tasks();
        // = seg000:1886 enable the two HUD buttons (book/dialogue icons).
        self.ui_elements[21].flags = 0x80;
        self.ui_elements[22].flags = 0x80;
        // = seg000:1892 bp = ui_set_and_draw_frieze_sides_closed_book; draw it
        // offscreen.
        self.gfx_call_bp_with_front_buffer_as_screen(|s| {
            s.ui_set_and_draw_frieze_sides_closed_book()
        });
        // = seg000:1898 al = 0x34 (transition effect); fall into
        // ui_present_room_screen.
        self.ui_present_room_screen(0x34);
    }

    // = seg000:d717 ui_setup_and_draw_nav_panel — set up and draw the bottom-right
    // navigation panel (HUD records 12..18, the move/travel compass). Picks the
    // panel template for the current view, copies its 6 records into place, then
    // draws them. Called from draw_room_game_screen via the offscreen helper.
    // (DOS named it the "room HUD strip"; it is in fact the nav panel.)
    pub(crate) fn ui_setup_and_draw_nav_panel(&mut self) {
        // = seg000:d717 cmp data_046eb,0; jnz -> alt panel (1cca). Else
        // = seg000:d721 test game_screen_mode_flags,3 -> map/book panel (1d72),
        // otherwise the room panel (1c76).
        let template = if self.data_046eb != 0 {
            &NAV_PANEL_ALT
        } else if self.game_screen_mode_flags & 3 != 0 {
            &NAV_PANEL_MAP
        } else {
            &NAV_PANEL_ROOM
        };
        // = seg000:d715 jmp loc_0d72b (the d712 path falls in the same way).
        self.ui_install_nav_panel(template);
    }

    // = seg000:d72b loc_0d72b — copy a 6-record nav-panel template (ds:si) into
    // the live HUD records 12..17 (di=1b8eh, cx=2ah rep movsw), then loc_0d735
    // fills the panel background and draws them. Reached from ui_setup_nav_panel
    // with the view template, and directly from callback_transition_look_at_mirror
    // (si=1d1eh) to blank the compass for the mirror still.
    pub(crate) fn ui_install_nav_panel(&mut self, template: &[UiElement; 6]) {
        // = seg000:d72b di=1b8eh; cx=2ah; rep movsw.
        self.ui_elements[12..18].copy_from_slice(template);
        // = seg000:d735 loc_0d735.
        self.ui_draw_nav_panel();
    }

    // = seg000:98f5 main_ui_elements_clear_flags_18_19_20 — clear the flags of
    // HUD records 18, 19 and 20 (the head/portrait/dialogue and game-area
    // hotspots), disabling their hit-tests.
    pub(crate) fn main_ui_elements_clear_flags_18_19_20(&mut self) {
        self.ui_elements[20].flags = 0;
        self.ui_elements[19].flags = 0;
        self.ui_elements[18].flags = 0;
    }

    // = seg000:d735 loc_0d735 — fill the nav-panel background (loc_0d741) then
    // draw the 6 nav records. Also reached from the command panel's nav rebuild
    // (loc_02ffb) once it has populated records 12..18.
    pub(crate) fn ui_draw_nav_panel(&mut self) {
        // = seg000:d741 loc_0d741 — when record[2].sprite_id is in 3..6 (the
        // closed/open-book frieze states), fill the nav-panel rect (seg001:2458)
        // with colour 0xf0 before drawing the compass over it.
        if (self.ui_elements[2].sprite_id as u16).wrapping_sub(3) < 3 {
            gfx::vga_fill_rect(self, 254, 162, 296, 193, 0xf0);
        }
        // = seg000:d738 si=1b8eh; cx=6; draw records 12..18.
        self.draw_ui_elements_list(12, 6);
    }

    // ---- Not-yet-ported callees (no-op stubs, each linked to its DOS address).

    // = seg000:daa3 clear_some_mouse_rect — clear the active mouse hotspot rect.
    // TODO: port; no-op stub.
    pub(crate) fn clear_some_mouse_rect(&mut self) {}

    // = seg000:d2bd dismiss_stacked_overlays — before a view switch, tear down the
    // transient menus/overlays stacked over the base room menu, running each one's
    // cleanup func (screen_element_stack_pop_and_cleanup). The drain stops at a
    // base/locked entry: a leading priority byte of 0xff (the room command menu or
    // the look-away overlay) or one whose low nibble is 0. in_transition is forced
    // to 0x80 across the loop so the cleanups' repaints do not arm a panel fold,
    // then restored.
    pub(crate) fn dismiss_stacked_overlays(&mut self) {
        // = seg000:d2bd al = in_transition; push — saved and restored around the loop.
        let saved = self.in_transition;
        loop {
            // = seg000:d2c1 in_transition = 0x80.
            self.in_transition = 0x80;
            // = seg000:d2c6 si = [screen_element_stack_ptr]; si = [si]; al = [si] —
            //   the top screen element's leading priority byte.
            let lead = self.get_active_screen_element().priority_byte();
            // = seg000:d2cd cmp al,0ffh; jz — a 0xff-locked base/overlay stops here.
            if lead == 0xff {
                break;
            }
            // = seg000:d2d1 and al,0fh; jz — a base menu (low nibble 0) stops here.
            if lead & 0x0f == 0 {
                break;
            }
            // = seg000:d2d5 call screen_element_stack_pop_and_cleanup — pop the top
            //   transient overlay and run its cleanup; never reached for a 0xff
            //   entry, so the loop always makes progress toward the base.
            self.screen_element_stack_pop_and_cleanup();
        }
        // = seg000:d2da pop ax; in_transition = al — restore.
        self.in_transition = saved;
    }

    // = seg000:5adf reset_room_scene_state — reset per-scene state (particles and
    // assorted scene globals) before drawing the room.
    // TODO: port; no-op stub.
    fn reset_room_scene_state(&mut self) {}

    // = seg000:b930 remove_globe_frame_tasks — stop the globe/map animation frame
    // tasks.
    // TODO: port; no-op stub.
    fn remove_globe_frame_tasks(&mut self) {}

    // = seg000:d75a ui_set_and_draw_frieze_sides_closed_book — draw the in-game
    // HUD's left panel: the frieze sides (room view = closed-book template), the
    // date/time indicator, and the two bottom-left book/companion buttons. Run
    // via gfx_call_bp_with_front_buffer_as_screen so it composes offscreen.
    pub(crate) fn ui_set_and_draw_frieze_sides_closed_book(&mut self) {
        // = seg000:d75a si = data_01c36 (the closed-book frieze template).
        self.ui_set_and_draw_frieze_sides(&FRIEZE_SIDES_CLOSED_BOOK);
        // = seg000:d760 call ui_draw_date_and_time_indicator.
        self.ui_draw_date_and_time_indicator();
        // = seg000:d763 fall into the book/companion button redraw.
        self.ui_hud_draw_companions();
    }

    // = seg000:d763 the tail of ui_set_and_draw_frieze_sides_closed_book (also
    // reached from the command panel at seg000:2eef): redraw the two bottom-left
    // buttons (records 21,22) — first the frame sprite 0x40, then their state
    // icon (0x41 + data_01152 / data_01153) overlaid on top.
    pub(crate) fn ui_hud_draw_companions(&mut self) {
        // = seg000:d766 both records' sprite_id = 0x40 (the empty button frame).
        self.ui_elements[21].sprite_id = 0x40;
        self.ui_elements[22].sprite_id = 0x40;
        // = seg000:d76f cx=2; draw records 21,22.
        self.draw_ui_elements_list(21, 2);
        // = seg000:d778 sprite_id = (sign-extended state byte) + 0x41, per button.
        self.ui_elements[21].sprite_id = self.companion_1 + 0x41;
        self.ui_elements[22].sprite_id = self.companion_2 + 0x41;
        // = seg000:d78c cx=2; draw records 21,22 again over the frames.
        self.draw_ui_elements_list(21, 2);
    }

    // = seg000:d795 ui_set_and_draw_frieze_sides — apply a frieze template to the
    // flags + sprite_id of HUD records 0..4 (DOS copies 2 words per record from
    // the template into di = record[0]+8, stepping one 0eh-byte record), then
    // draw records 0,1,2. The template chooses the side decoration for the
    // current view (closed book / open book / map / globe at seg001:1c36/1c56/
    // 1c66/1c46).
    fn ui_set_and_draw_frieze_sides(&mut self, template: &[(u16, i16); 4]) {
        // = seg000:d797 di=1aeeh; cx=4; copy (flags, sprite_id) into records 0..4.
        for (i, &(flags, sprite_id)) in template.iter().enumerate() {
            self.ui_elements[i].flags = flags;
            self.ui_elements[i].sprite_id = sprite_id;
        }
        // = seg000:d7a4 si=1ae6h; cx=3; draw records 0,1,2.
        self.draw_ui_elements_list(0, 3);
    }

    // = seg000:1a34 ui_draw_date_and_time_indicator — draw the moon/sun
    // time-of-day phase and the in-game day number into the HUD, onto the active
    // framebuffer's "screen". Guarded by record[1].sprite_id == 0.
    fn ui_draw_date_and_time_indicator(&mut self) {
        // = seg000:1a34 cmp record[1].sprite_id, 0; jnz ret.
        if self.ui_elements[1].sprite_id != 0 {
            return;
        }
        // = seg000:1a3b push [active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();

        // = seg000:1a42 ax = game_time & 0xf — the time-of-day phase; index the
        // sun/moon coordinate table. draw_sun_and_moon (seg000:1a9b) blits the
        // body via vga_blit_clipped from the active bank (ICONES, opened by the
        // frieze draw above); the port uses the regular active-bank sprite
        // blitter. A body with x == 0 is off screen (= seg000:1aa3 `or dx,dx; jz`).
        let [sun, moon] = SUN_MOON_COORDS[self.game_time as usize % SUN_MOON_COORDS.len()];
        // = seg000:1a53 mov ax,4ah; call draw_sun_and_moon — ICONES sprite 0x4a.
        self.ui_draw_date_and_time_indicator_sprite(0x4a, sun);
        // = seg000:1a59 mov ax,4bh; call draw_sun_and_moon — ICONES sprite 0x4b.
        self.ui_draw_date_and_time_indicator_sprite(0x4b, moon);

        // = seg000:1a5f font_select_small_font; colour 0xf1fa (fg 0xfa, bg 0xf1).
        self.font_select_small_font();
        self.font_state.color = 0xf1fa;
        // = seg000:1a68 get_ingame_day_in_ax; day = (day mod 365) + 1.
        let day = self.get_ingame_day_in_ax() % 365 + 1;
        // = seg000:1a77 x = 0xb, shifted left 2px per missing digit; y = 190.
        let mut x = 11;
        if day < 100 {
            x -= 2;
        }
        if day < 10 {
            x -= 2;
        }
        // = seg000:1a8d call loc_0e290 (draw the 3-digit day).
        self.font_draw_number_right_aligned_at(x, 190, day);
        // = seg000:1a90 mov al,20h; call glyph func — a trailing space.
        self.font_state.color = 0xf1fa;
        self.font_draw_glyph(b' ');

        // = seg000:1a96 pop [active_seg].
        self.active_fb = saved;
    }

    // = seg000:1a42
    fn ui_draw_date_and_time_indicator_sprite(&mut self, id: u16, (x, y): (u16, u16)) {
        let clip = crate::Rect {
            x0: 6,
            y0: 181,
            x1: 30,
            y1: 190,
        };
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_sprite_from_sheet_clipped(sheet, id, x as i16, y as i16, clip);
        });
    }

    // = seg000:1a0f loc_01a0f — refresh just the date/time indicator on the live
    // screen (the path run_events_for_current_time_period takes when the game
    // clock advances). Selects the ICONES bank, redraws the date-area frieze
    // background (HUD record 1) to erase the previous sun/moon/day, redraws the
    // indicator, presents the updated rect, then restores the previous bank.
    pub(crate) fn ui_redraw_date_and_time_indicator(&mut self) {
        // = seg000:1a0f cmp ui_elements[1].sprite_id, 0; jnz ret — only when the
        // room view shows the date/time area (the same guard the indicator draw
        // re-checks); other views park a nonzero sprite_id there.
        if self.ui_elements[1].sprite_id != 0 {
            return;
        }
        // = seg000:1a16 call call_restore_cursor — restore the cursor-covered
        // pixels before redrawing under it. TODO: port the cursor save/restore.
        // = seg000:1a19 push [2784]; 1a1d open_icones_spritesheet — the sun/moon and
        // frieze sprites live in ICONES; keep the previous bank to restore.
        let prev = self.open_icones_spritesheet();
        // = seg000:1a20 si = ui_elements[1]; 1a23 draw_ui_element — repaint the
        // date-area frieze background (clears the old indicator first).
        let bg = self.ui_elements[1];
        self.draw_ui_element(bg);
        // = seg000:1a26 call ui_draw_date_and_time_indicator.
        self.ui_draw_date_and_time_indicator();
        // = seg000:1a29 si = 1f06h; gfx_copy_rect_to_screen — DOS copies the
        // updated rect to VGA. The port draws straight into the screen buffer, so
        // present the frame (unless composing offscreen, where the caller blits).
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
        // = seg000:1a30 pop ax; open_spritesheet — restore the prior bank.
        self.open_sprite_bank(prev as i16);
    }

    // = seg000:1ad1 get_ingame_day_in_ax — the in-game day index, (game_time+3)>>4.
    fn get_ingame_day_in_ax(&self) -> u16 {
        (self.game_time + 3) >> 4
    }

    // = seg000:5a1a ui_show_globe_map_view — leave the room view and bring up the
    // globe/map view (the else-branch of ui_toggle_room_view).
    // TODO: port; no-op stub.
    fn ui_show_globe_map_view(&mut self) {}

    // = seg000:d443
    fn dispatch_command_menu_slot_0(&mut self) {
        self.dispatch_command_menu_slot(0);
    }

    // = seg000:d43e
    fn dispatch_command_menu_slot_1(&mut self) {
        self.dispatch_command_menu_slot(1);
    }

    // = seg000:d439
    fn dispatch_command_menu_slot_2(&mut self) {
        self.dispatch_command_menu_slot(2);
    }

    // = seg000:d434
    fn dispatch_command_menu_slot_3(&mut self) {
        self.dispatch_command_menu_slot(3);
    }

    // = seg000:d42f
    fn dispatch_command_menu_slot_4(&mut self) {
        self.dispatch_command_menu_slot(4);
    }

    // = seg000:d6b7 loc_0d6b7 — walk the HUD element list and return the index of
    // the first clickable element whose rect contains the pointer. DOS loops si
    // over the [1ae4h] = 0x18 (24) records, skipping any without flag 0x80, and
    // tests x0 < x < x1, y0 < y <= y1 — the dec bx/inc bx around the y1 compare
    // (seg000:d6f0) makes the bottom bound inclusive, so the shared edge between
    // two stacked verb slots belongs to the upper one, matching the hover test
    // in verb_strip_hovered_slot (loc_0d5c7).
    //
    // The special-cursor branch _dc4b (set_di_to_ui_elements_ptr_based_on_cursor_image,
    // seg000:d6fd) that picks a specific record for the room-edge travel-arrow
    // cursors is skipped: only the Arrow cursor is ported (get_mouse_cursor_image
    // always returns Arrow), which already takes this normal hit-test path.
    // TODO: port the _dc4b travel-arrow-cursor branch once those cursors select.
    pub(crate) fn hit_test_ui_elements(&self) -> Option<usize> {
        let x = self.mouse_pos_x;
        let y = self.mouse_pos_y;
        // = seg000:d6bc cx = [1ae4h] = the element count (24); di = ui_elements[0].
        // = seg000:d6c4 cmp [active_mouse_handlers], settings_ui_mouse_handlers; jnz
        //   loc_0d6cf — while the mixer panel is active, drop the last five records
        //   (cx -= 5): ui_elements[19..24] are the game-area hotspot (20), the two
        //   person buttons (21/22) and the book button (23), so only the panel and
        //   the command strip stay clickable beneath the overlay.
        //
        // TODO: seg000:d6cf the pending_room_screen_request != 0 branch (cx = 5,
        //   di = ui_elements[7]) that restricts the test to the five command slots
        //   during a room swap is not modelled — the port's room swap is itself
        //   stubbed, so honouring it here could strand clicks while it stays set.
        let end = if std::ptr::eq(self.active_mouse_handlers, &MIXER_MOUSE_HANDLERS) {
            self.ui_elements.len() - 5
        } else {
            self.ui_elements.len()
        };
        // = seg000:d6dc the test loop, one 0eh-byte record per pass.
        self.ui_elements[..end].iter().position(|e| {
            // = seg000:d6dc test [di+8],80h — only clickable records (`jns` skips).
            e.flags & 0x80 != 0
                // = seg000:d6e2..d6f4 rect test: x strict on both sides, y
                // strict at the top but inclusive at the bottom (dec bx; cmp
                // bx,[di+6]; inc bx; jb).
                && e.x0 < x && x < e.x1
                && e.y0 < y && y <= e.y1
        })
    }

    // = seg000:x call word ptr bitfield_Paul_events[si] — invoke the matched
    // ui_element's func_ptr handler. `button` mirrors DOS's al (the [data_0dc35]
    // click-button byte the handler reads).
    fn dispatch_ui_click(&mut self, i: usize) {
        if let Some(handler) = self.ui_elements[i].handler {
            handler(self);
            return;
        }

        // = seg000:d8d4 call cs:[di+0xC] — dispatch by the element's raw seg000
        // func_ptr for the handlers ported so far.
        let func_ptr = self.ui_elements[i].func_ptr;
        eprintln!("unhandled ui_element[{i}] click handler: 0x{func_ptr:04x}");
    }

    // = seg000:1ae7 loc_01ae7 — the room-screen record's idle handler ([si] at
    // seg000:d88c). game_loop calls highlight_hovered_text_action_item just
    // before this on the idle path. DOS: if the active screen element is
    // menu_NPC_actions and the npc_menu_idle_timer (base/limit pair armed by
    // arm_npc_menu_idle_timer, seg000:c85b) has expired, drive the NPC
    // idle/glance animation (loc_0c868) and re-arm the timer.
    // TODO: port the loc_01ae7 NPC idle-glance body; no-op stub meanwhile.
    fn room_mouse_idle(&mut self) {}

    // = seg000:d8fe..d914 the left-button press dispatch. DOS runs the ui_element
    // hit-test HERE in the game loop, NOT inside the per-screen record handler, so
    // it is shared across every screen: si == [active_mouse_handlers] (the unbiased
    // LMB) takes this path. A matched HUD element is dispatched directly; only a
    // miss falls through to the game-area / person click and then the active
    // record's [si+2] press handler (room: fn_0d917_noop; mixer: mixer_panel_lmb /
    // loc_0a576, which closes the panel on an outside-panel click). This is why the
    // command strip stays clickable with the mixer open: the slot is a HUD element
    // here, dispatched before the mixer's exit-on-miss [si+2] ever runs.
    //
    // `button` is the [data_0dc35] click-button byte. game_loop lifts the software
    // cursor (= seg000:d8f4 call_restore_cursor) before this, so a press redraw
    // lands on clean background.
    pub(crate) fn game_loop_dispatch_lmb_press(&mut self) {
        // = seg000:d904 call hit_test_ui_elements; d907 jb
        //   loc_0d918.
        match self.hit_test_ui_elements() {
            // = seg000:d918 loc_0d918 — a ui_element was hit: arm it (if
            //   repeatable), latch the click time, and fire its handler. [si+2] is
            //   NOT run on a hit.
            Some(i) => self.ui_element_press(i),
            None => {
                // = seg000:d909 push si; d90a call callback_main_ui_element_21_22;
                //   d90d pop si — the game-area / person click. It self-guards on
                //   the room command menu being active, so it is a no-op while the
                //   mixer (or any other overlay) is up.
                self.callback_main_ui_element_21_22();
                // = seg000:d90e mov al,[mouse_button_state_prev]; d911 call [si+2]
                //   — the active record's press handler, run only on a miss.
                let lmb = self.active_mouse_handlers.lmb;
                lmb(self);
            }
        }
    }

    // = seg000:d917 fn_0d917_noop — the room-screen record's LMB press handler
    // ([si+2]), a no-op. The HUD element hit-test + dispatch (which the game loop
    // runs for every screen) lives in game_loop_dispatch_lmb_press
    // (seg000:d904..d941); the room record itself has nothing to do on a miss.
    fn room_mouse_lmb(&mut self) {}

    // = seg000:d918 loc_0d918 — finish a press that landed on HUD element `i`:
    // optionally arm it for held auto-repeat, latch the click time, and fire it.
    fn ui_element_press(&mut self, i: usize) {
        // = seg000:d918 mov [data_0dc60],di — record the pressed element.
        // = seg000:d91c call game_loop_sub_0d65a — press-feedback redraw: if the
        //   record carries flag 0x2000 it bumps the sprite one frame, redraws, and
        //   restores it. TODO: port the press-down sprite redraw (cosmetic).
        // = seg000:d91f test [di+9],40h — the 0x4000 flag marks a repeatable /
        //   draggable element (e.g. a +/- knob).
        if self.ui_elements[i].flags & 0x4000 != 0 {
            // = seg000:d925 mov [data_0dc5c],di; jmp loc_0d935 — arm it and skip the
            //   keyboard-latch clear so the arming press alone does not consume a
            //   queued Enter.
            self.drag_armed_element = Some(i);
            self.mouse_last_click_time = self.game_ticks() as u16;
            self.dispatch_ui_click(i);
        } else {
            // = seg000:d92b not repeatable: clear the keyboard latches, then fire.
            self.dispatch_element_with_latch(i);
        }
    }

    // = seg000:d92b..d941 loc_0d92b/loc_0d935 — clear the Enter/data_0ceba keyboard
    // latches so a queued key action does not also fire, snapshot the click time
    // (= seg000:d935), and dispatch the element's handler ([di+0ch]). Reused by the
    // release and held-auto-repeat paths in game_loop.
    pub(crate) fn dispatch_element_with_latch(&mut self, i: usize) {
        // = seg000:d92b mov [kb_keys_enter],0; d930 mov [data_0ceba],0.
        self.input.lock().unwrap().kb_keys[crate::input::SCANCODE_ENTER as usize] = 0;
        self.data_0ceba = 0;
        // = seg000:d935 mov [mouse_last_click_time], pit counter.
        self.mouse_last_click_time = self.game_ticks() as u16;
        // = seg000:d93e call word ptr [di+0ch].
        self.dispatch_ui_click(i);
    }

    // = seg000:1707 loc_01707 — advance/skip the on-screen dialogue line on a fresh
    // LMB press while data_04774 is set. DOS checks the active screen element rect
    // (ui_hud_elements[8]) and drives the lip-sync / subtitle advance (loc_09ed5).
    // TODO: port the dialogue-advance body once the dialogue runtime is modelled;
    // no-op stub meanwhile.
    pub(crate) fn dialogue_advance_on_click(&mut self) {}

    // = the room-screen record's RMB handler ([si+4] = fn_0d917_noop, a no-op):
    // the room view assigns no right-button action.
    fn room_mouse_rmb(&mut self) {}

    // = the room-screen record's LMB-release handler ([si+6] = fn_0d917_noop, a
    // no-op): the room view arms no drag target to clear on release.
    fn room_mouse_release(&mut self) {}

    // = the room-screen record's RMB-release handler ([si+8] = fn_0d917_noop, a
    // no-op): the room view arms no drag target to clear on the right-button up.
    fn room_mouse_rmb_release(&mut self) {}

    // = the room-screen record's LMB-drag handler ([si+0ah] = fn_0d917_noop, a
    // no-op): the room view has nothing to drag.
    fn room_mouse_drag(&mut self, _dx: i16, _dy: i16) {}

    // = the room-screen record's RMB-drag handler ([si+0ch] = fn_0d917_noop, a
    // no-op): the room view has nothing to drag with the right button either.
    fn room_mouse_rmb_drag(&mut self, _dx: i16, _dy: i16) {}
}
