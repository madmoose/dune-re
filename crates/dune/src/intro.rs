#![allow(unused)]

use std::io::Cursor;

use bytes_ext::ReadBytesExt;

use crate::{
    Color, FrameBuffer, GameState, Rect, SpriteSheet,
    attack::AttackState,
    draw_sprite_from_sheet, gfx,
    sprite_bank::{
        self, BACK, INT02, INT04, INT05, INT06, INT07, INT08, INT09, INT10, INT11, INT13, INT14,
        INT15, SUNRS,
    },
    sprite_blitter,
};

struct IntroStage {
    wait_init: u16,
    init: fn(&mut GameState),
    wait_play: u16,
    transition: i8,
    play: fn(&mut GameState),
    wait_pcm: u16,
}

const INTRO_SCRIPT_START: usize = 0;

const fn tick(measure: u16, tick: u16) -> u16 {
    16 * measure + tick
}

macro_rules! intro_stage {
    (
        $num:literal,
        $wait_init_a:tt / $wait_init_b:tt,
        $wait_play_a:tt / $wait_play_b:tt,
        $transition:expr,
        $wait_pcm:expr
    ) => {
        IntroStage {
            wait_init: tick($wait_init_a, $wait_init_b),
            init: paste::paste!(GameState::[<stage_ $num _init>]),
            wait_play: tick($wait_play_a, $wait_play_b),
            transition: $transition,
            play: paste::paste!(GameState::[<stage_ $num _play>]),
            wait_pcm: $wait_pcm,
        }
    };
}

#[rustfmt::skip]
const INTRO_SCRIPT: [IntroStage; 48] = [
    // Play VIRGIN.HNM.
    intro_stage!(00,   0/ 0, 0/ 0, 0x3a,     1 ),
    intro_stage!(01,   0/ 0, 0/ 0, 0x3a,     1 ),

    // Play CRYO.HNM.
    intro_stage!(02,   0/ 0, 0/ 0, 0x30,     1 ),

    // Play CRYO2.HNM.
    intro_stage!(03,   0/ 0,  6/15, 0x30,    1 ),
    intro_stage!(04,   0/ 0, 10/ 8,   -1,    1 ),

    // Play PRESENT.HNM.
    intro_stage!(05,   0/ 0,  0/ 0, 0x3a,    1 ),

    // Play IRULAN.HNM.
    intro_stage!(06,   0/ 0,  0/ 0, 0x3a,    1 ),

    // Clear frame.
    intro_stage!(07,   0/ 0,  0/ 0, 0x3a,    1 ),

    // Play TITLE.HNM (86 frames)
    intro_stage!(08,   0/ 0,  0/ 0, 0x36,  400 ),
    intro_stage!(09,   0/ 0,  9/ 0, 0x30,  400 ),

    // Continue playing TITLE.HNM to completion.
    intro_stage!(10,   0/ 0, 16/12,   -1,    1 ),

    // Desert sky scene.
    intro_stage!(11,   0/ 0,  0/ 0, 0x3a, 1200 ),

    // Desert flight to palace.
    intro_stage!(12,  20/ 8, 20/14, 0x10, 6400 ),

    // Palace equipment room.
    intro_stage!(13,  36/11, 36/14, 0x10,  133 ),

    // Lady Jessica
    intro_stage!(14,   0/ 0, 37/ 8, 0x10,  133 ),

    // Duke Leto talking head.
    intro_stage!(15,   0/ 0, 38/ 2, 0x10,  600 ),

    // Lady Jessica talking head.
    intro_stage!(16,  44/10, 44/15, 0x10,  600 ),

    // Paul on red background.
    intro_stage!(17,  50/14, 51/ 0, 0x10,  200 ),

    // Outside of palace.
    intro_stage!(18,  51/12, 52/ 4, 0x10,  133 ),

    // Desert flight to sietch.
    intro_stage!(19,   0/ 0, 52/14, 0x10, 9800 ),

    // Inside sietch.
    intro_stage!(20,  75/14, 13/ 8, 0x10,  133 ),

    // Chani in sietch.
    intro_stage!(21,   0/ 0, 14/ 8,   -1,  600 ),

    // Kynes in front of a desert city.
    intro_stage!(22,  80/12, 18/ 8, 0x10,  600 ),

    // Stilgar in a sietch.
    intro_stage!(23,  84/12, 22/ 8, 0x10,  600 ),

    // Midnight desert sky.
    intro_stage!(24,  88/ 8, 26/ 7, 0x10,  800 ),

    // Feyd-Rautha on purple background.
    intro_stage!(25,  90/12, 28/ 6, 0x30,  600 ),

    // Baron Harkonnen on purple background.
    intro_stage!(26,  94/14, 32/ 8, 0x30,  600 ),

    // Baron's guard slides in.
    intro_stage!(27,  97/ 4,  0/ 0,   -1,   50 ),

    // Night attack on the sietch.
    intro_stage!(28,  97/14, 35/ 6, 0x10, 2000 ),

    // MTG3.HNM.
    intro_stage!(29, 100/12,  0/ 0, 0x10, 3200 ),

    // INT05.HSQ still.
    intro_stage!(30, 108/ 6, 46/ 0, 0x3a,  400 ),

    // INT10.HSQ still.
    intro_stage!(31,   0/ 0, 48/ 6, 0x10,  400 ),

    // INT08.HSQ still.
    intro_stage!(32,   0/ 0, 49/ 6, 0x10,  400 ),

    // INT04.HSQ still.
    intro_stage!(33,   0/ 0, 50/ 6, 0x10,  400 ),

    // INT09.HSQ still.
    intro_stage!(34,   0/ 0, 52/ 6, 0x10,  400 ),

    // INT11.HSQ still.
    intro_stage!(35,   0/ 0, 53/ 6, 0x10,  400 ),

    // INT02.HSQ still.
    intro_stage!(36,   0/ 0, 54/ 6, 0x10,  400 ),

    // INT06.HSQ still.
    intro_stage!(37,   0/ 0, 56/ 6, 0x10,  400 ),

    // Water ripples in cave.
    intro_stage!(38,   0/ 0, 57/ 6, 0x10, 1200 ),

    // PLANT.HNM.
    intro_stage!(39, 122/12, 60/ 7, 0x10, 1000 ),

    // INT07.HSQ still.
    intro_stage!(40, 124/14, 62/ 6, 0x10,  400 ),

    // VER.HNM.
    intro_stage!(41,   0/ 0,  0/12, 0x3a,    1 ),

    // INT15.HSQ still.
    intro_stage!(42,   0/ 0,  4/12, 0x3a,  400 ),

    // INT13.HSQ still.
    intro_stage!(43,   0/ 0,  6/14, 0x10,  400 ),

    // INT14.HSQ still.
    intro_stage!(44,   0/ 0,  7/14, 0x10,  200 ),

    // Clear.
    intro_stage!(45,   0/ 0,  8/14, 0x3a,    1 ),

    // INT15.HSQ still.
    intro_stage!(46,   0/ 0,  8/14, 0x36,  400 ),

    // Clear.
    intro_stage!(47,   0/ 0, 17/ 0, 0x38,    1 ),
];

// = seg001:1500 _stru_209B0_icon_list: the desert-sky icon list, (sprite, x, y)
// triples terminated by 0xffff. Drawn from SUNRS.HSQ by the sky scenes (the
// desert sky, midnight, and the Kynes backdrop).
#[rustfmt::skip]
const SKY_ICON_LIST: [(u16, i16, i16); 6] = [
    (2,   0,   0),
    (3,   0,  25),
    (4,   0,  50),
    (5,   0,  74),
    (6, 134,  92),
    (0,   0, 102),
];

impl GameState {
    // Run one intro stage's init function by script index. Exposed for the
    // headless render examples / tests so they exercise the real INTRO_SCRIPT
    // wiring; not used by play_intro itself.
    #[doc(hidden)]
    pub fn intro_stage_init(&mut self, idx: usize) {
        (INTRO_SCRIPT[idx].init)(self);
    }

    // Run one intro stage's play function by script index. Exposed for the
    // headless render examples / tests; not used by play_intro itself.
    #[doc(hidden)]
    pub fn intro_stage_play(&mut self, idx: usize) {
        (INTRO_SCRIPT[idx].play)(self);
    }

