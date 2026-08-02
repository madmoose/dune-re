//! The GLOBE view — the rotating-planet screen opened from the SEE DUNE MAP
//! view's left-frieze planet ornament (ui_elements[0], seg000:b8c6
//! callback_main_ui_element_00).
//!
//! The screen composes the FRESK blue background + atmosphere ring with the
//! globe pixels rendered inside it (globe_renderer.rs), the FRESK side
//! decorations, the globe frieze sides, and the rotation-control nav panel
//! (ui_globe_rotation_controls[0..6]). The globe rotation frame task keeps
//! the planet spinning one 1/398th revolution per finished draw pass, with
//! the player's position pin (ICONES sprite 0x36) projected onto the disc.
//!
//! Interactions: the four held-repeat arrows tilt (±8 rows) and rotate
//! (±0x20 phase) the view, the centre button animates the view back onto the
//! player, the EXIT GLOBE verb / compass button zooms back out to the map
//! interface, and a click on the disc itself picks the map cell under the
//! cursor (GlobeRenderer::pick_map_position), recentres the map there, and
//! zooms out to the map interface through the expanding-box + 2x-shimmer
//! effect (seg000:bc99).
//!
//! Not ported yet: the SEE RESULTS mode (menu_callback_choice_globe_see_
//! results, seg000:b96b) — the side-decoration slide (seg000:b8f3/b915), the
//! stats overlay (results_draw_text_and_icones, seg000:be1d) and the
//! globe_draw_skips_pixel_stores patch draw stay stubs.

use crate::{
    GameState, Rect,
    game_ui::{MouseHandlers, NAV_PANEL_GLOBE},
    globe_renderer::GLOBE_CLIP_RECT,
    menu_defs::MenuRef,
    sprite_bank,
};

// = seg001:2562 data_02562 — the globe view's MouseHandlers record: only the
// idle hover (globe_mouse_idle) and the LMB press (globe_mouse_lmb) do work; every other
// slot is fn_0d917_noop.
pub(crate) static GLOBE_MOUSE_HANDLERS: MouseHandlers = MouseHandlers {
    idle: GameState::globe_mouse_idle,
    lmb: GameState::globe_mouse_lmb,
    rmb: GameState::globe_mouse_noop,
    release: GameState::globe_mouse_noop,
    rmb_release: GameState::globe_mouse_noop,
    drag: GameState::globe_mouse_drag_noop,
    rmb_drag: GameState::globe_mouse_drag_noop,
};

impl GameState {
    // = seg000:b8c6 callback_main_ui_element_00 — the left-frieze planet
    // ornament on the map view: open the GLOBE screen.
    pub(crate) fn callback_main_ui_element_00(&mut self) {
        // = seg000:b8c6 call dismiss_stacked_overlays.
        self.dismiss_stacked_menus();
        // = seg000:b8c9 call reset_room_scene_state — tear the map-view state
        // down (data_046eb = 0, troop icons/popups cleared, icon task off).
        self.reset_room_scene_state();
        // = seg000:b8cc call setup_globe_draw — GLOBDATA/TABLAT into the
        // renderer, orientation from the zoomed-globe centre, FRESK palette.
        self.setup_globe_draw();
        // = seg000:b8cf inc globe_screen_active — the globe screen owns the display
        // (also the music-situation classifier's globe input).
        self.globe_screen_active = self.globe_screen_active.wrapping_add(1);
        // = seg000:b8d3..b8db al = 0; dx = 0ffffh; bp = draw_globe_and_ui_to_
        // front_buffer; call transition — the vertical curtain, negative dl:
        // the map view slides up off the screen revealing the globe beneath.
        self.transition(0, -1, |s| s.draw_globe_and_ui_to_front_buffer());
        // = seg000:b8de call service_midi_music.
        self.service_midi_music();
        // = seg000:b8e1..b8e4 ax = data_02562; call set_active_mouse_handlers.
        self.active_mouse_handlers = &GLOBE_MOUSE_HANDLERS;
        // = seg000:b8e7 call ui_hud_head_animate_up.
        self.ui_hud_head_animate_up();
        // = seg000:b8ea fall through into add_globe_rotation_frame_task.
        self.add_globe_rotation_frame_task();
    }

