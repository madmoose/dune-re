//! The ending: the cutscene actions of the game-won script (phase 0xc8) —
//! the final room (action 09), the FINAL.HSQ scene with its palette fades
//! (action 0a) and the end-credits slideshow that exits to DOS (action 0b) —
//! with the palette-range fade helpers and the mirror dual-head setup they
//! share with LOOK AT MIRROR.
//!
//! Ported from seg000:0f13 (the dual-head setup), 148d/1498, 14ac, 14c9,
//! 1556/1566/157e (the fades), 15a8 (the caption), 15b7..164c (the
//! slideshow stills), 165a (their table), 167c and 16fc. Routines are in DOS
//! address order.

use crate::{GameState, Rect, TaskId, gfx, rect::rect, sprite_bank};

// = seg000:165a array_callbacks_intro_0165a — the end-credits slideshow
// scenes, each rendered offscreen by callback_action_in_continue_sequence_0b
// and revealed with transition 0x3a.
const ENDING_SCENES: [fn(&mut GameState); 16] = [
    GameState::ending_scene_00_final_sprite_5,
    GameState::ending_scene_01_sky_and_paul,
    GameState::ending_scene_02_leto,
    GameState::ending_scene_03_jessica,
    GameState::ending_scene_04_gurney,
    GameState::ending_scene_05_thufir,
    GameState::ending_scene_06_baron_fortress,
    GameState::ending_scene_07_harah,
    GameState::ending_scene_08_duncan,
    GameState::ending_scene_09_stilgar,
    GameState::ending_scene_0a_baron,
    GameState::ending_scene_0b_feyd_fortress,
    GameState::ending_scene_0c_chani,
    GameState::ending_scene_0d_liet,
    GameState::ending_scene_0e_credits,
    GameState::ending_scene_0f_final_room,
];

impl GameState {
    // = seg000:0f08 mirror_head_setup — LOOK AT MIRROR's head overlay: with
    // Chani travelling along (persons_travelling_with bit 7) the two-head
    // still, else Paul alone over the backdrop.
    pub(crate) fn mirror_head_setup(&mut self) {
        // = seg000:0f08/0f0e test [persons_travelling_with],80h; jnz
        //   mirror_dual_head_setup.
        if self.persons_travelling_with & 0x80 != 0 {
            self.mirror_dual_head_setup();
            return;
        }
        // = seg000:0f10 jmp loc_00960: the backdrop into fb2; al = 2dh; dx =
        //   0; loc_009c7; data_0478c = 1 (the mirror Paul plays 4 lively
        //   frames, 32..35, before settling); start_room_lip_sync.
        self.copy_active_framebuffer_to_framebuffer_2();
        self.setup_talking_head(0x2d, 0);
        self.subtitle_word_count = 1;
        self.start_room_lip_sync();
    }

    // = seg000:0f13 mirror_dual_head_setup — the two-head still shared by
    // LOOK AT MIRROR and the ending's final room: Chani (lip-sync resource 7)
    // set up first with her box lowered 15 px and swapped into the shadow
    // state, mirror mode raised, then Paul beside her, shifted right by 0x2d.
    // frame_task_callback_099be then ticks both heads.
    pub(crate) fn mirror_dual_head_setup(&mut self) {
        // = seg000:0f13 call copy_active_framebuffer_to_framebuffer_2.
        self.copy_active_framebuffer_to_framebuffer_2();
        // = seg000:0f16..0f1f current_lip_sync_resource_id = 7; setup_lip_
        //   sync_data_from_current; data_0478c = 0.
        self.setup_talking_head(7, 0);
        self.subtitle_word_count = 0;
        // = seg000:0f24 ui_hud_elements[19].y0 += 0fh — the head rect.
        if let Some(head) = self.talking_head.as_mut() {
            head.rect.1 = head.rect.1.wrapping_add(0x0f);
        }
        // = seg000:0f29 call start_room_lip_sync.
        self.start_room_lip_sync();
        // = seg000:0f2c call swap_talking_head_state — Chani into the shadow.
        self.swap_talking_head_state();
        // = seg000:0f2f talking_head_id = 0ffffh — no head in the live slot
        //   (the swapped-in shadow was empty), so Paul's open builds afresh.
        self.talking_head = None;
        // = seg000:0f35 inc [mirror_dual_head].
        self.mirror_dual_head = self.mirror_dual_head.wrapping_add(1);
        // = seg000:0f39 data_047c6 = 1 — the idle task counts as installed;
        //   the port's add_frame_task is idempotent.
        // = seg000:0f3f call copy_active_framebuffer_to_framebuffer_2.
        self.copy_active_framebuffer_to_framebuffer_2();
        // = seg000:0f42/0f45 dx = 2dh; jmp loc_00965: al = 2dh; loc_009c7;
        //   data_0478c = 1; start_room_lip_sync.
        self.setup_talking_head(0x2d, 0x2d);
        self.subtitle_word_count = 1;
        self.start_room_lip_sync();
    }

