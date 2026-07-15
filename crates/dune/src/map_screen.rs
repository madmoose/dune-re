//! The map main view — the windowed desert map the TAKE AN ORNITHOPTER
//! (seg000:42e9) and CALL A WORM (seg000:42d1) verbs open over the room screen.
//! The view draws a one-cell-per-pixel window of MAP.HSQ centred on the
//! player's map position inside data_046e3_rect, with curved globe edges at
//! extreme latitudes; in ornithopter mode the ORNYPAN.HSQ cockpit frames the
//! window. Destination selection, the travel departure and the full-globe
//! (data_046eb bit 0x80) drawing are not ported yet.

use crate::{
    GameState, Rect, TaskId,
    game_ui::{MouseHandlers, NAV_PANEL_ALT},
    gfx,
    rect::rect,
    room_game_screen::{CommandMenuRecord, ScreenElement, rec},
    sprite_bank,
};

/// = seg001:212e menu_multiple_cancel — the single-record command strip the
/// map screen folds in (map_screen_open_with_cancel_menu, bp = 0x212e). DOS
/// stores a leading priority word (0x00f8) and a trailing 0-word fence, both
/// implicit here. The lone verb closes the map screen back to the room.
pub(crate) const MENU_MULTIPLE_CANCEL: [CommandMenuRecord; 1] = [
    rec(0x00a3, 0xd2e2), // menu_callback_choice_exit_menu
];

// = seg001:14b4 icon_list_ornypan_cockpit — the full ORNYPAN.HSQ ornithopter
// cockpit interior. The final entry doubles as ORNYPAN_WINDOW_OVERLAY_ICONS
// (DOS overlaps the two lists; the 0xffff terminator at seg001:14c6 ends both).
const ORNYPAN_COCKPIT_ICONS: [(u16, i16, i16); 3] = [
    (0x0000, 0x00, 0x13),
    (0x0001, 0x0a, 0x2b),
    (0x0002, 0x4f, 0x2d),
];

// = seg001:14c0 icon_list_ornypan_window_overlay — just the cockpit window
// frame (sprite 2), redrawn over the freshly drawn map window on every
// map_view_redraw pass.
const ORNYPAN_WINDOW_OVERLAY_ICONS: [(u16, i16, i16); 1] = [(0x0002, 0x4f, 0x2d)];

// = seg001:149c map_view_rect_template — the map window rect, copied into
// data_046e3_rect (map_view_rect) when the map screen opens (seg000:432e).
const MAP_VIEW_RECT_TEMPLATE: Rect = rect(81, 45, 241, 134);

/// = seg001:1ac8 mouse_handlers_01ac8 — the map screen's MouseHandlers record.
/// The idle and both drag slots run the hover tracker (loc_04586: the location
/// marker / travel-arrow hover state in data_046fc); the LMB press is the
/// destination click (loc_0450e). Both are TODO stubs — the live map
/// interaction is not ported.
pub(crate) static MAP_MOUSE_HANDLERS: MouseHandlers = MouseHandlers {
    idle: GameState::map_mouse_idle,
    lmb: GameState::map_mouse_lmb,
    rmb: GameState::map_mouse_rmb,
    release: GameState::map_mouse_release,
    rmb_release: GameState::map_mouse_rmb_release,
    drag: GameState::map_mouse_drag,
    rmb_drag: GameState::map_mouse_rmb_drag,
};

impl GameState {
    // = seg000:42e9 menu_callback_choice_map_main_take_an_ornithopter_notransition
    // — the TAKE AN ORNITHOPTER room verb (command record seg001:21dc): open the
    // map screen in ornithopter (cockpit) mode. Also reached from the room
    // ornithopter click (callback_main_ui_element_21_22, seg000:9282) and as the
    // fall-through tail of the map-main-menu entry (seg000:42d9, not ported).
    pub(crate) fn menu_callback_choice_map_main_take_an_ornithopter_notransition(&mut self) {
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
        self.map_reset_travel_state();
        self.map_screen_open(MENU_MULTIPLE_CANCEL.to_vec());
    }