    // = seg000:0580 play_intro. Iterates the intro_script (seg000:0337) and
    // drives each stage in sequence: pre-load midi sync, init, post-load
    // midi sync, transition, play, then the wait-for-pcm-voice gate.
    pub fn play_intro(&mut self, skip: bool) {
        // = seg000:0589 — initial midi driver reset before the script loop.
        self.midi.midi_reset();

        if !skip {
            self.intro_aborted = false;
            for (idx, stage) in INTRO_SCRIPT.iter().enumerate().skip(INTRO_SCRIPT_START) {
                // = seg000:0592 mov ax, 18h; call vga_set_fb_row — every stage
                // starts with fb_base_ofs=24, the standard in-game blit baseline.
                gfx::vga_set_fb_row(self, 24);

                // = seg000:05a3 midi_wait_until(unk0) — wait for pre-load music sync point.
                if !self.midi.midi_wait_until(stage.wait_init) {
                    return;
                }

                // = seg000:05a8 remove_all_frame_tasks. Each stage starts with a
                // clean frame-task list; a previous stage's installed tasks (e.g.
                // stage 11's sky palette cycler) do not leak into the next.
                // remove_all_frame_tasks also resets sky_skydn_selector to 1
                // (= seg000:0920), so every intro stage's sky load/fade uses the
                // SKYDN.HSQ / 73..223 branch (stage 29's desert flyover; stages
                // 11/24 cyclers are unaffected because their extra span fades
                // toward the just-saved palette).
                self.remove_all_frame_tasks();

                // = seg000:05ab j_vga_save_palette_to_fade_target — snapshot the
                // currently-visible palette so a 0x3a transition can fade OLD→
                // black→NEW.
                self.palette_fade_target = self.palette.clone();

                // = seg000:05b4 gfx_call_bp_with_front_buffer_as_screen — invoke
                // the stage's load/init function with fb1 as both the active target
                // and the front buffer, so the init renders fully offscreen and the
                // following transition reveals it.
                self.gfx_call_bp_with_front_buffer_as_screen(stage.init);

                // = seg000:05bc remove_room_frame_task — drop any room task left from
                // a prior stage before this stage's transition runs. (remove_all_frame_tasks
                // above already cleared it; this mirrors the DOS sequence exactly.)
                self.remove_room_frame_task();

                // = seg000:05c4 midi_wait_until(midi) — wait for post-load music sync point.
                if !self.midi.midi_wait_until(stage.wait_play) {
                    return;
                }

                // = seg000:05c9 if transition >= 0: call transition; else skip.
                if stage.transition >= 0 {
                    gfx::vga_transition(self, stage.transition as u16, 0);

                    // = seg000:05d6 call update_screen_palette — flush the new
                    // palette to the screen now the fade-out has finished ("load
                    // and fade in"). Reached only on transition stages; the `js` at
                    // seg000:05ce skips it (and the room task) when transition < 0,
                    // so stage 21 must flush itself — see stage_21_play.
                    self.update_screen_palette();
                    // = seg000:05d9 add_room_frame_task — reinstall the room frame
                    // task after the fade-in. Only runs on stages with a transition
                    // (the 05ce `js` skips both for transition < 0); its own guard
                    // means it installs nothing for the intro's room values.
                    self.add_room_frame_task();
                }

                // = seg000:05e4 any_key_pressed; jb loc_005fd — a keypress
                // pending before the stage plays aborts the whole intro.
                if self.intro_input_pressed() {
                    break;
                }

                // = seg000:05ed call stage.play.
                (stage.play)(self);
                // = seg000:05ef jb loc_005fd — the stage's HNM loop sets
                // intro_aborted when a keypress skipped the clip; that ends the
                // whole intro, not just the current stage.
                if self.intro_aborted {
                    break;
                }

                // = seg000:05f8 wait_for_pcm_voice_interruptable; 05fb jnb -> next
                // stage. A keypress during the wait aborts the whole intro.
                if self.wait_for_pcm_voice_interruptable(stage.wait_pcm as u64) {
                    break;
                }
            }

            // = seg000:05fd loc_105FD — the play_intro exit tail (taken both on
            // normal script completion and on ESC/midi abort). The clear_global_
            // y_offset / hnm_close_resource / cleanup_sub_19985 work is handled
            // elsewhere in the port; the two state writes below must happen here
            // because the in-game code (play_intro2 → set_sky_palette via
            // get_sky_palette_id_from_game_time_in_bl) reads them.
            //
            // = seg000:0611 mov game_time, 2 — seed the in-game clock so the
            // intro2 cutscene's set_sky_palette picks sub-palette byte_21730[2]=9
            // (the dusk gradient that matches the first WORMSUIT scene).
            self.game_time = 2;
            // = seg000:0617 remove_all_frame_tasks: among other things sets
            // sky_skydn_selector = 1 (seg000:0920), routing the sky load through
            // SKYDN.HSQ until the in-game room view explicitly reselects it.
            self.remove_all_frame_tasks();
        }
    }

    // = seg000:0309 play_CREDITS_HNM. The second startup step (start calls it
    // right after play_intro): fade in CREDITS.HNM (resource 0x14) under the
    // WORMSUIT score and play it to completion.
    //
    //   seg000:030b kb_clear_scancode
    //   seg000:030e set_fb1_as_active_framebuffer
    //   seg000:0311 gfx_clear_active_framebuffer
    //   seg000:0314 vga_set_fb_row(18h)                ; game-area top = row 24
    //   seg000:031b bp = play_CREDITS_HNM_load (seg000:09ef: mov ax,14h; jmp
    //               hnm_load_first_frame — resource 0x14 = CREDITS.HNM)
    //   seg000:031e copy_pal_and_transition            ; load offscreen + 3a fade
    //   seg000:0321 play_music_WORMSUIT_HSQ
    //   seg000:0324 loop loc_00a16 / check_if_hnm_complete / any_key_pressed
    //   seg000:0332 clear_global_y_offset
    //
    // DOS skips the whole routine when entered with ZF set (the intro aborted
    // with ESC); the port has no intro-abort path yet, so it always plays. The
    // frame loop is the data_0227d != 0 branch of loc_00a23
    // (set_screen_as_active_framebuffer + hnm_do_frame): decode straight onto
    // the visible screen, the same idiom as intro_play_hnm_skippable.
    pub fn play_credits(&mut self, skip: bool) {
        // = seg000:030b kb_clear_scancode — drop any scancode left from the
        // intro so a stale keypress doesn't instantly skip the credits.
        self.kb_clear_scancode();
        // = seg000:030e/0311 set fb1 active and clear it (the load target).
        self.set_fb1_as_active_framebuffer();
        self.gfx_clear_active_framebuffer();
        // = seg000:0314 mov ax,18h; call vga_set_fb_row — game-area top row 24.
        gfx::vga_set_fb_row(self, 0x18);

        if !skip {
            // = seg000:031e copy_pal_and_transition (bp = the HNM 0x14 loader):
            // snapshot the visible palette as the fade-from target (= seg000:c102
            // [3959h] j_vga_save_palette_to_fade_target), load CREDITS.HNM's first
            // frame + palette into fb1 offscreen, then fade OLD→black→NEW (0x3a).
            self.palette_fade_target = self.palette.clone();
            self.gfx_call_bp_with_front_buffer_as_screen(|s| {
                // CREDITS.HNM carries no SD audio (the score is the WORMSUIT MIDI),
                // so it is tick-paced; 16 ticks/frame matches the other full-screen
                // intro clips (e.g. VIRGIN).
                s.hnm_load_first_frame("CREDITS.HNM", 24);
            });
            gfx::vga_transition(self, 0x3a, 0);
            // = seg000:c12a/c12d gfx_copy_whole_framebuf_to_screen + palette_flush.
            self.gfx_copy_whole_framebuf_to_screen();
            self.update_screen_palette();

            // = seg000:0321 play_music_WORMSUIT_HSQ.
            self.midi.play_music_wormsuit_hsq(&mut self.dat_file);

            // = seg000:0324 loc_00324 — step CREDITS.HNM straight onto the screen
            // until the clip ends or the player presses a key/mouse button.
            self.set_screen_as_active_framebuffer();
            loop {
                self.spin_until_hnm_advances();
                self.send_frame_to_display();
                // = seg000:0327 check_if_hnm_complete; jnz done.
                if self.hnm_is_complete() {
                    break;
                }
                // = seg000:032c call any_key_pressed; jnb loc_00324 — loop while
                // there is no input, break to skip the rest of the credits.
                if self.intro_input_pressed() {
                    break;
                }
            }
        }

        // = seg000:0332 clear_global_y_offset.
        self.clear_global_y_offset();
    }

    // = seg000:021c play_intro2 lives in intro2.rs (the WORMSUIT second-intro act).

    // = seg000:ddf0 wait_for_pcm_voice_interruptable. When a talking-head voice
    // is playing, block until it finishes (DOS loops on
    // check_pcm_voice_file_open, ignoring the tick count) so the head talks to
    // the end. Otherwise fall back to the fixed timed wait — the frame-task
    // list keeps ticking either way (e.g. stage 11's sky palette cycler).
    //
    // Returns true if a keypress interrupted the wait (= seg000:de01
    // any_key_pressed; CF=1), which play_intro treats as a request to abort the
    // whole intro (05fb jnb -> exit tail).
    fn wait_for_pcm_voice_interruptable(&mut self, wait: u64) -> bool {
        if self.talking_head.as_ref().is_some_and(|h| h.speaking) {
            // = seg000:ddfc voice-playing loop.
            while self.talking_head.as_ref().is_some_and(|h| h.speaking) {
                if self.intro_input_pressed() {
                    return true;
                }
                self.tick_one_frame();
            }
            false
        } else if wait != 0 {
            // The timed-wait branch, interruptable by a keypress (mirrors
            // wait_frame_tasks_for_ticks but reports whether it was interrupted).
            let deadline = self.game_ticks() + wait;
            while self.game_ticks() < deadline {
                if self.intro_input_pressed() {
                    return true;
                }
                self.tick_one_frame();
            }
            false
        } else {
            false
        }
    }

    // = seg000:061c load_VIRGIN_HNM. play_music_MORNING_HSQ + open VIRGIN.HNM
    // (resource 0x15) via hnm_load_first_frame. Matches DOS exactly: no
    // y-offset reset — VIRGIN decodes into framebuffer rows 24-199 via
    // the gfx-layer's fb_base_ofs application.
    fn stage_00_init(&mut self) {
        self.midi.play_music_morning_hsq(&mut self.dat_file);
        self.hnm_load_first_frame("VIRGIN.HNM", 0);
    }

