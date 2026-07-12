//! The in-game mixer / settings panel (the CD release's audio overlay).
//!
//! Ported from `menu_callback_choice_mixer_panel` (seg000:a3f0) and its draw /
//! interaction helpers (seg000:a3f9..a671, plus the cleanup loc_0a541). The
//! panel is a MIXR.HSQ background with three volume sliders (PCM / music /
//! voice), one stereo balance/pan knob below each slider, and a button grid for
//! the voice-subtitle mode + language. It is shown as a screen-element overlay
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
//! Still deferred: the music-playlist (jukebox) controls (loc_0ac3a).

use crate::{
    GameState, Rect,
    rect::rect,
    room_game_screen::{CMD_HIGHLIGHT, MENU_MIXER_PANEL, ScreenElement, grey_if},
    sprite_bank,
};

/// One 8-byte settings record (= the seg001 slider/knob layout at
/// 288e/2896/289e and 28a6/28ae/28b6): `[value:u8, drawn_flag:u8, dx:u16,
/// screen_y:u16, apply_ofs:u16]`. `value` and `drawn_flag` are mutated as the
/// panel is drawn and dragged; the other three are static layout. `[si+6]`
/// (`apply_ofs`) is the seg000 offset of the audio-apply callback.
#[derive(Clone, Copy)]
pub(crate) struct SettingsRecord {
    /// = `[si+0]` — the 0..0xf0 slider/indicator value byte.
    pub value: u8,
    /// = `[si+1]` — set to 1 once drawn; gates the drag hit-test (loc_0a685).
    pub drawn_flag: u8,
    /// = `[si+2]` — panel-local x of the slider track / indicator.
    pub dx: i16,
    /// = `[si+4]` — panel-local y of the handle, recomputed each draw for the
    /// volume sliders; static for the balance knobs.
    pub screen_y: i16,
    /// = `[si+6]` — seg000 offset of the audio-apply callback the drag commits
    /// (loc_0a637 PCM voices / loc_0a650 MIDI music / loc_0d917 no-op),
    /// dispatched by `settings_ui_apply`.
    pub apply_ofs: u16,
}

