//! The in-game room game-screen presenter.
//!
//! Ported from `draw_room_game_screen` (seg000:2db1), the routine that paints
//! the whole playable room screen: the top HUD strip, the bottom command /
//! dialogue panel, the room scene, and the character portrait, then reveals it
//! (fade or straight blit) and kicks off any pending dialogue / lip-sync.
//!
//! This is a faithful control-flow skeleton. The render/setup helpers it calls
//! are not ported yet, so they are no-op stubs below, each linked to its DOS
//! address; with the default state flags the routine follows the normal
//! room-view path. The flag state lives on `GameState` (see lib.rs).

use crate::{
    Equipment, GameState, Location, attack::AttackState, game_ui::NAV_PANEL_MIRROR, gfx,
    sprite_bank,
};

/// One verb-menu record — a 4-byte entry of the `command_menu_buf` list
/// (seg001:1f0e). = the `[text_id:u16, handler_ofs:u16]` pair the seg001
/// command-record templates store and build_room_command_records copies out.
#[derive(Clone, Copy)]
pub(crate) struct CommandMenuRecord {
    /// COMMAND/PHRASE string id, OR-ed with the state bits the builder sets:
    /// 0x4000 = greyed/disabled, 0x8000 = highlighted. The low 0x3fff resolve
    /// the text via get_phrase_or_command_string_si.
    pub text_id: u16,
    /// seg000 offset of the verb's click/action handler. Stored raw; the click
    /// dispatcher is gameplay code not yet ported, so nothing reads it yet.
    pub handler: u16,
}

const fn rec(text_id: u16, handler: u16) -> CommandMenuRecord {
    CommandMenuRecord { text_id, handler }
}

// = the seg001 command-record templates (seg001:21dc..221c), each a 4-byte
// [text_id:u16, handler_ofs:u16]. build_room_command_records copies these into
// the verb list, sometimes OR-ing a 0x4000 "greyed" bit into the text_id.
//
// The trailing phrase comment on each line is the COMMAND.BIN string the
// text_id resolves to (= the get_phrase_or_command_string_si path: value-1
// indexes the offset table; COMMAND1.TXT lines are 1-indexed by text_id).

// = 21dc: "TAKE AN ORNITHOPTER" — appended on the special-room dl==1 path
// (the location's entry room) when the night-attack stage is not active.
// Greyed until orni_count >= 1.
const CMD_TAKE_ORNITHOPTER: CommandMenuRecord = rec(0x00a7, 0x42e9);
// = 21e0: "WAIT FOR EVENING" — plain-room time-skip verb when the in-game
// time-of-day phase is < 0x0b (i.e. before evening).
const CMD_WAIT_FOR_EVENING: CommandMenuRecord = rec(0x00a5, 0x0f48);
// = 21e4: "WAIT FOR MORNING" — plain-room time-skip verb when the in-game
// time-of-day phase is >= 0x0b (i.e. evening/night).
const CMD_WAIT_FOR_MORNING: CommandMenuRecord = rec(0x00a6, 0x0f67);
// = 21e8: "VIEW NEW MESSAGES" — the palace communications-room verb
// (bh==1, dl==8) for reading newly-received transmissions; gated on
// data_000c8 != 0 (a new message is queued).
const CMD_VIEW_NEW_MESSAGES: CommandMenuRecord = rec(0x00d7, 0x283a);
// = 21ec: "Messages already seen" — the communications-room companion
// verb to CMD_VIEW_NEW_MESSAGES (replay previously-viewed messages).
const CMD_VIEW_OLD_MESSAGES: CommandMenuRecord = rec(0x00d8, 0x283e);
// = 21f0: "LOOK AT MIRROR" — the palace bedroom verb (bh==1, dl==9; Paul's
// room with the mirror).
const CMD_LOOK_AT_MIRROR: CommandMenuRecord = rec(0x0099, 0x0ea6);
// = 21f4: "Mixer Panel" — the always-available audio mixer-panel verb,
// appended at the tail of the special-room and plain-room verb lists. The
// CD release of Dune exposes its in-game music/voice mixer here.
const CMD_MIXER_PANEL: CommandMenuRecord = rec(0x009e, 0xa3f0);
// = 21f8: "CHANGE DESTINATION" — the map/book-mode travel verb (the third
// slot in both map sub-modes).
const CMD_CHANGE_DESTINATION: CommandMenuRecord = rec(0x0058, 0x497a);
// = 21fc: "SKIP TO DESTINATION" — the default map-mode verb when the
// phase-gated alternates do not apply (data_011cb == 0 || game_phase < 0x32).
const CMD_SKIP_TO_DESTINATION: CommandMenuRecord = rec(0x00a9, 0x4ffb);
// = 2200: "BACK TO STARTING POINT" — the first phase-gated map-mode verb
// (data_011cb != 0 && game_phase >= 0x32).
const CMD_BACK_TO_STARTING_POINT: CommandMenuRecord = rec(0x00ac, 0x50a5);
// = 2204: "TOWARDS NEAREST PLACE" — the second phase-gated map-mode verb
// (same gate as CMD_BACK_TO_STARTING_POINT).
const CMD_TOWARDS_NEAREST_PLACE: CommandMenuRecord = rec(0x00aa, 0x50c4);
// = 220c: "SEE DUNE MAP" — the leading verb on every special-room and
// plain-room verb list (opens the planet-map view).
const CMD_SEE_DUNE_MAP: CommandMenuRecord = rec(0x0098, 0x186b);
// = 2214: "CALL A WORM" — the worm-summon verb. Greyed until game_phase
// >= 0x4f. Appears on plain rooms and on the night-attack sietch (dl==1).
const CMD_CALL_A_WORM: CommandMenuRecord = rec(0x00a8, 0x42d1);
// = 2218: "MASSIVE ATTACK" — the first night-attack stage verb (special
// room dl==1 with night_attack_stage != 0).
const CMD_MASSIVE_ATTACK: CommandMenuRecord = rec(0x009a, 0x7317);
// = 221c: "FIGHT FOR A WHOLE DAY" — the second night-attack stage verb,
// adjacent to CMD_MASSIVE_ATTACK.
const CMD_FIGHT_FOR_A_WHOLE_DAY: CommandMenuRecord = rec(0x009b, 0x0fc5);

/// The greyed/disabled flag the builder ORs into a verb's text_id when its
/// precondition is unmet (= the `and ah,40h` writes; loc_0d48a draws it in the
/// 0xf5 "disabled" colour). The low 0x3fff still selects the string.
const CMD_GREY: u16 = 0x4000;

/// The highlight flag in a verb's text_id (= the bit draw_command_menu_item's
/// loc_0d4d6 swaps fg/bg for). Set transiently on the hovered slot, and
/// persistently by `draw_command_menu`'s `cl` argument (loc_0d393) to mark a
/// menu's currently-selected entry — e.g. the active MUSIC ON variant.
pub(crate) const CMD_HIGHLIGHT: u16 = 0x8000;

/// Apply [`CMD_GREY`] to `r`'s text_id when `disabled` — the common
/// `cmp …; sbb ah,ah; and ah,40h; stosw` idiom in build_room_command_records.
pub(crate) const fn grey_if(r: CommandMenuRecord, disabled: bool) -> CommandMenuRecord {
    if disabled {
        CommandMenuRecord {
            text_id: r.text_id | CMD_GREY,
            handler: r.handler,
        }
    } else {
        r
    }
}

/// = seg001:20c2 menu_palace_mirror_room — the static command-menu record
/// buffer installed as the active verb menu while the LOOK AT MIRROR still is
/// up (callback_transition_look_at_mirror, seg000:0eff bp=20c2h). Unlike the
/// per-room list build_room_command_records assembles, it is fixed. DOS stores
/// a leading priority word (0x00ff: priority byte 0xff, header skip byte 0) and
/// a trailing 0-word terminator that the flat port models implicitly. Each row
/// is the COMMAND.BIN string the text_id resolves to and its menu handler.
#[rustfmt::skip]
const MENU_PALACE_MIRROR_ROOM: [CommandMenuRecord; 5] = [
    rec(0x00ba, 0x0e47), // RESTART GAME              menu_callback_choice_multiple_restart_game
    rec(0x00b4, 0xb29e), // LOAD GAME                 menu_callback_choice_mirror_room_load_game
    rec(0x00b3, 0xb28c), // SAVE GAME                 menu_callback_choice_mirror_room_save_game
    rec(0x00bb, 0x0e3e), // EXIT GAME                 menu_callback_choice_exit_game
    rec(0x009d, 0x0eb9), // Look away from the mirror menu_callback_choice_palace_look_away_from_mirror
];

/// = seg001:201a menu_mixer_panel — the static command-menu record buffer the
/// mixer installs as the active verb strip while it is open. settings_ui_update_
/// music_playlist_flags (seg000:ac3a) sets bp = menu_mixer_panel and leaves it
/// there, so the following screen_element_stack_insert (the `jmp loc_0d32f` tail of
/// settings_ui_draw) installs this buffer and the panel fold transitions the strip
/// from the room verbs to these music entries. DOS stores a leading priority word
/// (0x00f8: priority byte 0xf8, header skip byte 0) and a trailing 0-word fence,
/// both modelled implicitly by the flat port. The three MUSIC entries are greyed
/// (CMD_GREY) when music is disabled (= the ac3d..ac58 `or/and [bp+3]/[bp+7]/[bp+0bh]`
/// flag-byte toggles); EXIT GAME and " Done" are always live.
#[rustfmt::skip]
pub(crate) const MENU_MIXER_PANEL: [CommandMenuRecord; 5] = [
    rec(0x010e, 0xaeaf), // MUSIC OFF                menu_callback_choice_music_off
    rec(0x010b, 0xac6e), // MUSIC ON (GAME RELATIVE) menu_callback_choice_music_on_game_relative
    rec(0x010a, 0xac7e), // MUSIC ON (CD-STYLE)      menu_callback_choice_music_on_cd_style
    rec(0x00bb, 0x0e3e), // EXIT GAME                menu_callback_choice_exit_game
    rec(0x00a1, 0xd2e2), // " Done"                  menu_callback_choice_exit_menu
];

/// = seg001:20b6 menu_exit_game_confirmation — the EXIT GAME confirmation submenu
/// menu_callback_choice_exit_game pushes over the active menu. DOS stores a leading
/// priority word (0x00f6) and a trailing 0-word fence, both implicit here. YES
/// quits to the OS (exit_to_dos); NO closes the submenu (menu_callback_choice_
/// exit_menu) and folds back to the menu beneath.
#[rustfmt::skip]
pub(crate) const MENU_EXIT_GAME_CONFIRMATION: [CommandMenuRecord; 2] = [
    rec(0x00b8, 0x003a), // YES I WANT TO EXIT GAME  exit_to_dos
    rec(0x00b9, 0xd2e2), // NO I WISH TO CONTINUE    menu_callback_choice_exit_menu
];

/// = seg001:2012 menu_done — the single-record command strip installed while the
/// full-screen PALACE PLAN overlay is up (ui_draw_palace_plan, bp=menu_done). DOS
/// stores a leading priority word (0x00f8) and a trailing 0-word fence, both
/// implicit here. The lone " Done" button closes the overlay.
#[rustfmt::skip]
pub(crate) const MENU_DONE: [CommandMenuRecord; 1] = [
    rec(0x00a1, 0xd2e2), // " Done"                  menu_callback_choice_exit_menu
];

/// One entry of the seg001:0fd8 room-person table (= the chani `RoomPerson`
/// struct). The DOS layout is 16 bytes; the eight bytes between `handler` and
/// `person_index` are static-zero padding the port does not store.
///
/// GameState owns a 16-entry mutable copy of this table (`room_persons`).
/// Entries 12..16 have their `(location_and_room, location_appearance)` overwritten
/// at runtime: `init_room_persons` resets `location_appearance` to 0x7f80, and the
/// (not-yet-ported) loc_06603 + loc_0316e classification path on the special-
/// room branch writes fresh values that make those entries match the room.
#[derive(Clone, Copy)]
pub(crate) struct RoomPerson {
    /// Matched against `location_and_room` in scan_matching_room_person_entries.
    pub(crate) location_and_room: u16,
    /// Matched against `location_appearance` (data_00006).
    pub(crate) location_appearance: u16,
    /// seg000 offset of the verb's handler — stored as the second word of the
    /// built command-menu record. Like CommandMenuRecord.handler, nothing reads
    /// it yet.
    handler: u16,
    /// 0..15, the bit position OR-ed into persons_in_room and the offset of the
    /// "&Person" text (0x78..0x87) the verb-menu record displays.
    pub(crate) person_index: u8,
    /// Bit 0x40 splits the two scan passes (template loc_030b9 / loc_03120).
    /// Static-data values are 0x00 / 0x02 / 0x80 — bit 0x40 is never set, so
    /// the second pass never matches; the port keeps both passes for fidelity.
    pub(crate) flags: u8,
}

const fn rp(
    location_and_room: u16,
    location_appearance: u16,
    handler: u16,
    person_index: u8,
    flags: u8,
) -> RoomPerson {
    RoomPerson {
        location_and_room,
        location_appearance,
        handler,
        person_index,
        flags,
    }
}

// = the seg001 base address of room_persons. DOS scan_matching_room_person_
// entries stores the matched entry's pointer (0x0fd8 + i * 0x10) in
// data_047aa; build_room_person_record_a reconstructs it from the entry's
// 0..15 index.
pub(crate) const ROOM_PERSON_TABLE_BASE: u16 = 0x0fd8;

// = seg001:0fd8 room_persons — the static initializer of the 16-entry
// room-person table. GameState owns a mutable copy in `room_persons` that
// scan_matching_room_person_entries walks; this constant only seeds it on
// startup. The last four entries' (location_and_room, location_appearance) are
// rewritten at runtime by init_room_persons + the loc_06603 classification.
pub(crate) const ROOM_PERSON_TABLE_INIT: [RoomPerson; 16] = [
    rp(0x200a, 0x0180, 0x92f2, 0x00, 0x02),
    rp(0x2004, 0x0180, 0x92f7, 0x01, 0x02),
    rp(0x2008, 0xff80, 0x92fc, 0x02, 0x02),
    rp(0x2004, 0xff80, 0x9301, 0x03, 0x02),
    rp(0x0002, 0x0d80, 0x9306, 0x04, 0x00),
    rp(0x0402, 0x2e80, 0x930b, 0x05, 0x00),
    rp(0x1002, 0x3f80, 0x9310, 0x06, 0x00),
    rp(0x0503, 0x1b80, 0x9315, 0x07, 0x02),
    rp(0x0703, 0x1180, 0x931a, 0x08, 0x00),
    rp(0x3002, 0x0280, 0x931f, 0x09, 0x80),
    rp(0x3002, 0x0280, 0x9324, 0x0a, 0x80),
    rp(0x3002, 0x0280, 0x9329, 0x0b, 0x80),
    rp(0x3002, 0x0080, 0x932e, 0x0c, 0x00),
    rp(0x3002, 0x0080, 0x936f, 0x0d, 0x00),
    rp(0x3002, 0x0080, 0x9373, 0x0e, 0x00),
    rp(0x0202, 0x0080, 0x937e, 0x0f, 0x80),
];

// = the active screen-element identities the ported click dispatch needs to
// tell apart. DOS keeps a z-ordered stack of [record_buf_ptr, render_func] slots
// (seg001:21da screen_element_stack_ptr / data_021da, room at the bottom, menus
// and overlays on top) and get_active_screen_element (seg000:d41b) returns the
// top buffer pointer; loc_0941d compares it against 0x20c2. The port models only
// the two identities that path distinguishes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ScreenElement {
    // = command_menu_buf (seg001:1f0e) — the room verb menu, the stack bottom.
    RoomCommandMenu,
    // = data_020c2 (seg001:20c2, leading priority byte 0xff) — the "look away
    // from mirror" overlay shown over the MIRROR.HSQ still.
    LookAwayFromMirror,
    // = the in-game mixer / settings panel overlay (menu_callback_choice_mixer_
    // panel, seg000:a3f0). Inserted with cleanup func loc_0a541 (settings_ui_
    // cleanup); its own mouse-handler table (seg001:1ad6) drives interaction.
    MixerPanel,
    // = menu_NPC_actions (seg001:1f7e) — the dialogue verb panel set_dialogue_
    // speaker pushes over the room command menu when a conversation starts (TALK
    // TO ME / a per-NPC verb / STOP TALKING). Its render/cleanup func is menu_npc_actions_cleanup.
    NpcActionsMenu,
    // = menu_exit_game_confirmation (seg001:20b6, leading priority byte 0xf6) — the
    // EXIT GAME confirmation submenu (YES I WANT TO EXIT GAME / NO I WISH TO
    // CONTINUE) menu_callback_choice_exit_game pushes over the mixer or mirror
    // menu. Its cleanup func is nullsub_00f66 (a no-op).
    ExitGameConfirmation,
    // = menu_done (seg001:2012, priority byte 0xf8) — the full-screen PALACE PLAN
    // overlay ui_draw_palace_plan pushes over the room command menu. DOS shares
    // the menu_done header with the unported on-map troop screen; PalacePlan is
    // the only identity the port maps to 0x2012. Its cleanup func is
    // loc_019fc (palace_plan_cleanup).
    PalacePlan,
}