    // = seg000:0625 play_VIRGIN_HNM. Spin until hnm_do_frame advances,
    // copy fb1 → screen, run the midi-reset gate at measure 8, loop until
    // the clip completes.
    fn stage_00_play(&mut self) {
        // = seg000:0628 set_fb1_as_active_framebuffer — decode HNM into fb1
        // (already active after the gfx_call_bp init, re-asserted here).
        self.set_fb1_as_active_framebuffer();
        loop {
            // = seg000:0628 any_key_pressed; 062b jb -> ret with CF=1 — a keypress
            // ends the whole intro (intro_aborted), not just VIRGIN.
            if self.intro_input_pressed() {
                self.intro_aborted = true;
                break;
            }

            // = seg000:0628 wait for the next HNM frame.
            self.spin_until_hnm_advances();
            // = seg000:0632 gfx_copy_whole_framebuf_to_screen.
            self.gfx_copy_whole_framebuf_to_screen();
            self.send_frame_to_display();
            // = seg000:0635 once the music has reached measure 8 and a song
            // is still loaded, midi_reset stops the song. One-shot: midi_reset
            // clears the song index, so the gate fails on later iterations.
            if self.midi.current_measure() >= 8 && self.midi.is_song_loaded() {
                self.midi.midi_reset();
            }
            // = seg000:0646 check_if_hnm_complete.
            if self.hnm_is_complete() {
                break;
            }
        }
    }

    // = seg000:0345 stage 1 load: gfx_clear_active_framebuffer (script
    // entry's load field points directly at the helper).
    fn stage_01_init(&mut self) {
        self.gfx_clear_active_framebuffer();
    }

    // = seg000:034b stage 1 play: loc_00f66 (no-op return).
    fn stage_01_play(&mut self) {}

    // = seg000:064d load_CRYO_HNM. midi_play_song(0x0a) + open CRYO.HNM
    // (resource 0x16). The DOS function does not reset the global
    // y-offset; with the gfx-layer now applying `state.y_offset` to HNM
    // blits (mirroring DOS `fb_base_ofs`), CRYO.HNM correctly decodes
    // into framebuffer rows 24-199 with rows 0-23 left at zero.
    fn stage_02_init(&mut self) {
        self.midi.midi_play_song(10, &mut self.dat_file);
        self.hnm_load_first_frame("CRYO.HNM", 0);
    }

    // = seg000:0661 play_CRYO_OR_CRYO2_HNM.
    fn stage_02_play(&mut self) {
        self.play_cryo_or_cryo2_hnm();
    }

    // = seg000:0658 load_CRYO2_HNM. gfx_clear_active_framebuffer + open
    // CRYO2.HNM (resource 0x17). Matches DOS exactly: no y-offset reset.
    fn stage_03_init(&mut self) {
        self.gfx_clear_active_framebuffer();
        self.hnm_load_first_frame("CRYO2.HNM", 0);
    }

    // = seg000:0661 play_CRYO_OR_CRYO2_HNM.
    fn stage_03_play(&mut self) {
        self.play_cryo_or_cryo2_hnm();
    }

    // Stage 4:
    // gate at unk0=0 / midi=0xa8 in play_intro provides the timing.
    fn stage_04_init(&mut self) {}
    fn stage_04_play(&mut self) {}

    // = seg000:0678 load_PRESENT_HNM. clear_global_y_offset +
    // gfx_clear_active_framebuffer + open PRESENT.HNM (resource 0x18).
    fn stage_05_init(&mut self) {
        self.clear_global_y_offset();
        self.gfx_clear_active_framebuffer();
        self.hnm_load_first_frame("PRESENT.HNM", 0);
    }

    // = seg000:0684 play_PRESENT_HNM → jmp intro_play_hnm_skippable.
    fn stage_05_play(&mut self) {
        self.intro_play_hnm_skippable();
    }

    // = seg000:cefc load_IRULn_HSQ. open_spritesheet(0x69 +
    // language_setting) for the subtitle bank, reset the subtitle pointer,
    // vga_set_fb_row(0) + gfx_clear_active_framebuffer + open IRULAN.HNM
    // (resource 0x19).
    fn stage_06_init(&mut self) {
        self.open_sprite_bank(sprite_bank::IRUL1 + self.language_setting as i16);
        self.clear_global_y_offset();
        self.gfx_clear_active_framebuffer();
        self.hnm_load_first_frame("IRULAN.HNM", 0);
    }

    // = seg000:cf1b play_IRULx_HSQ. Each iteration: read the next subtitle
    // (frame, action) pair; if the current HNM frame has passed it, call
    // IRULx_draw_or_clear_subtitle; then hnm_do_frame_skippable + check
    // completion.
    fn stage_06_play(&mut self) {
        const SUBTITLE_FRAMES: [i16; 61] = [
            119, 137, 138, 173, 186, 238, 248, 269, 270, 305, 314, 338, 348, 358, 360, 388, 389,
            415, 425, 460, 470, 518, 528, 571, 576, 604, 605, 659, 660, 685, 693, 744, 746, 757,
            761, 818, 827, 866, 875, 945, 950, 1000, 1012, 1042, 1044, 1075, 1085, 1119, 1120,
            1142, 1147, 1169, 1172, 1214, 1226, 1259, 1266, 1285, 1294, 1315, -1,
        ];
        let mut idx = 0;

        let saved = self.active_fb;
        // = seg000:cf0e/cf12 vga_set_fb_row(0) + gfx_clear_active_framebuffer:
        // IRULAN renders straight to the screen. hnm_decode_video_frame's
        // seg000:ccd7 dispatch routes IRULAN (video id 0x19) through the
        // checkerboard 2x blit, so the decoded 160x91 frame is spread across
        // 320x182 writing only the even-column/even-row pixels; the cleared
        // background shows through the gaps.
        self.set_screen_as_active_framebuffer();
        loop {
            let hnm_frame = self.hnm_frame_counter as i16;

            // = seg000:cf22..cf2d advance the subtitle cursor once the HNM frame
            // passes the next (frame, action) threshold.
            if SUBTITLE_FRAMES[idx] > 0 && hnm_frame > SUBTITLE_FRAMES[idx] {
                idx += 1;
            }

            // = seg000:cf30 hnm_do_frame_skippable — advance one frame; a
            // keypress skips the rest of the narration.
            if self.hnm_do_frame_skippable() {
                break;
            }

            // Draw the subtitle.
            if idx > 0 {
                let last = idx - 1;
                if last % 2 == 0 {
                    // = seg000:cf53
                    self.draw_active_bank_sprite((last / 2) as u16, 0, 190);
                } else {
                    // = seg000:cf61 clear the subtitle.
                    for y in 190..200 {
                        for x in 0..320 {
                            self.screen.set(x, y, 0);
                        }
                    }
                }
            }
            self.send_frame_to_display();
            self.global_frame_count += 1;
            // = seg000:cf35 check_if_hnm_complete.
            if self.hnm_is_complete() {
                break;
            }
        }

        // = seg000:cf3c call hnm_close_resource — release the HNM clip (a no-op
        // when it already finished and self-closed; needed on the skip path).
        self.hnm_close();
        // = seg000:cf3f call pcm_stop_voc — stop the dnsdb driver now the clip
        // is done. Each HNM SD buffer was queued with the loop-whole flag
        // (copy_sd_chunk_to_pcm_buf, seg000:aa91), so without this the final
        // chunk loops forever.
        self.pcm_player.stop();
        // = seg000:cf42 call play_music_MORNING_HSQ.
        self.midi.play_music_morning_hsq(&mut self.dat_file);

        // = seg000:cf46 pop framebuffer_active_seg.
        self.active_fb = saved;
    }

    fn stage_06_draw_or_clear_subtitle(&mut self) {}

    fn stage_07_init(&mut self) {
        self.gfx_clear_active_framebuffer();
    }

    fn stage_07_play(&mut self) {}

    // = seg000:069e load_TITLE_HNM.
    fn stage_08_init(&mut self) {
        self.clear_global_y_offset();
        self.hnm_load_first_frame("TITLE.HNM", 0);
    }

    // Stage 8 play=loc_00f66 (no-op). The 0x36 transition fades the title
    // in; play_intro then runs wait_for_pcm_voice_interruptable(0x190).
    fn stage_08_play(&mut self) {}

    // Stage 9:
    fn stage_09_init(&mut self) {}

    // = seg000:06aa intro_play_hnm_86_frames. Spin frames until the HNM
    // frame counter reaches 86 (0x56), then return. Stage 10 continues
    // playback from there.
    fn stage_09_play(&mut self) {
        self.clear_global_y_offset();

        self.set_screen_as_active_framebuffer();
        loop {
            // = seg000:06b0 hnm_do_frame_skippable; jb -> done.
            if self.hnm_do_frame_skippable() {
                break;
            }
            self.send_frame_to_display();
            let hnm_frame = self.hnm_frame_counter;
            if hnm_frame == 86 {
                break;
            }
        }
    }

    // Stage 10:
    fn stage_10_init(&mut self) {}

    // = seg000:06bd intro_play_hnm_skippable. Plays the rest of TITLE.HNM
    // (picks up from where stage 9 left off) until completion.
    fn stage_10_play(&mut self) {
        self.intro_play_hnm_skippable();
    }