    // = seg000:430b map_screen_open — open the map main view. DOS bp = the
    // command-menu record buffer to fold in (the port's `records`).
    pub(crate) fn map_screen_open(&mut self, records: Vec<CommandMenuRecord>) {
        // = seg000:430b bx=map_screen_cleanup; call loc_0d323 — request the
        //   panel transition, push the screen element (cleanup func =
        //   map_screen_cleanup by the MapScreen identity), fold the menu in,
        //   then refresh the hover highlight.
        self.screen_overlay_request_transition();
        self.screen_element_stack_push(ScreenElement::MapScreen, records);
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
        //   call set_some_mouse_rect; call copy_rect_at_si_to_di — install the
        //   map window rect. The mouse hotspot half (set_some_mouse_rect) is
        //   TODO with the live map interaction.
        self.map_view_rect = MAP_VIEW_RECT_TEMPLATE;
        // = seg000:433a call map_screen_draw_base.
        self.map_screen_draw_base();
        // = seg000:433d ax=2bch; call start_narration_voice_clip — "select
        //   destination on map".
        self.start_narration_voice_clip(0x2bc);
        // = seg000:4343 call map_add_select_destination_text_task.
        self.map_add_select_destination_text_task();
        // = seg000:4346 current_main_view_drawing_function = map_view_redraw.
        //   The port has no function-pointer main-view redraw yet; the live
        //   game loop hookup is TODO (callers invoke map_view_redraw directly).
        // = seg000:434c call loc_05b93 — clip sprites to the map window. The
        //   port passes clip rects per draw call instead of storing the segvga
        //   clip rect; the marker draws (TODO) take map_view_rect directly.
        // = seg000:434f call map_draw_zoomed_globe.
        self.map_draw_zoomed_globe();
        // = seg000:4352 call load_icones_sprites.
        self.open_icones_spritesheet();
        // = seg000:4355 call loc_05dce — build + draw the visible-location
        //   markers over the map window. TODO: not ported.
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
        // = seg000:4371 call map_arm_location_marker_task.
        self.map_arm_location_marker_task();
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
        // = seg000:4383 call loc_05dce — the visible-location markers. TODO.
        // = seg000:4386..4396 in ornithopter mode redraw the cockpit window
        //   frame over the map.
        if self.map_ornithopter_mode != 0 {
            self.open_sprite_bank(sprite_bank::ORNYPAN);
            self.with_active_bank_sheet(|s, sheet| {
                s.draw_icons_list_at_si(&ORNYPAN_WINDOW_OVERLAY_ICONS, sheet);
            });
        }
        // = seg000:4399 call update_screen_at_sprite_rect_updating_head — push
        //   the redrawn sprite rect to the screen. The port pushes the whole
        //   game area (= present_game_area) until the sprite-rect tracking is ported.
        self.present_game_area();
        // = seg000:439c jmp map_arm_location_marker_task.
        self.map_arm_location_marker_task();
    }

    // = seg000:4415 map_screen_cleanup — the map screen element's cleanup func
    // (the DOS bx passed to the screen-element push at seg000:430b), run when
    // the element pops (the Cancel verb / menu_callback_choice_exit_menu).
    pub(crate) fn map_screen_cleanup(&mut self) {
        // = seg000:4415 xor al,al; xchg al,[data_046eb]; jnz — only once per
        //   open; data_046eb also drops back to the room nav panel.
        if std::mem::take(&mut self.data_046eb) == 0 {
            return;
        }
        // = seg000:4420 data_0a5c0 = 0 — clear the visible-location marker
        //   list head. TODO: the marker list (loc_05dce) is not ported.
        // = seg000:4426 call clear_some_mouse_rect.
        self.clear_some_mouse_rect();
        // = seg000:4429 si=frame_task_callback_044ab; call remove_frame_task —
        //   TODO: the marker blink task is not ported (see
        //   map_arm_location_marker_task).
        // = seg000:442f call map_remove_select_destination_text_task.
        self.map_remove_select_destination_text_task();
        // = seg000:4432 call copy_game_area_rect_to_unknown_rect.
        self.copy_game_area_rect_to_unknown_rect();
        // = seg000:4435 call loc_043e3 — restore the room backdrop.
        self.map_screen_restore_room_view();
        // = seg000:4438 call update_screen_palette.
        self.update_screen_palette();
        // = seg000:443b cmp data_011c5,0; jnz — keep the mode flags while a
        //   travel is pending. The travel machinery (data_011c5) is not ported,
        //   so the reset is unconditional.
        // = seg000:4442 game_screen_mode_flags = 0.
        self.game_screen_mode_flags = 0;
        // = seg000:4447 call select_room_ui_table.
        self.select_room_ui_table();
        // = seg000:444a call ui_setup_and_draw_nav_panel — data_046eb is 0
        //   again, so this reinstalls the room (or map/book) nav panel.
        self.ui_setup_and_draw_nav_panel();
        // = seg000:444d call rebuild_and_draw_room_nav_panel.
        self.rebuild_and_draw_room_nav_panel();
        // = seg000:4450 cmp data_04728,0; jle — with a destination armed, enter
        //   the travel departure (loc_049d4). TODO: travel is not ported.
        if self.data_04728 > 0 {
            println!("map_screen_cleanup: travel departure (loc_049d4) not ported");
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
        // = seg000:b6c3 test data_046eb,80h — the full interpolated globe
        //   (vga_draw_map_zoomed, centre (0xa0,0x4c), latitude clamped ±0x4b).
        //   TODO: wire the standalone MapRenderer port up to this path.
        if self.data_046eb & 0x80 != 0 {
            println!("map_draw_zoomed_globe: full-globe mode not ported");
            return;
        }
        // = seg000:b714 loc_0b714 — the windowed map. Window geometry from
        //   data_046e3_rect: width (data_0dcf2), height (data_0dcf4) and centre
        //   (data_0dcf6/data_0dcf8, unused until the marker draws are ported).
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
            //   tablat entry's +6 scratch word (not modelled; nothing ported
            //   reads it yet).
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
            let src = &self.map[row_off..row_off + row_len];
            if avail >= eff_w {
                dst[..eff_w].copy_from_slice(&src[left..left + eff_w]);
            } else {
                dst[..avail].copy_from_slice(&src[left..]);
                dst[avail..eff_w].copy_from_slice(&src[..eff_w - avail]);
            }
        }

        // = seg000:b7c6 test data_046eb,40h; jnz — bit 0x40 suppresses the blit.
        if self.data_046eb & 0x40 == 0 {
            // = seg000:b7cd call [gfx_vtable_vga_blit_shaded].
            gfx::vga_blit_shaded(self, &rows, width, height, r.x0, r.y0, top_lat);
        }
    }