    // = seg000:148d callback_action_in_continue_sequence_09 — the final
    // room: the Continue menu, then transition 0x10 onto
    // callback_transition_ending_room.
    pub(crate) fn sequence_action_09_ending_room(&mut self) {
        self.change_menu_to_continue_menu();
        self.transition(0x10, 0, |s| s.callback_transition_ending_room());
    }

    // = seg000:1498 callback_transition_ending_room — the room from the
    // script's action-00 bytes, the HUD head reset and the first companion
    // portrait cleared, then the two heads.
    fn callback_transition_ending_room(&mut self) {
        // = seg000:1498 call callback_action_in_continue_sequence_00.
        self.sequence_action_00_set_room();
        // = seg000:149b..14a6 ui_hud_head_index = 0; ui_hud_companion_1 =
        //   0ffffh; call ui_hud_draw_companions.
        self.ui_hud_head_index = 0;
        self.companions[0] = -1;
        self.ui_hud_draw_companions();
        // = seg000:14a9 jmp mirror_dual_head_setup.
        self.mirror_dual_head_setup();
    }

    // = seg000:14ac callback_intro_0f_014ac — the final room still: FINAL.HSQ
    // sprites 0..2 at the origin, the lip-sync stopped, then the two heads.
    fn ending_scene_0f_final_room(&mut self) {
        // = seg000:14ac..14c1 ax = 1eh (FINAL); the three sprites at (0, 0).
        self.open_sprite_bank(sprite_bank::FINAL);
        for id in 0..3 {
            self.draw_sprite_at(id, 0, 0);
        }
        // = seg000:14c3 call stop_lip_sync_and_remove_idle_head_task.
        self.stop_lip_sync_and_remove_idle_head_task();
        // = seg000:14c6 jmp mirror_dual_head_setup.
        self.mirror_dual_head_setup();
    }

    // = seg000:14c9 callback_action_in_continue_sequence_0a — the FINAL.HSQ
    // scene: the sky span fades to black, the final room still comes in as
    // its colours fade up from black, then the black game area with
    // FINAL.HSQ sprite 3, the 0x22 transition, a pause, and sprite 4 fading
    // up; the blink task is put back at the end.
    pub(crate) fn sequence_action_0a_final_scene(&mut self) {
        // = seg000:14c9/14cc remove_frame_task(frame_task_callback_blink).
        self.remove_frame_task(TaskId::SequenceBlink);
        // = seg000:14cf..14dd the fade target's entries 128..239 go black
        //   (cx = 150h bytes at bx = 180h), 40 steps, then the fade wait.
        self.palette_range_set_black(0x28, 0x150, 0x180, true);
        self.palette_range_fade_wait();
        // = seg000:14e0/14e3 the final room still, composed offscreen.
        self.gfx_call_bp_with_front_buffer_as_screen(|s| s.ending_scene_0f_final_room());
        // = seg000:14e6 vga_save_palette_to_fade_target.
        gfx::vga_save_palette_to_fade_target(self);
        // = seg000:14ea..14f4 the live entries 99..174 go black (0e4h bytes
        //   at 129h), 48 steps; flush, present, and fade them back up.
        self.palette_range_set_black(0x30, 0xe4, 0x129, false);
        gfx::palette_flush(self);
        self.present_game_area();
        self.palette_range_fade_wait();
        // = seg000:1501 call stop_lip_sync_and_remove_idle_head_task.
        self.stop_lip_sync_and_remove_idle_head_task();
        // = seg000:1504..150d fill the game-area rect with 8fh in the active
        //   framebuffer.
        let yoff = self.y_offset as i16;
        let dest = self.active_fb();
        gfx::vga_fill_rect(self, dest, 0, yoff as u16, 320, (152 + yoff) as u16, 0x8f);
        // = seg000:1511..151f FINAL.HSQ sprite 3 at (34h, 0).
        self.open_sprite_bank(sprite_bank::FINAL);
        self.draw_sprite_at(3, 0x34, 0);
        // = seg000:1522..1527 transition(al = 22h, bp = nullsub).
        self.transition(0x22, 0, |_| {});
        // = seg000:152a/152d wait_a_bit(12ch).
        self.wait_a_bit(0x12c);
        // = seg000:1530..153a the live entries 160..174 go black (2dh bytes
        //   at 1e0h), 32 steps.
        self.palette_range_set_black(0x20, 0x2d, 0x1e0, false);
        // = seg000:153d..1546 FINAL.HSQ sprite 4 at (5ah, 40h).
        self.draw_sprite_at(4, 0x5a, 0x40);
        // = seg000:1549..1550 flush, present, fade up.
        gfx::palette_flush(self);
        self.present_game_area();
        self.palette_range_fade_wait();
        // = seg000:1553 jmp loc_0178e — add_frame_task(blink, 64h).
        self.add_frame_task(0x64, TaskId::SequenceBlink);
    }