    // = seg000:07fd intro_load_desert_sky_animation.
    // gfx_clear_active_framebuffer + bl=8 + fall through to loc_00802
    // (open SUNRS sub-palette 8, draw the icon list). Writes only to the
    // offscreen framebuffer; the subsequent 0x3a transition flips fb1 onto
    // the screen at the black moment.
    fn stage_11_init(&mut self) {
        self.gfx_clear_active_framebuffer();
        self.load_sky_palette_and_draw_sky_sprite_list(8);
    }

    // = seg000:085d intro_play_desert_sky_animation. Installs the sky
    // palette-cycling frame task at loc_00826 (interval=9 PIT ticks) and
    // returns. play_intro's wait_for_pcm_voice_interruptable(0x4b0) drives
    // the task for the duration of the scene.
    fn stage_11_play(&mut self) {
        // = seg000:0860 add_frame_task(loc_00826, bp=9). The starting
        // sub-palette is the one stage_11_init's loc_00802(8) loaded
        // ([46d6h]=8, _byte_23B86_current_sky_palette); the fade countdown
        // ([46d7h]) is 0.
        self.add_sky_palette_cycler(9);
    }

    // = seg000:0826 loc_00826 — install the sky palette-cycling frame task. The
    // cycler advances through sky sub-palettes, cross-fading each, until it
    // reaches sub-palette 0x0e (done). Shared by the desert-sky scene (stage 11,
    // start=8, interval=9) and the midnight scene (stage 24, start=0x0b,
    // interval=0x10, installed by loc_0087b).
    // = seg000:085d intro_play_desert_sky_animation / seg000:087b loc_0087b —
    // install the cycler. DOS does NOT set [46d6h] here: the starting
    // sub-palette is whatever the preceding loc_00802 loaded into
    // current_sky_palette. [46d7h] (the fade countdown) is 0 from the stage's
    // remove_all_frame_tasks; set it explicitly to make the no-fade start state
    // independent of call order.
    fn add_sky_palette_cycler(&mut self, interval: u16) {
        self.sky_fade_countdown = 0;
        self.add_frame_task(interval, crate::TaskId::SkyPaletteCycler);
    }

    // = seg000:0826 loc_00826 — one tick of the sky palette cycler. When no
    // fade is in flight, advance to the next sub-palette (finishing at 0x0e or
    // stalling at 0x0b), set up its fade target, then step the active fade.
    pub(crate) fn tick_sky_palette_cycler(&mut self) {
        if self.sky_fade_countdown == 0 {
            // No fade in progress: advance to the next sub-palette.
            let next = self.current_sky_palette.wrapping_add(1);
            match next {
                // = seg000:0867 loc_00867: al==0x0e — all transitions done.
                0x0e => {
                    self.remove_frame_task(crate::TaskId::SkyPaletteCycler);
                    return;
                }
                // = seg000:0857: al==0x0b — open CHAN.HSQ resource (pre-cache
                // for the Chani scene). current_sky_palette is NOT updated here
                // (matches DOS: open_spritesheet doesn't write [46d6h]).
                // The cycling stalls at sub-palette 10 for the remainder of the
                // 0x4b0-tick scene while ambient audio plays.
                0x0b => return,
                // = seg000:0841 loc_00841: set countdown and load the new
                // sub-palette as the fade target.
                _ => {
                    // al==0x0d (13) uses a shorter 10-step fade; others use 30.
                    self.sky_fade_countdown = if next == 0x0d { 0x0a } else { 0x1e };
                    // = seg000:0847 call open_sunrs_palette (bl=next) +
                    // seg000:0850 j_vga_set_fade_target_data(cx=0xf0, bx=0x180):
                    // write the new sub-palette into palette_to_transition_from
                    // entries 128..207 so vga_fade_step can step toward it. The
                    // load also advances current_sky_palette (= seg000:3982),
                    // exactly as DOS does — loc_00826 has no [46d6h] write of its
                    // own.
                    self.load_sunrs_palette_to_fade_target(next as usize);
                }
            }
        }
        // = loc_0391d / segvga:0ad7 vga_fade_step(al=countdown, bx=0x180, cx=0xf0):
        // advance each of palette[128..207] by
        //   (palette_to_transition_from[i] - palette[i]) / countdown
        // (signed integer division, truncates toward zero, matching idiv).
        let countdown = self.sky_fade_countdown;
        self.sky_palette_fade_step(countdown);
        self.sky_fade_countdown -= 1;

        // = the DOS vga_fade_step writes the VGA DAC directly, so each step is
        // visible immediately. Here the scene pixels are static on self.screen
        // (the stage transition left the sky there); only the palette changed,
        // so re-present it to make the cross-fade visible.
        self.send_frame_to_display();
    }

    // = seg000:06ce intro_load_desert_flyover_video. loc_006f3:
    // add 0x1e0 to active framebuffer segment (shifts write target 24
    // rows; play_intro already set that offset via vga_set_fb_row(24))
    // then open PLANT.HNM (resource 0x10; ticks_per_frame = high byte
    // of RES_PLANT_HNM.unk0 = 0x19 = 25).
    fn stage_12_init(&mut self) {
        self.hnm_load_first_frame("MTG1.HNM", 24);
    }

    // = seg000:0704 intro_play_hnm_with_frame_task. Installs a frame
    // task (bp=0, fires every tick) that calls hnm_do_frame. In DOS
    // the active framebuffer segment already points at VGA screen
    // memory + 24 rows so hnm_do_frame writes directly to the visible
    // screen; the Rust port mirrors that with an explicit
    // framebuffer→screen copy and display after each decoded frame.
    fn stage_12_play(&mut self) {
        self.intro_play_hnm_with_frame_task();
    }

    // = seg000:0972 intro_palace_equipment_room. Clears the active framebuffer
    // and draws the palace equipment room through the generic room/scene
    // renderer, driven by the same constants the DOS routine uses:
    //
    //   seg000:0972 gfx_clear_active_framebuffer
    //   seg000:0975 mov dx, 2002h        ; location_and_room
    //   seg000:0978 persons_in_room = 0  ; no characters in this scene
    //   seg000:097e mov bx, 180h         ; location_appearance (bh=1)
    //   seg000:0981 call loc_008f0       ; -> open_SAL_resource
    //   seg000:0984 call loc_037b2       ; -> draw_SAL
    //   seg000:0987 jmp  copy_active_framebuffer_to_framebuffer_2
    //
    // draw_location_room resolves dx/bx into PALACE.SAL room 9 + EQUI.HSQ via
    // the ported scene tables (see room_scene). The room lands in the game-area
    // rect (screen rows 24..175) at fb_base_ofs = 24, set by play_intro. DOS
    // then copies the active framebuffer to framebuffer_2; the dune-rs model
    // tracks a single offscreen framebuffer, so that copy is implicit.
    fn stage_13_init(&mut self) {
        self.gfx_clear_active_framebuffer();
        // = seg000:0978 persons_in_room = 0: the equipment room has no person.
        self.persons_in_room = 0;
        self.draw_location_room(0x2002, 0x180);
    }

    fn stage_13_play(&mut self) {}

    // = seg000:098a intro_lady_jessica_1. persons_in_room = 2 makes
    // draw_location_room draw Lady Jessica standing in palace room 1
    // (sal_read_position_markers -> sal_draw_character).
    fn stage_14_init(&mut self) {
        self.persons_in_room = 2;
        self.draw_location_room(0x2004, 0x180);
    }

    fn stage_14_play(&mut self) {}

    // = seg000:0995 intro_duke_leto. Draw the empty palace room
    // (persons_in_room = 0), then overlay Duke Leto's talking head: loc_00978
    // draws the room, al=0 selects LETO.HSQ, and loc_0099d (dx=0) sets up the
    // lip-sync data and renders the first frame via loc_0978e.
    fn stage_15_init(&mut self) {
        self.persons_in_room = 0;
        self.draw_location_room(0x200a, 0x180);
        // = seg000:099b xor al,al (LETO); seg000:099d loc_0099d xor dx,dx.
        self.setup_talking_head(0, 0);
    }

    // = seg000:0798 intro_talking_head_play — load + play Duke Leto's voice
    // .voc and sync his lips to it (falling back to the idle animation if no
    // voice file / PCM is available).
    fn stage_15_play(&mut self) {
        self.intro_talking_head_play();
    }

    // = seg000:0798 intro_talking_head_play. Set the voc index ([04780]=0x190)
    // and run loc_09efd: try to load the character's voice .voc and, on
    // success, start PCM playback and install the lip-sync frame task.
    pub fn intro_talking_head_play(&mut self) {
        // = seg000:0798 current_subtitle_id = 0x190. load_voc_and_lipsync_data's
        // index transform leaves it 0x190 (base table 0), and the special-room
        // suffix condition gives 'I' for the intro's scene.
        self.play_talking_head_voc(0x190, 'I');
    }

    // = seg000:0771 intro_lady_jessica_2. Draw the empty palace room
    // (persons_in_room = 0), then overlay Lady Jessica's talking head: dx=2004h,
    // loc_00978 draws the room, al=1 selects JESS.HSQ, loc_0099d sets up the
    // lip-sync data and renders the first frame.
    fn stage_16_init(&mut self) {
        self.persons_in_room = 0;
        self.draw_location_room(0x2004, 0x180);
        // = seg000:0777 mov al,1 (JESS); seg000:0779 jmp loc_0099d.
        self.setup_talking_head(1, 0);
    }

    // = seg000:0798 intro_talking_head_play — play Lady Jessica's voice and
    // sync her lips to it.
    fn stage_16_play(&mut self) {
        self.intro_talking_head_play();
    }

