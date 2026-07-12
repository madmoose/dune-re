//! = seg000:021c play_intro2 — the WORMSUIT second-intro act.
//!
//! The third startup step (start calls it after play_intro / play_CREDITS_HNM):
//! eight narrated "WORMSUIT" cutscenes over the WORMSUIT MIDI score, each with a
//! voice clip and a subtitle line, followed by a night->day sky fade, ending by
//! setting the game up at the palace throne room.
//!
//! Scene draws are dispatched from the `script_2` table (seg000:020c) by the
//! per-scene render callback loc_002c1. This first pass ports the loop control
//! flow + the game-setup tail; the individual scene draws and the sky fade are
//! documented stubs filled in by later stages.

use crate::{
    FbId, GameState, Rect, TextSize, blit, gfx,
    settings_ui::{
        SETTINGS_RECORD_BALANCE_MUSIC, SETTINGS_RECORD_BALANCE_MUSIC_DURING_VOICES,
        SETTINGS_RECORD_VOLUME_MUSIC, SETTINGS_RECORD_VOLUME_MUSIC_DURING_VOICES,
    },
    sprite_bank,
};

impl GameState {
    // = seg000:021c play_intro2. DOS skips the cutscenes when entered with ZF set
    // (the intro aborted with ESC); the port passes `skip` (= start's skip_intro),
    // and when set only the game-setup tail runs.
    pub fn play_intro2(&mut self, skip: bool) {
        // = seg000:021c data_0289e = 0x8c (the music-ducking level
        // midi_duck_music_volume reads; no dedicated port field yet).
        self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC_DURING_VOICES].value = 0x8c;

        // = seg000:0221 voice_subtitle_mode = 1 — narration subtitles on.
        self.voice_subtitle_mode = 1;

        // = seg000:0226 jz loc_00292 — skip the cutscene act on the abort path.
        if !skip {
            self.play_intro2_cutscenes();
        }