impl ScreenElement {
    // = the leading priority byte of each element's menu/overlay record buffer
    // (`[buf]`, the z-stack key screen_element_stack_insert sorts on and
    // dismiss_stacked_overlays reads to decide what to drain): command_menu_buf
    // (seg001:1f0e) 0xff, menu_NPC_actions (1f7e) 0xfc, menu_mixer_panel (201a)
    // 0xf8, menu_done (2012) 0xf8, menu_exit_game_confirmation (20b6) 0xf6,
    // menu_palace_mirror_room (20c2) 0xff. A low nibble of 0 or a value of 0xff
    // marks a base/locked entry that the transient-overlay drain stops at.
    pub(crate) fn priority_byte(self) -> u8 {
        match self {
            ScreenElement::RoomCommandMenu | ScreenElement::LookAwayFromMirror => 0xff,
            ScreenElement::NpcActionsMenu => 0xfc,
            ScreenElement::MixerPanel | ScreenElement::PalacePlan => 0xf8,
            ScreenElement::ExitGameConfirmation => 0xf6,
        }
    }
}

impl GameState {
    // = seg000:2db1 draw_room_game_screen — present the full in-game room screen.
    // Entered from ui_present_room_screen (when pending_room_screen_request != 0)
    // and from several scene-change sites (seg000:0ecd/13de/1bcf/037aa/9450/b424).
    pub fn draw_room_game_screen(&mut self) {
        // = seg000:2db1 bp = ui_setup_and_draw_nav_panel; draw the top HUD strip
        // offscreen (front buffer redirected to fb1 for the call).
        self.gfx_call_bp_with_front_buffer_as_screen(|s| s.ui_setup_and_draw_nav_panel());
        // = seg000:2db7 call select_room_ui_table.
        self.select_room_ui_table();
        // = seg000:2dba data_047a6 = 0xff.
        self.data_047a6 = 0xff;

        // = seg000:2dbf loc_02dbf — also re-entered to reload the scene.
        // = seg000:2dbf call open_SAL_resource — open the room's scene resource.
        self.sal_open_resource();
        // = seg000:2dc4 clear the in-transition / render / lip-sync-index state.
        self.in_transition = 0;
        self.room_render_flags = 0;
        self.data_047aa = 0;
        // = seg000:2dcd bp = ui_draw_room_command_panel; draw it offscreen.
        self.gfx_call_bp_with_front_buffer_as_screen(|s| s.ui_draw_room_command_panel());

        // = seg000:2dd3 loc_02dd3.
        if self.night_attack_stage != 0 {
            // = seg000:2dda the scripted night-attack scene branch.
            self.data_04732 = 0;
            self.sal_open_resource();
            self.data_011bc |= 1;
            self.sky_fade_active = false;
            self.night_attack_start();
            self.ui_hud_head_draw();
            self.gfx_copy_whole_framebuf_to_screen();
            self.update_screen_palette();
            // Present unless rendering offscreen (transition presents afterwards).
            if !self.front_buffer_is_fb1() {
                self.send_frame_to_display();
            }
            // = seg000:2df8 jmp ui_hud_head_animate_up.
            self.ui_hud_head_animate_up();
            return;
        }

        // = seg000:2dfb loc_02dfb — the normal room render path.
        if self.data_04732 & 1 != 0 {
            // = seg000:2e02 call loc_0488a — draw the extra location overlay SAL.
            self.draw_location_overlay_sal();
        }
        // = seg000:2e05 clear the active speaker and the day/night fade flag.
        self.persons_talking_to = 0;
        self.sky_fade_active = false;
        // = seg000:2e0d render the room: select fb1, lay down the game-area
        // backdrop, then draw the current location/room scene.
        self.set_fb1_as_active_framebuffer();
        self.copy_game_area_rect_to_unknown_rect();
        self.draw_room_scene();
        // = seg000:2e16 unless a non-room mode is active (mask 3), snapshot the
        // clean composed scene into fb2.
        if self.game_screen_mode_flags & 3 == 0 {
            self.copy_active_framebuffer_to_framebuffer_2();
        }
        // = seg000:2e20 advance room music, save the portrait background, draw
        // the head-and-shoulders portrait.
        self.update_room_music();
        self.ui_hud_head_save_rect();
        self.ui_hud_head_draw();
        // = seg000:2e29 reveal the screen. data_046e0 holds the previous
        // sky_fade_active state; when it changed (day<->night) fade in, otherwise
        // just re-flush the palette and blit. The fade is skipped while the front
        // buffer is still redirected to fb1 (offscreen).
        let sky = self.sky_fade_active as u8;
        let prev = self.data_046e0;
        self.data_046e0 = sky;
        if sky == prev {
            // = seg000:2e4c update palette and copy fb1 -> screen.
            self.update_screen_palette();
            self.gfx_copy_whole_framebuf_to_screen();
            // Present unless we are rendering offscreen (front buffer redirected
            // to fb1, e.g. inside transition, which presents afterwards).
            if !self.front_buffer_is_fb1() {
                self.send_frame_to_display();
            }
        } else if !self.front_buffer_is_fb1() {
            // = seg000:2e3f al = 10h, bp = 0f66h (loc_00f66, a no-op render),
            // transition; then service music.
            self.transition(0x10, |_| {});
            self.service_midi_music();
        }

        // = seg000:2e52 loc_02e52 — post-render bookkeeping + dialogue tail.
        self.finish_room_screen_setup();
        // = seg000:2e55 game_clock_tick_base = the current PIT counter.
        self.game_clock_tick_base = self.pit_timer_callback_counter;
        // = seg000:2e5b data_047a7 != 0 suppresses the dialogue/lip-sync tail.
        if self.data_047a7 != 0 {
            return;
        }
        // = seg000:2e62 data_04735 sign bit set -> run the auto-action handler.
        if (self.data_04735 as i8) < 0 {
            // = seg000:2e69 jmp loc_03723.
            self.handle_pending_dialogue_action();
            return;
        }
        // = seg000:2e6c.
        if self.data_00008 != 0xff {
            // = seg000:2e73 a room scene is present; auto-start the head animation
            // unless a dialogue is already active.
            if !self.is_dialogue_active {
                // = seg000:2e7a jmp ui_hud_head_animate_up.
                self.ui_hud_head_animate_up();
            }
            return;
        }
        // = seg000:2e7d only auto-start lip-sync in the plain room mode.
        if self.game_screen_mode_flags != 0 {
            return;
        }
        // = seg000:2e84 data_047aa indexes the persons array; 0 = nobody to voice.
        let si = self.data_047aa;
        if si == 0 {
            return;
        }
        // = seg000:2e8e al = (byte) persons_met[si] — index the contiguous
        // persons array (headed by persons_met) by the byte offset si, then start
        // that speaker's lip-sync. The port stores those persons as separate
        // scalar fields, so the [si] read is not modelled; si is always 0 above,
        // so this path is currently unreachable.
        // TODO: port the persons-array indexing.
        self.current_lip_sync_resource_id = self.persons_met;
        // = seg000:2e94 call start_room_lip_sync.
        self.start_room_lip_sync();
    }

    // = seg000:189a ui_present_room_screen — finish presenting the room screen
    // (also reached from loc_0eca/0fac/2c8c). When pending_room_screen_request is
    // set, jump straight to draw_room_game_screen; otherwise render it through a
    // transition wipe and start the head animation. `transition_effect` is the al
    // the caller falls in with (0x34 from the room-enter path).
    pub(crate) fn ui_present_room_screen(&mut self, transition_effect: u8) {
        // = seg000:189a bp = draw_room_game_screen.
        if self.pending_room_screen_request != 0 {
            // = seg000:18a4 jmp draw_room_game_screen.
            self.draw_room_game_screen();
            return;
        }
        // = seg000:18a6 loc_018a6 — dx = 0; transition renders draw_room_game_
        // screen offscreen (bp) then wipes it onto the screen.
        self.transition(transition_effect, |s| s.draw_room_game_screen());
        // = seg000:18ab set fb1 active; service music; snapshot the clock tick.
        self.set_fb1_as_active_framebuffer();
        self.service_midi_music();
        // = seg000:18b1 game_clock_tick_base = the current PIT counter.
        self.game_clock_tick_base = self.pit_timer_callback_counter;
        // = seg000:18b7 jmp ui_hud_head_animate_up.
        self.ui_hud_head_animate_up();
    }

    // ---- Command / HUD click dispatch -------------------------------------
    //
    // game_ui's room_mouse_lmb -> hit_test_ui_elements -> dispatch_ui_click
    // (seg000:d6b7 / d8d4) already pick the clicked HUD element and route to its
    // handler; the per-element entries below are the targets it dispatches to by
    // func_ptr. (The live game_loop mouse-button edge that would invoke
    // room_mouse_lmb is still TODO, so nothing triggers these from real input
    // yet.)

    // = seg000:d445 dispatch_command_menu_slot (entered from the per-row handlers
    // d443..d42f with cx = `slot`). Read the active menu's record for `slot`
    // (read_command_menu_record_for_slot), and unless it has no handler or is
    // greyed, dispatch it (DOS `jmp bx`).
    pub(crate) fn dispatch_command_menu_slot(&mut self, slot: usize) {
        // = seg000:d454 read_command_menu_record_for_slot — the active screen
        // element's record buffer = command_menu_records[slot]: the room verbs
        // normally, or the mirror menu (MENU_PALACE_MIRROR_ROOM) while the LOOK
        // AT MIRROR still is up. The "Look away from the mirror" row (slot 4,
        // handler 0x0eb9) dispatches through dispatch_command_handler below.
        let Some(rec) = self.command_menu_records.get(slot).copied() else {
            return;
        };
        // = seg000:d448 or bx,bx; jz — no handler.
        if rec.handler == 0 {
            return;
        }
        // = seg000:d44c test ah,40h; jnz — the greyed flag (0x4000 in text_id).
        if rec.text_id & CMD_GREY != 0 {
            return;
        }
        // = seg000:d451 jmp bx — dispatch the verb handler.
        self.dispatch_command_handler(rec.handler);
    }

    // The `jmp bx` target: resolve the verb handler offset to its ported routine.
    // A match (not an `if`) because this is the verb-dispatch table; the TODO arm
    // below is where the remaining handlers slot in.
    fn dispatch_command_handler(&mut self, handler: u16) {
        match handler {
            // = seg000:92f2..9371 the per-character dialogue trampolines, each a
            // `mov al,N; jmp common_code_for_ui_dialogue_related_functions`
            // (seg000:93aa). room_persons[].handler holds the trampoline offset;
            // map it back to the speaker's lip-sync resource index N and run the
            // shared dialogue entry (common_dialogue). The on-screen verb dispatch
            // (callback_main_ui_element_21_22) and the command menu both arrive here.
            0x92f2 => self.common_dialogue(0x0), // Duke Leto Atreides
            0x92f7 => self.common_dialogue(0x1), // Lady Jessica Atreides
            0x92fc => self.common_dialogue(0x2), // Thufir Hawat
            0x9301 => self.common_dialogue(0x3), // Duncan Idaho
            0x9306 => self.common_dialogue(0x4), // Gurney Halleck
            0x930b => self.common_dialogue(0x5), // Stilgar
            0x9310 => self.common_dialogue(0x6), // Liet Kynes
            0x9315 => self.common_dialogue(0x7), // Chani
            0x931a => self.common_dialogue(0x8), // Harah
            0x931f => self.common_dialogue(0x9), // Baron Vladimir Harkonnen
            0x9324 => self.common_dialogue(0xa), // Feyd-Rautha Harkonnen
            0x9329 => self.common_dialogue(0xb), // Emperor Shaddam IV
            0x936f => self.common_dialogue(0xd), // Smugglers
            // = seg000:932e/9373/937e — HarkonnenCaptains / Fremen1 / Fremen2
            // trampolines gate on troop data (troop_prepare_troop_data_for_condit)
            // before reaching the shared tail. TODO: port once the troop system
            // lands; fall through to the no-op below for now.
            0x9472 => self.menu_callback_choice_talk_to_me(),
            // 0x95e2 => self.menu_callback_choice_come_with_me(),
            // 0x9ed5 => self.menu_callback_choice_what(),

            // = seg000:0ea6 loc_00ea6 — LOOK AT MIRROR (palace bedroom, slot 1 /
            // seg000:d43e).
            0x0ea6 => self.look_at_mirror(),
            // = seg000:0eb9 menu_callback_choice_palace_look_away_from_mirror —
            // the "Look away from the mirror" verb (mirror menu slot 4 /
            // data_020d4). DOS jmps straight to 0eb9 and leaves the mirror entry
            // on the screen-element stack: its 0xff priority is locked against
            // screen_element_stack_pop_and_cleanup (which skips priority&0xf==0xf), so draw_room_game_screen
            // just re-pushes the room menu above it. The flattened port has no
            // priority stack, so it pops the overlay to make the room menu active
            // again — the same dismissal game_area_click performs.
            0x0eb9 => {
                self.screen_element_stack.pop();
                self.look_away_from_mirror();
            }
            // = seg000:a3f0 menu_callback_choice_mixer_panel — the always-
            // available "Mixer Panel" verb (CMD_MIXER_PANEL). Opens the in-game
            // audio mixer / settings overlay (settings_ui.rs).
            0xa3f0 => self.open_mixer_panel(),
            // = the mixer panel's music-menu verbs (MENU_MIXER_PANEL), shown in
            // the command strip while the mixer is open. Stubbed: the jukebox /
            // music-playlist feature (music_playlist_flags) is not ported.
            0xaeaf => self.menu_callback_choice_music_off(),
            0xac6e => self.menu_callback_choice_music_on_game_relative(),
            0xac7e => self.menu_callback_choice_music_on_cd_style(),
            // = seg000:0e3e menu_callback_choice_exit_game — the EXIT GAME verb
            // (mixer + mirror menus): opens the YES/NO confirmation submenu.
            0x0e3e => self.menu_callback_choice_exit_game(),
            // = seg000:003a exit_to_dos — the confirmation submenu's YES verb
            // (menu_exit_game_confirmation 0xb8): quit the game to the OS.
            0x003a => self.exit_to_dos(),
            // TODO: the other mirror-menu verbs (RESTART 0x0e47, LOAD 0xb29e,
            // SAVE 0xb28c) and the room verbs (SEE DUNE MAP 0x186b, ornithopter
            // travel, ...) are not ported.
            0xd2e2 => self.menu_callback_choice_exit_menu(),
            _ => {
                println!("dispatch_command_handler: unhandled 0x{handler:04x}");
            }
        }
    }

    // = seg000:d2e2 menu_callback_choice_exit_menu — close the active overlay and
    // reveal the menu beneath it with the command-panel fold. DOS chains three
    // steps: screen_overlay_request_transition (arm the pending-transition flag so the repaint stages into
    // fb1), screen_element_stack_pop_and_cleanup (run the popped element's cleanup func, pop it, and repaint
    // the revealed menu), then `jmp play_pending_panel_fold` (fold it onto the
    // screen). Reached from the mixer panel's LMB miss path (loc_0a576 -> a57e) and
    // from the dialogue verb panel's STOP TALKING verb (record 0x94/0xd2e2).
    pub(crate) fn menu_callback_choice_exit_menu(&mut self) {
        // = seg000:d2e2 call screen_overlay_request_transition — arm in_transition (unless an HNM is
        //   playing) so the menu repaint below stages into fb1 for the fold.
        self.screen_overlay_request_transition();
        // = seg000:d2e5 call screen_element_stack_pop_and_cleanup — cleanup + pop + repaint the revealed menu.
        self.screen_element_stack_pop_and_cleanup();
        // = seg000:d2e8 jmp play_pending_panel_fold — reveal the staged panel with
        //   the 17-frame accordion fold.
        self.play_pending_panel_fold();
    }

    // = seg000:0e3e menu_callback_choice_exit_game — the EXIT GAME verb (shared by
    // the mixer and mirror menus). Pushes the YES/NO confirmation submenu
    // (menu_exit_game_confirmation) as the active command menu, revealed with the
    // panel fold. DOS: bx = nullsub_00f66 (no-op cleanup), bp = menu_exit_game_
    // confirmation, jmp loc_0d323.
    pub(crate) fn menu_callback_choice_exit_game(&mut self) {
        // = seg000:d323 call screen_overlay_request_transition — arm in_transition
        //   so the submenu repaint stages into fb1 for the fold.
        self.screen_overlay_request_transition();
        // = seg000:d326 call screen_element_stack_push — install the confirmation
        //   submenu (bp) and repaint it (cl = 0xff, no slot pre-highlighted). Its
        //   nullsub_00f66 cleanup is modelled by the ExitGameConfirmation identity.
        self.screen_element_stack_push(
            ScreenElement::ExitGameConfirmation,
            MENU_EXIT_GAME_CONFIRMATION.to_vec(),
        );
        // = seg000:d329 call play_pending_panel_fold — fold the submenu onto screen.
        self.play_pending_panel_fold();
        // = seg000:d32c jmp loc_0d410 -> highlight_hovered_text_action_item — light
        //   up the slot under the cursor now the submenu is shown.
        self.highlight_hovered_text_action_item();
    }