    // = seg000:1556 palette_range_fade_step — vga_fade_step(al =
    // sky_fade_countdown, bx, cx) over the range palette_range_set_black
    // recorded: each entry moves 1/countdown of the way to the fade target.
    fn palette_range_fade_step(&mut self) {
        let countdown = self.sky_fade_countdown;
        let steps = if countdown == 0 { 1 } else { countdown as i16 };
        let (offset, count) = self.ending_fade_range;
        let start = offset as usize / 3;
        let end = (start + count as usize / 3).min(256);
        for i in start..end {
            let current = self.palette.get(i);
            let target = self.palette_fade_target.get(i);
            self.palette.set(i, current.lerp(target, steps));
            self.screen_pal.set(i, current.lerp(target, steps));
        }
    }

    // = seg000:1566 palette_range_fade_wait — with the sky span held
    // (suppress_sky_240_255), step the range fade every 16 ticks until
    // sky_fade_countdown runs out.
    fn palette_range_fade_wait(&mut self) {
        // = seg000:1566 inc [suppress_sky_240_255].
        self.data_0227d = self.data_0227d.wrapping_add(1);
        loop {
            // = seg000:156a..1570 bp = palette_range_fade_step; ax = 10h;
            //   call wait_processing_frame_tasks_interruptable.
            self.wait_processing_frame_tasks_interruptable(0x10, |s| s.palette_range_fade_step());
            // = seg000:1573/1577 dec [sky_fade_countdown]; jnz.
            self.sky_fade_countdown = self.sky_fade_countdown.wrapping_sub(1);
            if self.sky_fade_countdown == 0 {
                break;
            }
        }
        // = seg000:1579 dec [suppress_sky_240_255].
        self.data_0227d = self.data_0227d.wrapping_sub(1);
    }

    // = seg000:157e palette_range_set_black — record the fade (countdown
    // al, the byte range bx..bx+cx) and write black over that range of the
    // live palette (bp = 0, vga_set_palette) or of the fade target
    // (vga_set_fade_target_data).
    fn palette_range_set_black(
        &mut self,
        countdown: u8,
        count: u16,
        offset: u16,
        to_fade_target: bool,
    ) {
        // = seg000:157e..1585 sky_fade_countdown = al; data_0d81a = cx;
        //   map_disc_centre_y = bx (the fade step's scratch).
        self.sky_fade_countdown = countdown;
        self.ending_fade_range = (offset, count);
        // = seg000:1589..15a1 a zeroed stack buffer of cx bytes into the
        //   palette range.
        let start = offset as usize / 3;
        let end = (start + count as usize / 3).min(256);
        let black = crate::Color(0, 0, 0);
        for i in start..end {
            if to_fade_target {
                self.palette_fade_target.set(i, black);
            } else {
                self.palette.set(i, black);
                self.screen_pal.set(i, black);
            }
        }
    }

    // = seg000:15a8 ending_draw_caption — the plain glyph drawer, then the
    // current caption string (string_subst_id_table[0]) at (10, 157).
    fn ending_draw_caption(&mut self) {
        // = seg000:15a8 call font_select_plain_glyph_func.
        self.font_select_plain_glyph_func();
        // = seg000:15ab..15b4 ax = [string_subst_id_table]; dx = 0ah; bx =
        //   9dh; font_draw_phrase_or_command_string_with_color_at_pos (cx as
        //   the caller left it: the current colour word).
        let color = self.font_state.color;
        let id = self.string_subst_id_table[0];
        self.font_draw_phrase_or_command_string_with_color_at_pos(id, color, 0x0a, 0x9d);
    }