    // = seg000:b827 draw_globe_and_ui_to_front_buffer — compose the whole
    // globe screen into fb1 (run inside the transition, front buffer
    // redirected).
    pub(crate) fn draw_globe_and_ui_to_front_buffer(&mut self) {
        // = seg000:b827 globe_draw_skips_pixel_stores = 0 — the full redraw
        // patch (the SEE RESULTS mode sets it back nonzero).
        self.globe_draw_skips_pixel_stores = 0;
        // = seg000:b82c call globe_fill_blue_background_to_front_buffer.
        self.globe_fill_blue_background_to_front_buffer();
        // = seg000:b82f _word_2D1BF_globe_decoration_offset = 0.
        self.globe_decoration_offset = 0;
        // = seg000:b835 call draw_globe_side_decorations.
        self.draw_globe_side_decorations();
        // = seg000:b838 call ui_hud_head_draw.
        self.ui_hud_head_draw();
        // = seg000:b83b call globe_menu_push — push the globe verb menu.
        self.globe_menu_push();
        // = seg000:b83e call ui_set_and_draw_frieze_sides_globe.
        self.ui_set_and_draw_frieze_sides_globe();
        // = seg000:b841..b844 si = ui_globe_rotation_controls; call loc_0d72b
        // — install and draw the six rotation controls over the nav panel.
        self.ui_install_nav_panel(&NAV_PANEL_GLOBE);
        // = seg000:b847 jmp loc_0ad5e — re-pick the music for the globe mode.
        self.update_room_music();
    }

    // = seg000:b84a globe_fill_blue_background_to_front_buffer — fill the
    // game area of fb1 with the FRESK blue (0xf0) and fall through into
    // draw_globe_with_atmosphere.
    pub(crate) fn globe_fill_blue_background_to_front_buffer(&mut self) {
        // = seg000:b84a call set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = seg000:b851..b856 vga_fill_rect(_stru_20920_game_area_rect, 0f0h).
        let dest = self.active_fb();
        crate::gfx::vga_fill_rect(self, dest, 0, 0, 320, 152, 0xf0);
        // = seg000:b856 falls through into draw_globe_with_atmosphere.
        self.draw_globe_with_atmosphere();
    }

    // = seg000:b87e draw_globe_side_decorations — the two FRESK side panels
    // framing the globe, clipped to the game area; the decoration offset
    // slides them apart for the SEE RESULTS reveal (0 = closed).
    pub(crate) fn draw_globe_side_decorations(&mut self) {
        // = seg000:b87e call copy_game_area_rect_to_clip_rect.
        let yoff = self.y_offset as i16;
        let clip = Rect {
            x0: 0,
            y0: yoff,
            x1: 320,
            y1: 152 + yoff,
        };
        // = seg000:b881..b884 ax = 1; open_resource_by_index — FRESK.HSQ.
        self.open_sprite_bank(sprite_bank::FRESK);
        let offset = self.globe_decoration_offset;
        self.with_active_bank_sheet(|s, sheet| {
            // = seg000:b887..b88f sprite 0 (the left panel) at
            // (decoration offset, 0).
            s.draw_sprite_from_sheet_clipped(sheet, 0, offset, yoff, clip);
            // = seg000:b892..b89b sprite 1 (the right panel) at
            // (0xd6 - offset, 0).
            s.draw_sprite_from_sheet_clipped(sheet, 1, 0xd6 - offset, yoff, clip);
        });
        // = seg000:b89e..b8a4 sprite clip rect = _word_218F0_rect. The port
        // passes clip rects per draw call, so nothing is stored here.
    }

    // = seg000:b941 globe_menu_push — push the globe verb menu. DOS patches record
    // 1 in place: text 0xb1 SEE RESULTS + menu_callback_choice_globe_see_
    // results while globe_draw_skips_pixel_stores is clear, text 0xb2
    // STANDARD VISION + the see_standard_vision callback while it is set.
    // The port keeps the flag at 0 (the results mode is not ported), so the
    // static MENU_GLOBE row is already the SEE RESULTS variant.
    pub(crate) fn globe_menu_push(&mut self) {
        // = seg000:b95b..b95e bx = fn_0d917_noop; jmp screen_element_stack_
        // push.
        self.menu_stack_push(MenuRef::MenuGlobe, None);
    }