    // = seg000:d2ea screen_element_stack_pop_and_cleanup — run the active screen element's cleanup func
    // ([si+2]), pop it off the stack, and repaint the menu revealed beneath it
    // (draw_command_menu with the new top). DOS skips the whole routine for a
    // priority-0xf-locked entry (the mirror overlay, 0xff). The flattened port
    // dispatches the cleanup by element identity and rebuilds the revealed room
    // verb records (the flattened push replaced them).
    pub(crate) fn screen_element_stack_pop_and_cleanup(&mut self) {
        // = seg000:d2f0 al = [di] & 0xf; cmp 0xf; jz loc_0d315 — the 0xff-locked
        //   look-away overlay is never closed through here (game_area_click /
        //   the 0x0eb9 verb pop it directly).
        let active = self.get_active_screen_element();
        if active == ScreenElement::LookAwayFromMirror {
            return;
        }

        // = seg000:d2f8 mov ax,[si+2]; call ax — the element's cleanup func.
        match active {
            // = loc_0a541 settings_ui_cleanup — the mixer panel's cleanup.
            ScreenElement::MixerPanel => self.settings_ui_cleanup(),
            // = seg000:97cf menu_npc_actions_cleanup — end the conversation.
            ScreenElement::NpcActionsMenu => self.menu_npc_actions_cleanup(),
            // = seg000:19fc loc_019fc — restore the room view the PALACE PLAN
            //   overlay covered.
            ScreenElement::PalacePlan => self.palace_plan_cleanup(),
            // RoomCommandMenu is the stack base; it has no cleanup func.
            _ => {}
        }

        // = seg000:d2fd screen_element_stack_pop_and_redraw — pop the entry unless already at the room base
        //   (DOS `cmp si,21beh; jz`).
        if self.screen_element_stack.len() <= 1 {
            return;
        }
        self.screen_element_stack.pop();

        // = seg000:d30e bp = [si]; cl = 0xff; call draw_command_menu — repaint the
        //   now-active menu. DOS keeps each element's record buffer on its stack;
        //   the flattened port dropped the revealed element's records on push, so
        //   rebuild them from its identity. With in_transition armed above,
        //   redraw_active_command_menu paints into fb1 for the fold.
        match self.get_active_screen_element() {
            ScreenElement::RoomCommandMenu => {
                self.build_room_command_records();
                if self.game_screen_mode_flags == 0 {
                    self.build_persons_in_room_records();
                }
            }
            // The mixer panel's command strip is its music menu; rebuild it (with
            // the per-music-state greying) when an EXIT GAME confirmation pops back.
            ScreenElement::MixerPanel => self.settings_ui_update_music_playlist_flags(),
            // The mirror still's verb menu is the fixed mirror record set.
            ScreenElement::LookAwayFromMirror => {
                self.command_menu_records = MENU_PALACE_MIRROR_ROOM.to_vec();
            }
            // TODO: revealing NpcActionsMenu (a dialogue with an overlay popped off
            //   it) needs the speaker context to rebuild its verbs; not modelled.
            _ => {}
        }
        self.redraw_active_command_menu();
    }

    // = seg000:97cf menu_npc_actions_cleanup — the NpcActionsMenu (dialogue verb
    // panel) cleanup, run when STOP TALKING pops the menu: stop the speaker's
    // voice lip-sync, then tear the conversation down and put the room back the
    // way it was before the dialogue zoom. common_dialogue (93b9) zoomed the room
    // in on the speaker (dialogue_zoom_room set room_render_flags 0x80 and 4×-
    // scaled fb1); this cleanup re-renders the room at 1:1 and presents it, so
    // STOP TALKING returns to the un-zoomed room view.
    //
    // TODO: 097cf also marks the active speaker's room_person (data_047a2->[0fh]:
    // set 0x20, clear 0x04), clears data_047e1, restores the subtitle backdrop
    // (subtitle_restore_prior), and rebuilds the room nav panel / NPC portraits
    // (rebuild_and_draw_room_nav_panel / NPC_09655). Those read the active-speaker
    // pointer, subtitle, and nav state not modelled yet. The data_00023-gated
    // transition-reveal variant (loc_09898, a wiped re-render) is not ported; the
    // port always takes the instant re-render path (loc_09879).
    fn menu_npc_actions_cleanup(&mut self) {
        // = seg000:97cf call lip_sync_stop — stop the speaker's voice lip-sync
        //   (also patching the TALK TO ME verb template back to its idle text
        //   for the next dialogue).
        self.lip_sync_stop();
        // = seg000:97d2 cmp current_lip_sync_resource_id,0ffffh; jz ret — no
        //   active conversation, so there is nothing to restore.
        if self.current_lip_sync_resource_id == 0xffff {
            return;
        }
        // = seg000:9868 and room_render_flags,7fh — drop the redraw-for-zoom flag
        //   dialogue_zoom_room set, so the room renders un-zoomed from here on.
        self.room_render_flags &= 0x7f;
        // = seg000:9886 call draw_room_scene — re-render the room scene (un-zoomed)
        //   into fb1, tearing down the talking head (reset_scene_lip_sync_state).
        self.draw_room_scene();
        // = seg000:9889 call copy_active_framebuffer_to_framebuffer_2 — keep fb2 in
        //   sync with the freshly rendered fb1.
        self.copy_active_framebuffer_to_framebuffer_2();
        // = seg000:988c call update_screen_palette.
        self.update_screen_palette();
        // = seg000:988f call ui_hud_head_save_rect — grab the game-area strip under
        //   the HUD head out of the new fb1 before the head rises.
        self.ui_hud_head_save_rect();
        // = seg000:9892 call loc_0c4dd — present the un-zoomed game area to screen.
        self.present_dialogue_head();
        // = seg000:9895 jmp ui_hud_head_animate_up — raise the small HUD head
        //   ornament back into view now the conversation is over.
        self.ui_hud_head_animate_up();
        // NOTE: build_room_command_records / build_persons_in_room_records (the
        // loc_09879 head of this block) are run by screen_element_stack_pop_and_
        // cleanup after this cleanup returns and pops back to RoomCommandMenu, so
        // they are not duplicated here.
    }

    // ---- LOOK AT MIRROR (palace bedroom, location_and_room 0x2009) ---------

    // = seg000:0ea6 look_at_mirror — the LOOK AT MIRROR verb handler. Suspend the
    // clock and fold the hud head down, then run the mirror still through a
    // transition: transition renders callback_transition_look_at_mirror offscreen
    // (DOS bp) and wipes it onto the screen.
    fn look_at_mirror(&mut self) {
        // = seg000:0ea6 call suspend_game_clock.
        self.suspend_game_clock();
        // = seg000:0ea9 call reset_scene_lip_sync_state.
        self.reset_scene_lip_sync_state();
        // = seg000:0eac call ui_hud_head_animate_down.
        self.ui_hud_head_animate_down();
        // = seg000:0eaf al=4; 0eb1 dx=0; 0eb3 bp=callback_transition_look_at_
        //   mirror; 0eb6 jmp transition.
        self.transition(4, |s| s.callback_transition_look_at_mirror());
    }

    // = seg000:0eb9 menu_callback_choice_palace_look_away_from_mirror — leave the
    // mirror still and return to the bedroom: clear the suspend, re-arm the room
    // screen, and redraw it.
    fn look_away_from_mirror(&mut self) {
        // = seg000:0eb9 data_047c3 = 0. TODO: data_047c3 not modelled.
        // = seg000:0ebe call reset_game_suspend.
        self.reset_game_suspend();
        // = seg000:0ec1 data_047a6 = 0xff.
        self.data_047a6 = 0xff;
        // = seg000:0ec6 al=4; dx=0; call ui_present_room_screen.
        self.ui_present_room_screen(4);
        // = seg000:0ecd jmp draw_room_game_screen.
        self.draw_room_game_screen();
    }

    // = seg000:0ed0 callback_transition_look_at_mirror — draw the mirror still:
    // open MIRROR.HSQ (bank 0x3b) and lay down sprites 0..2, redraw the portrait,
    // blank the nav compass, then arm the look-away overlay (a game-area click
    // returns to the room) and swap the command menu to the mirror verbs
    // (RESTART / LOAD / SAVE / EXIT GAME + "Look away from the mirror").
    fn callback_transition_look_at_mirror(&mut self) {
        // = seg000:0ed0 al=0x3b; loc_0c2f2 — open MIRROR.HSQ and draw sprite 0.
        self.open_resource_and_draw_sprite0(sprite_bank::MIRROR);
        // = seg000:0ed5 ax=1; draw_sprite_clobbering_bx_dx — sprite 1 at (0,0)
        //   from the still-active MIRROR bank.
        self.draw_active_bank_sprite(1, 0, 0);
        // = seg000:0edb call loc_00f08 — overlay Paul's talking head. The
        //   player-only path (loc_00960) saves the mirror backdrop into fb2 and
        //   draws Paul's PAUL.HSQ talking head (character 0x2d) over it, then
        //   starts its idle/lip-sync animation. The MIRROR sprite 2 drawn next
        //   frames it. (The persons_travelling_with & 0x80 companion path,
        //   loc_00f13, runs a second lip-sync resource 7 first — TODO.)
        self.setup_talking_head(0x2d, 0);
        // = seg000:0ede ax=0x3b open MIRROR (already active); 0ee4 ax=2 draw
        //   sprite 2 at (0,0).
        self.open_sprite_bank(sprite_bank::MIRROR);
        self.draw_active_bank_sprite(2, 0, 0);
        // = seg000:0eee call ui_hud_head_draw — redraw the small ICONES
        //   HUD head-and-shoulders ornament (the head + arms wrapped around the
        //   command menu, ICONES sprite 0x10 + ui_hud_head_index). This is NOT the
        //   mirror reflection — that is the big PAUL.HSQ lip-sync portrait drawn by
        //   setup_talking_head(0x2d) above; this is the fixed HUD decoration. The
        //   room-entry ui_hud_head_animate_up leaves ui_hud_head_index fully raised (0x0a),
        //   so the ornament shows; at the folded index 0 only the near-invisible
        //   frame draws.
        self.ui_hud_head_draw();
        // = seg000:0ef1 si=0x1d1e; loc_0d72b — install the mirror nav-panel
        //   template into HUD records 12..17, blanking the bottom-right compass
        //   (no sprites, no clickable records) for the mirror still.
        self.ui_install_nav_panel(&NAV_PANEL_MIRROR);
        // = seg000:0ef7 call main_ui_elements_clear_flags_18_19_20.
        self.main_ui_elements_clear_flags_18_19_20();
        // = seg000:0efa ui_elements[20].flags = 0x80 — enable the full game-area
        //   hotspot (rect 0,0..320,152) so any click there dismisses the mirror.
        self.ui_elements[20].flags = 0x80;
        // = seg000:0eff bp=menu_palace_mirror_room (seg001:20c2); 0f02 bx=menu_
        //   callback_choice_palace_look_away_from_mirror; 0f05 jmp
        //   screen_element_stack_push — install the mirror verb menu (priority
        //   0xff) as the active command menu and paint it (RESTART / LOAD / SAVE
        //   / EXIT GAME, then "Look away from the mirror"). bx is the overlay's
        //   look-away cleanup func, modelled here by the ScreenElement identity.
        //   look_away_from_mirror -> draw_room_game_screen rebuilds the room
        //   verbs when the still is dismissed.
        self.screen_element_stack_push(
            ScreenElement::LookAwayFromMirror,
            MENU_PALACE_MIRROR_ROOM.to_vec(),
        );
    }

    // = seg000:941d room_game_area_click — the game-area hotspot (ui_elements[20])
    // click. When the look-away overlay is the active screen element, pop it and return
    // to the room (menu_callback_choice_palace_look_away_from_mirror).
    pub(crate) fn game_area_click(&mut self) {
        // = seg000:9422 cmp data_047a9,0 (the smuggler branch) is not modelled.
        // = seg000:9427 call get_active_screen_element; 942a cmp bp,20c2h.
        if self.get_active_screen_element() != ScreenElement::LookAwayFromMirror {
            // TODO: the other game-area click branches (dialogue / map modes,
            //   seg000:9436..9458) are not ported.
            return;
        }
        // = seg000:9430 call screen_element_stack_pop_and_cleanup. DOS leaves the 0xff-locked mirror entry on
        // the stack (screen_element_stack_pop_and_cleanup skips priority&0xf==0xf); the flattened port pops it
        // so the room menu is active again. Same dismissal as the verb path
        // (dispatch_command_handler 0x0eb9).
        self.screen_element_stack.pop();
        // = seg000:9433 jmp menu_callback_choice_palace_look_away_from_mirror.
        self.look_away_from_mirror();
    }

    // = seg000:9215 callback_main_ui_element_21_22 — the game-area / person click
    // dispatch. Over the room command menu in the normal room view, hit-test the
    // on-screen people (person_hit_test); a hit on person index < 0x0f dispatches
    // that person's verb handler (room_persons[id].handler, = `jmp word ptr
    // [si+4]`). The room LMB handler reaches this when a click lands on no
    // ui_element (= seg000:d904 hit-test miss -> d90a call), and it is also the
    // handler armed on ui_elements[21]/[22].
    //
    // Not modelled: the bp==1f7e dialogue branch (loc_09248), the person
    // index >= 0x0f / cl==0x2f branches (loc_09240 / 09282), and the no-person
    // room-edge up-travel branch (loc_09263 -> ui_click_room_up).
    pub(crate) fn callback_main_ui_element_21_22(&mut self) {
        // = seg000:9215 get_active_screen_element; cmp bp,1f0eh; jnz loc_09248.
        // = seg000:921e cmp game_screen_mode_flags,0; jnz loc_09281.
        if self.get_active_screen_element() != ScreenElement::RoomCommandMenu
            || self.game_screen_mode_flags != 0
        {
            return;
        }
        // = seg000:9225 call person_hit_test_at_cursor; jnb loc_09263 (no person hit).
        let Some(person_id) = self.person_hit_test() else {
            return;
        };
        // = seg000:922f cmp cl,0fh; jnb loc_09240 — only person index < 0x0f
        // dispatches a room_persons handler here.
        if (person_id as usize) < 0x0f {
            // = seg000:9234 al=0x10; mul cl; si = room_persons + cl*0x10;
            // jmp word ptr [si+4] — dispatch the matched person's handler.
            let handler = self.room_persons[person_id as usize].handler;
            self.dispatch_command_handler(handler);
        }
    }

    // = seg000:d41b get_active_screen_element — return the identity of the top
    // screen-element-stack entry (the room command menu when nothing is layered
    // over it).
    pub(crate) fn get_active_screen_element(&self) -> ScreenElement {
        self.screen_element_stack
            .last()
            .copied()
            .unwrap_or(ScreenElement::RoomCommandMenu)
    }

    // = seg000:b2b9 suspend_game_clock — inc game_suspend_count, suspending the
    // in-game clock and idle events one nesting level.
    fn suspend_game_clock(&mut self) {
        self.game_suspend_count = self.game_suspend_count.saturating_add(1);
    }

    // ---- Not-yet-ported callees (no-op stubs, each linked to its DOS address).
    //      With the default flag state none of the gameplay branches run, so
    //      these are placeholders until the underlying systems are ported.

    // = seg000:d95b select_room_ui_table — set the active mouse-handler table
    // (data_02570) to the room-screen variant. DOS loads ax = 0d95eh's stored
    // ROOM_MOUSE_HANDLERS pointer and falls into loc_0d95e (mov [data_02570],ax).
    // Called when entering the room view and from the mixer-panel cleanup
    // (loc_0a541) to restore the room handlers after the overlay closes.
    pub(crate) fn select_room_ui_table(&mut self) {
        self.active_mouse_handlers = &crate::game_ui::ROOM_MOUSE_HANDLERS;
    }

    // = seg000:08f0 open_SAL_resource — open the current location/room's scene
    // (.SAL) resource. The port currently opens + renders together inside
    // draw_location_room (room_scene.rs).
    // TODO: port the standalone open; no-op stub.
    #[allow(non_snake_case)]
    fn sal_open_resource(&mut self) {}

