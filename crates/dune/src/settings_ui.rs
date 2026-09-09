//! The in-game mixer / settings panel (the CD release's audio overlay).
//!
//! Ported from `menu_callback_choice_mixer_panel` (seg000:a3f0) and its draw /
//! interaction helpers (seg000:a3f9..a671, plus the cleanup loc_0a541). The
//! panel is a MIXR.HSQ background with three volume sliders (PCM / music /
//! voice), one stereo balance/pan knob below each slider, and a button grid for
//! the voice-subtitle mode + language. It is shown as a menu overlay
//! with its own mouse-handler table (`MIXER_MOUSE_HANDLERS`, = seg001:1ad6).
//!
//! Dragging a slider (loc_0a5df, dispatched as the drag handler `[si+0ah]`)
//! adjusts its value byte, redraws the handle, and runs the `[si+6]` apply hook:
//! loc_0a637 sets the PCM/voices volume (the dnsdb driver), loc_0a650 sets the
//! MIDI music volume, and loc_0d917 is a no-op (the "music during voices" level
//! is consumed by the MIDI duck instead). The same drag handler also turns the
//! balance knobs (the rotary group-2 arm), whose value is passed as the balance
//! byte (`ah`) of the channel's set_volume call. DOS's drivers discarded that
//! byte (`dnsdb_set_volume` is a `retf` no-op, and the AdLib path ignored it),
//! but the port honours it as a per-channel pan over its CPAL mixer
//! (`pcm_player::balance_to_gains`). The button grid selects the voice-subtitle
//! mode + language. The
//! "test voice" button (loc_0a553) plays a sample line with the music ducked.
//! The command strip carries the music-playlist (jukebox) verbs: MUSIC OFF /
//! MUSIC ON in its two modes (game-relative and the CD-order submenu; the
//! playlist machinery lives in `music.rs`).

use crate::{
    GameState, Rect,
    menu_defs::{self, MenuRef},
    rect::rect,
    sprite_bank,
};

/// One 8-byte settings record (= the seg001 slider/knob layout at
/// 288e/2896/289e and 28a6/28ae/28b6): `[value:u8, drawn_flag:u8, dx:u16,
/// y:u16, apply_ofs:u16]`. `value` and `drawn_flag` are mutated as the
/// panel is drawn and dragged; the other three are static layout. `[si+6]`
/// (`apply_ofs`) is the seg000 offset of the audio-apply callback.
#[derive(Clone, Copy)]
pub(crate) struct SettingsRecord {
    /// `[si+0]` — the 0..0xf0 slider/indicator value byte.
    pub value: u8,
    /// `[si+1]` — set to 1 once drawn; gates the drag hit-test (loc_0a685).
    pub drawn_flag: u8,
    /// `[si+2]` — panel-local x of the slider track / indicator.
    pub x: i16,
    /// `[si+4]` — panel-local y of the handle, recomputed each draw for the
    /// volume sliders; static for the balance knobs.
    pub y: i16,
    /// `[si+6]` — seg000 offset of the audio-apply callback the drag commits
    /// (loc_0a637 PCM voices / loc_0a650 MIDI music / loc_0d917 no-op),
    /// dispatched by `settings_ui_apply`.
    pub apply_ofs: u16,
}

const fn sr(value: u8, drawn_flag: u8, x: i16, y: i16, apply_ofs: u16) -> SettingsRecord {
    SettingsRecord {
        value,
        drawn_flag,
        x,
        y,
        apply_ofs,
    }
}

pub(crate) const SETTINGS_RECORD_VOLUME_VOICES: usize = 0;
pub(crate) const SETTINGS_RECORD_VOLUME_MUSIC: usize = 1;
pub(crate) const SETTINGS_RECORD_VOLUME_MUSIC_DURING_VOICES: usize = 2;

pub(crate) const SETTINGS_RECORD_BALANCE_VOICES: usize = 3;
pub(crate) const SETTINGS_RECORD_BALANCE_MUSIC: usize = 4;
pub(crate) const SETTINGS_RECORD_BALANCE_MUSIC_DURING_VOICES: usize = 5;

/// = seg001:288e..28bd — the six settings records: indices 0..3 are the volume
/// sliders (voices / music / music-during-voices, drawn by
/// `settings_ui_draw_slider`), 3..6 the stereo balance/pan knobs (one per
/// channel, drawn by `settings_ui_draw_balance_knob`). The knobs share each
/// channel's apply hook with its slider — the slider sets the level (`al`), the
/// knob the balance (`ah`). GameState owns a mutable copy (`settings_records`);
/// this constant only seeds it.
pub(crate) const SETTINGS_RECORDS_INIT: [SettingsRecord; 6] = [
    sr(255, 0, 12, 34, 0xa637),  // 288e VOICES (digital PCM) volume
    sr(230, 0, 50, 34, 0xa650),  // 2896 MUSIC (MIDI) volume
    sr(180, 0, 88, 34, 0xd917),  // 289e MUSIC during voices (MIDI duck level)
    sr(100, 0, 17, 103, 0xa637), // 28a6 VOICES balance/pan knob
    sr(120, 0, 55, 103, 0xa650), // 28ae MUSIC balance/pan knob (center)
    sr(140, 0, 93, 103, 0xd917), // 28b6 MUSIC-during-voices balance/pan knob
];

/// = seg001:2886 - the settings panel rect
const SETTINGS_RECT: Rect = rect(40, 1, 250, 144);

/// = seg001:28bf — the panel-local "test voice" button rect [x0, y0, x1, y1]
/// (loc_0a553).
const SETTINGS_VOICE_RECT: Rect = rect(31, 122, 90, 132);

/// = seg001:28c7 settings_button_grid_rect — the panel-local button-grid rect
/// [x0, y0, x1, y1] (loc_0a5b0). Its x0/y0 also drive the button sprite
/// positions (loc_0a465 reads [28c7] as x, [28c9] as y).
const SETTINGS_BUTTON_GRID_RECT: Rect = rect(134, 39, 200, 130);

/// = seg001:28cf settings_button_grid_xlat — maps a button-grid row
/// `(local_y - y0) / 7` to an action: `< 7` selects a language, `== 7` is a
/// no-op gap, `> 7` selects voice_subtitle_mode `val - 8`.
const SETTINGS_BUTTON_GRID_XLAT: [u8; 13] = [0, 3, 1, 2, 4, 5, 6, 7, 7, 7, 8, 9, 10];

/// = seg001:28dc settings_language_sprite_xlat — maps a language/voice-mode
/// value to its sprite-row position (loc_0a465). Indexable by 0..=10 (the
/// language values 0..6 and the voice-mode values 8..10).
const SETTINGS_LANGUAGE_SPRITE_XLAT: [u8; 11] = [0, 2, 3, 1, 4, 5, 6, 7, 10, 11, 12];

// = seg000:a45c settings_ui_add_global_offset — panel-local to screen coordinates.
fn global_xy(x: i16, y: i16) -> (i16, i16) {
    (x + SETTINGS_RECT.x0, y + SETTINGS_RECT.y0)
}

// = seg000:a453 settings_ui_sub_global_offset — screen to panel-local coordinates.
fn local_xy(x: i16, y: i16) -> (i16, i16) {
    (x - SETTINGS_RECT.x0, y - SETTINGS_RECT.y0)
}

impl GameState {
    // ---- Entry / draw -----------------------------------------------------

    // = seg000:a3f0 menu_callback_choice_mixer_panel — open the in-game mixer /
    // settings panel. Installs the panel's mouse handlers, drains pending UI
    // tasks, then draws the panel (settings_ui_draw, which also inserts it as
    // the active menu). Wired as the CMD_MIXER_PANEL verb's callback. `pub` so
    // headless renders can open the panel directly.
    pub fn open_mixer_panel(&mut self) {
        // = seg000:a3f0 mov ax,1ad6h; call loc_0d95e — select the mixer handler table.
        self.set_active_mouse_handlers(&crate::game_ui::MIXER_MOUSE_HANDLERS);
        // = seg000:a3f6 call dismiss_stacked_overlays.
        self.dismiss_stacked_menus();
        // = seg000:a3f9 fall into settings_ui_draw.
        self.settings_ui_draw();
    }