    // = seg000:ba15 globe_increment_tilt_by_ax — step the tilt and clamp its
    // magnitude to 98 rows. (The DOS fall-through rebuild of the tilt window
    // table happens inside the next GlobeRenderer draw.)
    pub(crate) fn globe_increment_tilt(&mut self, ax: i16) {
        self.globe_tilt = (self.globe_tilt + ax).clamp(-0x62, 0x62);
    }

    // = seg000:b9b9 callback_ui_element_0b9b9 — the tilt-up arrow (sprite 49,
    // held-repeat): tilt += 8 while the LMB is down.
    pub(crate) fn callback_globe_tilt_up(&mut self) {
        // = seg000:b9b9 shr ax,1; jnb ret — only while the LMB is down; the
        // release edge's final armed-element fire is a no-op.
        if self.prev_mouse_buttons & 1 == 0 {
            return;
        }
        self.globe_increment_tilt(8);
        self.globe_redraw_and_present();
    }

    // = seg000:b9c0 callback_ui_element_0b9c0 — the tilt-down arrow (sprite
    // 51): tilt -= 8.
    pub(crate) fn callback_globe_tilt_down(&mut self) {
        if self.prev_mouse_buttons & 1 == 0 {
            return;
        }
        self.globe_increment_tilt(-8);
        self.globe_redraw_and_present();
    }

    // = seg000:b9cc callback_ui_element_0b9cc — the rotate arrow (sprite 50):
    // rotation phase -= 0x20.
    pub(crate) fn callback_globe_rotate_east(&mut self) {
        if self.prev_mouse_buttons & 1 == 0 {
            return;
        }
        self.globe_rotation_increment(-0x20);
        self.globe_redraw_and_present();
    }

    // = seg000:b9d3 callback_ui_element_0b9d3 — the rotate arrow (sprite 52):
    // rotation phase += 0x20.
    pub(crate) fn callback_globe_rotate_west(&mut self) {
        if self.prev_mouse_buttons & 1 == 0 {
            return;
        }
        self.globe_rotation_increment(0x20);
        self.globe_redraw_and_present();
    }

    // = seg000:ba9e callback_ui_element_0ba9e — the centre button (sprite
    // 53): step the view toward the player's map position, at most 0x20
    // phase / 0x18 tilt rows per redraw, repeating until no step was clamped
    // (the seg000:baef `loop` back to the function start).
    pub(crate) fn callback_globe_center_on_player(&mut self) {
        loop {
            // = seg000:ba9e call get_map_position; baa1..baa4 the target
            // phase = hi word of 398 * longitude.
            let (lon, lat) = self.get_map_position();
            let target = ((398 * lon as u32) >> 16) as i16;
            // = seg000:baa6..baaa bp = 2 * TABLAT[0].len — one revolution in
            // phase units (398).
            let modulus = 2 * self
                .globe_renderer
                .as_ref()
                .map_or(199, |g| g.equator_row_len()) as i16;
            // = seg000:baac..bab0 the shortest signed phase delta
            // (loc_0b683 picks the smaller-magnitude wrap).
            let mut d = target - self.globe_rotation as i16;
            let (a, b) = if d < 0 {
                (d + modulus, d)
            } else {
                (d, d - modulus)
            };
            d = if (-b) as u16 >= a as u16 { a } else { b };
            // = seg000:bab3..bac8 clamp the phase step to ±0x20, counting
            // each clamp into cx.
            let mut steps = 1;
            if d >= 0x20 {
                d = 0x20;
                steps += 1;
            }
            if d <= -0x20 {
                d = -0x20;
                steps += 1;
            }
            // = seg000:bac9..badf the tilt step toward the player latitude,
            // clamped to ±0x18.
            let mut db = lat - self.globe_tilt;
            if db >= 0x18 {
                db = 0x18;
                steps += 1;
            }
            if db <= -0x18 {
                db = -0x18;
                steps += 1;
            }
            // = seg000:bae0..baeb apply and redraw.
            let step_start = self.game_ticks();
            self.globe_rotation_increment(d);
            self.globe_increment_tilt(db);
            self.globe_redraw_and_present();
            // = seg000:baef loop callback_ui_element_0ba9e — repeat while a
            // step was clamped.
            if steps == 1 {
                break;
            }
            // DOS paces the roll implicitly: every pass is a full globe
            // redraw straight to VGA, tens of milliseconds on period
            // hardware. The port's redraw is near-instant, so hold each
            // presented step a few PIT ticks to keep the roll visible.
            self.sleep_ticks(step_start, 24);
        }
    }