    // = seg000:07ee intro_paul_on_red_background. Opens BACK.HSQ (resource
    // 0x30 — the layered red-background sprite sheet) and draws its icon list
    // (seg001:1526) into the active framebuffer: sprite 0 is the full 320x152
    // red backdrop, sprites 1 and 2 are inner red vignette panels. Then
    // loc_00960 saves the backdrop to fb2 and overlays Paul's talking head
    // (character 0x2d → PAUL.HSQ) on top.
    fn stage_17_init(&mut self) {
        let back = self
            .dat_file
            .read("BACK.HSQ")
            .expect("failed to read BACK.HSQ");
        let sprite_sheet = SpriteSheet::from_slice(&back).expect("failed to parse BACK.HSQ");
        // = open_spritesheet -> apply_sprite_sheet_palette: BACK.HSQ
        // carries the red-background palette.
        sprite_sheet
            .apply_palette_update(&mut self.palette)
            .expect("failed to apply palette");

        // = seg001:1526 icon list: (sprite, x, y) triples, terminated by 0xffff.
        const ICON_LIST: [(u16, i16, i16); 3] = [(0, 0, 0), (1, 52, 25), (2, 108, 51)];
        self.draw_icons_list_at_si(&ICON_LIST, &sprite_sheet);

        // = seg000:0960 loc_00960: al=2dh (the player → PAUL.HSQ), dx=0.
        // setup_talking_head also performs the
        // copy_active_framebuffer_to_framebuffer_2 that saves the red backdrop
        // into fb2. (DOS's [478ch]=1 override only shortens the idle countdown,
        // which no longer freezes the head — see tick_talking_head_idle.)
        self.setup_talking_head(0x2d, 0);
    }

    // = seg000:0798 intro_talking_head_play — Paul has no intro voice file, so
    // this falls through to the (brief, [478ch]=1) idle animation.
    fn stage_17_play(&mut self) {
        self.intro_talking_head_play();
    }

    // = seg000:09a5 intro_outside_of_palace.
    fn stage_18_init(&mut self) {
        self.gfx_clear_active_framebuffer();
        let palais = self
            .dat_file
            .read("PALAIS.HSQ")
            .expect("failed to read PALAIS.HSQ");

        let sprite_sheet = SpriteSheet::from_slice(&palais).expect("failed to parse PALAIS.HSQ");

        sprite_sheet
            .apply_palette_update(&mut self.palette)
            .expect("failed to apply palette");

        gfx::draw_sprite_on_framebuffer(self, &sprite_sheet, 0, 0, 0);
    }

    fn stage_18_play(&mut self) {}

    fn stage_19_init(&mut self) {
        self.hnm_load_first_frame("MTG2.HNM", 24);
    }

    fn stage_19_play(&mut self) {
        self.intro_play_hnm_with_frame_task();
    }

    // = seg000:077c intro_inside_sietch. Establishing shot of the empty sietch
    // interior (Chani arrives in the next stage). Mirrors
    // intro_palace_equipment_room but with its own dx/bx and, crucially, a jmp
    // to loc_00981 that bypasses loc_00978's `persons_in_room = 0` write and
    // loc_0097e's default `bx = 180h`:
    //
    //   seg000:077c call gfx_clear_active_framebuffer
    //   seg000:077f mov  bx, 1080h        ; location_appearance (bh=0x10)
    //   seg000:0782 mov  dx, 803h         ; location_and_room
    //   seg000:0785 jmp  loc_00981        ; -> open_SAL + draw_SAL + copy
    //
    // draw_location_room resolves dx=803h/bx=1080h into SIET.SAL room 10 +
    // SIET1.HSQ (apparence locations[15]=8 -> SIET; scene record at dh=8/dl=3
    // has room byte 7bh). persons_in_room is left at whatever the prior stage
    // set it to — it is still 0 from stage 16, so the room draws with no
    // standing person.
    fn stage_20_init(&mut self) {
        self.gfx_clear_active_framebuffer();
        // = seg000:0785 jmp loc_00981 — DOS does NOT set persons_in_room here;
        // it carries over (= 0), drawing the empty sietch.
        self.draw_location_room(0x803, 0x1080);
    }

    // Stage 20 play = loc_00f66 (no-op). The 0x10 transition reveals the room
    // and play_intro's wait_for_pcm_voice_interruptable(133) holds the shot.
    fn stage_20_play(&mut self) {}

    // = seg000:0788 intro_chani_in_sietch: `mov al,7; jmp loc_0099d`. Chani's
    // talking head in the sietch. Unlike the palace talking-head stages it
    // draws no room and clears nothing — it overlays the sietch interior left
    // in fb1 by stage 20, which setup_talking_head saves into fb2 as the clean
    // backdrop. al=7 -> CHAN.HSQ; dx=0. (Stage 21 also skips the transition:
    // transition = 0xffff, so the head must reach the screen via the play
    // step, not a fade — see stage_21_play.)
    fn stage_21_init(&mut self) {
        self.setup_talking_head(7, 0);
    }

    // = seg000:078d loc_0078d (stage 21 play). DOS does, before falling into
    // intro_talking_head_play (seg000:0798):
    //
    //   seg000:078d call update_screen_palette  ; flush the CHAN palette to the
    //                                           ; DAC (no transition did it)
    //   seg000:0790 mov  byte ptr [data_0dbe6], 6  ; zoom-step tick delay
    //   seg000:0795 call loc_0c868              ; cinematic zoom-in reveal
    //
    // setup_talking_head loaded CHAN.HSQ's palette into `palette`. Stage 21 has
    // transition = -1, so play_intro's post-transition flush (seg000:05d6) is
    // skipped and `screen_pal` still holds stage 20's sietch palette; this
    // explicit flush makes the CHAN palette the displayed one before
    // scene_zoom_in_reveal starts sending frames. scene_zoom_in_reveal ports the
    // loc_0c868 camera push-in (segvga vga_zoom_screen, the per-scene zoom
    // tables at seg001:27b6/279a/2792, keyed on the talking head id [22a6h]).
    // The tail is the shared intro_talking_head_play: load Chani's intro voice
    // (.voc) and lip-sync it, falling back to the idle animation when no voice
    // file is present.
    fn stage_21_play(&mut self) {
        // = seg000:078d call update_screen_palette.
        self.update_screen_palette();
        self.scene_zoom_in_reveal();
        self.intro_talking_head_play();
    }

    // = seg000:07a3 intro_kynes. Open SUNRS.HSQ (resource 0x2e), draw the
    // desert-sky icon list and one extra sprite, then overlay Dr Kynes' talking
    // head (al=6 -> KYNE.HSQ).
    //
    //   seg000:07a3 open_spritesheet(2eh)        ; SUNRS.HSQ + palette
    //   seg000:07a9 draw_icons_list_at_si(_stru_209B0) ; the sky background
    //   seg000:07af draw_sprite(1, dx=54h, bx=0bh)     ; extra sky element
    //   seg000:07bb copy_active_framebuffer_to_framebuffer_2
    //   seg000:07be al=6; bp=12h; jmp loc_0099d        ; KYNE head, dx=0
    //
    // bp=0x12 selects the first idle pose in loc_09908; the port's
    // setup_talking_head renders a random idle frame 0 instead (the established
    // model — the idle animator re-randomises immediately anyway).
    fn stage_22_init(&mut self) {
        // = seg000:07a3 open_spritesheet(2eh) — SUNRS.HSQ + palette.
        self.open_sprite_bank(SUNRS);
        self.with_active_bank_sheet(|s, sheet| {
            // = seg000:07a9 draw_icons_list_at_si(_stru_209B0) — the sky.
            s.draw_icons_list_at_si(&SKY_ICON_LIST, sheet);
            // = seg000:07af mov ax,1; mov dx,54h; mov bx,0bh.
            let _ = gfx::draw_sprite_on_framebuffer(s, sheet, 1, 0x54, 0x0b);
        });
        // = seg000:07be mov al,6 (KYNE); loc_0099d xor dx,dx.
        self.setup_talking_head(6, 0);
    }

    // = seg000:0798 intro_talking_head_play — Kynes' voice + lip-sync.
    fn stage_22_play(&mut self) {
        self.intro_talking_head_play();
    }

    // = seg000:07c6 intro_somebody_in_sietch. Draw a SIET room with a Fremen
    // standing in it (persons_in_room = 0x100 -> person id 8), then reset
    // persons_in_room and overlay Stilgar's talking head (al=5 -> STIL.HSQ).
    //
    //   seg000:07c6 bx=1080h; dx=802h
    //   seg000:07cc persons_in_room = 100h
    //   seg000:07d2 call loc_00981                  ; open_SAL + draw_SAL + copy
    //   seg000:07d5 persons_in_room = 0             ; ([12h] = _word_1F4C2)
    //   seg000:07db al=5 (STIL); jmp loc_0099d      ; dx=0
    //
    // Unlike intro_inside_sietch this does not clear the framebuffer first — the
    // room's polygons fill the game area.
    fn stage_23_init(&mut self) {
        // = seg000:07cc persons_in_room = 0x100: person 8 stands in the room.
        self.persons_in_room = 0x100;
        self.draw_location_room(0x802, 0x1080);
        // = seg000:07d5 mov word ptr [12h], 0 — reset after the room is drawn.
        self.persons_in_room = 0;
        // = seg000:07db mov al,5 (STIL).
        self.setup_talking_head(5, 0);
    }

    // = seg000:0798 intro_talking_head_play — Stilgar's voice + lip-sync.
    fn stage_23_play(&mut self) {
        self.intro_talking_head_play();
    }