    // = seg000:15b7 callback_intro_00_015b7 — FINAL.HSQ sprite 5 at (64, 52).
    fn ending_scene_00_final_sprite_5(&mut self) {
        self.open_sprite_bank(sprite_bank::FINAL);
        self.draw_sprite_at(5, 0x40, 0x34);
    }

    // = seg000:095d callback_intro_01_0095d — the desert sky scene, then
    // loc_00960: the backdrop into fb2 and Paul's head (2dh) with a 1-frame
    // lively budget.
    fn ending_scene_01_sky_and_paul(&mut self) {
        self.intro_floppy_scene_sky();
        self.copy_active_framebuffer_to_framebuffer_2();
        self.setup_talking_head(0x2d, 0);
        self.subtitle_word_count = 1;
        self.start_room_lip_sync();
    }

    // = seg000:097e loc_0097e + 0099d loc_0099d — the still shared by the
    // portrait scenes: draw_room_for_scene(dx, bx), then the head `head`
    // with the intro's 0x1e lively budget (seg000:09d0).
    fn ending_room_and_head(&mut self, location_and_room: u16, location_appearance: u16, head: u8) {
        self.draw_room_for_scene(location_and_room, location_appearance);
        self.subtitle_word_count = 0x1e;
        self.setup_talking_head(head, 0);
    }

    // = seg000:15c9 callback_intro_02_015c9 — palace room 0x200a, Leto (0).
    fn ending_scene_02_leto(&mut self) {
        self.ending_room_and_head(0x200a, 0x180, 0);
    }

    // = seg000:15d4 callback_intro_03_015d4 — palace room 0x2003, Jessica (1).
    fn ending_scene_03_jessica(&mut self) {
        self.ending_room_and_head(0x2003, 0x180, 1);
    }

    // = seg000:15df callback_intro_04_015df — palace room 0x2006, Gurney (4).
    fn ending_scene_04_gurney(&mut self) {
        self.ending_room_and_head(0x2006, 0x180, 4);
    }

    // = seg000:15ea callback_intro_05_015ea — palace room 0x2004, Thufir (3).
    fn ending_scene_05_thufir(&mut self) {
        self.ending_room_and_head(0x2004, 0x180, 3);
    }

    // = seg000:15f5 callback_intro_06_015f5 — fortress room 0x3002 (slot 2),
    // the Baron (0bh).
    fn ending_scene_06_baron_fortress(&mut self) {
        self.ending_room_and_head(0x3002, 0x280, 0x0b);
    }

    // = seg000:1617 callback_intro_07_01617 — sietch room 0x703 (slot 0x11),
    // Harah (8).
    fn ending_scene_07_harah(&mut self) {
        self.ending_room_and_head(0x703, 0x1180, 8);
    }

    // = seg000:1625 callback_intro_08_01625 — palace room 0x2008, Duncan (2).
    fn ending_scene_08_duncan(&mut self) {
        self.ending_room_and_head(0x2008, 0x180, 2);
    }

    // = seg000:1630 callback_intro_09_01630 — sietch room 0x802 (slot 0x10),
    // Stilgar (5).
    fn ending_scene_09_stilgar(&mut self) {
        self.ending_room_and_head(0x802, 0x1080, 5);
    }

    // = seg000:09ad intro_26_baron — the intro's Baron scene.
    fn ending_scene_0a_baron(&mut self) {
        self.stage_26_init();
    }

    // = seg000:1603 callback_intro_0b_01603 — fortress room 0x3002 (slot 2),
    // Feyd (0ah) shifted right by 3ah, and the lip-sync started.
    fn ending_scene_0b_feyd_fortress(&mut self) {
        self.draw_room_for_scene(0x3002, 0x280);
        // = seg000:160c..1614 al = 0ah; dx = 3ah; loc_009c7; jmp
        //   start_room_lip_sync.
        self.setup_talking_head(0x0a, 0x3a);
        self.start_room_lip_sync();
    }

    // = seg000:164c callback_intro_0c_0164c — sietch room 0x803 (slot 0x10),
    // Chani (7).
    fn ending_scene_0c_chani(&mut self) {
        self.ending_room_and_head(0x803, 0x1080, 7);
    }