    // = seg000:b98b globe_redraw_and_present — redraw the globe pixels and present.
    pub(crate) fn globe_redraw_and_present(&mut self) {
        // = seg000:b98b call map_func_gfx.
        self.map_func_gfx();
        self.globe_present_and_advance();
    }

    // = seg000:b98e globe_present_and_advance — the shared present tail: project the player
    // pin, present the globe clip rect from fb1, stamp the pin onto the
    // screen, and advance the rotation phase for the next pass.
    pub(crate) fn globe_present_and_advance(&mut self) {
        // = seg000:b98e call globe_player_screen_pos — the player globe-pin position
        // (dx = 0 while _byte_227D_suppress_sky_240_255 is set, so no pin in
        // the intro's globe scene).
        let pin = self.globe_player_screen_pos();
        // = seg000:b993..b999 restore_mouse_if_rect_intersects(sprite_clip_
        // rect) + update_screen_at_sprite_rect_updating_head; the
        // seg000:b9a5 draw_mouse_cursor_if_needed rearm closes the bracket.
        // The port's present handles the cursor through its own protocol.
        self.present_screen_rect(GLOBE_CLIP_RECT);
        // = seg000:b99e..b9a2 or dx,dx; jz; call draw_globe_cursor_at_dx_bx.
        if let Some((x, y)) = pin {
            self.draw_globe_cursor_at(x, y);
        }
        // = seg000:b9a8 mov ax,1; jmp globe_rotation_increment_ax.
        self.globe_rotation_increment(1);
    }

    // = seg000:baf2 globe_player_screen_pos — the GameState half of the player pin
    // projection: the suppress gate and the map position feed the renderer's
    // geometry (GlobeRenderer::player_screen_pos).
    pub(crate) fn globe_player_screen_pos(&mut self) -> Option<(i16, i16)> {
        // = seg000:baf4 cmp _byte_227D_suppress_sky_240_255,0; jnz — no pin
        // outside the in-game screens.
        if self.data_0227d != 0 {
            return None;
        }
        // = seg000:bafb call get_map_position.
        let (lon, lat) = self.get_map_position();
        let phase = self.globe_rotation;
        let tilt = self.globe_tilt;
        self.globe_renderer
            .as_mut()?
            .player_screen_pos(phase, tilt, lon, lat)
    }

    // = seg000:bc0c draw_globe_cursor_at_dx_bx — stamp the player pin
    // (ICONES sprite 0x36) onto the visible screen, bottom-anchored at
    // (x, y) (bl -= the sprite height from its header).
    pub(crate) fn draw_globe_cursor_at(&mut self, x: i16, y: i16) {
        // = seg000:bc0c call set_screen_as_active_framebuffer.
        self.set_screen_as_active_framebuffer();
        // = seg000:bc0f call load_icones_sprites.
        self.open_icones_spritesheet();
        let yoff = self.y_offset as i16;
        self.with_active_bank_sheet(|s, sheet| {
            // = seg000:bc12..bc18 the subresource header's height byte.
            let Some(sprite) = sheet.get_sprite(0x36) else {
                return;
            };
            let h = sprite.height() as i16;
            // = seg000:bc1c jmp draw_sprite_clobbering_bx_dx.
            let fb = s.active_fb_mut();
            let _ = crate::sprite_blitter::draw_sprite_from_sheet(sheet, 0x36, x, y - h + yoff, fb);
        });
        // DOS wrote straight to VGA memory; push the stamped frame out.
        self.send_frame_to_display();
    }

    // = seg000:bc4e globe_disc_hit_test — the globe-disc hit test: carry set (Some)
    // when (x, y) falls inside (96,25)-(224,134), returning the disc-relative
    // coordinates the pick uses (DOS leaves them in dx/bx).
    fn globe_disc_hit(&self) -> Option<(i16, i16)> {
        let x = self.mouse_pos_x as i16 - 0x60;
        let y = self.mouse_pos_y as i16 - 0x19;
        ((0..0x80).contains(&x) && (0..0x6d).contains(&y)).then_some((x, y))
    }