    // = seg000:2eb2 ui_draw_room_command_panel — draw the bottom command /
    // dialogue panel. With a dialogue active (data_04774 != 0) it renders the
    // dialogue (loc_0301a) and enqueues its render task; otherwise it builds and
    // draws the verb menu for command_list_ptr. Run via the offscreen helper from
    // draw_room_game_screen.
    fn ui_draw_room_command_panel(&mut self) {
        // = seg000:2eb2 cmp data_04774,0; jnz -> the dialogue branch.
        if self.is_dialogue_active {
            // = seg000:2eb9 call loc_0301a (render the dialogue panel).
            self.draw_dialogue_panel();
            // = seg000:2ebc call loc_098e6 (reset the per-scene lip-sync indices).
            self.reset_scene_lip_sync_state();
            // = seg000:2ebf loc_02ebf: bp = [data_02220] (the dialogue record
            // buffer), bx = 0f66h; jmp screen_element_stack_push — install the
            // dialogue panel as the active screen element.
            self.draw_task_list_insert();
            return;
        }

        // = seg000:2ec9 loc_02ec9 — the verb-menu branch.
        // = seg000:2ec9 di = [data_0114e] = command_list_ptr.
        // = seg000:2ecd call set_command_menu_origin (menu x/y from the header).
        self.set_command_menu_origin();
        // = seg000:2ed0 call build_room_command_records (assemble the verb list).
        self.build_room_command_records();
        // = seg000:2ed3 in plain room mode (game_screen_mode_flags == 0) also
        // append the people-present records.
        if self.game_screen_mode_flags == 0 {
            // = seg000:2eda call build_persons_in_room_records.
            self.build_persons_in_room_records();
        }

        // = seg000:2edd when the cursor sits over the command-panel area
        // (mouse_pos_x >= 0x74) restore the hardware cursor (rect 0dbech) so the
        // about-to-be-redrawn verbs are not painted under a stale cursor image.
        if self.input.lock().unwrap().mouse_x >= 0x74 {
            // = seg000:2ee5 ax = 0dbech; push; call call_restore_cursor.
            self.restore_cursor_over_panel();
        }

        // = seg000:2eec call rebuild_and_draw_room_nav_panel (records 12..18).
        self.rebuild_and_draw_room_nav_panel();
        // = seg000:2eef call loc_0d763 — redraw the book/companion buttons.
        self.ui_hud_draw_companions();
        // = seg000:2ef2 bp = command_menu_buf, bx = 0f66h; jmp
        // screen_element_stack_push, which paints the menu via redraw_active_
        // command_menu. command_menu_buf is the room's persistent stack bottom
        // (RoomCommandMenu); re-inserting an equal-priority buffer replaces it in
        // place (d345 jz), so unlike the mirror overlay the flattened port skips
        // the push and just repaints the freshly built records here.
        self.redraw_active_command_menu();
    }

    // ---- Command-panel callees (linked stubs; see the .chani annotations).

    // = seg000:2e98 set_command_menu_origin — compute the verb-menu draw origin
    // from the command_list header at command_list_ptr (command_menu_x =
    // header[0]; command_menu_y = header[1] + 0xc) and save the list to
    // command_menu_list.
    // TODO: port; needs the command-list data model (command_list_ptr is the
    // seg-relative offset of a static verb list). No-op stub.
    fn set_command_menu_origin(&mut self) {}

    // = seg000:2efb build_room_command_records — assemble the verb-menu record
    // list for the current room into command_menu_records (= the seg001:1f0e
    // buffer, whose leading skip byte the port models implicitly as empty). The
    // records come from the seg001 command-record templates (21dc..221c), gated
    // by the room type (location_appearance low byte 0x80 = special room),
    // location_and_room, game phase, ornithopter count, smuggler flag, and
    // time-of-day. The DOS `xor ax,ax; stosw` terminator is the empty Vec tail.
    fn build_room_command_records(&mut self) {
        // = seg000:2efd di=1f0fh; xor al,al; stosb — the empty header skip byte.
        let mut recs: Vec<CommandMenuRecord> = Vec::new();
        // = seg000:2f03 bx = data_00006 (location_appearance); dx = location_and_room.
        let bx = self.location_appearance;
        let dx = self.location_and_room;
        let (bl, bh) = (bx as u8, (bx >> 8) as u8);
        let dl = dx as u8;

        if bl == 0x80 {
            // = seg000:2f13 loc_02f13 — the special/palace-room branch.
            // = seg000:2f13 si=220ch; movsw movsw — "SEE DUNE MAP".
            recs.push(CMD_SEE_DUNE_MAP);
            if dl == 1 {
                // = seg000:2f1b loc_02f13 dl==1 — the sietch / night-attack room.
                if self.night_attack_stage != 0 {
                    // = seg000:2f24 si=2218h; copy the two night-attack verbs
                    // ("MASSIVE ATTACK" + "FIGHT FOR A WHOLE DAY"), then the
                    // worm-summon verb greyed until game_phase >= 0x4f.
                    recs.push(CMD_MASSIVE_ATTACK);
                    recs.push(CMD_FIGHT_FOR_A_WHOLE_DAY);
                    recs.push(grey_if(CMD_CALL_A_WORM, self.game_phase < 0x4f));
                } else {
                    // = seg000:2f3d loc_02f3d — di = [command_list_ptr] (the current
                    // location pointer stashed there at room commit); call
                    // compute_location_available_equipment (seg000:7f27) to refresh
                    // orni_count for this location, then "TAKE AN ORNITHOPTER" greyed
                    // while orni_count < 1.
                    self.compute_location_available_equipment();
                    recs.push(grey_if(
                        CMD_TAKE_ORNITHOPTER,
                        self.available_equipment.ornithopters < 1,
                    ));
                }
            } else if bh == 1 {
                // = seg000:2f58 loc_02f58 — the bh==1 palace branch.
                if dl == 8 && self.data_000c8 != 0 {
                    // = seg000:2f62 palace room 8 is the communications room
                    // with a new transmission queued (data_000c8 != 0). The
                    // verbs are the message viewer ("VIEW NEW MESSAGES" /
                    // "Messages already seen").
                    // = seg000:2f6d ch picks a sprite (27h/26h/28h via
                    // RES_SMUG_HSQ and data_047a9) and stores it into
                    // palace_rooms[7]; the verbs grey off the RES_SMUG_HSQ
                    // loaded flag (treated as not loaded here) and data_000c8.
                    // TODO: port the palace_rooms[7] sprite side-effect + the
                    // RES_SMUG_HSQ / data_047a9 inputs.
                    let messages_loaded = false;
                    recs.push(grey_if(CMD_VIEW_NEW_MESSAGES, !messages_loaded));
                    recs.push(grey_if(CMD_VIEW_OLD_MESSAGES, !messages_loaded));
                } else if dl == 9 {
                    // = seg000:2f9e si=21f0h; "LOOK AT MIRROR" — palace room 9
                    // is Paul's bedroom with the mirror.
                    recs.push(CMD_LOOK_AT_MIRROR);
                }
            }
            // = seg000:2fa3 loc_02fa3 — si=21f4h; "Mixer Panel" trailing verb.
            recs.push(CMD_MIXER_PANEL);
        } else if self.game_screen_mode_flags & 3 != 0 {
            // = seg000:2fd7 loc_02fd7 — the map/book-mode verbs.
            if self.data_011cb != 0 && self.game_phase >= 0x32 {
                // = seg000:2fe1 si=2200h; the phase-gated map verb pair
                // ("BACK TO STARTING POINT" + "TOWARDS NEAREST PLACE").
                recs.push(CMD_BACK_TO_STARTING_POINT);
                recs.push(CMD_TOWARDS_NEAREST_PLACE);
            } else {
                // = seg000:2fda si=21fch; "SKIP TO DESTINATION" default.
                recs.push(CMD_SKIP_TO_DESTINATION);
            }
            // = seg000:2ff2 si=21f8h; "CHANGE DESTINATION" trailing verb.
            recs.push(CMD_CHANGE_DESTINATION);
        } else {
            // = seg000:2faa loc_02faa — the plain (non-special) room branch.
            // = seg000:2fb1 si=220ch; "SEE DUNE MAP".
            recs.push(CMD_SEE_DUNE_MAP);
            // = seg000:2fb6 si=2214h; "CALL A WORM" greyed until phase >= 0x4f.
            recs.push(grey_if(CMD_CALL_A_WORM, self.game_phase < 0x4f));
            // = seg000:2fc6 the time-skip verb: "WAIT FOR EVENING" while the
            // in-game time-of-day phase is < 0x0b, else "WAIT FOR MORNING".
            if self.get_ingame_time_of_day() < 0x0b {
                recs.push(CMD_WAIT_FOR_EVENING);
            } else {
                recs.push(CMD_WAIT_FOR_MORNING);
            }
            // = seg000:2fa3 loc_02fa3 — "Mixer Panel" trailing verb.
            recs.push(CMD_MIXER_PANEL);
        }

        self.command_menu_records = recs;
    }

    // = seg000:1ae0 get_ingame_time_of_day — the time-of-day phase, game_time & 0xf.
    fn get_ingame_time_of_day(&self) -> u8 {
        (self.game_time & 0xf) as u8
    }

    // = seg000:7f27 compute_location_available_equipment — recompute the current
    // location's per-type available equipment (DOS buffer at seg001:46fe,
    // location_available_equipment); the ornithopters slot is orni_count, read
    // just below to grey TAKE AN ORNITHOPTER.
    //
    // DOS takes the location pointer in di — the room-commit at seg000:4024 stashes
    // it in command_list_ptr, which the 2f3d call site reloads into di. The port
    // instead recovers the location index from location_appearance, whose high byte
    // the same commit set to index+1 (seg000:40ae div by 0x1c, then seg000:4067
    // stores bx). This call site is the sietch night-attack room, reached only via
    // the special-room commit, so the high byte is the index there.
    fn compute_location_available_equipment(&mut self) {
        // = seg000:2f3e di = [command_list_ptr]; the location index is the high
        // byte of location_appearance minus 1 (0 = palace). A 0 high byte would
        // underflow past the table; this guard covers it.
        let loc = (self.location_appearance >> 8).wrapping_sub(1) as usize;
        if loc >= self.locations.len() {
            return;
        }
        self.available_equipment = self.location_available_equipment(&self.locations[loc]);
    }

    // = seg000:7f2a location_iterate_on_troops_in_location — the shared body the
    // seg000:7f27 entry falls into with di = the location pointer and bx = the
    // seg001:46fe buffer. Copy the location's harvesters..bulbs equipment row,
    // then walk its troop list (head location->troop_id, next
    // troop->next_troop_id) subtracting each troop's held equipment: one slot per
    // set bit of troop->equipment, MSB first (harvesters, ornithopters,
    // krys_knives, laser_guns, weirding_modules, atomics, bulbs), each decrement
    // clamped at 0. The result — the equipment present at the location but not
    // yet held by its troops — is returned as an Equipment value where DOS fills
    // the seg001:46fe buffer.
    fn location_available_equipment(&self, location: &Location) -> Equipment {
        // = seg000:7f2d/7f30/7f38 copy location->harvesters..bulbs into the buffer.
        let e = &location.equipment;
        let mut buf = [
            e.harvesters,
            e.ornithopters,
            e.krys_knives,
            e.laser_guns,
            e.weirding_modules,
            e.atomics,
            e.bulbs,
        ];
        // = seg000:7f2d al = location->troop_id (list head; 0 = no troops).
        let mut troop_id = location.troop_id;
        // = seg000:7f3a..7f5b walk the troop list.
        while troop_id != 0 {
            // = seg000:7f3e get_address_of_troop_by_ID (troops + (id-1)*0x1b). The
            // table only spans the 68 real troops; bad data just ends the walk.
            let Some(troop) = self.troops.get((troop_id - 1) as usize) else {
                break;
            };
            // = seg000:7f41 al = troop->equipment.
            let mask = troop.equipment;
            // = seg000:7f44..7f56 distribute the bitmask: bit 7 -> slot 0, down to
            // bit 1 -> slot 6, decrementing the matching slot, saturating at 0.
            for (slot, avail) in buf.iter_mut().enumerate() {
                if mask & (0x80 >> slot) != 0 {
                    *avail = avail.saturating_sub(1);
                }
            }
            // = seg000:7f58 al = troop->next_troop_id.
            troop_id = troop.next_troop_id;
        }
        Equipment {
            harvesters: buf[0],
            ornithopters: buf[1],
            krys_knives: buf[2],
            laser_guns: buf[3],
            weirding_modules: buf[4],
            atomics: buf[5],
            bulbs: buf[6],
        }
    }

    // = seg000:d338 screen_element_stack_push — insert a command-record buffer
    // (DOS bp) with its per-frame render/cleanup func (DOS bx) onto the z-ordered
    // screen-element stack and repaint the now-active verb menu. DOS chains
    // screen_element_stack_insert (d33a, the priority-sorted insert that pops
    // higher-priority entries and runs their render funcs) -> draw_command_menu
    // (d36d, set the top slot and clear the records' 0x8000 highlight bits) ->
    // redraw_active_command_menu (d397). The port flattens the priority stack
    // (cf. get_active_screen_element and lib.rs): it pushes the element identity,
    // swaps in its records, and repaints. cl=0xff (no slot pre-highlighted) is
    // implicit in redraw_active_command_menu starting from "nothing hovered".
    pub(crate) fn screen_element_stack_push(
        &mut self,
        element: ScreenElement,
        records: Vec<CommandMenuRecord>,
    ) {
        self.screen_element_stack.push(element);
        self.command_menu_records = records;
        self.redraw_active_command_menu();
    }

    // = seg000:90bd setup_npc_dialogue_menu — pick the dialogue verb panel's
    // per-NPC second verb (the slot between TALK TO ME and STOP TALKING) and push
    // menu_NPC_actions onto the screen-element stack so the dialogue verbs render
    // in the command panel. DOS receives the speaker's room_person in si; the port
    // takes its table index.
    pub(crate) fn setup_npc_dialogue_menu(&mut self, person_index: u8) {
        let npc = self.room_persons[person_index as usize];
        // = seg000:90bd al = npc->person_index. The dynamic verb (text id `bx`,
        // callback handler `dx`) is chosen per-NPC; a greyed verb carries the
        // 0x4000 bit redraw_active_command_menu draws dimmed.
        let pi = npc.person_index;
        let dynamic = if pi == 0x0c
            && (self.persons_in_room & 0x1000) != 0
            && (self.room_persons[12].flags & 0x10) == 0
        {
            // = seg000:90c0 the Harkonnen-Captain prisoner: while the captain
            // (persons_in_room bit 0x1000) stands in the room and room_persons[12]
            // is not yet flagged 0x10, offer OVERPOWER THE PRISONER.
            rec(0x9c, 0x9584)
        } else if pi == 0x0f {
            // = seg000:90d9 person 0x0f: text 0x93, handler loc_05a03.
            rec(0x93, 0x5a03)
        } else if pi == 0x0e {
            // = seg000:90e3 person 0x0e: text 0x96, bumped to 0x97 once Paul-event
            // bit 0x10 is set; handler loc_095c1.
            let id = if (self.bitfield_paul_events & 0x10) != 0 {
                0x97
            } else {
                0x96
            };
            rec(id, 0x95c1)
        } else {
            // = seg000:90f7 the general NPC.
            let flags = npc.flags;
            if (flags & 0x80) != 0 {
                // = seg000:90fd greyed COME WITH ME (text 0x91 | 0x4000). DOS
                // leaves the callback (dx) stale; the verb is disabled, so it is
                // never dispatched.
                rec(0x4091, 0)
            } else if (flags & 0x40) != 0 {
                // = seg000:910d the NPC already travels with you, so offer STAY
                // HERE (text 0x92, handler menu_callback_choice_stay_here).
                rec(0x92, 0x9533)
            } else {
                // = seg000:9102 COME WITH ME (text 0x91, handler
                // menu_callback_choice_come_with_me).
                rec(0x91, 0x95e2)
            }
        };
        // = seg000:9111 menu_NPC_actions (seg001:1f7e) with the dynamic verb
        // spliced into slot 1 (DOS overwrites [bp+6]/[bp+8] of the static menu).
        // Its leading flags word 0xfc is the priority byte the flattened
        // screen-element stack drops.
        let records = vec![
            // = seg001:1f80 TALK TO ME (menu_callback_choice_talk_to_me). The
            // text id is the template value set_talk_to_me_verb_text patches:
            // 0x90 ('>>>> TALK TO ME <<<<') while a voice plays, 0x9f
            // ('" TALK TO ME "') once it stops.
            rec(self.menu_npc_actions_talk_text_id, 0x9472),
            dynamic,
            // = seg001:1f88 " WHAT ? " (text 0x95, handler menu_callback_choice_what).
            rec(0x95, 0x9ed5),
            // = seg001:1f8c "STOP TALKING" (text 0x94, menu_callback_choice_exit_menu).
            rec(0x94, 0xd2e2),
        ];
        // = seg000:911a call screen_overlay_request_transition — arm the in-transition flag so the verbs
        // stage in fb1 (draw_command_menu_item routes there when in_transition > 0).
        // The pending fold (play_pending_panel_fold / play_pending_panel_fold) then reveals the
        // staged panel onto the screen.
        self.screen_overlay_request_transition();
        // = seg000:911d bx = menu_npc_actions_cleanup (the menu's render/cleanup func); 9120 jmp
        // screen_element_stack_push. With in_transition armed, redraw_active_command_
        // menu paints the verbs into fb1, not the visible screen.
        self.screen_element_stack_push(ScreenElement::NpcActionsMenu, records);
    }

