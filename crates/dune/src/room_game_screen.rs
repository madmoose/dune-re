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
    Equipment, GameState, Location,
    attack::AttackState,
    cmd,
    game_ui::{NAV_PANEL_BLANK, NAV_PANEL_FLIGHT, NAV_PANEL_RECORD_OFFSET},
    gfx,
    menu_defs::{self, CMD_GREY, MenuCleanupFn, MenuItem, MenuItemCallback, MenuRef, item},
    sprite_bank,
};

// = the seg001 command-record templates (seg001:21dc..221c), each a 4-byte
// [text_id:u16, handler_ofs:u16]. build_room_command_records copies these into
// the verb list, sometimes OR-ing a 0x4000 "greyed" bit into the text_id. The
// port binds each template's ported click callback at the same time.
//
// The trailing phrase comment on each line is the COMMAND.BIN string the
// text_id resolves to (= the get_phrase_or_command_string_si path: value-1
// indexes the offset table; COMMAND1.TXT lines are 1-indexed by text_id).

// = 21dc: "TAKE AN ORNITHOPTER" — appended on the special-room dl==1 path
// (the location's entry room) when the night-attack stage is not active.
// Greyed until orni_count >= 1.
const CMD_TAKE_ORNITHOPTER: MenuItem = item(
    0x00a7,
    0x42e9,
    GameState::menu_callback_choice_map_main_take_an_ornithopter_notransition,
);
// = 21e0: "WAIT FOR EVENING" — plain-room time-skip verb when the in-game
// time-of-day phase is < 0x0b (i.e. before evening).
const CMD_WAIT_FOR_EVENING: MenuItem = item(
    cmd::WAIT_FOR_EVENING,
    0x0f48,
    GameState::menu_callback_choice_wait_for_evening,
);
// = 21e4: "WAIT FOR MORNING" — plain-room time-skip verb when the in-game
// time-of-day phase is >= 0x0b (i.e. evening/night).
const CMD_WAIT_FOR_MORNING: MenuItem = item(
    cmd::WAIT_FOR_MORNING,
    0x0f67,
    GameState::menu_callback_choice_wait_for_morning,
);
// = 21e8: "VIEW NEW MESSAGES" — the palace communications-room verb
// (bh==1, dl==8) for reading newly-received transmissions; gated on
// data_000c8 != 0 (a new message is queued).
const CMD_VIEW_NEW_MESSAGES: MenuItem = item(cmd::VIEW_NEW_MESSAGES, 0x283a, |_, _, _| {
    println!("menu: VIEW NEW MESSAGES (seg000:283a) not ported")
});
// = 21ec: "Messages already seen" — the communications-room companion
// verb to CMD_VIEW_NEW_MESSAGES (replay previously-viewed messages).
const CMD_MESSAGES_ALREADY_SEEN: MenuItem = item(cmd::MESSAGES_ALREADY_SEEN, 0x283e, |_, _, _| {
    println!("menu: Messages already seen (seg000:283e) not ported")
});
// = 21f0: "LOOK AT MIRROR" — the palace bedroom verb (bh==1, dl==9; Paul's
// room with the mirror).
const CMD_LOOK_AT_MIRROR: MenuItem =
    item(cmd::LOOK_AT_MIRROR, 0x0ea6, |s, _, _| s.look_at_mirror());
// = 21f4: "Mixer Panel" — the always-available audio mixer-panel verb,
// appended at the tail of the special-room and plain-room verb lists. The
// CD release of Dune exposes its in-game music/voice mixer here.
const CMD_MIXER_PANEL: MenuItem = item(cmd::MIXER_PANEL, 0xa3f0, |s, _, _| s.open_mixer_panel());
// = 21f8: "CHANGE DESTINATION" — the map/book-mode travel verb (the third
// slot in both map sub-modes).
const CMD_CHANGE_DESTINATION: MenuItem = item(
    cmd::CHANGE_DESTINATION,
    0x497a,
    GameState::menu_callback_choice_change_destination,
);
// = 21fc: "SKIP TO DESTINATION" — the default map-mode verb for a flight
// homing on a real location (travel_no_location_dest == 0).
const CMD_SKIP_TO_DESTINATION: MenuItem = item(
    cmd::SKIP_TO_DESTINATION,
    0x4ffb,
    GameState::menu_callback_choice_skip_to_destination,
);
// = 2200: "BACK TO STARTING POINT" — replaces SKIP TO DESTINATION for a
// fixed-heading (directional) flight with no location target (travel_no_location_dest != 0).
const CMD_BACK_TO_STARTING_POINT: MenuItem = item(
    cmd::BACK_TO_STARTING_POINT,
    0x50a5,
    GameState::menu_callback_choice_back_to_starting_point,
);
// = 2204: "TOWARDS NEAREST PLACE" — appended after BACK TO STARTING POINT once
// game_phase >= 0x32 (travel_no_location_dest != 0 && game_phase >= 0x32).
const CMD_TOWARDS_NEAREST_PLACE: MenuItem = item(
    cmd::TOWARDS_NEAREST_PLACE,
    0x50c4,
    GameState::menu_callback_choice_towards_nearest_place,
);
// = 220c: "SEE DUNE MAP" — the leading verb on every special-room and
// plain-room verb list (opens the planet-map view).
const CMD_SEE_DUNE_MAP: MenuItem =
    item(cmd::SEE_DUNE_MAP, 0x186b, |s, _, _| s.ui_toggle_room_view());
// = 2214: "CALL A WORM" — the worm-summon verb. Greyed until game_phase
// >= 0x4f. Appears on plain rooms and on the night-attack sietch (dl==1).
const CMD_CALL_A_WORM: MenuItem = item(cmd::CALL_A_WORM, 0x42d1, |_, _, _| {
    println!("menu: CALL A WORM (seg000:42d1) not ported")
});
// = 2218: "MASSIVE ATTACK" — the first night-attack stage verb (special
// room dl==1 with night_attack_stage != 0).
const CMD_MASSIVE_ATTACK: MenuItem = item(cmd::MASSIVE_ATTACK, 0x7317, |_, _, _| {
    println!("menu: MASSIVE ATTACK (seg000:7317) not ported")
});
// = 221c: "FIGHT FOR A WHOLE DAY" — the second night-attack stage verb,
// adjacent to CMD_MASSIVE_ATTACK.
const CMD_FIGHT_FOR_A_WHOLE_DAY: MenuItem = item(cmd::FIGHT_FOR_A_WHOLE_DAY, 0x0fc5, |_, _, _| {
    println!("menu: FIGHT FOR A WHOLE DAY (seg000:0fc5) not ported")
});

/// One entry of the seg001:0fd8 room-person table (= the chani `RoomPerson`
/// struct). The DOS layout is 16 bytes; of the eight bytes between `handler`
/// and `person_index`, the words at +8/+0xa are the runtime travel timestamps
/// (stored below), and the rest (+6, +0xc) are static-zero padding the port
/// does not store.
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
    /// built command-menu record (room_person_menu_item binds its ported
    /// callback from it) and dispatched directly by the game-area person
    /// click (callback_main_ui_element_21_22). The savegame block carries it
    /// at entry offset +4.
    pub(crate) handler: u16,
    /// = entry word +8 (RoomPerson.time_joined) — game_time when the person
    /// last joined the player (COME WITH ME, npc_refresh_travel_timestamp with
    /// bx=0). loc_094f3 seeds for_condit_ds_16 from it while flags bit 0x40 is
    /// set; that reader is not yet ported.
    pub(crate) time_joined: u16,
    /// = entry word +0xa (RoomPerson.time_dismissed) — game_time when the
    /// person last stopped travelling (STAY HERE / npc_clear_travelling,
    /// npc_refresh_travel_timestamp with bx=2).
    pub(crate) time_dismissed: u16,
    /// 0..15, the bit position OR-ed into persons_in_room and the offset of the
    /// "&Person" text (0x78..0x87) the verb-menu record displays.
    pub(crate) person_index: u8,
    /// Bit 0x40 splits the two scan passes (template loc_030b9 / loc_03120):
    /// static-data values are 0x00 / 0x02 / 0x80, and bit 0x40 is set at
    /// runtime while the person travels with Paul (COME WITH ME, seg000:9608;
    /// cleared by npc_clear_travelling), flipping their dialogue verb to STAY
    /// HERE and their scan match to the second pass.
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
        time_joined: 0,
        time_dismissed: 0,
        person_index,
        flags,
    }
}

// = the DOS `jmp word ptr [si+4]` of a room-person entry (seg000:9234 /
// seg000:d451): resolve the entry's trampoline offset to its ported routine.
// seg000:92f2..936f are the per-character `mov al,N; jmp
// common_code_for_ui_dialogue_related_functions` trampolines — map the offset
// back to the speaker's lip-sync resource index N and run the shared dialogue
// entry. The Harkonnen-Captain / Fremen-1 trampolines stage their troop's
// CONDIT block first, and the Fremen-2 one (seg000:937e) decodes its
// fremen2_troop_ptrs slot from the record's text id (DOS ax at the jmp).
pub(crate) fn room_person_callback(handler: u16) -> MenuItemCallback {
    match handler {
        0x92f2 => |s, _, _| s.common_dialogue(0x0), // Duke Leto Atreides
        0x92f7 => |s, _, _| s.common_dialogue(0x1), // Lady Jessica Atreides
        0x92fc => |s, _, _| s.common_dialogue(0x2), // Thufir Hawat
        0x9301 => |s, _, _| s.common_dialogue(0x3), // Duncan Idaho
        0x9306 => |s, _, _| s.common_dialogue(0x4), // Gurney Halleck
        0x930b => |s, _, _| s.common_dialogue(0x5), // Stilgar
        0x9310 => |s, _, _| s.common_dialogue(0x6), // Liet Kynes
        0x9315 => |s, _, _| s.common_dialogue(0x7), // Chani
        0x931a => |s, _, _| s.common_dialogue(0x8), // Harah
        0x931f => |s, _, _| s.common_dialogue(0x9), // Baron Vladimir Harkonnen
        0x9324 => |s, _, _| s.common_dialogue(0xa), // Feyd-Rautha Harkonnen
        0x9329 => |s, _, _| s.common_dialogue(0xb), // Emperor Shaddam IV
        0x932e => |s, _, _| s.ui_dialogue_related_to_harkonnen_captains(),
        0x936f => |s, _, _| s.common_dialogue(0xd), // Smugglers
        0x9373 => |s, _, _| s.ui_dialogue_related_to_fremen1(),
        0x937e => |s, text_id, _| s.ui_dialogue_related_to_fremen2(text_id),
        _ => |_, _, _| println!("menu: room-person row with an unexpected handler"),
    }
}

