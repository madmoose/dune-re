//! The map main view — the windowed desert map the TAKE AN ORNITHOPTER
//! (seg000:42e9) and CALL A WORM (seg000:42d1) verbs open over the room screen.
//! The view draws a one-cell-per-pixel window of MAP.HSQ centred on the
//! player's map position inside data_046e3_rect, with curved globe edges at
//! extreme latitudes; in ornithopter mode the ORNYPAN.HSQ cockpit frames the
//! window. The mouse handlers (marker/compass-ray hover, the destination
//! click) and the travel-confirm chain (map_confirm_travel_and_close: the
//! pending-travel arm, the ornithopter takeoff, the close back to the room)
//! are live; the flight itself (the game loop's travel pump) and the
//! full-globe (data_046eb bit 0x80) drawing are not ported yet.

use crate::{
    FbId, GameState, Rect, TaskId,
    game_ui::{MouseHandlers, NAV_PANEL_ALT},
    gfx,
    locations::{location_index_from_ptr, location_ptr},
    menu_defs::{self, MenuItem, MenuRef},
    rect::rect,
    room_game_screen::RoomPerson,
    sprite_bank,
};

// = seg001:14b4 icon_list_ornypan_cockpit — the full ORNYPAN.HSQ ornithopter
// cockpit interior. The final entry doubles as ORNYPAN_WINDOW_OVERLAY_ICONS
// (DOS overlaps the two lists; the 0xffff terminator at seg001:14c6 ends both).
#[rustfmt::skip]
const ORNYPAN_COCKPIT_ICONS: [(u16, i16, i16); 3] = [
    (0x0000,  0, 19),
    (0x0001, 10, 43),
    (0x0002, 79, 45),
];

// = seg001:14c0 icon_list_ornypan_window_overlay — just the cockpit window
// frame (sprite 2), redrawn over the freshly drawn map window on every
// map_view_redraw pass.
#[rustfmt::skip]
const ORNYPAN_WINDOW_OVERLAY_ICONS: [(u16, i16, i16); 1] = [
    (0x0002, 79, 45)
];

// = seg001:149c map_view_rect_template — the map window rect, copied into
// data_046e3_rect (map_view_rect) when the map screen opens (seg000:432e).
const MAP_VIEW_RECT_TEMPLATE: Rect = rect(81, 45, 241, 134);

// = seg001:145e map_scroll_delta_up .. seg001:146a map_scroll_delta_left —
// the (delta longitude, delta latitude) word pairs ui_click_map_up/right/
// down/left point si at: a vertical step is 12 latitude rows, a horizontal
// step is 0x1002 longitude units (~1/16 of the 0x10000 circle).
const MAP_SCROLL_DELTA_UP: (u16, i16) = (0, -0x0c);
const MAP_SCROLL_DELTA_RIGHT: (u16, i16) = (0x1002, 0);
const MAP_SCROLL_DELTA_DOWN: (u16, i16) = (0, 0x0c);
const MAP_SCROLL_DELTA_LEFT: (u16, i16) = (0x1002u16.wrapping_neg(), 0);

// = seg001:196d table_196d — the cockpit fly-over silhouette sprite id per
// location SAL tier (calc_sal_index 0..4): SIET 0x11, PALACE 0x10, VILG 0x12,
// HARK 0x13/0x13. Indexed by travel_flyover_detect after the xlat at
// seg000:424a.
const TABLE_196D: [u8; 5] = [0x11, 0x10, 0x12, 0x13, 0x13];

// = seg001:148a travel_minimap_rect — the flight minimap view rect, copied
// into data_046e3_rect (map_view_rect) by travel_minimap_setup.
const TRAVEL_MINIMAP_RECT: Rect = rect(0xcc, 4, 0x13c, 0x3c);

// = seg001:14a4 map_caption_rect — the map caption strip (77, 33)-(245, 41),
// the cursor-bracket rect of map_caption_frame_task (seg000:46c4).
const MAP_CAPTION_RECT: Rect = rect(77, 33, 245, 41);

// = seg001:1492 travel_minimap_restore_rect — the minimap + border rect
// hnm_present_flight_frame copies from the back buffer over each flight frame.
const TRAVEL_MINIMAP_RESTORE_RECT: Rect = rect(0xc8, 0, 320, 0x40);

// = seg000:514e compass_angle_from_delta — the octant-interpolated compass
// angle of a signed screen delta (dx right, dy down): 0 due north, growing
// clockwise, 0x20 per compass point (0x40 east, 0x80 south, 0xc0 west),
// interpolated inside each octant as 0x20 * minor / major delta. A zero
// delta returns 0 (DOS falls out with CF set and al = 0; the caller ignores
// the flag).
fn compass_angle_from_delta(dx: i16, dy: i16) -> u8 {
    // = seg000:514e..5160 ax = |dy|, cx = |dx| (di saves dx).
    // = seg000:5160 cmp cx,ax; jb loc_05180 — the shallow octants.
    if dx.unsigned_abs() >= dy.unsigned_abs() {
        // = seg000:5164 cmp cx,1; jb — |dx| == 0 means both deltas are 0.
        if dx == 0 {
            return 0;
        }
        // = seg000:5169..5170 al = 0x20 * dy / dx (idiv truncates to zero).
        let al = ((0x20 * dy as i32) / dx as i32) as u8;
        // = seg000:5174..517c +0x40 on the east side (dx positive), +0xc0 on
        //   the west.
        if dx >= 0 {
            al.wrapping_add(0x40)
        } else {
            al.wrapping_add(0xc0)
        }
    } else {
        // = seg000:5185..518a the steep octants: al = 0x20 * dx / dy.
        let mut al = ((0x20 * dx as i32) / dy as i32) as u8;
        // = seg000:518e..5192 on the south side (dy positive) bias by -0x80.
        if dy >= 0 {
            al = al.wrapping_sub(0x80);
        }
        // = seg000:5194 neg al — north lands at 0, angles grow clockwise.
        al.wrapping_neg()
    }
}

// = seg000:5198 travel_heading_deltas — split a compass heading into per-step
// (delta longitude, delta latitude) travel deltas: the major axis gets ±0x20
// (one full step) and the minor axis the signed remainder, interpolated
// inside each quadrant (heading 0 = north → (0, -0x20), 0x40 = east →
// (0x20, 0), ...).
fn travel_heading_deltas(heading: u8) -> (i16, i16) {
    // = seg000:5198..51a5 bl = heading + 0x20; the masked quadrant test.
    let bl = heading.wrapping_add(0x20);
    if bl & 0x7f >= 0x40 {
        // = seg000:51a7..51b9 the east/west-major quadrants: dlng = ±0x20,
        //   dlat = the signed remainder (heading - 0x40, mirrored on the
        //   west side).
        let mut dlng = 0x20i16;
        let mut al = heading.wrapping_sub(0x40);
        if bl & 0x80 != 0 {
            dlng = -0x20;
            al = al.wrapping_sub(0x80).wrapping_neg();
        }
        (dlng, al as i8 as i16)
    } else {
        // = seg000:51ba..51ca the north/south-major quadrants: dlat = ∓0x20,
        //   dlng = the signed remainder (mirrored on the south side).
        let mut dlat = -0x20i16;
        let mut al = heading;
        if bl & 0x80 != 0 {
            al = al.wrapping_sub(0x80).wrapping_neg();
            dlat = 0x20;
        }
        (al as i8 as i16, dlat)
    }
}

/// = seg001:1ac8 mouse_handlers_01ac8 — the map screen's MouseHandlers record.
/// The idle and both drag slots run the hover tracker (map_mouse_hover_tracker,
/// seg000:4586: the location-marker / compass-ray hover state in data_046fc);
/// the LMB press is the destination click (map_mouse_lmb_select_destination,
/// seg000:450e). The RMB and both release slots are the no-op loc_00f66.
pub(crate) static MAP_MOUSE_HANDLERS: MouseHandlers = MouseHandlers {
    idle: GameState::map_mouse_hover_tracker,
    lmb: GameState::map_mouse_lmb_select_destination,
    rmb: GameState::map_mouse_rmb,
    release: GameState::map_mouse_release,
    rmb_release: GameState::map_mouse_rmb_release,
    drag: GameState::map_mouse_drag,
    rmb_drag: GameState::map_mouse_rmb_drag,
};

/// One entry of the visible-location marker list (seg001:a5c0,
/// visible_location_markers): a location currently inside the map window and
/// its marker's screen position. DOS packs [location ptr:u16, screen x:u16,
/// screen y:u8, data_046eb copy:u8] with a 0-word terminator; the port stores
/// the location index and unpacked coordinates. The map hover tracker resolves
/// the cursor against this list (find_nearest_location_marker).
#[derive(Clone, Copy)]
pub(crate) struct MapLocationMarker {
    pub(crate) location_index: u16,
    pub(crate) x: i16,
    pub(crate) y: i16,
    /// = the entry's +5 byte, the data_046eb value the entry was built under;
    /// bit 0x40 marks the travel-pass entries a 0x40 rebuild replaces.
    pub(crate) mode: u8,
}