    // = seg000:d316 screen_overlay_request_transition — when no HNM movie is playing, set the in-transition
    // flag's low bit so the verb-panel paint stages into fb1 (offscreen) until the
    // pending fold reveals it. The port does not model the HNM file handle (treated
    // as none), so the bit is always armed here.
    pub(crate) fn screen_overlay_request_transition(&mut self) {
        self.in_transition |= 1;
    }

    // = seg000:d397 redraw_active_command_menu — paint the active verb menu
    // (command_menu_records) into HUD rows 7..11. Up to five slots are drawn; a
    // sixth-or-later verb collapses into the 0xa0 "more" arrow in slot 4, and any
    // slots past the last record are filled blank (clearing stale verbs). Falls
    // into highlight_hovered_text_action_item (loc_0d410) so the slot under the
    // pointer immediately gets the inverse highlight.
    pub(crate) fn redraw_active_command_menu(&mut self) {
        // = seg000:d397 mov [index_of_last_hovered_action_item], 0ffh —
        // discard any prior hover so the highlight repaint that follows runs
        // against a fresh "nothing highlighted" baseline.
        self.index_of_last_hovered_action_item = 0xff;
        let n = self.command_menu_records.len();
        // = seg000:d3b5 walk the records, one per slot (cl = 0..4).
        for slot in 0..5u8 {
            let i = slot as usize;
            // = seg000:d3b9 a 0 record (past the end) draws a blank slot.
            // = seg000:d3be slot 4 with more records behind it shows the "more"
            // arrow (text_id 0xa0); the skip-byte path that also forces it is the
            // empty header here, so only the overflow case applies.
            let text_id = if i >= n {
                0
            } else if slot == 4 && n > 5 {
                0xa0
            } else {
                self.command_menu_records[i].text_id
            };
            self.draw_command_menu_item(slot, text_id);
        }
        // = seg000:d3ed jmp loc_0d410; loc_0d410 jmp highlight_hovered_text_action_item.
        // DOS falls through so the slot under the pointer is highlighted as part
        // of the same paint pass.
        self.highlight_hovered_text_action_item();
    }

    // = seg000:d50f highlight_hovered_text_action_item — repaint at most two
    // verb slots so the one under the pointer shows the 0x8000 inverse
    // highlight. Two hover sources feed the same highlight:
    //   - the person-hover branch (seg000:d523..d55d): over the room command
    //     menu in the normal room view, hit-test the on-screen people
    //     (person_hit_test) and map the hovered person to its verb slot, so a
    //     mouseover on a character lights up its "&Person" verb;
    //   - the verb-strip branch (seg000:d5b1, verb_strip_hovered_slot): the slot
    //     whose ui_elements[7..] rect the pointer falls in.
    // On a change vs. index_of_last_hovered_action_item, re-draw the old slot
    // without highlight (= seg000:d5f5) and the new slot with the 0x8000 bit
    // OR'd into its text_id (= seg000:d602..d60a).
    //
    // The di=1bf0h talking-head sub-branch (d533..d543) and the dialogue
    // (data_04774) early return are not modelled.
    // Returns true when a slot was repainted so game_loop can re-present.
    pub(crate) fn highlight_hovered_text_action_item(&mut self) -> bool {
        // = seg000:d3ac data_0dce8 = the slot count painted by the preceding
        // redraw — at most five record slots, plus one for the "more" arrow
        // when n > 5 (slot 4 already holds it; the count stays at five).
        let n = self.command_menu_records.len();
        let slot_count = n.min(5) as u8;
        if slot_count == 0 {
            return false;
        }

        // = seg000:d523 get_active_screen_element; cmp bp,1f0eh; cmp
        // game_screen_mode_flags,0 — the person-hover branch runs only over the
        // room command menu in the normal room view.
        let new_slot = if self.get_active_screen_element() == ScreenElement::RoomCommandMenu
            && self.game_screen_mode_flags == 0
        {
            // = seg000:d545 call person_hit_test_at_cursor. On a hit, map the
            // person to its verb slot (d55d); on a miss (jnb loc_0d575), fall to
            // the verb-strip rect test.
            match self.person_hit_test() {
                Some(person_id) => {
                    self.slot_for_person_text_id(0x78 + person_id as u16, slot_count)
                }
                None => self.verb_strip_hovered_slot(slot_count),
            }
        } else {
            // = seg000:d575/d5b1 the plain verb-strip rect-test path.
            self.verb_strip_hovered_slot(slot_count)
        };

        // = seg000:d5df xchg cl,[index_of_last_hovered_action_item] —
        // swap in the new slot and read the previous one.
        let old_slot = std::mem::replace(&mut self.index_of_last_hovered_action_item, new_slot);
        // = seg000:d5e3 cmp al,cl; jz loc_0d610 — nothing changed.
        if old_slot == new_slot {
            return false;
        }

        // = seg000:d5e7 call call_restore_cursor — erase the software cursor
        // before repainting the slots under it (no-op for the GPU/system cursor).
        self.call_restore_cursor();

        // = seg000:d5ea..d5fb un-highlight the previously hovered slot, if
        // any. The plain text_id (no 0x8000) lets draw_command_menu_item
        // paint it as a normal enabled / greyed verb.
        if old_slot < slot_count {
            let text_id = self.slot_text_id(old_slot);
            self.draw_command_menu_item(old_slot, text_id);
        }
        // = seg000:d5fc..d60a paint the new slot with the 0x8000 highlight
        // bit set so draw_command_menu_item's loc_0d4d6 branch swaps fg/bg.
        if new_slot < slot_count {
            let text_id = self.slot_text_id(new_slot) | 0x8000;
            self.draw_command_menu_item(new_slot, text_id);
        }
        // = seg000:d60d call draw_mouse — re-composite the software cursor over
        // the freshly painted slots.
        self.draw_mouse();
        true
    }

    // = seg000:d454 loc_0d454 — resolve the text_id painted into the
    // requested slot. Mirrors the slot-selection in redraw_active_command_menu
    // so the un-highlight / highlight repaint uses the same string the slot
    // originally held (including the 0xa0 "more" arrow at slot 4).
    fn slot_text_id(&self, slot: u8) -> u16 {
        let i = slot as usize;
        let n = self.command_menu_records.len();
        if i >= n {
            0
        } else if slot == 4 && n > 5 {
            0xa0
        } else {
            self.command_menu_records[i].text_id
        }
    }

    // = seg000:d5b1 loc_0d5b1 — the verb-strip rect test: the first painted slot
    // whose ui_elements[7..7+slot_count] rect contains the pointer, else 0xff. The
    // hit is gated below the bottom HUD strip (= seg000:d5b1 cmp bx,98h; jb).
    fn verb_strip_hovered_slot(&self, slot_count: u8) -> u8 {
        let x = self.mouse_pos_x;
        let y = self.mouse_pos_y;
        if y < 0x98 {
            return 0xff;
        }
        // = seg000:d5bc..d5c3 x is tested once against ui_elements[7] (all
        // slots share the column): x0 <= x < x1 (jb / jnb).
        let el7 = &self.ui_elements[7];
        if x < el7.x0 || x >= el7.x1 {
            return 0xff;
        }
        // = seg000:d5c7 walk ui_elements[7..7+slot_count]; per-slot y test is
        // y0 < y <= y1 (jbe = miss on y <= y0, hit on y <= y1), so the shared
        // edge between two stacked slots belongs to the upper one — the same
        // edge ownership as the click test (hit_test_ui_elements, seg000:d6f0).
        // Slots are sorted by y, so y <= y0 ends the walk.
        for slot in 0..slot_count {
            let el = &self.ui_elements[7 + slot as usize];
            if y <= el.y0 {
                return 0xff;
            }
            if y <= el.y1 {
                return slot;
            }
        }
        0xff
    }

    // = seg000:d621 set_talk_to_me_verb_text (entries mark_talk_to_me_verb_
    // talking, d617: ax = 0x90, and mark_talk_to_me_verb_idle, d61d: ax = 0x9f)
    // — write `text_id` into menu_NPC_actions record 0's text id (seg001:1f80).
    // A voice line starting sets 0x90 ('   >>>>  TALK TO ME  <<<<',
    // seg000:a757); the voice stopping sets 0x9f ('" TALK TO ME "',
    // lip_sync_stop seg000:a7b1). When the id changed and the NPC menu is the
    // active screen element, redraw verb slot 0 in place.
    pub(crate) fn set_talk_to_me_verb_text(&mut self, text_id: u16) {
        // = seg000:d62a cmp [si+2],ax; mov [si+2],ax — patch the menu template
        // (DOS patches the static record the active stack entry points at; the
        // flattened port keeps the template value in menu_npc_actions_talk_
        // text_id and the active element's live copy in command_menu_records).
        let changed = self.menu_npc_actions_talk_text_id != text_id;
        self.menu_npc_actions_talk_text_id = text_id;
        // = seg000:d630 jz — unchanged; d632 cmp bp,si; jnz — the NPC menu is
        // not the active screen element (the template patch alone persists).
        if !changed || self.get_active_screen_element() != ScreenElement::NpcActionsMenu {
            return;
        }
        if let Some(rec0) = self.command_menu_records.first_mut() {
            rec0.text_id = text_id;
        }
        // = seg000:d636 call call_restore_cursor — erase the software cursor
        // before repainting the slot under it.
        self.call_restore_cursor();
        // = seg000:d639 cx = 0; read_command_menu_record_for_slot;
        // d63e call draw_command_menu_item — repaint slot 0 with the new text.
        self.draw_command_menu_item(0, text_id);
        // = seg000:d641 index_of_last_hovered_action_item = 0xff — drop the
        // hover baseline so the next highlight pass repaints cleanly.
        self.index_of_last_hovered_action_item = 0xff;
        // = seg000:d646 call draw_mouse.
        self.draw_mouse();
    }

    // = seg000:d55d loc_0d55d — map a person's verb text_id (0x78 + person index)
    // to the menu slot displaying it: bp = person_index + 0x78, then walk the
    // painted slots for the first whose record text_id matches (= the
    // read_command_menu_record_for_slot compare), else 0xff (no highlight; the
    // loc_0d5db fallthrough).
    fn slot_for_person_text_id(&self, text_id: u16, slot_count: u8) -> u8 {
        for slot in 0..slot_count {
            if self.slot_text_id(slot) == text_id {
                return slot;
            }
        }
        0xff
    }

    // = seg000:9285 person_hit_test_at_cursor — hit-test the cursor against the
    // on-screen person markers (character_screen_pos, seg001:47f8), returning the
    // person id (0..0x16) of the first marker the cursor falls in, or None. The
    // marker is each person's draw anchor (top-left); the test is a fixed
    // person-sized box below-and-right of it — mouse_x 1..=32 px right of the
    // anchor and mouse_y 1..=80 px below it. Gated on mouse_pos_y < 0x98 (the
    // room scene area).
    pub(crate) fn person_hit_test(&self) -> Option<u8> {
        let mouse_x = self.mouse_pos_x;
        let mouse_y = self.mouse_pos_y;
        // = seg000:9285 cmp bx,98h; jnb loc_092c9 — only below the HUD strip.
        if mouse_y >= 0x98 {
            return None;
        }
        // = seg000:928e cx = 0x17 person slots, indexed by person id.
        for id in 0..0x17u8 {
            let (x, y) = self.character_screen_pos[id as usize];
            // = seg000:9297 or di,di; js loc_092a9 — skip an absent marker
            // (0xffff, high bit set).
            if x & 0x8000 != 0 {
                continue;
            }
            // = seg000:929b sub di,dx; cmp di,0e0h; jb loc_092a9. The 0x83
            // opcode sign-extends the 0e0h immediate to 0xffe0 (-32), so the hit
            // needs di = (x - mouse_x) in [0xffe0, 0xffff] (signed -32..-1): the
            // cursor 1..=32 px right of the anchor, a fixed 32-px-wide person box.
            if x.wrapping_sub(mouse_x) < 0xffe0 {
                continue;
            }
            // = seg000:92a2 sub bp,bx; cmp bp,0b0h; jnb loc_092eb — hit. The 0b0h
            // immediate likewise sign-extends to 0xffb0 (-80): the cursor 1..=80
            // px below the anchor, a fixed 80-px-tall person box.
            if y.wrapping_sub(mouse_y) >= 0xffb0 {
                return Some(id);
            }
        }
        None
    }

    // = seg000:d48a draw_command_menu_item — draw one verb slot (`slot` 0..4,
    // `text_id` with state bits) into ui_elements[7+slot]: a leading space + the
    // resolved string at x=0x5d, y = the row's y0 + 1 (small font), then fill the
    // rest of the row with the background colour. text_id & 0x3fff == 0 leaves the
    // slot blank (just the fill, which clears any previous verb).
    fn draw_command_menu_item(&mut self, slot: u8, text_id: u16) {
        // = seg000:d48a push [active_seg]; set_screen_as_active_framebuffer. When
        // a transition is mid-flight (in_transition > 0) DOS targets fb1 instead.
        let saved = self.active_fb();
        if (self.in_transition as i8) > 0 {
            self.set_fb1_as_active_framebuffer();
        } else {
            self.set_screen_as_active_framebuffer();
        }

        // = seg000:d49b font_select_small_font; di = ui_elements[7+slot].
        self.font_select_small_font();
        let row = &self.ui_elements[7 + slot as usize];
        // = seg000:d4aa bx = row.y0 + 1; dx = 0x5d; font_set_draw_position(x, y).
        let y = row.y0 + 1;
        let x = 0x5du16;
        self.font_set_draw_position(x, y);

        // = seg000:d4b4 font_draw_bg_color (= the font colour word's bg/high byte)
        // = 0xf3, the row background colour. The text bg matches the row fill below,
        // so the whole row reads as a uniform 0xf3 band.
        let mut bg = 0xf3u8;
        // = seg000:d4b9 and ui_elements[7+slot].flags low byte, 0x7f — clear the
        // "enabled" bit; the draw below re-sets it for a live verb.
        self.ui_elements[7 + slot as usize].flags &= 0xff7f;

        // = seg000:d4bf si &= 0x3fff — the bare string id (0 = blank slot).
        let id = text_id & 0x3fff;
        if id != 0 {
            // = seg000:d4c5 al = 0xf5 (the greyed foreground colour).
            let mut fg = 0xf5u8;
            // = seg000:d4c7 test ah,40h — the 0x4000 greyed flag stays 0xf5.
            if text_id & 0x4000 == 0 {
                // = seg000:d4cc set the enabled bit; al = 0xfa (the live colour).
                self.ui_elements[7 + slot as usize].flags |= 0x80;
                fg = 0xfa;
                // = seg000:d4d2 the 0x8000 highlight flag swaps fg/bg (inverse):
                // = seg000:d4d6 xchg al,[font_draw_bg_color].
                if text_id & 0x8000 != 0 {
                    std::mem::swap(&mut fg, &mut bg);
                }
            }
            // = seg000:d4da font_draw_fg_color = al (fg). The bg byte is
            // font_draw_bg_color above; together the colour word is (bg << 8) | fg.
            self.font_state.color = ((bg as u16) << 8) | fg as u16;
            // = seg000:d4dd resolve the string; = seg000:d4e0 a leading space;
            // = seg000:d4e6 font_draw_string.
            let s = self.get_phrase_or_command_string(id).to_vec();
            self.font_draw_glyph(b' ');
            self.font_draw_string(&s);
        }

        // = seg000:d4e9 fill the rest of the row (current pen x .. 0xe3, y .. y+7)
        // with the background colour, clearing whatever the slot held before.
        let (pen_x, pen_y) = self.font_get_draw_position();
        gfx::vga_fill_rect(self, pen_x, pen_y, 0xe3, pen_y + 7, bg);

        // = seg000:d50a pop [active_seg].
        self.active_fb = saved;
    }

    // = seg000:3090 build_persons_in_room_records — append the people-present
    // records to the verb list. Resets the four person slots (init_room_persons),
    // clears persons_in_room, then scans the room-person table at seg001:0fd8
    // twice: first pass picks up entries with flags bit 0x40 clear (template
    // loc_030b9), second pass the ones with it set (template loc_03120). The
    // DOS routine advances di past the existing list before appending; the port
    // keeps command_menu_records as a Vec and appends.
    //
    fn build_persons_in_room_records(&mut self) {
        // = seg000:3090 call reset_scene_lip_sync_state.
        self.reset_scene_lip_sync_state();
        // = seg000:3093 call init_room_persons.
        self.init_room_persons();
        // = seg000:3096..30a0 find the terminator of the existing command list.
        // The port's command_menu_records is a Vec, so appending past the end
        // is implicit.
        // = seg000:30a1 persons_in_room = 0.
        self.persons_in_room = 0;
        // = seg000:30a9 bp = build_room_person_record_a (flags bit 0x40 clear).
        self.scan_matching_room_person_entries(Self::build_room_person_record_a);
        // = seg000:30af bp = build_room_person_record_b (flags bit 0x40 set).
        self.scan_matching_room_person_entries(Self::build_room_person_record_b);
        // = seg000:30b5 xor ax,ax; stosw — the DOS terminator. The Vec needs none.
    }