    // = seg000:0868 intro_midnight. The desert sky again, but at the night
    // sub-palette 0x0b, plus one extra sprite; no talking head.
    //
    //   seg000:0868 bl=0bh; call loc_00802          ; night sky sub-palette
    //   seg000:086d ax=7; dx=13h; bx=4ah; draw_sprite
    //
    // No framebuffer clear (the sky icon list paints the scene).
    fn stage_24_init(&mut self) {
        self.load_sky_palette_and_draw_sky_sprite_list(0x0b);
        // = seg000:086d mov ax,7; mov dx,13h; mov bx,4ah — extra sky sprite.
        // loc_00802 dropped the SUNRS sheet, so re-open it for this one sprite.
        let sunrs = self.dat_file.read("SUNRS.HSQ").expect("read SUNRS.HSQ");
        let sheet = SpriteSheet::from_slice(&sunrs).expect("parse SUNRS.HSQ");
        let _ = gfx::draw_sprite_on_framebuffer(self, &sheet, 7, 0x13, 0x4a);
    }

    // = seg000:087b loc_0087b — install the sky palette-cycling frame task
    // (loc_00826) at interval bp=0x10, continuing from sub-palette 0x0b. The
    // 0x0b start cross-fades 0x0b -> 0x0c -> 0x0d, then ends at 0x0e.
    fn stage_24_play(&mut self) {
        self.add_sky_palette_cycler(0x10);
    }