    // = seg000:a3f9 settings_ui_draw — paint the whole panel (MIXR background,
    // sliders, balance knobs, language + voice buttons), then insert it as
    // the active menu. Also re-entered to repaint after an
    // interaction (loc_0a5db / loc_0a5c8), in which case the panel is already
    // the active element so the insert is a no-op.
    fn settings_ui_draw(&mut self) {
        // = seg000:a3f9 push [active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:a400 open MIXR; a406 draw sprite 0 (the panel background) at the
        // global offset.
        self.open_sprite_bank(sprite_bank::MIXR);
        self.draw_active_bank_sprite(0, SETTINGS_RECT.x0, SETTINGS_RECT.y0);
        // = seg000:a413 pop [active_seg].
        self.active_fb = saved;
        // = seg000:a417 call settings_ui_draw_volume_sliders.
        self.settings_ui_draw_volume_sliders();
        // = seg000:a41a call settings_ui_draw_balance_knobs — the stereo balance knobs.
        self.settings_ui_draw_balance_knobs();
        // = seg000:a41d call settings_ui_draw_language_buttons.
        self.settings_ui_draw_language_buttons();
        // = seg000:a420 call loc_0a44c — the voice/subtitle-mode button.
        self.settings_ui_draw_voice_mode_button();
        // = seg000:a423 call loc_0ac3a — the music-playlist element flags and
        //   the cl pre-highlight slot draw_command_menu applies.
        let cl = self.settings_ui_update_music_playlist_flags();
        // = seg000:a426 mov bx,0a541h; a429 jmp loc_0d32f — insert the panel as the
        // active menu WITH the command-panel fold transition (bx is the
        // cleanup func loc_0a541, settings_ui_cleanup). loc_0d32f
        // chains screen_overlay_request_transition -> screen_element_stack_insert
        // -> play_pending_panel_fold. The mixer panel itself was drawn straight to
        // the screen above (SETTINGS_RECT, rows 1..144); only the command/verb
        // strip below it (rows 159..199) folds. settings_ui_draw re-runs this on
        // every repaint, so a language / voice-mode button toggle replays the fold.

        // = seg000:d32f call screen_overlay_request_transition — arm in_transition
        //   (unless an HNM is playing) so the command-menu repaint stages into fb1.
        self.screen_overlay_request_transition();
        // = seg000:d332 call screen_element_stack_insert — push the panel identity
        //   (a re-insert of the already-active element is a no-op, matching d345's
        //   in-place replace) and repaint the command menu (draw_command_menu).
        //   With in_transition armed, redraw_active_command_menu stages the verb
        //   strip into fb1 ready for the fold to reveal.
        if self.get_active_menu_ref() != MenuRef::MenuMixerPanel {
            self.menu_stack.push((
                MenuRef::MenuMixerPanel,
                Some(GameState::settings_ui_cleanup),
            ));
        }
        self.draw_command_menu(cl);

        // DOS draws the mixer straight to VGA, so it is visible the instant it is
        // painted. The port renders into `screen`, so flush the MIXR palette and
        // present the composed panel now (unless composing offscreen, where the
        // caller presents) before the fold animates the command strip — otherwise
        // the panel stays invisible until an unrelated screen update presents.
        if !self.front_buffer_is_fb1() {
            self.update_screen_palette();
            self.send_frame_to_display();
        }

        // = seg000:d335 jmp play_pending_panel_fold — reveal the staged command
        //   strip with the 17-frame accordion fold.
        self.play_pending_panel_fold();
    }