    // = seg000:36ee scan_matching_room_person_entries — walk the 16-entry
    // room-person table at seg001:0fd8; for each entry whose
    // (location_and_room, location_appearance) matches the current room, invoke
    // `builder` with the entry and its 0..15 index. DOS passes the entry's
    // seg001 pointer in si; the index lets a builder reconstruct that pointer
    // when it stores it elsewhere (e.g. template-a's data_047aa write).
    fn scan_matching_room_person_entries(&mut self, builder: fn(&mut Self, u8, &RoomPerson)) {
        // = seg000:36f0..36f6 si = 0fd8h; cx = 0x10; bx = location_appearance;
        //   dx = location_and_room.
        for index in 0..self.room_persons.len() {
            // Snapshot the entry: RoomPerson is Copy, and the builder needs
            // `&mut self` so we cannot keep a borrow into self.room_persons
            // live across the call. The classification path that mutates the
            // table runs in init_room_persons before this scan, so a snapshot
            // here matches DOS behavior.
            let entry = self.room_persons[index];
            // = seg000:36fe cmp bx, game_time[si]; cmp dx, rand_bits[si].
            if entry.location_appearance == self.location_appearance
                && entry.location_and_room == self.location_and_room
            {
                // = seg000:370e call bp.
                builder(self, index as u8, &entry);
            }
        }
    }

    // = seg000:36d3 run_room_leave_dialogue_scan — the data_00023-gated room-person dialogue scan run
    // when leaving a room (ui_click_move_room) or re-entering one. When the leave
    // flag is set, walk the standing room-persons (bp = room_person_present_auto_dialogue) so one of them
    // can speak an auto-dialogue line, then clear the flag.
    pub(crate) fn run_room_leave_dialogue_scan(&mut self) {
        // = seg000:36d3 cmp byte [data_00023], 0; jz ret.
        if self.data_00023 == 0 {
            return;
        }
        // = seg000:36da call tear_down_prior_talking_head_overlay — when a prior
        //   head overlay is up, restore the game area and drop it before a
        //   standing person speaks over it.
        self.tear_down_prior_talking_head_overlay();
        // = seg000:36dd mov byte [data_047a7], 0 — clear the "someone spoke" latch.
        self.data_047a7 = 0;
        // = seg000:36e2 bp = room_person_present_auto_dialogue; call scan_matching_room_person_entries.
        self.scan_matching_room_person_entries(Self::npc_auto_dialogue);
        // = seg000:36e8 mov byte [data_00023], 0.
        self.data_00023 = 0;
    }

    // = seg000:3520 room_person_present_auto_dialogue — per standing room-person, present their auto-
    // dialogue line if its condition matches and, having spoken, install the
    // person's dialogue verb menu. data_047a7 latches after the first person
    // speaks so only one interrupts the move.
    //
    // MINIMAL PORT: the present path (present_room_person_dialogue) and the verb-menu install
    // (loc_03595) are modelled. Deferred: the messages_02aaf queued-message path
    // taken when no line is selected (seg000:3533), and the data_00023 == 3 / == 4
    // come-with-me / special-menu branches (seg000:3555..3592) — the room-leave
    // scan runs with data_00023 == 1.
    fn npc_auto_dialogue(&mut self, _index: u8, entry: &RoomPerson) {
        // = seg000:3520 cmp byte [data_047a7], 0; jnz ret — someone already spoke.
        if self.data_047a7 != 0 {
            return;
        }
        // = seg000:3527 al = entry.person_index; call present_room_person_dialogue — present this
        //   person's topic-4 auto-dialogue line if a condition selects one.
        // = seg000:3531 jnb loc_03542 — only continue to the menu install when a
        //   line was actually spoken; otherwise DOS takes the messages_02aaf path
        //   (not modelled), which does not install the verb menu for our case.
        if !self.present_room_person_line(entry.person_index) {
            return;
        }

        // = seg000:3551 loc_03551 inc byte [data_047a7] — latch so no other
        //   standing person speaks during this scan.
        self.data_047a7 = self.data_047a7.wrapping_add(1);

        // = seg000:3595 loc_03595 — the data_00023 == 1 (room-leave) path. The
        //   data_04774 gate (seg000:3595) and the data_00023 >= 0x64 guard
        //   (seg000:359c) both pass for value 1.
        // = seg000:35a3 ax = current_lip_sync_resource_id; 35a6 call
        //   set_dialogue_speaker — mark the speaker met and stage their dialogue
        //   verb panel (TALK TO ME / COME WITH ME / WHAT? / STOP TALKING).
        let speaker = self.current_lip_sync_resource_id as u8;
        self.set_dialogue_speaker(speaker);
        // = seg000:35a9 call play_pending_panel_fold — reveal the staged verb panel
        //   with the accordion fold (animating the speaker's mouth through it).
        self.play_pending_panel_fold();
    }

    // = seg000:30b9 build_room_person_record_a — template-a builder for
    // scan_matching_room_person_entries. Skip when the entry's flags bit 0x40
    // is set; on the first non-skipped match, capture the entry's si into
    // data_047aa so draw_room_game_screen's tail picks it as the lip-sync
    // speaker; then fall into the shared body.
    fn build_room_person_record_a(&mut self, index: u8, entry: &RoomPerson) {
        // = seg000:30b9 test byte ptr [si+0fh], 40h; jnz ret.
        if entry.flags & 0x40 != 0 {
            return;
        }
        // = seg000:30bf cmp [data_047aa], 0; jnz loc_030ca.
        if self.data_047aa == 0 {
            // = seg000:30c6 mov [data_047aa], si — the matched entry's
            //   seg001 pointer, 0x0fd8 + index * 0x10.
            self.data_047aa = ROOM_PERSON_TABLE_BASE + (index as u16) * 0x10;
        }
        // = seg000:30c9 jmp loc_030ca (fall through).
        self.build_room_person_record_body(entry);
    }

    // = seg000:3120 build_room_person_record_b — template-b builder. Mirror of
    // template-a with the flags-bit-0x40 test inverted: process only entries
    // whose bit 0x40 is set by jumping into the shared body, otherwise return
    // without touching data_047aa. The static room-person table has no
    // bit-0x40 entries, so this only fires once game state writes the bit at
    // runtime.
    fn build_room_person_record_b(&mut self, _index: u8, entry: &RoomPerson) {
        // = seg000:3120 test byte ptr [si+0fh], 40h; jnz loc_030ca.
        if entry.flags & 0x40 == 0 {
            return;
        }
        self.build_room_person_record_body(entry);
    }

    // = seg000:30ca build_room_person_record_body — shared tail of the two
    // templates. Emit a verb-menu record (text_id = 0x78 + person_index,
    // handler = entry.handler), OR (1 << person_index) into persons_in_room,
    // and — only when person_index == 0x0f — emit `data_0476a - 1` chained
    // 0x88.. records, then patch one of them to 0x8f when game_phase >= 5
    // and data_0476b is non-zero.
    fn build_room_person_record_body(&mut self, entry: &RoomPerson) {
        // = seg000:30ca mov al, [si+0eh] — entry.person_index. The DOS disasm
        //   spells it `_word_1F4BE_persons_met[si]`, but that resolves to
        //   `[si + persons_met_offset(0x0e)]` — the byte at offset 0x0e
        //   inside the room-person entry, not the global persons_met word.
        let cl = entry.person_index;
        // = seg000:30cf..30d4 ax = 0x78 + cl; stosw — the verb's text_id.
        let text_id = 0x78u16 + cl as u16;
        // = seg000:30d5..30da persons_in_room |= 1 << cl.
        self.persons_in_room |= 1u16 << cl;
        // = seg000:30de..30e1 ax = [si+4] (= entry.handler); stosw.
        self.command_menu_records.push(CommandMenuRecord {
            text_id,
            handler: entry.handler,
        });

        // = seg000:30e2 cmp cl, 0fh; jnz loc_0311f — only the cl==0x0f case
        //   runs the chained-records loop and the game_phase patch.
        if cl != 0x0f {
            return;
        }

        // = seg000:30e7..30ee cx = data_0476a; dec cx; jle loc_030fe — the
        //   loop runs (data_0476a - 1) times, emitting one extra record per
        //   iteration. data_0476a == 0 sentinels skip the loop entirely.
        let chained = (self.data_0476a as i16).saturating_sub(1).max(0) as usize;
        // = seg000:30f3 mov ax, 0x87; the inc-then-store sequence yields
        //   text_ids 0x88, 0x89, …; each shares entry.handler.
        let base_handler = entry.handler;
        for k in 0..chained {
            self.command_menu_records.push(CommandMenuRecord {
                text_id: 0x88 + k as u16,
                handler: base_handler,
            });
        }

        // = seg000:30fe cmp [game_phase], 5; jb loc_0311f.
        if self.game_phase < 5 {
            return;
        }
        // = seg000:3105..310a mov al, [data_0476b]; or al,al; jz loc_0311f.
        if self.data_0476b == 0 {
            return;
        }
        // = seg000:310d..3118 ax = (data_0476b - 1 - data_0476a) * 4; di += ax.
        //   di was at the end of the just-pushed run, so this lands on a
        //   record's text_id slot some signed number of 4-byte records away.
        //   Within the body's run of `data_0476a` records (indices
        //   base..base+data_0476a-1 in command_menu_records), DOS lands on
        //   index base + (data_0476b - 1). Skip if that falls outside the run
        //   — DOS would silently corrupt adjacent memory.
        let run_len = self.data_0476a as usize;
        let target_within_run = (self.data_0476b as usize).wrapping_sub(1);
        if target_within_run >= run_len {
            return;
        }
        let base = self.command_menu_records.len() - run_len;
        // = seg000:311a mov word ptr [di], 0x8f — patch the text_id.
        self.command_menu_records[base + target_within_run].text_id = 0x8f;
    }

    // = seg000:3127 init_room_persons — reset the scene's dynamic person slots
    // before build_persons_in_room_records walks the room-person table.
    //
    // The unconditional reset:
    //   - data_0476a / data_0476b cleared so build_room_person_record_body
    //     emits no chained 0x88.. records and applies no 0x8f patch unless the
    //     classification path below grows them.
    //   - room_persons[12..16].location_appearance = 0x7f80 — the four "scene"
    //     entries (the last 4 rows of the seg001:0fd8 table at addresses
    //     data_0109a/10aa/10ba/10ca). 0x7f80 cannot match any room's
    //     location_appearance, so the scan ignores them until the classification
    //     overwrites both location_and_room and location_appearance with values
    //     that do match.
    //
    // The location_appearance.lo == 0x80 special-room branch (most rooms, including
    // the palace at 0x180) classifies the room-person linked list reachable
    // through command_list_ptr: walks data_00009[command_list_ptr] via the
    // shared loc_06603 iterator with bp = loc_0316e (which buckets entries by
    // travel-mate/day/etc. and writes back into room_persons[12], [14], [15]
    // plus data_0476a/b), then specially handles data_00008[command_list_ptr]
    // == 0x21 by writing room_persons[13] and calling loc_02318, then runs
    // loc_0331e. The port has none of those structures yet: command_list_ptr,
    // the loc_06906 entry decoder, loc_0316e, loc_0331e, loc_02318. While
    // those are stubs the dynamic slots stay at 0x7f80, which is what the
    // unconditional reset above already establishes — so the scan behaves
    // exactly as DOS does for the "no classification ran" steady state, with
    // entries 12..16 contributing nothing to the verb panel.
    fn init_room_persons(&mut self) {
        // = seg000:3127..312f mov byte ptr [data_0476b], 0; same for 0476a.
        self.data_0476a = 0;
        self.data_0476b = 0;
        // = seg000:3131..313d mov ax, 0x7f80; stored into
        //   room_persons[15/14/13/12].location_appearance.
        for i in 12..16 {
            self.room_persons[i].location_appearance = 0x7f80;
        }
        // = seg000:3140..3147 bx = location_appearance; cmp bl,0x80; jnz loc_0316d —
        //   the classification chain runs only for "special" rooms whose
        //   location_appearance low byte is 0x80 (the palace 0x180 and most others).
        if (self.location_appearance & 0xff) as u8 == 0x80 {
            // = seg000:3149..316a the classification chain on command_list_ptr.
            // TODO: port once command_list_ptr, loc_06603/loc_06906 iteration,
            //   loc_0316e (room-person bucketing), loc_0331e and loc_02318
            //   land. Until then the dynamic room_persons[12..16] stay at
            //   0x7f80 and the verb panel reflects only the static-table
            //   matches.
        }
    }

    // = seg000:2ffb rebuild_and_draw_room_nav_panel — flip the four compass
    // direction buttons (ui_elements[13..17]) between visible-and-clickable
    // (flags 0x80) and hidden (flags 0x20) according to the current scene's
    // four direction-exit bytes, then redraw HUD records 12..18.
    //
    // The DOS routine also handles three special cases (night attack, map/book
    // mode, sietch entrance) by re-installing alternate templates; the port
    // leaves whatever `ui_setup_and_draw_nav_panel` already placed for those modes
    // and only customizes the standard palace/sietch room path. The command-
    // panel identity (`command_list_ptr`) gate on the centre element [17] is
    // also approximated: until command_list_ptr lands in the port we keep the
    // centre at its template default. The data_01cc4 mirror is dropped — no
    // consumer is ported yet.
    fn rebuild_and_draw_room_nav_panel(&mut self) {
        // = seg000:2ffb cmp byte ptr [night_attack_stage], 0; jnz loc_0301a (alt). The
        // night-attack path leaves the existing panel in place and just
        // redraws.
        if self.night_attack_stage != 0 {
            self.ui_draw_nav_panel();
            return;
        }
        // = seg000:3002 test game_screen_mode_flags,3; jz loc_03020. Book/map
        // mode also leaves whatever ui_setup_and_draw_nav_panel placed (the
        // NAV_PANEL_MAP template) and just redraws.
        if self.game_screen_mode_flags & 3 != 0 {
            self.ui_draw_nav_panel();
            return;
        }
        // = seg000:3020 mov bx,[data_00006]; cmp bl,80h; jnz loc_03073. Only
        // "special" rooms (location_appearance low byte 0x80) get the per-scene
        // compass rebuild; everything else gets the alt all-clickable template.
        let bl = (self.location_appearance & 0xff) as u8;
        let dh = (self.location_and_room >> 8) as u8;
        let room = (self.location_and_room & 0xff) as u8;

        if bl != 0x80 || dh == 0x21 {
            // = seg000:3073 alt template: all four directions clickable with
            //   sprite_ids 0x1d..0x20, box [12].sprite_id = 0x23.
            self.ui_elements[12].sprite_id = 0x23;
            for i in 0..4 {
                self.ui_elements[13 + i].flags = 0x80;
                self.ui_elements[13 + i].sprite_id = 0x1d + i as i16;
            }
            self.ui_draw_nav_panel();
            return;
        }
        // = seg000:3032 call loc_03efe; inc si — fetch the current scene's
        //   four direction-exit bytes. None falls back to the alt path (the
        //   lookup is only None during startup transitions where the room is
        //   undefined).
        let Some(exits) = self.current_scene_exits() else {
            self.ui_draw_nav_panel();
            return;
        };

        // = seg000:3045 bx = 0x21; if dl == 1: bx = 0x22 — the box backing
        //   sprite_id depends on whether this is the location's entry room.
        let box_sprite_id = if room == 1 { 0x22 } else { 0x21 };
        self.ui_elements[12].sprite_id = box_sprite_id;
        // = seg000:305c the exit-classification loop: for each compass
        //   direction (i = 0..3 → UP / RIGHT / DOWN / LEFT), show the arrow
        //   (flags 0x80) only when the exit byte is in 0xFB..0xFF; otherwise
        //   hide it (flags 0x20). Destination-room exits (0x01..0x7F) and
        //   in-scene/scripted exits don't get a HUD arrow.
        for (i, exit) in exits.iter().enumerate() {
            let exit = *exit as i8;
            let flag = if exit != 0 && exit >= -5 { 0x80 } else { 0x20 };
            self.ui_elements[13 + i].flags = flag;
        }
        // = seg000:3070 jmp loc_0d735 — fall into the panel redraw.
        self.ui_draw_nav_panel();
    }

    // = seg000:301a loc_0301a — render the active dialogue into the command-panel
    // area (the data_04774 != 0 branch).
    // TODO: port the dialogue text system; no-op stub.
    fn draw_dialogue_panel(&mut self) {}

