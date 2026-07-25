//! The SEE DUNE MAP full-planet map view — the room verb (seg001:220c, handler
//! seg000:186b ui_toggle_room_view) that leaves the room screen for the whole-
//! planet map: the interpolated flat map (map_renderer.rs, segvga:1f4c
//! vga_draw_map_zoomed) in the full-map window (4,4)-(316,148), the vegetation
//! marks and location markers over it, the map main verb menu (EXIT MAPS /
//! CONTACT FREMEN TROOPS / SEE SPICE DENSITY / TAKE AN ORNITHOPTER / FIND
//! PROSPECTORS), and the first-visit "Map to command rallied troops" popup.
//!
//! This is the troop-command screen. The troop icons render through the
//! layered-icon system in troop_icons.rs (the "particle system" the night
//! attack scene reuses; see attack/mod.rs); clicking an icon selects the
//! troop (the rotating highlight ring), right-clicking toggles its info
//! panel, and clicking a location marker opens the location info popup with
//! its GO THERE command menu. The info/location panels scale in and out with
//! an XOR outline animation (xor_rect_outline_advance / _reverse). Still
//! stubbed: the contact verb menu + troop
//! dialogue (troop_0780a / troop_07c02), the GO THERE launch's CALL A WORM
//! branch, the water/spice popup extra (loc_0605c), popup dragging, and the
//! spice-density overlay (data_046eb bit 0x40).

use crate::{
    GameState, Rect,
    game_ui::{MouseHandlers, NAV_PANEL_ALT},
    gfx,
    locations::location_index_from_ptr,
    rect::rect,
    room_game_screen::{CommandMenuRecord, ScreenElement, rec},
};

/// = seg001:20f2 menu_map_main — the map main verb menu's compiled-in records
/// (the leading 0xff priority word lives in the MenuBuffer). The ids and grey
/// bits are rewritten by map_setup_main_menu before every push.
#[rustfmt::skip]
pub(crate) const MENU_MAP_MAIN: [CommandMenuRecord; 5] = [
    rec(0x0063, 0x186b), // EXIT MAPS -> ui_toggle_room_view
    rec(0x0062, 0x86cc), // CONTACT FREMEN TROOPS
    rec(0x0064, 0x53f1), // SEE SPICE DENSITY
    rec(0x00a7, 0x42d9), // TAKE AN ORNITHOPTER
    rec(0x0067, 0x5b1e), // FIND PROSPECTORS
];

// = seg001:1482 full_map_view_rect — the SEE DUNE MAP window (4,4)-(316,148),
// copied into data_046e3_rect (map_view_rect) by the open transition
// callback. vga_draw_map_zoomed fills exactly this window: 36 bands of 4
// rows, 312 px wide.
const FULL_MAP_VIEW_RECT: Rect = rect(4, 4, 316, 148);

/// = seg001:194a data_0194a — the rallied-troops title popup's panel record:
/// rect (10,10)-(190,64), frame colour 0xf5 (+8), fill colour 0xfb (+9). The
/// record's seg001 offset doubles as the popup identity in map_popup_ptr.
pub(crate) const MAP_POPUP_RALLIED: u16 = 0x194a;
pub(crate) const RALLIED_POPUP_RECT: Rect = rect(10, 10, 190, 64);

/// = seg001:18df data_018df — the troop info panel record: the rect is
/// written at open time (loc_05f25), frame colour 0xfb (+8), fill colour
/// 0xf0 (+9). The record's seg001 offset is the popup identity.
pub(crate) const MAP_POPUP_TROOP_INFO: u16 = 0x18df;

/// = seg001:1668 data_01668 — the location info panel record (frame 0xf8,
/// fill 0x10). The record's seg001 offset is the popup identity.
pub(crate) const MAP_POPUP_LOCATION: u16 = 0x1668;

/// The GO THERE menu variant map_click_location_marker folds in.
#[derive(Clone, Copy)]
enum MoveMenu {
    /// = seg001:20da menu_multiple_move_to_location_flying_an_orni.
    Orni,
    /// = seg001:20e6 menu_multiple_move_to_location_riding_a_worm.
    Worm,
}

/// = seg001:1a9e mouse_handlers_01a9e — the full-map view's MouseHandlers
/// record, installed by ui_main_view_map_interface. The release/drag pair
/// moves a dragged popup panel (not ported); rmb_release/rmb_drag are the
/// no-op loc_00f66.
pub(crate) static DUNE_MAP_MOUSE_HANDLERS: MouseHandlers = MouseHandlers {
    idle: GameState::dune_map_mouse_idle,
    lmb: GameState::dune_map_mouse_lmb,
    rmb: GameState::dune_map_mouse_rmb,
    release: GameState::dune_map_mouse_release,
    rmb_release: GameState::dune_map_mouse_noop,
    drag: GameState::dune_map_mouse_drag,
    rmb_drag: GameState::dune_map_mouse_drag_noop,
};

impl GameState {
    // = seg000:5a1a ui_show_globe_map_view — leave the room view and bring up
    // the full DUNE MAP view (the else-branch of ui_toggle_room_view).
    pub(crate) fn ui_show_globe_map_view(&mut self) {
        // = seg000:5a1a voice_subtitle_mode = 1.
        self.voice_subtitle_mode = 1;
        // = seg000:5a1f call ui_teardown_room_view.
        self.ui_teardown_room_view();
        // = seg000:5a22 call set_zoomed_globe_pos_from_map_position — centre
        //   the map on the player.
        self.set_zoomed_globe_pos_from_map_position();
        // = seg000:5a25 bp=callback_transition_05a56; al=34h; dx=0ffffh; call
        //   transition — build the map view offscreen and dissolve it in. (The
        //   dx direction byte is only read by the effect-4 curtain; 0x34
        //   ignores it, so the port's fixed dl=0 matches.)
        self.transition(0x34, |s| s.callback_transition_dune_map_view());
        // = seg000:5a30 cmp map_view_reentry_count,0; jnz — the rallied-troops
        //   popup only on the first visit.
        if self.map_view_reentry_count == 0 {
            self.map_show_rallied_troops_popup();
        }
        // = seg000:5a3a jmp ui_hud_head_animate_up.
        self.ui_hud_head_animate_up();
    }

    // = seg000:5a56 callback_transition_05a56 — the SEE DUNE MAP view builder,
    // run inside the transition (front buffer redirected to fb1).
    fn callback_transition_dune_map_view(&mut self) {
        // = seg000:5a56 cmp data_046eb,0; js — already in the map view: only
        //   redraw.
        if self.data_046eb & 0x80 != 0 {
            self.ui_main_view_map_interface();
            return;
        }
        // = seg000:5a5d call dismiss_stacked_overlays.
        self.dismiss_stacked_overlays();
        // = seg000:5a60 call loc_04aca — data_011ca = 1 (suspend the pending
        //   room-swap machinery while the map owns the screen).
        self.data_011ca = 1;
        // = seg000:5a63 call remove_globe_frame_tasks.
        self.remove_globe_frame_tasks();
        // = seg000:5a66..5a6c add troop_icon_anim_task (interval 15).
        self.arm_troop_icon_anim_task();
        // = seg000:5a6f..5a75 data_046e3_rect = full_map_view_rect.
        self.map_view_rect = FULL_MAP_VIEW_RECT;
        // = seg000:5a78 call draw_map_view_border.
        self.draw_map_view_border();
        // = seg000:5a7b call ui_hud_head_draw.
        self.ui_hud_head_draw();
        // = seg000:5a7e data_046eb = 0x80 — the full-map view owns the screen.
        self.data_046eb = 0x80;
        // = seg000:5a83 call loc_0ad5e — re-pick the background music for the
        //   new screen mode.
        self.update_room_music();
        // = seg000:5a86 troop_icon_draw_order_func = troop_icons_pick_next_
        //   by_depth — back-to-front icon layering down the map.
        self.troop_icon_draw_by_depth = true;
        // = seg000:5a8c..5a92 install ui_main_view_map_interface as the
        //   main-view drawing function and run it.
        self.current_main_view_drawing_function = Some(GameState::ui_main_view_map_interface);
        self.ui_main_view_map_interface();
        // = seg000:5a94 call ui_set_and_draw_frieze_sides_map.
        self.ui_set_and_draw_frieze_sides_map();
        // = seg000:5a97 jmp loc_0d712 — install and draw the alternate
        //   (map-scroll) nav panel.
        self.ui_install_nav_panel(&NAV_PANEL_ALT);
    }

    // = seg000:5a9a ui_main_view_map_interface — compose the full DUNE MAP
    // view into fb1. Installed as the main-view drawing function while the
    // view is up (map_refresh_main_view re-runs it after a scroll/recentre).
    pub(crate) fn ui_main_view_map_interface(&mut self) {
        // = seg000:5a9a call set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = seg000:5a9d call loc_05b8d — restore rect (data_d83c_rect) and
        //   sprite clip rect = the map window. The port passes clip rects per
        //   draw call (see map_view_clip_rect).
        // = seg000:5aa0..5aa6 force data_046eb = 0x80 around the draws,
        //   keeping the entry value (the 0x40 spice sub-mode restores below).
        let saved_046eb = std::mem::replace(&mut self.data_046eb, 0x80);
        // = seg000:5aa7 call map_draw_zoomed_globe — the interpolated full-
        //   planet map (MapRenderer).
        self.map_draw_zoomed_globe();
        // = seg000:5aaa call open_onmap_resource — the on-map marker sprites.
        self.open_onmap_spritesheet();
        // = seg000:5aad call map_build_and_draw_location_markers — the
        //   vegetation marks, then the location markers (ONMAP base 0x7a).
        self.map_build_and_draw_location_markers();
        // = seg000:5ab0 call map_draw_player_position_sprite.
        self.map_draw_player_position_sprite();
        // = seg000:5ab3 call copy_active_framebuffer_to_framebuffer_2 — the
        //   clean map snapshot the popup dismissals restore from.
        self.copy_active_framebuffer_to_framebuffer_2();
        // = seg000:5ab6 troop_icon_count = 0 (the focused slots are cleared
        //   with the list, port-side, so they cannot dangle); 5abc call
        //   map_spawn_troop_icons.
        self.troop_icons.clear();
        self.troop_icon_focused = [None; 2];
        self.map_spawn_troop_icons();
        // = seg000:5abf/5ac2 si = data_046e3_rect; call troop_icons_update_
        //   dirty_rect — draw the icons over the snapshot and present the
        //   map window.
        let r = self.map_view_rect;
        self.troop_icons_update_dirty_rect(r);
        // = seg000:5ac5 call map_setup_main_menu.
        self.map_setup_main_menu();
        // = seg000:5ac8/5ac9 restore the entry data_046eb.
        self.data_046eb = saved_046eb;
        // = seg000:5acc..5ad0 bit 0x40 re-enters the spice-density overlay
        //   (loc_05406) — not ported yet. TODO.
        if self.data_046eb & 0x40 != 0 {
            println!("ui_main_view_map_interface: spice-density overlay (loc_05406) not ported");
        }
        // = seg000:5ad3 install mouse_handlers_01a9e.
        self.active_mouse_handlers = &DUNE_MAP_MOUSE_HANDLERS;
        // = seg000:5ad9 nav rect = the map window.
        self.set_mouse_nav_rect(self.map_view_rect);
    }