const fn sr(value: u8, drawn_flag: u8, dx: i16, screen_y: i16, apply_ofs: u16) -> SettingsRecord {
    SettingsRecord {
        value,
        drawn_flag,
        dx,
        screen_y,
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
    sr(0xff, 0, 0x0c, 0x22, 0xa637), // 288e VOICES (digital PCM) volume
    sr(0xe6, 0, 0x32, 0x22, 0xa650), // 2896 MUSIC (MIDI) volume
    sr(0xb4, 0, 0x58, 0x22, 0xd917), // 289e MUSIC during voices (MIDI duck level)
    sr(0x64, 0, 0x11, 0x67, 0xa637), // 28a6 VOICES balance/pan knob
    sr(0x78, 0, 0x37, 0x67, 0xa650), // 28ae MUSIC balance/pan knob (center)
    sr(0x8c, 0, 0x5d, 0x67, 0xd917), // 28b6 MUSIC-during-voices balance/pan knob
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

fn local_xy(x: i16, y: i16) -> (i16, i16) {
    (x - SETTINGS_RECT.x0, y - SETTINGS_RECT.y0)
}

impl GameState {
    // ---- Entry / draw -----------------------------------------------------

    // = seg000:a3f0 menu_callback_choice_mixer_panel — open the in-game mixer /
    // settings panel. Installs the panel's mouse handlers, drains pending UI
    // tasks, then draws the panel (settings_ui_draw, which also inserts it as
    // the active screen element). Wired from dispatch_command_handler's 0xa3f0
    // verb arm. `pub` so headless renders can open the panel directly.
    pub fn open_mixer_panel(&mut self) {
        // = a3f0 mov ax,1ad6h; call loc_0d95e — select the mixer handler table.
        self.active_mouse_handlers = &crate::game_ui::MIXER_MOUSE_HANDLERS;
        // = a3f6 call dismiss_stacked_overlays.
        self.dismiss_stacked_overlays();
        // = a3f9 fall into settings_ui_draw.
        self.settings_ui_draw();
    }

    // = seg000:a3f9 settings_ui_draw — paint the whole panel (MIXR background,
    // sliders, balance knobs, language + voice buttons), then insert it as
    // the active screen element. Also re-entered to repaint after an
    // interaction (loc_0a5db / loc_0a5c8), in which case the panel is already
    // the active element so the insert is a no-op.
    fn settings_ui_draw(&mut self) {
        // = a3f9 push [active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = a400 open MIXR; a406 draw sprite 0 (the panel background) at the
        // global offset.
        self.open_sprite_bank(sprite_bank::MIXR);
        self.draw_active_bank_sprite(0, SETTINGS_RECT.x0, SETTINGS_RECT.y0);
        // = a413 pop [active_seg].
        self.active_fb = saved;
        // = a417 call settings_ui_draw_volume_sliders.
        self.settings_ui_draw_volume_sliders();
        // = a41a call settings_ui_draw_balance_knobs — the stereo balance knobs.
        self.settings_ui_draw_balance_knobs();
        // = a41d call settings_ui_draw_language_buttons.
        self.settings_ui_draw_language_buttons();
        // = a420 call loc_0a44c — the voice/subtitle-mode button.
        self.settings_ui_draw_voice_mode_button();
        // = a423 call loc_0ac3a — the music-playlist element flags (deferred).
        self.settings_ui_update_music_playlist_flags();
        // = a426 mov bx,0a541h; a429 jmp loc_0d32f — insert the panel as the
        // active screen element WITH the command-panel fold transition (bx is the
        // cleanup func loc_0a541, modelled by the MixerPanel identity). loc_0d32f
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
        if self.get_active_screen_element() != ScreenElement::MixerPanel {
            self.screen_element_stack.push(ScreenElement::MixerPanel);
        }
        self.redraw_active_command_menu();

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
        // = a4c6 call check_pcm_enabled; jz skip the PCM slider.
        if self.check_pcm_enabled() {
            // = a4cb si=288e; draw the PCM slider.
            self.settings_ui_draw_slider(0);
            // = a4d1 test settings_flags,4; when clear, reset the drawn flag
            // (data_0288f = 0) so the PCM slider is not draggable.
            if self.settings_flags & 0x4 == 0 {
                self.settings_records[SETTINGS_RECORD_VOLUME_VOICES].drawn_flag = 0;
            }
        }
        // = a4de call loc_0ae28; jz skip the music + voice sliders.
        if self.settings_music_enabled() {
            // = a4e3 si=2896; a4e9 si=289e — the music + voice sliders.
            self.settings_ui_draw_slider(1);
            self.settings_ui_draw_slider(2);
            // = a4ef test settings_flags,400h; when clear, reset both drawn
            // flags (data_02897 = data_0289f = 0).
            if self.settings_flags & 0x400 == 0 {
                self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC].drawn_flag = 0;
                self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC_DURING_VOICES].drawn_flag = 0;
            }
        }
    }

    // = seg000:a502 settings_ui_draw_slider — draw volume slider record `i`: the
    // track sprite (1) at the record's (dx, 0x22) + global offset, then compute
    // the handle's y from the value byte (`((~value) >> 2) + 0x22`), store it
    // into the record's screen_y, and draw the handle sprite (2). Marks the
    // record drawn (drawn_flag = 1) so it becomes draggable.
    fn settings_ui_draw_slider(&mut self, i: usize) {
        // = a502 push [active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = a50a open MIXR.
        self.open_sprite_bank(sprite_bank::MIXR);
        // = a510 dx=record.dx, bx=0x22; add_global_offset — the track position.
        let track_x = self.settings_records[i].dx + SETTINGS_RECT.x0;
        let track_y = 34 + SETTINGS_RECT.y0;
        // = a519 draw sprite 1 (the slider track).
        self.draw_active_bank_sprite(1, track_x, track_y);
        // = a520 al=value; a521 mark drawn (record.drawn_flag = 1).
        let value = self.settings_records[i].value;
        self.settings_records[i].drawn_flag = 1;
        // = a524 ax=~value; a526 al >>= 2; a52a cbw; a52b ax += track_y — the
        // handle's screen y.
        let handle_y = ((!value) >> 2) as i16 + track_y;
        // = a52f ax -= gy; a533 store the handle's panel-local y in record.screen_y.
        self.settings_records[i].screen_y = handle_y - SETTINGS_RECT.y0;
        // = a536 draw sprite 2 (the slider handle) at (track_x, handle_y).
        self.draw_active_bank_sprite(2, track_x, handle_y);
        // = a53c pop [active_seg].
        self.active_fb = saved;
    }

    // = seg000:a47d settings_ui_draw_balance_knobs — draw the stereo balance/pan
    // knobs (records 3..6) gated by settings_flags: bit 0x8 draws the voices
    // knob, bit 0x800 draws the music and music-during-voices knobs.
    fn settings_ui_draw_balance_knobs(&mut self) {
        // = a47d test settings_flags,8.
        if self.settings_flags & 0x8 != 0 {
            // = a485 si=28a6; call settings_ui_draw_balance_knob.
            self.settings_ui_draw_balance_knob(3);
        }
        // = a48b test settings_flags,800h.
        if self.settings_flags & 0x800 != 0 {
            // = a493 si=28ae; a499 si=28b6.
            self.settings_ui_draw_balance_knob(4);
            self.settings_ui_draw_balance_knob(5);
        }
    }

    // = seg000:a49c settings_ui_draw_balance_knob — draw one balance/pan knob:
    // its needle sprite is `value / 10 + 3` (aam 0ah), 25 frames sweeping
    // left<->right over value 0..0xf0, drawn at the record's (dx, screen_y) +
    // global offset. Marks the record drawn.
    fn settings_ui_draw_balance_knob(&mut self, i: usize) {
        // = a49c push [active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = a4a3 open MIXR.
        self.open_sprite_bank(sprite_bank::MIXR);
        // = a4a9 lodsb value; a4aa aam 0ah; al=value/10; a4b0 add al,3 — sprite.
        let sprite = (self.settings_records[i].value / 10 + 3) as u16;
        // = a4b2 mark drawn (record.drawn_flag = 1).
        self.settings_records[i].drawn_flag = 1;
        // = a4b6 dx=record.dx, bx=record.screen_y; add_global_offset.
        let x = self.settings_records[i].dx + SETTINGS_RECT.x0;
        let y = self.settings_records[i].screen_y + SETTINGS_RECT.y0;
        // = a4be draw the indicator sprite.
        self.draw_active_bank_sprite(sprite, x, y);
        // = a4c1 pop [active_seg].
        self.active_fb = saved;
    }

    // = seg000:a42c settings_ui_draw_language_buttons — open MIXR, then draw the
    // current language_setting's button (loc_0a435).
    fn settings_ui_draw_language_buttons(&mut self) {
        // = a42f open MIXR.
        self.open_sprite_bank(sprite_bank::MIXR);
        // = a432 al = language_setting; fall into loc_0a435.
        self.settings_ui_draw_button(self.language_setting);
    }

    // = seg000:a44c loc_0a44c — draw the voice/subtitle-mode button: input
    // `voice_subtitle_mode + 8` into the shared button draw (loc_0a435). Relies
    // on MIXR already being the active bank (the preceding language-button draw).
    fn settings_ui_draw_voice_mode_button(&mut self) {
        // = a44c al = voice_subtitle_mode + 8; jmp loc_0a435.
        self.settings_ui_draw_button(self.voice_subtitle_mode + 8);
    }

    // = seg000:a435 loc_0a435 — draw a panel button for input `al`: its sprite
    // is `al * 2 + 0x1c`, drawn at the position loc_0a465 computes from `al`.
    fn settings_ui_draw_button(&mut self, al: u8) {
        // = a435 push [active_seg]; set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = a43d call loc_0a465 — the draw position (al preserved across it).
        let (x, y) = self.settings_ui_button_pos(al);
        // = a440 shl ax,1; add al,1ch — the button sprite.
        let sprite = (al as u16) * 2 + 0x1c;
        // = a444 draw the button sprite.
        self.draw_active_bank_sprite(sprite, x, y);
        // = a447 pop [active_seg].
        self.active_fb = saved;
    }

    // = seg000:a465 loc_0a465 — compute a panel button's draw position for input
    // `al`: x = button_grid_rect.x0 + gx; y = sprite_xlat[al] * 7 +
    // button_grid_rect.y0 + gy. The input `al` itself is preserved (the DOS
    // push/pop ax), so loc_0a435 can still derive the sprite from it.
    fn settings_ui_button_pos(&self, al: u8) -> (i16, i16) {
        // = a466 dx = [data_028c7] = button_grid_rect.x0.
        let x = SETTINGS_BUTTON_GRID_RECT.x0 + SETTINGS_RECT.x0;
        // = a46a xlat through settings_language_sprite_xlat; a46e *7; a474 add
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
        // = a576 di=2886; call loc_0d6fe — rect-test the panel.
        if !SETTINGS_RECT.contains_interior(x, y) {
            // = a57e jmp menu_callback_choice_exit_menu — a miss closes the panel.
            self.menu_callback_choice_exit_menu();
            return;
        }
        // = a581 fall into loc_0a581 — interact with the panel interior.
        self.mixer_panel_click_interior(x, y);
    }

    // = seg000:a581 loc_0a581 — dispatch an interior click. Re-base to
    // panel-local coords, then test the test-voice button (28bf), the button
    // grid (28c7), and finally the slider handles (loc_0a594).
    fn mixer_panel_click_interior(&mut self, x: i16, y: i16) {
        // = a581 call settings_ui_sub_global_offset — panel-local coords.
        let (lx, ly) = local_xy(x, y);
        // = a584 di=28bf; loc_0d6fe — the test-voice button.
        if SETTINGS_VOICE_RECT.contains_interior(lx, ly) {
            // = a58a jb loc_0a553.
            self.settings_ui_play_test_voice();
            return;
        }
        // = a58c di=28c7; loc_0d6fe — the button grid.
        if SETTINGS_BUTTON_GRID_RECT.contains_interior(lx, ly) {
            // = a592 jb loc_0a5b0.
            self.mixer_panel_button_grid_click(ly);
            return;
        }
        // = a594 fall into loc_0a594 — grab a slider handle.
        self.mixer_panel_set_drag_target(lx, ly);
    }

    // = seg000:a594 loc_0a594 — record which slider group the click grabbed.
    // The drag motion re-finds the exact handle each frame, so a plain click only
    // arms the group; the value moves once the pointer is dragged.
    fn mixer_panel_set_drag_target(&mut self, lx: i16, ly: i16) {
        self.settings_ui_grab_handle(lx, ly);
    }

    // = seg000:a594 loc_0a594 (returning what loc_0a672/loc_0a69f leave in
    // si/ax/bp) — find the handle under the panel-local pointer and set
    // settings_drag_target: 1 for a volume slider (records 0..3, 22 x 5 box,
    // loc_0a672), 2 for a balance knob (records 3..6, 13 x 11 box,
    // loc_0a69f), 0 for neither. Returns `(group, index, rx, ry)` where rx/ry are
    // the pointer's offset into the matched handle's box, which the knob drag
    // (loc_0a5df) uses to pick the rotation direction.
    fn settings_ui_grab_handle(&mut self, lx: i16, ly: i16) -> (u8, usize, i16, i16) {
        // = a594 loc_0a672 — the volume slider handles.
        for i in 0..3 {
            if let Some((rx, ry)) = self.settings_handle_hit(i, lx, ly, 22, 5) {
                // = a599 data_028be = 1.
                self.settings_drag_target = 1;
                return (1, i, rx, ry);
            }
        }
        // = a59f loc_0a69f — the balance knob handles.
        for i in 3..6 {
            if let Some((rx, ry)) = self.settings_handle_hit(i, lx, ly, 13, 11) {
                // = a5a4 data_028be = 2.
                self.settings_drag_target = 2;
                return (2, i, rx, ry);
            }
        }
        // = a5aa data_028be = 0 — no handle grabbed.
        self.settings_drag_target = 0;
        (0, 0, 0, 0)
    }

    // = seg000:a685 loc_0a685 / a6b2 loc_0a6b2 — slider-handle hit-test: the
    // record must be drawn (drawn_flag == 1), and the panel-local pointer must
    // fall in the `w` x `h` box anchored at the record's (dx, screen_y). The
    // volume sliders use a 0x16 x 5 box, the balance knobs 0x0d x 0x0b.
    // Returns the pointer's `(rx, ry)` offset into the box on a hit.
    fn settings_handle_hit(
        &self,
        i: usize,
        lx: i16,
        ly: i16,
        w: i16,
        h: i16,
    ) -> Option<(i16, i16)> {
        let r = &self.settings_records[i];
        // = a685 cmp byte[si+1],1 — require the record drawn.
        if r.drawn_flag != 1 {
            return None;
        }
        // = a68c ax = lx - dx; a691 bp = ly - screen_y; a696/a69b range checks
        // (unsigned, so a pointer above/left of the box wraps high and misses).

        if !rect(r.dx, r.screen_y, r.dx + w, r.screen_y + h).in_rect(lx, ly) {
            return None;
        }
        Some((lx - r.dx, ly - r.screen_y))
    }

    // = seg000:a5b0 loc_0a5b0 — a button-grid click. Map the row
    // `(local_y - grid.y0) / 7` through the xlat table to either a language
    // selection (< 7), a no-op gap (== 7), or a voice_subtitle_mode (> 7), then
    // redraw the panel (settings_ui_draw).
    fn mixer_panel_button_grid_click(&mut self, ly: i16) {
        // = a5b0 sub bx,[di+2]=grid.y0; a5b5 div 7 — the grid row.
        let row = (ly - SETTINGS_BUTTON_GRID_RECT.y0) / 7;
        // = a5bc xlat through settings_button_grid_xlat.
        let Some(&action) = SETTINGS_BUTTON_GRID_XLAT.get(row as usize) else {
            return;
        };
        // = a5bd cmp al,7.
        if action > 7 {
            // = a5c3 sub al,8; voice_subtitle_mode = al.
            self.voice_subtitle_mode = action - 8;
            // = a5c8 jmp loc_0a5db (settings_ui_draw).
            self.settings_ui_draw();
        } else if action == 7 {
            // = a5c1 jz loc_0a5de — the no-op gap rows.
        } else {
            // = a5ca loc_0a5ca — a language selection.
            // = a5ca cmp al,language_setting; jz ret — unchanged.
            if action == self.language_setting {
                return;
            }
            // = a5d0 and voice_subtitle_mode,0fdh — clear the subtitle bit.
            self.voice_subtitle_mode &= 0xfd;
            // = a5d5 language_setting = al.
            self.language_setting = action;
            // = a5d8 call settings_ui_reload_language — reload the language fonts/strings.
            self.settings_ui_reload_language();
            // = a5db jmp settings_ui_draw.
            self.settings_ui_draw();
        }
    }

    // = the mixer panel's drag handler (cs:[si+0ah] = loc_0a5df), dispatched each
    // pass the LMB is held without an edge and the pointer moved, with the
    // (dx, dy) motion delta. It re-grabs the handle at the *previous* frame's
    // position (current minus the delta) and nudges its value.
    pub(crate) fn mixer_panel_drag(&mut self, dx: i16, dy: i16) {
        // = a5df settings_ui_sub_global_offset — current panel-local pointer.
        let lx = self.mouse_pos_x as i16 - SETTINGS_RECT.x0;
        let ly = self.mouse_pos_y as i16 - SETTINGS_RECT.y0;
        // = a5e2 sub bx,cx — re-base Y to the previous frame's position.
        let prev_ly = ly - dy;
        // = a5e4 call loc_0a594 — re-grab the handle there.
        let (group, i, rx, bp_off) = self.settings_ui_grab_handle(lx, prev_ly);
        match group {
            // = a5ec/a61a a volume slider: move the handle by the Y delta.
            1 => {
                // = a61a jcxz loc_0a619 — no Y motion, no change.
                if dy == 0 {
                    return;
                }
                // = a61c ax = screen_y + dy - 0x22; a624 cmp ax,40h; jnb ret.
                let raw = self.settings_records[i].screen_y + dy - 0x22;
                if !(0..64).contains(&raw) {
                    return;
                }
                // = a629 ax <<= 2; a62d not ax; a62f record.value = al.
                self.settings_records[i].value = !((raw << 2) as u8);
                // = a631 push [si+6]; a634 jmp settings_ui_draw_slider — redraw +
                // run the audio-apply hook (bracketed by the cursor lift).
                self.settings_ui_commit_drag(i, false);
            }
            // = a5ee a balance knob: turn it, nudging the value by +/-0x0a per
            // the 2D (rotary) drag direction.
            2 => {
                // = a5f5 if (lx - dx) < 6, negate the X delta's contribution (cx).
                let cx = if rx < 6 { -dy } else { dy };

                // = a5fc if (prev_ly - screen_y) >= 5, negate di.
                let di = if bp_off >= 5 { -dx } else { dx };

                // = a603 step = +0x0a when (cx + di) >= 0, else -0x0a.
                let step: i8 = if cx + di >= 0 { 0x0a } else { -0x0a };
                // = a60b al = record.value + step; a60d cmp al,0f1h; jnb ret.
                let new_value = self.settings_records[i]
                    .value
                    .saturating_add_signed(step)
                    .min(240);
                // = a611 record.value = al; a613 push [si+6]; a616 jmp
                // settings_ui_draw_balance_knob.
                self.settings_records[i].value = new_value;
                self.settings_ui_commit_drag(i, true);
            }
            // = a619 loc_0a619 — no handle grabbed, nothing to move.
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
        // = a541 voice_subtitle_mode_default = voice_subtitle_mode.
        self.voice_subtitle_mode_default = self.voice_subtitle_mode;
        // = a547 call clear_some_mouse_rect.
        self.clear_some_mouse_rect();
        // = a54a call select_room_ui_table — restore the room handlers.
        self.select_room_ui_table();
        // = a54d si=2886; jmp draw_head_if_needed_and_update_screen_rect_at_si.
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
    fn check_pcm_enabled(&self) -> bool {
        self.settings_flags & 0x1 != 0
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
        // = a637 test settings_flags,4; when clear, force the value to 0xff.
        if self.settings_flags & 0x4 == 0 {
            self.settings_records[SETTINGS_RECORD_VOLUME_VOICES].value = 0xff;
        }
        // = a644 al = record[0].value (level), ah = record[3].value (the voices
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
        // = a650 test settings_flags,400h; when clear, force music + voice
        // values to 0xff.
        if self.settings_flags & 0x400 == 0 {
            self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC].value = 0xff;
            self.settings_records[SETTINGS_RECORD_VOLUME_MUSIC_DURING_VOICES].value = 0xff;
        }
        // = a660 al = record[1].value (music level); a667 clamp al >= 4; ah =
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
        // = a553 call check_pcm_enabled; jz ret.
        if !self.check_pcm_enabled() {
            return;
        }
        // = a558 ax=4, bx=5; create_voc_file_name_from_bx (seg000:a8bc) — build
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
        // = a561 voc_get_lipsync_data — load the clip; bail if the resource is
        // absent (e.g. a DAT without narration), matching DOS's open failure.
        let Ok(data) = self.dat_file.read(&name) else {
            return;
        };
        // = a564 midi_duck_music_volume — drop the score to the "music during
        // voices" level so the test line is audible over it.
        self.midi_duck_music_volume();
        // = a567 is_voc_pcm_playing=1; a56c si=3811h; a56f [pcm_vtable_start_
        // playback] — start the clip on the single dnsdb driver.
        self.pcm_player.stop();
        self.pcm_player.start_playback(&data, 0);
        // = a573 jmp loc_0aba9 — DOS blocks, pumping frame_task_callback_0ab92
        // until the clip drains and it restores the music. The port's mixer
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
    // `jmp loc_0d32f` tail of settings_ui_draw) installs it; the flattened port
    // builds the record set into command_menu_records instead, which the tail's
    // redraw_active_command_menu then paints (staged to fb1 for the panel fold).
    //
    // It also computes the `cl` pre-highlight DOS passes to draw_command_menu
    // (loc_0d393): the slot of the menu's currently-selected entry, marked with
    // CMD_HIGHLIGHT so redraw_active_command_menu draws it inverse. The persistent
    // bit lives in the record's text_id, so it coexists with the transient hover
    // highlight (highlight_hovered_text_action_item reads slot_text_id, preserving
    // it). cl is 0xff (no highlight) when music is disabled.
    //
    // The MUSIC OFF/ON click handlers (which would flip cmd_args bit 0x10 /
    // music_playlist_flags and so move this highlight) and the jukebox playback are
    // still stubbed, so the highlight reflects the default state (game-relative).
    pub(crate) fn settings_ui_update_music_playlist_flags(&mut self) {
        // = ac4b call loc_0ae28 — grey all three MUSIC entries (ac3d..ac45 set
        //   the 0x40 bit) unless music is enabled, in which case ac50..ac58 clear
        //   it again.
        let music_enabled = self.settings_music_enabled();
        let disabled = !music_enabled;
        self.command_menu_records = vec![
            grey_if(MENU_MIXER_PANEL[0], disabled),
            grey_if(MENU_MIXER_PANEL[1], disabled),
            grey_if(MENU_MIXER_PANEL[2], disabled),
            MENU_MIXER_PANEL[3],
            MENU_MIXER_PANEL[4],
        ];

        // = ac49 cl = 0xff (no pre-highlight); ac4e jz loc_0ac6d — when music is
        //   disabled the entries stay greyed and none is highlighted.
        if music_enabled {
            // = ac5c xor cx,cx; ac5e test cmd_args_memory,10h.
            let cl = if self.cmd_args_memory & 0x10 != 0 {
                // = ac63 jnz loc_0ac6d with cl = 0 — music is off: highlight MUSIC
                //   OFF (slot 0).
                0
            } else {
                // = ac65 cl = (music_playlist_flags & 1) + 1 — the active MUSIC ON
                //   variant: GAME RELATIVE (slot 1) or CD-STYLE (slot 2).
                (self.music_playlist_flags & 1) as usize + 1
            };
            // = loc_0d393 or byte ptr [bx+si+3], 80h — set the highlight bit on
            //   entry `cl`'s text_id (cl is 0..2 here, always < 5).
            self.command_menu_records[cl].text_id |= CMD_HIGHLIGHT;
        }
    }

    // ---- Music-menu verb handlers (stubs) ---------------------------------
    //
    // These are the MENU_MIXER_PANEL command-strip verbs. The whole background-
    // music playlist / jukebox feature (music_playlist_flags, the CD-vs-game-
    // relative song selection, and the CD-order sub-menu it opens) is not ported,
    // so each handler is a logging stub for now. EXIT GAME / " Done" are routed to
    // their real handlers (menu_callback_choice_exit_game stub / menu_callback_
    // choice_exit_menu).

    // = seg000:aeaf menu_callback_choice_music_off — MUSIC OFF: disable the background-music playlist.
    // TODO: port the jukebox off path (music_playlist_flags / midi_reset).
    pub(crate) fn menu_callback_choice_music_off(&mut self) {
        println!("menu_callback_choice_music_off (seg000:aeaf): TODO — music playlist off");
    }

    // = seg000:ac6e menu_callback_choice_music_on_game_relative — MUSIC ON (GAME RELATIVE): play the song tied to
    // the current game state. TODO: port the game-relative playlist mode.
    pub(crate) fn menu_callback_choice_music_on_game_relative(&mut self) {
        println!(
            "menu_callback_choice_music_on_game_relative (seg000:ac6e): TODO — game-relative music"
        );
    }

    // = seg000:ac7e menu_callback_choice_music_on_cd_style — MUSIC ON (CD-STYLE): play the CD-jukebox order
    // (opens the standard/shuffle/cancel sub-menu). TODO: port the CD-order menu.
    pub(crate) fn menu_callback_choice_music_on_cd_style(&mut self) {
        println!("menu_callback_choice_music_on_cd_style (seg000:ac7e): TODO — CD-style music");
    }
}