    // = seg000:bc1f globe_mouse_idle — the globe idle handler: swap the verb strip
    // between the globe menu and the single SEE MAP OF THIS AREA row as the
    // cursor moves on and off the disc.
    pub(crate) fn globe_mouse_idle(&mut self) {
        // = seg000:bc1f..bc2c only while one of the two globe menus is the
        // active screen element.
        let top = self.get_active_menu_ref();
        if top != MenuRef::MenuGlobe && top != MenuRef::MenuGlobeDefaultClickOnGlobe {
            return;
        }
        if self.globe_disc_hit().is_some() {
            // = seg000:bc3c..bc4a over the disc: insert the SEE MAP OF THIS
            // AREA element (equal priority replaces the top in place).
            if top != MenuRef::MenuGlobeDefaultClickOnGlobe {
                self.menu_stack_push(MenuRef::MenuGlobeDefaultClickOnGlobe, None);
            }
        } else if top != MenuRef::MenuGlobe {
            // = seg000:bc33..bc39 off the disc: restore the globe menu
            // (jmp globe_menu_push).
            self.globe_menu_push();
        }
    }

    // = seg000:bc64 globe_mouse_lmb — the globe LMB handler: a click on the disc
    // picks the map cell under the cursor, recentres the map there, and
    // zooms out to the map interface.
    pub(crate) fn globe_mouse_lmb(&mut self) {
        // = seg000:bc66 call globe_disc_hit_test; jnb — only on the disc.
        let Some((rx, ry)) = self.globe_disc_hit() else {
            return;
        };
        // = seg000:bc6b call globe_pick_map_position; jnb — only on the globe outline.
        let phase = self.globe_rotation;
        let tilt = self.globe_tilt;
        let Some((lon, lat)) = self
            .globe_renderer
            .as_mut()
            .and_then(|g| g.pick_map_position(phase, tilt, rx, ry))
        else {
            return;
        };
        // = seg000:bc70..bc73 the picked cell becomes the map centre.
        self.zoomed_globe_longitude = lon;
        self.zoomed_globe_latitude = lat;
        // = seg000:bc77 call globe_redraw_and_present — one redraw at the picked spot.
        self.globe_redraw_and_present();
        // = seg000:bc7a..bc7c pop the click position; jmp globe_zoom_out_to_map.
        let (mx, my) = (self.mouse_pos_x as i16, self.mouse_pos_y as i16);
        self.globe_zoom_out_to_map(mx, my);
    }

    // = seg000:0f66 nullsub — the unused globe mouse slots.
    pub(crate) fn globe_mouse_noop(&mut self) {}
    pub(crate) fn globe_mouse_drag_noop(&mut self, _dx: i16, _dy: i16) {}

    // = seg000:bc81 callback_ui_element_0bc81 — the EXIT GLOBE compass
    // button and verb: recentre on the player, then zoom out to the map
    // interface from the disc centre.
    pub(crate) fn callback_ui_element_globe_exit(&mut self) {
        // = seg000:bc81 call callback_ui_element_0ba9e.
        self.callback_globe_center_on_player();
        // = seg000:bc84 call set_zoomed_globe_pos_from_map_position.
        self.set_zoomed_globe_pos_from_map_position();
        // = seg000:bc87..bc8a dx = 0a0h; bx = 4fh — the disc centre.
        self.globe_zoom_out_to_map(0xa0, 0x4f);
    }

    // = seg000:bc8d globe_zoom_out_to_map — the shared globe exit tail: the zoom-box
    // animation from (x, y), the music fade, and the transition into the
    // map interface.
    fn globe_zoom_out_to_map(&mut self, x: i16, y: i16) {
        // = seg000:bc8d call call_restore_cursor.
        self.call_restore_cursor();
        // = seg000:bc90 call globe_zoom_box_animation.
        self.globe_zoom_box_animation(x, y);
        // = seg000:bc93 call midi_begin_song_fade_out — a 300-tick fade to
        // silence unless a ramp is already running.
        if !self.midi.is_fading() {
            self.midi.set_ducking(0x12c, 0, 0);
        }
        // = seg000:bc96 jmp ui_transition_to_map_interface.
        self.ui_transition_to_map_interface();
    }