impl GameState {
    // = seg000:42d9 menu_callback_choice_map_main_take_an_ornithopter — the map
    // main menu's (MENU_MAP_TROOPS) TAKE AN ORNITHOPTER slot, reached from the SEE
    // DUNE MAP view: board from the current location's outdoor room 1, leave the
    // full-planet map for the room view, then fall through into the notransition
    // tail that opens the cockpit map.
    pub(crate) fn menu_callback_choice_map_main_take_an_ornithopter(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:42d9/42dd dx = location_and_room, bx = location_appearance;
        //   42e1 dl = 1 — the orni boards from the location's outdoor room 1
        //   (the pad), whatever room the map was opened from.
        let new_room = (self.location_and_room & 0xff00) | 1;
        let new_appearance = self.location_appearance;
        // = seg000:42e3 call callback_transition_04057 — commit the room move
        //   (state only here: the full-map view's data_046eb bit 7 skips its
        //   room redraw).
        self.commit_room_move(new_room, new_appearance);
        // = seg000:42e6 call ui_toggle_room_view — tear the full-planet map
        //   down and re-enter the room view.
        self.ui_toggle_room_view();
        // = seg000:42e9 falls through into the notransition entry.
        self.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);
    }

    // = seg000:42e9 menu_callback_choice_map_main_take_an_ornithopter_notransition
    // — the TAKE AN ORNITHOPTER room verb (command record seg001:21dc): open the
    // map screen in ornithopter (cockpit) mode. Also reached from the room
    // ornithopter click (callback_main_ui_element_21_22, seg000:9282) and as the
    // fall-through tail of the map-main-menu entry (seg000:42d9).
    pub(crate) fn menu_callback_choice_map_main_take_an_ornithopter_notransition(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:42e9 call tear_down_prior_talking_head_overlay.
        self.tear_down_prior_talking_head_overlay();
        // = seg000:42ec call loc_038e1 — refresh the sky palette if the
        //   time-of-day advanced while a sky scene is live.
        self.loc_038e1_sky_refresh();
        // = seg000:42ef ax=24h; call open_resource_by_index — preload ORNYPAN.HSQ.
        self.open_sprite_bank(sprite_bank::ORNYPAN);
        // = seg000:42f5 map_ornithopter_mode = 1 — cockpit mode.
        self.map_ornithopter_mode = 1;
        // = seg000:42fa game_screen_mode_flags = 4 — the map-travel screen mode.
        self.game_screen_mode_flags = 4;
        // = seg000:42ff travel_vehicle_mode = 2 — travel by ornithopter.
        self.travel_vehicle_mode = 2;
        self.map_screen_open_with_cancel_menu();
    }

    // = seg000:4305 map_screen_open_with_cancel_menu — open the map main view
    // with the single-entry Cancel menu (bp = menu_multiple_cancel). Shared tail
    // of TAKE AN ORNITHOPTER and CALL A WORM.
    pub(crate) fn map_screen_open_with_cancel_menu(&mut self) {
        // = seg000:4308 call loc_049ea — reset the travel path scratch.
        self.travel_reset_trail();
        self.map_screen_open(menu_defs::MENU_CANCEL.records.to_vec());
    }

    // = seg000:430b map_screen_open — open the map main view. DOS bp = the
    // command-menu record buffer to fold in (the port's `records`).
    pub(crate) fn map_screen_open(&mut self, records: Vec<MenuItem>) {
        // = seg000:430b bx=map_screen_cleanup; call loc_0d323 — request the
        //   panel transition, push the menu with the
        //   map_screen_cleanup func, fold the menu in, then refresh the hover
        //   highlight.
        self.screen_overlay_request_transition();
        // = the caller's record buffer (bp) — installed into menu_multiple_
        //   cancel (seg001:212e) before the push, like DOS building the buffer
        //   it is about to insert.
        self.menu_cancel.records = records;
        self.menu_stack_push(MenuRef::MenuCancel, Some(GameState::map_screen_cleanup));
        self.play_pending_panel_fold();
        self.highlight_hovered_text_action_item();
        // = seg000:4311 ax=mouse_handlers_01ac8; call set_active_mouse_handlers.
        self.active_mouse_handlers = &MAP_MOUSE_HANDLERS;
        // = seg000:4317 call loc_04aca — data_011ca = 1 (suspends the pending
        //   room-swap machinery while the map owns the screen).
        self.data_011ca = 1;
        // = seg000:431a data_046fc = 0 — clear the hover tracker state.
        self.data_046fc = 0;
        // = seg000:4320 call set_zoomed_globe_pos_from_map_position — centre the
        //   map window on the player's map position.
        self.set_zoomed_globe_pos_from_map_position();
        // = seg000:4323 data_046eb = 1 — the map/travel view owns the screen
        //   (selects the alternate nav panel and the windowed map drawing).
        self.data_046eb = 1;
        // = seg000:4328 si=data_01cca; call loc_0d72b — install and draw the
        //   alternate (travel) nav panel.
        self.ui_install_nav_panel(&NAV_PANEL_ALT);
        // = seg000:432e si=map_view_rect_template; di=data_046e3_rect;
        //   call set_mouse_nav_rect; call copy_rect_at_si_to_di — install the
        //   map window as both the navigation mouse hot-zone (the hand /
        //   travel-arrow cursor regions) and the map view rect.
        self.set_mouse_nav_rect(MAP_VIEW_RECT_TEMPLATE);
        self.map_view_rect = MAP_VIEW_RECT_TEMPLATE;
        // = seg000:433a call map_screen_draw_base.
        self.map_screen_draw_base();
        // = seg000:433d ax=2bch; call start_narration_voice_clip — "select
        //   destination on map".
        self.start_narration_voice_clip(0x2bc);
        // = seg000:4343 call map_add_select_destination_text_task.
        self.map_add_select_destination_text_task();
        // = seg000:4346 current_main_view_drawing_function = 4377h — install
        //   map_view_redraw as the main-view redraw the dispatch sites call.
        self.current_main_view_drawing_function = Some(GameState::map_view_redraw);
        // = seg000:434c call loc_05b93 — clip sprites to the map window. The
        //   port passes clip rects per draw call instead of storing the segvga
        //   clip rect; the marker draws take map_view_clip_rect directly.
        // = seg000:434f call map_draw_zoomed_globe.
        self.map_draw_zoomed_globe();
        // = seg000:4352 call load_icones_sprites.
        self.open_icones_spritesheet();
        // = seg000:4355 call map_build_and_draw_location_markers.
        self.map_build_and_draw_location_markers();
        // = seg000:4358..436b in ornithopter mode, re-open ORNYPAN and redraw
        //   the cockpit window frame over the map, then flush the palette.
        if self.map_ornithopter_mode != 0 {
            self.open_sprite_bank(sprite_bank::ORNYPAN);
            self.with_active_bank_sheet(|s, sheet| {
                s.draw_icons_list_at_si(&ORNYPAN_WINDOW_OVERLAY_ICONS, sheet);
            });
            self.update_screen_palette();
        }
        // = seg000:436e call present_game_area — push the composed game area to the
        //   visible screen.
        self.present_game_area();
        // = seg000:4371 call map_arm_player_marker_task.
        self.map_arm_player_marker_task();
        // = seg000:4374 jmp play_pending_panel_fold — no-op here unless a
        //   transition re-armed during the draws.
        self.play_pending_panel_fold();
    }

    // = seg000:439f map_screen_draw_base — draw the map screen base into fb1.
    fn map_screen_draw_base(&mut self) {
        // = seg000:439f call set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = seg000:43a2 cmp map_ornithopter_mode,0; jnz loc_043cc.
        if self.map_ornithopter_mode == 0 {
            // = seg000:43a9..43c9 the globe/worm-mode base: present the
            //   framebuffer, snapshot it to fb2, flush the palette, draw the
            //   nested border around the map window (loc_05b69, colour 0xfc)
            //   and fill the title strip (data_014a4, colour 0xf5), then
            //   present_game_area. TODO: port with CALL A WORM.
            println!("map_screen_draw_base: globe-mode base not ported");
            return;
        }
        // = seg000:43cc cmp night_attack_stage,0; jnz — the sky backdrop is
        //   skipped while the night attack is staged.
        if self.night_attack_stage == 0 {
            // = seg000:43d3 call draw_sky.
            self.draw_sky();
        }
        // = seg000:43d6 ax=24h; open_resource_by_index; si=icon_list_ornypan_
        //   cockpit; call draw_icons_list_at_si — the ornithopter cockpit.
        self.open_sprite_bank(sprite_bank::ORNYPAN);
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_icons_list_at_si(&ORNYPAN_COCKPIT_ICONS, sheet);
        });
    }

    // = seg000:4377 map_view_redraw — redraw the map main view into fb1.
    // Installed as _word_23B9D_current_main_view_drawing_function while the map
    // screen is up; the live game loop invokes it after scrolling.
    pub(crate) fn map_view_redraw(&mut self) {
        // = seg000:4377 call set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = seg000:437a call loc_05b93 — sprite clip = map window (see
        //   map_screen_open; the port passes clips per draw call).
        // = seg000:437d call map_draw_zoomed_globe.
        self.map_draw_zoomed_globe();
        // = seg000:4380 call load_icones_sprites.
        self.open_icones_spritesheet();
        // = seg000:4383 call map_build_and_draw_location_markers.
        self.map_build_and_draw_location_markers();
        // = seg000:4386..4396 in ornithopter mode redraw the cockpit window
        //   frame over the map.
        if self.map_ornithopter_mode != 0 {
            self.open_sprite_bank(sprite_bank::ORNYPAN);
            self.with_active_bank_sheet(|s, sheet| {
                s.draw_icons_list_at_si(&ORNYPAN_WINDOW_OVERLAY_ICONS, sheet);
            });
        }
        // = seg000:4399 call update_screen_at_sprite_rect_updating_head —
        //   present the sprite clip rect (= the map window; loc_05b93 keeps
        //   _unk_2CCE4_sprite_clip_rect = data_046e3_rect while the map view
        //   is up) from fb1 to the screen. Only the window is pushed: the
        //   caption / hover label strip above it lives on the visible screen
        //   only (tick_map_caption, map_draw_hover_label), so a full
        //   game-area push would erase it.
        self.present_screen_rect(self.map_view_clip_rect());
        // = seg000:439c jmp map_arm_player_marker_task.
        self.map_arm_player_marker_task();
    }

    // = seg000:8829 ui_click_map_up — the alternate nav panel's up arrow (live
    // HUD record 13; also the pseudo record the up travel-arrow cursor resolves
    // to in hit_test_ui_elements). si = map_scroll_delta_up.
    pub(crate) fn ui_click_map_up(&mut self) {
        self.ui_click_map_buttons(MAP_SCROLL_DELTA_UP);
    }

    // = seg000:8824 ui_click_map_right — the right arrow (live HUD record 14).
    // si = map_scroll_delta_right.
    pub(crate) fn ui_click_map_right(&mut self) {
        self.ui_click_map_buttons(MAP_SCROLL_DELTA_RIGHT);
    }

    // = seg000:882e ui_click_map_down — the down arrow (live HUD record 15).
    // si = map_scroll_delta_down.
    pub(crate) fn ui_click_map_down(&mut self) {
        self.ui_click_map_buttons(MAP_SCROLL_DELTA_DOWN);
    }

    // = seg000:881f ui_click_map_left — the left arrow (live HUD record 16).
    // si = map_scroll_delta_left.
    pub(crate) fn ui_click_map_left(&mut self) {
        self.ui_click_map_buttons(MAP_SCROLL_DELTA_LEFT);
    }

    // = seg000:8831 ui_click_map_buttons — scroll the map view by an arrow's
    // (delta longitude, delta latitude) pair and redraw. DOS al = the live
    // button byte (mouse_button_state_prev at dispatch); the arrows carry the
    // 0x4000 repeat flag, so the game loop re-fires this every 0x32 ticks
    // while the button is held over the record.
    fn ui_click_map_buttons(&mut self, (dlng, dlat): (u16, i16)) {
        // = seg000:8831 test al,1; jz loc_08857 — only while the LMB is down;
        //   the release edge's final armed-element fire is a no-op.
        if self.prev_mouse_buttons & 1 == 0 {
            return;
        }
        // = seg000:8835 cmp [data_046eb],0; jns loc_08846 — the full-map view
        //   (bit 0x80): the spice-density sub-mode (bit 0x40) scrolls
        //   globe_param_3/4 instead (loc_08858 -> loc_0542f, not ported); the
        //   plain full map dismisses the rallied-troops popup and scrolls the
        //   shared zoomed position below.
        if self.data_046eb & 0x80 != 0 {
            if self.data_046eb & 0x40 != 0 {
                println!("ui_click_map_buttons: spice-density scroll (loc_08858) not ported");
                return;
            }
            // = seg000:8843 call map_dismiss_rallied_troops_popup.
            self.map_close_rallied_troops_popup();
        }
        // = seg000:8846 lodsw; add [zoomed_globe_longitude],ax; lodsw; add
        //   [zoomed_globe_latitude],ax — the longitude wraps around the map;
        //   the latitude is clamped by the next map_draw_zoomed_globe.
        self.zoomed_globe_longitude = self.zoomed_globe_longitude.wrapping_add(dlng);
        self.zoomed_globe_latitude = self.zoomed_globe_latitude.wrapping_add(dlat);
        // = seg000:884c falls through into map_refresh_main_view.
        self.map_refresh_main_view();
    }

    // = seg000:5b05 ui_click_map_center — the alternate nav panel's centre
    // button (live HUD record 12): recentre the map view on the player's map
    // position and redraw.
    pub(crate) fn ui_click_map_center(&mut self) {
        // = seg000:5b05 call loc_082a0; jnz loc_05b0d — ZF is set only when
        //   the active menu is menu_multiple_cancel/menu_map_move_prospectors
        //   AND data_046eb bit 0x40 is set (the globe-0x40 sub-mode); that
        //   path copies the zoomed position into globe_param_3/4 instead
        //   (map_enter_spice_density_overlay_in_place). The windowed map
        //   view (data_046eb == 1) never takes
        //   it. TODO: port with SEE DUNE MAP (the full-globe view).
        if self.data_046eb & 0x40 != 0 {
            println!("ui_click_map_center: globe-0x40 branches not ported");
            return;
        }
        // = seg000:5b0d call set_zoomed_globe_pos_from_map_position.
        self.set_zoomed_globe_pos_from_map_position();
        // = seg000:5b10 call map_refresh_main_view.
        self.map_refresh_main_view();
        // = seg000:5b13 test [data_046eb],40h; jz ret — the globe-0x40
        //   sub-mode reinstalls its nav rect (loc_05575, si = data_04710);
        //   unreachable here (bit 0x40 bailed out above).
    }

    // = seg000:8850 map_refresh_main_view — redraw the current main view after
    // a map scroll/recentre.
    fn map_refresh_main_view(&mut self) {
        // = seg000:8850 call map_dismiss_troop_popups.
        self.map_dismiss_troop_popups();
        // = seg000:8853 call [_word_23B9D_current_main_view_drawing_function]
        //   — the installed main-view redraw (map_view_redraw while the map
        //   screen is up; the travel/globe flows install their own). DOS
        //   reaches this only after an installer ran (the callers are gated
        //   on data_046eb != 0), so a missing function is a port bug.
        let redraw = self
            .current_main_view_drawing_function
            .expect("map_refresh_main_view with no main-view drawing function installed");
        redraw(self);
    }

    // = seg000:7b36 map_dismiss_troop_popups — close any troop-contact UI over
    // the map before a redraw, bracketed by in_transition = 0x80 so the
    // repaints do not arm a panel fold: the no-more-orders teardown
    // (menu_callback_choice_map_troop_contact_no_more_orders), the location
    // popup menu exit (loc_05f79) and the troop info panel close (loc_079de).
    pub(crate) fn map_dismiss_troop_popups(&mut self) {
        // = seg000:7b38 data_046d8 = 1 — the panels go away without their
        //   outline scale-out; the view is about to be repainted anyway.
        self.map_popup_anim_suppress = true;
        let saved = self.in_transition;
        self.in_transition = 0x80;
        // = seg000:7b42 call menu_callback_choice_map_troop_contact_no_more_orders.
        self.menu_callback_choice_map_troop_contact_no_more_orders(0, 0);
        self.map_close_location_troop_popup();
        self.map_close_troop_info_popup();
        self.in_transition = saved;
    }

    // = seg000:5f79 loc_05f79 map_close_location_troop_popup — close an open
    // location popup: when its GO THERE menu is up, exit the menu (its cleanup,
    // map_close_location_popup, closes the panel); otherwise (the panel-only
    // case) close the panel directly.
    pub(crate) fn map_close_location_troop_popup(&mut self) {
        // = seg000:5f7b xor ax,ax; xchg ax,[data_046f8]; jz ret.
        if self.map_location_popup_loc.is_none() {
            return;
        }
        // = seg000:5f83..5f8d the active element's priority byte: a locked base
        //   (0xff, no menu) closes directly (loc_05f91); a menu exits.
        if matches!(
            self.get_active_menu_ref(),
            MenuRef::MenuGoThereFlyingAnOrni | MenuRef::MenuGoThereRidingAWorm
        ) {
            self.menu_callback_choice_exit_menu(0, 0);
        } else {
            self.map_close_location_popup();
        }
    }

    // = seg000:4415 map_screen_cleanup — the map screen menu's cleanup func
    // (the DOS bx passed to the menu-stack push at seg000:430b), run when
    // the menu pops (the Cancel verb / menu_callback_choice_exit_menu).
    pub(crate) fn map_screen_cleanup(&mut self) {
        // = seg000:4415 xor al,al; xchg al,[data_046eb]; jnz — only once per
        //   open; data_046eb also drops back to the room nav panel.
        if std::mem::take(&mut self.data_046eb) == 0 {
            return;
        }
        // = seg000:4420 visible_location_markers head = 0 — clear the list.
        self.visible_location_markers.clear();
        // = seg000:4426 call clear_mouse_nav_rect.
        self.clear_mouse_nav_rect();
        // = seg000:4429 remove the map_player_marker_blink_task.
        self.remove_frame_task(TaskId::MapPlayerMarker);
        // = seg000:442f call map_remove_select_destination_text_task.
        self.map_remove_select_destination_text_task();
        // = seg000:4432 call copy_game_area_rect_to_unknown_rect.
        self.copy_game_area_rect_to_unknown_rect();
        // = seg000:4435 call loc_043e3 — restore the room backdrop.
        self.map_screen_restore_room_view();
        // = seg000:4438 call update_screen_palette.
        self.update_screen_palette();
        // = seg000:443b cmp travel_destination_ptr,0; jnz — keep the mode
        //   flags while a travel is pending (the confirm chain armed it
        //   before popping the element).
        // = seg000:4442 game_screen_mode_flags = 0.
        if self.travel_destination_ptr == 0 {
            self.game_screen_mode_flags = 0;
        }
        // = seg000:4447 call select_room_ui_table.
        self.select_room_ui_table();
        // = seg000:444a call ui_setup_and_draw_nav_panel — data_046eb is 0
        //   again, so this reinstalls the room (or map/book) nav panel.
        self.ui_setup_and_draw_nav_panel();
        // = seg000:444d call rebuild_and_draw_room_nav_panel.
        self.rebuild_and_draw_room_nav_panel();
        // = seg000:4450 cmp travel_minimap_state,0; jle — closing the map
        //   screen mid-flight (CHANGE DESTINATION armed state 1 at
        //   seg000:4980) re-enters the flight minimap view.
        if self.travel_minimap_state > 0 {
            self.travel_enter_minimap_view();
        }
        // = seg000:445a jmp loc_04ac4 — data_011ca = 0.
        self.data_011ca = 0;
    }

    // = seg000:43e3 loc_043e3 — restore the room backdrop the map screen drew
    // over, from the fb2 snapshot.
    fn map_screen_restore_room_view(&mut self) {
        if self.map_ornithopter_mode == 0 {
            // = seg000:43ea..43f9 globe mode: unless an HNM is mid-play
            //   (hnm_counter_2), restore the data_014ac rect (77,33)-(245,138)
            //   from fb2 and push it to the screen
            //   (present_screen_rect).
            //   TODO: port with CALL A WORM.
            println!("map_screen_restore_room_view: globe-mode restore not ported");
            return;
        }
        // = seg000:43fc cmp byte ptr [location_appearance],80h; jnz — only
        //   restore when a special room (the location entry) is beneath.
        if self.location_appearance as u8 != 0x80 {
            return;
        }
        // = seg000:4403 call set_sky_palette.
        self.set_sky_palette();
        // = seg000:4406 call copy_game_area_to_screen_fb2_to_fb1 — the room
        //   game area back from the fb2 snapshot.
        let yoff = self.y_offset as i16;
        let r = rect(0, yoff, 320, 152 + yoff);
        gfx::vga_copy_rect(&mut self.framebuffer, &self.framebuffer_saved, r);
        // = seg000:4409 call present_game_area — push it to the visible screen.
        self.present_game_area();
        // = seg000:440c jmp update_screen_palette.
        self.update_screen_palette();
    }

    // = seg000:b6c3 map_draw_zoomed_globe — draw the map into the map window.
    pub(crate) fn map_draw_zoomed_globe(&mut self) {
        // = seg000:b6c3 test data_046eb,80h — the full-map mode: the whole
        //   planet through the interpolated flat-map renderer.
        if self.data_046eb & 0x80 != 0 {
            // = seg000:b6cc/b6d2 the fixed window centre (0xa0, 0x4c) into
            //   data_0dcf6/data_0dcf8 — map_position_to_screen projects the
            //   markers against it (see the 0x80 branch there).
            // = seg000:b6d8..b6f5 clamp the latitude to ±0x4b so all 37 band
            //   row pairs have tablat entries.
            self.zoomed_globe_latitude = self.zoomed_globe_latitude.clamp(-0x4b, 0x4b);
            // = seg000:b6f8..b70f dx = the longitude, ax = latitude - 0x12
            //   (the top band), es:di = the MAP bytes, bp = TABLAT, si =
            //   RESOURCE_GLOBDATA, bx = the active framebuffer; call
            //   vga_draw_map_zoomed (ported as MapRenderer).
            let lat = self.zoomed_globe_latitude;
            let lng = self.zoomed_globe_longitude;
            let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
            // Disjoint field borrows: the renderer, the map bytes and the
            // active framebuffer.
            let fb = match self.active_fb {
                FbId::Screen => &mut self.screen,
                FbId::Fb1 => &mut self.framebuffer,
                FbId::Saved => &mut self.framebuffer_saved,
                FbId::Back => &mut self.framebuffer_back,
            };
            self.map_renderer.draw(fb, &self.map, tablat, lat, lng);
            return;
        }
        // = seg000:b714 loc_0b714 — the windowed map.
        let (rows, width, height, top_lat) = self.map_fill_window_rows_from(None);
        // = seg000:b7c6 test data_046eb,40h; jnz — bit 0x40 suppresses the
        //   blit (the spice-density overlay renders the same rows through
        //   vga_draw_landscape instead).
        if self.data_046eb & 0x40 == 0 {
            let r = self.map_view_rect;
            // = seg000:b7cd call [gfx_vtable_vga_blit_shaded].
            gfx::vga_blit_shaded(self, &rows, width, height, r.x0, r.y0, top_lat);
        }
        let _ = height;
    }

    // = seg000:b714..b7c4 the windowed map's row fill: one map row per window
    // row into the ss row buffer at RESOURCE_GLOBDATA (stride 0xc8). `source`
    // selects the byte layer — None = the terrain MAP.HSQ, Some(bytes) = the
    // layer DOS installs by swapping res_map_seg (the spice-density overlay
    // passes MAP2.HSQ, seg000:5487). Returns the buffer plus the window
    // width/height and the top row's latitude.
    pub(crate) fn map_fill_window_rows_from(
        &mut self,
        source: Option<&[u8]>,
    ) -> (Vec<u8>, usize, usize, i16) {
        // Window geometry from data_046e3_rect: width (data_0dcf2), height
        // (data_0dcf4) and centre (data_0dcf6/data_0dcf8).
        let r = self.map_view_rect;
        let width = (r.x1 - r.x0) as usize;
        let height = (r.y1 - r.y0) as usize;
        // = seg000:b745 dec ax; shr ax,1 — half the window height (rounded on
        //   the (height-1) form).
        let half = ((height - 1) / 2) as i16;
        // = seg000:b74a..b766 clamp the latitude to ±(0x56 - half) so every
        //   window row has a tablat entry.
        let max_lat = 0x56 - half;
        self.zoomed_globe_latitude = self.zoomed_globe_latitude.clamp(-max_lat, max_lat);
        // = seg000:b76c dx = longitude; b770 ax = latitude - half (the top row).
        let lng = self.zoomed_globe_longitude;
        let top_lat = self.zoomed_globe_latitude - half;

        // = seg000:b776..b7ac the row loop: one map row per window row into the
        //   ss row buffer at RESOURCE_GLOBDATA (stride 0xc8). DOS splits it into
        //   a northern walk (negated tablat offsets, sub bp,8) and a southern
        //   walk (add bp,8); the port's Tablat::offset/len handle both sides of
        //   the y = latitude + 98 index.
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        let mut rows = vec![0u8; 0xc8 * height];
        for (i, dst_row) in rows.chunks_exact_mut(0xc8).enumerate() {
            let y = (top_lat + i as i16 + 98) as u16;
            let row_off = tablat.offset(y) as usize;
            let row_len = tablat.len(y) as usize;
            // = seg000:b7df..b7e6 map_copy_window_row: the longitude cell =
            //   high word of longitude * row byte length (truncating — unlike
            //   map_position_to_offset's rounding). DOS also caches it in the
            //   tablat entry's +6 scratch word; its sole reader,
            //   map_screen_to_position (seg000:b62c), recomputes the same
            //   value in the port, so the cache itself is not modelled.
            let cell = ((row_len as u32 * lng as u32) >> 16) as usize;
            // = seg000:b7e8..b7f9 rows shorter than the window are centred.
            let (dst, eff_w) = if row_len < width {
                (&mut dst_row[(width - row_len) / 2..], row_len)
            } else {
                (&mut dst_row[..], width)
            };
            // = seg000:b7fb..b805 the window's left edge cell, wrapped.
            let left = (cell + row_len - eff_w / 2) % row_len;
            // = seg000:b807..b81d copy, wrapping around the row end.
            let avail = row_len - left;
            let bytes = source.unwrap_or(&self.map);
            let src = &bytes[row_off..row_off + row_len];
            if avail >= eff_w {
                dst[..eff_w].copy_from_slice(&src[left..left + eff_w]);
            } else {
                dst[..avail].copy_from_slice(&src[left..]);
                dst[avail..eff_w].copy_from_slice(&src[..eff_w - avail]);
            }
        }
        (rows, width, height, top_lat)
    }

    // = seg000:b647 map_position_to_screen — project a map position (x =
    // longitude units, lat = latitude row) to map-view screen coordinates:
    // y = (lat - zoomed_globe_latitude) + the window centre, x = the window
    // centre + the high word of (lng - zoomed_globe_longitude) * the
    // latitude's tablat row byte length (the same lng * len >> 16 cell
    // projection map_copy_window_row uses). Globe mode (data_046eb bit 0x80)
    // scales both deltas by 4.
    pub(crate) fn map_position_to_screen(&self, x: u16, lat: i16) -> (i16, i16) {
        // = seg000:b649 cl = 2 on the full globe, else 0.
        let shift = if self.data_046eb & 0x80 != 0 { 2 } else { 0 };
        // = data_0dcf6 / data_0dcf8 — the window centre, as map_draw_zoomed_
        // globe stores it: the fixed (0xa0, 0x4c) in full-map mode
        // (seg000:b6cc/b6d2), else derived from data_046e3_rect.
        let r = self.map_view_rect;
        let (centre_x, centre_y) = if self.data_046eb & 0x80 != 0 {
            (0xa0, 0x4c)
        } else {
            (r.x0 + (r.x1 - r.x0) / 2, r.y0 + (r.y1 - r.y0 - 1) / 2)
        };
        // = seg000:b652..b65a the screen y.
        let sy = ((lat - self.zoomed_globe_latitude) << shift) + centre_y;
        // = seg000:b65e..b67e the screen x: `imul bp` then the cl-step
        // `shl ax,1; rcl dx,1` — the 32-bit product shifted left, high word.
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        let row_len = tablat.len((lat + 98) as u16) as i32;
        let dlng = (x.wrapping_sub(self.zoomed_globe_longitude) as i16) as i32;
        let sx = (((dlng * row_len) << shift) >> 16) as i16 + centre_x;
        (sx, sy)
    }

    // = seg000:62d6 map_position_to_screen_if_visible — project the map
    // position and bounds-check it against the map window: Some(screen pos)
    // iff inside (DOS: CF clear, dx/bx = the position).
    pub(crate) fn map_position_to_screen_if_visible(&self, x: u16, lat: i16) -> Option<(i16, i16)> {
        let (sx, sy) = self.map_position_to_screen(x, lat);
        // = seg000:62d9..62f0 inside = x0 <= sx < x1 && y0 <= sy < y1.
        let r = self.map_view_rect;
        if sx < r.x0 || sx >= r.x1 || sy < r.y0 || sy >= r.y1 {
            return None;
        }
        Some((sx, sy))
    }

    // = seg000:62c9 location_visible_on_map — Some(screen pos) iff the
    // location is visible on the map view.
    fn location_visible_on_map(&self, location_index: usize) -> Option<(i16, i16)> {
        // = seg000:62c9 cmp [data_046eb],1; jb ret — no map view on screen.
        if self.data_046eb == 0 {
            return None;
        }
        // = seg000:62d0 dx/bx = the location's map_x/map_y.
        let loc = &self.locations[location_index];
        self.map_position_to_screen_if_visible(loc.map_x as u16, loc.map_y)
    }

    // = seg000:7c8f location_distance_from_player — the Chebyshev distance in
    // map cells between the player and the location:
    // max(|delta longitude| / the player row's longitude-units-per-cell,
    // |delta latitude|).
    fn location_distance_from_player(&self, location_index: usize) -> u16 {
        // = seg000:7c90 call get_map_position.
        let (x, lat) = self.get_map_position();
        // = seg000:7c94..7c9c bp = lng_units_per_cell_table[|player latitude|].
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        let per_cell = tablat.lng_units_per_cell((lat + 98) as u16);
        let loc = &self.locations[location_index];
        // = seg000:7ca0..7cab |location->map_x - x| / bp (truncating).
        let dx = (loc.map_x as u16).abs_diff(x) / per_cell;
        // = seg000:7cad..7cb2 |lat - location->map_y|.
        let dy = lat.abs_diff(loc.map_y);
        // = seg000:7cb4 the larger of the two.
        dx.max(dy)
    }

    // = seg000:5e42 calc_location_marker_sprite — the map marker sprite for a
    // location: ICONES base 0x3a (windowed view; the full globe uses 0x7a),
    // tiered by the location's appearance through the shared calc_SAL_index
    // thresholds. Returns (sprite, appearance) (DOS ax, cl).
    fn calc_location_marker_sprite(&self, location_index: usize) -> (u16, u8) {
        let base = if self.data_046eb & 0x80 != 0 {
            0x7a
        } else {
            0x3a
        };
        let appearance = self.locations[location_index].appearance;
        // = seg000:5e4f falls into calc_SAL_index.
        (
            base + crate::room_scene::calc_sal_index(appearance) as u16,
            appearance,
        )
    }

    // = seg000:5b93 loc_05b93 — the sprite clip rect while the map view is up:
    // the map window (data_046e3_rect). The port passes it per draw call
    // instead of storing the segvga clip rect; like the segvga clip, it lives
    // in fb_base_ofs (y_offset) space.
    pub(crate) fn map_view_clip_rect(&self) -> Rect {
        let yoff = self.y_offset as i16;
        let r = self.map_view_rect;
        rect(r.x0, r.y0 + yoff, r.x1, r.y1 + yoff)
    }

    // = seg000:5dce map_build_and_draw_location_markers — rebuild the
    // visible-location marker list and draw the markers over the map view.
    pub(crate) fn map_build_and_draw_location_markers(&mut self) {
        // = seg000:5dd1 or al,al; js — full-map mode first draws the
        // vegetation tufts (map_draw_vegetation_marks).
        if self.data_046eb & 0x80 != 0 {
            self.map_draw_vegetation_marks();
        }
        // = seg000:5dda..5def — data_046eb bit 0x40 keeps the list's leading
        // non-0x40 entries and rebuilds from the first 0x40-flagged one (the
        // travel pass); otherwise rebuild the whole list.
        let start = if self.data_046eb & 0x40 != 0 {
            self.visible_location_markers
                .iter()
                .position(|m| m.mode & 0x40 != 0)
                .unwrap_or(self.visible_location_markers.len())
        } else {
            0
        };
        self.visible_location_markers.truncate(start);

        let clip = self.map_view_clip_rect();
        let yoff = self.y_offset as i16;
        // = seg000:5df1..5e3b walk locations[] to the 0xffff fence.
        for i in 0..self.locations.len() {
            // = seg000:5df9 test byte ptr [si+0ah],80h — a hidden location.
            if self.locations[i].status & 0x80 != 0 {
                continue;
            }
            // = seg000:5dff call location_visible_on_map; jb — off-window.
            let Some((x, y)) = self.location_visible_on_map(i) else {
                continue;
            };
            // = seg000:5e04..5e0d append [location ptr, x, y, data_046eb].
            self.visible_location_markers.push(MapLocationMarker {
                location_index: i as u16,
                x,
                y,
                mode: self.data_046eb,
            });
            // = seg000:5e12 call calc_location_marker_sprite.
            let (mut sprite, appearance) = self.calc_location_marker_sprite(i);
            // = seg000:5e15 cmp cl,20h; jnb — only sietches get the distance
            // variant: = seg000:5e1e..5e2b farther than
            // location_visibility_distance draws the +5 distant sprite.
            if appearance < 0x20
                && self.location_distance_from_player(i) > self.location_visibility_distance
            {
                sprite += 5;
            }
            // = seg000:5e2e call loc_0c343 — the marker sprite centred on the
            // screen position, clipped to the map window.
            self.with_active_bank_sheet(|s, sheet| {
                s.draw_sprite_centered_clipped(sheet, sprite, x, y + yoff, clip);
            });
        }
        // = seg000:5e3d the list's 0 terminator — implicit in the Vec.
    }

    // = seg000:49ea travel_reset_trail — reset the travel state:
    // travel_minimap_state = 0 and every travel_trail_ring entry refilled with
    // the 0x800 empty sentinel (the write cursor keeps its position, like DOS).
    pub(crate) fn travel_reset_trail(&mut self) {
        self.travel_minimap_state = 0;
        self.travel_trail_ring = [(0x800, 0x800); crate::game_state::TRAVEL_TRAIL_LEN];
    }

    // = seg000:4658 map_add_select_destination_text_task — arm the map caption
    // typewriter: the map_caption_frame_task then writes "SELECT DESTINATION
    // ON MAP" over the map one letter per firing.
    fn map_add_select_destination_text_task(&mut self) {
        // = seg000:4658 cmp [data_0473f],0; jnz ret — already armed.
        if !self.map_caption_text.is_empty() {
            return;
        }
        // = seg000:4662 si=57h; get_phrase_or_command_string_si — "SELECT
        //   DESTINATION ON MAP"; 4668/466c store the far pointer
        //   (data_04741:data_0473f).
        self.map_caption_text = self.get_phrase_or_command_string(0x57).to_vec();
        self.map_caption_pos = 0;
        // = seg000:4670 [data_04743] = 0x55 — the pen x.
        self.map_caption_x = 0x55;
        // = seg000:4676 cx=0f561h; ax=22h — globe mode pen y 0x22, colour word
        //   0xf561 (bg 0xf5, fg 0x61). = seg000:467c in ornithopter mode:
        //   al += 4 (y 0x26) and ch = 0x20 (colour word 0x2061).
        let (y, color) = if self.map_ornithopter_mode == 0 {
            (0x22, 0xf561)
        } else {
            (0x26, 0x2061)
        };
        // = seg000:4687/468a store the pen y and colour word.
        self.map_caption_y = y;
        self.map_caption_color = color;
        // = seg000:468e si=map_caption_frame_task; bp=18h; call add_frame_task.
        self.add_frame_task(24, TaskId::MapCaption);
    }

    // = seg000:469b map_remove_select_destination_text_task — disarm the map
    // caption typewriter.
    fn map_remove_select_destination_text_task(&mut self) {
        // = seg000:469b cmp [data_0473f],0; jz ret.
        if self.map_caption_text.is_empty() {
            return;
        }
        // = seg000:46a2 [data_0473f] = 0; 46ab remove_frame_task.
        self.map_caption_text.clear();
        self.map_caption_pos = 0;
        self.remove_frame_task(TaskId::MapCaption);
    }

    // = seg000:46b5 map_caption_frame_task — the map caption typewriter: each
    // firing draws ONE glyph of the armed string to the front buffer at the
    // stored pen, advancing the pointer and the pen. A space costs no firing
    // (seg000:46fe jumps straight back for the next glyph); a high-bit byte is
    // the string terminator — the task then idles until removed.
    pub(crate) fn tick_map_caption(&mut self) {
        let mut drew = false;
        // = seg000:46b5 les si,[data_0473f]; al = es:[si]; or al,al; js
        //   loc_04702 — stop on the high-bit terminator (or the slice end).
        while let Some(&c) = self.map_caption_text.get(self.map_caption_pos) {
            if c & 0x80 != 0 {
                break;
            }
            // = seg000:46c0 inc word ptr [data_0473f].
            self.map_caption_pos += 1;
            // = seg000:46c4/46c7 si = map_caption_rect; call
            //   restore_mouse_if_rect_intersects — lift the cursor off the
            //   caption strip before the glyph draws under it.
            self.restore_mouse_if_rect_intersects(MAP_CAPTION_RECT);
            // = seg000:46ca push [framebuffer_active_seg];
            //   call set_screen_as_active_framebuffer — draw to the front
            //   buffer.
            let saved = self.active_fb();
            self.set_screen_as_active_framebuffer();
            // = seg000:46d1 font_set_draw_position([data_04743], [data_04745]).
            self.font_set_draw_position(self.map_caption_x, self.map_caption_y);
            // = seg000:46dc mov [font_draw_fg_color], [data_04747] — the whole
            //   colour word (fg + bg).
            self.font_state.color = self.map_caption_color;
            // = seg000:46e4 call font_select_small_font; 46e8 call
            //   font_draw_glyph_func_small.
            self.font_select_small_font();
            self.font_draw_glyph(c);
            // = seg000:46eb font_get_draw_position; 46ee/46f2 store the
            //   advanced pen back.
            let (x, y) = self.font_get_draw_position();
            self.map_caption_x = x;
            self.map_caption_y = y;
            // = seg000:46f7 pop [framebuffer_active_seg]; 46fb call
            //   draw_mouse_cursor_if_needed — close the bracket before the
            //   post-loop publish, so the presented frame keeps the cursor.
            self.active_fb = saved;
            self.draw_mouse_cursor_if_needed();
            drew = true;
            // = seg000:46fe cmp al,20h; jz map_caption_frame_task — a space
            //   costs no firing: draw the next glyph immediately.
            if c != 0x20 {
                break;
            }
        }
        // DOS writes straight to the visible A000 buffer; the port publishes
        // the touched screen. Skipped while the front buffer is redirected to
        // fb1 (the glyph then landed offscreen, like every other screen push).
        if drew && !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:445d map_arm_player_marker_task — arm the blinking "you are
    // here" marker over the player's map position.
    fn map_arm_player_marker_task(&mut self) {
        // = seg000:445d remove_frame_task(map_player_marker_blink_task).
        self.remove_frame_task(TaskId::MapPlayerMarker);
        // = seg000:4463 get_map_position; call map_position_to_screen_if_
        //   visible; jnb loc_04472.
        let (x, lat) = self.get_map_position();
        let Some((sx, sy)) = self.map_position_to_screen_if_visible(x, lat) else {
            // = seg000:446b map_player_marker_rect = 0 — the player is off
            //   the window, no marker.
            self.map_player_marker_rect = Rect::default();
            return;
        };
        // = seg000:4472 call load_icones_sprites; 4475..4484 read ICONES
        //   sprite 0x4c's header words for its width (low 12 bits) and height.
        self.open_icones_spritesheet();
        let (mut w, mut h) = (0i16, 0i16);
        self.with_active_bank_sheet(|_, sheet| {
            if let Some(sprite) = sheet.get_sprite(0x4c) {
                w = sprite.width() as i16;
                h = sprite.height() as i16;
            }
        });
        // = seg000:4486..449a the marker's bounding rect:
        //   (x - 13, y - height) .. (x0 + width, y0 + height).
        let (x0, y0) = (sx - 13, sy - h);
        self.map_player_marker_rect = rect(x0, y0, x0 + w, y0 + h);
        // = seg000:449d re-add the blink task at interval 0x12c; 44a6 phase = 0.
        self.add_frame_task(0x12c, TaskId::MapPlayerMarker);
        self.map_player_marker_phase = 0;
        // = seg000:44a6 falls through into the task body — the immediate
        //   first draw (an odd phase, so the marker shows).
        self.tick_map_player_marker();
    }

    // = seg000:44ab map_player_marker_blink_task — the blinking "you are here"
    // map marker: each firing restores the marker rect from fb1 (the clean map
    // with the location markers), then on odd phases draws ICONES sprite 0x4c
    // at the rect's origin, so the marker alternates hidden/visible.
    pub(crate) fn tick_map_player_marker(&mut self) {
        // = seg000:44ab inc map_player_marker_phase.
        self.map_player_marker_phase = self.map_player_marker_phase.wrapping_add(1);
        // = seg000:44af push [framebuffer_active_seg]; call
        //   set_screen_as_active_framebuffer — draw to the front buffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:44b6 call loc_05b93 (the map-window sprite clip);
        //   44b9 call load_icones_sprites.
        let clip = self.map_view_clip_rect();
        self.open_icones_spritesheet();
        // = seg000:44bc/44bf si = map_player_marker_rect; call
        //   restore_mouse_if_rect_intersects — lift the cursor off the marker
        //   rect before the restore below erases it from the screen.
        let m = self.map_player_marker_rect;
        let r = self.map_view_rect;
        let yoff = self.y_offset as i16;
        self.restore_mouse_if_rect_intersects(rect(m.x0, m.y0 + yoff, m.x1, m.y1 + yoff));
        // = seg000:44c2..44ee clamp the marker rect to the map window and push
        //   it fb1 -> screen (present_screen_rect_regs) — erases the previous
        //   marker image with the clean map beneath.
        self.present_screen_rect(rect(
            m.x0.max(r.x0),
            m.y0.max(r.y0) + yoff,
            m.x1.min(r.x1),
            m.y1.min(r.y1) + yoff,
        ));
        // = seg000:44f1 bl = phase; shr bl,1; jnb — odd phases draw ICONES
        //   sprite 0x4c at the rect origin, clipped to the map window.
        if self.map_player_marker_phase & 1 != 0 {
            let (x0, y0) = (m.x0, m.y0 + yoff);
            self.with_active_bank_sheet(|s, sheet| {
                s.draw_sprite_from_sheet_clipped(sheet, 0x4c, x0, y0, clip);
            });
        }
        // = seg000:4507 pop [framebuffer_active_seg].
        self.active_fb = saved;
        // = seg000:450b jmp draw_mouse_cursor_if_needed — close the cursor
        //   bracket before the publish below, so the frame the display gets
        //   carries the re-drawn cursor.
        self.draw_mouse_cursor_if_needed();
        // DOS draws straight to the visible A000 buffer; the port publishes
        // the touched screen (cf. tick_map_caption).
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:5e6d find_nearest_location_marker — walk the visible-location
    // marker list for the entry nearest (x, y) by Manhattan distance. Entries
    // are skipped unless the location's appearance is below `max_appearance`
    // and the entry's stored mode byte equals data_046eb. Returns the winning
    // location ptr and its distance ((0, 0xffff) with no match, DOS di/ax);
    // the first entry wins ties.
    pub(crate) fn find_nearest_location_marker(
        &self,
        max_appearance: u8,
        x: i16,
        y: i16,
    ) -> (u16, u16) {
        // = seg000:5e73/5e7e [bp-8] = 0xffff (best distance), [bp-2] = 0.
        let mut best = 0u16;
        let mut best_dist = 0xffffu16;
        // = seg000:5e86 the entry walk to the 0-word terminator.
        for m in &self.visible_location_markers {
            // = seg000:5e8f cmp [di+8],al; jnb — appearance at or above the
            //   cap.
            if self.locations[m.location_index as usize].appearance >= max_appearance {
                continue;
            }
            // = seg000:5e97 cmp bh,[data_046eb]; jnz — the entry was built
            //   under a different view mode.
            if m.mode != self.data_046eb {
                continue;
            }
            // = seg000:5e9f..5eb0 dx = |entry.x - x| + |entry.y - y|.
            let dist = m.x.abs_diff(x).wrapping_add(m.y.abs_diff(y));
            // = seg000:5eb2 cmp dx,[bp-8]; jnb — strictly closer only.
            if dist < best_dist {
                best_dist = dist;
                best = location_ptr(m.location_index);
            }
        }
        (best, best_dist)
    }

    // = seg000:4586 map_mouse_hover_tracker — [si] of mouse_handlers_01ac8
    // (the idle slot; both drag slots re-run it too): recompute the hover
    // state into data_046fc, redrawing the hover label strip when it changes.
    // States: 0 = pointer outside the map window; a location ptr = within 9
    // pixels (Manhattan) of that visible location marker; 0xfff0+n = aligned
    // within ~4 degrees of compass point n (0 N .. 7 NW) out from the player
    // marker's tip; otherwise 0xffff.
    pub(crate) fn map_mouse_hover_tracker(&mut self) {
        let x = self.mouse_pos_x as i16;
        let y = self.mouse_pos_y as i16;
        // = seg000:4586 call map_window_contains_point (seg000:5d1d, the
        //   half-open data_046e3_rect test); 4589 di = 0; jnb loc_045d3 —
        //   outside the map window the state is 0.
        let mut hover: u16 = 0;
        if self.map_view_rect.in_rect(x, y) {
            // = seg000:458e al = 0xff (no appearance cap); call
            //   find_nearest_location_marker.
            let (marker, dist) = self.find_nearest_location_marker(0xff, x, y);
            if dist < 9 {
                // = seg000:4593 cmp ax,9; jb loc_045d3 — hover that marker.
                hover = marker;
            } else {
                // = seg000:4598 di = 0xffff — inside the window, no marker.
                hover = 0xffff;
                // = seg000:459b..45a1 dx = map_player_marker_rect.x0; jz —
                //   no "you are here" marker, no compass rays.
                let m = self.map_player_marker_rect;
                if m.x0 != 0 {
                    // = seg000:45a3..45b2 the pointer's delta from the marker
                    //   anchor (rect x0 + 11, rect y1) — the sprite's tip.
                    let dx = x.wrapping_sub(m.x0 + 0xb);
                    let dy = y.wrapping_sub(m.y1);
                    // = seg000:45b4 call compass_angle_from_delta; 45b7 add
                    //   al,3; 45b9..45c4 gate: only within (angle+3)%32 < 6
                    //   of a compass point, else di stays 0xffff.
                    let a = compass_angle_from_delta(dx, dy).wrapping_add(3);
                    if a & 0x1f < 6 {
                        // = seg000:45c6..45d1 di = 0xfff0 | the point index
                        //   (the angle's top 3 bits: 0 N, 1 NE, ... 7 NW).
                        hover = 0xfff0 | (a >> 5) as u16;
                    }
                }
            }
        }
        // = seg000:45d3 xchg ax,[data_046fc]; cmp; jnz map_draw_hover_label —
        //   on change redraw the hover label strip.
        let prev = std::mem::replace(&mut self.data_046fc, hover);
        if prev != hover {
            self.map_draw_hover_label(hover);
        }
    }

    // = seg000:45de map_draw_hover_label — draw the map hover label strip to
    // the front buffer in the small font. hover = a location ptr: its type
    // string + name ("Sietch: Tuono-Tabr"); 0xfff0+n: "DESERT" + the
    // compass-point phrase; 0xffff: "DESERT" alone; 0: nothing (and the
    // select-destination caption typewriter is re-armed; every other state
    // removes it). The strip is space-padded past x 0xed, erasing the
    // previous label.
    fn map_draw_hover_label(&mut self, hover: u16) {
        // = seg000:45de push [framebuffer_active_seg]; 45e2 call
        //   set_screen_as_active_framebuffer — draw to the front buffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:45e5 call call_restore_cursor — lift the cursor off the
        //   strip; 45e8 call font_select_small_font.
        self.call_restore_cursor();
        self.font_select_small_font();
        // = seg000:45eb..45fe pen (0x55, 0x22), colour word 0xf5fe; in
        //   ornithopter mode the pen drops to y 0x26 and the background byte
        //   becomes 0x20 (cockpit grey).
        let x = 0x55;
        let (y, color) = if self.map_ornithopter_mode == 0 {
            (0x22, 0xf5fe)
        } else {
            (0x26, 0x20fe)
        };
        // = seg000:4600 or di,di; jz loc_0462a.
        if hover == 0 {
            // = seg000:462a call map_add_select_destination_text_task —
            //   re-arm the caption typewriter; 462d/4631 set the colour word
            //   and the pen for the erasing space-pad below.
            self.map_add_select_destination_text_task();
            self.font_state.color = color;
            self.font_set_draw_position(x, y);
        } else {
            // = seg000:4604 call map_remove_select_destination_text_task.
            self.map_remove_select_destination_text_task();
            if hover < 0xfff0 {
                // = seg000:4636..463e a location: its type string, then the
                //   name at the advanced pen.
                let index = location_index_from_ptr(hover);
                self.draw_string_location_type(index, color, x, y);
                let (px, py) = self.font_get_draw_position();
                self.draw_location_name(index, color, px, py);
            } else {
                // = seg000:460c ax = 0xa4 "DESERT"; call font_draw_phrase_or_
                //   command_string_with_color_at_pos.
                self.font_draw_phrase_or_command_string_with_color_at_pos(0xa4, color, x, y);
                // = seg000:4612..4618 di -= 0xfff0; only the compass states
                //   (di < 8, not the plain-window 0xffff) name a direction.
                let dir = hover - 0xfff0;
                if dir < 8 {
                    // = seg000:461a..4625 a space, then the compass-point
                    //   phrase 0xda + n ("northwards", ...).
                    self.font_draw_glyph(0x20);
                    self.font_draw_phrase_or_command_string(dir + 0xda);
                }
            }
        }
        // = seg000:4641..464f space-pad until the pen passes x 0xed, erasing
        //   the rest of the previous label.
        while self.font_state.x <= 0xed {
            self.font_draw_glyph(0x20);
        }
        // = seg000:4651 pop [framebuffer_active_seg]; 4655 jmp draw_mouse.
        self.active_fb = saved;
        self.draw_mouse();
        // DOS draws straight to the visible A000 buffer; the port publishes
        // the touched screen (cf. tick_map_caption).
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:456c map_hover_narration_clip — map a hover state to its
    // narration voice clip index: a location ptr gives 0x2bc +
    // (first_name - 1) * 16 + last_name; the 0xffXX desert states add 0x2bc
    // with 16-bit wrap (0xffff -> 0x2bb, 0xfff0+n -> 0x2ac+n).
    fn map_hover_narration_clip(&self, hover: u16) -> u16 {
        // = seg000:456e cmp ah,0ffh; jz loc_04582 — the desert states pass
        //   through unchanged.
        let ax = if hover & 0xff00 == 0xff00 {
            hover
        } else {
            // = seg000:4573 ax = the location's first_name/last_name word;
            //   4575 dec ax; 4576..4580 al = (first_name - 1) << 4 |
            //   last_name, ah = 0.
            let loc = &self.locations[location_index_from_ptr(hover)];
            ((loc.first_name as u16 - 1) << 4) | loc.last_name as u16
        };
        // = seg000:4582 add ax,2bch.
        ax.wrapping_add(0x2bc)
    }

    // = seg000:450e map_mouse_lmb_select_destination — [si+2] of
    // mouse_handlers_01ac8, the LMB press: select the hovered travel
    // destination. Narrates the destination with the score ducked while the
    // hover label blinks 9 times, then enters the travel-confirm chain.
    pub(crate) fn map_mouse_lmb_select_destination(&mut self) {
        // = seg000:450e test [game_screen_mode_flags],0fh; jz ret — only in a
        //   map/travel mode.
        if self.game_screen_mode_flags & 0x0f == 0 {
            return;
        }
        // = seg000:4515/4516 push bx/dx — the click screen position rides the
        //   stack past the blink loop into the confirm chain (the pointer may
        //   move meanwhile).
        let x = self.mouse_pos_x as i16;
        let y = self.mouse_pos_y as i16;
        // = seg000:4517 call map_mouse_hover_tracker — refresh the hover
        //   state; 451c di = [data_046fc].
        self.map_mouse_hover_tracker();
        let hover = self.data_046fc;
        // = seg000:4520 or di,di; jz ret — outside the map window.
        if hover == 0 {
            return;
        }
        // = seg000:4524 js loc_04534 — the desert states (0xffff / 0xfff0+n)
        //   are always selectable; a location marker also needs either a
        //   mode-3 bit (= seg000:4526) or to differ from the player's current
        //   location (= seg000:452d cmp di,[current_location_ptr]; jnz).
        if hover & 0x8000 == 0
            && self.game_screen_mode_flags & 3 == 0
            && self.current_location_index != 0xffff
            && hover == location_ptr(self.current_location_index)
        {
            return;
        }
        // = seg000:4537 call map_hover_narration_clip; 453a call
        //   duck_music_and_start_narration_voice_clip.
        let clip = self.map_hover_narration_clip(hover);
        self.duck_music_and_start_narration_voice_clip(clip);
        // = seg000:453e cx = 9 — blink the hover label: 0x14 frame-task ticks
        //   erased (a hover-0 redraw), 0x0a ticks drawn.
        for _ in 0..9 {
            // = seg000:4543/4546 ax=14h; call wait_processing_frame_tasks.
            for _ in 0..0x14 {
                self.tick_one_frame();
            }
            // = seg000:454a xor di,di; call map_draw_hover_label.
            self.map_draw_hover_label(0);
            // = seg000:4550/4553 ax=0ah; call wait_processing_frame_tasks.
            for _ in 0..0x0a {
                self.tick_one_frame();
            }
            // = seg000:4556 call map_draw_hover_label with the selection.
            self.map_draw_hover_label(hover);
        }
        // = seg000:455e call wait_for_narration_voice_clip.
        self.wait_for_narration_voice_clip();
        // = seg000:4564 [data_04732] = 80h — arm the takeoff for the
        //   departure transition.
        self.data_04732 = 0x80;
        // = seg000:4569 jmp map_confirm_travel_and_close.
        self.map_confirm_travel_and_close(hover, x, y);
    }

    // = seg000:b5f9 map_screen_to_position — the inverse of
    // map_position_to_screen: convert a screen point inside the map window to
    // a map position (longitude units, latitude row).
    fn map_screen_to_position(&self, x: i16, y: i16) -> (u16, i16) {
        // = seg000:b5f9/b5fd subtract the window centre (data_0dcf6 /
        //   data_0dcf8; the port derives it from map_view_rect the way
        //   map_position_to_screen does).
        let r = self.map_view_rect;
        let centre_x = r.x0 + (r.x1 - r.x0) / 2;
        let centre_y = r.y0 + (r.y1 - r.y0 - 1) / 2;
        let dx = x - centre_x;
        let dy = y - centre_y;
        // = seg000:b603 the full-globe branch scales both deltas down by 4
        //   and offsets by the band's rotated offset (the tablat +4 word).
        //   TODO: port with the full-globe view.
        if self.data_046eb & 0x80 != 0 {
            println!("map_screen_to_position: full-globe mode not ported");
            return (self.zoomed_globe_longitude, self.zoomed_globe_latitude);
        }
        // = seg000:b60c/b60e the latitude row.
        let lat = dy + self.zoomed_globe_latitude;
        // = seg000:b612..b621 the row's tablat entry (indexed by |lat|; the
        //   port's +98 index resolves to the same mirrored entry).
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        // = seg000:b62f/b632 cx = [si+2] << 1 — the row byte length.
        let len = tablat.len((lat + 98) as u16) as i32;
        // = seg000:b62c dx += [si+6] — the row's cached view-centre cell (the
        //   scratch map_copy_window_row fills); the port recomputes the same
        //   truncating len * lng >> 16 cell.
        let cell = ((len as u32 * self.zoomed_globe_longitude as u32) >> 16) as i32 + dx as i32;
        // = seg000:b634..b63e wrap the cell into [0, len).
        let cell = if cell < 0 {
            cell + len
        } else if cell >= len {
            cell - len
        } else {
            cell
        };
        // = seg000:b640/b642 the dx:0 div — longitude = cell * 0x10000 / len.
        ((((cell as u32) << 16) / len as u32) as u16, lat)
    }

    // = seg000:5133 compass_angle_to_map_position — the compass angle from
    // the player map position (x, lat) to a target map position: both deltas
    // are halved once when the latitude delta is outside [-0x80, 0x80), then
    // the latitude delta is scaled by 256 onto the longitude scale (65536
    // units per circle vs ~197 rows) and the shared compass math runs.
    pub(crate) fn compass_angle_to_map_position(
        &self,
        target_x: u16,
        target_lat: i16,
        x: u16,
        lat: i16,
    ) -> u8 {
        // = seg000:5133..5139 the target - player deltas.
        let mut dlat = target_lat.wrapping_sub(lat);
        let mut dx = target_x.wrapping_sub(x) as i16;
        // = seg000:513b..5148 halve once outside [-0x80, 0x80) so the << 8
        //   below cannot overflow (latitude spans ±98 rows).
        if !(-0x80..0x80).contains(&dlat) {
            dlat >>= 1;
            dx >>= 1;
        }
        // = seg000:514a mov bh,bl; xor bl,bl — dlat * 256, then the shared
        //   octant math.
        compass_angle_from_delta(dx, (dlat as i8 as i16) << 8)
    }

    // = seg000:5124 compass_angle_to_location — the compass angle from the
    // player map position to a location's map cell.
    fn compass_angle_to_location(&self, location_index: usize) -> u8 {
        // = seg000:5125 call get_map_position; 5128/512b cx/di = the
        //   location's map_y/map_x; 512e the shared angle math.
        let (x, lat) = self.get_map_position();
        let loc = &self.locations[location_index];
        self.compass_angle_to_map_position(loc.map_x as u16, loc.map_y, x, lat)
    }

    // = seg000:5119 adjust_travel_heading — travel_heading += delta, and
    // re-seed the travel step accumulator to half a cell.
    fn adjust_travel_heading(&mut self, delta: u8) {
        // = seg000:5119 add [travel_heading], al.
        self.travel_heading = self.travel_heading.wrapping_add(delta);
        // = seg000:511d travel_step_accum = 0x80.
        self.travel_step_accum = 0x80;
    }

    // = seg000:4944 arm_pending_travel — arm the pending travel from the
    // selected hover state and the click screen position.
    fn arm_pending_travel(&mut self, hover: u16, x: i16, y: i16) {
        // = seg000:4944 call loc_050be — travel_no_location_dest = 0.
        self.travel_no_location_dest = 0;
        // = seg000:4947 cmp di,0fff0h; jb travel_aim_at_location — a
        //   location ptr aims home at it.
        if hover < 0xfff0 {
            self.travel_aim_at_location(location_index_from_ptr(hover));
            return;
        }
        // = seg000:494c dec [travel_no_location_dest] — 0xff marks a directional
        //   flight with no location destination; the map verbs switch on it.
        self.travel_no_location_dest = self.travel_no_location_dest.wrapping_sub(1);
        // = seg000:4950 call map_screen_to_position — the clicked map
        //   cell; 4953/4955 it becomes the target; 4957/495a aim at it
        //   from the player position.
        let (tx, tlat) = self.map_screen_to_position(x, y);
        let (px, plat) = self.get_map_position();
        let angle = self.compass_angle_to_map_position(tx, tlat, px, plat);
        // = seg000:495d di = [last_location_ptr]; 4961 cl = 1; 4963 jmp
        //   travel_commit_destination.
        self.travel_commit_destination(location_ptr(self.last_location_index as u16), 1, angle);
    }

    // = seg000:4965 travel_aim_at_location — aim the travel at a location:
    // the compass angle from the player position, homing mode.
    fn travel_aim_at_location(&mut self, location_index: usize) {
        // = seg000:4965 call compass_angle_to_location; 4968 cl = 0.
        let angle = self.compass_angle_to_location(location_index);
        self.travel_commit_destination(location_ptr(location_index as u16), 0, angle);
    }

    // = seg000:496a travel_commit_destination — commit the destination, the
    // heading mode and a zeroed heading; the tail jmp adjust_travel_heading
    // lands the angle in the heading and re-seeds the step accumulator.
    fn travel_commit_destination(&mut self, dest: u16, mode: u8, angle: u8) {
        self.travel_destination_ptr = dest;
        self.travel_heading_mode = mode;
        self.travel_heading = 0;
        self.adjust_travel_heading(angle);
    }

    // = seg000:41c5 ungrey_skip_to_destination_verb — clear the map verbs'
    // heading-adjust accumulator and the SKIP TO DESTINATION verb flags.
    fn ungrey_skip_to_destination_verb(&mut self) {
        // = seg000:41c5 data_04726 = 0; 41ca al = 0 falls into 41cc.
        self.data_04726 = 0;
        self.set_skip_to_destination_verb_flags(0);
    }

    // = seg000:41cc set_skip_to_destination_verb_flags — store the flags byte
    // into the SKIP TO DESTINATION command template (data_021fd; 0x40 =
    // greyed) and into the live command_menu_buf copy when its first record
    // is that verb.
    fn set_skip_to_destination_verb_flags(&mut self, flags: u8) {
        // = seg000:41cc mov [data_021fd], al — the template byte (the port
        //   applies it when build_room_command_records copies the template).
        self.cmd_skip_to_destination_flags = flags;
        // = seg000:41cf..41d7 cmp [data_01f12], 4ffbh — command_menu_buf's
        //   first record (seg001:1f12 is its handler word); patch its flags
        //   byte (data_01f11) when it is the SKIP TO DESTINATION verb.
        if let Some(rec0) = self.command_menu_buf.records.first_mut() {
            if rec0.handler == 0x4ffb {
                rec0.text_id = (rec0.text_id & 0x00ff) | ((flags as u16) << 8);
            }
        }
    }

    // = seg000:40d5 run_travel_departure_npc_scans — the NPC half of a travel
    // departure: the room-leave dialogue scan and the boarding companions.
    fn run_travel_departure_npc_scans(&mut self) {
        // = seg000:40d5 pending_room_action = 7 — arm the leave
        //   scan's action code.
        self.pending_room_action = 7;
        // = seg000:40da call run_room_leave_dialogue_scan.
        self.run_room_leave_dialogue_scan();
        // = seg000:40dd call loc_04ac4 — data_011ca = 0.
        self.data_011ca = 0;
        // = seg000:40e0/40e3 the companion-detach scan.
        self.scan_matching_room_person_entries(Self::npc_travel_detach_companion);
    }

    // = seg000:40e6 NPC_travel_detach_companion — room-person scan callback
    // for a travel departure: a companion in the room (flags 0x40 and 2 both
    // set) loses the companion flag and its HUD slot updates.
    fn npc_travel_detach_companion(&mut self, index: u8, entry: &RoomPerson) {
        // = seg000:40e6/40ec both flag bits gate the detach.
        if entry.flags & 0x40 == 0 || entry.flags & 2 == 0 {
            return;
        }
        // = seg000:40f2 call npc_clear_travelling — drop the travelling flag
        //   and the persons_travelling_with bit.
        self.npc_clear_travelling(index as usize);
        // = seg000:40f5 call npc_remove_companion_slot — vacate the person's
        //   companion HUD slot (ui_hud_companion_1/2) and redraw.
        self.npc_remove_companion_slot(index as usize);
    }

    // = seg000:4b3b travel_advance_step — one travel movement step: the
    // 16-step event clock, the position advance along travel_heading, and the
    // companion NPC move; the new position encoding is committed to
    // location_and_room / location_appearance.
    fn travel_advance_step(&mut self) {
        // = seg000:4b3b/4b3f every 16th step runs one time period of events.
        self.travel_step_counter = self.travel_step_counter.wrapping_add(1);
        if self.travel_step_counter & 0x0f == 0 {
            // = seg000:4b47/4b4a cx=1; call run_events_for_n_time_periods
            //   (seg000:0fd9) — advance one time period of events.
            self.run_events_for_n_time_periods(1);
        }
        // = seg000:4b4d call get_map_position; 4b50 call travel_step_position
        //   — advance the position one step along travel_heading.
        let (x, lat) = self.get_map_position();
        let (nx, nlat) = self.travel_step_position(x, lat);
        // = seg000:4b53 the companions follow to the stepped position.
        self.move_all_npcs_whose_bit_6_of_flags_is_set(nx, nlat as u16);
        // = seg000:4b56/4b5a commit the stepped encoding (dx = longitude,
        //   bx = the signed latitude row).
        self.location_and_room = nx;
        self.location_appearance = nlat as u16;
    }

    // = seg000:51cb travel_update_heading — re-aim travel_heading before a
    // step from the position (x, lat). In fixed-heading mode
    // (travel_heading_mode or travel_no_location_dest) only the polar guard applies: past
    // |lat| 0x4d a poleward heading is bent to due east/west. In homing mode
    // the heading re-aims at travel_destination_ptr.
    fn travel_update_heading(&mut self, x: u16, lat: i16) {
        // = seg000:51cb/51d2 either flag selects the fixed-heading path.
        if self.travel_no_location_dest != 0 || self.travel_heading_mode != 0 {
            // = seg000:51d9..51e1 within lat -0x4d..0x4d nothing to do.
            if (-0x4d..=0x4d).contains(&lat) {
                return;
            }
            // = seg000:51e3..51ed ah = heading - 0x40 (sign set = a
            //   northward-ish heading); xor with the latitude sign byte — a
            //   set sign means the heading points away from the near pole, so
            //   it stays.
            let al = self.travel_heading;
            let ah = al.wrapping_sub(0x40);
            let bh = (lat >> 8) as u8;
            if (ah ^ bh) & 0x80 != 0 {
                return;
            }
            // = seg000:51ef..5202 bend to due east/west on the heading's side.
            self.travel_heading = (al & 0x80) | 0x40;
        } else {
            // = seg000:51f5..5202 homing: re-aim at the destination
            //   (compass_angle_to_location with the position regs); the
            //   zero-delta carry (standing on the destination) keeps the
            //   heading.
            let loc = &self.locations[location_index_from_ptr(self.travel_destination_ptr)];
            let (tx, tlat) = (loc.map_x as u16, loc.map_y);
            if tx == x && tlat == lat {
                return;
            }
            self.travel_heading = self.compass_angle_to_map_position(tx, tlat, x, lat);
        }
    }

    // = seg000:5206 travel_step_position — advance the desert position (x =
    // longitude, lat = latitude row) one step along travel_heading:
    // travel_update_heading re-aims, travel_heading_deltas splits the heading
    // into per-step deltas (0x20 = one full step on the major axis), both
    // scaled by the row's lng_units_per_cell. travel_step_accum accumulates
    // fractional latitude (0x100 = one row); past 0x1ff the longitude is
    // rescaled to a single-row step. Past |lat| 0x60 the pole flips: heading
    // += 0x80 and longitude += 0x8000.
    fn travel_step_position(&mut self, x: u16, lat: i16) -> (u16, i16) {
        // = seg000:5206 call travel_update_heading.
        self.travel_update_heading(x, lat);
        // = seg000:5209 al = travel_heading.
        let heading = self.travel_heading;
        // = seg000:520e..5214 bp = lng_units_per_cell[|lat|] (the port's +98
        //   index resolves to the same mirrored entry).
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        let per_cell = tablat.lng_units_per_cell((lat + 98) as u16) as i32;
        // = seg000:5218 call travel_heading_deltas.
        let (ddx, dlat) = travel_heading_deltas(heading);
        // = seg000:521b..5227 cx = 0x20: the longitude delta = per_cell *
        //   ddx / 32 and the scaled latitude delta = dlat * per_cell / 32
        //   (both truncating idiv).
        let lng_delta = (per_cell * ddx as i32) / 0x20;
        let lat_scaled = (dlat as i32 * per_cell) / 0x20;
        // = seg000:522d..5233 the accumulator gains |lat_scaled|.
        let acc = lat_scaled.unsigned_abs() as u16 + self.travel_step_accum;
        // = seg000:5237 cmp ah,1; jbe loc_0524e — under 0x200 the whole rows
        //   (ah) step and the fraction (al) stays.
        let (rows, frac, lng) = if acc < 0x200 {
            ((acc >> 8) as i16, acc & 0xff, lng_delta)
        } else {
            // = seg000:523c..524b past 0x1ff the step caps at exactly one
            //   row and the longitude shrinks proportionally
            //   (lng_delta * 0x100 / acc, sign-preserving), no leftover.
            (1, 0, (lng_delta * 0x100) / acc as i32)
        };
        // = seg000:524e the fraction back to the accumulator (byte store).
        self.travel_step_accum = frac;
        // = seg000:5254..5258 the row step takes the latitude delta's sign.
        let rows = if lat_scaled < 0 { -rows } else { rows };
        // = seg000:525a..525e the stepped position.
        let new_lat = lat + rows;
        let mut new_x = x.wrapping_add(lng as u16);
        // = seg000:5260..526f the pole flip: past lat -0x60..0x5f the heading
        //   reverses and the longitude jumps to the antipode.
        if !(0..0xc0).contains(&(new_lat + 0x60)) {
            self.travel_heading = self.travel_heading.wrapping_add(0x80);
            new_x = new_x.wrapping_add(0x8000);
        }
        (new_x, new_lat)
    }

    // ---- The in-game flight (the travel pump and its minimap) --------------

    // = seg000:4f0c travel_pump — the in-game travel pump, run once per
    // game_loop pass (seg000:d851): drive one flight-HNM frame every pass and
    // one travel step every 0x300 ticks, maintaining the minimap trail and
    // checking arrival.
    pub(crate) fn travel_pump(&mut self) {
        // = seg000:4f0c/4f13 the gates: a travel is active and no room swap
        //   is pending.
        if self.travel_active == 0 || self.data_011ca != 0 {
            return;
        }
        // = seg000:4f1a re-arm the game-area hotspot (HUD element 20).
        self.ui_elements[20].flags = 0x80;
        // = seg000:4f20..4f24 push 0dbech; call hnm_do_frame — one flight-HNM
        //   frame (the pushed word is the decoder's return trampoline). The
        //   MNT clips carry resource flag bit 4, so DOS presents each decoded
        //   frame through hnm_present_flight_frame (from hnm_decode_video_
        //   frame, seg000:ccee); the port runs it after the frame advances.
        if self.hnm_do_frame() {
            self.hnm_present_flight_frame();
        }
        // = seg000:4f27..4f31 one travel step every 0x300 ticks.
        let now = self.game_ticks() as u16;
        if now.wrapping_sub(self.travel_step_tick_stamp) < 0x300 {
            return;
        }
        // = seg000:4f34/4f37 restamp.
        self.travel_step_tick_stamp = now;
        // = seg000:4f3a the step; 4f3d stamp the previous position as a
        //   permanent trail dot; 4f40/4f43 append the new position.
        self.travel_advance_step();
        self.travel_trail_stamp_last();
        let (x, lat) = self.get_map_position();
        self.travel_trail_append(x, lat);
        // = seg000:4f46..4f50 arrived when the position's map offset (di from
        //   map_func) equals the destination's cached map_offset.
        let offset = self.map_position_to_offset(x, lat).0;
        let dest = location_index_from_ptr(self.travel_destination_ptr);
        if offset == self.locations[dest].map_offset as usize {
            // = seg000:4fb0 loc_04fb0 — the arrival.
            self.travel_arrive();
            return;
        }
        // = seg000:4f52 call loc_02e52 — the post-step settle. It opens by
        //   calling loc_035ad (unconditionally): during a travel that runs the
        //   mode != 0 branch (loc_035e9), which re-detects a passed location
        //   and raises the fly-over companion cabin. The remaining loc_02e52
        //   tail (2e5b onward: the auto-action / head-raise branches) bails in
        //   flight, so only the game-clock stamp is modelled after it.
        self.travel_settle_companion_dispatch();
        self.game_clock_tick_base = self.game_ticks() as u16;
        // = seg000:4f55 a staged night attack pauses the flight side effects.
        if self.data_047a7 != 0 {
            return;
        }
        // = seg000:4f5c..4f6d the minimap upkeep — skipped entirely while
        //   hidden (bit 0x80); state 1 is consumed as a recenter + redraw.
        if self.travel_minimap_state >= 0 {
            if self.travel_minimap_state != 0 {
                // = seg000:4f65..4f6d consume: state 0, recenter, redraw.
                self.travel_minimap_state = 0;
                self.set_zoomed_globe_pos_from_map_position();
                self.travel_refresh_view();
            }
            // = seg000:4f70..4f76 the position on the minimap; off-window
            //   skips to the probe.
            if let Some((sx, sy)) = self.map_position_to_screen_if_visible(x, lat) {
                // = seg000:4f78..4f8c outside the inner inset bounds the
                //   view schedules a recenter for the next step.
                if !(0xd6..0x132).contains(&sx) || !(0xa..0x36).contains(&sy) {
                    // = seg000:4f8e travel_minimap_state = 1.
                    self.travel_minimap_state = 1;
                } else if self.data_011ca == 0 {
                    // = seg000:4f95..4faa the live position marker (ICONES
                    //   0x30) into the back buffer at (-1, -1).
                    self.open_icones_spritesheet();
                    self.set_backbuffer_as_frame_buffer();
                    self.draw_active_bank_sprite(0x30, sx - 1, sy - 1);
                    self.set_fb1_as_active_framebuffer();
                }
            }
        }
        // = seg000:4fad jmp travel_probe_terrain_ahead — re-pick the flight
        //   clip from the terrain ahead.
        self.travel_probe_terrain_ahead();
    }

    // = seg000:4fb0 loc_04fb0 — the travel arrival: tear the flight down and
    // commit entry into the destination.
    fn travel_arrive(&mut self) {
        // = seg000:4fb0 disarm the game-area hotspot.
        self.ui_elements[20].flags = 0;
        // = seg000:4fb6..4fc0 data_04732 = mode & 1 — an orni travel leaves
        //   the arrival-overlay SAL bit armed and keeps the flight HNM open;
        //   otherwise the HNM closes here.
        let al = self.game_screen_mode_flags & 1;
        self.data_04732 = al;
        if al == 0 {
            self.hnm_close();
        }
        self.travel_finish_at_destination();
    }

    // = seg000:4fc3 travel_finish_at_destination — the shared travel-finish
    // tail (the arrival above and SKIP TO DESTINATION both land here): tear
    // the travel state down and enter the destination — so an unfinished skip
    // still lands at the destination's cell.
    fn travel_finish_at_destination(&mut self) {
        // = seg000:4fc3/4fc6 re-seed the person marker base.
        self.person_marker_base = (self.rand() & 0xff) as u8;
        // = seg000:4fc9..4fd2 disarm the pump and take the mode flags.
        self.travel_active = 0;
        let mode = std::mem::take(&mut self.game_screen_mode_flags);
        // = seg000:4fd2..4fdc an ornithopter arrival (mode & 3 == 1) lands
        //   the orni on the destination's pad.
        if mode & 3 == 1 {
            let dest = location_index_from_ptr(self.travel_destination_ptr);
            self.locations[dest].equipment.ornithopters =
                self.locations[dest].equipment.ornithopters.wrapping_add(1);
        }
        // = seg000:4fdf loc_04ac4 — data_011ca = 0.
        self.data_011ca = 0;
        // = seg000:4fe2 call call_restore_cursor.
        self.call_restore_cursor();
        // = seg000:4fe5 call ui_setup_nav_panel — back to the room panel.
        self.ui_setup_and_draw_nav_panel();
        // = seg000:4fe8..4ff2 the destination's (map_x, map_y), then clear
        //   the destination; 4ff8 jmp loc_04002 — the desert arrival check
        //   enters the location's room 1 and reloads the scene.
        let dest = location_index_from_ptr(self.travel_destination_ptr);
        let (dx, bx) = (
            self.locations[dest].map_x as u16,
            self.locations[dest].map_y as u16,
        );
        self.travel_destination_ptr = 0;
        self.desert_check_arrival(dx, bx);
    }

    // = seg000:4ffb menu_callback_choice_skip_to_destination — the SKIP TO
    // DESTINATION map-mode verb: fast-forward the travel to the destination
    // (or into the first hostile cell along the way).
    pub(crate) fn menu_callback_choice_skip_to_destination(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:4ffb disarm the game-area hotspot (HUD element 20).
        self.ui_elements[20].flags = 0;
        // = seg000:5001 call hnm_close_resource — the flight HNM closes.
        self.hnm_close();
        // = seg000:5004 travel_heading_mode = 0 — home on the destination.
        self.travel_heading_mode = 0;
        // = seg000:5009 cx = 0c8h — fast-forward up to 200 steps.
        for _ in 0..0xc8 {
            // = seg000:500d call travel_advance_step.
            self.travel_advance_step();
            // = seg000:5010..501d arrived when the stepped position's map
            //   offset (map_func di) equals the destination's cached
            //   map_offset ([si+6]).
            let (x, lat) = self.get_map_position();
            let offset = self.map_position_to_offset(x, lat).0;
            let dest = location_index_from_ptr(self.travel_destination_ptr);
            if offset == self.locations[dest].map_offset as usize {
                // = seg000:5039 pop cx; jmp travel_finish_at_destination.
                return self.travel_finish_at_destination();
            }
            // = seg000:501f pending_room_action = 0; 5024 the hostile-zone
            //   check; 5027/502d loopz — keep stepping while no action armed.
            self.pending_room_action = 0;
            self.travel_route_hostile_zone_check();
            if self.pending_room_action != 0 {
                // = seg000:5031 data_04726 += 20h — re-add the accumulator
                //   step the check just consumed.
                self.data_04726 = self.data_04726.wrapping_add(0x20);
                // = seg000:5036 jmp loc_02e52 — the post-step settle:
                //   finish_room_screen_setup (loc_035ad, the port stub that
                //   would run the armed hostile-zone warning) and the
                //   game-clock stamp; the dialogue/lip-sync tail bails in a
                //   travel mode (seg000:2e7d).
                self.finish_room_screen_setup();
                self.game_clock_tick_base = self.game_ticks() as u16;
                return;
            }
        }
        // = seg000:502f jz travel_finish_at_destination — 200 steps without
        //   arrival or action still land the travel at the destination.
        self.travel_finish_at_destination();
    }

    // = seg000:497a menu_callback_choice_change_destination — the CHANGE
    // DESTINATION map-mode verb: reopen the map main view over the flight.
    pub(crate) fn menu_callback_choice_change_destination(&mut self, _text_id: u16, _index: usize) {
        // = seg000:497a call reset_scene_lip_sync_state.
        self.reset_scene_lip_sync_state();
        // = seg000:4980 travel_minimap_state = 1 — the map-screen cleanup's
        //   `> 0` gate re-enters the flight minimap view on close.
        self.travel_minimap_state = 1;
        // = seg000:497d bp = menu_multiple_cancel; 4985 jmp map_screen_open.
        self.map_screen_open(menu_defs::MENU_CANCEL.records.to_vec());
    }

    // = seg000:50a5 menu_callback_choice_back_to_starting_point — the BACK TO
    // STARTING POINT map-mode verb: aim the travel home at the starting
    // location.
    pub(crate) fn menu_callback_choice_back_to_starting_point(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:50a5/50a8 reopen the current flight clip at its first
        //   frame.
        self.hnm_load_first_frame_by_id(self.hnm_video_id, 0);
        // = seg000:50ab di = [last_location_ptr]; 50af call get_map_position;
        //   50b2 call travel_aim_at_location.
        self.travel_aim_at_location(self.last_location_index);
        // = seg000:50b5 call loc_04ac4 — data_011ca = 0.
        self.data_011ca = 0;
        // = seg000:50b8 call loc_050be — travel_no_location_dest = 0 (back to the homing
        //   verb pair).
        self.travel_no_location_dest = 0;
        // = seg000:50bb jmp ui_draw_room_command_panel.
        self.ui_draw_room_command_panel();
    }

    // = seg000:50c4 menu_callback_choice_towards_nearest_place — the TOWARDS
    // NEAREST PLACE map-mode verb: aim the travel at the nearest location.
    pub(crate) fn menu_callback_choice_towards_nearest_place(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:50c4 call get_map_position; 50c7 call iterate_over_
        //   locations_and_coordinates.
        let (x, lat) = self.get_map_position();
        let nearest = self.iterate_over_locations_and_coordinates(x, lat);
        // = seg000:50ca call arm_pending_travel — di is a location ptr, so
        //   the click-coordinate arguments go unused.
        self.arm_pending_travel(location_ptr(nearest as u16), 0, 0);
        // = seg000:50cd call loc_04ac4 — data_011ca = 0.
        self.data_011ca = 0;
        // = seg000:50d0 travel_heading_mode = 0 (already 0 from the location
        //   path of arm_pending_travel).
        self.travel_heading_mode = 0;
        // = seg000:50d5 call loc_050be — travel_no_location_dest = 0.
        self.travel_no_location_dest = 0;
        // = seg000:50d8 jmp ui_draw_room_command_panel.
        self.ui_draw_room_command_panel();
    }

    // = seg000:5344 iterate_over_locations_and_coordinates — the nearest
    // non-hidden location to the map position (x = longitude, lat).
    fn iterate_over_locations_and_coordinates(&self, x: u16, lat: i16) -> usize {
        // = seg000:5345 bp = 0ffffh — the best distance so far.
        let mut best_dist = u16::MAX;
        let mut best = 0;
        // = seg000:5348/534b the walk to the table's 0xffff fence.
        for (i, loc) in self.locations.iter().enumerate() {
            // = seg000:5350 test [si+0ah],80h — skip hidden locations.
            if loc.status & 0x80 != 0 {
                continue;
            }
            // = seg000:5356..535d cx = |map_x - dx|; 535f..5366 ax =
            //   |map_y - bx|.
            let dlng = loc.map_x.wrapping_sub(x as i16).unsigned_abs();
            let dlat = loc.map_y.wrapping_sub(lat).unsigned_abs();
            // = seg000:5368..5370 cl = the longitude delta's high byte
            //   (mov cl,ch), then the byte compare `cmp cl,al` keeps it when
            //   it is at least the latitude delta's LOW byte, else the FULL
            //   16-bit latitude delta wins.
            let dlng_cells = dlng >> 8;
            let dist = if dlng_cells as u8 >= dlat as u8 {
                dlng_cells
            } else {
                dlat
            };
            // = seg000:5372..5378 strictly closer only.
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        best
    }

    // = seg000:4182 travel_route_hostile_zone_check — the per-step hostile-
    // zone check along an ornithopter travel route: grey/un-grey SKIP TO
    // DESTINATION and arm the hostile-zone warning (pending_room_action 4)
    // when flying over fully hostile terrain.
    fn travel_route_hostile_zone_check(&mut self) {
        // = seg000:4182..4189 only during an ornithopter travel (mode & 3
        //   == 1).
        if self.game_screen_mode_flags & 3 != 1 {
            return;
        }
        // = seg000:418b a fixed-heading (desert) travel skips the
        //   destination test.
        if self.travel_no_location_dest == 0 {
            // = seg000:4192/4196/4199 a homing travel to an Atreides
            //   destination is safe.
            let dest = location_index_from_ptr(self.travel_destination_ptr);
            if self.location_is_atreides(dest) {
                return self.ungrey_skip_to_destination_verb();
            }
        }
        // = seg000:419b..41a5 the terrain byte at the current position:
        //   hostile only when both 0x30 bits are set.
        let (x, lat) = self.get_map_position();
        if self.read_map_byte(x, lat) & 0x30 != 0x30 {
            return self.ungrey_skip_to_destination_verb();
        }
        // = seg000:41a7..41ae arm the hostile-zone warning (action code 4)
        //   when the accumulator is empty.
        if self.data_04726 == 0 {
            self.pending_room_action = 4;
        }
        // = seg000:41b3/41b5 grey the SKIP TO DESTINATION verb.
        self.set_skip_to_destination_verb_flags(0x40);
        // = seg000:41b8..41bf the accumulator steps down by 0x20; on
        //   reaching 0 raise pending_room_screen_request 2.
        self.data_04726 = self.data_04726.wrapping_sub(0x20);
        if self.data_04726 == 0 {
            self.pending_room_screen_request = 2;
        }
    }

    // = seg000:35ad loc_035ad / loc_035e9 — the per-settle companion dispatch,
    // run once per travel step (from loc_02e52). During a travel
    // (game_screen_mode_flags != 0) this is the mode != 0 branch (loc_035e9):
    // it re-detects a location the flight passes near
    // (travel_scan_nearby_location) and the hostile-zone warning
    // (travel_route_hostile_zone_check); if either armed a room action and a
    // companion is aboard, it raises the fly-over cabin — ORNYCAB drawn over
    // the game area plus the companion as a talking head. The mode == 0
    // room-auto-dialogue branch (loc_035b4) is unrelated to travel.
    fn travel_settle_companion_dispatch(&mut self) {
        // = seg000:35e9..35f1 reset the per-frame staging flags.
        self.data_047a7 = 0;
        self.pending_room_action = 0;
        // = seg000:35f4..35fa a staged action (data_047a6) skips this pass.
        let staged = self.data_047a6;
        self.data_047a6 = 0;
        if staged != 0 {
            return;
        }
        // = seg000:35fc without a companion aboard only the hostile-zone
        //   warning runs (loc_03637); the cabin view needs someone to speak.
        if self.companions[0] == -1 && self.companions[1] == -1 {
            self.travel_route_hostile_zone_check();
            return;
        }
        // = seg000:3603 detect a location the flight passes near.
        self.travel_scan_nearby_location();
        // = seg000:3606 the hostile-zone warning.
        self.travel_route_hostile_zone_check();
        // = seg000:3609/360e nothing armed this pass.
        if self.pending_room_action == 0 {
            return;
        }
        // = seg000:3610/3613 pick the companion who speaks.
        let Some(companion) = self.travel_pick_speaking_companion() else {
            return;
        };
        // = seg000:3615 restore the cursor before the cabin overlay.
        self.call_restore_cursor();
        // = seg000:3618 draw the ORNYCAB cabin and raise the companion head.
        self.travel_show_companion_cabin(companion);
        // = seg000:361b push ax — keep the speaking companion across the pause.
        // = seg000:361c ax = 0x4b; call wait_a_bit — a 0x4b PIT-tick beat (a
        //   busy-wait that keeps the idle frame tasks running) before the voice
        //   plays; no deterministic state effect headless.
        // = seg000:3624 call loc_096d8 — the companion speaks the fly-over line.
        // = seg000:3628 jb loc_03636 — no sentence matched: skip the menu install.
        if self.travel_play_flyover_line(companion) {
            // = seg000:362a..3631 si = &room_persons[companion] — loaded for
            //   loc_03551, but its pending_room_action 3/4 branches read
            //   current_lip_sync_resource_id, not si, so the port omits it.
            // = seg000:3633 call loc_03551 — install the GO TOWARDS THIS PLACE
            //   (action 3) or CHANGE DESTINATION / IGNORE WARNING (action 4)
            //   command menu and, for action 4, rebuild the room nav panel.
            self.install_pending_room_action_menu();
        }
    }

    // = seg000:40f9 loc_040f9 — scan the map around the flight for a location
    // it passes near. With companions aboard, sweep the 9×9 block of map cells
    // centred on the player; a qualifying location — one flagged a
    // discoverable landmark (status bit 0x80) already revealed by the story
    // phase, whose bearing sits within ±0x60 of the heading — arms room action
    // 3 and records the location-type caption strings the companion's line
    // substitutes. DOS keeps scanning, so the last match in the block wins.
    fn travel_scan_nearby_location(&mut self) {
        // = seg000:40f9 only while a map/travel sub-mode is active.
        if self.game_screen_mode_flags & 3 == 0 {
            return;
        }
        // = seg000:4101 nothing to say without travelling companions.
        if self.persons_travelling_with == 0 {
            return;
        }
        // = seg000:4108..4111 the 9×9 map-cell block around the player.
        let (x, lat) = self.get_map_position();
        let strip = self.map_build_cell_strip(x, lat, 9, 9);
        // = seg000:4114..417f scan the 81 cells.
        for (byte, offset) in strip {
            // = seg000:4118 not a location cell.
            if byte & 0x40 == 0 {
                continue;
            }
            // = seg000:411c..4123 the location owning this cell.
            let Some(idx) = self.find_location_by_map_offset(offset) else {
                continue;
            };
            let loc = self.locations[idx];
            // = seg000:4125..4131 a discoverable landmark the phase has
            //   revealed.
            if loc.status & 0x80 == 0 {
                continue;
            }
            if self.game_phase < loc.discoverable_at_phase as u8 {
                continue;
            }
            // = seg000:4133..4142 the bearing must be within ±0x60 of the
            //   heading; a location on the player cell (CF set) is skipped.
            let (px, plat) = self.get_map_position();
            if loc.map_x as u16 == px && loc.map_y == plat {
                continue;
            }
            let angle = self.compass_angle_to_location(idx);
            let rel = angle.wrapping_sub(self.travel_heading).wrapping_add(0x60);
            if rel >= 0xc0 {
                continue;
            }
            // = seg000:4144..4152 the caption id + which side of the heading.
            let (side, caption) = if rel < 0x60 {
                (0u8, 0x00ce)
            } else {
                (1u8, 0x00d0)
            };
            self.string_subst_id_table[5] = caption;
            self.data_000e1 = side;
            // = seg000:415a..4160 the location-type string ("Sietch: ", ...).
            self.string_subst_id_table[4] =
                self.get_location_type_string_offset(idx).wrapping_add(0x48);
            // = seg000:4163 arm room action 3 (companions are following).
            self.pending_room_action = 3;
            // = seg000:4168 mark the landmark discovered.
            self.location_mark_discovered(idx);
            // = seg000:416b call arm_pending_travel (di = the found location's
            //   record, so its travel_aim_at_location branch): re-aim the flight
            //   home at the passed location in homing mode. GO TOWARDS THIS
            //   PLACE (menu_callback_choice_exit_menu) then just closes the
            //   fly-over menu; travel_resume_flight_view resumes the flight,
            //   now bound for the new destination.
            self.arm_pending_travel(location_ptr(idx as u16), 0, 0);
            // = seg000:416e call call_restore_cursor.
            self.call_restore_cursor();
            // = seg000:4171 call build_room_command_records — rebuild the
            //   flight strip (command_menu_buf) beneath the fly-over menu:
            //   arm_pending_travel above just re-aimed the flight at the
            //   spotted location and cleared travel_no_location_dest, so the
            //   strip now offers SKIP TO DESTINATION instead of BACK TO
            //   STARTING POINT. GO TOWARDS THIS PLACE's exit pop reveals it.
            self.build_room_command_records();
            // = seg000:4174 call rebuild_and_draw_room_nav_panel.
            self.rebuild_and_draw_room_nav_panel();
            // = seg000:4177 call redraw_active_command_menu.
            self.redraw_active_command_menu();
        }
    }

    // = seg000:366f loc_0366f — pick which companion speaks the fly-over line:
    // the sole companion, or a rand_bits coin-flip between the two. None when
    // no companion is aboard.
    fn travel_pick_speaking_companion(&self) -> Option<u8> {
        // = seg000:3672 both slots empty.
        if self.companions[0] == -1 && self.companions[1] == -1 {
            return None;
        }
        // = seg000:3677..3684 one companion, or rand_bits bit 0x80 chooses.
        let pick = if self.companions[1] == -1 || self.rand_bits & 0x80 != 0 {
            self.companions[0]
        } else {
            self.companions[1]
        };
        Some(pick as u8)
    }

    // = seg000:368b loc_0368b — the fly-over companion cabin. While an
    // ornithopter travel is up (game_screen_mode_flags & 3 == 1) draw the
    // ORNYCAB cabin over the game area and raise `companion` (the speaking
    // companion's person index) as a lip-sync talking head; the worm branch
    // (& 3 == 2) instead re-composites the worm view. A recenter of the
    // minimap is scheduled for when the cabin closes.
    fn travel_show_companion_cabin(&mut self, companion: u8) {
        // = seg000:368e travel_minimap_state |= 1 — a recenter/redraw is due.
        self.travel_minimap_state |= 1;
        // = seg000:3693..36a1 branch on the travel sub-mode.
        match self.game_screen_mode_flags & 3 {
            2 => {
                // = seg000:36cb the worm-travel view (loc_04aeb +
                //   copy_game_rect_fb1_to_fb2). TODO: port the worm cabin.
            }
            1 => {
                // = seg000:36a3/36a8 cockpit mode; a room render is pending.
                self.map_ornithopter_mode = 1;
                self.room_render_flags = 1;
                // = seg000:36ae open ORNYCAB and draw its sprite 0 over the
                //   game area. The cockpit's transparent windshield shows fb1
                //   beneath — the clean upcoming flight frame, minimap-free,
                //   because the streaming pipeline decoded it over the stamp
                //   (see hnm_present_flight_frame's prefetch).
                self.open_resource_and_draw_sprite0(sprite_bank::ORNYCAB);
                // = seg000:36b3 fold the ORNYCAB palette in.
                self.update_screen_palette();
                // = seg000:36b6 snapshot the cabin as the head's backdrop.
                self.copy_active_framebuffer_to_framebuffer_2();
                // = seg000:36ba..36c1 setup the companion talking head
                //   (skipped for an empty slot); setup_talking_head bundles
                //   the DOS current_lip_sync_resource_id store +
                //   start_room_lip_sync.
                if (companion as i8) >= 0 {
                    self.current_lip_sync_resource_id = companion as u16;
                    // = seg000:36c1 call start_room_lip_sync, which opens with
                    //   loc_04aca (data_011ca = 1). That pauses travel_pump
                    //   (it bails while data_011ca != 0), so the flight HNM
                    //   stops decoding and the cabin + head is not overdrawn
                    //   while the companion speaks and the fly-over menu is up.
                    //   travel_resume_flight_view (loc_04abe) clears it when the
                    //   menu is dismissed. The rest of start_room_lip_sync (the
                    //   lip-sync data setup + head render) is setup_talking_head.
                    self.data_011ca = 1;
                    self.setup_talking_head(companion, 0);
                }
                // = seg000:36c4 push the composed cabin to the screen.
                self.present_game_area();
            }
            _ => {}
        }
    }

    // = seg000:37f4 loc_037f4 — (re)load the travel flight view: reset the
    // minimap state, set up the minimap + trail into the back buffer, open the
    // flight HNM at its first frame, then save the palette and install the sky
    // palette. Shared by the scene reload (draw_room_scene) and the fly-over
    // resume (travel_resume_flight_view).
    pub(crate) fn travel_load_flight_view(&mut self) {
        // = seg000:37f4 travel_minimap_state = 0.
        self.travel_minimap_state = 0;
        // = seg000:37f9 call travel_minimap_setup; 37fc call travel_trail_redraw.
        self.travel_minimap_setup();
        self.travel_trail_redraw();
        // = seg000:37ff/3802 ax = travel_vehicle_mode; call hnm_load_first_frame
        //   — open the flight HNM by the vehicle id (2 = MNT1) and decode its
        //   first frame at blit offset 0: the 320x152 frames span the whole game
        //   area. The MNT clips' bit-4 resource flag routes the frame through
        //   hnm_present_flight_frame (seg000:ccee), which stamps the minimap.
        let id = self.travel_vehicle_mode;
        self.hnm_load_first_frame_by_id(id, 0);
        self.hnm_present_flight_frame();
        // = seg000:3805 call [gfx_vtable_vga_save_palette_to_fade_target]; 3809
        //   jmp set_sky_palette.
        gfx::vga_save_palette_to_fade_target(self);
        self.set_sky_palette();
    }

    // = seg000:4abe loc_04abe — resume the flight after the fly-over cabin/menu
    // (reached from menu_npc_actions_cleanup's flight branch, seg000:9809):
    // reload the flight view, present it, then clear data_011ca (loc_04ac4) so
    // travel_pump resumes pumping the flight HNM.
    pub(crate) fn travel_resume_flight_view(&mut self) {
        // = seg000:4abe call loc_037f4.
        self.travel_load_flight_view();
        // = seg000:4ac1 call present_game_area.
        self.present_game_area();
        // = seg000:4ac4 loc_04ac4: data_011ca = 0.
        self.data_011ca = 0;
    }

    // = seg000:4988 travel_minimap_setup — set up the flight minimap view:
    // clear the hover state, centre the zoomed view on the player, make the
    // minimap rect the map view rect and install travel_minimap_redraw as the
    // main-view drawing function (falling through into it).
    pub(crate) fn travel_minimap_setup(&mut self) {
        // = seg000:4988 data_046fc = 0.
        self.data_046fc = 0;
        // = seg000:498e call set_zoomed_globe_pos_from_map_position.
        self.set_zoomed_globe_pos_from_map_position();
        // = seg000:4991..4997 travel_minimap_rect -> data_046e3_rect.
        self.map_view_rect = TRAVEL_MINIMAP_RECT;
        // = seg000:499a install travel_minimap_redraw; falls through into it.
        self.current_main_view_drawing_function = Some(GameState::travel_minimap_redraw);
        self.travel_minimap_redraw();
    }

    // = seg000:49a0 travel_minimap_redraw — redraw the flight minimap into the
    // back buffer: the windowed map, the nested border, the location markers
    // and the destination marker (ICONES 0x2e).
    pub(crate) fn travel_minimap_redraw(&mut self) {
        // = seg000:49a0 call set_backbuffer_as_frame_buffer.
        self.set_backbuffer_as_frame_buffer();
        // = seg000:49a3 call loc_05b93 — sprite clip = the minimap rect (the
        //   port passes clip rects per draw call).
        // = seg000:49a6 data_046eb = 1 — the windowed map path.
        self.data_046eb = 1;
        // = seg000:49ab call map_draw_zoomed_globe.
        self.map_draw_zoomed_globe();
        // = seg000:49ae call draw_map_view_border.
        self.draw_map_view_border();
        // = seg000:49b1/49b4 the location markers.
        self.open_icones_spritesheet();
        self.map_build_and_draw_location_markers();
        // = seg000:49b7..49c9 the destination marker when it is visible.
        if self.travel_destination_ptr != 0 {
            let index = location_index_from_ptr(self.travel_destination_ptr);
            if let Some((sx, sy)) = self.location_visible_on_map(index) {
                // = seg000:49c4/49c5 dec bx; dec dx; 49c6 ax = 2eh.
                self.draw_active_bank_sprite(0x2e, sx - 1, sy - 1);
            }
        }
        // = seg000:49cc data_046eb = 0; 49d1 jmp set_fb1_as_active_framebuffer.
        self.data_046eb = 0;
        self.set_fb1_as_active_framebuffer();
    }

    // = seg000:49d4 travel_enter_minimap_view — re-enter the flight minimap
    // view (map_screen_cleanup with travel_minimap_state > 0, and the room
    // draw's travel branch shares the pieces): the setup + the full trail.
    fn travel_enter_minimap_view(&mut self) {
        // = seg000:49d4 call travel_minimap_setup (includes the redraw).
        self.travel_minimap_setup();
        // = seg000:49d7/49e3 jmp travel_trail_redraw.
        self.travel_trail_redraw();
    }

    // = seg000:49d9 travel_refresh_view — refresh the travel view: the full
    // globe (data_046eb bit 0x80) dispatches the installed main-view drawing
    // function; the flight minimap redraws + the full trail.
    fn travel_refresh_view(&mut self) {
        // = seg000:49d9/49de js loc_049e6 — the full-globe dispatch.
        if (self.data_046eb as i8) < 0 {
            let redraw = self
                .current_main_view_drawing_function
                .expect("travel_refresh_view with no main-view drawing function installed");
            redraw(self);
            return;
        }
        // = seg000:49e0 call travel_minimap_redraw; 49e3 jmp travel_trail_redraw.
        self.travel_minimap_redraw();
        self.travel_trail_redraw();
    }

    // = seg000:4a00 travel_trail_append — append the position at the ring's
    // write cursor, wrapping at the end.
    fn travel_trail_append(&mut self, x: u16, lat: i16) {
        // = seg000:4a02..4a0b store (longitude, latitude) at the cursor.
        self.travel_trail_ring[self.travel_trail_cursor] = (x, lat as u16);
        // = seg000:4a0c..4a15 wrap the cursor past the ring's end.
        self.travel_trail_cursor =
            (self.travel_trail_cursor + 1) % crate::game_state::TRAVEL_TRAIL_LEN;
    }

    // = seg000:4a1a travel_trail_stamp_last — stamp the last appended trail
    // position into the back buffer as a permanent minimap dot (ICONES 0x2f).
    fn travel_trail_stamp_last(&mut self) {
        // = seg000:4a1a js loc_04a59 — nothing while the minimap is hidden.
        if self.travel_minimap_state < 0 {
            return;
        }
        // = seg000:4a21/4a24 si = travel_minimap_rect; call
        //   restore_mouse_if_rect_intersects — lift the cursor off the
        //   minimap. DOS has no balancing draw here: the recorded hide makes
        //   the next redraw_mouse pass redraw (and, in the port, re-present)
        //   the cursor.
        self.restore_mouse_if_rect_intersects(TRAVEL_MINIMAP_RECT);
        // = seg000:4a27..4a3d the last written entry (cursor - 4 bytes,
        //   ring-wrapped).
        let idx = (self.travel_trail_cursor + crate::game_state::TRAVEL_TRAIL_LEN - 1)
            % crate::game_state::TRAVEL_TRAIL_LEN;
        let (x, lat) = self.travel_trail_ring[idx];
        // = seg000:4a3f dec ah; jns loc_04a59 — the 0x800 empty sentinel
        //   (a real latitude's high byte is 0 or 0xff).
        if (((lat >> 8) as u8).wrapping_sub(1) as i8) >= 0 {
            return;
        }
        // = seg000:4a43 the projection against the minimap view rect.
        let Some((sx, sy)) = self.map_position_to_screen_if_visible(x, lat as i16) else {
            return;
        };
        // = seg000:4a48..4a56 the dot (ICONES 0x2f) at (-1, -1) into the back
        //   buffer.
        self.open_icones_spritesheet();
        self.set_backbuffer_as_frame_buffer();
        self.draw_active_bank_sprite(0x2f, sx - 1, sy - 1);
        self.set_fb1_as_active_framebuffer();
    }

    // = seg000:4a5a travel_trail_redraw — redraw every trail dot from the ring
    // into the back buffer, oldest to newest from the cursor, skipping 0x800
    // empties and dots outside the minimap inset.
    pub(crate) fn travel_trail_redraw(&mut self) {
        // = seg000:4a5a call load_icones_sprites; 4a5d..4a61 draw into the
        //   back buffer, restoring the previous active framebuffer after.
        self.open_icones_spritesheet();
        let saved = self.active_fb();
        self.set_backbuffer_as_frame_buffer();
        // = seg000:4a64 si = the cursor — the walk starts at the OLDEST entry
        //   and wraps once around the whole ring.
        let mut si = self.travel_trail_cursor;
        loop {
            let (x, lat) = self.travel_trail_ring[si];
            si = (si + 1) % crate::game_state::TRAVEL_TRAIL_LEN;
            // = seg000:4a70..4a72 dec ah; jns — skip the empty sentinel.
            if ((((lat >> 8) as u8).wrapping_sub(1)) as i8) < 0 {
                // = seg000:4a75 the projection; 4a7a..4a90 the (-1, -1) dot
                //   position must sit inside (0xcc,4)-(0x13a,0x3a).
                if let Some((sx, sy)) = self.map_position_to_screen_if_visible(x, lat as i16) {
                    let (px, py) = (sx - 1, sy - 1);
                    if (0xcc..0x13a).contains(&px) && (4..0x3a).contains(&py) {
                        // = seg000:4a92/4a95 the dot (ICONES 0x2f).
                        self.draw_active_bank_sprite(0x2f, px, py);
                    }
                }
            }
            // = seg000:4a99..4aa6 wrapped back to the cursor: done.
            if si == self.travel_trail_cursor {
                break;
            }
        }
        // = seg000:4aa8 pop framebuffer_active.
        self.active_fb = saved;
    }

    // = seg000:5b69 draw_map_view_border — the nested 4-line border around the
    // map view rect (data_046e3_rect), colours 0xfc, 0xfa, 0xf8, 0xf6 growing
    // outward (draw_nested_rect_border, seg000:5b6e).
    pub(crate) fn draw_map_view_border(&mut self) {
        let r = self.map_view_rect;
        let (mut x0, mut y0, mut x1, mut y1) = (r.x0, r.y0, r.x1, r.y1);
        let mut color = 0xfcu8;
        // = seg000:5b79 bp = 4 rings.
        for _ in 0..4 {
            // = seg000:5b7e/5b7f dec dx; dec bx — the ring grows one out on
            //   the top-left each pass; 5b80 call draw_rect_outline.
            x0 -= 1;
            y0 -= 1;
            self.draw_rect_outline(x0, y0, x1, y1, color);
            // = seg000:5b85/5b86 inc di; inc cx — and one out on the
            //   bottom-right for the next ring; 5b87 the colour steps by -2.
            x1 += 1;
            y1 += 1;
            color = color.wrapping_sub(2);
        }
    }

    // = seg000:4afd hnm_present_flight_frame — the resource-flag bit 4
    // full-screen present the flight clips take after each decoded frame
    // (from hnm_decode_video_frame, seg000:ccee): restore the minimap rect
    // from the back buffer into fb1, then push fb1 to the visible screen.
    pub(crate) fn hnm_present_flight_frame(&mut self) {
        // = seg000:4afd cmp [suppress_sky_240_255],0; jnz — suppressed skips
        //   the minimap restore (hnm_blit_frame_to_screen).
        if self.data_0227d == 0 {
            // = seg000:4b04 call travel_restore_minimap_rect (loc_04b2b):
            //   unless the minimap is hidden (travel_minimap_state bit 0x80),
            //   copy travel_minimap_restore_rect from the back buffer into
            //   fb1 (loc_0c46f); the loc_0dbca cursor-hide tail is the
            //   game_loop-driven cursor bracket in the port.
            if self.travel_minimap_state >= 0 {
                let yoff = self.y_offset as i16;
                let r = TRAVEL_MINIMAP_RESTORE_RECT;
                let src_rect = rect(r.x0, r.y0 + yoff, r.x1, r.y1 + yoff);
                gfx::vga_copy_rect(&mut self.framebuffer, &self.framebuffer_back, src_rect);
            }
        }
        // = seg000:4b07..4b0f es = screen; si = fb1; vga_copy_partial — the
        //   game area fb1 -> screen (skipped while composing offscreen, like
        //   every present).
        self.present_game_area();
        // = seg000:caa0 loc_0caa0 — the streaming pipeline: with the frame
        //   consumed (video_decode_buf_seg cleared), the reader immediately
        //   decodes the NEXT video chunk into fb1
        //   (hnm_decode_typed_chunk_video_to_bp, bp = framebuffer_1). That
        //   overwrites the minimap stamp above, so fb1 holds a clean frame
        //   between presents — which is why the fly-over cabin's transparent
        //   windshield shows plain desert, and the minimap returns with the
        //   next present when the flight resumes. hnm_do_frame consumes this
        //   prefetched frame without decoding when its tick arrives.
        if self.hnm_is_open() && !self.hnm_finished && self.hnm_step_frame() {
            self.hnm_video_frame_ready = true;
        }
    }

    // = seg000:4e8e travel_probe_terrain_ahead — probe the terrain ahead of
    // the flight: 6 preview steps from the current position (regs only; the
    // step fraction is saved/restored but the heading re-aims ride along,
    // like DOS), the map byte there and one step further, the fly-over
    // detector, then the averaged byte picks the flight clip.
    fn travel_probe_terrain_ahead(&mut self) {
        // = seg000:4e8e..4e96 dx/bx = the position encoding; push the step
        //   fraction.
        let saved_accum = self.travel_step_accum;
        let mut x = self.location_and_room;
        let mut lat = self.location_appearance as i16;
        // = seg000:4e9a..4ea9 six preview steps.
        for _ in 0..6 {
            (x, lat) = self.travel_step_position(x, lat);
        }
        // = seg000:4eac the map byte six steps ahead.
        let a = self.read_map_byte(x, lat);
        // = seg000:4eb0 one more step; 4eb4 pop the step fraction.
        let (nx, nlat) = self.travel_step_position(x, lat);
        self.travel_step_accum = saved_accum;
        // = seg000:4eb9 the byte seven steps ahead.
        let b = self.read_map_byte(nx, nlat);
        // = seg000:4ebd call travel_flyover_detect — with the position seven
        //   steps ahead (dx/bx) still live.
        self.travel_flyover_detect(nx, nlat);
        // = seg000:4ec2/4ec4 the byte-averaged terrain falls into
        //   travel_select_flight_video.
        let terrain = a.wrapping_add(b) >> 1;
        self.travel_select_flight_video(terrain);
    }

    // = seg000:41e1 travel_flyover_detect — detect a location the flight
    // passes near: scan an 8-cell strip across the heading (built at the probe
    // position x/lat, seven steps ahead) for a location cell (map bit 0x40)
    // whose bearing is within ±0x20 of travel_heading, and latch its signed
    // relative angle * 0x20 (data_01968) and SAL-tier silhouette sprite
    // (data_0196a) into a 3-entry (x, sprite) array at seg001:1960..196b, with
    // data_0196c the re-arm countdown. That silhouette array is vestigial in
    // this build: nothing reads it to draw (verified — no code references the
    // addresses and no pointer to 0x1960 exists), and its sprite ids 0x10..0x17
    // are absent from ORNYCAB/ORNYPAN, so the latch is inert. The visible
    // fly-over cabin is a separate path: travel_scan_nearby_location
    // (seg000:40f9) + travel_show_companion_cabin (seg000:368b).
    fn travel_flyover_detect(&mut self, x: u16, lat: i16) {
        // = seg000:41e1..41e6 while the re-arm countdown is live just tick it
        //   down (loc_041db); don't re-detect.
        if self.data_0196c != 0 {
            // = seg000:41db dec [data_0196c]; clc; ret.
            self.data_0196c -= 1;
            return;
        }
        // = seg000:41e8..41f7 orient an 8-cell strip across the heading: a 1×8
        //   longitude run when (heading + 0x20) & 0x40 == 0 (heading near
        //   E/W), else an 8×1 latitude run (near N/S).
        let (rows, cols) = if self.travel_heading.wrapping_add(0x20) & 0x40 == 0 {
            (1, 8)
        } else {
            (8, 1)
        };
        // = seg000:41f8 call loc_0b56c — build the (map byte, map offset) strip.
        let strip = self.map_build_cell_strip(x, lat, rows, cols);
        // = seg000:41fb..4209 scan the 8 cells for a location cell (bit 0x40).
        for (byte, offset) in strip {
            // = seg000:41ff..4206 test al,40h; not a location cell → next.
            if byte & 0x40 == 0 {
                continue;
            }
            // = seg000:420a..4211 find the location owning this cell; a cell
            //   with no owning record (DOS end sentinel, ZF clear) is skipped.
            let Some(idx) = self.find_location_by_map_offset(offset) else {
                continue;
            };
            let loc = self.locations[idx];
            // = seg000:4213..421f status bit 0x80 gates on story progress: the
            //   location stays hidden until game_phase reaches its
            //   discoverable phase (unsigned byte compare).
            if loc.status & 0x80 != 0 && self.game_phase < loc.discoverable_at_phase as u8 {
                continue;
            }
            // = seg000:4221..4226 the compass angle from the player to the
            //   location; a location sitting exactly on the player cell returns
            //   CF set (both deltas zero) and is skipped.
            let (px, plat) = self.get_map_position();
            if loc.map_x as u16 == px && loc.map_y == plat {
                continue;
            }
            let angle = self.compass_angle_to_location(idx);
            // = seg000:4228..4230 keep it only within ±0x20 of the heading.
            let rel = angle.wrapping_sub(self.travel_heading).wrapping_add(0x20);
            if rel >= 0x40 {
                continue;
            }
            // = seg000:4232..423f latch the signed relative angle × 0x20.
            let signed = rel.wrapping_sub(0x20) as i8 as i16;
            self.data_01968 = signed << 5;
            // = seg000:4242..424b the SAL-tier silhouette sprite (table_196d).
            self.data_0196a = TABLE_196D[crate::room_scene::calc_sal_index(loc.appearance)] as u16;
            // = seg000:4250..4256 arm the re-arm countdown; stop scanning.
            self.data_0196c = 6;
            return;
        }
        // = seg000:4208 clc; ret — no fly-over this pass.
    }

    // = seg000:b56c loc_0b56c (with loc_0b53b, seg000:b53b) — build a
    // rows × cols block of the planet map centred on (x = longitude, lat =
    // latitude row) into a strip of (map byte, map offset) cells, row-major
    // (the DOS data_09e68 scratch of 3-byte [byte, offset] entries). The
    // latitude is centred at lat - rows/2, clamped to the south pole, and each
    // row is a horizontal run of `cols` cells centred on the longitude's cell,
    // wrapping within the row.
    fn map_build_cell_strip(&self, x: u16, lat: i16, rows: usize, cols: usize) -> Vec<(u8, usize)> {
        let mut strip = Vec::with_capacity(rows * cols);
        // = seg000:b56d..b578 bx = lat - rows/2, clamped to -98 (0xff9e).
        let mut latitude = (lat - (rows / 2) as i16).max(-98);
        for _ in 0..rows {
            // = seg000:b58b map_func — the row's map offset, cell index and
            //   byte length. DOS reads one past the tablat at the poles; the
            //   port clamps the latitude into range so the lookup stays valid.
            let (offset0, cell0, bp) = self.map_position_to_offset(x, latitude.clamp(-98, 98));
            let bp = bp as i32;
            // = seg000:b58e..b592 di = res_map_ofs + row start; row_base is the
            //   row's map offset without the longitude cell.
            let row_base = offset0 as i32 - cell0 as i32;
            // = seg000:b543..b54f cell -= cols/2, wrapping once within the row.
            let mut cell = cell0 as i32 - (cols / 2) as i32;
            if cell < 0 {
                cell += bp;
            }
            // = seg000:b551..b566 the horizontal run of `cols` cells.
            for _ in 0..cols {
                let di = (row_base + cell) as usize;
                strip.push((self.map[di], di));
                cell += 1;
                if cell >= bp {
                    cell -= bp;
                }
            }
            // = seg000:b583 inc bx — the next latitude row.
            latitude += 1;
        }
        strip
    }

    // = seg000:4ec6 travel_select_flight_video — pick hnm_active_video_id from
    // the vehicle and the terrain byte; the HNM loop point switches clips when
    // it differs from the playing one (seg000:cb7c).
    pub(crate) fn travel_select_flight_video(&mut self, terrain: u8) {
        // = seg000:4ec7..4ece a vehicle below MNT1_SAND_LOOP plays its own id.
        const VEHICLE_MODE_ORNITHOPTER: u16 = 2;
        const MNT1_SAND_LOOP: u16 = 2;
        const MNT2_SAND_TO_ROCK: u16 = 3;
        const MNT3_ROCK_LOOP: u16 = 4;
        const MNT4_ROCK_TO_SAND: u16 = 5;
        const TERRAIN_TYPE_MASK: u8 = 0x0f;
        const TERRAIN_ROCK_THRESHOLD: u8 = 8;

        let vehicle = self.travel_vehicle_mode;
        if vehicle < VEHICLE_MODE_ORNITHOPTER {
            self.hnm_active_video_id = vehicle;
            return;
        }
        let current = self.hnm_video_id;
        // = seg000:4ed0..4ed7 the terrain low nibble: >= 8 is rock.
        if terrain & TERRAIN_TYPE_MASK >= TERRAIN_ROCK_THRESHOLD {
            // = seg000:4ef3..4f03 rock: the sand-to-rock transition MNT2 (3)
            //   from the sand loop or the rock-to-sand transition; the rock
            //   loop MNT3 (4) otherwise.
            self.hnm_active_video_id = if current <= MNT1_SAND_LOOP || current == MNT4_ROCK_TO_SAND
            {
                MNT2_SAND_TO_ROCK
            } else {
                MNT3_ROCK_LOOP
            };
            return;
        }
        // = seg000:4ed9..4ee9 sand: stay on the vehicle clip from the sand
        //   loop; the rock-to-sand transition MNT4 (5) from MNT2/MNT3; else
        //   back to MNT1 (2), also resetting the vehicle mode.
        self.hnm_active_video_id = if current <= MNT1_SAND_LOOP {
            vehicle
        } else if current <= MNT3_ROCK_LOOP {
            MNT4_ROCK_TO_SAND
        } else {
            self.travel_vehicle_mode = VEHICLE_MODE_ORNITHOPTER;
            MNT1_SAND_LOOP
        };
    }

    // = seg000:4795 play_travel_departure_transition — play the departure
    // transition for the mode the map screen was entered from (al = the
    // pre-confirm game_screen_mode_flags).
    fn play_travel_departure_transition(&mut self, old_mode_flags: u8) {
        // = seg000:4795 cmp [data_046eb],0; js ret — nothing on the full
        //   globe (cleanup already zeroed it for the windowed map).
        if self.data_046eb & 0x80 != 0 {
            return;
        }
        // = seg000:479c cmp al,4; jz loc_047ce — only the ornithopter map
        //   mode plays the takeoff.
        if old_mode_flags != 4 {
            // = seg000:47a0..47ca the game-phase-0x50 branch (the worm and
            //   globe flows, none ported): tear down the head overlay
            //   (loc_098af), set game phase 0x50 with its transition
            //   (transition al=0x10 bp=loc_04913, loc_0491c) and stop the
            //   voice. TODO: port with CALL A WORM.
            println!(
                "play_travel_departure_transition: phase-0x50 branch (seg000:47a0) not ported"
            );
            return;
        }
        // = seg000:47ce call prefetch_travel_hnm_resources (seg000:ce53) — a
        //   CD-era prefetch of the flight HNMs (resources 0x63..0x68); the
        //   port reads the DAT on demand.
        // = seg000:47d1..47d9 xchg al,[data_04732]; shl al,1; jnb ret — the
        //   destination click armed bit 7; consume it either way.
        let armed = std::mem::take(&mut self.data_04732);
        if armed & 0x80 == 0 {
            return;
        }
        // = seg000:47db fold the HUD head away.
        self.ui_hud_head_animate_down();
        // = seg000:47de..47e6 re-render the scene without the parked ornis
        //   (orni_anim_frame = 0xff) into fb1.
        self.orni_anim_frame = 0xff;
        self.set_fb1_as_active_framebuffer();
        self.draw_room_scene();
        // = seg000:47e9/47ec snapshot it to fb2 (the takeoff frames restore
        //   from it) and refresh the backdrop copy.
        self.copy_active_framebuffer_to_framebuffer_2();
        self.copy_game_area_rect_to_unknown_rect();
        // = seg000:47ef orni_anim_frame = 0; 47f4/47f6 al=6 audio_start_voc —
        //   the SN6 engine loop.
        self.orni_anim_frame = 0;
        self.audio_start_voc("SN6.HSQ");
        // = seg000:47f9 cl = 1 — upward; falls into orni_anim_loop.
        self.orni_anim_loop(1);
    }

    // = seg000:47fb orni_anim_loop — the ornithopter takeoff/landing
    // animation pump: one frame per pass (`step` +1 takeoff, -1 landing),
    // until orni_anim_frame passes 0x21.
    pub(crate) fn orni_anim_loop(&mut self, step: i8) {
        loop {
            // = seg000:47fb/47fe/4801 bp=orni_anim_draw_frame; ax=14h; call
            //   wait_processing_frame_tasks_interruptable — the e353 wait
            //   runs the bp callback once, then paces 0x14 ticks (with the
            //   sky-suppress flag clear the wait only honours the P pause,
            //   not a click).
            self.orni_anim_draw_frame(step);
            for _ in 0..0x14 {
                self.tick_one_frame();
            }
            // = seg000:4804 add [orni_anim_frame], cl.
            self.orni_anim_frame = self.orni_anim_frame.wrapping_add(step as u8);
            // = seg000:4808..4813 an upward pass through frame 0x1a releases
            //   the SN6 engine loop.
            if self.orni_anim_frame == 0x1a && step >= 0 {
                self.pcm_player.end_loop();
            }
            // = seg000:4816 call service_midi_music.
            self.service_midi_music();
            // = seg000:4819 cmp [orni_anim_frame],21h; jb loc_047fb.
            if self.orni_anim_frame >= 0x21 {
                break;
            }
        }
    }

    // = seg000:4821 orni_anim_draw_frame — draw one takeoff/landing frame:
    // the clean pad scene from fb2, the animated orni (climbing away past
    // frame 0xe) from ORNYTK.HSQ, the still-parked ornis, then present.
    fn orni_anim_draw_frame(&mut self, step: i8) {
        // = seg000:4822 call copy_game_area_to_screen_fb2_to_fb1.
        self.copy_game_area_fb2_to_fb1();
        // = seg000:4825..482b open ORNYTK.HSQ (the takeoff bank) and flush
        //   its bank palette.
        self.open_sprite_bank(sprite_bank::ORNYTK);
        self.update_screen_palette();
        // = seg000:482e draw into fb1.
        self.set_fb1_as_active_framebuffer();
        // = seg000:4831 the pad position.
        let (mut x, mut y) = self.get_orni_position();
        // = seg000:4834..483c frame 0xd (rotors up) also releases the
        //   engine-sound loop.
        if self.orni_anim_frame == 0x0d {
            self.pcm_player.end_loop();
        }
        // = seg000:4840..485b past frame 0xe the orni climbs away: x steps 5
        //   per frame against the direction sign, y drops (frame-0xe)^2 / 2.
        let t = (self.orni_anim_frame as i16) - 0x0e;
        if t > 0 {
            let dx = if step >= 0 { 5 } else { -5 };
            x -= dx * t;
            y -= (t * t) >> 1;
        }
        // = seg000:485d call draw_orni — the animated orni (the ORNYTK bank
        //   carries the same part layout the room pass animates).
        self.draw_orni(x, y);
        // = seg000:4860..4878 the ornis still parked beyond the departing
        //   slot, drawn at anim frame 0 (saved around the call): the
        //   step-first draw_ornis entry (seg000:3a73) skips the first pad
        //   slot and draws count - 1, so the departing orni's spot stays
        //   empty under the animated one.
        let (px, py) = self.get_orni_position();
        let count = self.available_equipment.ornithopters;
        if count != 0 {
            let saved = std::mem::replace(&mut self.orni_anim_frame, 0);
            self.draw_ornis(count, px, py);
            self.orni_anim_frame = saved;
        }
        // = seg000:487b call present_game_area.
        self.present_game_area();
        // = seg000:487e..4885 keep a running sky fade stepping.
        if self.sky_fade_countdown != 0 {
            self.tick_sky_fade();
        }
    }

    // = seg000:4703 map_confirm_travel_and_close — the travel-confirm chain
    // (entered from the destination click at seg000:4569; SKIP TO DESTINATION
    // at seg000:5116 re-enters it mid-flight, not ported): arm the pending
    // travel, close the map screen back to the room and depart.
    pub(crate) fn map_confirm_travel_and_close(&mut self, hover: u16, x: i16, y: i16) {
        // = seg000:4703 call arm_pending_travel.
        self.arm_pending_travel(hover, x, y);
        // = seg000:4706 call loc_038e1 — refresh the sky palette.
        self.loc_038e1_sky_refresh();
        // = seg000:4709..4711 fold the map mode bit into the travel bit
        //   (flags |= flags >> 2: the orni map bit 4 -> bit 1, the worm map
        //   bit 8 -> bit 2); the old value drives the branches below.
        let old_flags = self.game_screen_mode_flags;
        self.game_screen_mode_flags |= old_flags >> 2;
        // = seg000:4715 call update_room_music.
        self.update_room_music();
        // = seg000:4718..4724 a staged night attack is cancelled by the
        //   departure.
        if self.night_attack_stage != 0 {
            self.night_attack_stage = 0;
            // = seg000:4724 call loc_00b21 — stop the attack's audio loop
            //   and particles. TODO: the night-attack teardown is not ported.
            println!("map_confirm_travel_and_close: night-attack teardown (loc_00b21) not ported");
        }
        // = seg000:4727 call screen_element_stack_pop_and_cleanup — pop the
        //   map screen menu; map_screen_cleanup keeps the mode flags now
        //   travel_destination_ptr is armed.
        self.menu_stack_pop_and_cleanup();
        // = seg000:472a call loc_04d00 — remove the command-panel overlay
        //   task (frame_task_callback_04bb9); the port never arms it.
        // = seg000:472d..4730 with the old flags already in a travel mode
        //   (a mid-flight re-confirm) only the panel redraw below runs.
        if old_flags & 3 == 0 {
            // = seg000:4732 travel_step_counter = 0.
            self.travel_step_counter = 0;
            // = seg000:4739 call ungrey_skip_to_destination_verb.
            self.ungrey_skip_to_destination_verb();
            // = seg000:473c..4745 in orni-travel mode fold the HUD head.
            if self.game_screen_mode_flags & 3 == 1 {
                self.ui_hud_head_animate_down();
            }
            // = seg000:4748 call copy_game_rect_fb1_to_fb2 — snapshot the
            //   restored room game area into fb2 (the takeoff frames and the
            //   travel view restore from it).
            let yoff = self.y_offset as i16;
            let r = rect(0, yoff, 320, 152 + yoff);
            gfx::vga_copy_rect(&mut self.framebuffer_saved, &self.framebuffer, r);
            // = seg000:474b call run_travel_departure_npc_scans.
            self.run_travel_departure_npc_scans();
            // = seg000:474e..4758 play the departure transition with the old
            //   flags; travel_heading rides the stack around it (the
            //   phase-0x50 branch can clobber it).
            let heading = self.travel_heading;
            self.play_travel_departure_transition(old_flags);
            self.travel_heading = heading;
            // = seg000:475b current_scene = 0xff — no room scene while
            //   airborne; the next scene draw takes the desert branch.
            self.data_00008 = 0xff;
            // = seg000:4760 call travel_advance_step — the first step.
            self.travel_advance_step();
            // = seg000:4763 current_location_ptr = 0 — the player has left.
            self.current_location_index = 0xffff;
            // = seg000:4769 travel_step_tick_stamp = 0.
            self.travel_step_tick_stamp = 0;
            // = seg000:476f..4776 rebuild the room view beneath (skipped on
            //   the full globe).
            if self.data_046eb & 0x80 == 0 {
                self.draw_room_game_screen_scene_reload();
            }
            // = seg000:4779 call frame_task_callback_04ab8 — data_04727 =
            //   0xff arms the game loop's travel pump (tick_in_game_travel,
            //   still a stub).
            self.travel_active = 0xff;
            // = seg000:477c..478c in orni-travel mode the departing orni
            //   leaves the pad and the narration stops; done.
            if self.game_screen_mode_flags & 3 == 1 {
                // = seg000:4785/4789 dec byte [last_location_ptr + 15h].
                let loc = &mut self.locations[self.last_location_index];
                loc.equipment.ornithopters = loc.equipment.ornithopters.wrapping_sub(1);
                // = seg000:478c jmp pcm_stop_voc.
                self.pcm_player.stop();
                return;
            }
        }
        // = seg000:478f call ui_draw_room_command_panel; 4792 jmp
        //   update_screen_palette.
        self.ui_draw_room_command_panel();
        self.update_screen_palette();
    }

    // = [si+4]/[si+6]/[si+8] of mouse_handlers_01ac8 — the RMB and both
    // release slots are the no-op loc_00f66.
    fn map_mouse_rmb(&mut self) {}
    fn map_mouse_release(&mut self) {}
    fn map_mouse_rmb_release(&mut self) {}

    // = [si+0ah]/[si+0ch] of mouse_handlers_01ac8 — both drag slots re-run the
    // hover tracker (map_mouse_hover_tracker).
    fn map_mouse_drag(&mut self, _dx: i16, _dy: i16) {
        self.map_mouse_hover_tracker();
    }
    fn map_mouse_rmb_drag(&mut self, _dx: i16, _dy: i16) {
        self.map_mouse_hover_tracker();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{GameState, dat_file::DatFile, menu_defs::MenuRef};

    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn tmp_dump_ornycab_header() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        eprintln!("opening {dat_path}");
        let Ok(mut dat_file) = DatFile::open(dat_path) else {
            eprintln!("open failed");
            return;
        };
        for name in ["ORNYCAB.HSQ", "ORNYPAN.HSQ", "ICONES.HSQ"] {
            let data = dat_file.read(name).unwrap();
            let toc_pos = u16::from_le_bytes([data[0], data[1]]) as usize;
            let sprite0 = u16::from_le_bytes([data[toc_pos], data[toc_pos + 1]]) as usize;
            let count = sprite0 / 2;
            for id in 0..3usize.min(count) {
                let entry = u16::from_le_bytes([data[toc_pos + id * 2], data[toc_pos + id * 2 + 1]])
                    as usize;
                let ofs = toc_pos + entry;
                let w0 = u16::from_le_bytes([data[ofs], data[ofs + 1]]);
                let w1 = u16::from_le_bytes([data[ofs + 2], data[ofs + 3]]);
                eprintln!(
                    "{name} sprite {id}: w0={w0:04x} w1={w1:04x} flags={:02x} w={} h={} pal_offset={:02x}",
                    (w0 & 0xfe00) >> 8,
                    w0 & 0x1ff,
                    w1 & 0xff,
                    (w1 & 0xff00) >> 8
                );
            }
        }
    }

    // TAKE AN ORNITHOPTER with a bare-desert destination click: a directional
    // flight is steerable, so the departure's rebuild_and_draw_room_nav_panel
    // leaves the turn-left / flight / turn-right panel (ui_nav_panel_flight,
    // seg000:3010) in HUD records 12..17. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored directional_takeoff_nav_panel
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn directional_takeoff_keeps_the_steering_panel() {
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

        game.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);
        while rx.try_recv().is_ok() {}
        // 0xfff0 = the cursor was over bare map, not a location marker.
        game.map_confirm_travel_and_close(0xfff0, 200, 70);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.travel_no_location_dest, 0xff, "directional flight");
        assert_eq!(game.data_011ca, 0, "nothing holds the screen");

        let o = crate::game_ui::NAV_PANEL_RECORD_OFFSET;
        let handlers: Vec<u16> = (0..6).map(|i| game.ui_elements[o + i].func_ptr).collect();
        assert_eq!(
            handlers,
            vec![0x0f66, 0x4ad0, 0x4f09, 0x4ad7, 0x0f66, 0x0f66],
            "the steering panel survives the departure"
        );
    }

    // A companion spotting a discoverable landmark during an ornithopter
    // flight (pending_room_action 3): the ORNYCAB cockpit's transparent
    // windshield must show the plain desert frame — not the minimap stamp
    // hnm_present_flight_frame left in fb1 (the original shows no minimap
    // while the cabin is up; it returns when the flight resumes) — and the
    // scan tail's seg000:4171 rebuild must flip the flight strip to SKIP TO
    // DESTINATION now that arm_pending_travel re-aimed the flight at the
    // spotted location. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored flyover_spot
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn flyover_spot_hides_minimap_and_offers_skip_to_destination() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.cmd_args_memory |= 0x10;
        game.start(true);
        // Gurney travels with us.
        game.room_persons[4].flags |= 0x40;
        game.persons_travelling_with |= 1 << 4;
        game.npc_assign_companion_slot(4);
        while rx.try_recv().is_ok() {}

        // Orni map + a directional desert takeoff.
        game.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);
        while rx.try_recv().is_ok() {}
        game.map_confirm_travel_and_close(0xfff0, 200, 70);
        assert_eq!(game.travel_no_location_dest, 0xff, "directional flight");

        // Teleport the flight three cells west of a discoverable landmark and
        // aim at it, so the scan arms room action 3 (GO TOWARDS THIS PLACE).
        game.game_phase = 0x40;
        let idx = (0..game.locations.len())
            .find(|&i| {
                let l = &game.locations[i];
                l.status & 0x80 != 0
                    && game.game_phase >= l.discoverable_at_phase as u8
                    && l.map_x > 4
            })
            .expect("a discoverable location");
        let loc = game.locations[idx];
        game.location_and_room = (loc.map_x as u16).wrapping_sub(3);
        game.location_appearance = (game.location_appearance & 0xff00) | (loc.map_y as u8 as u16);
        game.travel_heading = game.compass_angle_to_location(idx);

        // Pump (forcing the step timer) until the cabin pauses the flight.
        let mut cabin = false;
        for _ in 0..60 {
            game.travel_step_tick_stamp = (game.game_ticks() as u16).wrapping_sub(0x300);
            game.travel_pump();
            game.process_frame_tasks();
            while rx.try_recv().is_ok() {}
            if game.data_011ca != 0 {
                cabin = true;
                break;
            }
            let start = game.game_ticks();
            game.sleep_ticks(start, 1);
        }
        assert!(cabin, "the fly-over cabin must rise");
        assert_eq!(game.pending_room_action, 3, "the spot arms room action 3");
        assert_eq!(game.get_active_menu_ref(), MenuRef::MenuGoTowardsThisPlace);

        // = seg000:4944/50be — the divert is a homing travel at the spotted
        //   location, and the rebuilt strip beneath offers SKIP TO DESTINATION.
        assert_eq!(game.travel_no_location_dest, 0);
        assert_eq!(
            game.command_menu_buf.records.first().map(|r| r.handler),
            Some(0x4ffb),
            "SKIP TO DESTINATION beneath the fly-over menu"
        );

        // The windshield region of fb1 holds the pre-stamp desert frame, not
        // the minimap: it must differ from the back buffer's minimap in the
        // restore rect.
        let yoff = game.y_offset;
        let mut same = 0;
        let mut total = 0;
        for y in 4..60u16 {
            for x in 204..316u16 {
                total += 1;
                if game.framebuffer.get(x, y + yoff) == game.framebuffer_back.get(x, y + yoff) {
                    same += 1;
                }
            }
        }
        assert!(
            same < total / 2,
            "the minimap must not sit in fb1 under the cabin ({same}/{total} pixels match)"
        );
    }

    // TAKE AN ORNITHOPTER (seg000:42e9) opens the map screen: the ORNYPAN
    // cockpit frames a one-cell-per-pixel map window centred on the player's
    // map position, the Cancel menu folds in, and the map screen menu owns
    // the stack; Cancel (menu_callback_choice_exit_menu) restores the room.
    // Asset-gated; run with:
    //   cargo test -p dune --lib -- --ignored ornithopter
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn take_an_ornithopter_opens_the_map_screen() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.start(true);
        while rx.try_recv().is_ok() {}

        game.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);

        assert_eq!(game.get_active_menu_ref(), MenuRef::MenuCancel);
        assert_eq!(game.map_ornithopter_mode, 1);
        assert_eq!(game.game_screen_mode_flags, 4);
        assert_eq!(game.travel_vehicle_mode, 2);
        assert_eq!(game.data_046eb, 1);

        game.framebuffer
            .write_png_scaled(&game.palette, "ornithopter_map.png")
            .expect("write ornithopter_map.png");

        // The map window holds map pixels: vga_blit_shaded remaps every cell
        // to palette bank 1 (0x10..0x1f, plus the 0x17..0x1c edge shades). The
        // ORNYPAN window overlay (sprite 2) then draws its green grid lines
        // over the map, so the window centre is mostly — not entirely —
        // bank-1 colours.
        let fb = game.framebuffer.pixels();
        let mut bank1 = 0usize;
        let mut total = 0usize;
        for y in 60..120 {
            for x in 90..230 {
                let p = fb[y * 320 + x];
                total += 1;
                if (0x10..0x20).contains(&p) {
                    bank1 += 1;
                }
            }
        }
        assert!(
            bank1 > total * 4 / 5,
            "map window is not mostly map pixels: {bank1}/{total} in palette bank 1"
        );

        // The cockpit frame (ORNYPAN sprite 0 at (0,0x13)) drew outside the
        // window: the left cockpit column must hold non-background pixels.
        let cockpit: usize = (60..120)
            .map(|y| (0..40).filter(|x| fb[y * 320 + x] != 0).count())
            .sum();
        assert!(
            cockpit > 500,
            "cockpit pixels missing left of the window: {cockpit}"
        );

        // The composed screen reached the display. The game area matches the
        // fb1 composition everywhere but inside the player-marker rect: the
        // blinking "you are here" sprite draws to the screen only (fb1 keeps
        // the clean map the blink restores from).
        let frames: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(!frames.is_empty(), "the map screen never presented a frame");
        let (screen, _) = frames.last().unwrap();
        let pm = game.map_player_marker_rect;
        for y in 0..137usize {
            for x in 0..320usize {
                if (pm.x0..pm.x1).contains(&(x as i16)) && (pm.y0..pm.y1).contains(&(y as i16)) {
                    continue;
                }
                assert_eq!(
                    screen.pixels()[y * 320 + x],
                    game.framebuffer.pixels()[y * 320 + x],
                    "presented game area differs from the composition at ({x},{y})"
                );
            }
        }

        // The alt nav panel replaced the room compass on the VISIBLE screen:
        // ui_draw_nav_panel's 0xf0 fill (seg000:d741, front-buffer targeted)
        // must have erased it. The panel's top-left corner (x 255..266,
        // y 163..171) is inside the fill rect but under none of the alt-panel
        // records, so it must be pure fill — any other colour is a leftover
        // room-compass pixel.
        let sp = screen.pixels();
        for y in 163..171 {
            for x in 255..266 {
                assert_eq!(
                    sp[y * 320 + x],
                    0xf0,
                    "stale nav-panel pixel at ({x},{y}) under the alt panel"
                );
            }
        }

        // The "SELECT DESTINATION ON MAP" caption is armed and types itself
        // over the map one letter per task firing (map_caption_frame_task,
        // seg000:46b5) — a typewriter, not a flash. Spaces ride along with the
        // following letter (seg000:46fe); the high-bit terminator idles the
        // task. Count the caption's fg-colour (0x61) pixels on the VISIBLE
        // screen along the ornithopter pen rows (y 0x26 + 7 small-font rows).
        assert!(!game.map_caption_text.is_empty(), "caption not armed");
        assert_eq!(game.map_caption_pos, 0, "caption drew before any firing");
        let strip = |g: &GameState| -> usize {
            let px = g.screen.pixels();
            (0x26..0x26 + 7)
                .map(|y| (0x55..240).filter(|&x| px[y * 320 + x] == 0x61).count())
                .sum()
        };
        let base = strip(&game);
        let non_space = game
            .map_caption_text
            .iter()
            .take_while(|&&c| c & 0x80 == 0)
            .filter(|&&c| c != 0x20)
            .count();
        assert!(non_space > 10, "caption string suspiciously short");
        game.tick_map_caption();
        let after_one = strip(&game);
        assert!(after_one > base, "the first firing drew no glyph");
        // One non-space glyph per firing: exactly `non_space` firings reach
        // the terminator, and the caption keeps growing on the way.
        for _ in 1..non_space {
            game.tick_map_caption();
        }
        assert!(strip(&game) > after_one, "the caption did not keep typing");
        game.screen
            .write_png_scaled(&game.palette, "ornithopter_map_caption.png")
            .expect("write ornithopter_map_caption.png");
        let done_pos = game.map_caption_pos;
        game.tick_map_caption();
        assert_eq!(
            game.map_caption_pos, done_pos,
            "the task did not idle at the terminator"
        );

        // The visible-location marker list was rebuilt (seg000:5dce): the
        // palace (location 0) is where the player stands, so its marker sits
        // at the window centre the view was seeded on.
        assert!(
            !game.visible_location_markers.is_empty(),
            "no visible-location markers were built"
        );
        let palace = game
            .visible_location_markers
            .iter()
            .find(|m| m.location_index == 0)
            .expect("the palace marker is missing");
        let r = game.map_view_rect;
        assert_eq!(palace.x, r.x0 + (r.x1 - r.x0) / 2);
        assert_eq!(palace.y, r.y0 + (r.y1 - r.y0 - 1) / 2);

        // The blinking "you are here" marker is armed over the same spot
        // (seg000:445d): the rect is set, the task fired its immediate first
        // draw (phase 1 = visible), so the marker rect on the SCREEN differs
        // from the clean fb1 map beneath.
        let m = game.map_player_marker_rect;
        assert_ne!(m.x0, 0, "player marker rect not armed");
        assert_eq!(game.map_player_marker_phase, 1);
        let marker_diff = |g: &GameState| -> usize {
            let (s, f) = (g.screen.pixels(), g.framebuffer.pixels());
            let mut n = 0;
            for y in m.y0.max(0)..m.y1.min(200) {
                for x in m.x0.max(0)..m.x1.min(320) {
                    let i = y as usize * 320 + x as usize;
                    if s[i] != f[i] {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(marker_diff(&game) > 0, "the player marker did not draw");
        // The next firing (even phase) restores the clean map — the marker
        // blinks off…
        game.tick_map_player_marker();
        assert_eq!(marker_diff(&game), 0, "the marker did not blink off");
        // …and the one after that blinks it back on.
        game.tick_map_player_marker();
        assert!(marker_diff(&game) > 0, "the marker did not blink back on");

        // Cancel closes the map screen back to the room verbs and disarms the
        // caption (map_screen_cleanup -> seg000:442f).
        game.menu_callback_choice_exit_menu(0, 0);
        assert_eq!(game.get_active_menu_ref(), MenuRef::CommandMenuBuf);
        assert_eq!(game.data_046eb, 0);
        assert_eq!(game.game_screen_mode_flags, 0);
        assert!(game.map_caption_text.is_empty(), "caption not disarmed");
        assert!(
            game.visible_location_markers.is_empty(),
            "marker list not cleared"
        );

        println!("wrote ornithopter_map.png");
    }

    // The live map mouse handlers (mouse_handlers_01ac8): the hover tracker
    // (map_mouse_hover_tracker, seg000:4586) resolves the pointer into
    // data_046fc — a location marker within 9 pixels, a desert compass ray
    // off the player marker, 0xffff inside the empty window, 0 outside — and
    // redraws the hover label strip (map_draw_hover_label, seg000:45de) on
    // every change; the LMB press (map_mouse_lmb_select_destination,
    // seg000:450e) selects the hovered destination, blinking the label,
    // while ignoring clicks on the player's current location. The map-window
    // hot-zone (set_mouse_nav_rect, seg000:432e) drives the hand /
    // travel-arrow cursor shapes (get_mouse_cursor_image_addr, seg000:dc6a).
    // Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored map_hover
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn map_hover_and_destination_click() {
        use crate::{locations::location_ptr, mouse::CursorShapeId};

        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        // set_headless defaults PCM off, so the destination click's narration
        // (check_pcm_enabled gates duck_music_and_start_narration_voice_clip /
        // wait_for_narration_voice_clip) exits up front instead of spinning out
        // its 1000-tick timeout against the test rig's absent audio drain.

        // Depart from the palace entrance (room 1, the outdoor landing-pad
        // view with the parked orni) — the real TAKE AN ORNITHOPTER context,
        // so the departure snapshot and the takeoff animation draw the pad.
        game.location_and_room = 0x2001;
        game.location_appearance = 0x180;
        game.draw_room_game_screen();

        game.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);
        assert!(game.mouse_nav_rect.is_some(), "map hot-zone not installed");

        // The label strip: fg 0xfe pixels along the ornithopter-mode pen rows
        // (y 0x26 + 7 small-font rows).
        let strip = |g: &GameState| -> usize {
            let px = g.screen.pixels();
            (0x26..0x26 + 7)
                .map(|y| (0x55..240).filter(|&x| px[y * 320 + x] == 0xfe).count())
                .sum()
        };

        // Hovering the palace marker (the player's location, at the window
        // centre) caches its location ptr, kills the caption typewriter and
        // draws the type + name label.
        let palace = *game
            .visible_location_markers
            .iter()
            .find(|m| m.location_index == 0)
            .expect("the palace marker is missing");
        game.mouse_pos_x = palace.x as u16;
        game.mouse_pos_y = palace.y as u16;
        game.map_mouse_hover_tracker();
        assert_eq!(
            game.data_046fc,
            location_ptr(0),
            "palace marker not hovered"
        );
        assert!(game.map_caption_text.is_empty(), "caption not disarmed");
        let palace_label = strip(&game);
        assert!(palace_label > 0, "no hover label drawn");

        // Clicking the player's current location is inert (seg000:452d).
        game.map_mouse_lmb_select_destination();
        assert_eq!(game.data_04732, 0, "a click on the current location armed");

        // Due north of the player marker's tip (rect x0+11, y1) the tracker
        // caches compass ray 0 and the label reads DESERT + the direction.
        let m = game.map_player_marker_rect;
        let (ax, ay) = (m.x0 + 0xb, m.y1);
        game.mouse_pos_x = ax as u16;
        game.mouse_pos_y = (ay - 20) as u16;
        // Probe precondition: no marker within the 9-pixel hover radius.
        assert!(
            game.visible_location_markers
                .iter()
                .all(|mk| mk.x.abs_diff(ax) + mk.y.abs_diff(ay - 20) >= 9),
            "probe point sits on a marker; pick another"
        );
        game.map_mouse_hover_tracker();
        assert_eq!(game.data_046fc, 0xfff0, "due north is not compass ray 0");
        assert!(strip(&game) > 0, "no DESERT label drawn");

        // Off-ray (32*(-20)/25 lands mid-octant) the state is the plain
        // in-window 0xffff and the label is DESERT alone.
        game.mouse_pos_x = (ax + 25) as u16;
        game.mouse_pos_y = (ay - 20) as u16;
        assert!(
            game.visible_location_markers
                .iter()
                .all(|mk| mk.x.abs_diff(ax + 25) + mk.y.abs_diff(ay - 20) >= 9),
            "probe point sits on a marker; pick another"
        );
        game.map_mouse_hover_tracker();
        assert_eq!(game.data_046fc, 0xffff, "off-ray hover is not 0xffff");

        // map_screen_to_position (seg000:b5f9) inverts the marker projection:
        // a screen point inside the window round-trips through
        // map_position_to_screen back to the same pixel (up to the one-cell
        // truncation of the cell -> longitude division).
        let (mx, mlat) = game.map_screen_to_position(palace.x, palace.y);
        assert_eq!(mlat, game.zoomed_globe_latitude);
        let (sx, sy) = game.map_position_to_screen(mx, mlat);
        assert!(
            (sx - palace.x).abs() <= 1 && sy == palace.y,
            "inverse projection did not round-trip: ({sx},{sy}) vs ({},{})",
            palace.x,
            palace.y
        );

        // arm_pending_travel's desert branch (seg000:494c): a compass-ray
        // state converts the click to a map cell and arms a fixed heading
        // (mode 1, destination = the last location, travel_no_location_dest = 0xff so the
        // map verbs switch to the CHANGE DESTINATION pair).
        game.arm_pending_travel(0xfff0, ax, ay - 20);
        assert_eq!(game.travel_heading_mode, 1);
        assert_eq!(game.travel_no_location_dest, 0xff);
        assert_eq!(
            game.travel_destination_ptr,
            location_ptr(game.last_location_index as u16)
        );
        assert_eq!(game.travel_step_accum, 0x80);
        // Reset the armed state so the marker click below starts clean.
        game.travel_destination_ptr = 0;
        game.travel_heading_mode = 0;
        game.travel_no_location_dest = 0;

        // Outside the map window the state drops to 0, the label strip is
        // space-padded clean and the caption typewriter re-arms.
        game.mouse_pos_x = 10;
        game.mouse_pos_y = 10;
        game.map_mouse_hover_tracker();
        assert_eq!(game.data_046fc, 0, "outside the window is not 0");
        assert_eq!(strip(&game), 0, "the label strip was not erased");
        assert!(!game.map_caption_text.is_empty(), "caption not re-armed");

        // The hot-zone cursor shapes: hand inside the window, travel arrows
        // in the scroll bands outside its edges (redraw_mouse latches the
        // shape into cursor_image).
        let r = game.map_view_rect;
        for (x, y, shape) in [
            (palace.x, palace.y, CursorShapeId::Hand),
            (r.x0 - 10, palace.y, CursorShapeId::Left),
            (r.x1 + 10, palace.y, CursorShapeId::Right),
            (palace.x, r.y0 - 10, CursorShapeId::Up),
            (palace.x, r.y1 + 10, CursorShapeId::Down),
            (10, 10, CursorShapeId::Arrow),
        ] {
            game.mouse_pos_x = x as u16;
            game.mouse_pos_y = y as u16;
            game.redraw_mouse();
            assert_eq!(game.cursor_image, Some(shape), "wrong cursor at ({x},{y})");
        }

        // Clicking a different location's marker selects it as the travel
        // destination: the label blinks 9 times, then the confirm chain
        // (map_confirm_travel_and_close, seg000:4703) arms the pending
        // travel, plays the ornithopter takeoff and closes the map screen
        // back to the room.
        let other = *game
            .visible_location_markers
            .iter()
            .find(|mk| mk.location_index != 0)
            .expect("no second marker to click");
        game.mouse_pos_x = other.x as u16;
        game.mouse_pos_y = other.y as u16;
        game.map_mouse_hover_tracker();
        assert_eq!(game.data_046fc, location_ptr(other.location_index));
        let pad_ornis_before = game.locations[game.last_location_index]
            .equipment
            .ornithopters;
        // The expected heading, computed while the player still stands at
        // the palace (the click detaches current_location_ptr).
        let expected_heading = game.compass_angle_to_location(other.location_index as usize);
        game.map_mouse_lmb_select_destination();

        // The pending travel is armed at the clicked location: a marker click
        // homes toward the destination (heading mode 0, re-aimed each step by
        // the pump), and the takeoff consumed the data_04732 arm bit.
        assert_eq!(
            game.travel_destination_ptr,
            location_ptr(other.location_index),
            "the destination was not armed"
        );
        assert_eq!(game.travel_heading_mode, 0);
        assert_eq!(game.data_04732, 0, "the takeoff did not consume the arm");
        // The heading matches the destination's compass angle from the
        // palace, and the step accumulator re-seeded to half a cell.
        assert_eq!(game.travel_heading, expected_heading);
        assert_eq!(game.travel_step_accum, 0x80);

        // The confirm chain closed the map screen itself (no Cancel): the
        // room element owns the stack again, the hot-zone is gone, and the
        // mode flags folded the map bit into the orni-travel bit — kept by
        // the cleanup because a travel is pending.
        assert_eq!(game.get_active_menu_ref(), MenuRef::CommandMenuBuf);
        assert!(game.mouse_nav_rect.is_none(), "hot-zone not cleared");
        assert_eq!(game.game_screen_mode_flags, 5, "mode flags dropped");
        assert_eq!(game.data_046eb, 0);

        // The departure state: the player detached from the location, the
        // scene forced to the no-scene sentinel, the travel pump armed, one
        // step counted, and the departing orni left the palace pad.
        assert_eq!(game.current_location_index, 0xffff);
        assert_eq!(game.data_00008, 0xff);
        assert_eq!(game.travel_active, 0xff, "the travel pump was not armed");
        assert_eq!(game.travel_step_counter, 1);
        assert_eq!(game.travel_step_tick_stamp, 0);
        assert_eq!(
            game.locations[game.last_location_index]
                .equipment
                .ornithopters,
            pad_ornis_before.wrapping_sub(1),
            "the departing orni did not leave the pad"
        );
        // The takeoff animation ran to completion and the SKIP TO DESTINATION
        // verb was un-greyed on the template.
        assert_eq!(game.orni_anim_frame, 0x21);
        assert_eq!(game.cmd_skip_to_destination_flags, 0);

        game.screen
            .write_png_scaled(&game.palette, "ornithopter_departure.png")
            .expect("write ornithopter_departure.png");

        // The hot-zone went with the map screen: the cursor is the plain
        // arrow again everywhere.
        game.mouse_pos_x = palace.x as u16;
        game.mouse_pos_y = palace.y as u16;
        game.redraw_mouse();
        assert_eq!(game.cursor_image, Some(CursorShapeId::Arrow));
    }

    // Map scrolling: the alternate nav panel's arrows (NAV_PANEL_ALT,
    // seg001:1cca, handlers ui_click_map_up/right/down/left -> ui_click_map_
    // buttons, seg000:8831) move zoomed_globe_longitude/latitude by the
    // map_scroll_delta_* pairs (seg001:145e) and redraw through
    // map_refresh_main_view (seg000:8850); the travel-arrow cursor shapes
    // resolve to the same records as pseudo hits (set_di_to_ui_elements_ptr_
    // based_on_cursor_image, seg000:d694), so a click anywhere with an arrow
    // cursor scrolls too; the centre button (ui_click_map_center, seg000:5b05)
    // recentres on the player. Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored map_scrolling
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn map_scrolling() {
        use crate::mouse::CursorShapeId;

        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        game.location_and_room = 0x2001;
        game.location_appearance = 0x180;
        game.draw_room_game_screen();

        game.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);

        // The map screen installed map_view_redraw as the main-view drawing
        // function (seg000:4346); every scroll below dispatches through it
        // (map_refresh_main_view, seg000:8853).
        assert!(
            game.current_main_view_drawing_function.is_some(),
            "map_screen_open did not install the main-view redraw"
        );

        // The view opens centred on the player (set_zoomed_globe_pos_from_
        // map_position, seg000:4320) with the palace marker at the window
        // centre.
        let (lng0, lat0) = (game.zoomed_globe_longitude, game.zoomed_globe_latitude);
        let r = game.map_view_rect;
        let centre_x = r.x0 + (r.x1 - r.x0) / 2;
        let centre_y = r.y0 + (r.y1 - r.y0 - 1) / 2;
        let palace_marker = |g: &GameState| {
            *g.visible_location_markers
                .iter()
                .find(|m| m.location_index == 0)
                .expect("the palace marker is missing")
        };
        assert_eq!(
            (palace_marker(&game).x, palace_marker(&game).y),
            (centre_x, centre_y)
        );

        // map_draw_zoomed_globe clamps the view latitude to ±(0x56 - half the
        // window height) so every row has a tablat entry (seg000:b74a).
        let max_lat = 0x56 - ((r.y1 - r.y0 - 1) / 2);
        let window_pixels = |g: &GameState| -> Vec<u8> {
            let fb = g.framebuffer.pixels();
            (r.y0..r.y1)
                .flat_map(|y| {
                    fb[y as usize * 320 + r.x0 as usize..y as usize * 320 + r.x1 as usize].to_vec()
                })
                .collect()
        };
        let before = window_pixels(&game);

        // Let the caption typewriter put a few glyphs of "SELECT DESTINATION
        // ON MAP" on the VISIBLE screen — fb1 never holds them
        // (tick_map_caption draws to the front buffer only), so they survive
        // a scroll only because map_view_redraw pushes just the map window
        // rect (update_screen_at_sprite_rect_updating_head, seg000:4399),
        // not the whole game area.
        for _ in 0..6 {
            game.tick_map_caption();
        }
        let caption = |g: &GameState| -> usize {
            let px = g.screen.pixels();
            (0x26..0x26 + 7)
                .map(|y| (0x55..240).filter(|&x| px[y * 320 + x] == 0x61).count())
                .sum()
        };
        let typed = caption(&game);
        assert!(typed > 0, "no caption glyphs typed");

        // A fresh LMB press on the up arrow (live HUD record 13) dispatches
        // ui_click_map_up through the shared hit-test: the view moves 12 rows
        // north, the redraw rebuilds the markers (the palace slides 12 rows
        // south of centre), and the 0x4000 flag arms the held auto-repeat.
        game.prev_mouse_buttons = 1;
        game.mouse_pos_x = 275;
        game.mouse_pos_y = 166;
        game.redraw_mouse(); // over the HUD: the plain arrow, no pseudo hit
        assert_eq!(game.cursor_image, Some(CursorShapeId::Arrow));
        assert_eq!(game.hit_test_ui_elements(), Some(13), "up arrow not hit");
        game.game_loop_dispatch_lmb_press();
        let lat_up = (lat0 - 12).clamp(-max_lat, max_lat);
        assert_eq!(
            game.zoomed_globe_latitude, lat_up,
            "up did not scroll north"
        );
        assert_eq!(game.zoomed_globe_longitude, lng0, "up moved the longitude");
        assert_eq!(game.drag_armed_element, Some(13), "auto-repeat not armed");
        assert_eq!(palace_marker(&game).y, centre_y + (lat0 - lat_up));
        assert_ne!(
            window_pixels(&game),
            before,
            "the map window did not redraw"
        );
        assert_eq!(caption(&game), typed, "the scroll erased the typed caption");

        // The right arrow (record 14) steps the longitude east by 0x1002.
        game.mouse_pos_x = 290;
        game.mouse_pos_y = 178;
        game.redraw_mouse();
        assert_eq!(game.hit_test_ui_elements(), Some(14), "right arrow not hit");
        game.game_loop_dispatch_lmb_press();
        assert_eq!(game.zoomed_globe_longitude, lng0.wrapping_add(0x1002));
        assert_eq!(game.zoomed_globe_latitude, lat_up);

        // In the scroll band left of the map window the cursor becomes the
        // left travel arrow, and the hit-test resolves the pseudo record 16
        // (seg000:d694) — clicking there scrolls back west.
        game.mouse_pos_x = (r.x0 - 10) as u16;
        game.mouse_pos_y = centre_y as u16;
        game.redraw_mouse();
        assert_eq!(game.cursor_image, Some(CursorShapeId::Left));
        assert_eq!(game.hit_test_ui_elements(), Some(16), "no pseudo arrow hit");
        game.game_loop_dispatch_lmb_press();
        assert_eq!(
            game.zoomed_globe_longitude, lng0,
            "left did not scroll west"
        );
        assert_eq!(game.drag_armed_element, Some(16));

        // The release edge's final armed-element fire arrives with the LMB
        // bit clear: ui_click_map_buttons is a no-op (test al,1; seg000:8831).
        game.prev_mouse_buttons = 0;
        game.ui_click_map_up();
        assert_eq!(
            game.zoomed_globe_latitude, lat_up,
            "a release-edge fire scrolled"
        );

        // The centre button (record 12) recentres the view on the player.
        game.prev_mouse_buttons = 1;
        game.mouse_pos_x = 275;
        game.mouse_pos_y = 178;
        game.redraw_mouse();
        assert_eq!(
            game.hit_test_ui_elements(),
            Some(12),
            "centre button not hit"
        );
        game.game_loop_dispatch_lmb_press();
        assert_eq!(
            (game.zoomed_globe_longitude, game.zoomed_globe_latitude),
            (lng0, lat0),
            "the centre button did not recentre on the player"
        );
        assert_eq!(
            (palace_marker(&game).x, palace_marker(&game).y),
            (centre_x, centre_y)
        );
        assert_eq!(
            window_pixels(&game),
            before,
            "recentre did not restore the view"
        );
        assert_eq!(caption(&game), typed, "scrolling erased the typed caption");
    }

    // The in-game travel flight: after the destination click departs, the
    // scene reload's travel branch (loc_037f4) opens the flight HNM (MNT1,
    // video id 2) and builds the minimap + trail in the back buffer; the
    // travel pump (travel_pump, seg000:4f0c) then advances the position one
    // step per 0x300 ticks along the homing heading (travel_step_position,
    // seg000:5206), records the trail, keeps the minimap centred, re-picks
    // the flight clip from the terrain ahead, and arrives when the position's
    // map offset matches the destination's (entering its room via
    // desert_check_arrival, seg000:4002). Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored travel_flight
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn travel_flight_to_destination() {
        use crate::locations::location_index_from_ptr;

        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        game.location_and_room = 0x2001;
        game.location_appearance = 0x180;
        game.draw_room_game_screen();

        // Depart: open the map and click a non-current location's marker.
        game.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);
        let other = *game
            .visible_location_markers
            .iter()
            .find(|mk| mk.location_index != 0)
            .expect("no second marker to click");
        game.mouse_pos_x = other.x as u16;
        game.mouse_pos_y = other.y as u16;
        game.map_mouse_lmb_select_destination();
        assert_eq!(game.travel_active, 0xff, "the travel pump was not armed");

        // The departure's scene reload took the travel branch (loc_037f4):
        // the flight HNM is open on MNT1 (video id 2 = the vehicle), the
        // minimap rect owns the map view, and the flight redraw is installed.
        assert!(game.hnm_is_open(), "the flight HNM did not open");
        assert_eq!(game.hnm_video_id, 2, "the flight did not start on MNT1");
        assert_eq!(game.map_view_rect.x0, 0xcc, "the minimap rect is not live");
        // The minimap landed in the back buffer and was stamped over the
        // flight frame at present time: the SCREEN's minimap window holds map
        // pixels (palette bank 1, incl. the 0x17..0x1c globe-edge shades).
        // fb1 itself holds the clean NEXT frame — the streaming pipeline
        // (hnm_present_flight_frame's loc_0caa0 prefetch) decodes it over the
        // stamp right after each present, so the stamp never lingers in fb1.
        let minimap_pixels = |g: &GameState| -> usize {
            let fb = g.screen.pixels();
            (4..60)
                .map(|y| {
                    (0xcc..0x13c)
                        .filter(|&x| (0x10..0x20).contains(&fb[y * 320 + x]))
                        .count()
                })
                .sum()
        };
        // One pump pass so a flight present lands on the visible screen (the
        // departure itself composed offscreen behind the reveal transition).
        // The step timer is re-armed so this pass advances no travel step.
        game.hnm_last_frame_tick = 0;
        game.travel_step_tick_stamp = game.game_ticks() as u16;
        game.travel_pump();
        let lit = minimap_pixels(&game);
        assert!(
            lit > 3000,
            "the minimap did not stamp over the presented frame: {lit}"
        );

        // Fly: force the 0x300-tick step cadence each pump pass until the
        // arrival disarms the pump.
        let dest = location_index_from_ptr(game.travel_destination_ptr);
        let pad_ornis_before = game.locations[dest].equipment.ornithopters;
        let mut steps = 0;
        while game.travel_active != 0 && steps < 2000 {
            game.travel_step_tick_stamp = (game.game_ticks() as u16).wrapping_sub(0x300);
            game.travel_pump();
            steps += 1;
            if steps == 5 {
                // A mid-flight frame: the trail + live marker over the map.
                game.screen
                    .write_png_scaled(&game.palette, "travel_flight.png")
                    .expect("write travel_flight.png");
            }
        }
        assert!(steps < 2000, "the flight never arrived after {steps} steps");
        println!("arrived after {steps} steps");

        // The arrival entered the destination's room: it is the current
        // location, the destination/mode state tore down, and the orni landed
        // on its pad.
        assert_eq!(
            game.current_location_index as usize, dest,
            "did not arrive at the clicked destination"
        );
        assert_eq!(game.last_location_index, dest);
        assert_eq!(game.travel_destination_ptr, 0);
        assert_eq!(game.game_screen_mode_flags, 0);
        // The arrival's scene reload ran the landing sequence
        // (travel_arrival_landing_sequence, seg000:488a) and cleared the
        // arrival-overlay bit at its loc_048d5 tail.
        assert_eq!(game.data_04732, 0, "the landing sequence did not run");
        assert_eq!(game.location_appearance & 0xff, 0x80, "not in a room view");
        assert_eq!(
            game.locations[dest].equipment.ornithopters,
            pad_ornis_before + 1,
            "the orni did not land on the destination pad"
        );

        // The flight recorded a trail (the ring holds real positions now).
        let dots = game
            .travel_trail_ring
            .iter()
            .filter(|&&(_, lat)| lat != 0x800)
            .count();
        assert!(dots > 5, "the trail ring stayed empty: {dots}");
        assert_eq!(dots, steps.min(crate::game_state::TRAVEL_TRAIL_LEN));

        game.screen
            .write_png_scaled(&game.palette, "travel_arrival.png")
            .expect("write travel_arrival.png");
    }

    // The four map-mode verbs (TOWARDS NEAREST PLACE seg000:50c4, CHANGE
    // DESTINATION 497a, BACK TO STARTING POINT 50a5, SKIP TO DESTINATION
    // 4ffb) driven mid-flight through their verb callbacks.
    // Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored map_mode_verbs
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn map_mode_verbs() {
        use crate::locations::{location_index_from_ptr, location_ptr};

        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        game.location_and_room = 0x2001;
        game.location_appearance = 0x180;
        game.draw_room_game_screen();

        // Depart towards some other location and fly a few steps out.
        game.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);
        let other = *game
            .visible_location_markers
            .iter()
            .find(|mk| mk.location_index != 0)
            .expect("no second marker to click");
        game.mouse_pos_x = other.x as u16;
        game.mouse_pos_y = other.y as u16;
        game.map_mouse_lmb_select_destination();
        assert_eq!(game.travel_active, 0xff, "the travel pump was not armed");
        // = the confirm's update_room_music (seg000:4715): the orni-travel
        // situation (index 6) queues the flight theme (song 5) and begins
        // fading the room theme out (midi_begin_song_fade_out, seg000:adbe).
        // set_headless() keeps the music gates closed (cmd_args_memory bit 4),
        // so re-run the selector with the gate opened; nothing is playing, so
        // no audio starts.
        game.cmd_args_memory &= !0x10;
        game.update_room_music();
        game.cmd_args_memory |= 0x10;
        assert_eq!(
            game.music_desired_song, 5,
            "the flight theme was not queued"
        );
        for _ in 0..5 {
            game.travel_step_tick_stamp = (game.game_ticks() as u16).wrapping_sub(0x300);
            game.travel_pump();
        }

        // TOWARDS NEAREST PLACE re-arms a homing travel at the nearest
        // non-hidden location.
        game.menu_callback_choice_towards_nearest_place(0, 0);
        let nearest = location_index_from_ptr(game.travel_destination_ptr);
        assert!(
            game.locations[nearest].status & 0x80 == 0,
            "a hidden nearest"
        );
        assert_eq!(game.travel_heading_mode, 0);
        assert_eq!(game.travel_no_location_dest, 0);
        assert_eq!(
            game.travel_step_accum, 0x80,
            "the step accumulator did not re-seed"
        );

        // CHANGE DESTINATION reopens the map view over the flight with the
        // Cancel menu; travel_minimap_state 1 re-enters the flight on close.
        game.menu_callback_choice_change_destination(0, 0);
        assert_eq!(game.data_046eb, 1, "the map view did not reopen");
        assert_eq!(game.travel_minimap_state, 1);
        assert!(game.mouse_nav_rect.is_some(), "map hot-zone not installed");
        // Clicking a marker re-arms the destination; the already-travelling
        // confirm chain (old flags & 3 != 0) closes the map without a second
        // departure.
        let target = *game
            .visible_location_markers
            .first()
            .expect("no marker on the reopened map");
        game.mouse_pos_x = target.x as u16;
        game.mouse_pos_y = target.y as u16;
        game.map_mouse_lmb_select_destination();
        assert_eq!(game.data_046eb, 0, "the map view did not close");
        assert_eq!(game.travel_active, 0xff, "the travel pump disarmed");
        assert_eq!(
            game.travel_destination_ptr,
            location_ptr(target.location_index),
            "the destination did not change"
        );

        // BACK TO STARTING POINT aims home at the departure location.
        let start = game.last_location_index;
        game.menu_callback_choice_back_to_starting_point(0, 0);
        assert_eq!(game.travel_destination_ptr, location_ptr(start as u16));
        assert_eq!(game.travel_heading_mode, 0);
        assert_eq!(game.travel_no_location_dest, 0);

        // SKIP TO DESTINATION fast-forwards the travel and lands it: the pump
        // disarms, the mode flags clear and the start location's room is
        // re-entered with the orni parked back on its pad.
        let pad_ornis_before = game.locations[start].equipment.ornithopters;
        game.menu_callback_choice_skip_to_destination(0, 0);
        assert_eq!(game.travel_active, 0, "the skip did not land the travel");
        assert_eq!(game.travel_destination_ptr, 0);
        assert_eq!(game.game_screen_mode_flags, 0);
        assert_eq!(game.current_location_index as usize, start);
        assert_eq!(game.location_appearance & 0xff, 0x80, "not in a room view");
        assert_eq!(
            game.locations[start].equipment.ornithopters,
            pad_ornis_before + 1,
            "the orni did not land on the pad"
        );
    }

    // A departure from a sietch keeps the UI palette span intact. The sietch
    // entry rooms use sprite-sheet index 0 (GENERIC.HSQ), which draw_SAL never
    // opens (seg000:3b62..3b68) — GENERIC's [240..254] sand-tint palette chunk
    // must NOT stamp over the sky palette's time-of-day UI span during the
    // departure re-render (seg000:47e6), or the takeoff's per-frame palette
    // flush (seg000:482b, ORNYTK.HSQ carries no palette chunk) shows a yellow
    // UI for the whole animation. Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored sietch_departure
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn sietch_departure_keeps_the_ui_palette() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // Fly from the palace pad to a sietch and land there.
        game.location_and_room = 0x2001;
        game.location_appearance = 0x180;
        game.draw_room_game_screen();
        game.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);
        let sietch = *game
            .visible_location_markers
            .iter()
            .find(|mk| {
                mk.location_index != 0
                    && game.locations[mk.location_index as usize].appearance < 0x20
            })
            .expect("no sietch marker visible");
        game.mouse_pos_x = sietch.x as u16;
        game.mouse_pos_y = sietch.y as u16;
        game.map_mouse_lmb_select_destination();
        let mut steps = 0;
        while game.travel_active != 0 && steps < 2000 {
            game.travel_step_tick_stamp = (game.game_ticks() as u16).wrapping_sub(0x300);
            game.travel_pump();
            steps += 1;
        }
        assert!(steps < 2000, "never arrived");
        let loc = game.current_location_index as usize;
        assert!(
            game.locations[loc].appearance < 0x20,
            "did not land at a sietch"
        );

        // The landed room shows the sky palette's UI span (SKYDN through the
        // in-game set_sky_palette), on screen and staged.
        let ui_before: Vec<_> = (240..256).map(|i| game.screen_pal.get(i)).collect();

        // The departure re-render draws the scene with the ornis hidden
        // (seg000:47de orni_anim_frame = 0xff), so no orni-pass
        // set_sky_palette runs after the SAL draw — the staged UI span must
        // survive the GENERIC-sheet SAL draw itself. (With ornis visible the
        // pass's set_sky_palette tail masks any stamp, so probe exactly the
        // hidden-orni render the confirm chain uses.)
        game.orni_anim_frame = 0xff;
        game.draw_room_scene();
        game.orni_anim_frame = 0;
        for i in 240..256 {
            assert_eq!(
                game.palette.get(i),
                ui_before[i - 240],
                "staged UI palette entry {i} changed across the hidden-orni re-render"
            );
        }

        // Take an ornithopter again and select another sietch: the confirm
        // chain (head fold, departure re-render, takeoff animation, flight
        // view) must never disturb the UI span on the visible screen.
        game.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);
        let target = *game
            .visible_location_markers
            .iter()
            .find(|mk| {
                mk.location_index as usize != loc
                    && game.locations[mk.location_index as usize].appearance < 0x20
            })
            .expect("no sietch marker visible");
        game.mouse_pos_x = target.x as u16;
        game.mouse_pos_y = target.y as u16;
        game.map_mouse_lmb_select_destination();
        assert_eq!(game.travel_active, 0xff, "the travel pump was not armed");
        for i in 240..256 {
            assert_eq!(
                game.screen_pal.get(i),
                ui_before[i - 240],
                "UI palette entry {i} changed across the sietch departure"
            );
        }
    }

    // The parked-orni draw has two DOS entry points: the room pass enters at
    // the loop head (draw_ornis_loop, seg000:3a6a — all `count` pad slots),
    // while the takeoff frames (orni_anim_draw_frame, seg000:4821) enter at
    // the loop's step point (draw_ornis, seg000:3a73 — advance and
    // loop-decrement BEFORE drawing), so the departing orni's first slot
    // stays empty under the climbing animation. Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored departing_slot
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn parked_orni_draw_skips_the_departing_slot() {
        use crate::sprite_bank;

        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.set_fb1_as_active_framebuffer();
        game.open_sprite_bank(sprite_bank::ORNY);
        game.orni_anim_frame = 0;

        let fb_nonzero = |g: &GameState| g.framebuffer.pixels().iter().filter(|&&p| p != 0).count();

        // The step-first entry with a single parked orni draws nothing —
        // that one IS the departing orni.
        game.draw_ornis(1, 100, 60);
        assert_eq!(fb_nonzero(&game), 0, "draw_ornis(1) must skip the slot");

        // With two, only the second slot draws: the result matches the loop
        // head drawing one orni at the stepped position (+0x46, +0x0a).
        game.draw_ornis(2, 100, 60);
        let stepped = game.framebuffer.pixels().to_vec();
        assert!(fb_nonzero(&game) > 0, "draw_ornis(2) drew nothing");
        game.gfx_clear_active_framebuffer();
        game.draw_ornis_loop(1, 100 + 70, 60 + 10);
        assert_eq!(
            game.framebuffer.pixels(),
            &stepped[..],
            "draw_ornis(2) must equal one orni at the second slot"
        );
    }

    // The parked ornithopter on the location-entrance landing pad is a live
    // hotspot (person_hit_test's orni tail, seg000:92ab, pseudo-person 0x2f):
    // hovering it highlights the TAKE AN ORNITHOPTER verb (0x78 + 0x2f = its
    // text id 0xa7), and clicking it opens the map screen in ornithopter mode
    // (callback_main_ui_element_21_22's seg000:922a branch).
    // Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored parked_orni
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn parked_orni_hover_highlights_and_click_opens_the_map() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // Move to the palace entrance (room 1, the outdoor landing-pad view;
        // the intro2 game setup parked one ornithopter at the palace).
        game.location_and_room = 0x2001;
        game.location_appearance = 0x180;
        game.draw_room_game_screen();

        // The scene's orni pass recorded the hover hotspot, and the verb menu
        // holds a live (un-greyed) TAKE AN ORNITHOPTER.
        assert_ne!(game.orni_hotspot_x, 0, "orni hotspot not recorded");
        let slot = game
            .active_menu_records()
            .iter()
            .position(|r| r.text_id == 0x00a7)
            .expect("TAKE AN ORNITHOPTER verb missing or greyed") as u8;

        // Hovering the parked orni resolves to pseudo-person 0x2f and
        // highlights the verb slot.
        game.mouse_pos_x = game.orni_hotspot_x + 16;
        game.mouse_pos_y = game.orni_hotspot_y + 16;
        assert_eq!(game.person_hit_test(), Some(0x2f));
        game.highlight_hovered_text_action_item();
        assert_eq!(
            game.index_of_last_hovered_action_item, slot,
            "hovering the orni did not highlight TAKE AN ORNITHOPTER"
        );

        // Moving off the orni drops the hit and the highlight.
        game.mouse_pos_x = 10;
        game.mouse_pos_y = 10;
        assert_eq!(game.person_hit_test(), None);
        game.highlight_hovered_text_action_item();
        assert_eq!(game.index_of_last_hovered_action_item, 0xff);

        // Clicking the orni opens the map screen in ornithopter mode. Drive
        // the real game-loop press dispatch: the game-area hit is a ui-element
        // miss, so it runs callback_main_ui_element_21_22 (which swaps in the
        // map screen) and then the press handler of the record that was live
        // when the press fired — DOS saves si around the callback
        // (seg000:d909/d90d), so that is the room record's no-op, NOT the
        // just-installed map record.
        game.mouse_pos_x = game.orni_hotspot_x + 16;
        game.mouse_pos_y = game.orni_hotspot_y + 16;
        game.game_loop_dispatch_lmb_press();
        assert_eq!(game.get_active_menu_ref(), MenuRef::MenuCancel);
        assert_eq!(game.map_ornithopter_mode, 1);

        // The same press did not leak into the map screen as a destination
        // click (the cursor position lands inside the just-opened map window;
        // a leaked press would narrate a destination, arm the travel and
        // close the map).
        assert_eq!(game.travel_active, 0, "the orni click leaked into the map");
        assert_eq!(game.travel_destination_ptr, 0);

        // The map screen owns the game area now: the hit test's room-view gate
        // (game_screen_mode_flags != 0) keeps further orni clicks inert.
        game.callback_main_ui_element_21_22();
        assert_eq!(game.get_active_menu_ref(), MenuRef::MenuCancel);
    }
}