    // = seg000:0886 stage_25_sting_start. Feyd-Rautha (played by Sting in the
    // 1984 film) on the layered red BACK.HSQ background.
    //
    //   seg000:0886 open_spritesheet(30h)          ; BACK.HSQ + palette
    //   seg000:088c vga_fill_rect(game_area_rect, 0deh)  ; red base fill
    //   seg000:0899 draw_icons_list_at_si(154eh)         ; the two side panels
    //   seg000:089f copy_game_area_rect_to_clip_rect     ; clip the next list
    //   seg000:08a2 draw_sprite_list(155ch)              ; the two guards
    //   seg000:08a8 copy_active_framebuffer_to_framebuffer_2
    //   seg000:08ab al=0ah (FEYD); dx=3ah; loc_009c7 + loc_0978e
    fn stage_25_init(&mut self) {
        // = seg000:0886 open_spritesheet(30h) — BACK.HSQ + palette.
        self.open_sprite_bank(BACK);

        // = seg000:088c..0895 vga_fill_rect(_word_20920_game_area_rect, 0deh):
        // the game area is (0,0)..(0x140,0x98).
        gfx::vga_fill_rect(self, 0, 0, 0x140, 0x98, 0xde);

        // = seg001:154e icon list: sprite 4 mirrored at x=0, sprite 4 at x=236.
        const PANELS: [(u16, i16, i16); 2] = [(0x4004, 0, 0), (0x0004, 236, 0)];
        // = seg001:155c sprite list (draw_sprite_list, clipped to the game
        // area): the guards flanking Feyd. The left guard's sprites sit at
        // negative x so they're clipped at the left edge; the right guard is at
        // x=204. (sprite, x, y) triples — same layout as the icon list.
        const GUARDS: [(u16, i16, i16); 6] = [
            (5, -58, 12),
            (6, -58, 12),
            (5, -5, 17),
            (6, -5, 17),
            (7, 204, 14),
            (8, 204, 14),
        ];
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_icons_list_at_si(&PANELS, sheet);
            s.draw_sprite_list_clipped_to_game_area(&GUARDS, sheet);
        });

        // = seg000:08ab mov al,0ah (FEYD); mov dx,3ah; loc_009c7.
        self.setup_talking_head(0x0a, 0x3a);
    }

    // = seg000:0798 intro_talking_head_play — Feyd's voice + lip-sync.
    fn stage_25_play(&mut self) {
        self.intro_talking_head_play();
    }

    // = seg000:09ad stage_26_baron. Baron Harkonnen on the red BACK.HSQ
    // background.
    //
    //   seg000:09ad open_spritesheet(30h)      ; BACK.HSQ + palette
    //   seg000:09b3 draw_icons_list_at_si(153ah)     ; mirrored side panels
    //   seg000:09b9 copy_active_framebuffer_to_framebuffer_2
    //   seg000:09bc al=9 (BARO); dx=52h; loc_009c7 + loc_0978e
    fn stage_26_init(&mut self) {
        // = seg000:09ad open_spritesheet(30h) — BACK.HSQ + palette.
        self.open_sprite_bank(BACK);

        // = seg001:153a icon list: sprite 4 mirrored at x=0, sprite 4 at x=236,
        // sprite 3 at x=83.
        const PANELS: [(u16, i16, i16); 3] = [(0x4004, 0, 0), (0x0004, 236, 0), (0x0003, 83, 0)];
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_icons_list_at_si(&PANELS, sheet);
        });

        // = seg000:09bc mov al,9 (BARO); mov dx,52h.
        self.setup_talking_head(9, 0x52);
    }

    // = seg000:0798 intro_talking_head_play — the Baron's voice + lip-sync.
    fn stage_26_play(&mut self) {
        self.intro_talking_head_play();
    }

    // Stage 27 init = loc_00f66 (no-op). The Baron scene from stage 26 stays in
    // fb1; with no transition (0xffff) it remains on screen for the guard slide.
    fn stage_27_init(&mut self) {}

    // = seg000:08b6 stage_27_baron_guards_play. A guard marches in from the left
    // over the (frozen) Baron scene. DOS:
    //
    //   seg000:08b6 open_spritesheet(30h)             ; BACK.HSQ + palette
    //   seg000:08bc copy_active_framebuffer_to_framebuffer_2 ; save the Baron
    //   seg000:08bf copy_game_area_rect_to_clip_rect         ; clip = game area
    //   seg000:08c2 dx = -96
    //   loc_008c5:                                           ; per frame:
    //     copy_game_area_to_screen_fb2_to_fb1               ;  restore the Baron
    //     draw_sprite_clipped(5, dx, 13); draw_sprite_clipped(6, dx, 13)
    //     loc_0c4dd                                          ;  flush to screen
    //     dx += 32; while dx <= 0                            ;  -96,-64,-32,0
    //
    // Sprites 5+6 are the same left-guard pair BACK.HSQ uses for Feyd; here they
    // slide from x=-96 to x=0. DOS runs the loop at its natural (uncapped) rate;
    // the port paces each step a frame so the slide is visible.
    fn stage_27_play(&mut self) {
        // = seg000:08b6 open_spritesheet(30h) — BACK.HSQ + palette.
        self.open_sprite_bank(BACK);

        // = seg000:08bc copy_active_framebuffer_to_framebuffer_2: snapshot the
        // Baron scene as the clean backdrop the guard composites over.
        self.copy_active_framebuffer_to_framebuffer_2();

        self.with_active_bank_sheet(|s, sheet| {
            // = seg000:08c2 mov dx, 0ffa0h (-96) .. add dx,20h; jle (i.e. -96,
            // -64, -32, 0).
            let mut dx: i16 = -96;
            loop {
                // = copy_game_area_to_screen_fb2_to_fb1: restore the clean Baron.
                s.framebuffer.copy_from(&s.framebuffer_saved);
                // = draw_sprite_clipped(5/6, dx, 13): the guard pair at the
                // current slide x, clipped to the game area.
                s.draw_sprite_list_clipped_to_game_area(&[(5, dx, 13), (6, dx, 13)], sheet);
                // = loc_0c4dd: copy the game area fb1 -> screen and present.
                s.gfx_copy_whole_framebuf_to_screen();
                s.present_transition_frame();

                dx += 0x20;
                if dx > 0 {
                    break;
                }
            }
        });
    }

    fn stage_28_init(&mut self) {
        self.night_attack_start();
    }

    fn stage_28_play(&mut self) {}

    // = seg000:06d8 loc_006d8. Set up MTG3.HNM (a desert dune flyover) with a
    // concurrent sky palette fade: while the clip plays, the live palette's sky
    // span cross-fades toward SKYDN.HSQ sub-palette 0x12 (dusk), turning the day
    // sky and dunes to a sunset. The clip's first frame sets the whole palette
    // once and never updates it again, so the fade owns the faded span outright.
    fn stage_29_init(&mut self) {
        // = seg000:06d8 call pcm_stop_voc.
        self.pcm_player.stop();
        // = seg000:06dd mov [46dfh], 1 — arm the sky-fade task (loc_03916 stops
        // itself if this is cleared).
        self.sky_fade_active = true;
        // = seg000:06e2 loc_038a2 (bl=0x12): load the fade *target*. It opens
        // open_sky_or_skydn_palette resource 0x28 + [22e3h] sub-palette 0x12 and
        // copies its colours into palette_to_transition_from (loc_039b9). It does
        // NOT touch the live palette. [46d7h]=0x30 set here is overwritten by
        // play's 0x3f.
        //
        // play_intro's remove_all_frame_tasks left [22e3h]=1, so this is the
        // SKYDN.HSQ (0x29, dusk) branch: loc_039b9 writes 0x1c5 bytes (151
        // colours) at byte offset 0xdb -> entries 73..223, sourced from the
        // sub-palette's first 151 colours. (With [22e3h]=0 it would be SKY.HSQ
        // and 0xf0 bytes / 80 colours at 128..207.) Using SKY.HSQ/128..207 here
        // is wrong: SKY.HSQ sub 0x12 carries green entries at colours 64..66,
        // which the dune pixels (palette 192..194) then fade to as stray green
        // specks.
        let (resource, dest_start, count) = if self.sky_skydn_selector != 0 {
            ("SKYDN.HSQ", 73, 151)
        } else {
            ("SKY.HSQ", 128, 80)
        };
        self.load_sky_palette_to_fade_target(resource, 0x12, 0, count, dest_start);
        // = seg000:39d2 add dx,cx + loc_039e5: when [227dh]==0, loc_039b9 also
        // writes the next 16 colours into entries 240..255. The intro keeps
        // [227dh]=1, so this is normally skipped; it is modelled for the
        // [227dh]=0 (in-game) case and is invisible in the flyover (no pixels
        // there).
        if self.data_0227d == 0 {
            self.load_sky_palette_to_fade_target(resource, 0x12, count, 16, 240);
        }
        // = seg000:06f3 add [0dbdah],1e0h (fb base += 24 rows = the game area) +
        // hnm_load_first_frame(HNM 0x12 = MTG3.HNM). The first frame sets the live
        // palette; the fade then steps its sky range toward the SKY target.
        self.hnm_load_first_frame("MTG3.HNM", 24);
    }

    // = seg000:06fc loc_006fc. Install both per-frame tasks: the sky-fade task
    // and the HNM player.
    fn stage_29_play(&mut self) {
        // = seg000:06fc mov [46d7h], 3fh — 63 fade steps.
        self.sky_fade_countdown = 0x3f;
        // = seg000:0701 loc_03901: add_frame_task(loc_03916, bp=0x10) — one fade
        // step every 16 ticks.
        self.add_frame_task(0x10, crate::TaskId::SkyFade);
        // = seg000:0704 intro_play_hnm_with_frame_task: the MTG3 frame player.
        self.intro_play_hnm_with_frame_task();
    }

    // = seg000:0704 intro_play_hnm_with_frame_task. Install a per-tick task that
    // decodes one HNM frame and blits it to the screen; play_intro's
    // wait_for_pcm_voice_interruptable(stage.wait) drives it for the clip's
    // duration. Shared by stages 12, 19, 29, 39.
    fn intro_play_hnm_with_frame_task(&mut self) {
        self.add_frame_task(0, crate::TaskId::HnmDoFrame);
    }

    // Story-still stages — each opens its INTxx.HSQ and draws sprite 0.
    fn stage_30_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT05);
    }

    fn stage_30_play(&mut self) {}

    fn stage_31_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT10);
    }

    fn stage_31_play(&mut self) {}

    fn stage_32_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT08);
    }

    fn stage_32_play(&mut self) {}

    fn stage_33_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT04);
    }

    fn stage_33_play(&mut self) {}

    fn stage_34_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT09);
    }

    fn stage_34_play(&mut self) {}

    fn stage_35_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT11);
    }

    fn stage_35_play(&mut self) {}

    fn stage_36_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT02);
    }

    fn stage_36_play(&mut self) {}

    fn stage_37_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT06);
    }

    fn stage_37_play(&mut self) {}

    // = seg000:07e0 intro_water_ripples_in_cave. A still sietch cave (with a
    // water pool). DOS plays PCM sound 4 (play_pcm_al -> PCM resource 0xb2, the
    // water ambience — not ported: the port has no general PCM sound-effect
    // path, cf. SN3/SN8.VOC), then `dx=804h; bx=1080h; jmp loc_00981` draws the
    // room: SIET.SAL room 12 + SIET1.HSQ. Like intro_inside_sietch it skips the
    // persons_in_room write (stays 0 -> empty cave). draw_SAL clears the game
    // area first (clear_game_area), so the water reflection's dithered gaps show
    // black, not the previous stage's leftover image. The stage's play is a
    // no-op; the scene is held for wait=1200.
    fn stage_38_init(&mut self) {
        self.draw_location_room(0x804, 0x1080);
    }

    fn stage_38_play(&mut self) {}

    // = seg000:050d loc_006ea -> set location_and_room=2, hnm_load_first_frame
    // (HNM 0x13 = PLANT.HNM, into the game area).
    fn stage_39_init(&mut self) {
        // Change location away from to stop drip sound
        self.location_and_room = 0x0002;
        self.hnm_load_first_frame("PLANT.HNM", 24);
    }

    fn stage_39_play(&mut self) {
        self.intro_play_hnm_with_frame_task();
    }

    fn stage_40_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT07);
    }

    fn stage_40_play(&mut self) {}

    // = seg000:0711 loc_00711 -> hnm_load_first_frame(HNM 0x0e = VER.HNM, into
    // the game area).
    fn stage_41_init(&mut self) {
        self.hnm_load_first_frame("VER.HNM", 24);
    }

    // = seg000:071d stage_41_play_VER_HNM (loc_0071d). Play VER.HNM to completion
    // in a blocking loop — the stage's wait is 1, so the clip cannot be driven by
    // a frame task; it plays here.
    fn stage_41_play(&mut self) {
        // = seg000:071f mov al,8; call audio_start_voc — start SN8.VOC. The clip
        // loops internally (its VOC repeat blocks) until end_loop is called.
        self.audio_start_voc("SN8.VOC");

        // = seg000:072c loop. DOS decodes via the active framebuffer (loc_00711
        // shifted its segment +0x1e0 = 24 rows); the port decodes into fb1 with
        // y_offset=24, then copies it to the screen below.
        self.set_fb1_as_active_framebuffer();
        loop {
            // = seg000:0731 any_key_pressed; 0734 -> 0736 ret with CF=1 — a
            // keypress ends the whole intro (intro_aborted). Like VIRGIN/CRYO this
            // raw loop does not clear the scancode (only the
            // hnm_do_frame_skippable stages do, c9f1).
            if self.intro_input_pressed() {
                self.intro_aborted = true;
                break;
            }
            // = seg000:072c hnm_do_frame_and_check_if_frame_advanced — decode the
            // next frame.
            self.spin_until_hnm_advances();
            // = seg000:0724 call hnm_blit_frame_to_screen (loc_04b16): copy the
            // decoded fb1 frame to the visible screen. DOS copies fb1 row 0 ->
            // screen row 24 (vga_copy_partial, es = screen_seg + 0x1e0); the port
            // decodes into fb1 rows 24..176 and whole-copies, landing on the same
            // screen rows.
            self.gfx_copy_whole_framebuf_to_screen();
            self.send_frame_to_display();
            // = seg000:0727 call hnm_end_voc_loop_check_complete (loc_04937): end
            // the SN8.VOC loop at frame 0x0b, then break when the clip is done.
            if self.hnm_end_voc_loop_check_complete() {
                break;
            }
        }
    }

    // = seg000:4937 hnm_end_voc_loop_check_complete (loc_04937). When the HNM
    // frame counter ([dbe8] _word_2D098_hnm_frame_counter, mirrored by the
    // decoder's current_frame) has low byte 0x0b, end the currently looping VOC
    // so it stops repeating; then return whether the clip has finished
    // (check_if_hnm_complete).
    fn hnm_end_voc_loop_check_complete(&mut self) -> bool {
        // = seg000:4937 mov ax,[dbe8]; cmp al,0bh.
        let frame = self.hnm_frame_counter;
        if frame & 0xff == 0x0b {
            // = seg000:493e call_pcm_vtable_end_loop -> dnsdb_end_loop: zero the
            // innermost VOC repeat count so SN8.VOC exits at its next repeat-end.
            self.pcm_player.end_loop();
        }
        // = seg000:4941 jmp check_if_hnm_complete.
        self.hnm_is_complete()
    }

    fn stage_42_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT15);
    }

    fn stage_42_play(&mut self) {}

    fn stage_43_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT13);
    }

    fn stage_43_play(&mut self) {}

    fn stage_44_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT14);
    }

    fn stage_44_play(&mut self) {}

    fn stage_46_init(&mut self) {
        self.open_resource_and_draw_sprite0(INT15);
    }

    fn stage_46_play(&mut self) {}

    // Clear-framebuffer stages. = load dw gfx_clear_active_framebuffer; the
    // following transition fades from/to the cleared (black) frame.
    fn stage_45_init(&mut self) {
        self.gfx_clear_active_framebuffer();
    }

    fn stage_45_play(&mut self) {}

    fn stage_47_init(&mut self) {
        self.gfx_clear_active_framebuffer();
    }

    fn stage_47_play(&mut self) {}

    // = seg000:0661 play_CRYO_OR_CRYO2_HNM. Shared body for CRYO.HNM and
    // CRYO2.HNM: spin frames until completion, no per-frame work.
    fn play_cryo_or_cryo2_hnm(&mut self) {
        // = set_fb1_as_active_framebuffer — decode into fb1, then copy to screen.
        self.set_fb1_as_active_framebuffer();
        loop {
            // = seg000:0664 any_key_pressed; 0667 jb -> ret with CF=1 — a keypress
            // ends the whole intro (intro_aborted), not just this clip. This raw
            // loop does not clear the scancode (matching VIRGIN at 0628; only the
            // hnm_do_frame_skippable stages do, c9f1).
            if self.intro_input_pressed() {
                self.intro_aborted = true;
                break;
            }
            self.spin_until_hnm_advances();
            // = seg000:066e gfx_copy_whole_framebuf_to_screen.
            self.gfx_copy_whole_framebuf_to_screen();
            self.send_frame_to_display();
            if self.hnm_is_complete() {
                break;
            }
        }
    }

    // = seg000:06bd intro_play_hnm_skippable. Generic HNM player used by
    // PRESENT.HNM and the post-frame-86 portion of TITLE.HNM.
    fn intro_play_hnm_skippable(&mut self) {
        self.clear_global_y_offset();
        // = seg000:06c0 set_screen_as_active_framebuffer — decode straight onto
        // the visible screen (PRESENT.HNM and the post-frame-86 part of
        // TITLE.HNM). The preceding full fade/instant transition leaves the
        // previous frame on the screen, so the HNM's delta/RLE frames apply
        // correctly with no separate fb1 → screen copy.
        self.set_screen_as_active_framebuffer();
        loop {
            // = seg000:06c3 hnm_do_frame_skippable; jb -> done.
            if self.hnm_do_frame_skippable() {
                break;
            }
            self.send_frame_to_display();
            if self.hnm_is_complete() {
                break;
            }
        }
    }

    // Spin in a tight loop calling hnm_do_frame until one returns true.
    // Each spin sleeps one tick so we don't peg a CPU core. Mirrors the
    // `jz` loop at seg000:0628 / seg000:0664 / seg000:06b0 / seg000:cf30.
    fn spin_until_hnm_advances(&mut self) {
        while !self.hnm_do_frame() {
            let now = self.game_ticks();
            self.sleep_ticks(now, 1);
        }
    }

    // = seg000:c9e8 hnm_do_frame_skippable. Advance the HNM by one frame (waiting
    // the per-clip tick interval via the spin idiom above) and report whether the
    // user asked to skip. DOS calls hnm_do_frame once then any_key_pressed,
    // returning CF set on a keypress and clearing the scancode (c9f1 jmp
    // kb_clear_scancode) so the press is consumed; the port polls every spin
    // iteration so a skip stays responsive while waiting between frames. Returns
    // true when a skip was requested.
    fn hnm_do_frame_skippable(&mut self) -> bool {
        loop {
            let advanced = self.hnm_do_frame();
            // = seg000:c9eb any_key_pressed; c9f1 kb_clear_scancode on a hit.
            // The skip ends the whole intro (= the CF that propagates to
            // play_intro's 05ef jb loc_005fd), not just this clip.
            if self.intro_input_pressed() {
                self.kb_clear_scancode();
                self.intro_aborted = true;
                return true;
            }
            if advanced {
                return false;
            }
            let now = self.game_ticks();
            self.sleep_ticks(now, 1);
        }
    }

    // Poll for input that interrupts the intro. Returns true on any key/mouse
    // press. ESC additionally latches intro_skip_to_game (kb_esc_was_hit, set by
    // any_key_pressed's kb_check_for_esc_key_hit) so the whole sequence skips
    // into the game; a non-ESC key or the mouse only ends the current phase.
    fn intro_input_pressed(&mut self) -> bool {
        if self.any_key_pressed() {
            // = seg000:de54 kb_check_for_esc_key_hit (run inside any_key_pressed):
            // ESC is scancode 1, which sets kb_esc_was_hit.
            if self.input.lock().unwrap().kb_esc_was_hit != 0 {
                self.intro_skip_to_game = true;
            }
            true
        } else {
            false
        }
    }

    // = seg000:0802 loc_00802 — load sky sub-palette and draw the sky-scene icon list.
    fn load_sky_palette_and_draw_sky_sprite_list(&mut self, sub_palette_index: usize) {
        self.open_sunrs_palette(sub_palette_index);

        let sunrs = self.dat_file.read("SUNRS.HSQ").unwrap();
        let sprite_sheet = SpriteSheet::from_slice(&sunrs).unwrap();

        self.draw_icons_list_at_si(&SKY_ICON_LIST, &sprite_sheet);
    }

    // = seg000:0820 open_sunrs_palette
    fn open_sunrs_palette(&mut self, sub_index: usize) {
        // = seg000:0847 open_sunrs_palette: vga_set_palette(cx=0xf0, bx=0x180) —
        // hardcoded 80 colours @ 128. (The SUNRS cycler is independent of the
        // sky_skydn_selector branch loc_0398c uses for SKY/SKYDN.)
        self.open_sky_palette("SUNRS.HSQ", sub_index, 0, 80, 128);
    }

    // = seg000:3978 open_sky_palette_al_sub_bl_dsdx + vga_set_palette: open
    // `resource` and write `count` colours starting at the sub-palette's colour
    // `color_start` into the LIVE palette at entries `dest_start..`. Mirrors
    // `load_sky_palette_to_fade_target`, but for the live palette (the DOS
    // vga_set_palette path) rather than palette_fade_target. SUNRS hardcodes
    // 80@128 (= seg000:0820 open_sunrs_palette); SKY/SKYDN (set_sky_palette ->
    // loc_0398c) picks the range from sky_skydn_selector — see the callers in
    // intro2_draw_sky and arm_sky_palette_fade.
    pub(crate) fn open_sky_palette(
        &mut self,
        resource: &str,
        sub_index: usize,
        color_start: usize,
        count: usize,
        dest_start: usize,
    ) {
        let data = self.dat_file.read(resource).unwrap();
        // = seg000:3982 mov [46d6h], al — loading a sub-palette records it as
        // the current sky sub-palette; this is what advances the cycler.
        self.current_sky_palette = sub_index as u8;
        let mut c = Cursor::new(&data[..]);
        let toc_pos = c.read_le_u16().unwrap() as u64;
        c.set_position(toc_pos + (sub_index as u64) * 2);
        let sub_ofs = c.read_le_u16().unwrap() as u64;
        c.set_position(toc_pos + sub_ofs + 6 + (color_start as u64) * 3);
        for i in 0..count {
            let r = c.read_u8().unwrap();
            let g = c.read_u8().unwrap();
            let b = c.read_u8().unwrap();
            self.palette.set(dest_start + i, Color(r, g, b));
        }
    }

    // = seg000:c21b draw_icons_list_at_si. Each icon is drawn into the
    // active framebuffer at logical (x, y); the gfx-layer wrapper adds
    // `state.y_offset` to the destination y the same way the DOS segvga
    // primitives auto-apply `fb_base_ofs`. The sprite word's high bits are
    // mirror flags (0x4000 = flip-x, 0x2000 = flip-y), used by the red-
    // background panels (BACK.HSQ) to mirror one sprite into a symmetric frame.
    pub(crate) fn draw_icons_list_at_si(&mut self, list: &[(u16, i16, i16)], sheet: &SpriteSheet) {
        for &(idx, x, y) in list {
            let flip_x = idx & 0x4000 != 0;
            let flip_y = idx & 0x2000 != 0;
            let _ = gfx::draw_sprite_on_framebuffer_flipped(
                self,
                sheet,
                idx & 0x1ff,
                x,
                y,
                flip_x,
                flip_y,
            );
        }
    }

    // = seg000:0847 open_sunrs_palette + seg000:0850
    // j_vga_set_fade_target_data(cx=0xf0, bx=0x180).
    // Reads SUNRS.HSQ sub-palette `sub_index` and writes the 80 colours
    // (entries 128..207) into palette_to_transition_from — the fade-target
    // buffer that vga_fade_step steps the live palette toward. loc_00841 always
    // hardcodes this 128..207 span (it does not consult [22e3h]); the cycler's
    // fade step (loc_0391d) does branch on [22e3h], but the extra entries fade
    // toward the stage's just-saved palette, so they stay put.
    fn load_sunrs_palette_to_fade_target(&mut self, sub_index: usize) {
        self.load_sky_palette_to_fade_target("SUNRS.HSQ", sub_index, 0, 80, 128);
    }

    // = seg000:3978 open_sky_palette_al_sub_bl_dsdx + j_vga_set_fade_target_data:
    // read `resource`'s sub-palette `sub_index` (a 6-byte header + RGB triples)
    // and write `count` colours, starting at the sub-palette's colour
    // `color_start`, into palette_to_transition_from entries dest_start.. — the
    // fade target. SUNRS.HSQ (sky cycler, loc_00841) hardcodes colours 0..80 ->
    // 128..207; SKY/SKYDN.HSQ (stage 29, loc_039b9) write colours 0..count of
    // the [22e3h] span (80 -> 128..207, or 151 -> 73..223) plus, when [227dh]==0,
    // the next 16 colours -> 240..255. All share the layout and the +6
    // sub-resource header skip; loc_039b9's `add dx,cx` is `color_start`.
    pub(crate) fn load_sky_palette_to_fade_target(
        &mut self,
        resource: &str,
        sub_index: usize,
        color_start: usize,
        count: usize,
        dest_start: usize,
    ) {
        let data = self.dat_file.read(resource).unwrap();
        // = seg000:3982 mov [46d6h], al — the loaded sub-palette becomes the
        // current one (how the cycler advances [46d6h]; loc_00826/loc_03916
        // never write it directly).
        self.current_sky_palette = sub_index as u8;
        let mut c = Cursor::new(&data[..]);
        let toc_pos = c.read_le_u16().unwrap() as u64;
        c.set_position(toc_pos + (sub_index as u64) * 2);
        let sub_ofs = c.read_le_u16().unwrap() as u64;
        c.set_position(toc_pos + sub_ofs + 6 + (color_start as u64) * 3);
        for i in 0..count {
            let r = c.read_u8().unwrap();
            let g = c.read_u8().unwrap();
            let b = c.read_u8().unwrap();
            self.palette_fade_target.set(dest_start + i, Color(r, g, b));
        }
    }
}