        // = seg000:0292 loc_00292 — game setup after intro2.
        self.play_intro2_game_setup();
    }

    // = seg000:0228..028f the WORMSUIT cutscene loop and the night->day sky fade.
    fn play_intro2_cutscenes(&mut self) {
        // = seg000:0228 play_music_WORMSUIT_HSQ (midi_reset + play song 3).
        self.midi.play_music_wormsuit_hsq(&mut self.dat_file);

        // // = seg000:022e si = 1 .. seg000:0262 cmp si,8 / jbe — eight scenes.
        for scene in 1..9 {
            // = seg000:0232 bp = loc_002c1; seg000:0235 copy_pal_and_transition —
            // render scene `si` offscreen, then fade the visible screen to it.
            self.intro2_render_and_transition_to_scene(scene);
            // = seg000:0238 midi_duck_music_volume — drop the MIDI score to its
            // narration "duck" level for the voice line.
            self.midi_duck_music_volume();
            // = seg000:023d start_narration_voice_clip — open + queue the next
            // PZ\PZ00<si>I.VOC narration clip.
            self.intro2_start_narration_voice(scene);

            // = seg000:0241 kb_check_for_esc_key_hit; seg000:0244 jz loc_00292 —
            // abort the whole act if ESC was pressed during the transition/voice.
            self.kb_check_for_esc_key_hit();
            if self.input.lock().unwrap().kb_esc_was_hit != 0 {
                return;
            }

            // = seg000:0247/024a ax = 0xfa0 (4000); seg000:024d wait_interruptable —
            // hold the scene, breaking early on ANY user input. A non-ESC key just
            // ends the wait, so the loop advances to the next scene (skip scene to
            // scene); the returned flag is set only when the break was ESC.
            let esc_pressed = self.wait_interruptable(0xfa0);
            // = seg000:0250 pushf — preserve the wait's ESC flag (DOS ZF) across cleanup.
            // = seg000:0251 remove_all_frame_tasks; seg000:0254 pcm_stop_voc.
            self.remove_all_frame_tasks();
            self.pcm_player.stop();

            // = seg000:0257 midi_restore_music_volume — ramp the score back to
            // its normal level after the clip finishes.
            self.midi_restore_music_volume();
            // = seg000:025a popf; seg000:025c jz loc_00292 — abort the whole act
            // only on ESC; a non-ESC skip falls through to inc si for the next scene.
            if esc_pressed {
                return;
            }
            // = seg000:025e inc si / seg000:025f cmp si,8 / seg000:0262 jbe loop.
        }

        self.remove_all_frame_tasks();

        // = seg000:0264..028f the night->day sky fade once all scenes have played.
        self.intro2_night_to_day_sky_fade();
    }

    // = seg000:c102 copy_pal_and_transition (bp = loc_002c1). Snapshot the visible
    // palette as the fade-from target, render scene `scene` into fb1 offscreen, then
    // fade OLD->black->NEW (transition type 0x3a). Same idiom as play_credits, but
    // the bp callback dispatches on the scene index so the front-buffer redirect
    // (= seg000:c097 gfx_call_bp_with_front_buffer_as_screen) is inlined here.
    fn intro2_render_and_transition_to_scene(&mut self, scene: u16) {
        // = seg000:c102 j_vga_save_palette_to_fade_target.
        self.palette_fade_target = self.palette.clone();

        // = seg000:c10f gfx_call_bp_with_front_buffer_as_screen(loc_002c1): make
        // fb1 the active target AND the front buffer so the scene's draws land in
        // fb1, leaving the visible screen for the fade to reveal.
        self.set_fb1_as_active_framebuffer();
        let saved_front = self.screen_buffer;
        self.screen_buffer = FbId::Fb1;
        self.intro2_render_scene(scene);
        self.screen_buffer = saved_front;

        // = seg000:c106 al = 0x3a; fall into transition (seg000:c108) — reveal fb1.
        gfx::vga_transition(self, 0x3a, 0);
        // = seg000:c12a/c12d gfx_copy_whole_framebuf_to_screen + palette flush.
        self.gfx_copy_whole_framebuf_to_screen();
        self.update_screen_palette();
    }

    // = seg000:020c script_2 — the per-scene draw-function table. DOS dispatches
    // it as `mov bp, cs:[si*2 + 0x20a]; call bp` (seg000:02c7), so SCENE_DRAWS[si - 1]
    // is the draw function for scene si (1..8). intro_paul_on_red_background and
    // intro_26_baron appear via the j_* jump trampolines at seg000:02f8 / 02fb.
    const SCENES: [fn(&mut GameState); 8] = [
        GameState::intro2_scene_stars, // si=1: intro2_scene_stars -> draw_stars(0)
        GameState::intro2_scene_globe, // si=2: intro2_scene_globe (draw_stars(0x20) + globe)
        GameState::intro2_scene_sky,   // si=3: intro2_scene_sky (sky + INTDS sprite)
        GameState::intro2_scene_paul,  // si=4: intro2_scene_paul -> intro_paul_on_red_background
        GameState::intro2_scene_baron, // si=5: intro2_scene_baron -> intro_26_baron
        GameState::intro2_scene_globe, // si=6: intro2_scene_globe (recap of si=2)
        GameState::intro2_scene_paul,  // si=7: intro2_scene_paul (recap of si=4)
        GameState::intro2_scene_back,  // si=8: intro2_scene_back -> loc_0076a
    ];

    #[doc(hidden)]
    pub fn intro2_render_scene_for_test(&mut self, scene: u16) {
        self.intro2_render_scene(scene);
    }

    #[doc(hidden)]
    pub fn open_sky_palette_pub(&mut self, name: &str, sub: usize) {
        // Test helper: load the SKY layout (80 colours @ 128) the way the
        // SUNRS cycler does. Callers that need the SKYDN 151@73 layout should
        // go through intro2_draw_sky_pub instead.
        self.open_sky_palette(name, sub, 0, 80, 128);
    }

    #[doc(hidden)]
    pub fn intro2_draw_sky_pub(&mut self) {
        self.draw_sky();
    }

    // = seg000:02c1 loc_002c1 — the per-scene render callback. Clear the active
    // framebuffer, draw the scene from the script_2 table (seg000:020c) selected
    // by `scene`, then request the matching narration subtitle (string si + 0x117).
    fn intro2_render_scene(&mut self, scene: u16) {
        // = seg000:02c2 gfx_clear_active_framebuffer.
        self.gfx_clear_active_framebuffer();
        // = seg000:02c7 mov bp, cs:[si*2 + 0x20a]; call bp — dispatch from script_2.
        Self::SCENES[(scene - 1) as usize](self);
        // = seg000:02cf add ax,0x117; seg000:02d2 font_select_tall_font.
        self.font_select_tall_font();
        // = seg000:02d5 loc_09901 — clears data_0479e (the "subtitle changed"
        // flag the room-screen path uses to repaint the bubble). Port has no
        // equivalent yet because the room-screen subtitle restore path isn't
        // wired up; intro2 redraws the scene every frame anyway.

        // self.loc_09901();

        // = seg000:02d8 loc_088af — request the narration subtitle
        self.draw_subtitle(scene + 0x117);

        // = seg000:02db loc_09901 (same as above).
        // self.loc_09901();
    }

    // = seg000:88af loc_088af — request voice subtitle `id` (= si + 0x117 here)
    // and, when voice_subtitle_mode < 2, lay it out and render it. DOS routes
    // through loc_088f1 (phrase-token expansion → 0xa840), then format_
    // interpolated_string (0xa840 → 0xa6b0), then loc_08b11 → loc_08c8a /
    // loc_08ccd / draw_speech_bubble / loc_08e16 / per-line render. The
    // WORMSUIT narration strings have neither phrase tokens nor %s placeholders
    // (no `data_046eb & 0x40` quick path either, since data_046eb == 0 here),
    // so the port collapses the expand/format steps into the COMMAND.BIN
    // lookup and runs the layout directly on the resulting bytes.
    //
    // Layout matches the suppress_sky_240_255 branch of loc_08ccd
    // (seg000:8d43): font_draw_fg_color = 6, padding zeroed, layout rect from
    // seg001:2275 = (x=0, y=153, w=320, h=47), data_04799 = 9 (vertical
    // centre, horizontal centre per line, interword padding 6 px). DOS's
    // draw_speech_bubble does not paint a bubble background on this path
    // (seg000:8f8d: suppress_sky != 0 → ret at loc_08fd0), so we skip it too.
    fn draw_subtitle(&mut self, id: u16) {
        // = seg000:88af or ax,ax / jz — bail on id 0.
        if id == 0 {
            return;
        }
        // = seg000:88ca get_phrase_or_command_string_si — COMMAND.BIN lookup.
        // The 0xff terminator is excluded from the returned slice.
        let text = self.get_phrase_or_command_string(id).to_vec();
        // = seg000:88e1 lodsb / js loc_088f0 — bail when the first byte has
        // its high bit set (a pure phrase-token entry with nothing to render).
        if text.is_empty() || text[0] & 0x80 != 0 {
            return;
        }

        // = seg000:8ccd byte data_04799 = 9 (vertical centre + horizontal
        // centre per line); = seg000:8d4a font_draw_fg_color = 6 (the
        // suppress_sky branch). Tall font is already selected by the caller
        // (= seg000:02d2 font_select_tall_font).
        self.font_state.color = 6;

        // = seg000:8e16 loc_08e16 — word-wrap into <= w pixel lines.
        let lines = intro2_wrap_subtitle(&self.font, &text, 320);
        if lines.is_empty() {
            return;
        }

        // = seg000:8b66..8b80 vertical centring inside the rect: total text
        // height = lines * 10 (tall font line height); pad_y = (rect_h -
        // total_h) / 2, clamped to 0 when the text overflows.
        const RECT_Y: u16 = 153;
        const RECT_H: u16 = 47;
        const RECT_W: u16 = 320;
        const LINE_H: u16 = 0x0a;
        let total_h = lines.len() as u16 * LINE_H;
        let pad_y = if total_h <= RECT_H {
            (RECT_H - total_h) / 2
        } else {
            0
        };
        let mut y = RECT_Y + pad_y;

        // = seg000:8b91..8c67 per-line render loop: position the pen at the
        // line's centred x, then walk the bytes through the selected glyph
        // func. The DOS loop also handles colour-change opcodes (0x01) and a
        // colour-swap (0x06) we don't see in the WORMSUIT narration entries.
        for line in lines {
            let w = measure_line(&self.font, &line);
            let x = if w <= RECT_W { (RECT_W - w) / 2 } else { 0 };
            self.font_set_draw_position(x, y);
            for &c in &line {
                // = seg000:d1c5 high-bit bytes render as 0x40 (the @ glyph).
                let g = if c & 0x80 != 0 { 0x40 } else { c };
                self.font_draw_glyph(g);
            }
            y += LINE_H;
        }
    }

    // = seg000:02de intro2_scene_stars — scene 1: the plain starfield (cx = 0).
    fn intro2_scene_stars(&mut self) {
        // = seg000:02de xor cx,cx; jmp draw_stars.
        self.draw_stars(0);
    }

    // = seg000:02e3 intro2_scene_globe — scenes 2 & 6: a scrolled starfield
    // (cx = 0x20) behind the rotating globe with atmosphere, plus a STARS.HSQ
    // overlay (= the 0x3a transition's palette target).
    fn intro2_scene_globe(&mut self) {
        // = seg000:02e6 mov cx,0x20; call draw_stars — the parallax-panned stars.
        self.draw_stars(0x20);
        // = seg000:02e9 setup_globe_draw; 02ec draw_globe_with_atmosphere — globe
        // rendering deliberately omitted; the scene shows the starfield backdrop
        // only.
        // = seg000:02ef ax=0x2c; open_spritesheet — re-applies STARS.HSQ
        // (and its palette), so the 0x3a transition fades to it.
        self.open_sprite_bank(sprite_bank::STARS);
        // = seg000:02f5 jmp loc_0b8ea — a no-op tail (just returns).
    }

    // = seg000:094a intro2_scene_sky — scene 3: the desert sky behind a low
    // INTDS.HSQ sprite (the desert/wormsuit horizon strip).
    fn intro2_scene_sky(&mut self) {
        // = seg000:094a draw_sky.
        self.draw_sky();
        // = seg000:094d ax=0x2d (INTDS); open_spritesheet — also applies
        // INTDS.HSQ's palette so the transition fades to it.
        self.open_sprite_bank(sprite_bank::INTDS);
        // = seg000:0953 ax=0; dx=0; bx=0x3c; jmp draw_sprite_clobbering_bx_dx —
        // sprite 0 at (X=0, Y=0x3c). DOS register convention: dx=X, bx=Y.
        let y = 0x3c + self.y_offset as i16;
        self.with_active_bank_sheet(|s, sheet| {
            if let Some(sprite) = sheet.get_sprite(0) {
                let fb = s.active_fb_mut();
                let _ = blit::Blitter::new(sprite.data(), fb)
                    .at(0, y)
                    .size(sprite.width(), sprite.height())
                    .pal_offset(sprite.pal_offset())
                    .rle(sprite.rle())
                    .draw();
            }
        });
    }

    // = seg000:07ee intro_paul_on_red_background — scenes 4 & 7: Paul's talking
    // head on the layered red BACK.HSQ panels. Same setup as stage_17_init in
    // intro.rs (= the same DOS routine); intro2 reaches it via the
    // intro2_scene_paul trampoline at seg000:02f8.
    fn intro2_scene_paul(&mut self) {
        // = seg000:07ee ax=0x30 (BACK); open_spritesheet + palette.
        self.open_sprite_bank(sprite_bank::BACK);
        // = seg001:1526 icon list: full red backdrop + two inner vignette panels.
        const PANELS: [(u16, i16, i16); 3] = [(0, 0, 0), (1, 52, 25), (2, 108, 51)];
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_icons_list_at_si(&PANELS, sheet);
        });
        // = seg000:0960 copy_active_framebuffer_to_framebuffer_2 (inside
        // setup_talking_head); seg000:0965 al=0x2d (PAUL); dx=0; loc_009c7.
        self.setup_talking_head(0x2d, 0);
        // = seg000:096f jmp start_room_lip_sync — installs the lip-sync frame
        // task that animates the head over the narration voice. Not invoked here
        // because the port's start_room_lip_sync is still a stub, so the head
        // stays as a single rendered frame for now.
    }

    // = seg000:09ad intro_26_baron — scene 5: the Baron on the red BACK.HSQ
    // background. Same setup as stage_26_init in intro.rs.
    fn intro2_scene_baron(&mut self) {
        // = seg000:09ad ax=0x30 (BACK); open_spritesheet + palette.
        self.open_sprite_bank(sprite_bank::BACK);
        // = seg001:153a icon list: mirrored side panels + a centre overlay.
        const PANELS: [(u16, i16, i16); 3] = [(0x4004, 0, 0), (0x0004, 236, 0), (0x0003, 83, 0)];
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_icons_list_at_si(&PANELS, sheet);
        });
        // = seg000:09b9 copy_active_framebuffer_to_framebuffer_2 (inside
        // setup_talking_head); seg000:09bc al=9 (BARO); dx=0x52; loc_009c7.
        self.setup_talking_head(9, 0x52);
        // = seg000:09c4 jmp start_room_lip_sync — same caveat as scene_paul.
    }

    // = seg000:076a loc_0076a — scene 8: full-screen INT15.HSQ image.
    // Reached via the intro2_scene_back -> loc_00739 -> loc_0c2f2 trampoline
    // (seg000:02fe).
    fn intro2_scene_back(&mut self) {
        // = seg000:076a gfx_clear_active_framebuffer (redundant after
        // intro2_render_scene's clear, but faithful to the DOS sequence).
        self.gfx_clear_active_framebuffer();
        // = seg000:076d al=0x61; jmp loc_00739 -> loc_0c2f2 -> open_resource_by_
        // index(ax=0x0061 INT15); draw_sprite(ax=0, dx=0, bx=0) — sprite 0 at the
        // origin. INT15.HSQ's palette comes along via open_spritesheet.
        self.open_resource_and_draw_sprite0(sprite_bank::INT15);
    }

    // = seg000:02e0 draw_stars -> seg000:0a44 loc_00a44. Open STARS.HSQ, clip to
    // the game area, clear it, then tile three full-width starfield sprites
    // (0,1,2) horizontally from a `count`-derived scroll offset, and overlay the
    // three moon sprites (loc_0c343, centered). `count` (DOS cx) is 0 for the
    // plain starfield and 0x20 for the globe scenes' parallax pan. DOS register
    // convention here: dx = X, bx = Y (see seg000:d230).
    fn draw_stars(&mut self, count: u16) {
        const BG_SPRITE_WIDTH: i16 = 304;
        // = seg000:0a44 ax=0x2c; open_spritesheet — STARS.HSQ. This also
        // applies STARS.HSQ's embedded palette (= seg000:c172 apply_sprite_sheet_
        // palette), which the following 0x3a transition fades to.
        self.open_sprite_bank(sprite_bank::STARS);

        // = seg000:0a4a copy_game_area_rect_to_clip_rect — clip to the game-area
        // rect {0,0,0x140,0x98} offset by the framebuffer base row.
        let y0 = self.y_offset as i16;
        let clip = Rect {
            x0: 0,
            y0,
            x1: 320,
            y1: y0 + 152,
        };

        // = seg000:0a4d set_fb1_as_active_framebuffer (already active here).
        self.set_fb1_as_active_framebuffer();
        // = seg000:0a51 clear_game_area.
        self.clear_game_area();

        // = seg000:0a56 mul al; shr ax,1; neg dx — scroll = -((count&0xff)^2 / 2).
        let sq = (count as i16) * (count as i16);
        let scroll = -(sq / 2);

        // = seg000:0a5f..0aca draw the starfield + moons from the active bank.
        self.with_active_bank_sheet(|s, sheet| {
            // = seg000:0a5f..0a77 the three tiled starfield sprites (0,1,2),
            // top-left at (x = scroll + i*0x130, y = 0), stepping x by 304.
            for i in 0..3 {
                s.draw_sprite_from_sheet_clipped(
                    sheet,
                    i as u16,
                    BG_SPRITE_WIDTH * i + scroll,
                    0,
                    clip,
                );
            }

            // = seg000:0a7a..0a8a moon 1: sprite 0x24 centered at (4*scroll+0x45, 0x4e).
            s.draw_sprite_centered_clipped(sheet, 0x24, 4 * scroll + 69, 78, clip);

            // = seg000:0a92..0aaf moon 2: sprite count/4 + 0x25 at y = 0x67; x
            // clamps its parallax once count exceeds 0x14 (= seg000:0a96 cmp/ja).
            let x = if count > 20 {
                scroll * 2 + 994
            } else {
                (count as i16) * 4 + 242
            };
            s.draw_sprite_centered_clipped(sheet, 0x25 + (count / 4), x, 103, clip);

            // = seg000:0ab4..0ac7 moon 3: sprite count + 3 centered at
            // (x = (count^2 / 32) + 0x80, y = 0x4f).
            let x = sq / 32 + 0x80;
            s.draw_sprite_centered_clipped(sheet, count + 3, x, 79, clip);
        });

        // = seg000:0aca jmp loc_0c4dd — restore cursor + update the game-area
        // screen rect. Here the scene composes offscreen (front buffer = fb1) and
        // is revealed by the following 0x3a transition, so that copy is superseded.
    }

    // = seg000:0264..028f the night->day sky fade after the cutscenes.
    // Reveal the XPLAIN9 night sky via a dotted-columns transition, hold it for
    // 0xc8 ticks, then arm a sky-palette cross-fade from the night palette
    // toward SKY/SKYDN sub-palette 0xc (the day palette) over 0x40 fade steps
    // driven by the loc_03916 frame task while wait_interruptable(0x4b0) runs.
    // Finally a second dotted-columns transition dissolves the lit sky away.
    fn intro2_night_to_day_sky_fade(&mut self) {
        // = seg000:0264 bp = draw_xplain9_night_sky_frame; al = 0x10;
        // seg000:0269 call transition. The transition's bp-callback idiom
        // (gfx_call_bp_with_front_buffer_as_screen) redirects the front buffer
        // to fb1 so the callback's draws land in fb1; vga_transition(0x10) then
        // dissolves the visible screen and reveals fb1 in the new palette.
        self.intro2_run_transition_with_callback(0x10, Self::draw_xplain9_night_sky_frame);

        // = seg000:026c wait_interruptable(0xc8) — hold the night scene.
        self.wait_interruptable(0xc8);

        // = seg000:0272 bl = 0x0c; seg000:0274 call loc_038f1 — arm the sky
        // cross-fade: load SKY/SKYDN sub-palette 0xc into palette_fade_target
        // (the day target), set sky_fade_countdown = 0x40, and install the
        // loc_03916 frame task. The fade step in tick_sky_fade lerps the live
        // (XPLAIN9 night) palette toward palette_fade_target one step per tick.
        self.arm_sky_palette_fade(0x0c);
        // = seg000:0277 sky_fade_active = 1 — armed by the caller, not loc_038f1.
        self.sky_fade_active = true;

        // = seg000:027c wait_interruptable(0x4b0) — drive the sky-fade task for
        // 0x40 steps × 0x10 ticks = 0x400 ticks, plus a tail hold.
        self.wait_interruptable(0x4b0);

        // = seg000:0282 call loc_03950 — disarm: countdown = 0,
        // remove_frame_task(loc_03916). seg000:0285 sky_fade_active = 0.
        self.sky_fade_countdown = 0;
        self.remove_frame_task(crate::TaskId::SkyFade);
        self.sky_fade_active = false;

        // = seg000:028a bp = gfx_clear_active_framebuffer (0xc0ad); al = 0x10;
        // seg000:028f call transition. Dissolve the now-daylit sky to black for
        // the post-intro2 game-setup tail.
        self.intro2_run_transition_with_callback(0x10, Self::gfx_clear_active_framebuffer);
    }

    // = seg000:c108 transition driver. Redirect the front buffer to fb1, invoke
    // the bp callback (which renders the destination frame into fb1), then run
    // vga_transition(`code`) to dissolve from the current screen to fb1. Mirrors
    // the gfx_call_bp_with_front_buffer_as_screen idiom in
    // intro2_render_and_transition_to_scene, but factored out so the night-sky
    // fade can reuse it with a non-scene callback.
    fn intro2_run_transition_with_callback(&mut self, code: u16, cb: fn(&mut GameState)) {
        self.set_fb1_as_active_framebuffer();
        let saved_front = self.screen_buffer;
        self.screen_buffer = FbId::Fb1;
        cb(self);
        self.screen_buffer = saved_front;
        gfx::vga_transition(self, code, 0);
        self.gfx_copy_whole_framebuf_to_screen();
        self.update_screen_palette();
    }

    // = seg000:0301 draw_xplain9_night_sky_frame (renamed from loc_00301) — the
    // bp callback for the night-sky reveal transition. Clears the active
    // framebuffer, then `mov al,0x1b; jmp loc_0c2f2` opens XPLAIN9.HSQ (the
    // night-sky still, applying its palette) and blits sprite 0 at (0, 0).
    fn draw_xplain9_night_sky_frame(&mut self) {
        // = seg000:0301 call gfx_clear_active_framebuffer.
        self.gfx_clear_active_framebuffer();
        // = seg000:0304 mov al,0x1b; seg000:0306 jmp loc_0c2f2 — open
        // XPLAIN9.HSQ (also applies its night palette) and draw sprite 0
        // at (dx=0, bx=0).
        self.open_resource_and_draw_sprite0(sprite_bank::XPLAIN9);
    }

    // = seg000:ade0 midi_duck_music_volume — MIDI SetDynamics(ax=0x64,
    // bl=data_0289e clamped to >=4, bh=data_028b6): drop the music to its
    // narration "duck" volume over 0x64 ticks before a voice line. data_0289e
    // is the "music during voices" mixer slider (settings_records[2]) and
    // data_028b6 its paired record (settings_records[5]).
    pub(crate) fn midi_duck_music_volume(&mut self) {
        let volume = self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC_DURING_VOICES]
            .value
            .max(4); // = loc_0adf8 clamp bl >= 4
        let balance = self.settings_records[SETTINGS_RECORD_BALANCE_MUSIC_DURING_VOICES].value;
        self.midi.set_ducking(100, volume, balance);
    }

    // = seg000:aded midi_restore_music_volume — MIDI SetDynamics(ax=0x190,
    // bl=data_02896 clamped to >=4, bh=data_028ae): ramp the music back up over
    // 0x190 ticks (longer than the duck, so it swells gently) once the voice
    // clip ends. data_02896 is the MUSIC slider (settings_records[1]) and
    // data_028ae its paired record (settings_records[4]).
    pub(crate) fn midi_restore_music_volume(&mut self) {
        let volume = self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC]
            .value
            .max(4); // = loc_0adf8 clamp bl >= 4
        let balance = self.settings_records[SETTINGS_RECORD_BALANCE_MUSIC].value;
        self.midi.set_ducking(0x190, volume, balance);
    }

    // = seg000:ab92 frame_task_callback_0ab92 — the per-frame monitor a ducked
    // voice clip installs (interval 1). Once PCM playback ends, ramp the music
    // back to its un-ducked level and remove this task. DOS also pumped the
    // streaming-VOC refill here (loc_0a9b9) and ran the same body inline from the
    // test-voice wait loop (loc_0aba9); the dnsdb driver owns the whole clip in
    // the port, so the monitor only needs the music-restore half.
    pub(crate) fn tick_pcm_voice_music_restore(&mut self) {
        // = ab95 check_pcm_voice_file_open; jnz loc_0ab44 — still playing, wait.
        if self.pcm_player.is_playing() {
            return;
        }
        // = ab9a midi_restore_music_volume; ab9d remove_frame_task.
        self.midi_restore_music_volume();
        self.remove_frame_task(crate::TaskId::PcmVoiceMusicRestore);
    }

    // = seg000:ab4f start_narration_voice_clip — open and start the next WORMSUIT narration voice
    // clip. DOS receives the scene index in ax (the saved si): builds the filename
    // via create_voc_file_name_from_bx (seg000:a8bc) from a "PF\PF001I .VOC"
    // template at seg001:37da, with bx=0x19 supplying the directory letter
    // ('A'+0x19='Z' → "PZ\PZ") and the saved ax supplying the 3-hex-digit clip
    // index (001..008). The trailing letter is a language suffix: DOS picks 'I'
    // (international) here because data_00006 == 0x80 at intro2 and no language
    // override is set — and indeed only PZ\PZNNNI.VOC entries exist in DUNE.DAT.
    //
    // DOS streams the clip in chunks through a loc_0ab92 frame task that polls
    // open_pcm_voice_file and steps _dword_22CC1_pcm_voc_resource_offset by 0x1a
    // bytes per refill. The port loads the whole .voc in one go via voc::parse
    // and hands it to the PCM mixer, so the streaming offset and frame task have
    // no equivalent. Missing file / disabled PCM is silent (= seg000:ab73 jb
    // loc_0ab8d / seg000:ab67 jz loc_0ab44).
    fn intro2_start_narration_voice(&mut self, si: u16) {
        // = seg000:a8bc create_voc_file_name_from_bx with bx=0x19, ax=si: the
        // template yields "PZ\PZ00<si>I.VOC" for si in 1..=8.
        let name = format!("PZ\\PZ00{si:X}I.VOC");

        // = seg000:ab70 open_pcm_voice_file; seg000:ab73 jb loc_0ab8d — bail on
        // missing resource (DAT without narration, e.g. floppy distributions).
        let Ok(data) = self.dat_file.read(&name) else {
            return;
        };
        // = voc_get_lipsync_data: parse the .voc to confirm it has a usable
        // type-1 PCM block (narration has no lip-sync stream); a missing block
        // means nothing to play — same effect as DOS's open failure.
        if crate::voc::parse(&data).is_none() {
            return;
        }

        // = seg000:ab6d pcm_stop_voc; seg000:ab89 word ptr [pcm_vtable_start_
        // playback] — drain whatever was queued, then start this clip on the
        // dnsdb driver. Mirrors the talking-head path (= seg000:a75c).
        self.pcm_player.stop();
        self.pcm_player.start_playback(&data, 0);
    }

    // = seg000:ddb0 wait_interruptable. Clear the pending scancode, then run the
    // frame-task driver for `ticks` PIT ticks, breaking early on ANY user input.
    // Returns true only when that input was the ESC key — DOS's ZF return, which
    // it preserves across the dde7 cleanup via pushf/popf and play_intro2's
    // `seg000:025c jz loc_00292` reads to abort the whole act. A non-ESC key, a
    // mouse/joystick button, or a full timeout returns false.
    //
    // Note: when _byte_227D_suppress_sky_240_255 == 0 DOS also writes the
    // secondary sky-colour span here (= seg000:ddc0 loc_0d64e); that sky-palette
    // step is not ported yet.
    fn wait_interruptable(&mut self, ticks: u64) -> bool {
        // = seg000:ddb4 [key_hit_scancode] = 0.
        self.kb_clear_scancode();
        // = seg000:ddca loop for `ticks` PIT ticks, polling any_key_pressed.
        let deadline = self.game_ticks() + ticks;
        while self.game_ticks() < deadline {
            // = seg000:ddcf any_key_pressed; jb loc_0dde7 — break out on ANY
            // input. The break's ZF distinguishes the cause: any_key_pressed
            // routes ESC through kb_check_for_esc_key_hit (seg000:dd66) and
            // reaches its `stc` return with ZF=1, while a non-ESC key/mouse/
            // joystick leaves ZF=0. kb_esc_was_hit holds that same bit here.
            if self.any_key_pressed() {
                return self.input.lock().unwrap().kb_esc_was_hit != 0;
            }
            self.tick_one_frame();
        }
        // = seg000:dde5 or al,1 — the timeout path clears ZF (not ESC).
        false
    }

    // = seg000:0292 loc_00292 — game setup after intro2.
    fn play_intro2_game_setup(&mut self) {
        // = seg000:0292 es=screen_buffer_seg; vga_clear_screen — clear the visible
        // screen buffer so no intro frame shows through before the room is drawn.
        self.screen.pixels_mut().fill(0);
        // = seg000:029a call pcm_stop_voc — drain any queued voice audio.
        self.pcm_player.stop();
        // = seg000:029d _byte_227D_suppress_sky_240_255 = 0 (in-game uses the full
        // sky palette span).
        self.data_0227d = 0;
        // = seg000:02a2 person_marker_base = 0 — sal_position_markers reads it
        // as the room-person arrangement base on the next room draw.
        self.person_marker_base = 0;
        // = seg000:02a7 remove_all_frame_tasks — also resets sky_skydn_selector
        // to 1 (= seg000:0920) so the in-game sky load goes through SKYDN.HSQ.
        self.remove_all_frame_tasks();
        // = seg000:02aa voice_subtitle_mode = 0.
        self.voice_subtitle_mode = 0;
        // = seg000:02af data_0dbe6 = 6 (zoom-step tick delay) — no port field yet.
        // = seg000:02b4 inc locations[0].nbr_orni
        self.locations[0].equipment.ornithopters += 1;
        // = seg000:02b8 dx=0x200a, bx=0x180, jmp loc_008f0 (open_SAL_resource
        // wrapper): set the game's location/room and slot. The actual SAL open
        // happens later via draw_location_room.
        self.location_and_room = 0x200a;
        self.location_appearance = 0x180;
        // Port-ism: reset fb_base_ofs to 0 for the in-game screen (the in-game HUD
        // + room scene draw there). DOS relies on the intro2 scenes having left it
        // at its segvga:01a3 static-init 0; the port stubs those scenes.
        self.clear_global_y_offset();
    }
}