    // = seg000:878c map_setup_main_menu — configure the map main verb menu's
    // ids and grey bits from the game state, push it onto the screen-element
    // stack (bx = nullsub_00f66, a no-op cleanup) and reopen ONMAP.
    fn map_setup_main_menu(&mut self) {
        // = seg000:878c dialogue_resume_entry_ptr = 0.
        self.dialogue_resume_entry_ptr = 0;
        // = seg000:8792..87bd the TAKE AN ORNITHOPTER id (0xa7): greyed
        //   (0x4000) unless a location scene is up (current_scene != 0xff and
        //   not a deep sietch room) and its available equipment has an orni.
        let mut orni_id = 0x40a7;
        if self.data_00008 != 0xff && (self.data_00008 < 0x20 || self.current_room < 3) {
            // = seg000:87aa..87bb compute_location_available_equipment; ax =
            //   0xa7, greyed when orni_count is 0.
            self.compute_location_available_equipment();
            orni_id = if self.available_equipment.ornithopters != 0 {
                0xa7
            } else {
                0x40a7
            };
        }
        // = seg000:87c0 bp = menu_map_main.
        // = seg000:87c3 [bp+0eh] = ax — the ornithopter entry's id.
        self.menu_map_main.records[3].text_id = orni_id;
        // = seg000:87c6 [bp+0bh] |= 0x40 — SEE SPICE DENSITY greyed;
        // = seg000:87ca [bp+12h] = 0 — FIND PROSPECTORS hidden.
        self.menu_map_main.records[2].text_id = 0x4064;
        self.menu_map_main.records[4].text_id = 0;
        // = seg000:87cf..87da game_phase >= 5 ungreys SEE SPICE DENSITY and
        //   shows FIND PROSPECTORS (id 0x67).
        if self.game_phase >= 5 {
            self.menu_map_main.records[2].text_id = 0x0064;
            self.menu_map_main.records[4].text_id = 0x0067;
        }
        // = seg000:87df..8813 the contact slot ([bp+6]).
        if self.location_visibility_distance < 2 {
            // = seg000:87e6 id 0x93, greyed unless the current location has a
            //   troop (the head of its troop chain resolves).
            let mut id = 0x4093;
            if let Some(location) = self.locations.get(self.current_location_index as usize) {
                // = seg000:87f3..87fd [di+9] != 0 and get_address_of_troop_by_ID.
                let troop_id = location.troop_id;
                if troop_id != 0 && self.troops.get((troop_id - 1) as usize).is_some() {
                    // = seg000:87ff and word ptr [bp+6],0bfffh.
                    id = 0x0093;
                }
            }
            self.menu_map_main.records[1].text_id = id;
        } else {
            // = seg000:8806..8813 CONTACT FREMEN TROOPS (0x62), greyed while
            //   no troop icons are on the map (troop_icon_count 0).
            self.menu_map_main.records[1].text_id = if self.troop_icons.is_empty() {
                0x4062
            } else {
                0x0062
            };
        }
        // = seg000:8816/8819 bx = nullsub_00f66; call screen_element_stack_push.
        self.screen_element_stack_push(ScreenElement::DuneMapScreen);
        // = seg000:881c jmp open_onmap_resource.
        self.open_onmap_spritesheet();
    }

    // = seg000:5bb0 map_show_rallied_troops_popup — draw the first-visit DUNE
    // MAP title popup straight to the visible screen: the panel record
    // data_0194a, then phrase 0xe2 ("DUNE MAP / * Map to command rallied
    // troops * / Number of rallied troops = N") with the live count patched
    // over the trailing digits.
    fn map_show_rallied_troops_popup(&mut self) {
        // = seg000:5bb0 call set_screen_as_active_framebuffer.
        self.set_screen_as_active_framebuffer();
        // = seg000:5bb3/5bb6 map_popup_ptr = data_0194a.
        self.map_popup_ptr = MAP_POPUP_RALLIED;
        self.map_draw_rallied_troops_popup();
        // = seg000:5be8 jmp set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // DOS drew straight to the visible A000 buffer; the port publishes the
        // touched screen.
        self.send_frame_to_display();
    }