    // = seg000:163e callback_intro_0d_0163e — sietch room 0x1005 (slot 0x3f),
    // Liet (6).
    fn ending_scene_0d_liet(&mut self) {
        self.ending_room_and_head(0x1005, 0x3f80, 6);
    }

    // = the play_credits entry of array_callbacks_intro_0165a.
    fn ending_scene_0e_credits(&mut self) {
        self.play_credits(false);
    }

    // = seg000:167c callback_action_in_continue_sequence_0b — the
    // end-credits slideshow: the dialogue and cast cleared, the screen
    // captured into fb1 at row 18, WORMSUIT playing; each scene of
    // array_callbacks_intro_0165a is composed offscreen, captioned with the
    // next string from 0x120 on and revealed with transition 0x3a, then held
    // (0x258 ticks, or until its clip ends); a key at the end exits to DOS.
    pub(crate) fn sequence_action_0b_ending_credits(&mut self) -> ! {
        // = seg000:167c..1688.
        self.is_dialogue_active = false;
        self.persons_in_room = 0;
        self.persons_travelling_with = 0;
        if let Some(head) = self.talking_head.as_mut() {
            head.talking_head_id = 0xffff;
        }
        // = seg000:168b..1697 clear, suppress the sky span, no pause, fb1 =
        //   the screen.
        self.gfx_clear_active_framebuffer();
        self.data_0227d = self.data_0227d.wrapping_add(1);
        self.pause_enabled = 0;
        self.gfx_copy_screen_to_framebuffer_1();
        // = seg000:169a/169d vga_set_fb_row(12h) — the game area sits at
        //   row 18.
        self.y_offset = 0x12;
        // = seg000:16a1 call play_music_WORMSUIT_HSQ.
        self.play_music_wormsuit_hsq();
        // = seg000:16a4 string_subst_id_table[0] = 120h.
        self.string_subst_id_table[0] = 0x120;
        // = seg000:16aa..16ef the scene loop over array_callbacks_intro_0165a.
        for scene in ENDING_SCENES {
            // = seg000:16b0 hnm_finished_flag = 0ffh.
            self.hnm_finished = true;
            // = seg000:16be..16c9 remove_all_frame_tasks; save the palette as
            //   the fade target; clear; the scene composed offscreen.
            self.remove_all_frame_tasks();
            gfx::vga_save_palette_to_fade_target(self);
            self.gfx_clear_active_framebuffer();
            self.gfx_call_bp_with_front_buffer_as_screen(scene);
            // = seg000:16cc inc [string_subst_id_table].
            self.string_subst_id_table[0] = self.string_subst_id_table[0].wrapping_add(1);
            // = seg000:16d0/16d3 fb1 active; ending_draw_caption.
            self.set_fb1_as_active_framebuffer();
            self.ending_draw_caption();
            // = seg000:16d6..16de transition(al = 3ah, bp = nullsub);
            //   update_screen_palette.
            self.transition(0x3a, 0, |_| {});
            self.update_screen_palette();
            // = seg000:16e1..16ea hold 258h ticks per pass until the clip (if
            //   any) is complete.
            loop {
                self.wait_interruptable(0x258);
                if self.hnm_is_complete() {
                    break;
                }
            }
            // = seg000:16ec call loc_09985.
            self.idle_run_to_window_boundary();
        }
        // = seg000:16f1..16f9 wait for a key, then exit_to_dos.
        while !self.any_key_pressed() {
            self.tick_one_frame();
        }
        self.exit_to_dos()
    }

    // = seg000:16fc game_phase_set_to_c8_game_ending — the game is won:
    // phase 0xc8 and the ending script.
    pub(crate) fn game_phase_set_to_c8_game_ending(&mut self) {
        self.game_phase = crate::game_phase::PHASE_C8_GAME_WON;
        self.start_scripted_dialogue(&crate::sequence::SCRIPT_GAME_WON);
    }

    /// = draw_sprite_clobbering_bx_dx from the active bank into the active
    /// framebuffer at (x, y), unclipped.
    fn draw_sprite_at(&mut self, id: u16, x: i16, y: i16) {
        let yoff = self.y_offset as i16;
        let full: Rect = rect(0, 0, 320, 200);
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_sprite_from_sheet_clipped(sheet, id, x, y + yoff, full);
        });
    }
}