    // = seg000:bc99 globe_zoom_box_animation — the zoom-out reveal: an outline box grows
    // from the click point toward the globe clip rect in 8 steps of
    // (4, 2) pixels per side, the interior 2x-shimmer-zooming through
    // blit_fb1_to_screen_effect(0) between steps.
    fn globe_zoom_box_animation(&mut self, x: i16, y: i16) {
        // = seg000:bc99..bcaa the data_0dd06 rect seeded around (x, y),
        // colour 7.
        let mut r = Rect {
            x0: x,
            y0: y,
            x1: x + 2,
            y1: y + 1,
        };
        // = seg000:bcae call set_screen_as_active_framebuffer — the outline
        // draws straight to the visible screen.
        self.set_screen_as_active_framebuffer();
        // = seg000:bcb1 cx = 8 passes.
        for _ in 0..8 {
            // = seg000:bcbe..bcd5 grow the min corner by (4, 2), clamped to
            // the clip rect (dx = 0fffch, sar per field).
            r.x0 = (r.x0 - 4).max(GLOBE_CLIP_RECT.x0);
            r.y0 = (r.y0 - 2).max(GLOBE_CLIP_RECT.y0);
            // = seg000:bcda..bcf1 grow the max corner by (4, 2), clamped.
            r.x1 = (r.x1 + 4).min(GLOBE_CLIP_RECT.x1);
            r.y1 = (r.y1 + 2).min(GLOBE_CLIP_RECT.y1);
            // = seg000:bcf6 call loc_0c551 — the outline at {x0, y0, x1-1,
            // y1-1}, colour byte [si+8] = 7.
            self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, 7);
            self.send_frame_to_display();
            // = seg000:bcf9 call globe_zoom_box_shimmer_step — shrink the rect by one, run the
            // 2x zoom shimmer inside it for 10 ticks, restore the rect.
            let inner = Rect {
                x0: r.x0 + 1,
                y0: r.y0 + 1,
                x1: r.x1 - 1,
                y1: r.y1 - 1,
            };
            self.blit_fb1_to_screen_effect(0, inner);
        }
    }

    // = seg000:5a3d ui_transition_to_map_interface — leave the globe for the
    // SEE DUNE MAP view.
    pub(crate) fn ui_transition_to_map_interface(&mut self) {
        // = seg000:5a3d room_view_toggle = 0ffh — the map side of the room
        // toggle.
        self.room_view_toggle = 0xff;
        // = seg000:5a42 call open_onmap_resource; 5a45 vga_palette_flush.
        self.open_onmap_spritesheet();
        self.update_screen_palette();
        // = seg000:5a49..5a50 bp = callback_transition_05a56; al = 2; dx = 0;
        // call transition — the expanding-box reveal spiralling out from the
        // screen centre.
        self.transition(2, 0, |s| s.callback_transition_dune_map_view());
        // = seg000:5a53 jmp service_midi_music.
        self.service_midi_music();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{GameState, dat_file::DatFile, menu_defs::MenuRef};

    // The GLOBE view from the SEE DUNE MAP screen: the planet ornament opens
    // the globe screen, the rotation controls spin it, the player pin
    // round-trips through the inverse pick, and a disc click zooms back out
    // to the map interface. Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored globe_screen
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn globe_screen_opens_rotates_and_exits() {
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

        // SEE DUNE MAP (the room verb) opens the map view; its frieze
        // template makes the planet ornament (element 0) clickable (0xc0).
        game.ui_toggle_room_view();
        while rx.try_recv().is_ok() {}
        assert_eq!(game.ui_elements[0].flags, 0xc0, "planet ornament clickable");

        // The ornament click (= seg000:b8c6).
        game.callback_main_ui_element_00();
        while rx.try_recv().is_ok() {}
        assert_eq!(game.globe_screen_active, 1, "globe screen owns the display");
        assert_eq!(game.get_active_menu_ref(), MenuRef::MenuGlobe);
        assert_eq!(
            game.ui_elements[12].func_ptr, 0xbc81,
            "the EXIT GLOBE compass button is installed"
        );
        // The globe disc renders in the 0x10..0x1f map palette block around
        // the disc centre (160, 79).
        let globe_pixels = (39..119)
            .map(|y| {
                (120..200)
                    .filter(|&x| (0x10..0x20).contains(&game.framebuffer.pixels()[y * 320 + x]))
                    .count()
            })
            .sum::<usize>();
        assert!(
            globe_pixels > 4000,
            "globe disc missing from fb1 ({globe_pixels} map-colour pixels)"
        );
        game.framebuffer
            .write_png(&game.palette, "globe_screen.png")
            .unwrap();

        // The idle hover swaps the verb strip on and off the disc.
        game.mouse_pos_x = 160;
        game.mouse_pos_y = 79;
        game.globe_mouse_idle();
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuGlobeDefaultClickOnGlobe,
            "SEE MAP OF THIS AREA over the disc"
        );
        game.mouse_pos_x = 30;
        game.globe_mouse_idle();
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuGlobe,
            "the globe menu off the disc"
        );

        // A held rotate arrow steps the phase by 0x20 (+1 for the present's
        // spin advance), wrapping within 0..398.
        let before = game.globe_rotation as i32;
        game.prev_mouse_buttons = 1;
        game.callback_globe_rotate_west();
        assert_eq!(
            game.globe_rotation as i32,
            (before + 0x20 + 1).rem_euclid(398),
            "rotate west: phase += 0x20 (+1 spin advance)"
        );
        game.prev_mouse_buttons = 0;
        game.callback_globe_rotate_east();
        assert_eq!(
            game.globe_rotation as i32,
            (before + 0x20 + 1).rem_euclid(398),
            "no step while the button is up"
        );

        // Recentre on the player, then the pin projection and the click pick
        // must roundtrip: picking at the pin's screen position returns the
        // player's map cell (within the cell granularity).
        game.prev_mouse_buttons = 1;
        game.callback_globe_center_on_player();
        let (plon, plat) = game.get_map_position();
        let (px, py) = game
            .globe_player_screen_pos()
            .expect("player pin visible after centring");
        assert!(
            (96..224).contains(&px) && (25..134).contains(&py),
            "pin ({px},{py}) inside the nav rect"
        );
        let phase = game.globe_rotation;
        let tilt = game.globe_tilt;
        let (klon, klat) = game
            .globe_renderer
            .as_mut()
            .unwrap()
            .pick_map_position(phase, tilt, px - 96, py - 25)
            .expect("pick hits the outline at the pin");
        let dlon = (klon as i32 - plon as i32).rem_euclid(65536);
        let dlon = dlon.min(65536 - dlon);
        assert!(
            dlon < 1200,
            "picked longitude {klon} vs player {plon} (delta {dlon})"
        );
        assert!(
            (klat - plat).abs() <= 3,
            "picked latitude {klat} vs player {plat}"
        );

        // The pin must drift against the tilt (= the seg000:bbf5 sign):
        // tilting the view centre south of the player leaves the pin north
        // of the disc centre (up), and vice versa.
        game.callback_globe_tilt_up(); // tilt += 8 (positive = south)
        let (_, py_south) = game.globe_player_screen_pos().expect("pin visible");
        assert!(
            py_south < py,
            "view south of the player: pin above centre ({py_south} vs {py})"
        );
        game.callback_globe_tilt_down();
        game.callback_globe_tilt_down(); // net tilt -8 (north)
        let (_, py_north) = game.globe_player_screen_pos().expect("pin visible");
        assert!(
            py_north > py,
            "view north of the player: pin below centre ({py_north} vs {py})"
        );
        game.callback_globe_tilt_up(); // back to the centred tilt

        // A click on the pin zooms out into the SEE DUNE MAP view centred
        // near the player.
        let (px, py) = game.globe_player_screen_pos().expect("pin visible");
        game.mouse_pos_x = px as u16;
        game.mouse_pos_y = py as u16;
        game.globe_mouse_lmb();
        while rx.try_recv().is_ok() {}
        assert_eq!(game.data_046eb, 0x80, "back in the full-map view");
        assert_eq!(game.room_view_toggle, 0xff);
        assert_eq!(
            game.globe_screen_active, 0,
            "globe screen released the display"
        );
        assert!(
            (game.zoomed_globe_latitude - plat).abs() <= 3,
            "map recentred near the player latitude"
        );
        game.framebuffer
            .write_png(&game.palette, "globe_screen_exit_map.png")
            .unwrap();
    }
}