// = seg000:8eda loc_08eda inner glyph-width loop. Sum tall-font glyph widths
// for one rendered byte sequence; bytes with the high bit set use the @ glyph
// (= seg000:d1c5 in font_draw_string), and 0xff stops the scan.
fn measure_line(font: &crate::Font, line: &[u8]) -> u16 {
    let mut w: u16 = 0;
    for &b in line {
        if b == 0xff {
            break;
        }
        let g = if b & 0x80 != 0 { 0x40 } else { b };
        w += font.glyph_width(g, TextSize::Large) as u16;
    }
    w
}

// = seg000:8e16 loc_08e16 — split `text` into lines that fit `max_w` pixels.
// DOS scans the text one byte at a time: 0x20 is a word break, 0x0d forces a
// new line, 0xff ends layout, anything else accumulates into the current word
// (measured by loc_08ed3). Each word adds 6 px of interword padding before
// the fit check (= seg000:8e52 add cx,6); when a word would overflow the
// remaining budget it starts a new line (= seg000:8e57 jb loc_08e5d). The
// port mirrors that idiom while emitting owned line bytes (with single spaces
// between words) ready for measure_line / font_draw_glyph.
fn intro2_wrap_subtitle(font: &crate::Font, text: &[u8], max_w: u16) -> Vec<Vec<u8>> {
    const INTERWORD_PAD: u16 = 6;
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut cur_w: u16 = 0;

    let mut i = 0;
    while i < text.len() {
        let b = text[i];
        if b == 0xff {
            break;
        }
        if b == 0x20 {
            // = seg000:8e36 inc si — collapse runs of spaces.
            i += 1;
            continue;
        }
        if b == 0x0d {
            // = seg000:8e30 explicit carriage return: commit the current line.
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
            i += 1;
            continue;
        }
        // = seg000:8e4b loc_08ed3 — scan one word's bytes and measure.
        let start = i;
        let mut word_w: u16 = 0;
        while i < text.len() {
            let c = text[i];
            if c == 0x20 || c == 0x0d || c == 0xff {
                break;
            }
            let g = if c & 0x80 != 0 { 0x40 } else { c };
            word_w += font.glyph_width(g, TextSize::Large) as u16;
            i += 1;
        }
        let word = &text[start..i];
        if word.is_empty() {
            continue;
        }
        // = seg000:8e52..8e5b add cx,6 / sub bx,cx / jb. The +6 is charged
        // before the fit check, so even the first word on a line pays it.
        let needed = word_w + INTERWORD_PAD;
        if cur_w + needed > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        if !cur.is_empty() {
            cur.push(b' ');
        }
        cur.extend_from_slice(word);
        cur_w += needed;
    }

    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}