    // = seg000:5bba..5be5 the popup's panel + text draw, onto the active
    // framebuffer. Split out so troop_icons_update_dirty_rect can re-draw the
    // panel over a repainted rect (the port's stand-in for DOS's screen-pixel
    // grab at loc_0c7d4).
    pub(crate) fn map_draw_rallied_troops_popup(&mut self) {
        // = seg000:5bba call loc_07b1b — fill the panel rect with its fill
        //   colour ([rec+9] = 0xfb), then outline it in the frame colour
        //   ([rec+8] = 0xf5) inset one pixel (loc_0c551 -> draw_rect_outline).
        let r = RALLIED_POPUP_RECT;
        gfx::vga_fill_rect(
            self,
            self.active_fb(),
            r.x0 as u16,
            r.y0 as u16,
            r.x1 as u16,
            r.y1 as u16,
            0xfb,
        );
        self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, 0xf5);
        // = seg000:5bbd call font_select_tall_font.
        self.font_select_tall_font();
        // = seg000:5bc0/5bc3 the phrase 0xe2 text.
        let mut text = self.get_phrase_or_command_string(0xe2).to_vec();
        // = seg000:5bc6..5bce find_last_numeric_digit_in_str_at_es_si +
        //   string_replace_number_ending_at_es_si — write the decimal
        //   number_of_rallied_troops backwards over the digit tail (the
        //   template's leading spaces stay for short counts).
        if let Some(last) = text.iter().rposition(|c| c.is_ascii_digit()) {
            let mut n = self.number_of_rallied_troops as u16;
            let mut i = last;
            loop {
                text[i] = b'0' + (n % 10) as u8;
                n /= 10;
                if n == 0 || i == 0 {
                    break;
                }
                i -= 1;
            }
        }
        // = seg000:5bd1..5be5 the pen: the panel origin + (10, 8); colour
        //   word 0x00f0 (fg 0xf0 on transparent bg).
        self.font_state.color = 0x00f0;
        self.font_set_draw_position(r.x0 as u16 + 10, r.y0 as u16 + 8);
        self.font_draw_string(&text);
    }

    // = seg000:5beb map_dismiss_rallied_troops_popup — when the open popup is
    // the rallied-troops title panel, close it and repaint the map beneath.
    pub(crate) fn map_dismiss_rallied_troops_popup(&mut self) {
        // = seg000:5beb cmp map_popup_ptr,194ah; jnz ret.
        if self.map_popup_ptr != MAP_POPUP_RALLIED {
            return;
        }
        // = seg000:5bf6/5bf8 xor si,si; xchg si,[map_popup_ptr].
        self.map_popup_ptr = 0;
        // = seg000:5bfc call troop_icons_update_dirty_rect — repaint the map
        //   beneath the panel rect from the fb2 snapshot.
        self.troop_icons_update_dirty_rect(RALLIED_POPUP_RECT);
    }

    // = seg000:6314 map_draw_player_position_sprite — draw the "you are here"
    // ICONES sprite 0x4c at the player's projected map position, anchored
    // 13 px left and one sprite height up from the point.
    pub(crate) fn map_draw_player_position_sprite(&mut self) {
        // = seg000:6314/6317 get_map_position; map_position_to_screen_if_
        //   visible; jb ret.
        let (x, lat) = self.get_map_position();
        let Some((sx, sy)) = self.map_position_to_screen_if_visible(x, lat) else {
            return;
        };
        let clip = self.map_view_clip_rect();
        let yoff = self.y_offset as i16;
        // = seg000:6324..632c push the active bank and open ICONES.
        let prev = self.open_icones_spritesheet();
        // = seg000:631a ax = 0x4c; 631e dx -= 13; 6330 bl -= the sprite
        //   height; 6334 call draw_sprite_clipped_clobbering_bx_dx.
        self.with_active_bank_sheet(|s, sheet| {
            if let Some(sprite) = sheet.get_sprite(0x4c) {
                let h = sprite.height() as i16;
                s.draw_sprite_from_sheet_clipped(sheet, 0x4c, sx - 13, sy - h + yoff, clip);
            }
        });
        // = seg000:6337/6338 restore the previous bank.
        self.open_sprite_bank(prev as i16);
    }

    // = seg000:633b map_draw_vegetation_marks — draw the vegetation tufts over
    // the full map view, one band at a time from the top of the window until a
    // band projects outside it. Run by map_build_and_draw_location_markers in
    // full-map mode before the location markers; the active bank is ONMAP.
    pub(crate) fn map_draw_vegetation_marks(&mut self) {
        // = seg000:633b dx = longitude; bx = latitude - 0x12 (the top band).
        let lng = self.zoomed_globe_longitude;
        let mut lat = self.zoomed_globe_latitude - 0x12;
        // = seg000:6346 loop while map_position_to_screen_if_visible clears CF.
        while let Some((sx, sy)) = self.map_position_to_screen_if_visible(lng, lat) {
            self.map_draw_vegetation_marks_row(lng, lat, sx, sy);
            lat += 1;
        }
    }

    // = seg000:634d map_draw_vegetation_marks_row — one band's marks: walk the
    // map row east (loc_0636a) then west (loc_0639a) from the projected centre
    // column, 4 screen px per map cell, wrapping around the row ends.
    fn map_draw_vegetation_marks_row(&mut self, lng: u16, lat: i16, sx: i16, sy: i16) {
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        let y = (lat + 98) as u16;
        let row_off = tablat.offset(y) as usize;
        let row_len = tablat.len(y) as usize;
        if row_len == 0 {
            return;
        }
        // = seg000:636e/639e map_func — the centre cell for (lng, lat).
        let cell = ((row_len as u32 * lng as u32) >> 16) as usize;
        let (wx0, wx1) = (self.map_view_rect.x0, self.map_view_rect.x1);

        // = seg000:636a map_draw_vegetation_marks_east — from the centre
        //   column to the window's right edge.
        let mut x = sx;
        let mut i = cell;
        loop {
            self.map_draw_vegetation_mark(row_off, row_len, i, x, sy);
            // = seg000:637f add dx,4; cmp dx,[data_046e3_rect.x1]; jnb ret.
            x += 4;
            if x >= wx1 {
                break;
            }
            // = seg000:6388..6390 the cell step, wrapping at the row end.
            i = (i + 1) % row_len;
        }

        // = seg000:639a map_draw_vegetation_marks_west — from the centre
        //   column to the window's left edge.
        let mut x = sx;
        let mut i = cell;
        loop {
            self.map_draw_vegetation_mark(row_off, row_len, i, x, sy);
            // = seg000:63af sub dx,4; cmp dx,[data_046e3_rect.x0]; jb ret.
            x -= 4;
            if x < wx0 {
                break;
            }
            // = seg000:63b8..63be the cell step, wrapping at the row start.
            i = (i + row_len - 1) % row_len;
        }
    }

    // = seg000:6375/63a5 the per-cell test + seg000:63c7 map_draw_vegetation_
    // mark_sprite — on a vegetation cell (map byte & 0x30 == 0x10) stamp a
    // tuft: ONMAP sprite 0x79 when the next cell east is also vegetation,
    // else 0x78, jittered 0..3 px in x and y from the cell's map offset so
    // the tufts do not grid-align.
    fn map_draw_vegetation_mark(
        &mut self,
        row_off: usize,
        row_len: usize,
        i: usize,
        x: i16,
        y: i16,
    ) {
        // = seg000:6375 ax = the cell pair; and ax,3030h; cmp al,10h; jnz.
        let cell = self.map[row_off + i];
        if cell & 0x30 != 0x10 {
            return;
        }
        let next = self.map[row_off + (i + 1) % row_len];
        // = seg000:63cd..63d5 sprite 0x79 when the neighbour matches, else 0x78.
        let sprite = if next & 0x30 == 0x10 { 0x79 } else { 0x78 };
        // = seg000:63d6..63e4 the jitter: di = the map byte offset, bp = the
        //   row length; y += di & 3, x += ((bp + di) >> 2) & 3.
        let di = row_off + i;
        let jy = (di & 3) as i16;
        let jx = (((row_len + di) >> 2) & 3) as i16;
        let clip = self.map_view_clip_rect();
        let yoff = self.y_offset as i16;
        // = seg000:63e6 call loc_0c343 — centred + clipped.
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_sprite_centered_clipped(sheet, sprite, x + jx, y + jy + yoff, clip);
        });
    }

    // = seg000:5c03 map_main_mouse_idle — the full-map view's idle/hover
    // handler (mouse_handlers_01a9e).
    pub(crate) fn dune_map_mouse_idle(&mut self) {
        // = seg000:5c03..5c17 auto-dismiss the rallied-troops popup 1000 PIT
        //   ticks (~5 s) after game_clock_tick_base — the game loop re-stamps
        //   that on every mouse-button edge (seg000:d893), so the budget runs
        //   from the click that opened the view.
        if self.map_popup_ptr == MAP_POPUP_RALLIED
            && (self.game_ticks() as u16).wrapping_sub(self.game_clock_tick_base) >= 1000
        {
            // = seg000:5c19 call_restore_cursor; 5c1c the dismissal; 5c1f
            //   jmp draw_mouse.
            self.call_restore_cursor();
            self.map_dismiss_rallied_troops_popup();
            self.draw_mouse();
        }
        // = seg000:5c22..5c75 the marker hover label + the occupation-panel
        //   readout — both gated on the troop occupation panel (data_04710)
        //   being open, which is not ported yet. TODO.
    }

    // = seg000:5c76 map_main_mouse_lmb — the full-map view's LMB handler.
    pub(crate) fn dune_map_mouse_lmb(&mut self) {
        // = seg000:5c76 call map_dismiss_rallied_troops_popup.
        self.map_dismiss_rallied_troops_popup();
        // = seg000:5c79 call open_onmap_resource.
        self.open_onmap_spritesheet();
        let x = self.mouse_pos_x as i16;
        let y = self.mouse_pos_y as i16;
        // = seg000:5c7c..5ca2 a click inside the open popup panel routes to the
        //   panel, not a dismiss: the troop occupation panel (data_04710 ->
        //   loc_05923) or the equipment spinners (loc_07e97/loc_07eb8), both
        //   stubbed, so an inside-click is a no-op here.
        if let Some(r) = self.map_open_popup_rect() {
            if r.in_rect(x, y) {
                return;
            }
        }
        // = seg000:5ca5 cmp data_046f5,0 — with the spice-density overlay up
        //   any other click exits the menu. Not ported (no overlay yet).
        // = seg000:5caf call loc_06946 (the icon hit-test); jb troop_0872c.
        if let Some((_, ti)) = self.troop_icon_hit_test(x, y) {
            self.map_click_troop_icon(ti);
            return;
        }
        // = seg000:5cb7 al=31h; call find_nearest_location_marker — a click
        //   within 20 px of a location marker (appearance < 0x31) opens the
        //   location troop popup (loc_05fb0).
        let (marker, dist) = self.find_nearest_location_marker(0x31, x, y);
        if dist < 0x14 && marker != 0 {
            let li = location_index_from_ptr(marker);
            // = seg000:5cc1 cmp di,[data_046f8]; jz — re-clicking the open
            //   location's marker is a no-op (its popup is already up).
            if self.map_location_popup_loc != Some(li) {
                self.map_click_location_marker(li);
            }
            return;
        }
        // = seg000:5cca..5cd0 a click on empty map space closes the open
        //   popups: the location popup menu (loc_05f79), the troop info panel
        //   (loc_079de) and the spice sub-mode (loc_058fa, stubbed).
        self.map_close_location_troop_popup();
        self.map_close_troop_info_popup();
        // = seg000:5cd3..5ce0 a live troop-contact strip (data_01954) tears
        //   down through the no-more-orders path — TODO with the contact
        //   menu.
    }

    // = the open popup panel's rect (seg000:5c7c/5c95 rect_contains against
    // map_popup_ptr / map_popup2_ptr): the rallied-troops title panel, the
    // troop info panel or the location info panel. None when no popup is up.
    fn map_open_popup_rect(&self) -> Option<Rect> {
        match self.map_popup_ptr {
            MAP_POPUP_RALLIED => Some(RALLIED_POPUP_RECT),
            MAP_POPUP_TROOP_INFO => Some(self.map_info_panel_rect),
            MAP_POPUP_LOCATION => Some(self.map_location_popup_rect),
            _ => None,
        }
    }

    // = seg000:5ce4 map_main_mouse_rmb — the full-map view's RMB handler:
    // toggle the troop info panel.
    pub(crate) fn dune_map_mouse_rmb(&mut self) {
        // = seg000:5ce4 call map_dismiss_rallied_troops_popup.
        self.map_dismiss_rallied_troops_popup();
        // = seg000:5ce7 call open_onmap_resource.
        self.open_onmap_spritesheet();
        // = seg000:5cea..5cef a secondary popup open blocks the toggle.
        if self.map_popup2_ptr != 0 {
            return;
        }
        let x = self.mouse_pos_x as i16;
        let y = self.mouse_pos_y as i16;
        // = seg000:5cf1..5d02 with a popup open: only the info panel is
        //   toggleable — a click inside it closes it; any other open popup
        //   blocks; a click outside falls through to the icon hit-test.
        if self.map_popup_ptr != 0 {
            if self.map_popup_ptr != MAP_POPUP_TROOP_INFO {
                return;
            }
            if self.map_info_panel_rect.in_rect(x, y) {
                // = seg000:5d02 jb loc_05d1a -> loc_079de.
                self.map_close_troop_info_popup();
                return;
            }
        }
        // = seg000:5d04 call loc_06946; jnb ret.
        let Some((icon, ti)) = self.troop_icon_hit_test(x, y) else {
            return;
        };
        // = seg000:5d0c cmp si,[data_046fa]; jz loc_05d1a — the same troop's
        //   panel toggles closed.
        if self.map_info_popup_troop == Some(ti) {
            self.map_close_troop_info_popup();
            return;
        }
        // = seg000:5d12/5d17 close the other troop popups, then open
        //   (troop_078bc).
        self.map_dismiss_troop_popups();
        self.map_open_troop_info_popup(icon, ti);
    }

    // = seg000:872c troop_0872c — an LMB click on a troop icon: select the
    // troop (data_01954) and open the contact UI over it; a click on the
    // already-selected troop goes straight to the troop dialogue.
    fn map_click_troop_icon(&mut self, ti: usize) {
        // = seg000:872c si = [si+0ah]; 872f al = [si] — the troop id.
        let id = self.troops[ti].troop_id;
        // = seg000:8731..873f while location_visibility_distance < 2, a troop
        //   at the current location is handled in the room, not here.
        if self.location_visibility_distance < 2 {
            let li = location_index_from_ptr(self.troops[ti].offset_of_location);
            if li == self.current_location_index as usize {
                return;
            }
        }
        // = seg000:8741..874d the already-selected troop goes straight to the
        //   dialogue (troop_07c02). TODO with the contact menu.
        if id == self.map_selected_troop_id {
            println!("map_click_troop_icon: the troop dialogue (troop_07c02) is not ported");
            return;
        }
        // = seg000:8747 data_01954 = al; 874a jmp loc_08685.
        self.map_selected_troop_id = id;
        self.map_select_troop();
    }

    // = seg000:8685 loc_08685 — (re)build the selected-troop UI: tear the old
    // contact UI down, spawn the highlight ring over the selection, then open
    // the contact verb menu and the troop dialogue.
    fn map_select_troop(&mut self) {
        // = seg000:8685 data_046d8 = 1 — suppress the info panel's outline
        //   scale-out below (the selection replaces it immediately).
        self.map_popup_anim_suppress = true;
        // = seg000:868a call loc_069a3 — remove the old highlight ring.
        self.map_remove_focused_troop_icon();
        // = seg000:868d call loc_07b58 — tear down the contact dialogue strip
        //   (data_046ef). TODO with the contact dialogue.
        // = seg000:8690/8693/8696 close the location popup menu (loc_05f79,
        //   not ported), the info panel (loc_079de) and the spice sub-mode
        //   (loc_058fa, not ported).
        self.map_close_troop_info_popup();
        // = seg000:8699..86a3 a valid selected id resolves its troop.
        let id = self.map_selected_troop_id;
        if id == 0 || id > 0x43 || self.troops.get((id - 1) as usize).is_none() {
            return;
        }
        let ti = (id - 1) as usize;
        // = seg000:86a5 data_01955 = al — the confirmed id; not modelled.
        // = seg000:86a9 call troop_0697c — the highlight ring.
        self.map_focus_troop_icon(ti);
        // = seg000:86ae call troop_0780a — the contact verb menu;
        // = seg000:86b2/86b5 di = the troop's location; call troop_07c02 —
        //   the troop dialogue. TODO: both flows.
        println!(
            "map_select_troop: the contact menu (troop_0780a) and dialogue (troop_07c02) are not ported"
        );
    }

    // = seg000:69a3 loc_069a3 — remove the selected-troop highlight ring
    // (troop_icon_focused_ptr slot 0).
    pub(crate) fn map_remove_focused_troop_icon(&mut self) {
        // = seg000:69a4..69ae xor di,di; xchg di,[troop_icon_focused_ptr];
        //   call troop_icon_remove.
        if let Some(i) = self.troop_icon_focused[0].take() {
            self.troop_icon_remove(i);
        }
    }

    // = seg000:697c troop_0697c — spawn the highlight ring (the rotating
    // FOCUS_RING_SCRIPT icon, flag 0x40 so it draws last and never hit-tests)
    // over the selected troop's icon and store it in the focused slot.
    fn map_focus_troop_icon(&mut self, ti: usize) {
        // = seg000:697c call troop_leaf_fn_06917; jnz ret — the troop must
        //   have a plain icon on the map.
        if self.troop_find_icon(ti).is_none() {
            return;
        }
        // = seg000:6981..698d a stationed troop's location must have a
        //   visible marker (location_leaf_fn_05ed0).
        let t = &self.troops[ti];
        let li = location_index_from_ptr(t.offset_of_location);
        let marker = self
            .visible_location_markers
            .iter()
            .find(|m| m.location_index as usize == li)
            .copied();
        if t.occupation & 0x40 == 0 && marker.is_none() {
            return;
        }
        // = seg000:698f call troop_icon_screen_pos; jb ret.
        let Some((x, y)) = self.troop_icon_screen_pos(ti, marker.as_ref()) else {
            return;
        };
        // = seg000:6994/6997 bp = data_018fd; call troop_icon_spawn_with_anim.
        let Some(i) =
            self.troop_icon_spawn_with_anim(crate::troop_icons::FOCUS_RING_SCRIPT, x, y, ti)
        else {
            return;
        };
        // = seg000:699a flag 0x40; 699e troop_icon_focused_ptr = the icon.
        self.troop_icons[i].flags |= 0x40;
        self.troop_icon_focused[0] = Some(i);
    }

    // = seg000:7c63 troop_07c63 — the Chebyshev distance in map cells between
    // the player and the troop's gps position (the troop twin of
    // location_distance_from_player).
    fn troop_distance_from_player(&self, ti: usize) -> u16 {
        // = seg000:7c64 call get_map_position; 7c68..7c70 bp = the
        //   longitude-units-per-cell for the player's |latitude|.
        let (px, plat) = self.get_map_position();
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        let per_cell = tablat.lng_units_per_cell((plat + 98) as u16);
        let t = &self.troops[ti];
        // = seg000:7c74..7c8e |gps_1 - x| / bp vs |gps_2 - lat|, the larger.
        let dx = t.gps_coordinates_1.abs_diff(px) / per_cell;
        let dy = (t.gps_coordinates_2 as i16).abs_diff(plat);
        dx.max(dy)
    }

    // = seg000:78bc troop_078bc — open the troop info panel next to the
    // troop's icon.
    fn map_open_troop_info_popup(&mut self, icon_index: usize, ti: usize) {
        // = seg000:78bf..78c6 out of visibility range shows no panel.
        if self.troop_distance_from_player(ti) >= self.location_visibility_distance {
            return;
        }
        // = seg000:78c8 troop_leaf_fn_06917 — the hit-test already resolved
        //   the icon. = seg000:78cd draw to the front buffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:78d0 data_046fa = the troop.
        self.map_info_popup_troop = Some(ti);
        // = seg000:78d5..78e0 si = data_018df; the icon's top-left in dx/bx;
        //   cx = 0x64; call loc_05f25 — place + draw the empty panel.
        let (ix, iy) = {
            let r = self.troop_icons[icon_index].rect;
            (r.x0, r.y0)
        };
        // = data_018df: fill 0xf0 (+9), frame 0xfb (+8).
        self.map_info_panel_rect =
            self.map_place_popup_panel(MAP_POPUP_TROOP_INFO, ix, iy, 0x64, 0xf0, 0xfb);
        // = seg000:78e4/78e6 data_01955 = the id (not modelled); falls into
        //   loc_078e9 — the panel content.
        self.map_draw_troop_info_panel_content(ti);
        // = seg000:79db jmp set_fb1_as_active_framebuffer; the port restores
        //   the bracket and publishes the touched screen.
        self.active_fb = saved;
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:5f25 loc_05f25 — place a popup panel record next to an icon /
    // marker at (x, y): 106 px wide, `h` tall, vertically centred and clamped
    // into the map window; on the right at x+15 unless that crosses x 210,
    // then 130 px to the left. Registers the panel as map_popup_ptr, draws
    // the comm-glow pointer arrow (not modelled) and the panel fill + frame,
    // and returns the rect. `popup_id`/`fill`/`frame` come from the record
    // (data_018df for the troop info panel, data_01668 for the location one).
    fn map_place_popup_panel(
        &mut self,
        popup_id: u16,
        x: i16,
        y: i16,
        h: i16,
        fill: u8,
        frame: u8,
    ) -> Rect {
        // = seg000:5f29..5f42 the vertical clamp [4, 0x94 - h].
        let py = (y - h / 2).clamp(4, 0x94 - h);
        // = seg000:5f42..5f4f the side pick.
        let mut px = x + 0x0f;
        if px >= 0xd2 {
            px -= 0x82;
        }
        // = seg000:5f4f..5f5f the record rect (width 0x6a) + map_popup_ptr.
        let r = rect(px, py, px + 0x6a, py + h);
        self.map_popup_ptr = popup_id;
        // = seg000:5f65..5f76 store the source point (the icon / marker
        //   position, less 10) and the panel rect for the outline animation,
        //   clear the suppress flag (data_046d8 = 0), then run the outline
        //   scale-in (effect al=6, xor_rect_outline_advance).
        self.map_popup_anim_src = (x, y);
        self.map_popup_anim_rect = r;
        self.map_popup_anim_suppress = false;
        self.animate_popup_outline(false);
        // = seg000:7b1b loc_07b1b — the panel fill + frame (fill [rec+9],
        //   frame [rec+8]).
        gfx::vga_fill_rect(
            self,
            self.active_fb(),
            r.x0 as u16,
            r.y0 as u16,
            r.x1 as u16,
            r.y1 as u16,
            fill,
        );
        self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, frame);
        r
    }

    // = segvga:38d8 xor_rect_outline_advance / segvga:39bb _reverse (effects
    // al=6 / al=8) — the panel's XOR outline scale animation: a rectangle
    // outline that grows from the source point (map_popup_anim_src, less 2) to
    // the panel rect on open, or shrinks back to it on close. 15 frames, each
    // drawing the outline, pacing one frame, then XOR-erasing it. The close
    // repaints the map under the panel first (the caller), so the shrinking
    // outline plays over the clean map.
    fn animate_popup_outline(&mut self, reverse: bool) {
        // The animation is a foreground timing effect; headless runs skip it.
        if self.is_headless() {
            return;
        }
        // = segvga:38d8/38e0 the start corner: the source point + 8 - 10 = -2.
        let (sx, sy) = self.map_popup_anim_src;
        let start_x = sx - 2;
        let start_y = sy - 2;
        let r = self.map_popup_anim_rect;
        let pw = r.x1 - r.x0;
        let ph = r.y1 - r.y0;
        // = segvga:3920/3925 the size steps (panel extent / 16).
        let wstep = pw >> 4;
        let hstep = ph >> 4;
        // = segvga:392a..395d the top-left steps: |panel - start| / 16, signed.
        let dxstep = ((r.x0 - start_x).abs() >> 4) * (r.x0 - start_x).signum();
        let dystep = ((r.y0 - start_y).abs() >> 4) * (r.y0 - start_y).signum();
        // = segvga:3962/396c the advance starts at the source, size 0; the
        //   reverse (segvga:39bb) starts at the panel rect, size = its extent,
        //   with every step negated.
        let (mut cx, mut cy, mut cw, mut ch, dx, dy, dw, dh) = if reverse {
            (r.x0, r.y0, pw, ph, -dxstep, -dystep, -wstep, -hstep)
        } else {
            (start_x, start_y, 0, 0, dxstep, dystep, wstep, hstep)
        };
        // = segvga:397a..39b7 the 15-frame draw / pace / erase loop. DOS XORs
        //   straight onto the visible screen, so the erase is seen at once; the
        //   port presents after the draw, so publish once more after the final
        //   erase to clear the last outline (otherwise it lingers — the open
        //   path's panel fill re-presents over it, but the close leaves it).
        for _ in 0..15 {
            cx += dx;
            cy += dy;
            cw += dw;
            ch += dh;
            self.xor_rect_outline(cx, cy, cw, ch);
            self.present_transition_frame();
            self.xor_rect_outline(cx, cy, cw, ch);
        }
        if reverse {
            self.send_frame_to_display();
        }
    }

    // = segvga:3733 vga_xor_rect_outline_inner — XOR the four edges of the
    // rect at (x, y) size (w, h) into the visible screen with colour 0x0f,
    // clamped to the map area [4, 0x13c] x [4, 0x94]. XOR-drawing the same
    // rect twice erases it.
    fn xor_rect_outline(&mut self, x: i16, y: i16, w: i16, h: i16) {
        let x0 = x.clamp(4, 0x13c);
        let x1 = (x + w).clamp(4, 0x13c);
        let y0 = y.clamp(4, 0x94);
        let y1 = (y + h).clamp(4, 0x94);
        if x1 < x0 || y1 < y0 {
            return;
        }
        let yoff = self.y_offset;
        let scr = &mut self.screen;
        let mut toggle = |px: i16, py: i16| {
            let px = px as u16;
            let py = (py + yoff as i16) as u16;
            scr.set(px, py, scr.get(px, py) ^ 0x0f);
        };
        // = segvga:3784/378f/379d/37a8 the top, right, bottom, left edges.
        for px in x0..=x1 {
            toggle(px, y0);
            toggle(px, y1);
        }
        for py in y0 + 1..y1 {
            toggle(x0, py);
            toggle(x1, py);
        }
    }

    // = an interpolated COMMAND/PHRASE string (the live-number placeholders
    // read the staged CONDIT block) at a pen with a colour word — the DOS
    // font_draw_interpolated_string_w_color_at_pos.
    fn map_draw_interp_string(&mut self, id: u16, color: u16, x: u16, y: u16) {
        let s = self.get_phrase_or_command_string(id).to_vec();
        let text = self.format_interpolated_string(&s);
        self.font_state.color = color;
        self.font_set_draw_position(x, y);
        self.font_draw_string(&text);
    }

    // = seg000:78e9 loc_078e9 — the troop info panel's content: the header,
    // the location name, the occupation/status lines and the equipment row,
    // all from the staged CONDIT block.
    fn map_draw_troop_info_panel_content(&mut self, ti: usize) {
        // = seg000:78e9/78ee a troop that lost its icon closes the panel.
        if self.troop_find_icon(ti).is_none() {
            self.map_close_troop_info_popup();
            return;
        }
        // = seg000:78f4 call troop_prepare_troop_data_for_condit; 78f7
        //   subst_id_04 += 0xc — the occupation caption's display variant
        //   (the subst staging is TODO in troop_prepare_troop_data_for_condit).
        self.troop_prepare_troop_data_for_condit(ti);
        // = seg000:78fc/78ff the panel fill + frame again (the refresh entry).
        let r = self.map_info_panel_rect;
        gfx::vga_fill_rect(
            self,
            self.active_fb(),
            r.x0 as u16,
            r.y0 as u16,
            r.x1 as u16,
            r.y1 as u16,
            0xf0,
        );
        self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, 0xfb);
        // = seg000:7902 the small font; 7905..7916 the header pen (x0+12,
        //   y0+4), colour 0x9a on the panel fill 0xf0 (ch = [data_018e8]).
        self.font_select_small_font();
        let header_x = (r.x0 + 12) as u16;
        let mut y = (r.y0 + 4) as u16;
        // = seg000:7919..7924 the header: string 0x3a, 0x3b for a moving
        //   troop (occupation bit 6). Only the header draws at x0+12.
        let occ = self.troops[ti].occupation;
        let hdr = if occ & 0x40 != 0 { 0x3b } else { 0x3a };
        self.map_draw_interp_string(hdr, 0xf09a, header_x, y);
        // = seg000:7929 sub dx,8 — the pen drops to x0+4 for the location
        //   name AND stays there for every following line.
        let x0 = header_x - 8;
        // = seg000:7927..7933 the location name at (x0+4, y+9), colour 0x96.
        y += 9;
        let li = location_index_from_ptr(self.troops[ti].offset_of_location);
        self.draw_location_name(li, 0xf096, x0, y);
        // = seg000:7936..7938 back to 0x9a; y += 10.
        y += 10;
        if occ & 0x20 != 0 {
            // = seg000:793e..794a occupation bit 5: string 0x41, 0x42 for
            //   occupation 0x22.
            let id = if occ == 0x22 { 0x42 } else { 0x41 };
            self.map_draw_interp_string(id, 0xf09a, x0, y);
            y += 0x11;
        } else {
            // = seg000:794c..794f the spice-rates string 0x3c
            //   ("Average: .. kgs/h / Current: .. kgs/h").
            self.map_draw_interp_string(0x3c, 0xf09a, x0, y);
            y += 0x0f;
            // = seg000:7955 occupation 2 skips the caption + status lines.
            if occ != 2 {
                // = seg000:795c..796b the occupation caption: the phrase at
                //   string_subst_id_table[6 + ((occ & 0xf) >> 2)]
                //   (seg001:11f7 = the table + 12).
                let idx = 6 + (((occ & 0x0f) >> 2) & 3) as usize;
                let phrase = self.string_subst_id_table[idx];
                self.font_draw_phrase_or_command_string_with_color_at_pos(phrase, 0xf09a, x0, y);
                y += 10;
                // = seg000:7971..79b9 the status line (stationed troops only).
                if occ & 0x40 == 0 {
                    let bf = self.troops[ti].bitfield_10;
                    let dissat = self.troops[ti].dissatisfaction_and_speech;
                    // = seg000:7978..79b4 the status pick. The DOS 0x100-clear
                    //   and dissatisfaction branches both land on 0x40.
                    let id = if bf & 0x200 != 0 {
                        0x3f
                    } else if bf & 0x100 == 0 || dissat & 0x30 != 0 {
                        0x40
                    } else if occ == 0 {
                        0x3d
                    } else if occ & 0x0f == 1 {
                        0x43
                    } else if occ == 6 {
                        0x3e
                    } else {
                        0
                    };
                    if id != 0 {
                        self.map_draw_interp_string(id, 0xf09a, x0, y);
                        y += 0x11;
                    }
                }
            }
        }
        // = seg000:79bc..79c7 the "Equipment:" header, colour 0x96.
        y += 4;
        self.font_draw_phrase_or_command_string_with_color_at_pos(0x6e, 0xf096, x0, y);
        y += 8;
        // = seg000:79ca..79d8 the equipment icon row: troop_unpack_equipment_
        //   flags (bitmask -> 0/1 per type) into the row, bottom = panel y1.
        let mask = self.troops[ti].equipment;
        let flags = std::array::from_fn(|slot| u8::from(mask & (0x80 >> slot) != 0));
        let bottom = self.map_info_panel_rect.y1;
        self.map_draw_equipment_columns(&flags, bottom, x0 as i16, y as i16);
    }

    // = seg000:7e3d loc_07e3d — the equipment row: `counts` is 7 per-type
    // ONMAP icon counts (the seg001:192f sprite table, harvesters..bulbs).
    // Each nonzero type stacks `count` icons vertically within [y, bottom]
    // (loc_061d3) and advances x by the icon width; an all-zero row draws the
    // "none" phrase (0x69). The troop info panel passes 0/1 flags
    // (troop_unpack_equipment_flags); the location popup passes real counts.
    fn map_draw_equipment_columns(&mut self, counts: &[u8; 7], bottom: i16, x0: i16, y: i16) {
        // = seg000:7e4f..7e64 nothing owned: the "none" phrase 12 px in
        //   (add dx,0ch; add bx,5) in the current colour.
        if counts.iter().all(|&c| c == 0) {
            self.font_set_draw_position((x0 + 12) as u16, (y + 5) as u16);
            self.font_draw_phrase_or_command_string(0x69);
            return;
        }
        let clip = self.map_view_clip_rect();
        let yoff = self.y_offset as i16;
        let mut x = x0;
        // = seg000:7e6b..7e93 one column per nonzero type.
        for (slot, &count) in counts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let sprite = crate::troop_icons::equipment_icon_sprite(slot);
            // = seg000:61d3 loc_061d3 — read the sprite dims.
            let (mut w, mut sh) = (0i16, 0i16);
            self.with_active_bank_sheet(|_, sheet| {
                if let Some(sp) = sheet.get_sprite(sprite) {
                    w = sp.width() as i16;
                    sh = sp.height() as i16;
                }
            });
            if w == 0 {
                continue;
            }
            // = seg000:61e2..620d the vertical spacing: fit `count` icons in
            //   the available height, squeezing (min step 2) only if they do
            //   not fit at the natural step of sprite_height + 2.
            let avail = bottom - y;
            let step_full = sh + 2;
            let n = count as i16;
            let (mut draw_n, step) = if avail / n >= step_full {
                (n, step_full)
            } else {
                let s = ((avail - step_full).max(0) / n).max(2);
                if s > 2 { (n, s) } else { (avail / 2, 2) }
            };
            draw_n = draw_n.max(1).min(n);
            // = seg000:6211..6220 stack the icons.
            let mut iy = y;
            for _ in 0..draw_n {
                self.with_active_bank_sheet(|s, sheet| {
                    s.draw_sprite_from_sheet_clipped(sheet, sprite, x, iy + yoff, clip);
                });
                iy += step;
            }
            // = seg000:6224..622d advance x by the icon width.
            x += w;
        }
    }

    // = seg000:79de loc_079de — close the troop info panel: clear data_046fa
    // and map_popup_ptr, repaint the map beneath the panel rect.
    pub(crate) fn map_close_troop_info_popup(&mut self) {
        // = seg000:79e0..79e6 xchg data_046fa,0; jz -> just fb1 active.
        if self.map_info_popup_troop.take().is_none() {
            return;
        }
        // = seg000:79e8/79eb si = data_018df; loc_05f9f: fb1 active, clear
        //   map_popup_ptr, repaint the map under the panel, then the outline
        //   scale-out (loc_07b2b, effect al=8) unless suppressed.
        self.set_fb1_as_active_framebuffer();
        self.map_popup_ptr = 0;
        let r = self.map_info_panel_rect;
        self.troop_icons_update_dirty_rect(r);
        if !self.map_popup_anim_suppress {
            self.animate_popup_outline(true);
        }
    }

    // = seg000:5fb0 loc_05fb0 — an LMB click near a location marker: open the
    // location info popup, and (unless it is the player's own location or an
    // in-room/desert case that cannot travel) push a GO THERE command menu.
    fn map_click_location_marker(&mut self, li: usize) {
        // = seg000:5fb0 call loc_058fa — leave the spice-density sub-mode
        //   (not ported). = seg000:5fb3 call map_dismiss_troop_popups.
        self.map_dismiss_troop_popups();
        // = seg000:5fb6..6008 decide the GO THERE menu.
        let menu = if li == self.current_location_index as usize {
            // = seg000:5fba the player's own location: panel only.
            None
        } else if self.data_00008 == 0xff {
            // = seg000:5fbc..5ffe the desert: GO THERE RIDING A WORM, but only
            //   once the worm is available (bitfield_Paul_events bit 0x40).
            if self.bitfield_paul_events & 0x40 != 0 {
                Some(MoveMenu::Worm)
            } else {
                None
            }
        } else if self.current_room <= 2 || self.data_00008 < 0x20 {
            // = seg000:5fc3..5fd8 GO THERE FLYING AN ORNI, but only from a
            //   shallow room: current_room <= 2, or the room's appearance is
            //   below 0x20. Deep inside a location (room > 2 AND appearance
            //   >= 0x20) shows the panel with no menu (seg000:5fca/5fcf jnb;
            //   the 5fd1 appearance >= 0x28 compare is subsumed by it). The
            //   verb greys without an ornithopter at the current location.
            self.compute_location_available_equipment();
            Some(MoveMenu::Orni)
        } else {
            None
        };
        // = seg000:6004 call loc_0600e — draw the panel.
        self.map_draw_location_popup(li);
        // = seg000:6008/600b bx = loc_05f91; jmp loc_0d323 — push the GO
        //   THERE menu over the panel (its cleanup closes the panel).
        if let Some(menu) = menu {
            let orni_greyed = self.available_equipment.ornithopters == 0;
            self.map_move_menu.records = match menu {
                // = seg000:5fe1..5ff4 GO THERE FLYING AN ORNI (id 0x59, greyed
                //   0x4000 without an orni) + Cancel.
                MoveMenu::Orni => vec![
                    rec(if orni_greyed { 0x4059 } else { 0x0059 }, 0x50db),
                    rec(0x00a3, 0xd2e2),
                ],
                // = seg000:6000 GO THERE RIDING A WORM (id 0x5a) + Cancel.
                MoveMenu::Worm => vec![rec(0x005a, 0x50ea), rec(0x00a3, 0xd2e2)],
            };
            self.screen_overlay_request_transition();
            self.screen_element_stack_push(ScreenElement::MoveToLocationMenu);
            self.play_pending_panel_fold();
        }
    }

    // = seg000:0600e loc_0600e — draw the location info popup: place the
    // panel next to the marker (location_05ee4), the location type + name,
    // then the class-specific extras and the equipment/battle section.
    fn map_draw_location_popup(&mut self, li: usize) {
        // = seg000:600e call set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:6011..6016 location_05ee4 places the panel and sets
        //   data_046f8 = the location.
        let placed = self.map_place_location_panel(li);
        self.map_location_popup_loc = Some(li);
        if !placed {
            self.active_fb = saved;
            return;
        }
        let r = self.map_location_popup_rect;
        // = seg000:601a..6034 the location type header (0x9a) at (x0+12, y0+4)
        //   and the name (0x96) at (x0+4, +9).
        self.font_select_tall_font();
        let hx = (r.x0 + 12) as u16;
        let ty = (r.y0 + 4) as u16;
        self.draw_string_location_type(li, 0xf09a, hx, ty);
        self.draw_location_name(li, 0xf096, hx - 8, ty + 9);
        // = seg000:603f..6056 the class dispatch: class 2 (Atreides /
        //   undiscovered / plain) shows only the header; class 0 with the
        //   Paul-events 0x20 water flag draws the water/spice extra first.
        let class = self.location_class(li);
        if class != 2 {
            if self.bitfield_paul_events & 0x20 != 0 && class == 0 {
                // = seg000:6052 call location_0605c — the water/spice line;
                //   niche late-game state, not ported. TODO.
                println!("map_draw_location_popup: water/spice extra (loc_0605c) not ported");
            }
            // = seg000:6056 call loc_060ac — the equipment/battle section.
            self.map_draw_location_equipment_or_battle(li);
        }
        // = seg000:6059 jmp set_fb1_as_active_framebuffer.
        self.active_fb = saved;
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:5ee4 location_05ee4 — place the location panel next to the
    // location's visible marker, its height keyed on the class
    // (troop_icon_panel_heights). Returns false when the location has no
    // visible marker.
    fn map_place_location_panel(&mut self, li: usize) -> bool {
        // = seg000:5ee4 call location_find_visible_marker; jnz ret.
        let Some(m) = self
            .visible_location_markers
            .iter()
            .find(|m| m.location_index as usize == li)
            .copied()
        else {
            return false;
        };
        // = seg000:5eec/5ef1 the class + its panel height [class+11d0h].
        let class = self.location_class(li);
        self.map_location_popup_class = class + 1;
        const HEIGHTS: [i16; 4] = [0x58, 0x3c, 0x1e, 0];
        let h = HEIGHTS[(class as usize).min(3)];
        // = seg000:5f1b..5f23 the marker position; bh sign-bit off-window bail.
        if m.y < 0 {
            return false;
        }
        // = data_01668: fill 0x10 (+9), frame 0xf8 (+8).
        self.map_location_popup_rect =
            self.map_place_popup_panel(MAP_POPUP_LOCATION, m.x, m.y, h, 0x10, 0xf8);
        true
    }

    // = seg000:60ac loc_060ac — the location panel's equipment-or-battle
    // section: a "Battle:" gauge when the location has active combat
    // (location_has_battle), else the "Equipment:" header + the location's
    // own equipment column row.
    fn map_draw_location_equipment_or_battle(&mut self, li: usize) {
        self.open_onmap_spritesheet();
        self.font_select_tall_font();
        let r = self.map_location_popup_rect;
        let x0 = (r.x0 + 4) as u16;
        // = seg000:60b8 bx += 0xc — the section sits a header-height below the
        //   name; the port derives the pen from the panel top.
        let y = (r.y0 + 4 + 9 + 0xc) as u16;
        if !self.location_has_battle(li) {
            // = seg000:60c3..60d3 "Equipment:" (0x6e, colour 0x9a) then the
            //   location's own equipment counts (record +0x14), bottom = y1.
            self.font_draw_phrase_or_command_string_with_color_at_pos(0x6e, 0xf09a, x0, y);
            let e = &self.locations[li].equipment;
            let counts = [
                e.harvesters,
                e.ornithopters,
                e.krys_knives,
                e.laser_guns,
                e.weirding_modules,
                e.atomics,
                e.bulbs,
            ];
            self.map_draw_equipment_columns(&counts, r.y1, x0 as i16, (y + 0x0a) as i16);
        } else {
            // = seg000:60d6..60f5 "Battle:" (0x4c) then the battle gauge
            //   sprite (0x8e + (gauge + 0xf) >> 5) at (x0+0x2f, y+6).
            self.font_draw_phrase_or_command_string_with_color_at_pos(0x4c, 0xf09a, x0, y);
            let gauge = self.location_battle_gauge(li);
            let sprite = 0x8e + ((gauge as u16 + 0x0f) >> 5);
            let clip = self.map_view_clip_rect();
            let yoff = self.y_offset as i16;
            self.with_active_bank_sheet(|s, sheet| {
                s.draw_sprite_from_sheet_clipped(
                    sheet,
                    sprite,
                    x0 as i16 + 0x2f,
                    y as i16 + 6 + yoff,
                    clip,
                );
            });
        }
    }

    // = seg000:6252 location_06252 — the location's popup class (0 full,
    // 1 battle, 2 header-only). Battle present -> 1; Atreides -> 2; else keyed
    // on the location-type string (type 3 -> 0, type 2 -> 1, else 2);
    // undiscovered (status bit 4 clear) or no type -> 2.
    fn location_class(&mut self, li: usize) -> u8 {
        // = seg000:6252 call location_0627e; jb -> class 1.
        if self.location_has_battle(li) {
            return 1;
        }
        // = seg000:6257 Atreides -> class 2.
        if self.location_is_atreides(li) {
            return 2;
        }
        // = seg000:6260 status bit 4 (discovered) clear -> class 2.
        if self.locations[li].status & 0x10 == 0 {
            return 2;
        }
        // = seg000:6266..6278 keyed on the location-type string offset.
        match self.get_location_type_string_offset(li) {
            0 => 2,
            3 => 0,
            2 => 1,
            _ => 2,
        }
    }

    // = seg000:627e location_0627e — the location has active combat: status
    // bit 1 (a battle site), or a non-Atreides location with attacking /
    // occupation-6 troops (location_do_accumulation_on_troops).
    fn location_has_battle(&mut self, li: usize) -> bool {
        // = seg000:6281 test status,2; jnz -> battle.
        if self.locations[li].status & 2 != 0 {
            return true;
        }
        // = seg000:6287 Atreides -> no battle.
        if self.location_is_atreides(li) {
            return false;
        }
        // = seg000:628c location_do_accumulation_on_troops (05098): dx counts
        //   the occupation-6 non-Harkonnen troops.
        let mut dx = 0u16;
        self.for_each_troop_in_location(li, |s, ti| {
            let t = &s.troops[ti];
            // = seg000:5082 callback: skip occupation bit 5; Harkonnen
            //   (bitfield_10 bit 7) goes to cx; occupation 6 -> dx.
            if t.occupation & 0x20 == 0 && t.bitfield_10 & 0x80 == 0 && t.occupation == 6 {
                dx += 1;
            }
        });
        dx != 0
    }

    // = seg000:60f8 location_060f8 — the location's battle gauge (0..0xff):
    // the population-weighted balance of the two sides' skill, biased toward
    // 0x80 (even). Accumulates over the location's troops (06155).
    fn location_battle_gauge(&mut self, li: usize) -> u8 {
        // = seg000:60fe/6118 data_0d81c = the Harkonnen population sum.
        let mut hark_pop = 0u32;
        // = seg000:6155 bx = Σ field_e, cx = Σ field_c, dx = Σ population
        //   (occupation-6 non-0x20 troops); Harkonnen add to hark_pop.
        let (mut sum_e, mut sum_c, mut attacker_pop) = (0u32, 0u32, 0u32);
        self.for_each_troop_in_location(li, |s, ti| {
            let t = &s.troops[ti];
            let pop = t.population as u32;
            if t.bitfield_10 & 0x80 != 0 {
                hark_pop += pop;
                return;
            }
            if t.occupation == 6 {
                if t.occupation & 0x20 == 0 {
                    attacker_pop += pop;
                }
                sum_c += t.field_c as u32;
                sum_e += t.field_e as u32;
            }
        });
        // = seg000:6108..6114 bx = Σfield_e / (Σfield_e's pop? ) — the DOS
        //   `add bx,dx; div bx` averages field_e over the attacker pop.
        let avg_e = (sum_e << 8).checked_div(sum_e + attacker_pop).unwrap_or(0) as u16;
        // = seg000:6116..6126 cx = Σfield_c / (Σfield_c + hark_pop).
        let avg_c = (sum_c << 8).checked_div(sum_c + hark_pop).unwrap_or(0) as u16;
        // = seg000:6128..6143 the balance: si = max(avg_e, avg_c); the signed
        //   difference over si, halved and biased +0x80.
        let hi = avg_e.max(avg_c) as i32;
        let diff = avg_e as i32 - avg_c as i32;
        let bal = if hi != 0 { (diff * 0x80 / hi) >> 1 } else { 0 };
        (bal + 0x80).clamp(0, 0xff) as u8
    }

    // = seg000:5f91 loc_05f91 — the GO THERE menu's cleanup: clear the
    // location popup gate (data_046f8/046f7) and close the panel.
    pub(crate) fn map_close_location_popup(&mut self) {
        // = seg000:5f91/5f97 data_046f8 = 0; data_046f7 = 0.
        if self.map_location_popup_loc.take().is_none() {
            return;
        }
        self.map_location_popup_class = 0;
        // = seg000:5f9c si = data_01668; loc_05f9f: fb1 active, clear
        //   map_popup_ptr, repaint the map under the panel, then the outline
        //   scale-out (loc_07b2b, effect al=8) unless suppressed.
        self.set_fb1_as_active_framebuffer();
        self.map_popup_ptr = 0;
        let r = self.map_location_popup_rect;
        self.troop_icons_update_dirty_rect(r);
        if !self.map_popup_anim_suppress {
            self.animate_popup_outline(true);
        }
    }

    // = seg000:50db menu_callback_choice_move_to_location_orni — GO THERE
    // FLYING AN ORNI: set ornithopter travel and confirm it.
    pub(crate) fn menu_callback_choice_move_to_location_orni(&mut self) {
        // = seg000:50db travel_vehicle_mode = 2; map_ornithopter_mode = 1.
        self.travel_vehicle_mode = 2;
        self.map_ornithopter_mode = 1;
        // = seg000:50e6 al = 4 (the orni mode flag); fall into the shared
        //   confirm tail.
        self.map_move_to_location_confirm(4);
    }

    // = seg000:50ea menu_callback_choice_move_to_location_worm — GO THERE
    // RIDING A WORM: the worm setup (loc_04285, CALL A WORM) is not ported.
    pub(crate) fn menu_callback_choice_move_to_location_worm(&mut self) {
        println!("GO THERE RIDING A WORM: the worm setup (loc_04285) is not ported");
    }

    // = seg000:50ef map_move_to_location_confirm — the shared GO THERE tail:
    // aim the pending travel at the popup's location, commit the room move
    // that boards the vehicle, tear the map down and confirm the travel
    // (map_confirm_travel_and_close).
    fn map_move_to_location_confirm(&mut self, mode_flag: u8) {
        // = seg000:50ef di = data_046f8 — the destination location.
        let Some(li) = self.map_location_popup_loc else {
            return;
        };
        let destination = crate::locations::location_ptr(li as u16);
        // = seg000:50f5 call dismiss_stacked_overlays; 50f8 travel_reset_trail.
        self.dismiss_stacked_overlays();
        self.travel_reset_trail();
        // = seg000:50fb/50ff dx = location_and_room, bx = location_appearance —
        //   the room the player is standing in, read after the overlays are
        //   dismissed.
        let mut new_room = self.location_and_room;
        let new_appearance = self.location_appearance;
        // = seg000:5105..5109 cmp al,4; jnz loc_0510b; mov dl,1 — the orni
        //   mode boards from the location's outdoor room 1 (the pad), so the
        //   move commits room 1 whatever room the verb was used from. The worm
        //   mode keeps the current room.
        if mode_flag == 4 {
            new_room = (new_room & 0xff00) | 1;
        }
        // = seg000:510b call callback_transition_04057 — commit the room move.
        //   data_046eb still carries the map's bit 7 here, so this only records
        //   the destination globals; the room redraw is the ui_toggle_room_view
        //   below.
        self.commit_room_move(new_room, new_appearance);
        // = seg000:510e call ui_toggle_room_view — back to the room view (the
        //   0x34 transition renders the just-committed room).
        self.ui_toggle_room_view();
        // = seg000:5111/5112 game_screen_mode_flags = al — set after the
        //   toggle, so the nav panel above still installs from the pre-travel
        //   flags.
        self.game_screen_mode_flags = mode_flag;
        // = seg000:5115/5116 pop di; jmp map_confirm_travel_and_close. A
        //   location destination ignores the x/y args (arm_pending_travel aims
        //   at the location ptr).
        self.map_confirm_travel_and_close(destination, 0, 0);
    }

    // = seg000:599f map_main_mouse_release — end a popup panel drag
    // (data_04723). Panel dragging is not ported yet.
    pub(crate) fn dune_map_mouse_release(&mut self) {}

    // = seg000:59c1 map_main_mouse_drag — move a dragged popup panel. Panel
    // dragging is not ported yet.
    pub(crate) fn dune_map_mouse_drag(&mut self, _dx: i16, _dy: i16) {}

    // = seg000:0f66 nullsub_00f66 — the rmb_release slot.
    pub(crate) fn dune_map_mouse_noop(&mut self) {}

    // = seg000:0f66 nullsub_00f66 — the rmb_drag slot.
    pub(crate) fn dune_map_mouse_drag_noop(&mut self, _dx: i16, _dy: i16) {}
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{GameState, dat_file::DatFile, room_game_screen::ScreenElement};

    // SEE DUNE MAP from the room screen: the full-planet map renders into the
    // (4,4)-(316,148) window, the map main menu becomes the active screen
    // element, and EXIT MAPS returns to the room verb menu. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored see_dune_map
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn see_dune_map_renders_and_exits() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // Stage troop icons: at the raw game start every troop still carries
        // occupation bit 7 (unrallied — "Number of rallied troops = 0"), and
        // those only show the 0x181f flag icon once their location's status
        // has bit 4. Give troop 1 (at location 12) a plain occupation so the
        // basic animated icon spawns, and flag troop 2's location (13).
        game.troops[0].occupation = 0x01;
        game.locations[13].status |= 0x10;

        // The SEE DUNE MAP verb (record seg001:220c, handler 0x186b).
        game.dispatch_command_handler(0x186b, 0x98);
        while rx.try_recv().is_ok() {}

        assert_eq!(game.data_046eb, 0x80, "full-map mode owns the screen");
        assert_eq!(
            game.get_active_screen_element(),
            ScreenElement::DuneMapScreen,
            "the map main menu is the active element"
        );
        assert_eq!(game.menu_map_main.records[0].text_id, 0x63, "EXIT MAPS");
        // The visible location markers at the game start carry stationed
        // troops, so the troop icon renderer spawns icons for them.
        eprintln!("troop icons spawned: {}", game.troop_icons.len());
        for ic in &game.troop_icons {
            eprintln!(
                "  icon sprite {:#04x} rect ({},{})-({},{}) flags {:#04x}",
                ic.sprite, ic.rect.x0, ic.rect.y0, ic.rect.x1, ic.rect.y1, ic.flags
            );
        }
        assert!(!game.troop_icons.is_empty(), "troop icons on the map");
        // With location_visibility_distance still 1 the contact slot is GIVE
        // ORDERS TO TROOP (0x93), greyed because the current location (index
        // 0 at the start) has no troop chain.
        assert_eq!(
            game.menu_map_main.records[1].text_id, 0x4093,
            "GIVE ORDERS TO TROOP greyed with no troop at the current location"
        );
        game.framebuffer
            .write_png(&game.palette, "troop_map_screen.png")
            .unwrap();
        game.screen
            .write_png(&game.palette, "troop_map_screen_popup.png")
            .unwrap();
        // The map window carries the map palette bank (0x10..0x20) everywhere
        // but the black polar-edge corners and the overlaid markers; probe the
        // equator row's ends and a mid-latitude column off the player marker.
        for (x, y) in [(4u16, 76u16), (315, 76), (80, 40), (240, 110)] {
            let p = game.framebuffer.get(x, y);
            assert!(
                (0x10..0x20).contains(&p),
                "map pixel at ({x},{y}) = {p:#04x} outside the map bank"
            );
        }

        // The rallied-troops popup stays up across idle passes until 1000
        // ticks after game_clock_tick_base (stamped by the room present and
        // by every button edge in the live loop).
        assert_eq!(game.map_popup_ptr, super::MAP_POPUP_RALLIED, "popup open");
        game.dune_map_mouse_idle();
        assert_eq!(
            game.map_popup_ptr,
            super::MAP_POPUP_RALLIED,
            "popup survives an early idle pass"
        );
        // Simulate the timeout by rewinding the stamp: the next idle pass
        // dismisses the popup and restores the map beneath it.
        game.game_clock_tick_base = (game.game_ticks() as u16).wrapping_sub(1000);
        game.dune_map_mouse_idle();
        assert_eq!(
            game.map_popup_ptr, 0,
            "popup auto-dismissed after 1000 ticks"
        );

        // The troop icon animation task (armed by the open): every 4th firing
        // steps the armed icons' scripts — the worker icon cycles its 4
        // sprites and repaints through the fb2 restore.
        let animated = game
            .troop_icons
            .iter()
            .position(|ic| ic.flags & 1 != 0)
            .expect("an animated troop icon");
        let mut seen = std::collections::HashSet::new();
        for _ in 0..16 {
            game.tick_troop_icon_anim();
            let s = game.troop_icons[animated].sprite;
            assert!(
                (0x17..=0x1a).contains(&s),
                "the worker icon stays in its 4-sprite cycle (got {s:#04x})"
            );
            seen.insert(s);
        }
        assert!(seen.len() >= 2, "the animated icon cycled through sprites");
        while rx.try_recv().is_ok() {}

        // The troop popups: give troop 1 a gps position at the player so the
        // info panel's visibility gate passes.
        let (px, plat) = game.get_map_position();
        game.troops[0].gps_coordinates_1 = px;
        game.troops[0].gps_coordinates_2 = plat as u16;
        // RMB over the unrallied troop's flag icon reports as a miss —
        // nothing opens.
        game.mouse_pos_x = 207;
        game.mouse_pos_y = 29;
        game.dune_map_mouse_rmb();
        assert_eq!(game.map_popup_ptr, 0, "no popup for an unrallied troop");
        // RMB over the rallied worker icon opens the troop info panel.
        game.mouse_pos_x = 241;
        game.mouse_pos_y = 145;
        game.dune_map_mouse_rmb();
        assert_eq!(
            game.map_popup_ptr,
            super::MAP_POPUP_TROOP_INFO,
            "the troop info panel is open"
        );
        assert_eq!(game.map_info_popup_troop, Some(0));
        while rx.try_recv().is_ok() {}
        game.screen
            .write_png(&game.palette, "troop_map_screen_info.png")
            .unwrap();
        // A second RMB on the same icon toggles the panel closed.
        game.dune_map_mouse_rmb();
        assert_eq!(game.map_popup_ptr, 0, "the info panel toggled closed");
        assert_eq!(game.map_info_popup_troop, None);
        // LMB on the icon selects the troop: data_01954 + the rotating
        // highlight ring in the focused slot (flag 0x40, on top).
        game.dune_map_mouse_lmb();
        assert_eq!(game.map_selected_troop_id, 1, "troop 1 selected");
        let ring = game.troop_icon_focused[0].expect("the highlight ring icon");
        assert!(game.troop_icons[ring].flags & 0x40 != 0, "ring draws last");
        // The anim task steps the focused ring every firing (not just every
        // 4th), painting it onto the screen.
        game.tick_troop_icon_anim();
        while rx.try_recv().is_ok() {}
        game.screen
            .write_png(&game.palette, "troop_map_screen_selected.png")
            .unwrap();

        // Click a location marker (loc_05fb0). Location 11's marker has no
        // troop icon over it (its troop is unrallied), so the icon hit-test
        // does not intercept. From a SHALLOW room (current_room <= 2) the
        // popup folds in the GO THERE FLYING AN ORNI menu.
        let marker = game
            .visible_location_markers
            .iter()
            .find(|m| m.location_index == 11)
            .copied()
            .expect("location 11's marker visible");
        game.current_room = 2;
        game.mouse_pos_x = marker.x as u16;
        game.mouse_pos_y = marker.y as u16;
        game.dune_map_mouse_lmb();
        assert_eq!(
            game.map_location_popup_loc,
            Some(11),
            "the location popup is open"
        );
        assert_eq!(game.map_popup_ptr, super::MAP_POPUP_LOCATION);
        assert_eq!(
            game.get_active_screen_element(),
            ScreenElement::MoveToLocationMenu,
            "the GO THERE menu is folded in from a shallow room"
        );
        assert_eq!(
            game.map_move_menu.records[0].text_id & 0xff,
            0x59,
            "GO THERE FLYING AN ORNI"
        );
        while rx.try_recv().is_ok() {}
        game.screen
            .write_png(&game.palette, "troop_map_screen_location.png")
            .unwrap();
        // Cancel (the menu's second verb) pops the menu; its cleanup closes
        // the info panel.
        game.dispatch_command_handler(0xd2e2, 0xa3);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.map_location_popup_loc, None, "location popup closed");
        assert_eq!(game.map_popup_ptr, 0);
        assert_eq!(
            game.get_active_screen_element(),
            ScreenElement::DuneMapScreen,
            "back to the map main menu"
        );

        // Deep inside a location (current_room > 2 AND appearance >= 0x20 —
        // the palace case) the same click shows the panel with NO GO THERE
        // menu: an ornithopter is not reachable from deep rooms.
        game.current_room = 0x0a; // data_00008 is already 0x20
        game.dune_map_mouse_lmb();
        assert_eq!(game.map_location_popup_loc, Some(11), "the panel is open");
        assert_eq!(game.map_popup_ptr, super::MAP_POPUP_LOCATION);
        assert_eq!(
            game.get_active_screen_element(),
            ScreenElement::DuneMapScreen,
            "no GO THERE menu deep inside a location"
        );
        while rx.try_recv().is_ok() {}

        // A click INSIDE the open panel does not dismiss it (it routes to the
        // panel, whose controls are stubbed).
        let pr = game.map_location_popup_rect;
        game.mouse_pos_x = ((pr.x0 + pr.x1) / 2) as u16;
        game.mouse_pos_y = ((pr.y0 + pr.y1) / 2) as u16;
        game.dune_map_mouse_lmb();
        assert_eq!(
            game.map_location_popup_loc,
            Some(11),
            "an inside-panel click keeps the popup open"
        );
        // A click on empty map space (>= 20 px from every marker, off the icons
        // and outside the panel) dismisses the popup.
        let empty = (4..316i16)
            .flat_map(|x| (4..148i16).map(move |y| (x, y)))
            .find(|&(x, y)| {
                !pr.in_rect(x, y)
                    && game.troop_icon_hit_test(x, y).is_none()
                    && game
                        .visible_location_markers
                        .iter()
                        .all(|m| (m.x - x).abs() + (m.y - y).abs() >= 20)
            })
            .expect("an empty map point");
        game.mouse_pos_x = empty.0 as u16;
        game.mouse_pos_y = empty.1 as u16;
        game.dune_map_mouse_lmb();
        assert_eq!(
            game.map_location_popup_loc, None,
            "a click on empty map space dismisses the popup"
        );
        assert_eq!(game.map_popup_ptr, 0);
        while rx.try_recv().is_ok() {}

        // Scroll one step north (the alt nav panel's up arrow, live only
        // while the LMB is down): the shared zoomed centre moves 12 latitude
        // rows and the redraw presents the shifted map from the fb2 snapshot.
        let lat_before = game.zoomed_globe_latitude;
        game.prev_mouse_buttons = 1;
        game.ui_click_map_up();
        game.prev_mouse_buttons = 0;
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.zoomed_globe_latitude,
            lat_before - 12,
            "scrolled north"
        );

        // The panel outline scale animation (xor_rect_outline_advance): the
        // XOR outline draw self-erases (a second XOR restores the pixels), the
        // property the draw/pace/erase loop depends on. Probe a top-edge pixel
        // (the outline draws in y_offset space, like the panel fill).
        let probe = (60u16, 30 + game.y_offset);
        let before = game.screen.get(probe.0, probe.1);
        game.xor_rect_outline(50, 30, 40, 30);
        assert_ne!(
            game.screen.get(probe.0, probe.1),
            before,
            "the outline toggled the border pixel"
        );
        game.xor_rect_outline(50, 30, 40, 30);
        assert_eq!(
            game.screen.get(probe.0, probe.1),
            before,
            "a second XOR erases the outline"
        );
        // Overlay three grow frames (near the marker, mid, at the panel rect)
        // to visualise the scale from the marker point to the info-panel rect.
        let (sx, sy) = (241i16, 145i16);
        let panel = crate::rect::rect(226, 116, 296, 148);
        for t in [3i16, 8, 14] {
            let cx = (sx - 2) + (panel.x0 - (sx - 2)) * t / 15;
            let cy = (sy - 2) + (panel.y0 - (sy - 2)) * t / 15;
            let cw = (panel.x1 - panel.x0) * t / 15;
            let ch = (panel.y1 - panel.y0) * t / 15;
            game.xor_rect_outline(cx, cy, cw, ch);
        }
        game.screen
            .write_png(&game.palette, "troop_map_screen_outline.png")
            .unwrap();

        // EXIT MAPS (menu_map_main record 0) back to the room.
        game.dispatch_command_handler(0x186b, 0x63);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.data_046eb, 0, "back in the plain room view");
        assert_eq!(
            game.get_active_screen_element(),
            ScreenElement::RoomCommandMenu,
            "the room verb menu is active again"
        );
    }

    // GO THERE FLYING AN ORNI boards from the location's outdoor room 1 (the
    // ornithopter pad), whatever inner room the map was opened from:
    // map_move_to_location_confirm forces dl = 1 into location_and_room and
    // commits the move (seg000:5105..510b) before ui_toggle_room_view redraws
    // the room behind the closing map. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored go_there_orni
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn go_there_orni_boards_from_the_outdoor_room() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // Walk up into an inner palace room (the equipment room is room 6 of
        // the starting location's appearance group); the pad is room 1.
        let inner_room = (game.location_and_room & 0xff00) | 6;
        game.commit_room_move(inner_room, game.location_appearance);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.current_room, 6, "standing in an inner room");

        // SEE DUNE MAP, then GO THERE FLYING AN ORNI on a location popup.
        game.dispatch_command_handler(0x186b, 0x98);
        game.map_location_popup_loc = Some(13);
        game.dispatch_command_handler(0x50db, 0);
        while rx.try_recv().is_ok() {}

        // location_and_room already carries the first travel step's desert
        // longitude by now (seg000:4760 travel_advance_step); current_room /
        // previous_room are only written by the commit, so they record which
        // room the flight departed from.
        assert_eq!(
            game.current_room, 1,
            "the departure commits the outdoor room 1, not the room the map was opened from"
        );
        assert_eq!(game.previous_room, 6, "the inner room became previous_room");
        // The confirm chain folded the map mode bit into the travel bit and
        // armed the travel pump.
        assert_eq!(game.game_screen_mode_flags & 3, 1, "orni travel mode");
        assert_eq!(game.travel_active, 0xff, "the travel pump is armed");
    }
}