    // = seg000:a4c6 settings_ui_draw_volume_sliders — draw the PCM slider (gated
    // by check_pcm_enabled) and the music + voice sliders (gated by loc_0ae28).
    // After each group, a clear settings_flags adjust-bit (0x4 PCM, 0x400
    // music/voice) resets the slider's drawn flag so it is not draggable.
    fn settings_ui_draw_volume_sliders(&mut self) {
        // = seg000:a4c6 call check_pcm_enabled; jz skip the PCM slider.
        if self.check_pcm_enabled() {
            // = seg000:a4cb si=288e; draw the PCM slider.
            self.settings_ui_draw_slider(0);
            // = seg000:a4d1 test settings_flags,4; when clear, reset the drawn flag
            // (data_0288f = 0) so the PCM slider is not draggable.
            if self.settings_flags & 0x4 == 0 {
                self.settings_records[SETTINGS_RECORD_VOLUME_VOICES].drawn_flag = 0;
            }
        }
        // = seg000:a4de call loc_0ae28; jz skip the music + voice sliders.
        if self.settings_music_enabled() {
            // = seg000:a4e3 si=2896; a4e9 si=289e — the music + voice sliders.
            self.settings_ui_draw_slider(1);
            self.settings_ui_draw_slider(2);
            // = seg000:a4ef test settings_flags,400h; when clear, reset both drawn
            // flags (data_02897 = data_0289f = 0).
            if self.settings_flags & 0x400 == 0 {
                self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC].drawn_flag = 0;
                self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC_DURING_VOICES].drawn_flag = 0;
            }
        }
    }

    // = seg000:a502 settings_ui_draw_slider — draw volume slider record `i`: the
    // track sprite (1) at the record's (x, 34) + global offset, then compute
    // the handle's y from the value byte (`((~value) >> 2) + 34`), store it
    // into the record's y, and draw the handle sprite (2). Marks the record
    // drawn (drawn_flag = 1) so it becomes draggable.
    fn settings_ui_draw_slider(&mut self, i: usize) {
        // = seg000:a502 push [active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:a50a open MIXR.
        self.open_sprite_bank(sprite_bank::MIXR);
        // = seg000:a510 dx=record.dx, bx=34; add_global_offset — the track position.
        let (track_x, track_y) = global_xy(self.settings_records[i].x, 34);
        // = seg000:a519 draw sprite 1 (the slider track).
        self.draw_active_bank_sprite(1, track_x, track_y);
        // = seg000:a520 al=value; a521 mark drawn (record.drawn_flag = 1).
        let value = self.settings_records[i].value;
        self.settings_records[i].drawn_flag = 1;
        // = seg000:a524 ax=~value; a526 al >>= 2; a52a cbw; a52b ax += track_y — the
        // handle's screen y.
        let handle_y = ((!value) >> 2) as i16 + track_y;
        // = seg000:a52f ax -= gy; a533 store the handle's panel-local y in record.y.
        self.settings_records[i].y = handle_y - SETTINGS_RECT.y0;
        // = seg000:a536 draw sprite 2 (the slider handle) at (track_x, handle_y).
        self.draw_active_bank_sprite(2, track_x, handle_y);
        // = seg000:a53c pop [active_seg].
        self.active_fb = saved;
    }

    // = seg000:a47d settings_ui_draw_balance_knobs — draw the stereo balance/pan
    // knobs (records 3..6) gated by settings_flags: bit 0x8 draws the voices
    // knob, bit 0x800 draws the music and music-during-voices knobs.
    fn settings_ui_draw_balance_knobs(&mut self) {
        // = seg000:a47d test settings_flags,8.
        if self.settings_flags & 0x8 != 0 {
            // = seg000:a485 si=28a6; call settings_ui_draw_balance_knob.
            self.settings_ui_draw_balance_knob(3);
        }
        // = seg000:a48b test settings_flags,800h.
        if self.settings_flags & 0x800 != 0 {
            // = seg000:a493 si=28ae; a499 si=28b6.
            self.settings_ui_draw_balance_knob(4);
            self.settings_ui_draw_balance_knob(5);
        }
    }

    // = seg000:a49c settings_ui_draw_balance_knob — draw one balance/pan knob:
    // its needle sprite is `value / 10 + 3` (aam 0ah), 25 frames sweeping
    // left<->right over value 0..0xf0, drawn at the record's (x, y) +
    // global offset. Marks the record drawn.
    fn settings_ui_draw_balance_knob(&mut self, i: usize) {
        // = seg000:a49c push [active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:a4a3 open MIXR.
        self.open_sprite_bank(sprite_bank::MIXR);
        // = seg000:a4a9 lodsb value; a4aa aam 0ah; al=value/10; a4b0 add al,3 — sprite.
        let sprite = (self.settings_records[i].value / 10 + 3) as u16;
        // = seg000:a4b2 mark drawn (record.drawn_flag = 1).
        self.settings_records[i].drawn_flag = 1;
        // = seg000:a4b6 x=record.x, bx=record.y; add_global_offset.
        let x = self.settings_records[i].x + SETTINGS_RECT.x0;
        let y = self.settings_records[i].y + SETTINGS_RECT.y0;
        // = seg000:a4be draw the indicator sprite.
        self.draw_active_bank_sprite(sprite, x, y);
        // = seg000:a4c1 pop [active_seg].
        self.active_fb = saved;
    }

    // = seg000:a42c settings_ui_draw_language_buttons — open MIXR, then draw the
    // current language_setting's button (loc_0a435).
    fn settings_ui_draw_language_buttons(&mut self) {
        // = seg000:a42f open MIXR.
        self.open_sprite_bank(sprite_bank::MIXR);
        // = seg000:a432 al = language_setting; fall into loc_0a435.
        self.settings_ui_draw_button(self.language_setting);
    }

    // = seg000:a44c loc_0a44c — draw the voice/subtitle-mode button: input
    // `voice_subtitle_mode + 8` into the shared button draw (loc_0a435). Relies
    // on MIXR already being the active bank (the preceding language-button draw).
    fn settings_ui_draw_voice_mode_button(&mut self) {
        // = seg000:a44c al = voice_subtitle_mode + 8; jmp loc_0a435.
        self.settings_ui_draw_button(self.voice_subtitle_mode + 8);
    }

    // = seg000:a435 loc_0a435 — draw a panel button for input `al`: its sprite
    // is `al * 2 + 28`, drawn at the position loc_0a465 computes from `al`.
    fn settings_ui_draw_button(&mut self, al: u8) {
        // = seg000:a435 push [active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:a43d call loc_0a465 — the draw position (al preserved across it).
        let (x, y) = self.settings_ui_button_pos(al);
        // = seg000:a440 shl ax,1; add al,1ch — the button sprite.
        let sprite = (al as u16) * 2 + 28;
        // = seg000:a444 draw the button sprite.
        self.draw_active_bank_sprite(sprite, x, y);
        // = seg000:a447 pop [active_seg].
        self.active_fb = saved;
    }

    // = seg000:a465 loc_0a465 — compute a panel button's draw position for input
    // `al`: x = button_grid_rect.x0 + gx; y = sprite_xlat[al] * 7 +
    // button_grid_rect.y0 + gy. The input `al` itself is preserved (the DOS
    // push/pop ax), so loc_0a435 can still derive the sprite from it.
    fn settings_ui_button_pos(&self, al: u8) -> (i16, i16) {
        // = seg000:a466 dx = [data_028c7] = button_grid_rect.x0.
        let x = SETTINGS_BUTTON_GRID_RECT.x0 + SETTINGS_RECT.x0;
        // = seg000:a46a xlat through settings_language_sprite_xlat; a46e *7; a474 add
        // [data_028c9] = button_grid_rect.y0.
        let row = SETTINGS_LANGUAGE_SPRITE_XLAT[al as usize];
        let y = row as i16 * 7 + SETTINGS_BUTTON_GRID_RECT.y0 + SETTINGS_RECT.y0;
        (x, y)
    }

    // ---- Mouse handlers (= seg001:1ad6 table) -----------------------------

    // = the mixer panel's idle handler (cs:[si] = loc_00f66, a no-op).
    pub(crate) fn mixer_panel_idle(&mut self) {}

    // = the mixer panel's RMB handler ([si+4] = loc_00f66, a no-op).
    pub(crate) fn mixer_panel_rmb(&mut self) {}

    // = the mixer panel's RMB-release handler ([si+8] = loc_00f66, a no-op):
    // the panel arms its drag target only on the left button.
    pub(crate) fn mixer_panel_rmb_release(&mut self) {}

    // = the mixer panel's RMB-drag handler ([si+0ch] = loc_00f66, a no-op):
    // the sliders are dragged with the left button only.
    pub(crate) fn mixer_panel_rmb_drag(&mut self, _dx: i16, _dy: i16) {}

    // = seg000:a5aa loc_0a5aa — the mixer panel's LMB-release handler ([si+6]):
    // clear the drag target (data_028be = 0), so get_mouse_cursor_image reverts
    // from the busy hand to the arrow once the slider is let go.
    pub(crate) fn mixer_panel_release(&mut self) {
        self.settings_drag_target = 0;
    }

    // = seg000:a576 loc_0a576 — the panel's LMB handler. Hit-test the whole
    // panel rect (di=2886): a click outside closes the panel
    // (menu_callback_choice_exit_menu); a hit dispatches to the interior.
    pub(crate) fn mixer_panel_lmb(&mut self) {
        // game_loop lifts the software cursor (= seg000:d8f4 call_restore_cursor)
        // before dispatching this, so the panel redraw / close repaint below lands
        // on clean background and the next redraw_mouse re-composites the cursor.
        let x = self.mouse_pos_x as i16;
        let y = self.mouse_pos_y as i16;
        // = seg000:a576 di=2886; call loc_0d6fe — rect-test the panel.
        if !SETTINGS_RECT.contains_interior(x, y) {
            // = seg000:a57e jmp menu_callback_choice_exit_menu — a miss closes the panel.
            self.menu_callback_choice_exit_menu(0, 0);
            return;
        }
        // = seg000:a581 fall into loc_0a581 — interact with the panel interior.
        self.mixer_panel_click_interior(x, y);
    }

    // = seg000:a581 loc_0a581 — dispatch an interior click. Re-base to
    // panel-local coords, then test the test-voice button (28bf), the button
    // grid (28c7), and finally the slider handles (loc_0a594).
    fn mixer_panel_click_interior(&mut self, x: i16, y: i16) {
        // = seg000:a581 call settings_ui_sub_global_offset — panel-local coords.
        let (lx, ly) = local_xy(x, y);
        // = seg000:a584 di=28bf; loc_0d6fe — the test-voice button.
        if SETTINGS_VOICE_RECT.contains_interior(lx, ly) {
            // = seg000:a58a jb loc_0a553.
            self.settings_ui_play_test_voice();
            return;
        }
        // = seg000:a58c di=28c7; loc_0d6fe — the button grid.
        if SETTINGS_BUTTON_GRID_RECT.contains_interior(lx, ly) {
            // = seg000:a592 jb loc_0a5b0.
            self.mixer_panel_button_grid_click(ly);
            return;
        }
        // = seg000:a594 fall into loc_0a594 — grab a slider handle.
        self.mixer_panel_set_drag_target(lx, ly);
    }

    // = seg000:a594 loc_0a594 — record which slider group the click grabbed.
    // The drag motion re-finds the exact handle each frame, so a plain click only
    // arms the group; the value moves once the pointer is dragged.
    fn mixer_panel_set_drag_target(&mut self, lx: i16, ly: i16) {
        self.settings_ui_grab_handle(lx, ly);
    }

    // = seg000:a594 loc_0a594 (returning what settings_grab_volume_slider /
    // settings_grab_balance_knob leave in si/ax/bp) — find the handle under
    // the panel-local pointer and set settings_drag_target: 1 for a volume
    // slider (records 0..3, 22 x 5 box), 2 for a balance knob (records 3..6,
    // 13 x 11 box), 0 for neither. Returns `(group, index, rx, ry)` where
    // rx/ry are the pointer's offset into the matched handle's box, which the
    // knob drag (loc_0a5df) uses to pick the rotation direction.
    fn settings_ui_grab_handle(&mut self, lx: i16, ly: i16) -> (u8, usize, i16, i16) {
        // = seg000:a594 call settings_grab_volume_slider; jnb.
        if let Some((i, rx, ry)) = self.settings_grab_volume_slider(lx, ly) {
            // = seg000:a599 data_028be = 1.
            self.settings_drag_target = 1;
            return (1, i, rx, ry);
        }
        // = seg000:a59f call settings_grab_balance_knob; jnb.
        if let Some((i, rx, ry)) = self.settings_grab_balance_knob(lx, ly) {
            // = seg000:a5a4 data_028be = 2.
            self.settings_drag_target = 2;
            return (2, i, rx, ry);
        }
        // = seg000:a5aa data_028be = 0 — no handle grabbed.
        self.settings_drag_target = 0;
        (0, 0, 0, 0)
    }

    // = seg000:a672 settings_grab_volume_slider — hit-test the three volume
    // slider handles (settings_slider_voices, _music, _music_during_voices)
    // in order; CF set + si = the record on a hit.
    fn settings_grab_volume_slider(&mut self, lx: i16, ly: i16) -> Option<(usize, i16, i16)> {
        (0..3).find_map(|i| {
            self.settings_slider_handle_hit(i, lx, ly)
                .map(|(rx, ry)| (i, rx, ry))
        })
    }

    // = seg000:a69f settings_grab_balance_knob — hit-test the three balance
    // knob handles (records 3..6) in order; CF set + si = the record on a hit.
    fn settings_grab_balance_knob(&mut self, lx: i16, ly: i16) -> Option<(usize, i16, i16)> {
        (3..6).find_map(|i| {
            self.settings_knob_handle_hit(i, lx, ly)
                .map(|(rx, ry)| (i, rx, ry))
        })
    }

    // = seg000:a685 settings_slider_handle_hit — one volume slider handle:
    // the record must be drawn (drawn_flag == 1), and the panel-local pointer
    // must fall in the 22 x 5 box anchored at the record's (x, y). Returns
    // the pointer's `(rx, ry)` offset into the box on a hit.
    fn settings_slider_handle_hit(&self, i: usize, lx: i16, ly: i16) -> Option<(i16, i16)> {
        let r = &self.settings_records[i];
        // = seg000:a685 cmp byte[si+1],1; cmc; jnb ret — require the record drawn.
        if r.drawn_flag != 1 {
            return None;
        }
        // = seg000:a68c ax = lx - x; a691 bp = ly - y; a696 cmp ax,16h; a69b
        //   cmp bp,5 (unsigned, so a pointer above/left of the box wraps high
        //   and misses).
        if !rect(r.x, r.y, r.x + 22, r.y + 5).in_rect(lx, ly) {
            return None;
        }
        Some((lx - r.x, ly - r.y))
    }

    // = seg000:a6b2 settings_knob_handle_hit — one balance knob handle: like
    // settings_slider_handle_hit with a 13 x 11 box.
    fn settings_knob_handle_hit(&self, i: usize, lx: i16, ly: i16) -> Option<(i16, i16)> {
        let r = &self.settings_records[i];
        // = seg000:a6b2 cmp byte[si+1],1; cmc; jnb ret.
        if r.drawn_flag != 1 {
            return None;
        }
        // = seg000:a6b9 ax = lx - x; a6be bp = ly - y; a6c3 cmp ax,0dh; a6c8
        //   cmp bp,0bh.
        if !rect(r.x, r.y, r.x + 13, r.y + 11).in_rect(lx, ly) {
            return None;
        }
        Some((lx - r.x, ly - r.y))
    }

    // = seg000:a5b0 loc_0a5b0 — a button-grid click. Map the row
    // `(local_y - grid.y0) / 7` through the xlat table to either a language
    // selection (< 7), a no-op gap (== 7), or a voice_subtitle_mode (> 7), then
    // redraw the panel (settings_ui_draw).
    fn mixer_panel_button_grid_click(&mut self, ly: i16) {
        // = seg000:a5b0 sub bx,[di+2]=grid.y0; a5b5 div 7 — the grid row.
        let row = (ly - SETTINGS_BUTTON_GRID_RECT.y0) / 7;
        // = seg000:a5bc xlat through settings_button_grid_xlat.
        let Some(&action) = SETTINGS_BUTTON_GRID_XLAT.get(row as usize) else {
            return;
        };
        // = seg000:a5bd cmp al,7.
        if action > 7 {
            // = seg000:a5c3 sub al,8; voice_subtitle_mode = al.
            self.voice_subtitle_mode = action - 8;
            // = seg000:a5c8 jmp loc_0a5db (settings_ui_draw).
            self.settings_ui_draw();
        } else if action == 7 {
            // = seg000:a5c1 jz loc_0a5de — the no-op gap rows.
        } else {
            // = seg000:a5ca loc_0a5ca — a language selection.
            // = seg000:a5ca cmp al,language_setting; jz ret — unchanged.
            if action == self.language_setting {
                return;
            }
            // = seg000:a5d0 and voice_subtitle_mode,0fdh — clear the subtitle bit.
            self.voice_subtitle_mode &= 0xfd;
            // = seg000:a5d5 language_setting = al.
            self.language_setting = action;
            // = seg000:a5d8 call settings_ui_reload_language — reload the language fonts/strings.
            self.settings_ui_reload_language();
            // = seg000:a5db jmp settings_ui_draw.
            self.settings_ui_draw();
        }
    }

    // = seg000:a5df mixer_panel_drag — the mixer panel's drag handler
    // (settings_ui_mouse_handlers [si+0ah]), dispatched each pass the LMB is
    // held without an edge and the pointer moved, with the (dx, dy) motion
    // delta. It re-grabs the handle at the *previous* frame's position
    // (current minus the delta) and nudges its value.
    pub(crate) fn mixer_panel_drag(&mut self, dx: i16, dy: i16) {
        // = seg000:a5df call settings_ui_sub_global_offset — current panel-local pointer.
        let lx = self.mouse_pos_x as i16 - SETTINGS_RECT.x0;
        let ly = self.mouse_pos_y as i16 - SETTINGS_RECT.y0;
        // = seg000:a5e2 sub bx,cx — re-base Y to the previous frame's position.
        let prev_ly = ly - dy;
        // = seg000:a5e4 call loc_0a594 — re-grab the handle there.
        let (group, i, rx, bp_off) = self.settings_ui_grab_handle(lx, prev_ly);
        match group {
            // = seg000:a5ec/a61a a volume slider: move the handle by the Y delta.
            1 => {
                // = seg000:a61a jcxz loc_0a619 — no Y motion, no change.
                if dy == 0 {
                    return;
                }
                // = seg000:a61c ax = y + dy - 34; a624 cmp ax,40h; jnb ret.
                let raw = self.settings_records[i].y + dy - 34;
                if !(0..64).contains(&raw) {
                    return;
                }
                // = seg000:a629 ax <<= 2; a62d not ax; a62f record.value = al.
                self.settings_records[i].value = !((raw << 2) as u8);
                // = seg000:a631 push [si+6]; a634 jmp settings_ui_draw_slider — redraw +
                // run the audio-apply hook (bracketed by the cursor lift).
                self.settings_ui_commit_drag(i, false);
            }
            // = seg000:a5ee a balance knob: turn it, nudging the value by +/-10 per
            // the 2D (rotary) drag direction.
            2 => {
                // = seg000:a5f5 if (lx - dx) < 6, negate the X delta's contribution (cx).
                let cx = if rx < 6 { -dy } else { dy };

                // = seg000:a5fc if (prev_ly - y) >= 5, negate di.
                let di = if bp_off >= 5 { -dx } else { dx };

                // = seg000:a603 step = +10 when (cx + di) >= 0, else -10.
                let step: i8 = if cx + di >= 0 { 10 } else { -10 };
                // = seg000:a60b al = record.value + step; a60d cmp al,0f1h; jnb ret.
                let new_value = self.settings_records[i]
                    .value
                    .saturating_add_signed(step)
                    .min(240);
                // = seg000:a611 record.value = al; a613 push [si+6]; a616 jmp
                // settings_ui_draw_balance_knob.
                self.settings_records[i].value = new_value;
                self.settings_ui_commit_drag(i, true);
            }
            // = seg000:a619 loc_0a619 — no handle grabbed, nothing to move.
            _ => {}
        }
    }

    // Commit a slider/knob drag: redraw the handle, run its audio-apply
    // hook, re-composite the software cursor, and present.
    //
    // = the `push [si+6]; jmp redraw` tail of loc_0a61a / loc_0a5df. game_loop has
    // already lifted the cursor (= seg000:d8ce call_restore_cursor) before
    // dispatching the drag, so this only redraws over the clean area. DOS wrote
    // straight to VGA and let the next redraw_mouse re-show the cursor; the port
    // renders into `screen`, so it re-composites the cursor here (draw_mouse) and
    // presents a complete frame — the discrete-frame adaptation. Both the
    // draw_mouse and the present are no-ops for the GPU cursor / while composing
    // offscreen.
    fn settings_ui_commit_drag(&mut self, i: usize, knob: bool) {
        if knob {
            self.settings_ui_draw_balance_knob(i);
        } else {
            self.settings_ui_draw_slider(i);
        }
        self.settings_ui_apply(i);
        self.draw_mouse();
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:a541 loc_0a541 — the mixer-panel cleanup, run when the panel
    // element pops: commit the voice/subtitle mode as the new default, restore
    // the room mouse handlers, and repaint the area the panel covered.
    pub(crate) fn settings_ui_cleanup(&mut self) {
        // = seg000:a541 voice_subtitle_mode_default = voice_subtitle_mode.
        self.voice_subtitle_mode_default = self.voice_subtitle_mode;
        // = seg000:a547 call clear_mouse_nav_rect.
        self.clear_mouse_nav_rect();
        // = seg000:a54a call select_room_ui_table — restore the room handlers.
        self.select_room_ui_table();
        // = seg000:a54d si=2886; jmp present_screen_rect.
        self.settings_ui_repaint_panel_rect();
    }

    // = seg000:c4f0 present_screen_rect (si=2886)
    // — repaint the panel rect. The rect overlaps the HUD head box, so the head is
    // refreshed in fb1 and the rect is copied fb1 -> screen. The c51e copy skips
    // only while the mixer handlers are still active, but cleanup ran
    // select_room_ui_table just above, so the copy proceeds.
    fn settings_ui_repaint_panel_rect(&mut self) {
        self.present_screen_rect(SETTINGS_RECT);
    }

    // ---- Audio gates / apply hooks ----------------------------------------

    // = seg000:ae2f check_pcm_enabled — digital sound (PCM) present. Stubbed to
    // its steady state via settings_flags bit 0x1.
    pub(crate) fn check_pcm_enabled(&self) -> bool {
        self.settings_flags & 0x1 != 0
    }

    // Port-only (no DOS equivalent): model the presence of a digital-sound (PCM)
    // card. Sets both the game-logic gate (settings_flags bit 0x1, read by
    // check_pcm_enabled) and the actual PCM output — the intro and some game
    // paths call the pcm player directly, bypassing check_pcm_enabled, so
    // disabling the player is what makes "no PCM card" fully silent. Wired to the
    // --pcm CLI arg; set_headless defaults it off.
    pub fn set_pcm_enabled(&mut self, enabled: bool) {
        if enabled {
            self.settings_flags |= 0x1;
        } else {
            self.settings_flags &= !0x1;
        }
        self.pcm_player.set_enabled(enabled);
    }

    // Port-only (no DOS equivalent): model the presence of a MIDI card. Sets both
    // the game-logic gate (settings_flags bit 0x100, read by
    // music_service_enabled) and the MIDI output — the intro plays its songs
    // directly, bypassing the gate, so silencing the player (which still advances
    // song timing) is what makes "no MIDI card" quiet through the intro too.
    // Wired to `--music off` (the equivalent of no MIDI hardware).
    pub fn set_music_enabled(&mut self, enabled: bool) {
        if enabled {
            self.settings_flags |= 0x100;
        } else {
            self.settings_flags &= !0x100;
        }
        self.midi.set_enabled(enabled);
    }

    // Port-only (no DOS equivalent): choose the startup music mode (the
    // `--music` selections). Two halves, applied at different times:
    //
    // * the card-presence half runs now, because it has to be in place before
    //   the intro, which drives the MIDI output directly (`Disabled` = no MIDI
    //   card);
    // * the mode itself is held pending, because `start()` resets
    //   music_playlist_flags at seg000:0019 and would wipe it. `start()`
    //   applies it through apply_pending_music_mode straight after that reset.
    pub fn set_music_mode(&mut self, mode: crate::MusicMode) {
        self.set_music_enabled(mode != crate::MusicMode::Disabled);
        self.pending_music_mode = Some(mode);
    }

    // Port-only: land the mode set_music_mode held, leaving the same
    // cmd_args_memory / music_playlist_flags state the mixer panel's MUSIC
    // verbs do, without their UI side effects (panel pop, submenu push,
    // update_room_music). Called by start() after the seg000:0019 reset; with
    // no `--music` selection (every non-CLI caller, tests included) there is
    // nothing pending and the startup state stands as it is.
    pub(crate) fn apply_pending_music_mode(&mut self) {
        let Some(mode) = self.pending_music_mode else {
            return;
        };
        // = seg000:aeaf MUSIC OFF sets cmd_args_memory bit 4; the MUSIC ON
        //   verbs clear it. Disabled leaves it alone: with no card,
        //   settings_flags bit 0x100 keeps everything silent on its own, and
        //   an in-game MUSIC ON must not be able to talk a card into existence.
        if mode == crate::MusicMode::Off {
            self.cmd_args_memory |= 0x10;
        } else if mode != crate::MusicMode::Disabled {
            self.cmd_args_memory &= !0x10;
        }
        match mode {
            // Nothing plays either way; the playlist mode is moot.
            crate::MusicMode::Disabled | crate::MusicMode::Off => {}
            // = seg000:ac6e GAME RELATIVE — playlist = 0.
            crate::MusicMode::GameRelative => self.music_playlist_flags = 0,
            // = seg000:ac97 STANDARD ORDER — CD-style (bit 0), no shuffle, with
            //   the pristine order recopied over the working playlist.
            crate::MusicMode::CdStandard => {
                self.music_playlist_flags = 1;
                self.music_cd_playlist = crate::music::MUSIC_CD_STANDARD_ORDER;
            }
            // = seg000:ac90 SHUFFLE — CD-style + shuffle (bits 0 and 1); the
            //   service reshuffles the playlist when it starts the next song.
            crate::MusicMode::CdShuffle => self.music_playlist_flags = 3,
        }
    }

    // = seg000:ae28 loc_0ae28 — music (MIDI) present. Stubbed to its steady
    // state via settings_flags bit 0x100.
    fn settings_music_enabled(&self) -> bool {
        self.settings_flags & 0x100 != 0
    }

    // = the [si+6] audio-apply dispatch a slider commit chains into (`push
    // [si+6]; jmp redraw`, so the redraw `ret`s into the apply hook). Routes the
    // record's apply_ofs to its ported hook.
    fn settings_ui_apply(&mut self, i: usize) {
        match self.settings_records[i].apply_ofs {
            // = seg000:a637 loc_0a637 — the PCM (voices) volume.
            0xa637 => self.settings_ui_apply_pcm(),
            // = seg000:a650 loc_0a650 — the MIDI (music) volume.
            0xa650 => self.settings_ui_apply_midi(),
            // = seg000:d917 fn_0d917_noop — the "music during voices" slider's
            // apply hook is a no-op `ret`; its value is consumed by the MIDI duck
            // (midi_duck_music_volume) rather than applied here.
            0xd917 => {}
            other => eprintln!("unhandled settings apply hook: {other:#06x}"),
        }
    }

    // = seg000:a637 loc_0a637 — apply the PCM (voices) volume on the single
    // dnsdb driver. All digital audio (standalone voices and HNM video sound)
    // runs through pcm_player, so this slider governs every digital sound at
    // once — exactly as the original, where one PCM driver served both.
    fn settings_ui_apply_pcm(&mut self) {
        // = seg000:a637 test settings_flags,4; when clear, force the value to 0xff.
        if self.settings_flags & 0x4 == 0 {
            self.settings_records[SETTINGS_RECORD_VOLUME_VOICES].value = 0xff;
        }
        // = seg000:a644 al = record[0].value (level), ah = record[3].value (the voices
        // balance/pan byte); call [pcm_vtable_set_volume] (= pcm_player, the
        // dnsdb driver). DOS's dnsdb_set_volume is a retf no-op, but the port's
        // CPAL mixer honours both: the level via set_volume, the balance knob via
        // set_balance.
        let volume = self.settings_records[SETTINGS_RECORD_VOLUME_VOICES].value;
        let balance = self.settings_records[SETTINGS_RECORD_BALANCE_VOICES].value;

        self.pcm_player.set_volume(volume);
        self.pcm_player.set_balance(balance);
    }

    // = seg000:a650 loc_0a650 — apply the MIDI (music) volume.
    fn settings_ui_apply_midi(&mut self) {
        // = seg000:a650 test settings_flags,400h; when clear, force music + voice
        // values to 0xff.
        if self.settings_flags & 0x400 == 0 {
            self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC].value = 0xff;
            self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC_DURING_VOICES].value = 0xff;
        }
        // = seg000:a660 al = record[1].value (music level); a667 clamp al >= 4; ah =
        // record[4].value (the music balance/pan byte); call [MIDI_SetVolume].
        // DOS's AdLib driver discarded the balance, but the port pans the OPL3
        // mix: the level via set_music_volume, the balance knob via set_balance.
        let volume = self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC]
            .value
            .max(4);
        let balance = self.settings_records[SETTINGS_RECORD_BALANCE_MUSIC].value;
        self.midi.set_music_volume(volume);
        self.midi.set_balance(balance);
    }

    // = seg000:a553 loc_0a553 — play the "test voice" sample with the music
    // ducked, so the player can judge the voices-volume slider against a real
    // line. Plays VOC (ax=4, bx=5) on the dnsdb driver.
    fn settings_ui_play_test_voice(&mut self) {
        // = seg000:a553 call check_pcm_enabled; jz ret.
        if !self.check_pcm_enabled() {
            return;
        }
        // = seg000:a558 ax=4, bx=5; create_voc_file_name_from_bx (seg000:a8bc) — build
        // the clip name "P<L>\P<L><idx><suffix>.VOC": the directory letter
        // L = 'A' + bx = 'F', idx = ax as three hex digits = "004", and the
        // suffix (= seg000:a8e1) is 'I' for the in-location desert scenes
        // (data_000ea <= 0 && location_appearance.lo == 0x80 && room != 1) or
        // 'O' otherwise. The DOS trailing data_047e0 letter is not modelled.
        let interior = self.data_000ea <= 0
            && (self.location_appearance & 0xff) == 0x80
            && (self.location_and_room & 0xff) != 1;
        let suffix = if interior { 'I' } else { 'O' };
        let name = format!("PF\\PF004{suffix}.VOC");
        // = seg000:a561 voc_get_lipsync_data — load the clip; bail if the resource is
        // absent (e.g. a DAT without narration), matching DOS's open failure.
        let Ok(data) = self.dat_file.read(&name) else {
            return;
        };
        if crate::voc::parse(&data).is_none() {
            return;
        }
        // = seg000:a564 midi_duck_music_volume
        self.midi_duck_music_volume();
        // = seg000:a567 is_voc_pcm_playing=1; a56c si=3811h; a56f
        // [pcm_vtable_start_playback] — voc_get_lipsync_data ran pcm_stop_voc
        // (a84a) and set up the stream, so the clip plays chunked like any
        // other voice.
        self.pcm_stop_voc();
        self.pcm_voice_stream_start(data);
        // = seg000:a573 jmp wait_for_narration_voice_clip (seg000:aba9) — DOS blocks,
        // pumping frame_task_callback_0ab92 until the clip drains and it
        // restores the music. The port's mixer
        // panel is event-driven, so install that monitor as a frame task
        // (interval 1) instead: it ramps the ducked music back up once the clip
        // ends, keeping the panel responsive meanwhile. Keep it a singleton so
        // repeated clicks (which restart the clip) don't stack monitors.
        self.remove_frame_task(crate::TaskId::PcmVoiceMusicRestore);
        self.add_frame_task(1, crate::TaskId::PcmVoiceMusicRestore);
    }

    // = seg000:ac3a settings_ui_update_music_playlist_flags — install the mixer's
    // music menu (bp = menu_mixer_panel) as the command verb strip and grey its
    // three MUSIC entries when music is disabled. In DOS this toggles the static
    // menu's 0x40 flag bytes ([bp+3]/[bp+7]/[bp+0bh]) in place and leaves
    // bp = menu_mixer_panel so the following screen_element_stack_insert (the
    // `jmp loc_0d32f` tail of settings_ui_draw) installs it; the port rebuilds
    // menu_mixer_panel.records from the template instead, which the tail's
    // redraw_active_command_menu then paints (staged to fb1 for the panel fold).
    //
    // It also returns the `cl` pre-highlight DOS leaves for draw_command_menu:
    // the slot of the menu's currently-selected entry, which draw_command_menu
    // marks with CMD_HIGHLIGHT so redraw_active_command_menu draws it inverse.
    // cl is 0xff (no highlight) when music is disabled.
    //
    pub(crate) fn settings_ui_update_music_playlist_flags(&mut self) -> u8 {
        // = seg000:ac4b call loc_0ae28 — grey all three MUSIC entries (ac3d..ac45 set
        //   the 0x40 bit) unless music is enabled, in which case ac50..ac58 clear
        //   it again.
        let music_enabled = self.settings_music_enabled();
        let disabled = !music_enabled;
        // = seg000:ac3d..ac58 toggle the 0x40 grey bit on the three MUSIC entries of
        //   menu_mixer_panel in place (the static buffer, seg001:201a); the
        //   highlight bit is applied by draw_command_menu, so rebuild from the template.
        let template = menu_defs::MENU_MIXER_PANEL.records;
        self.menu_mixer_panel.records = vec![
            template[0].grayed_if(disabled),
            template[1].grayed_if(disabled),
            template[2].grayed_if(disabled),
            template[3],
            template[4],
        ];

        // = seg000:ac49 cl = 0xff (no pre-highlight); ac4e jz loc_0ac6d — when music is
        //   disabled the entries stay greyed and none is highlighted.
        if !music_enabled {
            return 0xff;
        }
        // = seg000:ac5c xor cx,cx; ac5e test cmd_args_memory,10h.
        if self.cmd_args_memory & 0x10 != 0 {
            // = seg000:ac63 jnz loc_0ac6d with cl = 0 — music is off: highlight MUSIC
            //   OFF (slot 0).
            0
        } else {
            // = seg000:ac65 cl = (music_playlist_flags & 1) + 1 — the active MUSIC ON
            //   variant: GAME RELATIVE (slot 1) or CD-STYLE (slot 2).
            (self.music_playlist_flags & 1) + 1
        }
    }

    // ---- Music-menu verb handlers -----------------------------------------
    //
    // These are the MENU_MIXER_PANEL command-strip verbs and the CD-order
    // submenu (MENU_MUSIC) the CD-STYLE verb pushes over them. EXIT GAME
    // / " Done" are routed to their own handlers (menu_callback_choice_exit_
    // game / menu_callback_choice_exit_menu).

    // = seg000:aeaf menu_callback_choice_music_off — MUSIC OFF: set the
    // music-off toggle, close the mixer panel, and silence the current song.
    pub(crate) fn menu_callback_choice_music_off(&mut self, _text_id: u16, _index: usize) {
        // = seg000:aeaf or [cmd_args_memory],10h — check_music_enabled now
        //   gates every music path.
        self.cmd_args_memory |= 0x10;
        // = seg000:aeb4 call menu_callback_choice_exit_menu — pop the mixer
        //   panel (its settings_ui_cleanup runs) and fold the menu beneath in.
        self.menu_callback_choice_exit_menu(0, 0);
        // = seg000:aeb7 falls into midi_reset — stop the playing song.
        self.midi.midi_reset();
    }

    // = seg000:ac6e menu_callback_choice_music_on_game_relative — MUSIC ON
    // (GAME RELATIVE): re-enable music in the situation-driven jukebox mode.
    pub(crate) fn menu_callback_choice_music_on_game_relative(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:ac6e and [cmd_args_memory],0efh — clear the music-off toggle.
        self.cmd_args_memory &= !0x10;
        // = seg000:ac73 music_playlist_flags = 0 — game-relative mode.
        self.music_playlist_flags = 0;
        // = seg000:ac78 call menu_callback_choice_exit_menu — close the mixer.
        self.menu_callback_choice_exit_menu(0, 0);
        // = seg000:ac7b jmp update_room_music (loc_0ad5e) — pick the song for
        //   the current situation; service_midi_music starts it.
        self.update_room_music();
    }

    // = seg000:ac7e menu_callback_choice_music_on_cd_style — MUSIC ON
    // (CD-STYLE): push the CD-order submenu (STANDARD ORDER / SHUFFLE /
    // Cancel) over the mixer menu, pre-highlighting the active order.
    pub(crate) fn menu_callback_choice_music_on_cd_style(&mut self, _text_id: u16, _index: usize) {
        // = seg000:ac7e bp = menu_globe_music; ac81 bx = fn_0d917_noop (the
        //   no-op cleanup, modelled by the MusicCdOrderMenu identity);
        //   ac84..ac8b cl = (music_playlist_flags & 2) >> 1 — the slot to
        //   pre-highlight: 0 STANDARD ORDER, 1 SHUFFLE.
        // Stage menu_globe_music from its template with the pre-highlight
        // applied (clearing any highlight a previous open left behind).
        self.menu_music.records = menu_defs::MENU_MUSIC.records.to_vec();
        let cl = (self.music_playlist_flags & 2) >> 1;
        // = seg000:ac8d jmp loc_0d32f — request the panel transition, insert
        //   the submenu element (draw_command_menu pre-highlights record cl),
        //   and fold it onto the screen.
        self.screen_overlay_request_transition();
        self.menu_stack_push(MenuRef::MenuMusic, None, cl);
        self.play_pending_panel_fold();
    }

    // = seg000:ac97 menu_callback_choice_music_cd_order_standard — the
    // submenu's STANDARD ORDER choice: CD mode without shuffle, with the
    // pristine order recopied over the working playlist.
    pub(crate) fn menu_callback_choice_music_cd_order_standard(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:ac97 or [music_playlist_flags],1; ac9c and 0fdh.
        self.music_playlist_flags = (self.music_playlist_flags | 1) & !2;
        // = seg000:aca1..acac rep movsb — music_cd_standard_order (9 bytes)
        //   over music_cd_playlist (the port copies the terminator too; DOS's
        //   9-byte copy leaves the working copy's own 0xff in place).
        self.music_cd_playlist = crate::music::MUSIC_CD_STANDARD_ORDER;
        // = seg000:acae falls into music_cd_start_selected_order.
        self.music_cd_start_selected_order();
    }

    // = seg000:ac90 menu_callback_choice_music_cd_order_shuffle — the submenu's
    // SHUFFLE choice: CD mode with the playlist reshuffled on every restart.
    pub(crate) fn menu_callback_choice_music_cd_order_shuffle(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:ac90 or [music_playlist_flags],3.
        self.music_playlist_flags |= 3;
        // = seg000:ac95 jmp music_cd_start_selected_order.
        self.music_cd_start_selected_order();
    }

    // = seg000:acae music_cd_start_selected_order — the shared tail of the two
    // order choices: stop the current song, close both the submenu and the
    // mixer panel, clear the music-off toggle, and start the playlist.
    fn music_cd_start_selected_order(&mut self) {
        // = seg000:acae call midi_reset.
        self.midi.midi_reset();
        // = seg000:acb1 call screen_element_stack_pop_and_redraw — pop the
        //   CD-order submenu and redraw the mixer menu beneath (the port's
        //   pop_and_cleanup: MusicCdOrderMenu's cleanup is a no-op, matching
        //   the DOS fn_0d917_noop it was inserted with).
        self.menu_stack_pop_and_cleanup();
        // = seg000:acb4 call menu_callback_choice_exit_menu — close the mixer
        //   panel itself.
        self.menu_callback_choice_exit_menu(0, 0);
        // = seg000:acb7 and [cmd_args_memory],0efh — music is on again.
        self.cmd_args_memory &= !0x10;
        // = seg000:acbc jmp music_cd_playlist_restart (loc_0ad21).
        self.music_cd_playlist_restart();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{
        GameState, MusicMode,
        dat_file::DatFile,
        menu_defs::{CMD_HIGHLIGHT, MenuRef},
        music::MUSIC_CD_STANDARD_ORDER,
    };

    // The mixer panel's music verbs (MENU_MIXER_PANEL / MENU_MUSIC):
    // MUSIC OFF (seg000:aeaf) sets cmd_args_memory bit 4, closes the mixer and
    // silences the song; MUSIC ON GAME RELATIVE (seg000:ac6e) clears the bit
    // and re-picks the situation song; MUSIC ON CD-STYLE (seg000:ac7e) opens
    // the CD-order submenu whose STANDARD ORDER / SHUFFLE choices (seg000:
    // ac97/ac90) arm the playlist and start it, and whose service
    // (music_cd_playlist_service, seg000:ace6) advances it 0xc8 ticks after a
    // song ends. Headless mode defaults to music off (the same bit).
    // Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored music_menu
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn music_menu_verbs_and_headless_default() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        // Headless defaults to music off: the same cmd_args_memory bit MUSIC
        // OFF sets, so check_music_enabled gates every music path.
        assert_eq!(game.cmd_args_memory & 0x10, 0x10, "headless music not off");
        game.start(true);
        assert_eq!(game.midi.current_song(), None, "a song started while off");
        assert_eq!(game.music_desired_song, 0, "a song was picked while off");

        // MUSIC ON (GAME RELATIVE): clears the off bit, closes the mixer, and
        // re-picks the situation song, which the service then starts.
        game.open_mixer_panel();
        assert_eq!(game.get_active_menu_ref(), MenuRef::MenuMixerPanel);
        game.menu_callback_choice_music_on_game_relative(0, 0);
        assert_eq!(game.cmd_args_memory & 0x10, 0);
        assert_eq!(game.music_playlist_flags, 0);
        assert_eq!(game.get_active_menu_ref(), MenuRef::CommandMenuBuf);
        assert_ne!(game.music_desired_song, 0, "no situation song picked");
        game.service_midi_music();
        assert_eq!(game.midi.current_song(), Some(game.music_desired_song));

        // MUSIC OFF: sets the bit, closes the mixer and silences the song.
        game.open_mixer_panel();
        game.menu_callback_choice_music_off(0, 0);
        assert_eq!(game.cmd_args_memory & 0x10, 0x10);
        assert_eq!(game.get_active_menu_ref(), MenuRef::CommandMenuBuf);
        assert_eq!(game.midi.current_song(), None);
        assert!(!game.midi.is_playing(), "the song was not silenced");

        // MUSIC ON (CD-STYLE) opens the order submenu; with the shuffle bit
        // clear the STANDARD ORDER slot is pre-highlighted (cl = 0).
        game.open_mixer_panel();
        game.menu_callback_choice_music_on_cd_style(0, 0);
        assert_eq!(game.get_active_menu_ref(), MenuRef::MenuMusic);
        assert_eq!(game.active_menu_records().len(), 3);
        assert_ne!(game.active_menu_records()[0].text_id & CMD_HIGHLIGHT, 0);

        // STANDARD ORDER: CD mode, the pristine order, both menus closed, the
        // off bit cleared, and the first song (9) playing with the cursor past it.
        game.menu_callback_choice_music_cd_order_standard(0, 0);
        assert_eq!(game.music_playlist_flags, 1);
        assert_eq!(game.cmd_args_memory & 0x10, 0);
        assert_eq!(game.get_active_menu_ref(), MenuRef::CommandMenuBuf);
        assert_eq!(game.music_cd_playlist, MUSIC_CD_STANDARD_ORDER);
        assert_eq!(game.music_cd_playlist_cursor, 1);
        assert_eq!(game.midi.current_song(), Some(9));

        // The CD service (music_cd_playlist_service) advances the playlist
        // only 0xc8 ticks after it first sees the driver idle.
        game.midi.midi_reset(); // the song "ends": status idle, no current song
        game.music_cd_playlist_service(); // stamps the first idle sighting
        assert_eq!(
            game.midi.current_song(),
            None,
            "advanced before the 0xc8-tick debounce"
        );
        game.music_song_end_tick_stamp = (game.game_ticks() as u16).wrapping_sub(0xc8);
        game.music_cd_playlist_service();
        assert_eq!(
            game.midi.current_song(),
            Some(6),
            "did not advance to song 6"
        );
        assert_eq!(game.music_cd_playlist_cursor, 2);

        // SHUFFLE: flags 3, the playlist a permutation of the nine songs with
        // the terminator intact, playing the (new) head of the list.
        game.open_mixer_panel();
        game.menu_callback_choice_music_on_cd_style(0, 0);
        game.menu_callback_choice_music_cd_order_shuffle(0, 0);
        assert_eq!(game.music_playlist_flags, 3);
        let mut sorted = game.music_cd_playlist[..9].to_vec();
        sorted.sort();
        assert_eq!(sorted, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(game.music_cd_playlist[9], 0xff);
        assert_eq!(game.music_cd_playlist_cursor, 1);
        assert_eq!(game.midi.current_song(), Some(game.music_cd_playlist[0]));
        assert_eq!(game.get_active_menu_ref(), MenuRef::CommandMenuBuf);

        // Re-opening the submenu now pre-highlights SHUFFLE (cl = 1); Cancel
        // closes both menus and changes nothing.
        game.open_mixer_panel();
        game.menu_callback_choice_music_on_cd_style(0, 0);
        assert_ne!(game.active_menu_records()[1].text_id & CMD_HIGHLIGHT, 0);
        let flags = game.music_playlist_flags;
        let song = game.midi.current_song();
        game.menu_callback_choice_music_cd_order_cancel(0, 0);
        assert_eq!(game.music_playlist_flags, flags);
        assert_eq!(game.midi.current_song(), song);
        assert_eq!(game.get_active_menu_ref(), MenuRef::CommandMenuBuf);
    }

    // = seg000:ad50 play_music_WORMSUIT_HSQ — the cutscene score reaches the
    // driver through the same gated midi_play_song (seg000:ad55 jmp, whose
    // ad97 check_music_enabled is the gate) as every other song start, so
    // MUSIC OFF keeps it silent. Its in-game callers are the desert collapse
    // (seg000:0e77) and the book's credits page (loc_00a09). Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored music_off_silences
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn music_off_silences_the_wormsuit_cutscene_score() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless(); // music off, the same bit MUSIC OFF sets
        game.start(true);

        game.play_music_wormsuit_hsq();
        assert_eq!(
            game.midi.current_song(),
            None,
            "the cutscene score started with music off"
        );

        // MUSIC ON (GAME RELATIVE) and it plays: WORMSUIT is song 3.
        game.open_mixer_panel();
        game.menu_callback_choice_music_on_game_relative(0, 0);
        game.play_music_wormsuit_hsq();
        assert_eq!(game.midi.current_song(), Some(3));

        // MUSIC OFF again silences the next cutscene.
        game.open_mixer_panel();
        game.menu_callback_choice_music_off(0, 0);
        game.play_music_wormsuit_hsq();
        assert_eq!(game.midi.current_song(), None);
    }

    // The `--music` startup modes (set_music_mode + apply_pending_music_mode):
    // `Disabled` is no MIDI card, `Off` is the MUSIC OFF verb with a card
    // present, and a playlist mode survives start()'s seg000:0019 reset of
    // music_playlist_flags — which is why the mode is held pending rather than
    // written when it is chosen. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored music_startup_modes
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn music_startup_modes_survive_start() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        if DatFile::open(dat_path).is_err() {
            eprintln!("skipping: {dat_path} not found");
            return;
        }
        // A game launched with `--music <mode>`: the mode is chosen before
        // start(), as main.rs does.
        let started = |mode: MusicMode| {
            let dat_file = DatFile::open(dat_path).expect("open DUNE.DAT");
            let (tx, _rx) = mpsc::sync_channel(64);
            let mut game = GameState::new(dat_file, tx);
            game.set_headless();
            game.set_music_mode(mode);
            // A card-present mode just re-enabled the MIDI output; this test is
            // about the flags, so keep the run itself silent.
            game.midi.set_enabled(false);
            game.start(true);
            game
        };

        // Disabled — no card. Nothing plays, and no mixer verb can conjure one.
        let mut game = started(MusicMode::Disabled);
        assert_eq!(game.settings_flags & 0x100, 0, "a card is present");
        game.play_music_wormsuit_hsq();
        assert_eq!(game.midi.current_song(), None);
        game.open_mixer_panel();
        game.menu_callback_choice_music_on_game_relative(0, 0);
        game.play_music_wormsuit_hsq();
        assert_eq!(
            game.midi.current_song(),
            None,
            "MUSIC ON started a song with no MIDI card"
        );

        // Off — the card is there, the MUSIC OFF bit is armed. Silent in game,
        // and MUSIC ON in the mixer brings the music back.
        let mut game = started(MusicMode::Off);
        assert_eq!(game.settings_flags & 0x100, 0x100, "no card present");
        assert_eq!(game.cmd_args_memory & 0x10, 0x10, "MUSIC OFF not armed");
        game.play_music_wormsuit_hsq();
        assert_eq!(game.midi.current_song(), None);
        game.open_mixer_panel();
        game.menu_callback_choice_music_on_game_relative(0, 0);
        game.play_music_wormsuit_hsq();
        assert_eq!(
            game.midi.current_song(),
            Some(3),
            "MUSIC ON left the music off with a card present"
        );

        // A playlist mode reaches gameplay intact, past the seg000:0019 reset.
        let game = started(MusicMode::CdStandard);
        assert_eq!(
            game.music_playlist_flags, 1,
            "start() wiped the startup playlist mode"
        );
        assert_eq!(game.cmd_args_memory & 0x10, 0, "the music is off");
        assert_eq!(game.music_cd_playlist, MUSIC_CD_STANDARD_ORDER);
    }
}