    // = seg000:98e6 reset_scene_lip_sync_state — tear down the current scene's
    // talking head and its frame tasks before the room/panel is re-presented.
    // Without this the LOOK AT MIRROR idle animator keeps compositing Paul over
    // the bedroom after the player looks away.
    fn reset_scene_lip_sync_state(&mut self) {
        // = seg000:98f5 loc_098f5 — clear the head/portrait/dialogue UI element
        // flags. Element 20 carries the LOOK AT MIRROR game-area hotspot that
        // callback_transition_look_at_mirror armed (flags = 0x80).
        for idx in [18, 19, 20] {
            self.ui_elements[idx].flags = 0;
        }
        // = seg000:98e9 data_047aa = 0 — forget the per-scene speaker. (DOS also
        // clears data_047c8 and current_bubble_layout_ptr, not modelled here.)
        self.data_047aa = 0;

        // = the loc_098e6 tail `jmp loc_09b8b`.
        self.stop_lip_sync_and_remove_idle_head_task();
    }

    // = seg000:9b8b stop_lip_sync_and_remove_idle_head_task — stop any voice
    // lip-sync, clear the presenter state (data_047c3 / data_047ce / data_047d1
    // bit 7 — kept inside TalkingHead in the port) and, when a head is active
    // (data_047c6 != 0), remove the idle animator frame task
    // (frame_task_callback_099be) and drop it.
    pub(crate) fn stop_lip_sync_and_remove_idle_head_task(&mut self) {
        // = seg000:9b8b call lip_sync_stop.
        self.lip_sync_stop();
        // = seg000:9b9d xchg ax,[data_047c6]; or ax,ax; jz loc_09bab.
        if self.talking_head.is_some() {
            self.remove_frame_task(crate::TaskId::TalkingHeadIdle);
            self.talking_head = None;
        }
    }

    // = seg000:2ee5 call_restore_cursor (rect 0dbech) — repaint the saved
    // background under the hardware mouse cursor before the verbs redraw, so a
    // stale cursor image is not baked into the panel.
    // TODO: port the software-cursor save/restore; no-op stub.
    pub(crate) fn restore_cursor_over_panel(&mut self) {}

    // = seg000:2ec6 the dialogue branch's jmp screen_element_stack_push (loc_02ebf:
    // bp = [data_02220] the dialogue record buffer, bx = 0f66h). It would install
    // the dialogue panel as the active screen element. The dialogue record / draw
    // system is not ported (the data_04774 branch never runs with the default
    // flags), so this is a no-op stub.
    // TODO: port the dialogue panel; no-op stub.
    fn draw_task_list_insert(&mut self) {}

    // = seg000:0acd stage_28_night_attack_start. The night attack on the sietch:
    // an ATTACK.HSQ background with a particle system (bombs, debris, sky
    // flashes). The whole algorithm is ported in dune::attack::AttackState —
    // AttackState::new() loads ATTACK.HSQ and draws the tiled background
    // (= the blit_repeated_x / draw_icons_list_at_si setup), step_frame() is the
    // loc_00b45 particle tick, and draw() blits the result + palette out.
    //
    //   seg000:0ad9 open_onmap_spritesheet (ATTACK.HSQ)        ; AttackState::new
    //   seg000:0ae2..0af8 blit the tiled background + icons ; AttackState::new
    //   seg000:0b10 add_frame_task(loc_00b45, bp=3)         ; the task below
    //   seg000:0b19 copy_active_framebuffer_to_framebuffer_2 ; AttackState bg
    //   seg000:0b1e al=3; audio_start_voc (SN3.VOC)         ; the attack sound
    //
    // The AttackState is owned by the frame-task closure (like the sky cycler).
    // The sim uses AttackState's fixed default RNG seeds rather than the game
    // RNG ([2786h] etc.), so the pattern is plausible but not bit-identical to a
    // particular DOS run.
    pub(crate) fn night_attack_start(&mut self) {
        // = seg000:0ad9 open_onmap_spritesheet (ATTACK.HSQ). Seed the attack with
        // the live in-game palette so its ATTACK.HSQ palette overlays (rather
        // than replaces) the existing one — DOS applies it via
        // apply_sprite_sheet_palette, which only touches ATTACK.HSQ's own
        // entries. Seeding here also makes the field the same instance the
        // Intro28Attack frame task ticks (tick_intro_28).
        let spritesheet_data = self.dat_file.read("ATTACK.HSQ").unwrap();
        self.attack = Some(AttackState::new(&self.palette, &spritesheet_data));
        // Draw the static background into fb1 so the stage's 0x10 transition
        // reveals it; the task then animates the particles over it.
        self.attack
            .as_ref()
            .unwrap()
            .draw(&mut self.framebuffer, &mut self.palette);
        // = add_frame_task(loc_00b45, bp=3): one particle tick every 3 ticks.
        // play_intro's wait_for_pcm_voice_interruptable(2000) drives it.
        self.add_frame_task(3, crate::TaskId::IntroNightAttack);
        // = seg000:0b1e mov al,3; jmp audio_start_voc — the night-attack sound.
        // al=3 -> resource RES_SN3_HSQ; the DAT stores it (HSQ-compressed) as
        // SN3.HSQ, which dat_file decompresses to the underlying .voc.
        self.audio_start_voc("SN3.HSQ");
    }

    // = seg000:0b21
    pub(crate) fn _clear_night_attack(&mut self) {
        // TODO
    }

    // = seg000:0b45
    pub(crate) fn tick_intro_night_attack(&mut self) {
        self.attack.as_mut().unwrap().step_frame();
        self.attack
            .as_mut()
            .unwrap()
            .draw(&mut self.screen, &mut self.palette);
        self.send_frame_to_display();
    }

    // = seg000:e387 wait_a_bit(ax=8) — the per-step pause of the head fold,
    // spinning on the PIT counter (which services frame tasks) for 8 ticks. A
    // no-op while rendering offscreen, where the fold is invisible and the
    // transition reveals only the final frame, so the per-step waits are skipped.
    pub(crate) fn wait_a_bit_for_head_fold(&mut self) {
        if self.front_buffer_is_fb1() {
            return;
        }
        self.wait_frame_tasks_for_ticks(8);
    }

    // = seg000:488a loc_0488a — draw the extra per-location overlay SAL (opened
    // via calc_SAL_index entry 6) when data_04732 bit 0 is set.
    // TODO: port; no-op stub.
    fn draw_location_overlay_sal(&mut self) {}

    // = seg000:5ba0 copy_game_area_rect_to_unknown_rect — copy the game-area rect
    // (si=1470h) to the backdrop buffer (di=0d83ch) before drawing the room.
    // TODO: port; no-op stub.
    fn copy_game_area_rect_to_unknown_rect(&mut self) {}

    // = seg000:37b2 draw_room_scene — clear the game area and draw the current
    // location/room scene (= draw_SAL, seg000:3b59). The port's draw_location_room
    // does the SAL open + draw together (it also runs clear_game_area), driven by
    // the current location_and_room / location_appearance globals (data_00006).
    //
    // The DOS prologue (loc_098e6, loc_04d00, copy_game_area_rect_to_clip_rect)
    // and the room-byte >= 0x80 character branch are not ported yet.
    fn draw_room_scene(&mut self) {
        // = seg000:37b2 call reset_scene_lip_sync_state — tear down any active
        // talking head (and its idle/voc frame tasks) before redrawing the room,
        // so the LOOK AT MIRROR head stops compositing once the player looks away.
        self.reset_scene_lip_sync_state();
        self.draw_location_room(self.location_and_room, self.location_appearance);
    }

    // = seg000:c108 transition — render the new screen offscreen via `render`
    // (DOS passes the routine in bp and runs it through
    // gfx_call_bp_with_front_buffer_as_screen), then wipe it onto the visible
    // screen with effect `effect` (DOS al, via the segvga vga_transition) and
    // flush the palette.
    fn transition(&mut self, effect: u8, render: fn(&mut GameState)) {
        // = seg000:c108 in_transition = 0x80.
        self.in_transition = 0x80;
        // = seg000:c10f run the render routine with the front buffer redirected
        // to fb1 (offscreen) so its draws land in fb1 without touching the
        // visible screen.
        self.gfx_call_bp_with_front_buffer_as_screen(render);
        // = seg000:c124 vga_transition — dissolve/wipe the offscreen fb1 onto
        // the visible screen with effect `effect` (DOS al). The implemented
        // effects present their own intermediate frames as the wipe runs;
        // effects vga_transition does not yet handle simply fall through to the
        // plain copy below. DOS passes the caller's dx as the direction byte;
        // every caller of `transition` sets dx = 0 (look_at_mirror `xor dx,dx`,
        // ui_present_room_screen `dx = 0`), so the wipe runs in its default
        // direction.
        gfx::vga_transition(self, effect as u16, 0);
        // = seg000:c12a gfx_copy_whole_framebuf_to_screen — leave the final fb1
        // image on the screen (also covers the not-yet-ported effects).
        self.gfx_copy_whole_framebuf_to_screen();
        // = seg000:c12d vga_palette_flush.
        self.update_screen_palette();
        // DOS wrote straight to VGA memory (visible as the wipe ran); the port
        // renders into `screen`, so push the final frame to the display now.
        self.send_frame_to_display();
        // = seg000:c131 in_transition = 0.
        self.in_transition = 0;
    }

    // = seg000:35ad loc_035ad — post-render room-screen bookkeeping (clears
    // data_0001a/047a7 and consumes data_047a6 when game_screen_mode_flags == 0).
    // TODO: port; no-op stub.
    fn finish_room_screen_setup(&mut self) {}

    // = seg000:3723 loc_03723 — handle the pending dialogue / auto-action queued
    // in data_04735.
    // TODO: port; no-op stub.
    fn handle_pending_dialogue_action(&mut self) {}

    // = seg000:978e start_room_lip_sync — start the current speaker's lip-sync
    // (current_lip_sync_resource_id; 0xffff = none).
    // TODO: port; no-op stub.
    fn start_room_lip_sync(&mut self) {}
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::ScreenElement;
    use crate::{Equipment, GameState, dat_file::DatFile};