// = the runtime-built room-person verb record: the DOS record stores the
// entry's trampoline offset; the port binds the resolved callback alongside.
fn room_person_menu_item(text_id: u16, handler: u16) -> MenuItem {
    MenuItem {
        text_id,
        handler,
        callback: room_person_callback(handler),
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

impl GameState {
    // = the port's `bp` dereference: resolve a MenuRef to its
    // owned menu record buffer (the seg001 buffer DOS points at).
    pub(crate) fn menu_buffer(&self, e: MenuRef) -> &menu_defs::Menu {
        match e {
            MenuRef::CommandMenuBuf => &self.command_menu_buf,
            MenuRef::MenuNpcActions => &self.menu_npc_actions,
            MenuRef::MenuGoTowardsThisPlace => &self.menu_go_towards_this_place,
            MenuRef::MenuDestinationWarning => &self.menu_destination_warning,
            MenuRef::MenuProspectorContinue => &self.menu_prospector_continue,
            MenuRef::MenuContinue => &self.menu_continue,
            MenuRef::MenuDynamic => &self.menu_dynamic,
            MenuRef::MenuCommsRoomMessagesViewed => &self.menu_comms_room_messages_viewed,
            MenuRef::MenuArgueAcceptRefuse => &self.menu_argue_accept_refuse,
            MenuRef::MenuDone => &self.menu_done,
            MenuRef::MenuMixerPanel => &self.menu_mixer_panel,
            MenuRef::MenuBook => &self.menu_book,
            MenuRef::MenuGlobe => &self.menu_globe,
            MenuRef::MenuGlobeDefaultClickOnGlobe => &self.menu_globe_default_click_on_globe,
            MenuRef::MenuMusic => &self.menu_music,
            MenuRef::MenuSaveGame => &self.menu_save_game,
            MenuRef::MenuLoadGame => &self.menu_load_game,
            MenuRef::MenuRestartLoadExitGame => &self.menu_restart_load_exit_game,
            MenuRef::MenuExitGameConfirmation => &self.menu_exit_game_confirmation,
            MenuRef::MenuPalaceMirrorRoom => &self.menu_palace_mirror_room,
            MenuRef::MenuGoThereFlyingAnOrni => &self.menu_go_there_flying_an_orni,
            MenuRef::MenuGoThereRidingAWorm => &self.menu_go_there_riding_a_worm,
            MenuRef::MenuMapTroops => &self.menu_map_troops,
            MenuRef::MenuTroopDialog => &self.menu_troop_dialog,
            MenuRef::MenuNextTroop => &self.menu_next_troop,
            MenuRef::MenuCancel => &self.menu_cancel,
            MenuRef::MenuMoveProspectors => &self.menu_move_prospectors,
            MenuRef::MenuChangeTroopDestination => &self.menu_change_troop_destination,
            MenuRef::MenuSelectTroopOccupation => &self.menu_select_troop_occupation,
            MenuRef::MenuOccupationForSpiceTroop => &self.menu_occupation_for_spice_troop,
            MenuRef::MenuOccupationForArmyTroop => &self.menu_occupation_for_army_troop,
            MenuRef::MenuOccupationForEspionageTroop => &self.menu_occupation_for_espionage_troop,
            MenuRef::MenuOccupationForEcologyTroop => &self.menu_occupation_for_ecology_troop,
        }
    }

    pub(crate) fn menu_buffer_mut(&mut self, e: MenuRef) -> &mut menu_defs::Menu {
        match e {
            MenuRef::CommandMenuBuf => &mut self.command_menu_buf,
            MenuRef::MenuNpcActions => &mut self.menu_npc_actions,
            MenuRef::MenuGoTowardsThisPlace => &mut self.menu_go_towards_this_place,
            MenuRef::MenuDestinationWarning => &mut self.menu_destination_warning,
            MenuRef::MenuProspectorContinue => &mut self.menu_prospector_continue,
            MenuRef::MenuContinue => &mut self.menu_continue,
            MenuRef::MenuDynamic => &mut self.menu_dynamic,
            MenuRef::MenuCommsRoomMessagesViewed => &mut self.menu_comms_room_messages_viewed,
            MenuRef::MenuArgueAcceptRefuse => &mut self.menu_argue_accept_refuse,
            MenuRef::MenuDone => &mut self.menu_done,
            MenuRef::MenuMixerPanel => &mut self.menu_mixer_panel,
            MenuRef::MenuBook => &mut self.menu_book,
            MenuRef::MenuGlobe => &mut self.menu_globe,
            MenuRef::MenuGlobeDefaultClickOnGlobe => &mut self.menu_globe_default_click_on_globe,
            MenuRef::MenuMusic => &mut self.menu_music,
            MenuRef::MenuSaveGame => &mut self.menu_save_game,
            MenuRef::MenuLoadGame => &mut self.menu_load_game,
            MenuRef::MenuRestartLoadExitGame => &mut self.menu_restart_load_exit_game,
            MenuRef::MenuExitGameConfirmation => &mut self.menu_exit_game_confirmation,
            MenuRef::MenuPalaceMirrorRoom => &mut self.menu_palace_mirror_room,
            MenuRef::MenuGoThereFlyingAnOrni => &mut self.menu_go_there_flying_an_orni,
            MenuRef::MenuGoThereRidingAWorm => &mut self.menu_go_there_riding_a_worm,
            MenuRef::MenuMapTroops => &mut self.menu_map_troops,
            MenuRef::MenuTroopDialog => &mut self.menu_troop_dialog,
            MenuRef::MenuNextTroop => &mut self.menu_next_troop,
            MenuRef::MenuCancel => &mut self.menu_cancel,
            MenuRef::MenuMoveProspectors => &mut self.menu_move_prospectors,
            MenuRef::MenuChangeTroopDestination => &mut self.menu_change_troop_destination,
            MenuRef::MenuSelectTroopOccupation => &mut self.menu_select_troop_occupation,
            MenuRef::MenuOccupationForSpiceTroop => &mut self.menu_occupation_for_spice_troop,
            MenuRef::MenuOccupationForArmyTroop => &mut self.menu_occupation_for_army_troop,
            MenuRef::MenuOccupationForEspionageTroop => {
                &mut self.menu_occupation_for_espionage_troop
            }
            MenuRef::MenuOccupationForEcologyTroop => &mut self.menu_occupation_for_ecology_troop,
        }
    }

    // = the stack top's record buffer — what DOS reads through the active bp
    // (get_active_screen_element, seg000:d41b). The verb strip draws, the
    // hover highlight and the slot dispatch all read the panel through this.
    pub(crate) fn active_menu_records(&self) -> &[MenuItem] {
        &self.menu_buffer(self.get_active_menu_ref()).records
    }

    pub(crate) fn active_menu_records_mut(&mut self) -> &mut Vec<MenuItem> {
        let e = self.get_active_menu_ref();
        &mut self.menu_buffer_mut(e).records
    }

    // = seg000:2db1 draw_room_game_screen — present the full in-game room screen.
    // Entered from ui_present_room_screen (when pending_room_screen_request != 0)
    // and from several scene-change sites (seg000:0ecd/13de/1bcf/037aa/9450/b424).
    pub fn draw_room_game_screen(&mut self) {
        // = seg000:2db1 bp = ui_setup_and_draw_nav_panel; draw the top HUD strip
        // offscreen (front buffer redirected to fb1 for the call). This also
        // re-installs the nav-panel template.
        self.gfx_call_bp_with_front_buffer_as_screen(|s| s.ui_setup_and_draw_nav_panel());
        // = seg000:2db7 call select_room_ui_table.
        self.select_room_ui_table();
        // = seg000:2dba data_047a6 = 0xff.
        self.data_047a6 = 0xff;
        self.draw_room_game_screen_scene_reload();
    }

    // = seg000:2dbf loc_02dbf — the scene-reload entry: the room-move commit
    // (loc_0407b) and the travel arrival (seg000:4776) re-enter here, skipping
    // the seg000:2db1 prologue — in particular the nav-panel template copy, so
    // the compass records keep their previous flags until
    // rebuild_and_draw_room_nav_panel adjusts them (the desert's alt rebuild
    // leaves the centre palace-plan button [17] as the room it was left
    // through set it: hidden).
    pub(crate) fn draw_room_game_screen_scene_reload(&mut self) {
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
            // = seg000:2e02 call travel_arrival_landing_sequence — the orni
            //   arrival plays the approach video / landing animation before
            //   the room renders.
            self.travel_arrival_landing_sequence();
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
            self.transition(0x10, 0, |_| {});
            self.service_midi_music();
        }

        // = seg000:2e52 loc_02e52 — post-render bookkeeping + dialogue tail.
        self.finish_room_screen_setup();
        // = seg000:2e55 game_clock_tick_base = the current PIT counter.
        self.game_clock_tick_base = self.game_ticks() as u16;
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
        self.transition(transition_effect, 0, |s| s.draw_room_game_screen());
        // = seg000:18ab set fb1 active; service music; snapshot the clock tick.
        self.set_fb1_as_active_framebuffer();
        self.service_midi_music();
        // = seg000:18b1 game_clock_tick_base = the current PIT counter.
        self.game_clock_tick_base = self.game_ticks() as u16;
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
        // = seg000:d454 read_command_menu_record_for_slot
        let Some(rec) = self.active_menu_records().get(slot).copied() else {
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
        // = seg000:d451 jmp bx — run the record's bound callback with the
        // record's text id (DOS's ax at the jmp) and the clicked slot (DOS's
        // cx, loaded by the per-row trampolines d443..d42f).
        (rec.callback)(self, rec.text_id, slot);
    }

    // = seg000:0f48 menu_callback_choice_wait_for_evening — the plain-room "WAIT
    // FOR EVENING" verb. game_time's low nibble is the time-of-day phase (0..15);
    // the verb fast-forwards the game clock to phase 0x0c (evening), running the
    // scheduled events for each skipped period. When it is still pre-dawn (phase
    // < 2) the sky is first brightened one step toward phase+2 so the jump does
    // not snap straight from night to dusk.
    pub(crate) fn menu_callback_choice_wait_for_evening(&mut self, _text_id: u16, _index: usize) {
        // = seg000:0f48 ax = game_time; bx = ax; al &= 0x0f — the phase nibble.
        let game_time = self.game_time;
        let phase = (game_time & 0x0f) as u8;
        // = seg000:0f4f cmp al,0ch; jnb nullsub_00f66 — already evening or later,
        //   nothing to wait for.
        if phase >= 0x0c {
            return;
        }
        // = seg000:0f54 cmp al,2; jnb loc_00f5f — before phase 2 (deep pre-dawn)
        //   fade the sky one step toward phase+2 first (set_sky_palette_for_time).
        if phase < 2 {
            // = seg000:0f5a add al,2 — only the low byte is bumped.
            self.wait_step_sky_palette(game_time.wrapping_add(2));
        }
        // = seg000:0f60 al &= 0xf0; al |= 0x0c — the target game_time: same day,
        //   phase forced to 0x0c.
        let target = (game_time & 0xfff0) | 0x0c;
        // = seg000:0f64 jmp wait_verb_advance_to_target_time — commit the target and run the events.
        self.wait_advance_to_target_time(target);
    }

    // = seg000:0f67 menu_callback_choice_wait_for_morning — the plain-room "WAIT
    // FOR MORNING" verb. Fast-forwards the game clock past midnight to phase 0 of
    // the next day (morning), running the scheduled events for each skipped
    // period. When it is still dusk (phase 0x0b or 0x0c) the sky is first darkened
    // one step toward phase+2 so the jump does not snap straight to dawn.
    pub(crate) fn menu_callback_choice_wait_for_morning(&mut self, _text_id: u16, _index: usize) {
        // = seg000:0f67 ax = game_time; bx = ax; al &= 0x0f — the phase nibble.
        let game_time = self.game_time;
        let phase = (game_time & 0x0f) as u8;
        // = seg000:0f6e cmp al,0bh; jb nullsub_00f66 — before late evening (phase
        //   0x0b) it is not worth skipping to morning; WAIT FOR EVENING covers
        //   that range instead.
        if phase < 0x0b {
            return;
        }
        // = seg000:0f73 cmp al,0dh; jnb loc_00f7e — only at dusk (phase 0x0b/0x0c)
        //   fade the sky one step toward phase+2 first (set_sky_palette_for_time);
        //   deeper into the night there is nothing to fade.
        if phase < 0x0d {
            // = seg000:0f79 add al,2 — only the low byte is bumped.
            self.wait_step_sky_palette(game_time.wrapping_add(2));
        }
        // = seg000:0f7f al &= 0xf0; add ax,10h — the target game_time: next day
        //   (the day counter bumped), phase forced to 0 (morning).
        let target = (game_time & 0xfff0).wrapping_add(0x10);
        // = seg000:0f81 falls into wait_verb_advance_to_target_time — commit the
        //   target and run the events.
        self.wait_advance_to_target_time(target);
    }

    // = seg000:0f84 wait_verb_advance_to_target_time — the shared commit tail of the WAIT FOR
    // EVENING/MORNING verbs. Advances the clock to `target` running the skipped
    // periods' events (wait_verb_run_events_and_present); during the intro cutscene game_phase 0x14
    // (Tuono Tabr) it then rewinds game_clock_tick_base by 1000 so the room
    // re-entry timestamp is not thrown off by the skip.
    fn wait_advance_to_target_time(&mut self, target: u16) {
        // = seg000:0f8b/0f89 call wait_verb_run_events_and_present either way.
        self.wait_run_events_and_present(target);
        // = seg000:0f84 cmp [game_phase],14h; jnz — only the 0x14 phase rewinds
        //   game_clock_tick_base (seg000:0f8e sub [game_clock_tick_base],3e8h).
        if self.game_phase == 0x14 {
            self.game_clock_tick_base = self.game_clock_tick_base.wrapping_sub(0x3e8);
        }
    }

    // = seg000:0f95 wait_verb_run_events_and_present — advance the clock to `target`, running one time
    // period of events per skipped step, then repaint the room screen.
    fn wait_run_events_and_present(&mut self, target: u16) {
        // = seg000:0f96 call drain_sky_fade — drain any in-flight sky cross-fade to
        //   completion before the skip.
        self.drain_sky_fade();
        // = seg000:0f99 call loc_04d00 — remove the command-panel overlay frame
        //   task (frame_task_callback_04bb9). The port never arms it, so this is
        //   a no-op (matching map_confirm_travel_and_close's loc_04d00 call).
        // = seg000:0f9c call reset_game_suspend — resume the game clock/idle anims.
        self.reset_game_suspend();
        // = seg000:0fa0 cx = target - game_time — the number of time periods to
        //   advance (always positive: target's phase > the current one).
        let periods = target.wrapping_sub(self.game_time) as i16;
        // = seg000:0fa4 call run_events_for_n_time_periods.
        self.run_events_for_n_time_periods(periods);
        // = seg000:0fa7 loc_00fa7: restore the cursor-covered pixels, present the
        //   room screen (al = 0x2a), then redraw the companion HUD heads.
        self.call_restore_cursor();
        self.ui_present_room_screen(0x2a);
        self.ui_hud_draw_companions();
    }

    // = seg000:0e3e menu_callback_choice_exit_game — the EXIT GAME verb (shared by
    // the mixer and mirror menus). Pushes the YES/NO confirmation submenu
    // (menu_exit_game_confirmation) as the active command menu, revealed with the
    // panel fold. DOS: bx = nullsub_00f66 (no-op cleanup), bp = menu_exit_game_
    // confirmation, jmp loc_0d323.
    pub(crate) fn menu_callback_choice_exit_game(&mut self, _text_id: u16, _index: usize) {
        // = seg000:d323 call screen_overlay_request_transition — arm in_transition
        //   so the submenu repaint stages into fb1 for the fold.
        self.screen_overlay_request_transition();
        // = seg000:d326 call screen_element_stack_push — install the confirmation
        //   submenu (bp = its static buffer) and repaint it (cl = 0xff, no slot
        //   pre-highlighted).
        self.menu_stack_push(MenuRef::MenuExitGameConfirmation, None);
        // = seg000:d329 call play_pending_panel_fold — fold the submenu onto screen.
        self.play_pending_panel_fold();
        // = seg000:d32c jmp loc_0d410 -> highlight_hovered_text_action_item — light
        //   up the slot under the cursor now the submenu is shown.
        self.highlight_hovered_text_action_item();
    }

    // = seg000:97cf menu_npc_actions_cleanup — the NpcActionsMenu (dialogue verb
    // panel) cleanup, run when STOP TALKING pops the menu: stop the speaker's
    // voice lip-sync, then tear the conversation down and put the room back the
    // way it was before the dialogue zoom. common_dialogue (93b9) zoomed the room
    // in on the speaker (dialogue_zoom_room set room_render_flags 0x80 and 4×-
    // scaled fb1); this cleanup re-renders the room at 1:1 and presents it, so
    // STOP TALKING returns to the un-zoomed room view.
    //
    // The game_screen_mode_flags != 0 branch (97f2) resumes a paused travel
    // (the fly-over cabin's menu) via travel_resume_flight_view; the room branch
    // (loc_0980c) re-renders the room. room_render_flags bit 7 (the dialogue-zoom
    // flag) picks between two HUD-reconciliation paths — the zoom path
    // (loc_09849) and the non-zoom path (loc_0982e); both are modelled below.
    // TODO: 097cf also restores the subtitle backdrop
    // (subtitle_restore_prior) — subtitle state not modelled yet. The room path's
    // pending_room_action-gated transition-reveal variant (loc_09898, a wiped
    // re-render + leave scan that lets an evicted companion speak) is not ported.
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
        // = seg000:97e5 data_047e1 = 0 — the sign the speaker was holding goes
        //   with the conversation.
        self.head_sign_state = 0;
        // = seg000:980c call subtitle_restore_prior — take down a lingering
        //   subtitle/bubble before the room is restored.
        self.subtitle_restore_prior();
        // = seg000:97d9 si = [data_047a2] (the speaker's room_persons entry);
        //   97dd or [si+0fh],20h; 97e1 and [si+0fh],0fbh — mark the speaker
        //   talked-to (0x20) and drop bit 0x04 on the way out.
        let speaker = self.current_lip_sync_resource_id as usize;
        self.room_persons[speaker].flags = (self.room_persons[speaker].flags | 0x20) & !0x04;
        // = seg000:97eb cmp game_screen_mode_flags,0; jnz — the flight branch:
        //   the fly-over cabin is up over a travel, not a room dialogue. Tear the
        //   head down, rebuild the flight nav panel and resume the flight instead
        //   of re-rendering a room.
        if self.game_screen_mode_flags != 0 {
            // = seg000:97f5 tear_down_prior_talking_head_overlay -> its 98e2 tail
            //   stop_lip_sync_and_remove_idle_head_task: drop the companion head
            //   and its idle animator so it stops compositing over the flight.
            self.ui_elements[18].flags = 0;
            self.ui_elements[19].flags = 0;
            self.ui_elements[20].flags = 0;
            self.stop_lip_sync_and_remove_idle_head_task();
            // = seg000:97fb..9806 xor al,al; xchg al,[data_011ca]; push ax; call
            //   rebuild_and_draw_room_nav_panel; pop ax; mov [data_011ca],al —
            //   hold data_011ca at 0 across the rebuild so it re-installs the
            //   flight panel rather than the blank one the live (nonzero) value
            //   would pick. travel_resume_flight_view clears it for good below.
            let held = std::mem::take(&mut self.data_011ca);
            self.rebuild_and_draw_room_nav_panel();
            self.data_011ca = held;
            // = seg000:9809 jmp loc_04abe — reload the flight view, present it,
            //   and clear data_011ca so travel_pump resumes.
            self.travel_resume_flight_view();
            return;
        }
        // = seg000:980f cmp room_render_flags,0; 9818 js loc_09849 — bit 7 (the
        //   dialogue-zoom flag) selects the HUD path. dialogue_zoom_room sets
        //   bit 7 only when the speaker has an on-screen anchor, i.e. the dialogue
        //   was opened by clicking a person standing in the room. A dialogue
        //   opened from a companion's HUD portrait has no room anchor, so bit 7
        //   stays clear and the non-zoom path runs.
        if (self.room_render_flags as i8) < 0 {
            // = seg000:9849 loc_09849 — the zoom path (room-standing speaker):
            //   retire the head overlay element, then for a travelling speaker
            //   (COME WITH ME set flags 0x40) take a companion HUD slot. A
            //   non-travelling speaker keeps its slot here (there is no removal on
            //   this branch); it is dropped only at travel departure
            //   (npc_travel_detach_companion) or on eviction.
            self.ui_elements[20].flags = 0;
            // = seg000:984f test [si+0fh],40h; 9855 call npc_assign_companion_slot.
            if self.room_persons[speaker].flags & 0x40 != 0 {
                self.npc_assign_companion_slot(speaker);
            }
            // = seg000:9868 and room_render_flags,7fh — drop the redraw-for-zoom
            //   flag dialogue_zoom_room set, so the room renders un-zoomed.
            self.room_render_flags &= 0x7f;
            // = seg000:9879 loc_09879 — the shared re-render tail.
            self.menu_npc_actions_redraw_room();
        } else {
            // = seg000:9825 loc_09825 — the non-zoom path (HUD-portrait dialogue).
            //   test [si+0fh],40h routes on the travelling flag.
            // = seg000:981a..9824 — during the night attack, loc_09840 is pushed
            //   as the return continuation: after the slot bookkeeping the head
            //   overlay is torn down and the game area restored + presented from
            //   fb2 (no room re-render over the scripted attack scene).
            let night = self.night_attack_stage != 0;
            if self.room_persons[speaker].flags & 0x40 != 0 {
                // = seg000:982b jmp npc_assign_companion_slot — a still-travelling
                //   speaker keeps its slot (tail jump; no room re-render, and —
                //   outside the night attack — no head teardown either). The
                //   talking head deliberately STAYS on screen with its idle task
                //   running: verified against the original, a companion-bar
                //   dialogue (no standing anchor, so no zoom) ends with the head
                //   lingering until some later action redraws the room. The
                //   port matches that quirk.
                self.npc_assign_companion_slot(speaker);
                if night {
                    // = the pushed loc_09840 continuation (seg000:9821/9824):
                    //   during the night attack the head overlay is torn down
                    //   and the game area restored + presented over the
                    //   scripted attack scene.
                    self.tear_down_prior_talking_head_overlay();
                }
            } else {
                // = seg000:982e call npc_remove_companion_slot — the speaker no
                //   longer travels (STAY HERE ran npc_clear_travelling), so its
                //   HUD portrait is removed as the dialogue closes.
                self.npc_remove_companion_slot(speaker);
                if night {
                    // = seg000:9831/9836 jnz loc_0983f — the ret lands on the
                    //   pushed loc_09840 continuation: drop the head overlay and
                    //   restore the game area over the attack scene.
                    self.tear_down_prior_talking_head_overlay();
                } else if self.room_render_flags & 1 == 0 {
                    // = seg000:9838 test room_render_flags,1; 983d jz loc_09879 —
                    //   re-render the room + raise the HUD head unless bit 0 set.
                    self.menu_npc_actions_redraw_room();
                }
            }
        }
    }

    // = seg000:9879 loc_09879 — the cleanup re-render tail shared by both HUD
    // paths of menu_npc_actions_cleanup: re-render the room scene un-zoomed (the
    // draw_room_scene reset_scene_lip_sync_state tears down the talking head),
    // keep fb2 in sync, update the palette, and raise the small HUD head ornament.
    // DOS runs build_room_command_records / build_persons_in_room_records at the
    // head of this block; the port rebuilds those in
    // menu_stack_pop_and_cleanup after the cleanup returns and pops back
    // to the room command menu, so they are not duplicated here.
    fn menu_npc_actions_redraw_room(&mut self) {
        // = seg000:9879 call build_room_command_records; 987c..9883 call
        //   build_persons_in_room_records unless a map/book mode is up — the
        //   room verb list reflects the post-conversation state (a joined or
        //   dismissed companion changes the person verbs).
        self.build_room_command_records();
        if self.game_screen_mode_flags == 0 {
            self.build_persons_in_room_records();
        }
        // = seg000:9886 call draw_room_scene.
        self.draw_room_scene();
        // = seg000:9889 call copy_active_framebuffer_to_framebuffer_2.
        self.copy_active_framebuffer_to_framebuffer_2();
        // = seg000:988c call update_screen_palette.
        self.update_screen_palette();
        // = seg000:988f call ui_hud_head_save_rect.
        self.ui_hud_head_save_rect();
        // = seg000:9892 call present_game_area.
        self.present_game_area();
        // = seg000:9895 jmp ui_hud_head_animate_up.
        self.ui_hud_head_animate_up();
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
        self.transition(4, 0, |s| s.callback_transition_look_at_mirror());
    }

    // = seg000:0eb9 menu_callback_choice_palace_look_away_from_mirror — the
    // "Look away from the mirror" verb (mirror menu slot 4). DOS jmps straight
    // to 0eb9 and leaves the mirror entry on the menu stack: its 0xff priority
    // is locked against menu_stack_pop_and_cleanup, and draw_room_game_screen's
    // re-insert replaces it in place. The port pops the overlay directly so the
    // room menu is active again — the same dismissal game_area_click performs.
    pub(crate) fn menu_callback_choice_palace_look_away_from_mirror(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        self.menu_stack.pop();
        self.look_away_from_mirror();
    }

    // = seg000:0eb9 (the loc_00eb9 body) — leave the mirror still and return to
    // the bedroom: clear the suspend, re-arm the room screen, and redraw it.
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
        // = seg000:0ef1 si=ui_nav_panel_blank; loc_0d72b — install the blank
        //   nav-panel template into HUD records 12..17, clearing the bottom-right
        //   compass (no sprites, no clickable records) for the mirror still.
        self.ui_install_nav_panel(&NAV_PANEL_BLANK);
        // = seg000:0ef7 call main_ui_elements_clear_flags_18_19_20.
        self.main_ui_elements_clear_flags_18_19_20();
        // = seg000:0efa ui_elements[20].flags = 0x80 — enable the full game-area
        //   hotspot (rect 0,0..320,152) so any click there dismisses the mirror.
        self.ui_elements[20].flags = 0x80;
        // = seg000:0eff bp=menu_palace_mirror_room (seg001:20c2); 0f02 bx=menu_
        //   callback_choice_palace_look_away_from_mirror; 0f05 jmp
        //   screen_element_stack_push — install the mirror verb menu (priority
        //   0xff) as the active command menu and paint it (RESTART / LOAD / SAVE
        //   / EXIT GAME, then "Look away from the mirror"). bx is the look-away
        //   handler (0x0eb9), but the entry's 0xff priority keeps it out of both
        //   cleanup paths (pop_and_cleanup skips 0x?f, an equal-priority insert
        //   replaces without running the cleanup), so no callback is stored.
        //   look_away_from_mirror -> draw_room_game_screen rebuilds the room
        //   verbs when the still is dismissed.
        self.menu_stack_push(MenuRef::MenuPalaceMirrorRoom, None);
    }

    // = seg000:941d room_game_area_click — the game-area hotspot (ui_elements[20])
    // click. When the look-away overlay is the active menu, pop it and return
    // to the room (menu_callback_choice_palace_look_away_from_mirror).
    pub(crate) fn game_area_click(&mut self) {
        // = seg000:9422 cmp data_047a9,0 (the smuggler branch) is not modelled.
        // = seg000:9427 call get_active_screen_element; 942a cmp bp,20c2h.
        if self.get_active_menu_ref() != MenuRef::MenuPalaceMirrorRoom {
            // TODO: the other game-area click branches (dialogue / map modes,
            //   seg000:9436..9458) are not ported.
            return;
        }
        // = seg000:9430 call screen_element_stack_pop_and_cleanup — a no-op in
        // DOS (it skips the mirror entry's 0xf-locked priority); the pop lives
        // in the shared verb path below.
        // = seg000:9433 jmp menu_callback_choice_palace_look_away_from_mirror.
        self.menu_callback_choice_palace_look_away_from_mirror(0, 0);
    }

    // = seg000:9215 callback_main_ui_element_21_22 — the game-area / person click
    // dispatch. Over the room command menu in the normal room view, hit-test the
    // on-screen people (person_hit_test, which routes a HUD-area cursor to the
    // companion portrait slots); a hit on person index < 0x0f dispatches
    // that person's verb handler (room_persons[id].handler, = `jmp word ptr
    // [si+4]`), and >= 0x0f enters the Fremen-2 dialogue for round-robin slot
    // (index - 0x0f) — the "Fremen Chief" / "Nth Fremen Chief" figures
    // (loc_09240). Over the dialogue verb panel (loc_09248) only the companion
    // slots are live: the current speaker's own portrait re-enters their
    // dialogue (callback_main_ui_element_19), the other companion's switches
    // the conversation to them. The room LMB handler reaches this when a click
    // lands on no ui_element (= seg000:d904 hit-test miss -> d90a call), and it
    // is also the handler armed on ui_elements[21]/[22].
    pub(crate) fn callback_main_ui_element_21_22(&mut self) {
        // = seg000:9215 get_active_screen_element; cmp bp,1f0eh; jnz loc_09248.
        let active = self.get_active_menu_ref();
        if active == MenuRef::MenuNpcActions {
            // = seg000:9248 loc_09248 — dialogue active: 924e call companion_slot_hit_test;
            //   jnb ret — only a companion-slot hit does anything.
            let Some(pid) = self.companion_slot_hit_test() else {
                return;
            };
            if pid as u16 == self.current_lip_sync_resource_id {
                // = seg000:9253/9259 the speaker's own portrait — jmp
                //   callback_main_ui_element_19.
                self.callback_main_ui_element_19();
            } else {
                // = seg000:925c push cx; call dismiss_stacked_overlays; pop cx;
                //   9261 jmp loc_09234 — drop the dialogue panel and dispatch
                //   the other companion's handler: the conversation switches.
                self.dismiss_stacked_menus();
                let handler = self.room_persons[pid as usize].handler;
                (room_person_callback(handler))(self, 0, 0);
            }
            return;
        }
        // = seg000:921e cmp game_screen_mode_flags,0; jnz loc_09281.
        if active != MenuRef::CommandMenuBuf || self.game_screen_mode_flags != 0 {
            return;
        }
        // = seg000:9225 call person_hit_test_at_cursor; jnb loc_09263 (no person hit).
        let Some(person_id) = self.person_hit_test() else {
            // = seg000:9263 loc_09263 — no person under the cursor: clicking the
            //   game area of a location's outdoor arrival view (current_room ==
            //   1) walks inside through its UP exit. This is how clicking on the
            //   sietch / palace / fortress in the entry scene enters it. Gated
            //   out for a click below the game area (mouse_y >= 152), the
            //   smuggler den (current_scene == 0x21), and the night attack.
            if self.current_room == 1
                && self.mouse_pos_y < 152
                && self.data_00008 != 0x21
                && self.night_attack_stage == 0
            {
                // = seg000:927e jmp ui_click_room_up.
                self.ui_click_move_up();
            }
            return;
        };
        // = seg000:922a cmp cl,2fh; jz loc_09282 — a click on the parked
        // ornithopter opens the map screen in ornithopter mode (jmp
        // menu_callback_choice_map_main_take_an_ornithopter_notransition).
        if person_id == 0x2f {
            self.menu_callback_choice_map_main_take_an_ornithopter_notransition(0, 0);
            return;
        }
        // = seg000:922f cmp cl,0fh; jnb loc_09240 — person index < 0x0f
        // dispatches a room_persons handler here.
        if (person_id as usize) < 0x0f {
            // = seg000:9234 al=0x10; mul cl; si = room_persons + cl*0x10;
            // jmp word ptr [si+4] — dispatch the matched person's handler.
            let handler = self.room_persons[person_id as usize].handler;
            (room_person_callback(handler))(self, 0, 0);
        } else {
            // = seg000:9240 loc_09240 — sub cl,0fh; mov al,cl; jmp
            // ui_dialogue_related_to_common_and_Fremen2: a click on a
            // Fremen-2 person (draw ids 0x0f..) enters that person's dialogue
            // by round-robin slot, entering the shared body past the 937e
            // text-id decode.
            self.ui_dialogue_related_to_common_and_fremen2(person_id - 0x0f);
        }
    }

    // = seg000:932e ui_dialogue_related_to_HarkonnenCaptains — stage the
    // captain's troop CONDIT block, then the shared dialogue entry with
    // al = 0x0c. The middle (9335..9367: the map-position recompute, troop
    // harvest_rate seed, subst_id_09) is not yet ported.
    pub(crate) fn ui_dialogue_related_to_harkonnen_captains(&mut self) {
        if let Some(ti) = self.harkonnen_captain_troop {
            self.troop_prepare_troop_data_for_condit(ti);
        }
        self.common_dialogue(0x0c);
    }

    // = seg000:9373 ui_dialogue_related_to_Fremen1 — the Fremen chief:
    // si = [fremen1_troop_ptr]; stage its CONDIT block; al = 0x0e.
    pub(crate) fn ui_dialogue_related_to_fremen1(&mut self) {
        if let Some(ti) = self.fremen1_troop {
            self.troop_prepare_troop_data_for_condit(ti);
        }
        self.common_dialogue(0x0e);
    }

    // = seg000:937e ui_dialogue_related_to_Fremen2 — the verb-record entry:
    // al = text_id - 0x87 selects the fremen2_troop_ptrs slot; falls into the
    // shared seg000:9381 body.
    pub(crate) fn ui_dialogue_related_to_fremen2(&mut self, text_id: u16) {
        // = seg000:937e sub ax,87h.
        let idx = text_id.wrapping_sub(0x87) as u8;
        self.ui_dialogue_related_to_common_and_fremen2(idx);
    }

    // = seg000:9381 ui_dialogue_related_to_common_and_Fremen2 — enter the
    // Fremen-2 dialogue for the fremen2_troop_ptrs slot `idx`: clamp (>= 9
    // folds to 0; 8 is the prospector's slot via data_0476b), record
    // selected_fremen2, run the phase-0x64 gate and the CONDIT staging on the
    // slot's troop, then the shared dialogue entry with al = 0x0f. Reached
    // from the verb record (seg000:937e, al = text_id - 0x87) and from the
    // game-area click on a Fremen-2 person (seg000:9240, al = person - 0x0f).
    pub(crate) fn ui_dialogue_related_to_common_and_fremen2(&mut self, mut idx: u8) {
        // = seg000:9381 cmp al,9; jb loc_09387; xor ax,ax.
        if idx >= 9 {
            idx = 0;
        }
        // = seg000:9387..9392 slot 8 resolves to the prospector's recorded
        //   slot (data_0476b, 1-based; 0 falls back to slot 0).
        if idx == 8 {
            idx = self.data_0476b.wrapping_sub(1);
            if (idx as i8) < 0 {
                idx = 0;
            }
        }
        // = seg000:9394 selected_fremen2_index = al; 9397..93a0 si =
        //   fremen2_troop_ptrs[al].
        self.selected_fremen2 = idx;
        if let Some(ti) = self.fremen2_troops[(idx & 7) as usize] {
            // = seg000:93a2 call game_phase_set_to_64_if_conditions_met.
            self.game_phase_set_to_64_if_conditions_met(ti);
            // = seg000:93a5 call troop_prepare_troop_data_for_condit.
            self.troop_prepare_troop_data_for_condit(ti);
        }
        // = seg000:93a8 al = 0x0f; falls through into
        //   common_code_for_ui_dialogue_related_functions.
        self.common_dialogue(0x0f);
    }

    // = seg000:945b callback_main_ui_element_19 — a click on the talking-head
    // area, also reached from the current speaker's own companion portrait
    // (seg000:9259): with no live subtitle bubble/strip, re-enter the
    // speaker's dialogue (common_code_for_ui_dialogue_related_functions with
    // the current lip-sync id); with one, act as TALK TO ME. The seg000:946b
    // menu_argue_accept_refuse guard (the accept/refuse/argue submenu keeps
    // the click inert) is not ported — that menu is not modelled.
    pub(crate) fn callback_main_ui_element_19(&mut self) {
        // = seg000:945b cmp [current_bubble_layout_ptr],0; jnz loc_09468.
        if self.subtitle_bubble.is_none() {
            // = seg000:9462 ax = [current_lip_sync_resource_id]; jmp 93aa.
            let id = self.current_lip_sync_resource_id as u8;
            self.common_dialogue(id);
            return;
        }
        // = seg000:946f jnz menu_callback_choice_talk_to_me.
        self.menu_callback_choice_talk_to_me(0, 0);
    }

    // = seg000:d41b get_active_screen_element — return the identity of the top
    // menu-stack entry (the room command menu when nothing is layered over
    // it).
    pub(crate) fn get_active_menu_ref(&self) -> MenuRef {
        self.menu_stack
            .last()
            .copied()
            .map(|e| e.0)
            .unwrap_or(MenuRef::CommandMenuBuf)
    }

    // = seg000:b2b9 suspend_game_clock — inc game_suspend_count, suspending the
    // in-game clock and idle events one nesting level.
    pub(crate) fn suspend_game_clock(&mut self) {
        self.game_suspend_count = self.game_suspend_count.saturating_add(1);
    }

    // = seg000:b2be resume_game_clock — release one suspension level.
    pub(crate) fn resume_game_clock(&mut self) {
        self.game_suspend_count = self.game_suspend_count.saturating_sub(1);
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
    // draws the verb menu for current_location_ptr. Run via the offscreen helper from
    // draw_room_game_screen.
    pub(crate) fn ui_draw_room_command_panel(&mut self) {
        // = seg000:2eb2 cmp data_04774,0; jnz -> the dialogue branch.
        if self.is_dialogue_active {
            // = seg000:2eb9 call loc_0301a (render the dialogue panel).
            self.draw_dialogue_panel();
            // = seg000:2ebc call loc_098e6 (reset the per-scene lip-sync indices).
            self.reset_scene_lip_sync_state();
            // = seg000:2ebf loc_02ebf: bp = [data_02220] (the dialogue record
            // buffer), bx = 0f66h; jmp screen_element_stack_push — install the
            // dialogue panel as the active menu.
            self.sequence_push_continue_menu();
            return;
        }

        // = seg000:2ec9 loc_02ec9 — the verb-menu branch.
        // = seg000:2ec9 di = [data_0114e] = current_location_ptr.
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
        // = seg000:2ef2 bp = command_menu_buf, bx = nullsub_00f66; jmp
        // screen_element_stack_push — re-insert the room verb strip and paint
        // it. In room mode the equal-0xff insert replaces the base with itself
        // (a repaint); in map/flight mode the insert walk (seg000:d349) first
        // pops any transient overlays still stacked, their cleanups included,
        // making command_menu_buf the active strip.
        self.menu_stack_push(MenuRef::CommandMenuBuf, None);
    }

    // ---- Command-panel callees (linked stubs; see the .chani annotations).

    // = seg000:2e98 set_command_menu_origin — save the verb-menu list identity
    // (command_menu_list = di) and compute its draw origin from the two-byte
    // header at di (command_menu_x = [di]; command_menu_y = [di+1] + 0xc).
    // Popup menus pass a real template; this room-menu call site passes
    // current_location_ptr, whose first_name/last_name bytes serve as the
    // nominal header. TODO: port; needs command_menu_list and its identity
    // compare (seg000:a2aa) to matter first. No-op stub.
    pub(crate) fn set_command_menu_origin(&mut self) {}

    // = seg000:2efb build_room_command_records — assemble the verb-menu record
    // list for the current room into command_menu_buf (the seg001:1f0e
    // buffer, whose leading skip byte the port models implicitly as empty). The
    // records come from the seg001 command-record templates (21dc..221c), gated
    // by the room type (location_appearance low byte 0x80 = special room),
    // location_and_room, game phase, ornithopter count, smuggler flag, and
    // time-of-day. The DOS `xor ax,ax; stosw` terminator is the empty Vec tail.
    pub(crate) fn build_room_command_records(&mut self) {
        // = seg000:2efd di=1f0fh; xor al,al; stosb — the empty header skip byte.
        let mut recs: Vec<MenuItem> = Vec::new();
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
                    recs.push(CMD_CALL_A_WORM.grayed_if(self.game_phase < 0x4f));
                } else {
                    // = seg000:2f3d loc_02f3d — di = [current_location_ptr] (the current
                    // location pointer stashed there at room commit); call
                    // compute_location_available_equipment (seg000:7f27) to refresh
                    // orni_count for this location, then "TAKE AN ORNITHOPTER" greyed
                    // while orni_count < 1.
                    self.compute_location_available_equipment(self.current_location_index as usize);
                    recs.push(
                        CMD_TAKE_ORNITHOPTER.grayed_if(self.available_equipment.ornithopters < 1),
                    );
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
                    recs.push(CMD_VIEW_NEW_MESSAGES.grayed_if(!messages_loaded));
                    recs.push(CMD_MESSAGES_ALREADY_SEEN.grayed_if(!messages_loaded));
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
            if self.travel_no_location_dest != 0 {
                // = seg000:2fe1 si=2200h; a fixed-heading (directional) flight —
                // travel_no_location_dest != 0 means there is no specific location target
                // (the travel homes on the starting point, last_location_ptr),
                // so the verb is "BACK TO STARTING POINT" rather than "SKIP TO
                // DESTINATION".
                recs.push(CMD_BACK_TO_STARTING_POINT);
                // = seg000:2fe4 cmp game_phase,32h; jb — from phase 0x32 the
                // list also offers si=2204h "TOWARDS NEAREST PLACE".
                if self.game_phase >= 0x32 {
                    recs.push(CMD_TOWARDS_NEAREST_PLACE);
                }
            } else {
                // = seg000:2fda si=21fch; "SKIP TO DESTINATION" default (a flight
                // homing on a real location). The template copy carries the live
                // flags byte DOS patches in place (data_021fd,
                // set_skip_to_destination_verb_flags).
                let mut skip = CMD_SKIP_TO_DESTINATION;
                skip.text_id |= (self.cmd_skip_to_destination_flags as u16) << 8;
                recs.push(skip);
            }
            // = seg000:2ff2 si=21f8h; "CHANGE DESTINATION" trailing verb.
            recs.push(CMD_CHANGE_DESTINATION);
        } else {
            // = seg000:2faa loc_02faa — the plain (non-special) room branch.
            // = seg000:2fb1 si=220ch; "SEE DUNE MAP".
            recs.push(CMD_SEE_DUNE_MAP);
            // = seg000:2fb6 si=2214h; "CALL A WORM" greyed until phase >= 0x4f.
            recs.push(CMD_CALL_A_WORM.grayed_if(self.game_phase < 0x4f));
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

        // = the seg000:2efd..2ff9 stores land in command_menu_buf (seg001:1f0e)
        // regardless of what is stacked above it.
        self.command_menu_buf.records = recs;
    }

    // = seg000:1ae0 get_ingame_time_of_day — the time-of-day phase, game_time & 0xf.
    pub(crate) fn get_ingame_time_of_day(&self) -> u8 {
        (self.game_time & 0xf) as u8
    }

    // = seg000:7f27 compute_location_available_equipment — recompute location
    // `li`'s per-type available equipment into the global buffer (DOS
    // seg001:46fe, location_available_equipment); the ornithopters slot is
    // orni_count, read to grey TAKE AN ORNITHOPTER. DOS takes the location in
    // di — mostly [current_location_ptr], but map_setup_troop_dialog_menu
    // passes the contacted troop's location instead — so the port takes it as
    // an argument.
    pub(crate) fn compute_location_available_equipment(&mut self, li: usize) {
        let Some(location) = self.locations.get(li) else {
            return;
        };
        self.available_equipment = self.location_available_equipment(location);
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
    // (DOS bp) with its cleanup func (DOS bx) onto the z-ordered
    // menu stack and repaint the now-active verb menu. DOS chains
    // screen_element_stack_insert (d33a, the priority-sorted insert that pops
    // higher-priority entries and runs their cleanup funcs) -> draw_command_menu
    // (d36d, set the top slot and clear the records' 0x8000 highlight bits) ->
    // redraw_active_command_menu (d397). The port keeps the same priority walk
    // over the (MenuRef, cleanup) slots and repaints. cl=0xff (no slot
    // pre-highlighted) is implicit in redraw_active_command_menu starting from
    // "nothing hovered".
    pub(crate) fn menu_stack_push(&mut self, menu_ref: MenuRef, callback: Option<MenuCleanupFn>) {
        // The caller has already staged `element`'s record buffer (DOS builds
        // or patches the static buffer, then inserts its pointer). The insert
        // walk compares the incoming buffer's priority byte (`[buf]`, DOS al)
        // against the top's per iteration (= seg000:d343..d359):
        let priority = self.menu_buffer(menu_ref).priority;
        while let Some((top_menu_ref, callback)) = self.menu_stack.last().copied() {
            let top_priority = self.menu_buffer(top_menu_ref).priority;
            if priority == top_priority {
                // = seg000:d345 jz loc_0d368 — equal priority REPLACES the top
                // slot in place; the stack does not deepen.
                *self.menu_stack.last_mut().unwrap() = (menu_ref, callback);
                self.redraw_active_command_menu();
                return;
            }
            if priority < top_priority {
                // = seg000:d347 jb loc_0d35b — the incoming element is more
                // transient than the top: deepen (push above it).
                break;
            }
            // = seg000:d349..d359 — the incoming element sorts BENEATH the
            // top (its priority byte is higher): pop the more-transient top,
            // calling its cleanup func (`ax = [si+2]; call ax`), and retry
            // against the new top.
            if let Some(callback) = callback {
                callback(self);
            }
            self.menu_stack.pop();
        }
        self.menu_stack.push((menu_ref, callback));
        self.redraw_active_command_menu();
    }

    // = seg000:90bd setup_npc_dialogue_menu — pick the dialogue verb panel's
    // per-NPC second verb (the slot between TALK TO ME and STOP TALKING) and push
    // menu_NPC_actions onto the menu stack so the dialogue verbs render
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
            item(0x9c, 0x9584, |_, _, _| {
                println!("menu: OVERPOWER THE PRISONER (seg000:9584) not ported")
            })
        } else if pi == 0x0f {
            // = seg000:90d9 person 0x0f: text 0x93, handler loc_05a03.
            item(
                0x93,
                0x5a03,
                GameState::menu_callback_choice_give_orders_to_troop,
            )
        } else if pi == 0x0e {
            // = seg000:90e3 person 0x0e: text 0x96, bumped to 0x97 once Paul-event
            // bit 0x10 is set; handler loc_095c1.
            let id = if (self.bitfield_paul_events & 0x10) != 0 {
                0x97
            } else {
                0x96
            };
            item(
                id,
                0x95c1,
                GameState::menu_callback_choice_come_with_me_troop,
            )
        } else {
            // = seg000:90f7 the general NPC.
            let flags = npc.flags;
            if (flags & 0x80) != 0 {
                // = seg000:90fd greyed COME WITH ME (text 0x91 | 0x4000). DOS
                // leaves the callback (dx) stale; the verb is disabled, so it is
                // never dispatched.
                item(0x4091, 0, |_, _, _| {})
            } else if (flags & 0x40) != 0 {
                // = seg000:910d the NPC already travels with you, so offer STAY
                // HERE (text 0x92, handler menu_callback_choice_stay_here).
                item(0x92, 0x9533, GameState::menu_callback_choice_stay_here)
            } else {
                // = seg000:9102 COME WITH ME (text 0x91, handler
                // menu_callback_choice_come_with_me).
                item(0x91, 0x95e2, GameState::menu_callback_choice_come_with_me)
            }
        };
        // = seg000:9111..9118 splice the dynamic verb into menu_NPC_actions
        // slot 1 (DOS overwrites [bp+6]/[bp+8] of the static buffer). The other
        // records keep their buffer values: record 0's TALK TO ME text is
        // whatever set_talk_to_me_verb_text last patched (seg001:1f80), and
        // " WHAT ? " / STOP TALKING are the compiled-in entries.
        self.menu_npc_actions.records[1] = dynamic;
        // = seg000:911a call screen_overlay_request_transition — arm the in-transition flag so the verbs
        // stage in fb1 (draw_command_menu_item routes there when in_transition > 0).
        // The pending fold (play_pending_panel_fold / play_pending_panel_fold) then reveals the
        // staged panel onto the screen.
        self.screen_overlay_request_transition();
        // = seg000:911d bx = menu_npc_actions_cleanup (the menu's cleanup func); 9120 jmp
        // screen_element_stack_push. With in_transition armed, redraw_active_command_
        // menu paints the verbs into fb1, not the visible screen.
        self.menu_stack_push(
            MenuRef::MenuNpcActions,
            Some(GameState::menu_npc_actions_cleanup),
        );
    }

    // = seg000:d316 screen_overlay_request_transition — when no HNM movie is playing, set the in-transition
    // flag's low bit so the verb-panel paint stages into fb1 (offscreen) until the
    // pending fold reveals it. The port does not model the HNM file handle (treated
    // as none), so the bit is always armed here.
    pub(crate) fn screen_overlay_request_transition(&mut self) {
        self.in_transition |= 1;
    }

    // = seg000:d397 redraw_active_command_menu — paint the active verb menu
    // (the active element's record buffer) into HUD rows 7..11. Up to five slots are drawn; a
    // sixth-or-later verb collapses into the 0xa0 "more" arrow in slot 4, and any
    // slots past the last record are filled blank (clearing stale verbs). Falls
    // into highlight_hovered_text_action_item (loc_0d410) so the slot under the
    // pointer immediately gets the inverse highlight.
    pub(crate) fn redraw_active_command_menu(&mut self) {
        // = seg000:d397 mov [index_of_last_hovered_action_item], 0ffh —
        // discard any prior hover so the highlight repaint that follows runs
        // against a fresh "nothing highlighted" baseline.
        self.index_of_last_hovered_action_item = 0xff;
        // Snapshot the active buffer's records: the per-slot draw below needs
        // `&mut self`, so the walk reads a local copy (a handful of 4-byte
        // records).
        let recs = self.active_menu_records().to_vec();
        let n = recs.len();
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
                recs[i].text_id
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
        let n = self.active_menu_records().len();
        let slot_count = n.min(5) as u8;
        if slot_count == 0 {
            return false;
        }

        // = seg000:d523 get_active_screen_element; cmp bp,1f0eh; cmp
        // game_screen_mode_flags,0 — the person-hover branch runs only over the
        // room command menu in the normal room view.
        let new_slot = if self.get_active_menu_ref() == MenuRef::CommandMenuBuf
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
        let recs = self.active_menu_records();
        let n = recs.len();
        if i >= n {
            0
        } else if slot == 4 && n > 5 {
            0xa0
        } else {
            recs[i].text_id
        }
    }

    // = seg000:d5b1 loc_0d5b1 — the verb-strip rect test: the first painted slot
    // whose ui_elements[7..7+slot_count] rect contains the pointer, else 0xff. The
    // hit is gated below the bottom HUD strip (= seg000:d5b1 cmp bx,98h; jb).
    fn verb_strip_hovered_slot(&self, slot_count: u8) -> u8 {
        let x = self.mouse_pos_x;
        let y = self.mouse_pos_y;
        if y < 152 {
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
    // active menu, redraw verb slot 0 in place.
    pub(crate) fn set_talk_to_me_verb_text(&mut self, text_id: u16) {
        // = seg000:d62a cmp [si+2],ax; mov [si+2],ax — patch menu_NPC_actions
        // record 0's text id in place (seg001:1f80). The buffer is the single
        // source of truth: the next setup_npc_dialogue_menu reuses whatever
        // was last patched, exactly like DOS's static buffer.
        let changed = self.menu_npc_actions.records[0].text_id != text_id;
        self.menu_npc_actions.records[0].text_id = text_id;
        // = seg000:d630 jz — unchanged; d632 cmp bp,si; jnz — repaint the live
        // slot 0 only when the active command menu is specifically
        // menu_NPC_actions. The fly-over submenus (menu_go_towards_this_place /
        // menu_change_destination_ignore_warning) are separate buffers and
        // identities, so DOS's bp != si is the identity compare.
        let is_npc_dialogue_menu = self.get_active_menu_ref() == MenuRef::MenuNpcActions;
        if !changed || !is_npc_dialogue_menu {
            return;
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
    // person id (0..0x16) of the first marker the cursor falls in; when no person
    // matches, against the parked-ornithopter hotspot (orni_hotspot_x/y),
    // returning the 0x2f ornithopter pseudo-person; else None. The person marker
    // is each person's draw anchor (top-left); the test is a fixed person-sized
    // box below-and-right of it — mouse_x 1..=32 px right of the anchor and
    // mouse_y 1..=80 px below it. Gated on mouse_pos_y < 152 (the room scene
    // area).
    pub(crate) fn person_hit_test(&self) -> Option<u8> {
        let mouse_x = self.mouse_pos_x;
        let mouse_y = self.mouse_pos_y;
        // = seg000:9285 cmp bx,98h; jnb companion_slot_hit_test — a cursor below the game
        // area tests the companion HUD portrait slots instead of the person
        // markers.
        if mouse_y >= 152 {
            return self.companion_slot_hit_test();
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
        // = seg000:92ab the loop fall-through — no person matched: test the
        // parked-ornithopter hotspot. 0 = no ornis in this scene (cleared at
        // every scene draw, recorded by draw_room_ornis).
        let (ox, oy) = (self.orni_hotspot_x, self.orni_hotspot_y);
        if ox == 0 {
            return None;
        }
        // = seg000:92b2 sub ax,dx; cmp ax,0ffb2h; cmc; jnb loc_092c8 — the hit
        // needs (ox - mouse_x) in [0xffb2, 0xffff] (signed -78..-1): the cursor
        // 1..=78 px right of the hotspot.
        if ox.wrapping_sub(mouse_x) < 0xffb2 {
            return None;
        }
        // = seg000:92ba ax = bx - [orni_hotspot_y]; cmp ax,3ch; jnb loc_092c8 —
        // the cursor 0..=59 px below the hotspot (unsigned, so above misses).
        if mouse_y.wrapping_sub(oy) >= 0x3c {
            return None;
        }
        // = seg000:92c5 cx = 0x2f — the ornithopter pseudo-person id. Its verb
        // text is 0x78 + 0x2f = 0xa7, TAKE AN ORNITHOPTER, so the hover
        // highlight (slot_for_person_text_id) needs no special case.
        Some(0x2f)
    }

    // = seg000:92c9 companion_slot_hit_test — hit-test the cursor against the two companion
    // HUD portrait slots (ui_elements[21]/[22], the boxes at (35,182)/(58,182)):
    // an occupied slot (ui_hud_companion_N != 0xff) whose rect strictly
    // contains the cursor (rect_contains, seg000:d6fe) yields that companion's
    // person index. An empty slot 1 ends the test without trying slot 2 (the
    // 92d2 jz short-circuit).
    pub(crate) fn companion_slot_hit_test(&self) -> Option<u8> {
        let (x, y) = (self.mouse_pos_x as i16, self.mouse_pos_y as i16);
        let slot_rect = |e: &crate::game_ui::UiElement| crate::Rect {
            x0: e.x0 as i16,
            y0: e.y0 as i16,
            x1: e.x1 as i16,
            y1: e.y1 as i16,
        };
        // = seg000:92cb cl = [ui_hud_companion_1]; cmp cl,0ffh; jz loc_09281.
        if self.companions[0] < 0 {
            return None;
        }
        // = seg000:92d4 di = ui_hud_elements[21]; call rect_contains.
        if slot_rect(&self.ui_elements[21]).contains_interior(x, y) {
            return Some(self.companions[0] as u8);
        }
        // = seg000:92dc cl = [ui_hud_companion_2]; cmp cl,0ffh; jz loc_09281.
        if self.companions[1] < 0 {
            return None;
        }
        // = seg000:92e5 di = ui_hud_elements[22]; jmp rect_contains.
        if slot_rect(&self.ui_elements[22]).contains_interior(x, y) {
            return Some(self.companions[1] as u8);
        }
        None
    }

    // = seg000:d48a draw_command_menu_item — draw one verb slot (`slot` 0..4,
    // `text_id` with state bits) into ui_elements[7+slot]: a leading space + the
    // resolved string at x=0x5d, y = the row's y0 + 1 (small font), then fill the
    // rest of the row with the background colour. text_id & 0x3fff == 0 leaves the
    // slot blank (just the fill, which clears any previous verb).
    pub(crate) fn draw_command_menu_item(&mut self, slot: u8, text_id: u16) {
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
        // with the background colour, clearing whatever the slot held before
        // (seg000:d502 es = [_word_2D08A_framebuffer_active_seg]).
        let (pen_x, pen_y) = self.font_get_draw_position();
        gfx::vga_fill_rect(self, self.active_fb(), pen_x, pen_y, 0xe3, pen_y + 7, bg);

        // = seg000:d50a pop [active_seg].
        self.active_fb = saved;
    }

    // = seg000:3090 build_persons_in_room_records — append the people-present
    // records to the verb list. Resets the four person slots (init_room_persons),
    // clears persons_in_room, then scans the room-person table at seg001:0fd8
    // twice: first pass picks up entries with flags bit 0x40 clear (template
    // loc_030b9), second pass the ones with it set (template loc_03120). The
    // DOS routine advances di past the existing list before appending; the port
    // keeps command_menu_buf.records as a Vec and appends.
    //
    pub(crate) fn build_persons_in_room_records(&mut self) {
        // = seg000:3090 call reset_scene_lip_sync_state.
        self.reset_scene_lip_sync_state();
        // = seg000:3093 loc_03093 — the entry the come-with-me troop path
        // re-enters without the lip-sync reset.
        self.rebuild_persons_in_room_records();
    }

    // = seg000:3093 loc_03093 — the lip-sync-preserving half of
    // build_persons_in_room_records.
    pub(crate) fn rebuild_persons_in_room_records(&mut self) {
        // = seg000:3093 call init_room_persons.
        self.init_room_persons();
        // = seg000:3096..30a0 find the terminator of the existing command list.
        // The port's command_menu_buf.records is a Vec, so appending past the end
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
    pub(crate) fn scan_matching_room_person_entries(
        &mut self,
        builder: fn(&mut Self, u8, &RoomPerson),
    ) {
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

    // = seg000:36d3 run_room_leave_dialogue_scan — the pending_room_action-gated room-person dialogue scan run
    // when leaving a room (ui_click_move_room) or re-entering one. When the leave
    // flag is set, walk the standing room-persons (bp = room_person_present_auto_dialogue) so one of them
    // can speak an auto-dialogue line, then clear the flag.
    pub(crate) fn run_room_leave_dialogue_scan(&mut self) {
        // = seg000:36d3 cmp byte [pending_room_action], 0; jz ret.
        if self.pending_room_action == 0 {
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
        // = seg000:36e8 mov byte [pending_room_action], 0.
        self.pending_room_action = 0;
    }

    // = seg000:3520 room_person_present_auto_dialogue — per standing room-person, present their auto-
    // dialogue line if its condition matches and, having spoken, install the
    // person's dialogue verb menu. data_047a7 latches after the first person
    // speaks so only one interrupts the move.
    //
    // MINIMAL PORT: the present path (present_room_person_dialogue) and the
    // fall-through into install_pending_room_action_menu (loc_03551) are
    // modelled. Deferred: the messages_02aaf queued-message path taken when no
    // line is selected (seg000:3533). The room-leave scan runs with
    // pending_room_action == 1, so loc_03551 takes its speaker branch.
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
        // = seg000:3542..354c messages_02a51 (the "<person> is here" queued
        //   message) is not modelled; fall through into loc_03551.
        self.install_pending_room_action_menu();
    }

    // = seg000:3551 loc_03551 — install the command menu (or dialogue speaker)
    // for the currently-armed pending_room_action, then reveal it with the panel
    // fold. Fallen into by room_person_present_auto_dialogue after a room-leave
    // line (pending_room_action == 1 -> the loc_03595 speaker branch) and called
    // from the fly-over dispatch (travel_settle_companion_dispatch, seg000:3633)
    // after the companion's line (pending_room_action 3 / 4 -> the divert /
    // hostile-zone-warning menus).
    pub(crate) fn install_pending_room_action_menu(&mut self) {
        // = seg000:3551 inc byte [data_047a7] — latch so no other standing
        //   person speaks (or a further settle pass raises the cabin) this scan.
        self.data_047a7 = self.data_047a7.wrapping_add(1);
        match self.pending_room_action {
            // = seg000:3555 pending_room_action == 3: a fly-over passed a
            //   revealed location with companions aboard — the GO TOWARDS THIS
            //   PLACE divert menu.
            3 => {
                // = seg000:355c bp = menu_go_towards_this_place; 355f bx =
                //   menu_npc_actions_cleanup; 3562 call loc_0d323.
                self.stage_command_submenu(
                    MenuRef::MenuGoTowardsThisPlace,
                    GameState::menu_npc_actions_cleanup,
                );
                // = seg000:3565/356b ui_hud_elements[18]/[19].flags = 0 — drop the
                //   HUD head-ornament and balloon elements (the port handles those
                //   HUD elements structurally; no flags field to write).
            }
            // = seg000:3572 pending_room_action == 4: the flight is entering a
            //   hostile/non-Atreides zone — the CHANGE DESTINATION / IGNORE
            //   WARNING menu.
            4 => {
                // = seg000:357c and byte [menu_change_destination_ignore_warning
                //   + 0bh], 0bfh — clear the greyed (0x4000) bit on the WHAT ?
                //   entry's text id in the buffer.
                self.menu_destination_warning.records[2].text_id &= !CMD_GREY;
                // = seg000:3580 bx = menu_npc_actions_cleanup; 3583 call loc_0d323.
                self.stage_command_submenu(
                    MenuRef::MenuDestinationWarning,
                    GameState::menu_npc_actions_cleanup,
                );
                // = seg000:3586/358c ui_hud_elements[18]/[19].flags = 0 (as above).
                // = seg000:3592 jmp rebuild_and_draw_room_nav_panel — rebuild the
                //   bottom-right nav/compass HUD for the warning menu context.
                self.rebuild_and_draw_room_nav_panel();
            }
            // = seg000:3595 loc_03595 — the room-leave speaker branch (any other
            //   pending_room_action, e.g. the scan's value 1).
            action => {
                // = seg000:3595 cmp data_04774,0; jnz loc_035ac (ret) — skip while
                //   a dialogue panel is already up.
                if self.is_dialogue_active {
                    return;
                }
                // = seg000:359c cmp pending_room_action,64h; jnb loc_035ac (ret) —
                //   a >= 0x64 value is a speaking-person encoding, not an action.
                if action >= 0x64 {
                    return;
                }
                // = seg000:35a3 ax = current_lip_sync_resource_id; 35a6 call
                //   set_dialogue_speaker — mark the speaker met and stage their
                //   dialogue verb panel (TALK TO ME / COME WITH ME / WHAT? /
                //   STOP TALKING).
                let speaker = self.current_lip_sync_resource_id as u8;
                self.set_dialogue_speaker(speaker);
                // = seg000:35a9 call play_pending_panel_fold — reveal the staged
                //   verb panel with the accordion fold.
                self.play_pending_panel_fold();
            }
        }
    }

    // = seg000:d323 loc_0d323 — stage a submenu (DOS bp = record buffer, here
    // the element identity resolving to its owned buffer) with its cleanup
    // func (DOS bx) and reveal it with the panel fold: arm the fb1
    // transition, push the element, fold it in, then light the slot under
    // the cursor.
    pub(crate) fn stage_command_submenu(&mut self, menu: MenuRef, cleanup: MenuCleanupFn) {
        // = seg000:d323 call screen_overlay_request_transition — stage into fb1.
        self.screen_overlay_request_transition();
        // = seg000:d326 call screen_element_stack_push (bp menu, bx cleanup).
        self.menu_stack_push(menu, Some(cleanup));
        // = seg000:d329 call play_pending_panel_fold — reveal with the fold.
        self.play_pending_panel_fold();
        // = seg000:d32c jmp loc_0d410 -> highlight_hovered_text_action_item —
        //   light the slot under the cursor now the menu is shown.
        self.highlight_hovered_text_action_item();
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
        self.command_menu_buf
            .records
            .push(room_person_menu_item(text_id, entry.handler));

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
            self.command_menu_buf
                .records
                .push(room_person_menu_item(0x88 + k as u16, base_handler));
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
        //   base..base+data_0476a-1 in command_menu_buf.records), DOS lands on
        //   index base + (data_0476b - 1). Skip if that falls outside the run
        //   — DOS would silently corrupt adjacent memory.
        let run_len = self.data_0476a as usize;
        let target_within_run = (self.data_0476b as usize).wrapping_sub(1);
        if target_within_run >= run_len {
            return;
        }
        let base = self.command_menu_buf.records.len() - run_len;
        // = seg000:311a mov word ptr [di], 0x8f — patch the text_id (the
        //   handler word is untouched; the callback reads the id at dispatch,
        //   so nothing else needs rebinding).
        self.command_menu_buf.records[base + target_within_run].text_id = 0x8f;
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
    // through current_location_ptr: walks data_00009[current_location_ptr] via the
    // shared loc_06603 iterator with bp = loc_0316e (which buckets entries by
    // travel-mate/day/etc. and writes back into room_persons[12], [14], [15]
    // plus data_0476a/b), then specially handles data_00008[current_location_ptr]
    // == 0x21 by writing room_persons[13] and calling loc_02318, then runs
    // loc_0331e. The port has none of those structures yet: current_location_ptr,
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
        // = seg000:3140..316a the classification chain, run only for "special"
        //   rooms whose location_appearance low byte is 0x80 (the palace 0x180
        //   and the sietch/village/fortress rooms): the location's troops fill
        //   the dynamic room_persons[12..16] slots (Fremen chief / Fremen
        //   troops / Harkonnen captain), the smuggler den reveals SMUG, and
        //   the location's CONDIT block is staged (troops.rs).
        self.init_room_persons_special();
    }

    // = seg000:2ffb rebuild_and_draw_room_nav_panel — flip the four compass
    // direction buttons (ui_elements[13..17]) between visible-and-clickable
    // (flags 0x80) and hidden (flags 0x20) according to the current scene's
    // four direction-exit bytes, and gate the centre palace-plan button [17]
    // on being inside the Atreides palace, then redraw HUD records 12..18.
    //
    // A staged night attack and a travel/book mode do not customize the live
    // records at all: they re-install a whole template (blank, or the flight
    // panel) and draw that. The data_01cc4 mirror of the centre flags is dropped
    // — no consumer is ported yet.
    pub(crate) fn rebuild_and_draw_room_nav_panel(&mut self) {
        // = seg000:2ffb cmp byte ptr [night_attack_stage], 0; jnz loc_0301a —
        // the night attack clears the compass.
        if self.night_attack_stage != 0 {
            self.ui_install_nav_panel(&NAV_PANEL_BLANK);
            return;
        }
        // = seg000:3002 test game_screen_mode_flags,3; jz loc_03020 — a travel
        // or book mode owns the panel.
        if self.game_screen_mode_flags & 3 != 0 {
            // = seg000:3009 cmp data_011ca,0; jnz loc_0301a — an overlay owns
            //   the screen (a map screen, a fly-over dialogue), so the flight
            //   is suspended and its controls go away.
            // = seg000:3010 si = ui_nav_panel_flight; 3013 cmp
            //   travel_no_location_dest,0; jnz loc_0301d — only a directional
            //   flight is steerable. A flight homing on a location flies itself
            //   (the command panel offers SKIP TO DESTINATION / CHANGE
            //   DESTINATION instead), so it falls into loc_0301a and clears the
            //   panel.
            let steerable = self.data_011ca == 0 && self.travel_no_location_dest != 0;
            if steerable {
                self.ui_install_nav_panel(&NAV_PANEL_FLIGHT);
            } else {
                // = seg000:301a si = ui_nav_panel_blank.
                self.ui_install_nav_panel(&NAV_PANEL_BLANK);
            }
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
            self.ui_elements[NAV_PANEL_RECORD_OFFSET].sprite_id = 0x23;
            for i in 0..4 {
                self.ui_elements[NAV_PANEL_RECORD_OFFSET + 1 + i].flags = 0x80;
                self.ui_elements[NAV_PANEL_RECORD_OFFSET + 1 + i].sprite_id = 0x1d + i as i16;
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

        // = seg000:3039..3043 al = 0x20 (hidden), or 0x80 (visible) when the
        //   current location is the Atreides palace (current_location_ptr ==
        //   100h = locations[0]) — the centre button [17] opens the palace
        //   plan, which only exists there.
        // = seg000:3045..304e bx = 0x21; if dl == 1: bx = 0x22 and al = 0x20 —
        //   the box backing sprite depends on whether this is the location's
        //   entry room, which also hides the centre button.
        let (box_sprite_id, centre_flags) = if room == 1 {
            (0x22, 0x20)
        } else if self.current_location_index == 0 {
            (0x21, 0x80)
        } else {
            (0x21, 0x20)
        };
        self.ui_elements[NAV_PANEL_RECORD_OFFSET].sprite_id = box_sprite_id;
        // = seg000:3053 mov [di+46h],al — the centre element [17]'s flags.
        self.ui_elements[NAV_PANEL_RECORD_OFFSET + 5].flags = centre_flags;
        // = seg000:305c the exit-classification loop: for each compass
        //   direction (i = 0..3 → UP / RIGHT / DOWN / LEFT), show the arrow
        //   (flags 0x80) only when the exit byte is in 0xFB..0xFF; otherwise
        //   hide it (flags 0x20). Destination-room exits (0x01..0x7F) and
        //   in-scene/scripted exits don't get a HUD arrow.
        for (i, exit) in exits.iter().enumerate() {
            let exit = *exit as i8;
            let flag = if exit != 0 && exit >= -5 { 0x80 } else { 0x20 };
            self.ui_elements[NAV_PANEL_RECORD_OFFSET + 1 + i].flags = flag;
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
    pub(crate) fn reset_scene_lip_sync_state(&mut self) {
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

    // = seg000:2ee5 / seg000:c4e5 call call_restore_cursor — repaint the saved
    // background under the software cursor and mark it hidden, before the panel
    // verbs (seg000:2edd) or a fresh game-area frame (present_game_area,
    // seg000:c4dd) overwrite the screen. Two effects matter: no stale cursor
    // image is baked into the pushed rect, and — because the hide is committed
    // to cursor_hide_counter — the game loop's redraw_mouse (seg000:dc20) sees
    // the cursor as hidden and redraws it fresh on the new frame instead of
    // repainting its now-stale saved background over the new pixels. The latter
    // is what stops the software cursor from leaving turds over the per-pass
    // flight-HNM frames (travel_pump -> hnm_present_flight_frame).
    pub(crate) fn restore_cursor_over_panel(&mut self) {
        self.call_restore_cursor();
    }

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

    // = seg000:488a travel_arrival_landing_sequence — the orni-arrival landing
    // played before the arrival room renders (loc_02dfb gates it on data_04732
    // bit 0).
    fn travel_arrival_landing_sequence(&mut self) {
        // = seg000:488a..4891 ax = 6; si = [current_location_ptr]; call
        // calc_SAL_index — ax accumulates, so the result is directly the
        // approach-video id: 6 SIET (sietch), 7 PALACE (Atreides palace),
        // 8 (village), 9 FORT (Harkonnen fortress), 10 (Harkonnen palace).
        let appearance = self.locations[self.current_location_index as usize].appearance;
        let sal_video = 6 + crate::room_scene::calc_sal_index(appearance) as u16;
        // = seg000:4894/4896 cmp al,8; jnb loc_048e5.
        if sal_video < 8 {
            // = seg000:489a call_restore_cursor; 489d travel_minimap_state =
            // 0x80 — hide the minimap for the rest of the flight.
            self.call_restore_cursor();
            self.travel_minimap_state = 0x80u8 as i8;
            // = seg000:48b4..48bd the handoff frame: the sietch approach cuts
            // in at frame 0x3c of the sand loop, the palace at 0x16.
            let handoff: u16 = if sal_video == 6 { 0x3c } else { 0x16 };
            // = seg000:48a2 loc_048a2 — pump flight frames, forcing sand
            // terrain so the loop point re-aims at MNT1 (48a6/48a8), until the
            // playing clip has looped back into MNT1 (48ac) and its per-loop
            // frame count reaches the handoff frame (48c0). The MNT clips'
            // bit-4 resource flag presents each frame through
            // hnm_present_flight_frame (seg000:ccee); the port runs it after
            // the frame advances, like the travel pump.
            loop {
                if self.hnm_do_frame() {
                    self.hnm_present_flight_frame();
                }
                self.travel_select_flight_video(0);
                if self.hnm_video_id == 2 && self.hnm_counter_2 == handoff {
                    break;
                }
                self.tick_one_frame();
            }
            // = seg000:48c6 call hnm_switch_active_video (bx = the approach
            // video id, ax = the handoff frame) — the next loop-point check
            // (seg000:cb00) redirects the stream into the approach clip.
            self.hnm_switch_active_video(sal_video, handoff);
            // = seg000:48c9 loc_048c9 — play the approach clip to completion
            // (its resource flag has no loop bit, so the end marker finishes
            // and closes it).
            while !self.hnm_is_complete() {
                if self.hnm_do_frame() {
                    self.hnm_present_flight_frame();
                }
                self.tick_one_frame();
            }
            // DOS's streaming reader closes the resource itself as it runs
            // out of file (seg000:cb58/cb5d); the single-buffer port closes
            // it here.
            self.hnm_close();
            // = seg000:48d1 loc_048d1 — dec data_046e0: knock the saved
            // sky-fade state out of sync so the room reveal after the approach
            // runs the fade-in transition instead of a plain blit.
            self.data_046e0 = self.data_046e0.wrapping_sub(1);
        } else if sal_video == 9 {
            // = seg000:48dd/48e0 bp = gfx_copy_whole_framebuf_to_screen; call
            // loc_0c8fb — play the fortress approach FORT.HNM (ax is still the
            // SAL video id, 9) to completion in the game area.
            self.play_hnm_to_completion(9, gfx::gfx_copy_whole_framebuf_to_screen);
            // = seg000:48e3 jmp loc_048d1 — the fade-in force, as above.
            self.data_046e0 = self.data_046e0.wrapping_sub(1);
        } else {
            // = seg000:48e9..48fa the landing-pad types (village / Harkonnen
            // palace): re-render the pad scene without the parked orni that is
            // landing (orni_anim_frame = 0xff) into fb1 and snapshot it to fb2
            // for the animation frames to restore from.
            self.orni_anim_frame = 0xff;
            self.set_fb1_as_active_framebuffer();
            self.copy_game_area_rect_to_unknown_rect();
            self.draw_room_scene();
            self.update_screen_palette();
            self.copy_active_framebuffer_to_framebuffer_2();
            // = seg000:48fd..4909 the reverse landing animation: frames 0x1f
            // down to 0 (cl = -1) over the SN7 engine sound.
            self.orni_anim_frame = 0x1f;
            self.audio_start_voc("SN7.VOC");
            self.orni_anim_loop(-1);
            // = seg000:490c orni_anim_frame = 0 — the orni is parked again.
            self.orni_anim_frame = 0;
        }
        // = seg000:48d5 loc_048d5 — clear the arrival flags and reopen the
        // room's SAL resource (jmp open_SAL_resource).
        self.data_04732 = 0;
        self.sal_open_resource();
    }

    // = seg000:5ba0 copy_game_area_rect_to_unknown_rect — copy the game-area rect
    // (si=1470h) to the backdrop buffer (di=0d83ch) before drawing the room.
    // TODO: port; no-op stub.
    pub(crate) fn copy_game_area_rect_to_unknown_rect(&mut self) {}

    // = seg000:37b2 draw_room_scene — clear the game area and draw the current
    // location/room scene (= draw_SAL, seg000:3b59). The port's draw_location_room
    // does the SAL open + draw together (it also runs clear_game_area), driven by
    // the current location_and_room / location_appearance globals (data_00006).
    //
    // The DOS prologue (loc_098e6, loc_04d00, copy_game_area_rect_to_clip_rect)
    // is not ported yet.
    pub(crate) fn draw_room_scene(&mut self) {
        // = seg000:37b2 call reset_scene_lip_sync_state — tear down any active
        // talking head (and its idle/voc frame tasks) before redrawing the room,
        // so the LOOK AT MIRROR head stops compositing once the player looks away.
        self.reset_scene_lip_sync_state();
        // = seg000:37b8 orni_hotspot_x = 0 — no parked-orni hover hotspot until
        // this scene's orni pass (draw_room_ornis) records one.
        self.orni_hotspot_x = 0;
        // = seg000:37c4..37d7 ax = 0xffff; cmp [current_scene],al; jz — no room
        // scene (the desert, current_scene 0xff) short-circuits the scene-record
        // lookup and lands in the sign-bit branch loc_037dc. (An in-room scene
        // whose record's room byte is >= 0x80 — the character-scene renderer —
        // takes the same branch in DOS; that case is not ported.)
        if self.data_00008 == 0xff {
            // = seg000:37dc call loc_03ae9 — clear the character anchor tables
            // (no standing people in the desert).
            self.character_screen_pos = [(0xffff, 0xffff); 0x17];
            // = seg000:37df or [room_render_flags],1.
            self.room_render_flags |= 1;
            // = seg000:37e4 test [game_screen_mode_flags],3; jnz loc_037f4 —
            // only the plain room view draws the outdoor composite.
            if self.game_screen_mode_flags & 3 == 0 {
                // = seg000:37e9 falls into loc_037eb.
                self.draw_desert_view();
            } else {
                // = seg000:37f4 loc_037f4 — the travel flight view.
                self.travel_load_flight_view();
            }
            return;
        }
        self.draw_location_room(self.location_and_room, self.location_appearance);
    }

    // = seg000:c108 transition — render the new screen offscreen via `render`
    // (DOS passes the routine in bp and runs it through
    // gfx_call_bp_with_front_buffer_as_screen), then wipe it onto the visible
    // screen with effect `effect` (DOS al, via the segvga vga_transition) and
    // flush the palette.
    pub(crate) fn transition(&mut self, effect: u8, dx: i16, render: fn(&mut GameState)) {
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
        // plain copy below. `dx` is the DOS caller's dx register, preserved
        // across the render call (c10e push dx / c112 pop dx): the book page
        // turn (0x0e) reads its sign for the turn direction; most callers
        // leave it 0 (look_at_mirror `xor dx,dx`, ui_present_room_screen
        // `dx = 0`).
        gfx::vga_transition(self, effect as u16, dx);
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
    pub(crate) fn finish_room_screen_setup(&mut self) {}

    // = seg000:3723 loc_03723 — handle the pending dialogue / auto-action queued
    // in data_04735.
    // TODO: port; no-op stub.
    fn handle_pending_dialogue_action(&mut self) {}

    // = seg000:978e start_room_lip_sync — start the current speaker's lip-sync
    // (current_lip_sync_resource_id; 0xffff = none).
    // MINIMAL PORT: only its opening loc_04aca step (data_011ca = 1) is
    // modelled — during a travel that pauses travel_pump so the flight HNM does
    // not overdraw the head. The lip-sync data setup + head render (9799..97cb)
    // are the port's setup_talking_head at the call sites.
    fn start_room_lip_sync(&mut self) {
        // = seg000:978e call loc_04aca — data_011ca = 1.
        self.data_011ca = 1;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::NAV_PANEL_RECORD_OFFSET;
    use crate::{
        Equipment, GameState, dat_file::DatFile, game_ui::NAV_PANEL_ROOM, gfx, menu_defs::MenuRef,
    };

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

    // The orni-arrival landing sequence (travel_arrival_landing_sequence,
    // seg000:488a), approach-video branch: arriving at a sietch hides the
    // minimap and keeps pumping the flight on the MNT1 sand loop until the
    // per-loop frame count (hnm_counter_2) reaches 0x3c; the armed loop point
    // (hnm_switch_active_video + hnm_counter_4, seg000:cb00) then redirects
    // the stream into SIET.HNM, which plays to completion. Asset-gated and
    // real-time paced; run with:
    //   cargo test -p dune --bin dune -- --ignored travel_arrival_approach
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn travel_arrival_approach_video() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // The state an orni arrival at a sietch leaves for the scene reload:
        // the flight HNM open on the sand loop and the arrival bit armed.
        let sietch = game
            .locations
            .iter()
            .position(|l| l.appearance < 0x20)
            .expect("no sietch location");
        game.current_location_index = sietch as u16;
        game.travel_vehicle_mode = 2;
        game.hnm_load_first_frame_by_id(2, 0);
        // The flight palette the room draw's travel branch establishes
        // (seg000:3805/3809) — the MNT/SIET clips carry no header palette and
        // rely on it.
        gfx::vga_save_palette_to_fade_target(&mut game);
        game.set_sky_palette();
        game.data_04732 = 1;

        game.travel_arrival_landing_sequence();

        // The sequence hid the minimap, redirected the stream into the SIET
        // approach clip and played it out (finished + closed), cleared the
        // arrival bit and forced the sky-fade mismatch for the room fade-in.
        assert_eq!(game.travel_minimap_state, 0x80u8 as i8);
        assert_eq!(game.hnm_video_id, 6, "the stream never redirected to SIET");
        assert!(game.hnm_is_complete(), "the approach clip did not finish");
        assert!(!game.hnm_is_open(), "the approach clip was not closed");
        assert_eq!(game.data_04732, 0);
        assert_eq!(game.data_046e0, 0u8.wrapping_sub(1));

        game.screen
            .write_png_scaled(&game.palette, "travel_arrival_approach.png")
            .expect("write travel_arrival_approach.png");
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
            .active_menu_records()
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

        // present_game_area (present_game_area) must push the zoomed backdrop + head to
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
            game.get_active_menu_ref(),
            MenuRef::MenuNpcActions,
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
        game.menu_callback_choice_exit_menu(0, 0);

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

        // present_game_area (present_game_area) pushed the un-zoomed game area to the
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

    // Gurney's opening lines exercise the multi-part text continuation
    // (dialogue_text_continuation, = seg001:47b6). His second sentence entry
    // (word 0x40E8, phrase 0xBE8) holds two sentences split by a top-level
    // separator, so three presentations walk:
    //   1. entry 0BE7 — "I'm Gurney Halleck. I have served…"
    //   2. entry 40E8 part 1 — "I've just come into contact with the Fremen…"
    //      (the interpolator arms the continuation; the entry stays unspoken
    //      and the resume pointer stays AT the entry, seg000:a042)
    //   3. the continuation (loc_094dd) — "I have tried to convince them…"
    //      with current_subtitle_id += 0x1000 (the OB voc part), and only now
    //      the event/spoken-mark/advance (seg000:94ee -> a049..a0a7).
    // Asset-gated; run with:
    //   cargo test -p dune -- --ignored gurney_multi_part
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn gurney_multi_part_line_resumes_on_talk_to_me() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // Talk to Gurney (person 4). His first line's condition holds at game
        // start ((byte[0x18] & 0x20) == 0, game_phase < 0x14).
        game.common_dialogue(0x4);
        // The entry's raw phrase index is 0x131 (the analysis dump's absolute
        // 0xBE7 minus Gurney's bank base); the walk presents it as
        // (word1 byteswapped & 0x3ff) | 0x800.
        assert_eq!(
            game.current_subtitle_id, 0x931,
            "line 1: I'm Gurney Halleck"
        );
        assert!(
            game.dialogue_text_continuation.is_none(),
            "a single-sentence line leaves no continuation"
        );

        // Second click: the 40E8 entry's first sentence. The separator arms
        // the continuation; the entry is neither spoken nor advanced past.
        game.menu_callback_choice_talk_to_me(0, 0);
        assert_eq!(game.current_subtitle_id, 0x932, "line 2: part 1");
        assert!(
            game.dialogue_text_continuation.is_some(),
            "the separator arms the continuation"
        );
        let entry = game.dialogue_resume_entry_ptr as usize;
        assert_eq!(
            game.dialogue[entry] & 0x80,
            0,
            "the multi-part entry is not yet marked spoken"
        );
        let cont_len = game.dialogue_text_continuation.as_ref().unwrap().len();
        assert!(cont_len > 20, "sentence 2 pending ({cont_len} bytes)");
        // The part-2 voice exists in the DAT under the OB variant letter
        // (create_voc_file_name_from_bx, seg000:a8fd..a907: bits 12..15 of
        // the rebased index render as 'A'+v after the O suffix).
        let idx = (0x1932u16 & 0xf3ff).wrapping_sub(game.voc_base(4)) & 0xfff;
        let name = format!("PE\\PE{idx:03X}OB.VOC");
        assert!(
            game.dat_file.read(&name).is_ok(),
            "part-2 voice {name} in the DAT"
        );

        // Third click: the continuation presents sentence 2, steps the voc
        // part nibble, and only now fires the entry bookkeeping.
        game.menu_callback_choice_talk_to_me(0, 0);
        assert_eq!(game.current_subtitle_id, 0x1932, "line 3: part 2 (+0x1000)");
        assert!(
            game.dialogue_text_continuation.is_none(),
            "the final 0xff terminator clears the continuation"
        );
        assert_ne!(
            game.dialogue[entry] & 0x80,
            0,
            "the entry is marked spoken after its last part"
        );
        assert_eq!(
            game.dialogue_resume_entry_ptr as usize,
            entry + 4,
            "the resume pointer advances past the entry"
        );
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

        // The dialogue verb panel is the active menu, holding the four
        // menu_NPC_actions verbs (TALK TO ME / COME WITH ME / 0x95 / STOP TALKING).
        // Leto carries no travel/disabled flags, so slot 1 is the enabled COME
        // WITH ME (0x91, not greyed).
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuNpcActions,
            "dialogue verb panel should be on top of the menu stack"
        );
        let verbs: Vec<u16> = game
            .active_menu_records()
            .iter()
            .map(|r| r.text_id)
            .collect();
        assert_eq!(verbs, vec![0x90, 0x91, 0x95, 0x94], "NPC dialogue verbs");

        // Regression: when the voice finishes (lip_sync_stop) the voc task stops
        // and the head reverts to idle — mouth 0, not speaking. DOS does NOT force
        // a settle here; the idle finishes its lively animation and settles via the
        // countdown (see leto_idle_settles_to_calm_after_first_animation). The
        // prev-frame diff list survives untouched (= DOS [239F0], which
        // set_lipsync_data_to_al never updates), so the resumed idle diffs
        // calm-vs-calm and leaves the last speech frame on screen instead of
        // re-compositing — a full redraw here would pop the face-part z-order
        // (collar vs ear/chin) at every line boundary. Force "done".
        let head = game.talking_head.as_mut().unwrap();
        let pre_speech_flatten = vec![(1, 0, 0)];
        head.prev_images = pre_speech_flatten.clone();
        head.voc_total_samples = 0; // makes the next voc tick report "done"
        game.tick_talking_head_voc();
        let head = game.talking_head.as_ref().unwrap();
        assert!(!head.speaking, "voice should have stopped");
        assert_eq!(head.mouth, 0, "mouth should revert to closed (0)");
        assert_eq!(
            head.prev_images, pre_speech_flatten,
            "the diff list survives the voice end (= DOS [239F0]) so the idle does not recomposite"
        );
    }

    // The COME WITH ME verb (menu_callback_choice_come_with_me, seg000:95e2)
    // presents the speaker's topic-5 line and, when no spoken-line event drops
    // the interrupt gate, marks them travelling: room-person flags bit 0x40
    // (flipping the dialogue verb to STAY HERE on the next menu build) and
    // their persons_travelling_with bit. STAY HERE (seg000:9533) presents the
    // topic-6 line and undoes it (npc_clear_travelling). Asset-gated:
    //   cargo test -p dune --lib -- --ignored come_with_me
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn come_with_me_recruits_leto_and_stay_here_dismisses_him() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        game.common_dialogue(0x0); // Duke Leto; slot 1 offers COME WITH ME.
        assert_eq!(game.active_menu_records()[1].text_id, 0x91);
        assert_eq!(game.active_menu_records()[1].handler, 0x95e2);

        // Click COME WITH ME. Leto's topic-5 record selects his refusal line:
        // condition 32 `(byte[1b] == 0) |. (rand_bits & 4)` holds on the first
        // ask (ds:1b is still 0), and the line carries the stay-here event 2,
        // which drops the interrupt gate — Leto does not join.
        game.menu_callback_choice_come_with_me(0, 0);
        let phrase = game.current_subtitle_id;
        assert!(
            (0x800..=0xbff).contains(&phrase),
            "expected a come-with-me phrase id, got {phrase:#x}"
        );
        assert_eq!(game.data_0001b, 1, "ds:1b use counter");
        assert_eq!(game.pending_room_action, 0, "pending room-action cleared");
        assert_eq!(
            game.dialogue_interrupt_gate, 0,
            "the refusal drops the gate"
        );
        assert_eq!(game.persons_travelling_with & 1, 0, "Leto must not join");
        assert_eq!(
            game.active_menu_records()[1].text_id,
            0x91,
            "verb unchanged"
        );

        // Lady Jessica (person 1) accepts in game phases 6..8 (her topic-5
        // condition 96, `(game_phase == 0x51) |. (game_phase - 6 < 3)` — the
        // stage where she accompanies Paul to the Fremen). Enter dialogue with
        // her and ask.
        game.common_dialogue(0x1);
        game.game_phase = 6;
        game.menu_callback_choice_come_with_me(0, 0);
        assert_eq!(game.dialogue_interrupt_gate, 0xff, "gate must stay armed");
        assert_ne!(game.room_persons[1].flags & 0x40, 0, "flags bit 0x40");
        assert_ne!(game.persons_travelling_with & 2, 0, "travelling bit");
        // time_joined was refreshed: game_time 2 minus the static-zero
        // time_dismissed passes the 2-tick debounce.
        assert_eq!(game.room_persons[1].time_joined, game.game_time);

        // Ending the conversation (STOP TALKING pops the menu and runs
        // menu_npc_actions_cleanup, seg000:97cf) assigns the travelling
        // speaker to companion HUD slot 1 (npc_assign_companion_slot,
        // seg000:9855) and arms its 8-blink counter.
        game.menu_callback_choice_exit_menu(0, 0);
        assert_eq!(game.companions[0], 1, "Jessica in companion slot 1");
        assert_eq!(game.companions[1], -1, "slot 2 stays empty");
        assert_eq!(game.ui_hud_companion_blink[0], 0x10, "blink armed");
        assert_eq!(game.ui_elements[21].sprite_id, 0x42, "slot-1 portrait");
        // The cleanup also marks the speaker talked-to (seg000:97dd).
        assert_ne!(game.room_persons[1].flags & 0x20, 0, "flags bit 0x20");

        // The game-loop blink task (ui_hud_companion_blink_task, seg000:d7b7)
        // drains the 0x10 counter over 16 steps, blanking the fresh portrait
        // on the 8 odd counts. Force each 64-tick step edge by un-latching
        // instead of waiting ~5 s of real PIT time.
        let mut blanks = 0;
        for _ in 0..16 {
            game.companion_blink_step_latch = ((game.game_ticks() >> 6) as u8).wrapping_add(1);
            game.ui_hud_companion_blink_task();
            if game.ui_elements[21].sprite_id == 0x40 {
                blanks += 1;
            }
        }
        assert_eq!(blanks, 8, "the new portrait blinks 8 times");
        assert_eq!(game.ui_hud_companion_blink[0], 0, "blink counter drained");
        assert_eq!(game.companions[0], 1, "the real pair is restored");
        assert_eq!(
            game.ui_elements[21].sprite_id, 0x42,
            "portrait shown at the end"
        );

        // A click on the occupied companion HUD slot (ui_elements[21], the
        // box at (35,182)-(56,196)) resolves to Jessica through
        // person_hit_test's HUD branch (seg000:9289 -> companion_slot_hit_test) and
        // dispatches her person handler (loc_09234) — the dialogue reopens
        // from the portrait while she travels with Paul.
        game.mouse_pos_x = 45;
        game.mouse_pos_y = 190;
        assert_eq!(game.companion_slot_hit_test(), Some(1));
        assert_eq!(game.person_hit_test(), Some(1));
        game.callback_main_ui_element_21_22();
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuNpcActions,
            "the companion-portrait click opens the dialogue"
        );
        assert_eq!(game.current_lip_sync_resource_id, 1, "talking to Jessica");

        // With her dialogue open: the empty slot 2 misses (seg000:92e0 jz),
        // and her own portrait re-enters the dialogue via
        // callback_main_ui_element_19 (seg000:9259) without stacking a
        // second panel.
        game.mouse_pos_x = 68;
        game.mouse_pos_y = 190;
        game.callback_main_ui_element_21_22();
        game.mouse_pos_x = 45;
        game.mouse_pos_y = 190;
        game.callback_main_ui_element_21_22();
        assert_eq!(game.current_lip_sync_resource_id, 1);
        assert_eq!(
            game.menu_stack
                .iter()
                .filter(|e| e.0 == MenuRef::MenuNpcActions)
                .count(),
            1,
            "one dialogue panel after the portrait clicks"
        );
        // Close the reopened dialogue before the STAY HERE checks below.
        game.menu_callback_choice_exit_menu(0, 0);

        // A travelling companion is talked to from the HUD portrait, not from a
        // figure standing in the room: her location_and_room (0x2004) is not the
        // current room (0x200a), so a normal room render draws no sprite for her
        // and leaves character_screen_pos[1] absent (loc_03ae9 clears it to
        // 0xffff). The recruitment dialogue left a stale anchor here that the
        // synthetic flow never re-rendered away; clear it so the STAY HERE
        // dialogue below does not zoom (dialogue_zoom_room, seg000:3b1f), matching
        // the HUD-portrait open — the case whose cleanup removes the portrait.
        game.character_screen_pos[1] = (0xffff, 0xffff);

        // The next dialogue menu build offers STAY HERE in slot 1
        // (setup_npc_dialogue_menu, seg000:9108..910e).
        game.setup_npc_dialogue_menu(1);
        assert_eq!(game.active_menu_records()[1].text_id, 0x92);
        assert_eq!(game.active_menu_records()[1].handler, 0x9533);

        // Click STAY HERE: the travelling state is cleared again.
        // time_dismissed stays 0: the debounce reads time_joined (= game_time),
        // and game_time has not advanced 2 ticks since.
        game.menu_callback_choice_stay_here(0, 0);
        assert_eq!(game.room_persons[1].flags & 0x40, 0, "flag cleared");
        assert_eq!(
            game.persons_travelling_with & 2,
            0,
            "travelling bit cleared"
        );
        assert_eq!(game.room_persons[1].time_dismissed, 0, "debounced");

        // ...and the verb is COME WITH ME once more.
        game.setup_npc_dialogue_menu(1);
        assert_eq!(game.active_menu_records()[1].text_id, 0x91);

        // Closing the dialogue after STAY HERE vacates the HUD slot: the
        // portrait dialogue never zoomed (room_render_flags bit 7 clear), so the
        // cleanup takes the non-zoom path (loc_0982e) and, with the speaker no
        // longer travelling, calls npc_remove_companion_slot — clearing the slot
        // and its blink counter and reverting the portrait to the empty frame.
        game.menu_callback_choice_exit_menu(0, 0);
        assert_eq!(game.companions[0], -1, "STAY HERE removes the portrait");
        assert_eq!(game.ui_hud_companion_blink[0], 0, "blink cleared");
        assert_eq!(game.ui_elements[21].sprite_id, 0x40, "empty button frame");

        // Eviction (seg000:968c..96a8): with both slots full, a third
        // companion displaces slot 1 — the evictee's travelling state is
        // cleared, their person code lands in pending_room_action (0x64 + p),
        // slot 2 shifts down, and the newcomer takes slot 2.
        game.companions[0] = 3; // Duncan
        game.companions[1] = 4; // Gurney
        game.room_persons[3].flags |= 0x40;
        game.persons_travelling_with |= 1 << 3;
        game.npc_assign_companion_slot(1);
        assert_eq!(
            game.pending_room_action, 0x67,
            "evictee encoded as 0x64 + 3"
        );
        assert_eq!(game.room_persons[3].flags & 0x40, 0, "evictee detached");
        assert_eq!(game.persons_travelling_with & (1 << 3), 0);
        assert_eq!(game.companions[0], 4, "slot 2 shifted down");
        assert_eq!(game.companions[1], 1, "newcomer in slot 2");
        assert_eq!(game.ui_hud_companion_blink[1], 0x10, "newcomer blinks");
        game.pending_room_action = 0;

        // TALK TO ME resets the ds:1b counter (seg000:947a).
        game.menu_callback_choice_talk_to_me(0, 0);
        assert_eq!(game.data_0001b, 0, "TALK TO ME clears the use counter");
    }

    // = seg000:9263 loc_09263 — clicking the game area of a location's outdoor
    // arrival view (current_room == 1), with no person under the cursor, walks
    // inside through the scene's UP exit. This is how you enter a sietch by
    // clicking on it. Asset-gated:
    //   cargo test -p dune --lib -- --ignored click_sietch
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn click_sietch_game_area_enters_it() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // Stand outside Haga-Timin (locations[51], appearance 0x0a): its room 1
        // is the outdoor sietch entrance (scene background 0x01, up-exit 0x03).
        // = the arrival codes location_entry_room_codes(51) builds: dx =
        //   (appearance << 8) | 1, bx = ((index + 1) << 8) | 0x80.
        game.location_and_room = 0x0a01;
        game.location_appearance = 0x3480;
        game.current_room = 1;
        game.current_location_index = 51;
        game.draw_room_game_screen();
        assert_eq!(game.data_00008, 0x0a, "current_scene = the sietch code");
        assert_eq!(game.get_active_menu_ref(), MenuRef::CommandMenuBuf);

        // No Fremen stands in the outdoor entrance, so a game-area click hits
        // no person. Click inside the game area (mouse_y < 152).
        game.mouse_pos_x = 160;
        game.mouse_pos_y = 60;
        assert_eq!(game.person_hit_test(), None, "no person in the entrance");
        game.callback_main_ui_element_21_22();

        // The click walked us through the UP exit into the sietch interior.
        assert_eq!(
            game.location_and_room, 0x0a03,
            "entered the sietch (room 3)"
        );
        assert_eq!(game.current_room, 3);

        // A click that misses the game area (mouse_y >= 152, over the command
        // panel) does not enter — return to the entrance and confirm.
        game.location_and_room = 0x0a01;
        game.location_appearance = 0x3480;
        game.current_room = 1;
        game.draw_room_game_screen();
        game.mouse_pos_y = 0xb0;
        game.callback_main_ui_element_21_22();
        assert_eq!(
            game.location_and_room, 0x0a01,
            "a panel click does not enter"
        );
    }

    // Port-only debug overlay: the backquote (`) key toggles a panel of live
    // game state (game phase, location, charisma, …) drawn over the presented
    // frame. It composites onto a copy of the screen, so the game's own
    // framebuffers stay clean. Asset-gated:
    //   cargo test -p dune --lib -- --ignored debug_overlay
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn debug_overlay_toggles_and_draws_state() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // The backquote key edge toggles the overlay on, then off.
        assert!(!game.debug_overlay);
        game.input.lock().unwrap().kb_keys[0x29] = 0xff;
        game.poll_debug_overlay_toggle();
        assert!(game.debug_overlay, "` turns the overlay on");
        // Held (no new edge) does not re-toggle.
        game.poll_debug_overlay_toggle();
        assert!(game.debug_overlay, "holding ` does not flip it back");
        // Release, then press again toggles off.
        game.input.lock().unwrap().kb_keys[0x29] = 0;
        game.poll_debug_overlay_toggle();
        game.input.lock().unwrap().kb_keys[0x29] = 0xff;
        game.poll_debug_overlay_toggle();
        assert!(!game.debug_overlay, "a second press turns it off");

        // The overlay draws over a copy of the screen (top-left region), while
        // the far side of the frame is left untouched — the game framebuffer is
        // never modified.
        let mut fb = game.screen.clone();
        game.draw_debug_overlay(&mut fb);
        let mut changed = 0;
        for y in 0..80u16 {
            for x in 0..150u16 {
                if fb.get(x, y) != game.screen.get(x, y) {
                    changed += 1;
                }
            }
        }
        assert!(changed > 200, "the overlay drew text ({changed} px)");
        assert_eq!(
            fb.get(300, 120),
            game.screen.get(300, 120),
            "the overlay leaves the rest of the frame untouched"
        );
        if std::env::var_os("WRITE_PNG").is_some() {
            fb.write_png_scaled(&game.palette, "debug_overlay.png")
                .expect("write debug_overlay.png");
        }
    }

    // Toggling the overlay pushes a frame immediately, so it appears /
    // disappears at once even on an otherwise static screen. Asset-gated:
    //   cargo test -p dune --lib -- --ignored debug_overlay_toggle_frame
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn debug_overlay_toggle_forces_a_frame() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(16);
        // Not headless: send_frame_to_display must actually publish.
        let mut game = GameState::new(dat_file, tx);

        // A backquote edge toggles on and pushes one frame.
        game.input.lock().unwrap().kb_keys[0x29] = 0xff;
        game.poll_debug_overlay_toggle();
        assert!(game.debug_overlay);
        assert!(rx.try_recv().is_ok(), "toggling on pushes a frame");
        assert!(rx.try_recv().is_err(), "and only one");

        // A release (no edge) pushes nothing.
        game.input.lock().unwrap().kb_keys[0x29] = 0;
        game.poll_debug_overlay_toggle();
        assert!(rx.try_recv().is_err(), "a release does not push a frame");

        // The next press toggles off and pushes another frame.
        game.input.lock().unwrap().kb_keys[0x29] = 0xff;
        game.poll_debug_overlay_toggle();
        assert!(!game.debug_overlay);
        assert!(rx.try_recv().is_ok(), "toggling off pushes a frame");
    }

    // = seg000:d8f4 the per-click cursor hide — the game loop brackets every
    // button-edge dispatch with call_restore_cursor, so the cursor blinks off
    // while a HUD-arrow / command / game-area click is processed and comes back
    // afterwards. In Overlay (GPU/OS) cursor mode this drives
    // shared_cursor.hidden, which the present thread samples. Asset-gated:
    //   cargo test -p dune --lib -- --ignored cursor_hides
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn cursor_hides_during_a_click_in_overlay_mode() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        game.cursor_mode = crate::mouse::CursorMode::Overlay;

        // One game-loop mouse pass first: the cursor starts hidden
        // (= seg000:e64a, cursor_hide_counter -1) and redraw_mouse is what
        // clears the counter and shows it — a click can only follow a pass.
        game.get_mouse_pos_etc();
        let _ = game.redraw_mouse();

        // call_restore_cursor (the game loop's per-click hide) publishes the
        // cursor as hidden; the balancing draw_mouse shows it again.
        game.call_restore_cursor();
        assert!(
            game.shared_cursor.snapshot().hidden,
            "the click hides the cursor"
        );
        game.draw_mouse();
        assert!(
            !game.shared_cursor.snapshot().hidden,
            "the cursor comes back after the interaction"
        );

        // While a frame composes offscreen (front buffer == fb1), the live
        // cursor must not be touched — the hide is a no-op there.
        game.set_fb1_as_active_framebuffer();
        game.screen_buffer = crate::FbId::Fb1;
        game.call_restore_cursor();
        assert!(
            !game.shared_cursor.snapshot().hidden,
            "no cursor hide while composing offscreen"
        );
    }

    // The map-mode travel verb (build_room_command_records, seg000:2fd7)
    // depends on whether the flight has a location target. A homing flight
    // (travel_no_location_dest == 0) offers "SKIP TO DESTINATION" (seg000:4ffb); a
    // fixed-heading directional flight (travel_no_location_dest != 0 — no target, homing on
    // the starting point) replaces it with "BACK TO STARTING POINT"
    // (seg000:50a5), and from game_phase 0x32 also appends "TOWARDS NEAREST
    // PLACE" (seg000:50c4). Asset-gated:
    //   cargo test -p dune --lib -- --ignored directional_flight
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn directional_flight_replaces_skip_to_destination_with_back_to_starting_point() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();

        // A plain (non-special) room in map mode: location_appearance low byte
        // != 0x80 and game_screen_mode_flags & 3 != 0 select the loc_02fd7
        // travel-verb branch.
        game.location_appearance = 0x0000;
        game.game_screen_mode_flags = 1;

        // Homing flight (travel_no_location_dest == 0): SKIP TO DESTINATION, then CHANGE
        // DESTINATION.
        game.travel_no_location_dest = 0;
        game.game_phase = 0;
        game.build_room_command_records();
        assert_eq!(game.active_menu_records().len(), 2);
        assert_eq!(
            game.active_menu_records()[0].handler,
            0x4ffb,
            "SKIP TO DESTINATION"
        );
        assert_eq!(
            game.active_menu_records()[1].handler,
            0x497a,
            "CHANGE DESTINATION"
        );

        // Directional flight before phase 0x32: BACK TO STARTING POINT alone
        // (the case the port previously got wrong, showing SKIP TO DESTINATION).
        game.travel_no_location_dest = 0xff;
        game.game_phase = 0x20;
        game.build_room_command_records();
        assert_eq!(
            game.active_menu_records().len(),
            2,
            "no TOWARDS NEAREST PLACE yet"
        );
        assert_eq!(
            game.active_menu_records()[0].handler,
            0x50a5,
            "BACK TO STARTING POINT"
        );
        assert_eq!(
            game.active_menu_records()[1].handler,
            0x497a,
            "CHANGE DESTINATION"
        );

        // Directional flight from phase 0x32: BACK TO STARTING POINT + TOWARDS
        // NEAREST PLACE, then CHANGE DESTINATION.
        game.game_phase = 0x32;
        game.build_room_command_records();
        assert_eq!(game.active_menu_records().len(), 3);
        assert_eq!(
            game.active_menu_records()[0].handler,
            0x50a5,
            "BACK TO STARTING POINT"
        );
        assert_eq!(
            game.active_menu_records()[1].handler,
            0x50c4,
            "TOWARDS NEAREST PLACE"
        );
        assert_eq!(
            game.active_menu_records()[2].handler,
            0x497a,
            "CHANGE DESTINATION"
        );
    }

    // The travel branch of rebuild_and_draw_room_nav_panel (seg000:3002..301d):
    // in a travel mode the routine installs a whole template rather than editing
    // the live compass records, and only a steerable flight gets one with
    // buttons. Both orni entry points (TAKE AN ORNITHOPTER and the map screen's
    // GO THERE FLYING AN ORNI) land here, so the panel no longer depends on
    // which template the closing screen happened to leave behind. Asset-gated:
    //   cargo test -p dune --lib -- --ignored travel_nav_panel
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn travel_nav_panel_follows_the_flight_and_not_the_previous_screen() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();

        // The handler offsets of the live nav records 12..17.
        let handlers = |g: &GameState| {
            let o = NAV_PANEL_RECORD_OFFSET;
            [
                g.ui_elements[o].func_ptr,
                g.ui_elements[o + 1].func_ptr,
                g.ui_elements[o + 2].func_ptr,
                g.ui_elements[o + 3].func_ptr,
                g.ui_elements[o + 4].func_ptr,
                g.ui_elements[o + 5].func_ptr,
            ]
        };
        let flight = [0x0f66, 0x4ad0, 0x4f09, 0x4ad7, 0x0f66, 0x0f66];
        let blank = [0x0f66; 6];

        // Start from the room compass, as the room screen leaves it.
        game.ui_install_nav_panel(&NAV_PANEL_ROOM);

        // A directional flight (no location target, nothing holding the screen)
        // is steerable: turn left / flight button / turn right.
        game.game_screen_mode_flags = 5;
        game.data_011ca = 0;
        game.travel_no_location_dest = 0xff;
        game.rebuild_and_draw_room_nav_panel();
        assert_eq!(handlers(&game), flight, "directional flight steers");

        // Homing on a location: the orni flies itself, so the panel clears.
        game.travel_no_location_dest = 0;
        game.rebuild_and_draw_room_nav_panel();
        assert_eq!(
            handlers(&game),
            blank,
            "a homing flight has nothing to steer"
        );

        // An overlay holding the screen (a map screen, a fly-over dialogue)
        // suspends the flight and clears the panel with it.
        game.travel_no_location_dest = 0xff;
        game.data_011ca = 1;
        game.rebuild_and_draw_room_nav_panel();
        assert_eq!(handlers(&game), blank, "a suspended flight cannot steer");

        // A staged night attack clears it whatever the mode flags say.
        game.data_011ca = 0;
        game.night_attack_stage = 1;
        game.rebuild_and_draw_room_nav_panel();
        assert_eq!(
            handlers(&game),
            blank,
            "the night attack clears the compass"
        );
    }

    // Dialogue event 0x0b (callback_event_dialogue_line_0b_increase_game_phase_
    // by_1_and_do_more, seg000:a219): a story line advances the game phase,
    // zeroes the days-since-phase-change counter, runs the phase-trigger
    // record (DIALOGUE slot 135), and — at phase 1 — reveals Duncan Idaho
    // (room_persons[3].location_slot 0xff80 -> 0x0180). Covers the direct
    // dispatch (0 -> 1, Duncan) and the genuine data path: Leto's rally-
    // mission line (phrase 0x807) advancing phase 1 -> 2. Asset-gated:
    //   cargo test -p dune --lib -- --ignored event_0x0b
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn event_0x0b_advances_game_phase_and_reveals_duncan() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        assert_eq!(game.game_phase, 0, "a new game starts in phase 0");
        assert_eq!(
            game.room_persons[3].location_appearance, 0xff80,
            "Duncan starts hidden"
        );

        // The 0 -> 1 bump is not dialogue-reachable (it comes from the
        // unported troop-rally path via set_game_phase_and_trigger_callbacks),
        // so dispatch the event directly: phase 1 reveals Duncan Idaho.
        game.days_since_last_game_phase_change = 9;
        game.dispatch_dialogue_line_event(0x0b, 0);
        assert_eq!(game.game_phase, 1, "event 0x0b advances the phase");
        assert_eq!(game.days_since_last_game_phase_change, 0, "ds:ff zeroed");
        // = seg000:100b — Duncan's location_slot high byte flipped to 1: he
        // now matches palace room 0x2004.
        assert_eq!(
            game.room_persons[3].location_appearance, 0x0180,
            "Duncan Idaho revealed"
        );

        // An already-spoken line (word0 bit 0x80) is a no-op (seg000:a219).
        game.dispatch_dialogue_line_event(0x0b, 0x80);
        assert_eq!(game.game_phase, 1, "spoken bit gates a repeat");

        // The genuine data path: with the rally mission fulfilled (2 rallied
        // troops — the troop system that bumps the counter is unported),
        // Leto's topic-1 walk reaches entry 0x012a (phrase 0x807, event 0x0b,
        // condition 7 `(game_phase - 1 < 2) &. (rallied == 2)`) and advances
        // phase 1 -> 2. Each conversation presents one line — and a
        // multi-part line's event fires only after its LAST part (a042), so
        // walk any pending continuation with TALK TO ME (re-entering
        // common_dialogue would clear it, seg000:940c) before resuming.
        game.number_of_rallied_troops = 2;
        // Entry 0x0116 (phrase 0x802, word0 bit 6 = repeatable) blocks the
        // walk while Gurney (bit 0x10) is neither travelling nor present
        // (condition 2) — take him along, as the rally storyline does.
        game.persons_travelling_with |= 0x10;
        for _ in 0..6 {
            if game.game_phase != 1 {
                break;
            }
            game.common_dialogue(0x0);
            while game.game_phase == 1 && game.dialogue_text_continuation.is_some() {
                game.menu_callback_choice_talk_to_me(0, 0);
            }
        }
        assert_eq!(game.game_phase, 2, "Leto's mission line advances the phase");
        assert_eq!(
            game.current_subtitle_id & 0xfff,
            0x807,
            "the event-0x0b line spoke (its last part)"
        );
    }

    // set_game_phase_and_trigger_callbacks (seg000:121f): raising the phase
    // runs the phase-trigger record and the per-phase callback
    // (array_callbacks_for_game_phase_change): palace doors unlock, persons
    // move between rooms, locations appear on the map, charisma rises, and
    // COMM sightings / vision messages queue up. Asset-gated:
    //   cargo test -p dune --lib -- --ignored phase_callbacks
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn phase_callbacks_fire_on_phase_change() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // Phase 8 (callback_game_phase_change_08) unlocks palace room 2's
        // scripted west exit: palace_rooms[1].exits[3] 0x8c -> 0x0c.
        assert_eq!(game.scene_records[1].exits[3], 0x8c, "door starts locked");
        game.set_game_phase_and_trigger_callbacks(8);
        assert_eq!(game.game_phase, 8);
        assert_eq!(game.scene_records[1].exits[3], 0x0c, "door unlocked");

        // = seg000:121f cmp al,[game_phase]; jbe ret — a lower phase is a
        // no-op: callback 4 (background dec + stillsuit-maker locations) must
        // not run.
        game.set_game_phase_and_trigger_callbacks(4);
        assert_eq!(game.game_phase, 8, "phase never lowers");
        assert_eq!(game.scene_records[1].background, 0x3a, "callback 4 skipped");

        // Discovering Tuono-Harg (first_name 3 / last_name 6) advances to
        // phase 0x10 through the dispatcher (seg000:427e): Leto moves to
        // palace room 5, Jessica to room 9, and the Emperor's whereabouts
        // (location 1, person 0x0b) reach the COMM room.
        let th = game
            .locations
            .iter()
            .position(|l| l.first_name == 3 && l.last_name == 6)
            .expect("Tuono-Harg in locations[]");
        assert_ne!(game.locations[th].status & 0x80, 0, "starts undiscovered");
        game.location_mark_discovered(th);
        assert_eq!(game.game_phase, 0x10);
        assert_eq!(
            game.room_persons[0].location_and_room, 0x2005,
            "Leto in room 5"
        );
        assert_eq!(
            game.room_persons[1].location_and_room, 0x2009,
            "Jessica in room 9"
        );
        assert_eq!(
            game.comm_sightings,
            vec![0x10b],
            "Emperor sighting recorded"
        );

        // Phase 0x2c (met Stilgar): +0x14 charisma, Thufir restationed in
        // room 8 (slot 0x180), Paul-event bit 0x10 set, and Stilgar's five
        // sietches revealed on the map.
        let charisma_before = game.charisma;
        game.set_game_phase_and_trigger_callbacks(0x2c);
        assert_eq!(game.charisma, charisma_before + 0x14);
        assert_eq!(game.room_persons[2].location_and_room, 0x2008);
        assert_eq!(game.room_persons[2].location_appearance, 0x180);
        assert_ne!(game.bitfield_paul_events & 0x10, 0);
        for i in [45, 44, 46, 48, 49] {
            assert_eq!(
                game.locations[i].status & 0x80,
                0,
                "locations[{i}] revealed"
            );
        }

        // Phase 0x30 queues vision message 4 — but only once Paul has had his
        // first vision (bitfield_Paul_events bit 0, seg000:29f0); without it
        // the message is dropped. The Baron's sighting is recorded either way.
        game.set_game_phase_and_trigger_callbacks(0x30);
        assert!(game.vision_messages.is_empty(), "vision gated on bit 0");
        assert_eq!(game.comm_sightings, vec![0x10b, 0x1409]);

        // Phase 0x4c (Leto killed) with the vision bit set: message 0x105
        // queues and Jessica moves to room 2.
        game.bitfield_paul_events |= 1;
        game.set_game_phase_and_trigger_callbacks(0x4c);
        assert_eq!(game.vision_messages, vec![(0x105, 0)]);
        assert_eq!(game.room_persons[1].location_and_room, 0x2002);

        // Phase 0x38 was skipped over, so its callback (hide Leto,
        // location_slot high byte -> 0xff) never ran — DOS behaves the same:
        // only the final phase's callback fires.
        assert_eq!(game.room_persons[0].location_appearance, 0x180);
    }

    // Entering a sietch runs the room-entry troop classification
    // (init_room_persons -> callback_troop_location_0316e): the location's
    // rallied-troop chief (occupation bit 7) fills the dynamic Fremen-1 slot
    // (room_persons[14]) for room 2, appears in the verb panel, and is
    // talkable (ui_dialogue_related_to_Fremen1, seg000:9373). His COME WITH
    // ME verb (seg000:95c1) rallies the troop (troop_rally_troop_066ce).
    // Asset-gated:
    //   cargo test -p dune --lib -- --ignored fremen_chief
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn fremen_chief_present_in_sietch_and_rallies() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // init_troop_locations linked Carthag-Tuek (locations[12], troop
        // chain head 1 = troops[0], occupation 0x80) at startup.
        assert_eq!(
            game.troops[0].offset_of_location,
            crate::locations::location_ptr(12),
            "troops[0] linked to Carthag-Tuek"
        );

        // Arrive in Carthag-Tuek's audience room (room 2; the appearance
        // in-room form is (12+1)<<8 | 0x80 = 0x0d80 — the same codes Gurney's
        // static room_persons entry carries).
        game.location_and_room = 0x0002;
        game.location_appearance = 0x0d80;
        game.build_room_command_records();
        game.build_persons_in_room_records();

        // The classification put the chief's troop behind Fremen 1: his
        // dynamic entry matches the room, so he stands in it and gets a verb.
        assert_eq!(game.fremen1_troop, Some(0), "chief troop classified");
        assert_eq!(game.room_persons[14].location_and_room, 0x0002);
        assert_eq!(game.room_persons[14].location_appearance, 0x0d80);
        assert_ne!(game.persons_in_room & (1 << 14), 0, "chief in the room");
        assert_ne!(game.persons_in_room & (1 << 4), 0, "Gurney in the room");
        let chief = game
            .active_menu_records()
            .iter()
            .find(|r| r.text_id == 0x78 + 14)
            .expect("the chief's &Person verb");
        assert_eq!(chief.handler, 0x9373, "ui_dialogue_related_to_Fremen1");
        // The room draw resolves him through char_to_sprite_walk_facing:
        // troop_id 1 -> PERS pair 0x0e + 1 % 3 = 0x0f.
        assert_eq!(game.character_sprite_map()[14], 0x0f);

        // Talk to him: the trampoline stages his troop CONDIT block and runs
        // the common dialogue entry with the FRM head.
        game.ui_dialogue_related_to_fremen1();
        assert_eq!(game.current_lip_sync_resource_id, 0x0e);
        assert_eq!(game.troop_condit.troop_id, 1, "troop block staged");
        assert_eq!(game.location_condit.spice_density, 0x54, "location staged");
        assert!(game.talking_head.is_some(), "the chief's talking head");
        // The dialogue verb panel's dynamic slot is the chief's COME WITH ME
        // (person 0x0e: text 0x96, handler seg000:95c1).
        assert_eq!(game.active_menu_records()[1].text_id, 0x96);
        assert_eq!(game.active_menu_records()[1].handler, 0x95c1);

        // COME WITH ME: the charisma check passes and the troop is rallied —
        // occupation becomes "rallied, awaiting orders" (2), the rally count
        // and charisma rise, and the re-classified chief now stands behind
        // Fremen 2 (his entry matches the room again).
        let charisma_before = game.charisma;
        game.menu_callback_choice_come_with_me_troop(0x96, 0);
        assert_eq!(game.number_of_rallied_troops, 1, "troop rallied");
        assert_eq!(game.troops[0].occupation, 2, "rallied, awaiting orders");
        assert_eq!(game.charisma, charisma_before + 1);
        assert_eq!(game.troops[0].time_period_of_ralliement, game.game_time);
        assert_eq!(game.room_persons[15].location_and_room, 0x0002);
        assert_eq!(game.fremen2_troops[0], Some(0), "reclassified as Fremen 2");
        assert_eq!(
            game.locations[12].discoverable_at_phase, 2,
            "the sietch gains a discovery phase"
        );

        // The rally re-ran setup_npc_dialogue_menu (seg000:964f) to re-target
        // the panel at Fremen 2. Equal priority bytes (0xfc) mean the DOS
        // insert REPLACED the NpcActionsMenu slot (seg000:d343 jz) — the
        // stack must not deepen, so a single STOP TALKING closes the
        // dialogue and reveals the room menu.
        assert_eq!(
            game.menu_stack
                .iter()
                .filter(|e| e.0 == MenuRef::MenuNpcActions)
                .count(),
            1,
            "one NpcActionsMenu entry after the panel rebuild"
        );
        game.menu_callback_choice_exit_menu(0x94, 0);
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::CommandMenuBuf,
            "one STOP TALKING returns to the room menu"
        );
    }

    // = seg000:9240 loc_09240 — a game-area click on a Fremen-2 person (draw
    // ids 0x0f.., the figures titled "Fremen Chief" / "Nth Fremen Chief")
    // enters that person's dialogue by round-robin slot, exactly like
    // clicking their verb in the command panel. Asset-gated:
    //   cargo test -p dune --lib -- --ignored fremen2_sprite
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn fremen2_sprite_click_opens_their_dialogue() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // Rally Carthag-Tuek's troop (troops[0], the chain head of
        // locations[12]): a rallied troop (occupation bit 7 clear, not
        // Harkonnen) classifies behind Fremen 2, not Fremen 1.
        game.troops[0].occupation = 2;

        // Enter Carthag-Tuek's audience room (room 2); the draw classifies
        // the troops, rebuilds the person verbs, and records the on-screen
        // anchors.
        game.location_and_room = 0x0002;
        game.location_appearance = 0x0d80;
        game.current_room = 2;
        game.current_location_index = 12;
        game.draw_room_game_screen();
        assert_eq!(game.fremen2_troops[0], Some(0), "classified as Fremen 2");

        // His verb is "Fremen Chief" (0x87 = 0x78 + person id 0x0f).
        let slot = game
            .active_menu_records()
            .iter()
            .position(|r| r.text_id == 0x87)
            .expect("the Fremen Chief verb (text_id 0x87) not in the menu");

        // The room draw recorded his anchor under person id 0x0f.
        let (fx, fy) = game.character_screen_pos[0x0f];
        assert!(fx != 0xffff, "the Fremen-2 anchor was not recorded");

        // Hovering the sprite highlights the Fremen Chief verb slot.
        game.mouse_pos_x = fx + 16;
        game.mouse_pos_y = fy + 40;
        assert_eq!(game.person_hit_test(), Some(0x0f));
        game.highlight_hovered_text_action_item();
        assert_eq!(
            game.index_of_last_hovered_action_item as usize, slot,
            "hover did not highlight the Fremen Chief verb slot"
        );

        // Clicking the sprite enters his dialogue (the loc_09240 branch),
        // staging the same troop the panel verb would.
        game.callback_main_ui_element_21_22();
        assert_eq!(game.selected_fremen2, 0, "round-robin slot 0 selected");
        assert_eq!(game.current_lip_sync_resource_id, 0x0f);
        assert_eq!(game.troop_condit.troop_id, 1, "troop CONDIT block staged");
        assert!(game.talking_head.is_some(), "the Fremen head is up");
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuNpcActions,
            "the dialogue verb panel is active"
        );
    }

    // The Carthag-Timin chief (troops[2], troop_id 3) carries styling
    // variant 2: walk_facing_sprite (seg000:913b) gives sprite FRM1 and
    // facing 3/3 % 15 + 1 = 2, so his idle plays animation facing-1 = 1 and
    // his speech frames come from the variant-banked lip-id animation at
    // mouth + (facing-1)*4 (set_lipsync_data_to_al, seg000:9e12..9e31) —
    // four single mouth-region sprites per variant, drawn in that variant's
    // beard styling. FRM1's bank is 15 variants x 4 = 60 frames. Asset-gated:
    //   cargo test -p dune -- --ignored timin_chief
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn timin_chief_speech_uses_variant_banked_lip_frames() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // Arrive in Carthag-Timin's audience room (room 2; locations[11],
        // in-room appearance form (11+1)<<8 | 0x80 = 0x0c80).
        game.location_and_room = 0x0002;
        game.location_appearance = 0x0c80;
        game.build_room_command_records();
        game.build_persons_in_room_records();
        assert_eq!(game.fremen1_troop, Some(2), "chief troop classified");

        // Talk to him through his verb record's bound callback: FRM1 with the
        // variant-2 expression.
        let chief = *game
            .active_menu_records()
            .iter()
            .find(|r| r.text_id == 0x78 + 14)
            .expect("the chief's &Person verb");
        (chief.callback)(&mut game, chief.text_id, 0);
        let head = game
            .talking_head
            .as_ref()
            .expect("the chief's talking head");
        assert_eq!(head.talking_head_id, 0x0e, "FRM1");
        assert_eq!(head.facing, 2, "styling variant 2");

        // The lip-id animation (the sheet's last) is banked per variant:
        // FRM1 serves 15 variants (walk_facing_sprite modulus 0x0f), four
        // mouth frames each; the chief's are frames 4..7.
        let lip_anim = head.lipsync.animations.len() - 1;
        let lip_frames = head.lipsync.animations[lip_anim].frames.len();
        assert_eq!(lip_frames, 60, "FRM1 lip bank = 15 variants x 4 mouths");
    }

    // The dialogue text engine (show_voice_subtitle seg000:88af ->
    // draw_subtitle_body seg000:8b11): a spoken line's phrase decodes through
    // the PHRASE bank + token dictionary and renders per voice_subtitle_mode —
    // mode 0 as the outlined text strip above the command panel, mode 1 as
    // the ICONES speech balloon, mode 2 as no text at all. Asset-gated:
    //   cargo test -p dune --lib -- --ignored dialogue_text
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn dialogue_text_renders_as_strip_and_balloon() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // Mode 0 (TEXT): Leto's greeting renders as the outlined strip.
        game.voice_subtitle_mode = 0;
        game.voice_subtitle_mode_default = 0;
        game.common_dialogue(0x0);

        // The string pipeline decoded a real sentence: re-run it and check.
        let s = game
            .get_phrase_or_command_string(game.current_subtitle_id)
            .to_vec();
        let expanded = game.expand_phrase_tokens(&s);
        let text = game.format_interpolated_string(&expanded);
        let printable: String = text
            .iter()
            .take_while(|&&b| b < 0xf0)
            .map(|&b| {
                if (0x20..0x80).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        eprintln!("Leto greeting: {printable:?}");
        assert!(printable.len() > 10, "the phrase decoded to text");
        assert!(
            printable
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .count()
                > 8,
            "mostly letters: {printable:?}"
        );

        // The strip overlay is live and its glyphs + outline reached fb1
        // just above the command panel (rect (0, 0x92 - h .. 0x92)).
        let bubble = game.subtitle_bubble.as_ref().expect("strip overlay live");
        assert!(bubble.strip, "mode 0 draws the strip");
        let yoff = game.y_offset as i16;
        let (mut fg, mut outline) = (0, 0);
        for y in bubble.rect.y0..bubble.rect.y1 {
            for x in 0..320u16 {
                match game.framebuffer.get(x, y as u16) {
                    0x0f => fg += 1,
                    0xf0 => outline += 1,
                    _ => {}
                }
            }
        }
        assert!(fg > 50, "glyph pixels drawn ({fg})");
        assert!(
            outline > fg,
            "the outline pass wrapped the glyphs ({outline})"
        );
        assert_eq!(bubble.rect.y1, yoff + 0x92, "strip sits above the panel");
        game.framebuffer
            .write_png_scaled(&game.palette, "subtitle_strip.png")
            .expect("write subtitle_strip.png");

        // Mode 1 (TEXT + VOICE): the next line renders as a speech balloon —
        // the strip is taken down first (subtitle_restore_prior), the balloon
        // rect gets an fb2 save-under and the tiled ICONES background in fb1.
        game.voice_subtitle_mode = 1;
        game.voice_subtitle_mode_default = 1;
        game.menu_callback_choice_talk_to_me(0, 0);
        let bubble = game.subtitle_bubble.as_ref().expect("balloon overlay");
        assert!(!bubble.strip, "mode 1 draws the balloon");
        assert!(!bubble.saved_fb2.is_empty(), "fb2 save-under grabbed");
        // = seg000:91d4 — the balloon x0 carries the per-head patch from
        // talking_head_balloon_x_table (seg001:22a8); Leto is head 0 = 0x60.
        assert_eq!(bubble.rect.x0, 0x60, "the seg001:2224 rects, Leto's x");
        let mut non_bg = 0;
        for y in bubble.rect.y0..bubble.rect.y1 {
            for x in bubble.rect.x0..bubble.rect.x1 {
                if game.framebuffer.get(x as u16, y as u16) != 0 {
                    non_bg += 1;
                }
            }
        }
        assert!(non_bg > 500, "balloon background + text drawn ({non_bg})");

        // = seg000:97ba (recomposite_head_over_backdrop) — the presented frame
        // (present_game_area pushes fb1) must show the head *over* the balloon,
        // never the balloon over the head. In the head∩balloon overlap, fb1
        // should therefore be dominated by head pixels, not the balloon's tiled
        // background. Sample the background colour from a pure-balloon corner
        // (top-right, clear of the head and the centred text).
        let bg = game
            .framebuffer
            .get((bubble.rect.x1 - 6) as u16, (bubble.rect.y0 + 6) as u16);
        let head_rect0 = {
            let head = game.talking_head.as_ref().expect("head live");
            let (x0, y0, x1, y1) = head.rect;
            crate::Rect {
                x0,
                y0: y0 + game.y_offset as i16,
                x1,
                y1: y1 + game.y_offset as i16,
            }
        };
        let ov = crate::Rect {
            x0: head_rect0.x0.max(bubble.rect.x0),
            y0: head_rect0.y0.max(bubble.rect.y0),
            x1: head_rect0.x1.min(bubble.rect.x1),
            y1: head_rect0.y1.min(bubble.rect.y1),
        };
        assert!(
            ov.x1 > ov.x0 && ov.y1 > ov.y0,
            "the head and balloon overlap"
        );
        let (mut bg_px, mut total) = (0u32, 0u32);
        for y in ov.y0..ov.y1 {
            for x in ov.x0..ov.x1 {
                total += 1;
                if game.framebuffer.get(x as u16, y as u16) == bg {
                    bg_px += 1;
                }
            }
        }
        assert!(
            bg_px * 2 < total,
            "the balloon covers the head in the overlap ({bg_px}/{total} bg px) \
             — head not composited on top"
        );

        // = seg000:979f..97a9 (start_room_lip_sync) — the balloon was stamped
        // into fb2 as part of the head's backdrop, so the per-frame head
        // restores (copy fb2 -> fb1 over the head rect) keep the balloon
        // beneath the head sprites instead of punching a hole in it.
        let mut fb2_balloon = 0;
        for y in bubble.rect.y0..bubble.rect.y1 {
            for x in bubble.rect.x0..bubble.rect.x1 {
                if game.framebuffer_saved.get(x as u16, y as u16) != 0 {
                    fb2_balloon += 1;
                }
            }
        }
        assert!(
            fb2_balloon > 500,
            "balloon stamped into the fb2 backdrop ({fb2_balloon})"
        );
        // Simulate the head update's backdrop restore over its clip rect and
        // check the balloon survives in fb1.
        let head_rect = {
            let head = game.talking_head.as_ref().expect("head live");
            let (x0, y0, x1, y1) = head.rect;
            crate::Rect {
                x0,
                y0: y0 + game.y_offset as i16,
                x1,
                y1: y1 + game.y_offset as i16,
            }
        };
        crate::gfx::vga_copy_rect(&mut game.framebuffer, &game.framebuffer_saved, head_rect);
        let overlap = crate::Rect {
            x0: head_rect.x0.max(bubble.rect.x0),
            y0: head_rect.y0.max(bubble.rect.y0),
            x1: head_rect.x1.min(bubble.rect.x1),
            y1: head_rect.y1.min(bubble.rect.y1),
        };
        let mut survived = 0;
        for y in overlap.y0..overlap.y1 {
            for x in overlap.x0..overlap.x1 {
                if game.framebuffer.get(x as u16, y as u16) != 0 {
                    survived += 1;
                }
            }
        }
        assert!(
            survived > 200,
            "the balloon survives a head backdrop restore ({survived})"
        );
        game.framebuffer
            .write_png_scaled(&game.palette, "subtitle_balloon.png")
            .expect("write subtitle_balloon.png");

        // Mode 2 (VOICE ONLY): no text; ending the conversation restores the
        // room and leaves no overlay either way.
        game.voice_subtitle_mode = 2;
        game.voice_subtitle_mode_default = 2;
        game.menu_callback_choice_talk_to_me(0, 0);
        // The prior balloon was restored and (the line being voiced) no new
        // overlay was drawn.
        assert!(game.subtitle_bubble.is_none(), "mode 2 draws nothing");

        game.menu_callback_choice_exit_menu(0, 0);
        assert!(game.subtitle_bubble.is_none(), "cleanup leaves no overlay");
    }

    // Regression: a smaller balloon replacing a larger one must not leave the
    // old balloon's edges behind. The ICONES background is tiled clamped to
    // the balloon rect (blit_repeated_x); if a tile spilled past the rect,
    // those pixels were never saved to fb2 nor cleaned on restore, so a
    // subsequent smaller balloon left a frame of debris. Asset-gated:
    //   cargo test -p dune --lib -- --ignored balloon_shrink
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn balloon_shrink_leaves_no_debris() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        game.voice_subtitle_mode = 1;
        game.voice_subtitle_mode_default = 1;
        game.common_dialogue(0x0);

        // Present a long line (large balloon) then a short one (small balloon)
        // through the same per-line steps present_first_matching_dialogue_line
        // uses: take the prior bubble down, re-save the head backdrop, draw the
        // new balloon, then stamp it into fb2 (the seg000:979f a0aa step).
        let mimic = |game: &mut GameState, text: &[u8]| -> crate::Rect {
            game.set_fb1_as_active_framebuffer();
            game.subtitle_restore_prior();
            game.setup_talking_head(0, 0);
            game.set_fb1_as_active_framebuffer();
            game.subtitle_pad_left = 0x28;
            game.subtitle_pad_right = 0x10;
            game.subtitle_pad_top = 0x10;
            game.subtitle_pad_bottom = 0x10;
            game.font_state.color = 0x00f0;
            game.font_select_tall_font();
            game.rand_bits_seed = 0;
            let mut t = text.to_vec();
            t.push(0xff);
            game.draw_subtitle_body(&t);
            let r = game.subtitle_bubble.as_ref().unwrap().rect;
            crate::gfx::vga_copy_rect(&mut game.framebuffer_saved, &game.framebuffer, r);
            r
        };

        let long = b"The spice must flow my son for the Emperor demands his tribute \
and the Harkonnen vultures circle above us waiting for the smallest mistake \
we might make in the deep desert of Arrakis where the great worms roam";
        let r_big = mimic(&mut game, long);
        let r_small = mimic(&mut game, b"Yes my son.");
        assert!(
            r_big.y1 > r_small.y1 && r_big.x1 >= r_small.x1,
            "the second balloon is smaller ({r_big:?} -> {r_small:?})"
        );

        // A head backdrop restore (fb2 -> fb1 over the head clip rect, what a
        // head update does each frame) must not repaint any balloon in the
        // area the big balloon vacated.
        let head_rect = {
            let head = game.talking_head.as_ref().unwrap();
            let (x0, y0, x1, y1) = head.rect;
            let yoff = game.y_offset as i16;
            crate::Rect {
                x0,
                y0: y0 + yoff,
                x1,
                y1: y1 + yoff,
            }
        };
        crate::gfx::vga_copy_rect(&mut game.framebuffer, &game.framebuffer_saved, head_rect);

        // In the vacated area (inside r_big, outside r_small), fb1 and fb2 must
        // agree — no stray balloon that only one of them carries.
        let mut leftovers = 0;
        for y in r_big.y0..r_big.y1 {
            for x in r_big.x0..r_big.x1 {
                let in_small =
                    x >= r_small.x0 && x < r_small.x1 && y >= r_small.y0 && y < r_small.y1;
                if in_small {
                    continue;
                }
                if game.framebuffer.get(x as u16, y as u16)
                    != game.framebuffer_saved.get(x as u16, y as u16)
                {
                    leftovers += 1;
                }
            }
        }
        assert_eq!(
            leftovers, 0,
            "the shrunk balloon left {leftovers} px of debris"
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
            game.active_menu_records()[0].text_id,
            0x90,
            "the talking variant while the voice plays"
        );

        // Drain the voice without waiting it out: a zero clip length makes the
        // next lip-sync tick take the lip_sync_stop path.
        game.talking_head.as_mut().unwrap().voc_total_samples = 0;
        game.tick_talking_head_voc();
        assert!(!game.talking_head.as_ref().unwrap().speaking);

        // = seg000:d61d..d646 — slot 0 flipped in place in the menu_NPC_actions
        // buffer (seg001:1f80), which also carries the idle text into the next
        // conversation's panel.
        assert_eq!(
            game.active_menu_records()[0].text_id,
            0x9f,
            "the quoted idle variant once the voice ends"
        );
        assert_eq!(game.menu_npc_actions.records[0].text_id, 0x9f);

        // = seg000:9ed5 menu_callback_choice_what — the " WHAT ? " verb (slot
        // 2, text 0x95) replays the drained line: current_subtitle_id is
        // unchanged, its voice reloads and the head speaks again, and the
        // TALK TO ME verb flips back to its talking variant.
        let phrase = game.current_subtitle_id;
        game.menu_callback_choice_what(0x95, 0);
        assert_eq!(game.current_subtitle_id, phrase, "the same line replays");
        let head = game.talking_head.as_ref().unwrap();
        assert!(
            head.speaking && !head.voc_lipsync.is_empty(),
            "the voice restarted"
        );
        assert_eq!(
            game.active_menu_records()[0].text_id,
            0x90,
            "the talking variant while the replay plays"
        );
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