    // = seg000:49ea loc_049ea — reset the travel-path scratch: data_04728 = 0
    // and every cmd_arg_list (seg000:e40c, the cs-resident travel waypoint
    // array) word refilled with 0x800. The waypoint array is not modelled yet.
    fn map_reset_travel_state(&mut self) {
        self.data_04728 = 0;
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
        self.add_frame_task(0x18, TaskId::MapCaption);
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
            // = seg000:46c4 si=data_014a4; call restore_mouse_if_rect_intersects
            //   — lift the cursor off the caption strip before drawing. The
            //   port's cursor save/restore bracket is game_loop-driven; TODO
            //   with the live cursor-over-caption case.
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
            //   draw_mouse_cursor_if_needed (the cursor bracket, see above).
            self.active_fb = saved;
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

    // = seg000:445d map_arm_location_marker_task — resolve the location at the
    // current map position and (re-)arm the frame_task_callback_044ab marker
    // blink task over it. TODO: the marker task is not ported.
    fn map_arm_location_marker_task(&mut self) {}

    // = seg000:4586 loc_04586 — [si] of mouse_handlers_01ac8: the idle/drag
    // hover tracker (location-marker + travel-arrow hover state in data_046fc).
    // TODO: the live map interaction is not ported.
    fn map_mouse_idle(&mut self) {}

    // = seg000:450e loc_0450e — [si+2] of mouse_handlers_01ac8: the LMB press,
    // the destination click. TODO: not ported.
    fn map_mouse_lmb(&mut self) {}

    // = [si+4]/[si+6]/[si+8] of mouse_handlers_01ac8 — the RMB and both
    // release slots are the no-op loc_00f66.
    fn map_mouse_rmb(&mut self) {}
    fn map_mouse_release(&mut self) {}
    fn map_mouse_rmb_release(&mut self) {}

    // = [si+0ah]/[si+0ch] of mouse_handlers_01ac8 — both drag slots re-run the
    // hover tracker (loc_04586).
    fn map_mouse_drag(&mut self, _dx: i16, _dy: i16) {
        self.map_mouse_idle();
    }
    fn map_mouse_rmb_drag(&mut self, _dx: i16, _dy: i16) {
        self.map_mouse_idle();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{GameState, dat_file::DatFile, room_game_screen::ScreenElement};

    // TAKE AN ORNITHOPTER (seg000:42e9) opens the map screen: the ORNYPAN
    // cockpit frames a one-cell-per-pixel map window centred on the player's
    // map position, the Cancel menu folds in, and the map screen element owns
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

        game.menu_callback_choice_map_main_take_an_ornithopter_notransition();

        assert_eq!(game.get_active_screen_element(), ScreenElement::MapScreen);
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

        // The composed screen reached the display.
        let frames: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(!frames.is_empty(), "the map screen never presented a frame");
        let (screen, _) = frames.last().unwrap();
        assert_eq!(
            &screen.pixels()[..320 * 137],
            &game.framebuffer.pixels()[..320 * 137],
            "presented game area does not match the composed map screen"
        );

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

        // Cancel closes the map screen back to the room verbs and disarms the
        // caption (map_screen_cleanup -> seg000:442f).
        game.menu_callback_choice_exit_menu();
        assert_eq!(
            game.get_active_screen_element(),
            ScreenElement::RoomCommandMenu
        );
        assert_eq!(game.data_046eb, 0);
        assert_eq!(game.game_screen_mode_flags, 0);
        assert!(game.map_caption_text.is_empty(), "caption not disarmed");

        println!("wrote ornithopter_map.png");
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
            .command_menu_records
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

        // Clicking the orni opens the map screen in ornithopter mode.
        game.mouse_pos_x = game.orni_hotspot_x + 16;
        game.mouse_pos_y = game.orni_hotspot_y + 16;
        game.callback_main_ui_element_21_22();
        assert_eq!(game.get_active_screen_element(), ScreenElement::MapScreen);
        assert_eq!(game.map_ornithopter_mode, 1);

        // The map screen owns the game area now: the hit test's room-view gate
        // (game_screen_mode_flags != 0) keeps further orni clicks inert.
        game.callback_main_ui_element_21_22();
        assert_eq!(game.get_active_screen_element(), ScreenElement::MapScreen);
    }
}