    // = seg000:7f27/7f2a — the location available-equipment computation: the
    // location's equipment row minus each stationed troop's held equipment, per
    // the troop->equipment bitmask, clamped at 0. Runs on the game-start state,
    // so the values are those of the EXE's static seg001 location/troop tables.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn location_available_equipment_subtracts_troop_holdings() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let game = GameState::new(dat_file, tx);
        let avail = |loc: usize| {
            let e: Equipment = game.location_available_equipment(&game.locations[loc]);
            [
                e.harvesters,
                e.ornithopters,
                e.krys_knives,
                e.laser_guns,
                e.weirding_modules,
                e.atomics,
                e.bulbs,
            ]
        };
        // Palace (loc 0): no equipment, no troops.
        assert_eq!(avail(0), [0, 0, 0, 0, 0, 0, 0]);
        // Loc 7: [2,2,3,3,3,0,0], troops 27->28->29 each hold 0x38 (knife,guns,
        // mods) — slots 2/3/4 drop to 0; ornithopters (slot 1) untouched.
        assert_eq!(avail(7), [2, 2, 0, 0, 0, 0, 0]);
        // Loc 2: [2,2,3,3,3,3,0]; the troop chain leaves one atomic and clamps
        // knife/guns/mods at 0 (saturating subtraction never underflows).
        assert_eq!(avail(2), [2, 2, 0, 0, 0, 1, 0]);
        // Loc 55: [2,1,1,1,1,1,0]; one ornithopter remains, the rest clamp at 0.
        assert_eq!(avail(55), [2, 1, 0, 0, 0, 0, 0]);
    }

    // The screen-element priority bytes drive dismiss_stacked_overlays: an element is
    // drained (popped) iff its byte is not 0xff and its low nibble is non-zero.
    // The base room menu and the locked look-away overlay (0xff) stop the drain;
    // the transient overlays (0xfc/0xf8/0xf6) are torn down. Pure (no assets).
    #[test]
    fn screen_element_priority_bytes_gate_the_drain() {
        assert_eq!(ScreenElement::RoomCommandMenu.priority_byte(), 0xff);
        assert_eq!(ScreenElement::LookAwayFromMirror.priority_byte(), 0xff);
        assert_eq!(ScreenElement::NpcActionsMenu.priority_byte(), 0xfc);
        assert_eq!(ScreenElement::MixerPanel.priority_byte(), 0xf8);
        assert_eq!(ScreenElement::PalacePlan.priority_byte(), 0xf8);
        assert_eq!(ScreenElement::ExitGameConfirmation.priority_byte(), 0xf6);

        let drained = |e: ScreenElement| {
            let p = e.priority_byte();
            p != 0xff && p & 0x0f != 0
        };
        // Stops the drain (base / locked).
        assert!(!drained(ScreenElement::RoomCommandMenu));
        assert!(!drained(ScreenElement::LookAwayFromMirror));
        // Transient overlays are drained.
        for e in [
            ScreenElement::NpcActionsMenu,
            ScreenElement::MixerPanel,
            ScreenElement::PalacePlan,
            ScreenElement::ExitGameConfirmation,
        ] {
            assert!(
                drained(e),
                "{e:?} should be drained by dismiss_stacked_overlays"
            );
        }
    }

    // Bug 0001: a mouseover on the Duke Leto sprite in the starting throne room
    // highlights his command verb, and a click on the sprite dispatches the same
    // person handler. Asset-gated (needs assets/DUNE.DAT); run with:
    //   cargo test -p dune --lib -- --ignored leto_sprite
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn leto_sprite_hover_and_click_resolve_to_his_verb() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        // Nothing reads _rx; skip frame publishing so the screen pushes along
        // the dialogue path cannot fill the channel and block.
        game.set_headless();
        // Skip the intro; renders the starting throne room (Duke Leto present).
        game.start(true);

        // Duke Leto is person index 0; draw_sal_room recorded his on-screen
        // anchor into character_screen_pos[0].
        let (lx, ly) = game.character_screen_pos[0];
        assert!(lx != 0xffff, "Leto's screen marker was not recorded");
        assert!(ly < 0x97, "Leto's anchor should sit in the room scene area");

        // His verb (text_id = 0x78 + person index 0) is in the command menu.
        let slot = game
            .command_menu_records
            .iter()
            .position(|r| r.text_id == 0x78)
            .expect("Duke Leto verb (text_id 0x78) not in the command menu");

        // The hitbox is a fixed person-sized box: mouse_x 1..=32 px right of the
        // anchor, mouse_y 1..=80 px below it. A cursor well inside it hits Leto.
        game.mouse_pos_x = lx + 16;
        game.mouse_pos_y = ly + 40;
        assert_eq!(game.person_hit_test(), Some(0));

        // ...but the box is bounded: a cursor past 32 px right or 80 px below the
        // anchor (e.g. over the background guard sprite) is not a hit.
        game.mouse_pos_x = lx + 48; // 48 > 32 px right
        game.mouse_pos_y = ly + 40;
        assert_eq!(game.person_hit_test(), None, "hitbox extends too far right");
        game.mouse_pos_x = lx + 16;
        game.mouse_pos_y = ly + 90; // 90 > 80 px below
        assert_eq!(game.person_hit_test(), None, "hitbox extends too far down");
        // Left of / above the anchor is a miss too.
        game.mouse_pos_x = lx.saturating_sub(8);
        game.mouse_pos_y = ly + 40;
        assert_eq!(
            game.person_hit_test(),
            None,
            "hitbox extends left of anchor"
        );

        // Hover over him highlights his verb slot.
        game.mouse_pos_x = lx + 16;
        game.mouse_pos_y = ly + 40;
        game.highlight_hovered_text_action_item();
        assert_eq!(
            game.index_of_last_hovered_action_item as usize, slot,
            "hover did not highlight Leto's verb slot"
        );

        // Click on the sprite dispatches his person handler (common_dialogue),
        // showing his talking head over the zoomed throne room.
        game.callback_main_ui_element_21_22();
    }

    // Bug 0001 (cont.): clicking Duke Leto's verb runs the ported dialogue entry
    // (common_dialogue -> dialogue_zoom_room + setup_talking_head), zooming the
    // throne room in on him and compositing his LETO.HSQ talking head over it.
    // Asset-gated; run with:
    //   cargo test -p dune --lib -- --ignored leto_dialogue
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn leto_dialogue_zooms_room_and_shows_head() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.start(true); // throne room, Duke Leto present.

        // Park the pointer in the command panel: the head-present chain brackets
        // each push with the DOS cursor restore/draw (seg000:9a13/9a19), so a
        // pointer inside the game area would bake cursor pixels into the rows
        // the assertions below compare.
        game.mouse_pos_x = 8;
        game.mouse_pos_y = 0xa8;

        // Snapshot the plain room game area before the dialogue.
        let before: Vec<u8> = game.framebuffer.pixels().to_vec();
        // Keep the last room frame start() presented (its command panel shows the
        // room verbs) so we can confirm the dialogue verbs replace them on screen.
        let room_screen = {
            let mut last = None;
            while let Ok(frame) = rx.try_recv() {
                last = Some(frame);
            }
            last.expect("start() presented a room frame").0
        };

        // Duke Leto is person index 0 (lip-sync resource id 0 -> LETO.HSQ).
        game.common_dialogue(0x0);

        // His talking head is now active over the room backdrop.
        assert!(
            game.talking_head.is_some(),
            "common_dialogue did not show a talking head"
        );

        // The game area changed: dialogue_zoom_room 4x-zoomed the room and
        // setup_talking_head composited Leto's face on top, so the framebuffer
        // must differ from the plain room across many game-area pixels.
        let after = game.framebuffer.pixels();
        let changed = before
            .iter()
            .zip(after.iter())
            .take(320 * 152)
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 320 * 152 / 4,
            "expected the zoom + head to redraw most of the game area, only {changed} px changed"
        );

        // present_dialogue_head (loc_0c4dd) must push the zoomed backdrop + head to
        // the visible SCREEN, and the panel fold (play_pending_panel_fold / play_pending_panel_fold)
        // must present its 17 frames. Collect every frame the dialogue presented.
        let frames: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            frames.len() >= 18,
            "expected the head present + 17+ fold frames, got {}",
            frames.len()
        );
        let (screen, _palette) = frames.last().cloned().unwrap();

        // The last presented frame's game area must match the framebuffer's game
        // area (the head present; the panel fold never touches y < 152). Compare
        // above the HUD-head ornament box (y >= 0x89 = 137, x 126..194): the
        // present chain (present_screen_rect = seg000:c4fb) re-stamps the
        // ornament into fb1 on every overlapping push, so fb1 can hold a fresher
        // ornament than the last pushed rect carried — same as DOS.
        assert_eq!(
            &screen.pixels()[..320 * 137],
            &game.framebuffer.pixels()[..320 * 137],
            "presented screen game area does not match the composited backdrop + head"
        );

        // The final panel must show the dialogue verbs (TALK TO ME / COME WITH ME /
        // ...) — i.e. differ from the plain room's verbs on the VISIBLE screen.
        let panel = |p: &[u8]| p[320 * 152..320 * 200].to_vec();
        let panel_changed = panel(room_screen.pixels())
            .iter()
            .zip(panel(screen.pixels()).iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            panel_changed > 50,
            "the dialogue verb panel did not reach the visible command panel \
             (only {panel_changed} px changed in the bottom strip)"
        );

        // The fold animated: at least one mid frame collapses the panel to the
        // solid 0xfe band (panel_solid_fill, frame 9). Count 0xfe only within the
        // panel columns (x 92..228) of the panel rows (y 159..198) — the rest of
        // each row is the nav panel / date strip.
        let collapsed = frames.iter().position(|(f, _)| {
            let px = f.pixels();
            let mut fe = 0usize;
            for y in 159..199 {
                for x in 92..228 {
                    if px[y * 320 + x] == 0xfe {
                        fe += 1;
                    }
                }
            }
            fe > 40 * 136 / 2
        });
        assert!(
            collapsed.is_some(),
            "no collapsed (solid-fill) frame found — the panel fold did not play"
        );

        // animate_panel_hands closed the ICONES hands (sprite_id % 3 -> 2) before
        // the fold and reopened them (-> 0) after, so they return to rest: the left
        // hand at ICONES sprite 0, the right at sprite 3.
        assert_eq!(
            game.ui_elements[1].sprite_id, 0,
            "left hand not back at rest"
        );
        assert_eq!(
            game.ui_elements[2].sprite_id, 3,
            "right hand not back at rest"
        );

        // Write PNGs for visual inspection: the final head + verbs, the collapsed
        // fold midpoint, and the hands-closed frame (frame 2 = head + 2 close steps).
        game.framebuffer
            .write_png_scaled(&game.palette, "leto_dialogue.png")
            .expect("write leto_dialogue.png");
        frames[collapsed.unwrap()]
            .0
            .write_png_scaled(&game.palette, "leto_dialogue_fold.png")
            .expect("write leto_dialogue_fold.png");
        frames[2]
            .0
            .write_png_scaled(&game.palette, "leto_dialogue_hands.png")
            .expect("write leto_dialogue_hands.png");
        eprintln!(
            "wrote leto_dialogue.png + _fold + _hands ({changed} game-area px changed, \
             {} dialogue frames, collapse at frame {})",
            frames.len(),
            collapsed.unwrap()
        );
    }

    // In the initial palace throne room (0x200a), before Duke Leto has been met,
    // clicking the DOWN compass button (the exit toward room 4) is interrupted:
    // ui_click_move_room runs the room-leave dialogue scan (run_room_leave_dialogue_scan -> room_person_present_auto_dialogue),
    // which presents Leto's topic-4 line "Where are you going so fast? I have to
    // talk to you." (phrase 0x81f). That line's stay_here event (0x02) clears
    // dialogue_interrupt_gate, so test_dialogue_interrupt_gate aborts the move and the player stays in 0x200a.
    // Clicking RIGHT (toward room 5) matches no condition and moves normally.
    // Asset-gated; run with:
    //   cargo test -p dune --lib -- --ignored leto_blocks_leaving
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn leto_blocks_leaving_throne_room() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(256);
        let mut game = GameState::new(dat_file, tx);
        game.start(true); // throne room (0x200a), Duke Leto present, not yet met.

        assert_eq!(
            game.location_and_room, 0x200a,
            "should start in the throne room"
        );
        assert_eq!(game.persons_met & 1, 0, "Leto must not be met at the start");

        // = ui_click_room_down (seg000:3f1f) — the throne room's DOWN exit is room 4.
        game.ui_click_move_down();

        // The move was interrupted: the gate was cleared, the room is unchanged, and
        // Leto's "where are you going so fast" line (phrase 0x81f) was presented over
        // his talking head.
        assert_eq!(
            game.dialogue_interrupt_gate, 0,
            "stay_here event did not clear the move gate"
        );
        assert_eq!(
            game.location_and_room, 0x200a,
            "the move should have been aborted"
        );
        assert_eq!(game.current_subtitle_id, 0x81f, "wrong line presented");
        assert!(
            game.talking_head.is_some(),
            "Leto's talking head was not shown"
        );

        // The dialogue verb menu switched in (set_dialogue_speaker -> setup_npc_
        // dialogue_menu pushed menu_NPC_actions), and Leto is now marked met.
        assert_eq!(
            game.screen_element_stack.last(),
            Some(&ScreenElement::NpcActionsMenu),
            "the Leto dialogue menu did not become active"
        );
        assert_eq!(
            game.persons_met & 1,
            1,
            "Leto should be marked met after speaking"
        );

        // Drain the frames the interrupt presented.
        while rx.try_recv().is_ok() {}

        // Clicking RIGHT (toward room 5) matches no auto-dialogue condition, so the
        // move proceeds and the room changes away from the throne room.
        game.ui_click_move_right();
        assert_eq!(
            game.dialogue_interrupt_gate, 0xff,
            "RIGHT should not have been interrupted"
        );
        assert_ne!(
            game.location_and_room, 0x200a,
            "RIGHT should have moved out of the throne room"
        );
    }

    // Bug 0001 (cont.): clicking STOP TALKING (the dialogue panel's text 0x94 verb
    // -> menu_callback_choice_exit_menu) ends the conversation and returns to the
    // un-zoomed room view: menu_npc_actions_cleanup (097cf) clears the zoom flag
    // and re-renders the room scene 1:1, so the game area snaps back from the 4x
    // dialogue zoom to the plain throne room and Leto's talking head is gone.
    // Asset-gated; run with:
    //   cargo test -p dune --lib -- --ignored stop_talking_unzooms
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn stop_talking_unzooms_back_to_the_room() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(256);
        let mut game = GameState::new(dat_file, tx);
        game.start(true); // throne room, Duke Leto present.

        // The plain (un-zoomed) room game area, before the dialogue.
        let plain: Vec<u8> = game.framebuffer.pixels()[..320 * 152].to_vec();

        // Talk to Leto: zoom the room in on him and composite his talking head.
        game.common_dialogue(0x0);
        let zoomed: Vec<u8> = game.framebuffer.pixels()[..320 * 152].to_vec();
        assert!(game.talking_head.is_some(), "dialogue did not show a head");

        // Drain the dialogue's presented frames so we can isolate the STOP TALKING
        // ones below.
        while rx.try_recv().is_ok() {}

        // STOP TALKING: the dialogue panel's text 0x94 verb dispatches 0xd2e2 =
        // menu_callback_choice_exit_menu, whose NpcActionsMenu cleanup un-zooms.
        game.menu_callback_choice_exit_menu();

        // The talking head is gone and the room is back to its un-zoomed self: the
        // restored game area matches the plain room far more closely than the zoom
        // did (the HUD head ornament strip is the only expected residual diff).
        assert!(
            game.talking_head.is_none(),
            "STOP TALKING left the talking head composited"
        );
        let restored = &game.framebuffer.pixels()[..320 * 152];
        let count_diff = |base: &[u8]| {
            base.iter()
                .zip(restored.iter())
                .filter(|(a, b)| a != b)
                .count()
        };
        let vs_plain = count_diff(&plain);
        let vs_zoomed = count_diff(&zoomed);
        assert!(
            vs_plain * 2 < vs_zoomed,
            "the room did not un-zoom: the restored game area is closer to the zoom \
             ({vs_zoomed} px differ) than to the plain room ({vs_plain} px differ)"
        );

        // present_dialogue_head (loc_0c4dd) pushed the un-zoomed game area to the
        // visible screen: the last presented frame's game area matches fb1.
        let frames: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let (screen, _palette) = frames.last().cloned().expect("STOP TALKING presented");
        assert_eq!(
            &screen.pixels()[..320 * 152],
            restored,
            "presented screen game area does not match the re-rendered room"
        );

        game.framebuffer
            .write_png_scaled(&game.palette, "leto_stop_talking.png")
            .expect("write leto_stop_talking.png");
        eprintln!("wrote leto_stop_talking.png ({vs_plain} px differ from the plain room)");
    }

    // Bug 0001 (cont.): clicking Leto loads the DIALOGUE resource and selects his
    // greeting sentence (menu_callback_choice_talk_to_me -> the topic walk ->
    // dialogue_interpret_record). Verifies the dialogue-record format end to end.
    // Asset-gated; run with:
    //   cargo test -p dune --lib -- --ignored leto_greeting
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn leto_greeting_sentence_is_selected() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        game.common_dialogue(0x0); // Duke Leto.

        // DIALOGUE.HSQ loaded and a greeting sentence was selected: its phrase id
        // is `(word1 byteswapped & 0x3ff) | 0x800`, so it lands in 0x800..=0xbff.
        let phrase = game.current_subtitle_id;
        assert!(
            (0x800..=0xbff).contains(&phrase),
            "expected a phrase id in 0x800..=0xbff, got {phrase:#x}"
        );

        // The voice line loaded and is playing: play_talking_head_voc found Leto's
        // .voc (PA\PA001I.VOC), parsed its lip-sync stream, and started PCM, so the
        // head is now speaking with a non-empty mouth stream.
        let head = game.talking_head.as_ref().expect("talking head gone");
        assert!(
            head.speaking && !head.voc_lipsync.is_empty(),
            "Leto's voice .voc (phrase {phrase:#x}) did not start playing"
        );
        // = loc_09f1c: starting the voice settles the head (id 0 < 0x10) into the
        // calm idle, so when the line ends no lively "talk" frames play.
        assert!(
            head.settled,
            "starting the voice should settle the head (loc_09f1c)"
        );
        eprintln!(
            "Leto greeting phrase id = {phrase:#x}, voc lip-sync frames = {}",
            head.voc_lipsync.len()
        );

        // = set_dialogue_speaker (seg000:93df) primed the cursor (person*8 = 0)
        // + verb mask and pushed menu_NPC_actions (loc_090bd); the talk walk
        // (seg000:94ab) then advanced the cursor past the matchless topic 0 to
        // topic 1, whose record holds the greeting. The next TALK TO ME resumes
        // inside that record (dialogue_resume_entry_ptr, seg000:94a5).
        assert_eq!(
            game.dialogue_topic_index, 1,
            "cursor advanced to Leto's topic 1"
        );
        assert_ne!(
            game.dialogue_resume_entry_ptr, 0,
            "the talk walk recorded a resume entry"
        );
        assert_eq!(game.data_047c2, 0x80, "verb mask primed to 0x80");
        assert_ne!(game.persons_met & 1, 0, "Leto marked as met");
        assert_ne!(game.persons_talking_to & 1, 0, "Leto marked as talking-to");

        // The dialogue verb panel is the active screen element, holding the four
        // menu_NPC_actions verbs (TALK TO ME / COME WITH ME / 0x95 / STOP TALKING).
        // Leto carries no travel/disabled flags, so slot 1 is the enabled COME
        // WITH ME (0x91, not greyed).
        assert_eq!(
            game.get_active_screen_element(),
            super::ScreenElement::NpcActionsMenu,
            "dialogue verb panel should be on top of the screen-element stack"
        );
        let verbs: Vec<u16> = game
            .command_menu_records
            .iter()
            .map(|r| r.text_id)
            .collect();
        assert_eq!(verbs, vec![0x90, 0x91, 0x95, 0x94], "NPC dialogue verbs");

        // Regression: when the voice finishes (lip_sync_stop) the voc task stops
        // and the head reverts to idle — mouth 0, not speaking. DOS does NOT force
        // a settle here; the idle finishes its lively animation and settles via the
        // countdown (see leto_idle_settles_to_calm_after_first_animation). The port
        // also drops prev_images so the resumed idle redraws cleanly. Force "done".
        let head = game.talking_head.as_mut().unwrap();
        head.prev_images = vec![(1, 0, 0)]; // a stale previous frame
        head.voc_total_samples = 0; // makes the next voc tick report "done"
        game.tick_talking_head_voc();
        let head = game.talking_head.as_ref().unwrap();
        assert!(!head.speaking, "voice should have stopped");
        assert_eq!(head.mouth, 0, "mouth should revert to closed (0)");
        assert!(
            head.prev_images.is_empty(),
            "prev_images must be dropped on voc end so the resumed idle redraws cleanly"
        );
    }

    // The TALK TO ME verb text tracks the voice (mark_talk_to_me_verb_talking /
    // mark_talk_to_me_verb_idle, seg000:d617/d61d): while the speaker talks,
    // slot 0 shows COMMAND string 0x90 ('   >>>>  TALK TO ME  <<<<'); when the
    // voice drains, lip_sync_stop (seg000:a7b1) flips it to 0x9f
    // ('" TALK TO ME "') and redraws the slot in place. Asset-gated:
    //   cargo test -p dune --lib -- --ignored talk_to_me_verb
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn talk_to_me_verb_flips_to_idle_when_the_voice_ends() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        game.common_dialogue(0x0); // Duke Leto; his greeting voice starts.
        assert!(
            game.talking_head.as_ref().is_some_and(|h| h.speaking),
            "Leto's voice should be playing"
        );
        // = seg000:a757 mark_talk_to_me_verb_talking ran as the voice started.
        assert_eq!(
            game.command_menu_records[0].text_id, 0x90,
            "the talking variant while the voice plays"
        );

        // Drain the voice without waiting it out: a zero clip length makes the
        // next lip-sync tick take the lip_sync_stop path.
        game.talking_head.as_mut().unwrap().voc_total_samples = 0;
        game.tick_talking_head_voc();
        assert!(!game.talking_head.as_ref().unwrap().speaking);

        // = seg000:d61d..d646 — slot 0 flipped in place, and the template
        // (seg001:1f80) carries the idle text for the next panel build.
        assert_eq!(
            game.command_menu_records[0].text_id, 0x9f,
            "the quoted idle variant once the voice ends"
        );
        assert_eq!(game.menu_npc_actions_talk_text_id, 0x9f);
    }

    // Bug 0001 (cont.): the idle animator settles on its own — after one lively
    // animation the [47ceh] countdown runs out (data_0478c = 0) and the head
    // switches to the calm resting expression, which holds rest poses (pauses)
    // between eye gestures. Without a voice line involved.
    // Asset-gated; run with:
    //   cargo test -p dune --lib -- --ignored leto_idle_settles
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn leto_idle_settles_to_calm_after_first_animation() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(256);
        let mut game = GameState::new(dat_file, tx);
        game.start(true);
        game.common_dialogue(0x0); // sets up Leto's head.

        // Start fresh on a lively animation, as if just entering idle.
        {
            let head = game.talking_head.as_mut().unwrap();
            head.speaking = false;
            head.settled = false;
            head.idle_countdown = 0;
            head.anim = 0;
            head.frame = 0;
        }
        let calm = game.talking_head.as_ref().unwrap().lipsync.animations.len() - 2;

        // Tick the idle: it spends the budget on the first lively animation, sets
        // settled, then runs the calm resting idle in 8-frame windows of the calm
        // animation separated by random pauses. Over enough ticks we should see
        // the head settle and run a calm-animation window.
        let mut settled = false;
        let mut ran_calm_window = false;
        for _ in 0..300 {
            game.tick_talking_head_idle();
            let head = game.talking_head.as_ref().unwrap();
            settled |= head.settled;
            if head.settled && head.anim == calm && head.idle_countdown > 0 {
                ran_calm_window = true;
            }
        }
        assert!(settled, "idle never settled via the countdown");
        assert!(
            ran_calm_window,
            "settled idle never started a calm-animation window"
        );
    }
}
