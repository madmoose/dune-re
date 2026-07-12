//! Generic in-game room/scene drawing.
//!
//! This mirrors the DOS path shared by `intro_palace_equipment_room`
//! (seg000:0972) and the many in-game scenes that draw a room: a 16-bit
//! `location_and_room` code (the DOS `dx`, stored at seg001:0004) plus a
//! `location_appearance` (the DOS `bx`) are resolved into a SAL room sheet, a room
//! sub-chunk, and a sprite sheet, which are then drawn into the active
//! framebuffer.
//!
//! Resolution chain (all from static seg001 tables, ported below):
//!
//!   loc_008f0 / open_SAL_resource (seg000:08f0, 2d74)
//!     bh = location_appearance >> 8 selects locations[bh-1]; calc_SAL_index
//!     (seg000:5e4f) maps that location's `apparence` byte to one of the four
//!     SAL files (SIET / PALACE / VILG / HARK).
//!
//!   loc_03efe (seg000:3efe)
//!     dh = location_and_room >> 8 indexes SCENE_DISPATCH (seg001:13c4);
//!     dl = location_and_room & 0xff indexes the selected scene-record table
//!     as record (dl-1). The record's first byte is the "room byte".
//!
//!   draw_SAL (seg000:3b59)
//!     room sub-chunk = (room_byte - 1) & 0x0f
//!     sprite sheet   = ROOM_SHEET_NAMES[(room_byte - 1) >> 4]
//!                      (= resource ((room_byte-1)>>4) + 0x13)
//!
//! e.g. the palace equipment room (location_and_room = 0x2002, slot = 0x180):
//! apparence locations[0]=0x20 -> PALACE.SAL; scene record palace_rooms[1] =
//! 0x3a -> room 9 + EQUI.HSQ.

use crate::{
    DrawOptions, GameState, Rect, RoomRenderer, RoomSheet, SpriteSheet, blit, sal_position_markers,
    sprite_bank,
};

// = SAL room sheets, resources 0xa1..0xa4 (calc_SAL_index result + 0xa1).
const SAL_NAMES: [&str; 4] = ["SIET.SAL", "PALACE.SAL", "VILG.SAL", "HARK.SAL"];

// = room sprite sheets, resources 0x13..0x22, indexed by (room_byte-1) >> 4.
// "libre" is an unused slot; a scene that selects it would be unhandled.
const ROOM_SHEET_NAMES: [&str; 16] = [
    "GENERIC.HSQ", // 0x13
    "PROUGE.HSQ",  // 0x14
    "COMM.HSQ",    // 0x15
    "EQUI.HSQ",    // 0x16
    "BALCON.HSQ",  // 0x17
    "CORR.HSQ",    // 0x18
    "POR.HSQ",     // 0x19
    "SIET1.HSQ",   // 0x1a
    "XPLAIN9.HSQ", // 0x1b
    "libre",       // 0x1c
    "BUNK.HSQ",    // 0x1d
    "FINAL.HSQ",   // 0x1e
    "SERRE.HSQ",   // 0x1f
    "BOTA.HSQ",    // 0x20
    "PALPLAN.HSQ", // 0x21
    "SUN.HSQ",     // 0x22
];

// = seg001:1972 room1_backdrop_base — the per-SAL outdoor-backdrop base resource
// that draw_outdoor_backdrop (seg000:3839) reads as
// room1_backdrop_base[calc_SAL_index]: SIET -> DS0 (0x3c), PALACE -> DP1 (0x72),
// VILG -> 0x7f, HARK -> DF1 (0x76) / DH0 (0x84). The matching per-SAL thresholds
// in room1_backdrop_threshold (seg001:1977) are {5, 4, 5, 4, 4}; they gate the
// game_time/map branch (seg000:3834), which the port does not model (see
// draw_outdoor_backdrop), so the threshold table itself is not needed here.
const ROOM1_BACKDROP_BASE: [u8; 5] = [0x3c, 0x72, 0x7f, 0x76, 0x84];

// = chani struct PalaceRoom (seg001:1225 palace_rooms et al). The first byte
// selects the room's SAL sub-chunk and sprite sheet (see draw_SAL); the four
// `exits` bytes are the scene's per-direction exits resolved by
// ui_click_move_room (seg000:3f27) and rebuild_and_draw_room_nav_panel
// (seg000:2ffb).
#[derive(Clone, Copy)]
struct SceneRecord {
    background: u8,
    // One byte per compass direction; index i maps to the bottom-right HUD
    // compass arrow at i = 0..3 (UP / RIGHT / DOWN / LEFT, i.e. N/E/S/W). The
    // byte at index i is the exit reachable in that direction:
    //   0x00:        no exit in this direction
    //   0x01..0x7F:  destination room number — ui_click_move_room stores it as
    //                the new `location_and_room` low byte (seg000:3faa).
    //   0x80..0xFF:  special-exit dispatch via the jump table at cs:1454h
    //                (seg000:3fd2..3ff7). The 0xFB..0xFF subrange is what
    //                rebuild_and_draw_room_nav_panel renders as a visible
    //                HUD arrow; the rest are in-scene/scripted exits.
    exits: [u8; 4],
}

impl SceneRecord {
    const fn new(background: u8, exits: [u8; 4]) -> Self {
        Self { background, exits }
    }
}

// = seg001:13c4 scene dispatch table, indexed by dh (0x00..0x2f). Each entry
// is an index into SCENE_RECORDS picking the first record of a scene-record
// run (palace_rooms, sietch_rooms, …). The original table holds seg001 byte
// offsets (0x1225 + index * 5); converted here to record indices.
#[rustfmt::skip]
const SCENE_DISPATCH: [u8; 0x30] = [
    12, 14, 16, 18, 20, 22, 25, 28,
    31, 35, 39, 43, 47, 51, 55, 59,
    63, 14, 16, 18, 20, 22, 25, 28,
    31, 35, 39, 43, 47, 51, 55, 59,
     0, 68, 68, 68, 68, 68, 68, 68,
    69, 72, 75, 78, 69, 72, 75, 78,
];

// = seg001:1225..13c4 scene records (palace_rooms at index 0, sietch_rooms at
// index 12, and the rest), 5 bytes each in the original layout.
#[rustfmt::skip]
const SCENE_RECORDS: [SceneRecord; 83] = [
    SceneRecord::new(0x4c, [0x02, 0x00, 0xfd, 0x00]),
    SceneRecord::new(0x3a, [0x07, 0x00, 0x01, 0x8c]),
    SceneRecord::new(0xcf, [0x00, 0x00, 0x00, 0x0b]),
    SceneRecord::new(0x62, [0x0a, 0x00, 0x07, 0x00]),
    SceneRecord::new(0x4b, [0x00, 0x00, 0x00, 0x0a]),
    SceneRecord::new(0x15, [0x0b, 0x00, 0x00, 0x00]),
    SceneRecord::new(0x5d, [0x04, 0x8b, 0x02, 0x88]),
    SceneRecord::new(0x26, [0x00, 0x87, 0x0c, 0x00]),
    SceneRecord::new(0x63, [0x00, 0x00, 0x0a, 0x00]),
    SceneRecord::new(0x61, [0x09, 0x05, 0x04, 0x00]),
    SceneRecord::new(0x64, [0x00, 0x83, 0x06, 0x07]),
    SceneRecord::new(0x5e, [0x08, 0x02, 0x00, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x72, [0x00, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x73, [0x00, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x74, [0x00, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x75, [0x00, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x76, [0x00, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x77, [0x03, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x72, [0x00, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x01, [0x03, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x78, [0x00, 0x00, 0x03, 0x00]),
    SceneRecord::new(0x7b, [0x02, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x7a, [0x03, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x72, [0x00, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x01, [0x03, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x79, [0x04, 0x00, 0x03, 0x00]),
    SceneRecord::new(0x7b, [0x02, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x7d, [0x00, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x01, [0x03, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x7c, [0x04, 0x00, 0x03, 0x00]),
    SceneRecord::new(0x7b, [0x02, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x7d, [0x00, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x01, [0x03, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x74, [0x04, 0x00, 0x03, 0x00]),
    SceneRecord::new(0x7b, [0x02, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x7d, [0x00, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x75, [0x04, 0x03, 0x01, 0x00]),
    SceneRecord::new(0x72, [0x00, 0x00, 0x00, 0x02]),
    SceneRecord::new(0x7d, [0x00, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x76, [0x03, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x73, [0x04, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x7d, [0x00, 0x00, 0x03, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x77, [0x03, 0x04, 0x01, 0x00]),
    SceneRecord::new(0x72, [0x00, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x7d, [0x00, 0x00, 0x00, 0x02]),
    SceneRecord::new(0x01, [0x03, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x78, [0x04, 0x00, 0x03, 0x00]),
    SceneRecord::new(0x7b, [0x02, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x7d, [0x00, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x7a, [0x03, 0x04, 0x01, 0x00]),
    SceneRecord::new(0x72, [0x00, 0x00, 0x02, 0x00]),
    SceneRecord::new(0x7d, [0x00, 0x00, 0x00, 0x02]),
    SceneRecord::new(0x01, [0x03, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x7c, [0x05, 0x00, 0x03, 0x00]),
    SceneRecord::new(0x7b, [0x02, 0x00, 0x01, 0x00]),
    SceneRecord::new(0x7d, [0x00, 0x05, 0x00, 0x00]),
    SceneRecord::new(0xde, [0x00, 0x00, 0x02, 0x04]),
    SceneRecord::new(0x01, [0xff, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0x01, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0xa7, [0x00, 0x03, 0x01, 0x00]),
    SceneRecord::new(0xa6, [0x00, 0x00, 0x00, 0x02]),
    SceneRecord::new(0x02, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0xa7, [0x00, 0x00, 0x01, 0x03]),
    SceneRecord::new(0xa6, [0x00, 0x02, 0x00, 0x00]),
    SceneRecord::new(0x03, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0xa7, [0x00, 0x03, 0x01, 0x00]),
    SceneRecord::new(0xa6, [0x00, 0x00, 0x00, 0x02]),
    SceneRecord::new(0x04, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0xa7, [0x00, 0x00, 0x01, 0x03]),
    SceneRecord::new(0xa6, [0x00, 0x02, 0x00, 0x00]),
    SceneRecord::new(0x05, [0x02, 0xfe, 0xfd, 0xfc]),
    SceneRecord::new(0xa8, [0x00, 0x00, 0x01, 0x00]),
];

// = seg000:5e4f calc_SAL_index. Maps a location's `apparence` byte to a SAL
// index via ascending thresholds (0=SIET 1=PALACE 2=VILG 3/4=HARK).
fn calc_sal_index(apparence: u8) -> usize {
    let mut index = 0;
    if apparence >= 0x20 {
        index += 1;
    }
    if apparence >= 0x21 {
        index += 1;
    }
    if apparence >= 0x28 {
        index += 1;
    }
    if apparence >= 0x30 {
        index += 1;
    }
    index
}

// = seg000:3f67..3faa the destination-room resolution of ui_click_move_room.
// loc_03efe resolves si to the current scene record (5 bytes: background at
// offset 0, the four exit bytes at offsets 1..4), and `mov dl, [bp+si]` reads
// the exit at the 1-based offset `bp`; the port indexes `exits[direction]`
// directly (direction = bp - 1). Returns the new `location_and_room` for a
// plain destination-room exit (0x01..0x7F), or `None` when there is no exit in
// that direction (0x00) or the byte is a special-exit code (0x80..0xFF) —
// ui_click_move_room branches to the (unported) desert dispatch for those
// before calling here.
fn compass_move_target(location_and_room: u16, exits: [u8; 4], direction: usize) -> Option<u16> {
    let exit = exits[direction];
    // = seg000:3f6c or dl,dl; 3f6e jz loc_03f14 — no exit in this direction.
    if exit == 0 {
        return None;
    }
    // = seg000:3f70 js loc_03fd2 — sign bit set (0x80..0xFF) is a special exit
    //   dispatched via the jump table at cs:1454h (0xFB..0xFF are compass-HUD
    //   slot exits, 0x80..0xFA are scripted/in-scene exits). TODO: not ported.
    if exit & 0x80 != 0 {
        return None;
    }
    // = seg000:3f6a mov dl,[bp+si] — DOS keeps dh (the location high byte) in dx
    //   and replaces only dl with the new room number. The port carries the
    //   combined (dh:room) word as the new location_and_room; ui_click_move_room
    //   then records its low byte into pending_destination_room (seg000:3faa).
    Some((location_and_room & 0xff00) | exit as u16)
}

impl GameState {
    // = the four direction-exit bytes of the scene record selected by the
    // current (location_and_room, location_appearance). Returns `None` for
    // combinations that resolve outside the table (the only place this should
    // bite is during startup before location_and_room/location_appearance are
    // valid).
    pub(crate) fn current_scene_exits(&self) -> Option<[u8; 4]> {
        // = loc_03efe: index SCENE_DISPATCH by dh then offset by (dl - 1).
        let dh = (self.location_and_room >> 8) as usize;
        let dl = (self.location_and_room & 0xff) as usize;
        if dl == 0 || dh >= SCENE_DISPATCH.len() {
            return None;
        }
        let base = SCENE_DISPATCH[dh] as usize;
        let idx = base + (dl - 1);
        if idx >= SCENE_RECORDS.len() {
            return None;
        }
        Some(SCENE_RECORDS[idx].exits)
    }

    // = seg000:3f15
    pub fn ui_click_move_up(&mut self) {
        self.ui_click_move_room(1);
    }

    // = seg000:3f1a ui_click_room_right
    pub fn ui_click_move_right(&mut self) {
        self.ui_click_move_room(2);
    }

    // = seg000:3f1f ui_click_room_down
    pub fn ui_click_move_down(&mut self) {
        self.ui_click_move_room(3);
    }

    // = seg000:3f24 ui_click_room_left
    pub fn ui_click_move_left(&mut self) {
        self.ui_click_move_room(4);
    }

    // = seg000:3f27 ui_click_move_room — handle a compass-button click.
    // `direction` is 1..4 (UP/N, RIGHT/E, DOWN/S, LEFT/W), DOS's bp: the
    // 1-based offset into the current scene record's exit bytes.
    //
    // Two whole-subsystem gaps remain, both ending in the desert-position
    // dispatch at loc_03ff5 (the step-delta table at seg001:1454, applied by
    // loc_0b5cf, then the location-arrival resolution loc_04002..loc_0401f):
    //   - the outdoor desert-walk branch (seg000:3f49, location_appearance low
    //     byte != 0x80) — unreachable in the port, which only renders in-room
    //     views (location_appearance low byte 0x80);
    //   - the special-exit branch (seg000:3fd2, exit byte 0x80..0xFF): walking
    //     out of the location into the desert. Its teardown (command_list_ptr
    //     handoff, current_scene = 0xff) is deliberately not run here — without
    //     the dispatch it would strand the player with no scene.
    // The create_save_cl autosave hooks (cl=2/3) are stubbed: the save system
    // is not ported (see start()).
    pub(crate) fn ui_click_move_room(&mut self, direction: usize) {
        // = seg000:3f28
        self.dismiss_stacked_overlays();

        // = seg000:3f2b
        self.pcm_player.end_loop();

        // = seg000:3f2e call lip_sync_stop — stop any voice lip-sync before the
        // move (a speaker may still be mid-line when a compass button is hit).
        self.lip_sync_stop();

        // = seg000:3f32 mov [data_047a9],0 — clear the comms-room
        //   "message pending" flag (callback_main_ui_element_20_room_game_area
        //   jumps to menu_callback_choice_comms_room_message_viewed on it). The
        //   comms-room flow is not ported, so there is no flag to clear yet.

        // = seg000:3f37
        self.entering_new_sietch = 0;

        // = seg000:3f44 cmp bl,80h; jnz — an outdoor (desert) view: bump the
        //   walk counters and take the desert-position dispatch.
        if self.location_appearance & 0xff != 0x80 {
            // = seg000:3f49..3f56 data_04735 = ((data_04735 & 0x7f) + 1,
            //   saturated at 0x7f) | 0x80 — the high bit arms the auto-action
            //   dispatch in the game loop, the low bits count steps.
            let steps = ((self.data_04735 & 0x7f) + 1).min(0x7f);
            self.data_04735 = steps | 0x80;
            // = seg000:3f59..3f60 cap desert_walk_counter at 0x14.
            if self.desert_walk_counter < 0x14 {
                self.desert_walk_counter += 1;
            }
            // = seg000:3f64 jmp loc_03ff5 — the desert-position dispatch.
            // TODO: port the outdoor traversal (loc_03ff5/loc_0b5cf and the
            //   arrival resolution loc_04002..loc_0401f); unreachable until an
            //   outdoor view can be entered at all.
            return;
        }

        let Some(exits) = self.current_scene_exits() else {
            return;
        };
        // = seg000:3f70 js loc_03fd2 — a special exit (0x80..0xFF): leave the
        //   location into the desert. TODO: port loc_03fd2's teardown + the
        //   desert-position dispatch (see the function comment).
        if exits[direction - 1] & 0x80 != 0 {
            return;
        }
        // = seg000:3f67 loc_03f67 (the bl == 0x80 in-room path) -> loc_03efe
        //   resolves the scene record; compass_move_target reads the exit byte
        //   and bails on a no-exit value.
        let Some(new_room) = compass_move_target(self.location_and_room, exits, direction - 1)
        else {
            return;
        };

        // = seg000:3f72 cmp [current_room],1 — leaving the location's entry
        //   room (room 1, the one you arrive in from the desert).
        if self.current_room == 1 {
            // = seg000:3f79..3f81 create_save_cl cl=2 — TODO: autosave; the
            //   save system is not ported.
        }

        // = seg000:3f84 mov si,[command_list_ptr] — the current location record
        //   (the room verb list IS the location record, base seg001:0100). The
        //   port derives its index from location_appearance's high byte, the
        //   1-based location slot (the same index draw_location_room uses).
        let loc_index = ((self.location_appearance >> 8) as usize).wrapping_sub(1);
        if let Some(location) = self.locations.get_mut(loc_index) {
            // = seg000:3f88 test byte [si+0ah],10h — first in-room move inside
            //   an unvisited location.
            if location.status & 0x10 == 0 {
                // = seg000:3f8e or byte [si+0ah],10h — mark it visited.
                location.status |= 0x10;
                // = seg000:3f92 cmp dh,20h; adc [number_of_sietches_visited],0
                //   — the carry folds in +1 for location codes below 0x20 (the
                //   sietches); cities/palaces (>= 0x20) do not count.
                if (self.location_and_room >> 8) < 0x20 {
                    self.number_of_sietches_visited += 1;
                }
                // = seg000:3f9a mov [entering_new_sietch],0ffh.
                self.entering_new_sietch = 0xff;
                // = seg000:3f9f..3fa7 create_save_cl cl=3 — TODO: autosave; the
                //   save system is not ported.
            }
        }

        // = seg000:3faa mov [pending_destination_room], dl — record the pending destination room
        //   so the room-leave dialogue scan's conditions can read it (condition
        //   0x1c gates Leto's "where are you going so fast" on pending_destination_room == 4).
        self.pending_destination_room = (new_room & 0xff) as u8;
        // = seg000:3fae mov byte [data_00023], 1 — request the room-leave scan.
        self.data_00023 = 1;
        // = seg000:3fb3 call arm_dialogue_interrupt_gate — arm the interrupt gate to 0xff.
        self.dialogue_interrupt_gate = 0xff;
        // = seg000:3fb8 call run_room_leave_dialogue_scan — run the data_00023-gated room-person
        //   dialogue scan. A standing person whose auto-dialogue condition matches
        //   speaks a line; if that line carries the stay_here event (0x02) it
        //   clears dialogue_interrupt_gate to interrupt the move.
        self.run_room_leave_dialogue_scan();
        // = seg000:3fbd call test_dialogue_interrupt_gate; jz loc_03fc3 — the gate still 0xff means no
        //   person interrupted, so commit the move; otherwise abort it.
        if self.dialogue_interrupt_gate != 0xff {
            // = seg000:3fc2 ret — a person's auto-dialogue interrupted the move.
            return;
        }

        // = seg000:3fc3 loc_03fc3 — commit the move. DOS first calls loc_0abd5
        //   (drain any playing voc); none plays on the no-interrupt path, so it is
        //   a no-op here.
        // = seg000:3fca mov byte [data_00023], 5 — mark the committed transition.
        self.data_00023 = 5;

        // = seg000:3fcf jmp loc_04057; 4057 call move_all_NPCs_whose_bit_6_of_
        //   flags_is_set — companions in the room being left follow the player
        //   to the destination. Runs before location_and_room is updated: the
        //   scan matches against the room being left.
        self.move_all_npcs_whose_bit_6_of_flags_is_set(new_room, self.location_appearance);
        // = seg000:405a mov [location_and_room],dx — commit the destination
        //   (re-recorded by draw_location_room when the redraw runs below, but
        //   the no-redraw return paths still need it committed).
        self.location_and_room = new_room;
        // = seg000:405e..4064 rotate current_room into previous_room.
        self.previous_room = std::mem::replace(&mut self.current_room, (new_room & 0xff) as u8);
        // = seg000:4067 mov [location_appearance],bx — bx is unchanged on the
        //   in-room path, so this is a no-op here (the desert arrival paths
        //   that enter loc_04057 with a new bx are not ported).
        // = seg000:406b cmp [data_046eb],0; js ret — no room redraw while the
        //   ornithopter/travel view owns the screen.
        if self.data_046eb & 0x80 != 0 {
            return;
        }
        // = seg000:4072 cmp dx,3002h; jz game_phase_set_to_c8_game_ending —
        //   arriving in the Harkonnen fortress room 2 ends the game
        //   (seg000:16fc sets game_phase 0xc8 and runs the ending sequence).
        if new_room == 0x3002 {
            // = seg000:16fc mov [game_phase],0c8h.
            self.game_phase = 0xc8;
            // TODO: port the ending sequence game_phase_set_to_c8_game_ending
            //   falls into (loc_01771 onwards); the room redraw is skipped (DOS
            //   never returns here).
            return;
        }

        // = seg000:407b jmp loc_02dbf — re-enter draw_room_game_screen at its
        //   scene-reload entry to draw the destination room (draw_location_room
        //   re-records location_and_room at seg001:0004).
        self.draw_location_room(new_room, self.location_appearance);
        // The DOS room re-enter (loc_00d8e -> seg000:0dad jmp ui_toggle_room_view)
        // recomposes the left frieze + date/time indicator into fb1 — its
        // ui_set_and_draw_frieze_sides_closed_book runs offscreen — before the
        // room render copies the whole fb1 to the screen. draw_room_game_screen
        // (seg000:2db1) does NOT redraw the frieze; only the live-clock
        // ui_redraw_date_and_time_indicator does, and it touches the screen, not
        // fb1. This compass shortcut bypasses ui_toggle_room_view, so without the
        // refresh fb1 keeps the indicator drawn at the initial room entry
        // (game_time seeded to 2); blitting that stale fb1 snaps the sun/moon back
        // to the start until the next run_events_for_current_time_period redraw.
        self.gfx_call_bp_with_front_buffer_as_screen(|s| {
            s.ui_set_and_draw_frieze_sides_closed_book()
        });
        self.draw_room_game_screen();
        self.send_frame_to_display();
    }

    // = seg000:40c3 move_all_NPCs_whose_bit_6_of_flags_is_set — via
    // scan_matching_room_person_entries with the NPC_move_if_flag_bit_6_set_040c9
    // callback (seg000:40c9): every room-person entry matching the room being
    // left whose flags carry bit 0x40 is rewritten to the destination, so a
    // companion follows the player into the new room. The scan matches the
    // entries against the *memory* (location_and_room, location_appearance) —
    // the room being left — while the callback receives the caller's dx/bx (the
    // destination, restored around the call at seg000:3707..370a).
    fn move_all_npcs_whose_bit_6_of_flags_is_set(
        &mut self,
        location_and_room: u16,
        location_appearance: u16,
    ) {
        let (cur_room, cur_appearance) = (self.location_and_room, self.location_appearance);
        for entry in self.room_persons.iter_mut() {
            if entry.location_and_room == cur_room
                && entry.location_appearance == cur_appearance
                // = seg000:40c9 test byte [si+0eh],40h.
                && entry.flags & 0x40 != 0
            {
                // = seg000:40cf/40d1 — move the entry to the destination.
                entry.location_and_room = location_and_room;
                entry.location_appearance = location_appearance;
            }
        }
    }

    // = seg000:0972..0987 — the generic
    // "draw an in-game room" entry: open the SAL for `location_appearance`
    // (loc_008f0 -> open_SAL_resource) and draw the room selected by
    // `location_and_room` (loc_037b2 -> draw_SAL) into the active framebuffer.
    //
    // The normal draw_SAL path (room byte < 0x80) is modelled, including the
    // clear_game_area it runs first (see draw_sal_room) and the standing-person
    // drawing it does for `persons_in_room` (see draw_sal_room ->
    // sal_position_markers / RoomRenderer::draw_character). The separate
    // room-byte >= 0x80 branch (loc_037dc), which renders characters a
    // different way, is not ported. The caller still owns setting
    // `persons_in_room` before drawing.
    pub fn draw_location_room(&mut self, location_and_room: u16, location_appearance: u16) {
        // = seg001:0004 location_and_room: record the room being drawn so
        // get_location_and_room / add_room_frame_task can read it back.
        self.location_and_room = location_and_room;

        let dh = (location_and_room >> 8) as usize;
        let dl = (location_and_room & 0xff) as usize;
        let bh = (location_appearance >> 8) as usize;

        // = loc_008f0 / open_SAL_resource / calc_SAL_index: locations[bh-1]
        //   .apparence picks the SAL. open_SAL_resource maps a calc result of
        //   4 back to 3, so SAL indices clamp to the four SAL files.
        let apparence = self.locations[bh - 1].appearance;
        // open_SAL_resource clamps a calc result of 4 back to 3 for the four SAL
        // files; the outdoor-backdrop table (draw_outdoor_backdrop) keeps the
        // unclamped 0..4 index, so capture it before clamping.
        let sal_index = calc_sal_index(apparence);
        let sal_name = SAL_NAMES[sal_index.min(3)];

        // = loc_03efe: pick scene record (dl-1) in the table starting at
        //   SCENE_DISPATCH[dh]. The record's `background` byte drives draw_SAL.
        let record = &SCENE_RECORDS[SCENE_DISPATCH[dh] as usize + (dl - 1)];
        let background = record.background;

        // = draw_SAL (seg000:3b59): split the background byte into a SAL room
        //   sub-chunk and a sprite-sheet resource.
        let room = ((background - 1) & 0x0f) as usize;
        let sheet_name = ROOM_SHEET_NAMES[((background - 1) >> 4) as usize];

        // = seg000:37c1 clear_game_area — draw_room_scene clears the game-area
        // rect of the active framebuffer before drawing. Without it a scene that
        // does not paint every pixel (e.g. the dithered water reflection in the
        // SIET cave) shows the previous stage's leftover framebuffer through the
        // gaps. DOS does this in draw_room_scene before the sky/SAL; the port
        // does it here so draw_sky paints onto the cleared area.
        self.clear_game_area();

        // = seg000:39f5..3a16 draw_room_scene's pre-SAL backdrop dispatch.
        if location_and_room == 0x2005 || location_and_room == 0x1005 {
            // = seg000:39f8/39fd the palace balcony (0x2005) and sietch-side
            // window (0x1005) draw the sky gradient before the SAL. draw_sky also
            // runs set_sky_palette, which installs the sky-gradient palette
            // entries the room sheet's own palette update then leaves untouched.
            self.draw_sky();
        } else if dl == 1 {
            // = seg000:3a02 dec al; jnz loc_03a20 — every location's first room
            // (location_and_room low byte == 1) instead draws an outdoor view
            // sprite behind the SAL via loc_037eb -> loc_0380c. (The dh == 0x21
            // sub-case at seg000:3a06 only randomises which SAL room is drawn, not
            // the backdrop, so the backdrop runs for all first rooms.)
            self.draw_outdoor_backdrop(location_appearance, sal_index);
        }

        self.draw_sal_room(sal_name, room, sheet_name);

        // = seg000:3a24..3a7b draw_room_scene's post-SAL orni pass.
        self.draw_room_ornis();
    }

    // = seg000:3a24..3a7b — the orni pass at the end of draw_room_scene: on an
    // outdoor first-room view, draw one parked orni per available ornithopter
    // on the landing pad. DOS falls into this straight after the seg000:3a21
    // draw_SAL call.
    fn draw_room_ornis(&mut self) {
        // = seg000:3a24 cmp [sky_fade_active],0 — only sky scenes (the flag the
        // backdrop/sky path's set_sky_palette just set).
        if !self.sky_fade_active {
            return;
        }
        // = seg000:3a2b cmp byte ptr [location_and_room],1 — only each
        // location's first room (the outdoor view with the landing pad).
        if self.location_and_room & 0xff != 1 {
            return;
        }
        // = seg000:3a32 cmp [orni_anim_frame],0ffh — 0xff hides the ornis (the
        // take-off sequence re-renders the scene without them).
        if self.orni_anim_frame == 0xff {
            return;
        }
        // = seg000:3a39 cl = the available-equipment ornithopter count
        // (seg001:46ff); jcxz — nothing parked here.
        let count = self.available_equipment.ornithopters;
        if count == 0 {
            return;
        }
        // = seg000:3a45 restart the parked-orni animation.
        self.orni_anim_frame = 0;
        // = seg000:3a4a test [room_render_flags],81h — the night-attack /
        // no-character renders skip the sprites (but still exit through
        // set_sky_palette below).
        if self.room_render_flags & 0x81 == 0 {
            // = seg000:3a51 open ORNY.HSQ (applies its bank palette).
            self.open_sprite_bank(sprite_bank::ORNY);
            // = seg000:3a57 get_orni_position.
            let (mut x, mut y) = self.get_orni_position();
            // The seg000:3a5a..3a67 stores of the first orni's hover hotspot
            // (orni_hotspot_x/y = x+0xc, y+8, for the seg000:92ab room hover
            // scan) are not ported — nothing reads them yet.
            //
            // = seg000:3a6a..3a79 draw_ornis_loop: one orni per available
            // ornithopter, each stepped down-right by (0x46, 0x0a).
            for _ in 0..count {
                self.draw_orni(x, y);
                x += 0x46;
                y += 0x0a;
            }
        }
        // = seg000:3a41 push 388dh — the pass exits through set_sky_palette so
        // the sky-gradient palette entries ORNY's bank palette overwrote are
        // restored.
        self.set_sky_palette();
    }

    // = seg000:3a95 get_orni_position — the landing-pad screen position for the
    // current location: (149, 57) for location codes (location_and_room high
    // byte) below 0x20 (the sietches), (202, 73) for the rest (the palace /
    // city views).
    fn get_orni_position(&self) -> (i16, i16) {
        if (self.location_and_room >> 8) as u8 >= 0x20 {
            (202, 73)
        } else {
            (149, 57)
        }
    }

    // = seg000:3aa9 draw_orni — composite one orni from the ORNY bank at
    // (x, y), each part clipped to the game area: two fixed parts (sprites 0
    // and 1) and two parts selected by orni_anim_frame — sprites 8..0x16 animate
    // over frames 0..0x0e, sprites 2..7 over frames 0x0f and up.
    fn draw_orni(&mut self, x: i16, y: i16) {
        let frame = self.orni_anim_frame;
        // = seg000:3ac0..3ad2 clamp(frame - 0xf, 0, 5) + 2.
        let part_2_7 = (frame.saturating_sub(0x0f).min(5) + 2) as u16;
        // = seg000:3ade..3ae4 min(frame, 0xe) + 8.
        let part_8_16 = (frame.min(0x0e) + 8) as u16;
        self.with_active_bank_sheet(|g, sheet| {
            // = the seg000:37be copy_game_area_rect_to_clip_rect clip, offset
            // by fb_base_ofs like the draw position (cf.
            // draw_sprite_list_clipped_to_game_area).
            let yoff = g.y_offset as i16;
            let clip = Rect {
                x0: 0,
                y0: yoff,
                x1: 0x140,
                y1: 0x98 + yoff,
            };
            // = seg000:3aa9 sprite 0 at (x, y).
            g.draw_sprite_from_sheet_clipped(sheet, 0, x, y + yoff, clip);
            // = seg000:3aae sprite 1 at (x+6, y+0x1e).
            g.draw_sprite_from_sheet_clipped(sheet, 1, x + 6, y + 0x1e + yoff, clip);
            // = seg000:3aba..3ad4 the frame-selected part at (x+4, y+0x32).
            g.draw_sprite_from_sheet_clipped(sheet, part_2_7, x + 4, y + 0x32 + yoff, clip);
            // = seg000:3ad7..3ae6 the frame-selected part at (x-0x51, y-3).
            g.draw_sprite_from_sheet_clipped(sheet, part_8_16, x - 0x51, y - 3 + yoff, clip);
        });
    }

    // = seg000:380c draw_outdoor_backdrop (reached via loc_037eb from
    // draw_room_scene's seg000:3a02..3a16 dispatch). The outdoor "window/balcony"
    // view drawn behind the SAL for the first room of every location
    // (location_and_room low byte == 1): install the sky-gradient palette, pick a
    // backdrop sprite sheet from room1_backdrop_base (seg001:1972), open it and
    // draw its sprite 0 (loc_0c2f2, = open_spritesheet +
    // draw_active_bank_sprite). draw_room_scene runs
    // this before draw_SAL, so the SAL architecture (e.g. BALCON.HSQ for palace
    // room 0x2001) composites over the desert view; without it the game area
    // behind the SAL's window opening is left blank.
    //
    // Only the special-room branch (location_appearance low byte == 0x80) is
    // modelled — the path the intro/start scenes use. There al starts at 0, stays
    // below room1_backdrop_threshold (seg001:1977), and the resource is simply
    // ROOM1_BACKDROP_BASE[sal_index] (= 0x72 DP1 for the palace). The other two
    // branches are not ported: seg000:382d fades in a still keyed on the
    // location's own game_time field, and seg000:384a runs get_map_position /
    // map_func to pick a DN20..DN38 / VG.. desert tile from where the player
    // stands on the map. Both need the per-location structs and the map model the
    // port does not have yet.
    fn draw_outdoor_backdrop(&mut self, location_appearance: u16, sal_index: usize) {
        // = seg000:380c mov [sky_skydn_selector],1; 3811 call set_sky_palette —
        // the backdrop sits under the sky gradient, so install it first.
        self.sky_skydn_selector = 1;
        self.set_sky_palette();

        // = seg000:3827 cmp al,80h — al is location_appearance's low byte. Only
        // the special-room path (== 0x80) is ported.
        if location_appearance & 0xff != 0x80 {
            return;
        }

        // = seg000:3829 mov al,0 -> 3834 cmp al,room1_backdrop_threshold[bx]: al
        // (0) is always below the threshold (>= 4), so the jnb to the map branch is
        // not taken and 3839 add al,room1_backdrop_base[bx] yields al =
        // ROOM1_BACKDROP_BASE[sal_index]. The 0x7f (VILG) entry's extra
        // randomisation at seg000:383f reads a
        // per-location field the port lacks; for the palace (0x72) it is not hit.
        let resource = ROOM1_BACKDROP_BASE[sal_index] as i16;

        // = seg000:3847 jmp loc_0c2f2 — open the backdrop bank (applies its
        // palette) and draw sprite 0 at (0,0).
        self.open_resource_and_draw_sprite0(resource);
    }

    // = seg000:3b59 draw_SAL (inner work). Open the sprite-sheet resource
    // (applying its palette, mirroring open_spritesheet ->
    // apply_sprite_sheet_palette), read one room sub-chunk from a .SAL room
    // sheet, and blit its sprites / polygons / lines into the active
    // framebuffer at the current fb_base_ofs (state.y_offset), landing in the
    // game-area rect (rows 24..175). The recursive sprite/polygon/line decode
    // lives in RoomSheet/RoomRenderer.
    fn draw_sal_room(&mut self, sal_name: &str, room: usize, sprite_sheet_name: &str) {
        let sal = self.dat_file.read(sal_name).expect("failed to read SAL");
        let room_sheet = RoomSheet::new(&sal).expect("failed to parse SAL");
        let Some(room) = room_sheet.get_room(room) else {
            return;
        };

        let sheet_data = self
            .dat_file
            .read(sprite_sheet_name)
            .expect("failed to read sprite sheet");
        let sprite_sheet = SpriteSheet::from_slice(&sheet_data).expect("failed to parse sheet");
        // = apply_sprite_sheet_palette: the sprite sheet carries the room's
        // palette; it overlays the previous stage's palette with exactly the
        // entries the room draws with.
        sprite_sheet
            .apply_palette_update(&mut self.palette)
            .expect("failed to apply palette");

        // = sal_read_position_markers (seg000:3d83): resolve which person, if
        // any, stands in each of the room's standing slots from the current
        // persons_in_room set.
        let markers = sal_position_markers(
            room.position_marker_count(),
            self.persons_in_room,
            self.persons_travelling_with,
            self.person_marker_base,
        );

        // = sal_draw_character (seg000:3d2f) opens PERS.HSQ (RES_PERS_HSQ) only
        // when a person is actually present. open_spritesheet applies the
        // sheet's palette update; DOS restores the room sheet's palette after
        // each character, so re-apply the room palette last to keep its entries
        // winning on any overlap.
        let character_sheet = if markers.iter().any(|&m| m != -1) {
            let pers = self
                .dat_file
                .read("PERS.HSQ")
                .expect("failed to read PERS.HSQ");
            let sheet = SpriteSheet::from_slice(&pers).expect("failed to parse PERS.HSQ");
            sheet
                .apply_palette_update(&mut self.palette)
                .expect("failed to apply PERS palette");
            sprite_sheet
                .apply_palette_update(&mut self.palette)
                .expect("failed to re-apply room palette");
            Some(sheet)
        } else {
            None
        };

        let mut renderer = RoomRenderer::new();
        renderer.set_y_offset(self.y_offset as i16);
        renderer.set_room(room.clone());
        renderer.set_sprite_sheet(sprite_sheet);
        renderer.set_position_markers(markers);
        if let Some(character_sheet) = character_sheet {
            renderer.set_character_sheet(character_sheet);
        }
        // = sal_draw_character_entry's `test room_render_flags, 81h` gate
        // (seg000:3d12): bit 0 or 7 set suppresses the standing person sprites.
        // The dialogue-zoom re-render (zoom_room_to_dialogue_speaker) sets bit 7
        // so the close-up backdrop behind the talking head carries no tiny figure.
        let draw_characters = (self.room_render_flags & 0x81) == 0;
        let options = DrawOptions {
            draw_characters,
            ..DrawOptions::default()
        };
        renderer
            .draw(&options, &mut self.framebuffer)
            .expect("failed to draw room");

        // = loc_03ae9 — clear character_x_table/character_y_table (seg001:47f8)
        // to 0xffff (absent), then = sal_draw_character (seg000:3d2f) record each
        // drawn person's (x, y) anchor at [id*4], so person_hit_test_at_cursor can hit-test the
        // cursor against the on-screen people. Skipped along with the character
        // draw when suppressed, so the anchors recorded by the prior normal draw
        // (which the zoom already read) survive the re-render untouched.
        if draw_characters {
            self.character_screen_pos = [(0xffff, 0xffff); 0x17];
            for (id, x, y) in renderer.character_screen_positions() {
                if (0..0x17).contains(&id) {
                    self.character_screen_pos[id as usize] = (x as u16, y as u16);
                }
            }
        }
    }

    // = seg000:388d set_sky_palette — pick the sky sub-palette for the current
    // game_time and apply it. The sub-palette comes from
    // get_sky_palette_id_from_game_time_in_bl; the resource (SKY.HSQ vs
    // SKYDN.HSQ) and byte range come from sky_skydn_selector. When a sky
    // cross-fade is already running (sky_fade_countdown != 0) the new
    // sub-palette is written as the fade *target* and the in-flight fade is
    // re-aimed at it (loc_039b9); otherwise it is written straight into the
    // live palette (loc_0398c). draw_sky and the in-game balcony/window scenes
    // (draw_room_scene at seg000:39fb) call it.
    pub fn set_sky_palette(&mut self) {
        // = seg000:388d mov [sky_fade_active], 1 — a sky scene is now on
        // screen, so loc_038e1's time-period refresh may cross-fade it later.
        self.sky_fade_active = true;
        // = seg000:3892 call get_sky_palette_id_from_game_time_in_bl (bl).
        let sub = sky_palette_id_from_game_time(self.game_time);
        // = seg000:38a7/38ad ax = 0x28 + sky_skydn_selector selects the
        // resource; loc_0398c (live) and loc_039b9 (fade target) share the
        // byte offsets/counts: sky_skydn=0 → 80 colours @ entry 128, else →
        // 151 colours @ entry 73. The intro path keeps sky_skydn_selector = 1
        // (remove_all_frame_tasks default), so intro2 uses SKYDN.HSQ's
        // 151-colour layout — applying SKY.HSQ's 80@128 layout to a SKYDN
        // sub-palette would read the wrong 80 of its 151 colours and write
        // them at the wrong palette indices.
        let (resource, dest_start, count) = if self.sky_skydn_selector != 0 {
            // = seg000:3971 ax = 0x28 + sky_skydn_selector → 0x29 SKYDN.HSQ.
            ("SKYDN.HSQ", 73, 151)
        } else {
            ("SKY.HSQ", 128, 80)
        };
        // = seg000:3895 cmp [sky_fade_countdown], 0; jz loc_038ad.
        if self.sky_fade_countdown != 0 {
            // = seg000:389c cmp [current_sky_palette], bl; jz ret — a fade is
            // already heading to this sub-palette, leave it running.
            if self.current_sky_palette as usize == sub {
                return;
            }
            // = seg000:38a2 loc_038a2: re-aim the in-flight fade. Reset the
            // step counter, then open_sky_or_skydn_palette_al_sub_bl +
            // loc_039b9 write the new sub-palette into palette_fade_target; the
            // fade task already installed by the running fade keeps stepping
            // the live palette toward it.
            self.sky_fade_countdown = 0x30;
            // = seg000:38a7 open_sky_or_skydn_palette_al_sub_bl + 38aa jmp
            // loc_039b9 (primary span).
            self.load_sky_palette_to_fade_target(resource, sub, 0, count, dest_start);
            // = seg000:39d2/39e5 the secondary 240..255 span when [227dh]==0.
            if self.data_0227d == 0 {
                self.load_sky_palette_to_fade_target(resource, sub, count, 16, 240);
            }
            return;
        }
        // = seg000:38ad loc_038ad: no fade in progress, write the sub-palette
        // straight into the LIVE palette (open_sky_or_skydn_palette_al_sub_bl +
        // loc_0398c).
        self.open_sky_palette(resource, sub, 0, count, dest_start);
        // = seg000:39ae loc_039ae — the secondary 240..255 span when [227dh]==0
        // (in-game). data_0227d == 1 throughout the intro, so this is normally
        // a no-op there; modelled to match DOS for the in-game path.
        if self.data_0227d == 0 {
            self.open_sky_palette(resource, sub, count, 16, 240);
        }
    }

    // = seg000:38b4 draw_sky — tile SKY.HSQ as a 4-row × 8-column grid, one
    // sprite id per row (rows use sprite 0..3) at stride (dx=0x28, bp=0x14).
    // DOS first calls set_sky_palette (seg000:388d) to install the sky-gradient
    // palette entries, then loads SKY.HSQ and blits its row sprites. The
    // intro2 scene (intro2_scene_sky) and the in-game balcony/window scenes
    // (loc_03a1d / seg000:43d3) call it.
    pub(crate) fn draw_sky(&mut self) {
        // = seg000:38b4 call set_sky_palette.
        self.set_sky_palette();
        // = seg000:38b7 ax=0x28 (SKY); open_spritesheet — load the sprites.
        self.open_sprite_bank(sprite_bank::SKY);
        // = seg000:38bd ax=0; bp=0x14 (y stride); bx=0 (y); cx=4 (row count).
        let yoff = self.y_offset as i16;
        self.with_active_bank_sheet(|s, sheet| {
            for row in 0..4 {
                let y = yoff + (row as i16) * 0x14;
                let mut x: i16 = 0;
                while x < 0x140 {
                    if let Some(sprite) = sheet.get_sprite(row) {
                        let fb = s.active_fb_mut();
                        let _ = blit::Blitter::new(sprite.data(), fb)
                            .at(x, y)
                            .size(sprite.width(), sprite.height())
                            .pal_offset(sprite.pal_offset())
                            .rle(sprite.rle())
                            .draw();
                    }
                    x += 0x28;
                }
            }
        });
    }

    // = seg000:38e1 loc_038e1 — the time-period sky refresh
    // run_events_for_current_time_period calls (seg000:1b43). When the sky is
    // live (sky_fade_active) and the time-of-day has advanced the sky sub-palette
    // (get_sky_palette_id_from_game_time_in_bl differs from the one currently
    // showing), arm a cross-fade toward the new sub-palette; otherwise do nothing.
    pub(crate) fn loc_038e1_sky_refresh(&mut self) {
        // = seg000:38e1 cmp [sky_fade_active], 0; jz loc_038e0 — only refresh
        // while a sky scene is on screen.
        if !self.sky_fade_active {
            return;
        }
        // = seg000:38e8 call get_sky_palette_id_from_game_time_in_bl — the sub-
        // palette the current game_time maps to (bl).
        let sub = sky_palette_id_from_game_time(self.game_time);
        // = seg000:38eb cmp [current_sky_palette], bl; jz loc_038e0 — already
        // displaying it, nothing to do.
        if self.current_sky_palette as usize == sub {
            return;
        }
        // = seg000:38f1 loc_038f1 — fall through and arm the cross-fade.
        self.arm_sky_palette_fade(sub);
    }

    // = seg000:38f1 loc_038f1 — arm the sky-palette cross-fade. Load SKY/SKYDN
    // sub-palette `sub` into palette_fade_target and record it as
    // current_sky_palette (open_sky_or_skydn_palette_al_sub_bl writes
    // [46d6h] = bl at seg000:3982; the port folds that write into
    // load_sky_palette_to_fade_target), then set sky_fade_countdown = 0x40 and
    // install the loc_03916 frame task if not already armed. The byte range is
    // selected by sky_skydn_selector (0 → 80 entries @128, else 151 entries @73)
    // and extended with entries 240..255 when suppress_sky_240_255
    // (data_0227d) == 0. Shared by the intro2 night→day fade (the loc_038f1
    // entry, reached by jumping past loc_038e1's gate) and loc_038e1's
    // time-period sky refresh.
    pub(crate) fn arm_sky_palette_fade(&mut self, sub: usize) {
        // = seg000:39b9 loc_039b9 — fade-target write, mirroring stage_29_init's
        // load_sky_palette_to_fade_target call.
        let (resource, dest_start, count) = if self.sky_skydn_selector != 0 {
            ("SKYDN.HSQ", 73, 151)
        } else {
            ("SKY.HSQ", 128, 80)
        };
        self.load_sky_palette_to_fade_target(resource, sub, 0, count, dest_start);
        // = seg000:39d2/39e5 the secondary 240..255 span when [227dh]==0.
        if self.data_0227d == 0 {
            self.load_sky_palette_to_fade_target(resource, sub, count, 16, 240);
        }
        // = seg000:38f7 mov al,0x40; xchg al,[sky_fade_countdown]; or al,al;
        // jnz loc_038e0 — only install the frame task on the first arm; a
        // re-arm just resets the countdown.
        let prev = self.sky_fade_countdown;
        self.sky_fade_countdown = 0x40;
        if prev == 0 {
            // = seg000:3901 loc_03901: si = loc_03916; bp = 0x10; jmp
            // add_frame_task — one fade step every 0x10 ticks.
            self.add_frame_task(0x10, crate::TaskId::SkyFade);
        }
    }

    // = seg000:3916 loc_03916 — one tick of the sky palette fade task. Steps the
    // live palette's sky range toward palette_to_transition_from, decrements the
    // step counter, and self-removes when it reaches zero (or when disarmed).
    pub(crate) fn tick_sky_fade(&mut self) {
        // = seg000:3916 cmp [46dfh],0; jz loc_03950 — disarmed → stop.
        if !self.sky_fade_active {
            self.sky_fade_countdown = 0;
            self.remove_frame_task(crate::TaskId::SkyFade);
            return;
        }
        // = seg000:391d loc_0391d: vga_fade_step(al=[46d7h]) over the [22e3h]
        // span (entries 73..223 for the intro's [22e3h]=1), then dec [46d7h].
        let countdown = self.sky_fade_countdown;
        self.sky_palette_fade_step(countdown);
        self.sky_fade_countdown -= 1;
        // The DOS step writes the VGA DAC directly. The HNM task presents every
        // decoded frame, but the fade outlives the clip, so present here too.
        self.send_frame_to_display();
        // = seg000:394e/loc_03950: counter exhausted → remove_frame_task(3916h).
        if self.sky_fade_countdown == 0 {
            self.remove_frame_task(crate::TaskId::SkyFade);
        }
    }

    // = seg000:391d picks the span from [22e3h]: ==0 → bx=0x180/cx=0xf0 (entries
    // 128..207), else → bx=0xdb/cx=0x1c5 (entries 73..223). When [227dh]==0 it then
    // fades a second span, entries 240..255 (bx=0x2d0/cx=0x30); the intro keeps
    // [227dh]=1 so that span is normally skipped.
    pub(crate) fn sky_palette_fade_step(self: &mut GameState, countdown: u8) {
        let steps = if countdown == 0 { 1 } else { countdown as i16 };
        let primary = if self.sky_skydn_selector != 0 {
            73..224
        } else {
            128..208
        };
        let secondary = if self.data_0227d == 0 { 240..256 } else { 0..0 };
        for i in primary.chain(secondary) {
            let current = self.palette.get(i);
            let target = self.palette_fade_target.get(i);
            self.palette.set(i, current.lerp(target, steps));
            self.screen_pal.set(i, current.lerp(target, steps));
        }
    }
}

// = seg000:395c get_sky_palette_id_from_game_time_in_bl. The DOS routine
// indexes a 16-byte hour-of-day table at byte_21730 (seg001:2280) by the low
// nibble of `game_time` and adds the (low-byte >> 2) & 0x1c "stride" so each
// 16-tick day spans the table once and each whole-day rollover shifts the
// gradient by 4 sub-palettes. At intro2 entry game_time == 0 → table[0] = 8;
// the in-game clock advances it as the day progresses.
fn sky_palette_id_from_game_time(game_time: u16) -> usize {
    // = seg001:2280 byte_21730 db 8,8,9,9,9,9,9,9,9,9,9,Ah,Ah,Bh,Bh,Bh.
    const SKY_TABLE: [u8; 16] = [8, 8, 9, 9, 9, 9, 9, 9, 9, 9, 9, 10, 10, 11, 11, 11];
    let al = (game_time & 0xff) as u8;
    let table_index = (al & 0x0f) as usize;
    let stride = (al >> 2) & 0x1c;
    (SKY_TABLE[table_index] + stride) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calc_sal_index_thresholds() {
        assert_eq!(calc_sal_index(0x1f), 0); // < 0x20 -> SIET
        assert_eq!(calc_sal_index(0x20), 1); // == 0x20 -> PALACE
        assert_eq!(calc_sal_index(0x27), 2); // 0x21..0x27 -> VILG
        assert_eq!(calc_sal_index(0x28), 3); // 0x28..0x2f -> HARK
        assert_eq!(calc_sal_index(0x30), 4); // >= 0x30 -> HARK (clamped)
    }

    #[test]
    fn throne_room_has_no_compass_exits() {
        // = palace_rooms[9] at location_and_room=0x200a: every exit is a
        //   destination-room number (0x01..0x7F), so rebuild_and_draw_room_nav_panel
        //   hides every compass HUD arrow (the throne room reaches rooms 9, 5,
        //   and 4 to the N/E/S, but only via in-scene clicks, not the arrows).
        let dh = 0x200ausize >> 8;
        let dl = 0x200ausize & 0xff;
        let record = &SCENE_RECORDS[SCENE_DISPATCH[dh] as usize + (dl - 1)];
        assert_eq!(record.exits, [0x09, 0x05, 0x04, 0x00]);
        assert!(record.exits.iter().all(|&n| n < 0xfb));
    }

    #[test]
    fn palace_room_1_has_down_compass_exit() {
        // = palace_rooms[0] at location_and_room=0x2001: exits[2] = 0xfd is the
        //   only entry in the 0xfb..0xff compass-arrow range, so the DOWN arrow
        //   (exits[2]) is the only one the rebuild leaves visible.
        let dh = 0x2001usize >> 8;
        let dl = 0x2001usize & 0xff;
        let record = &SCENE_RECORDS[SCENE_DISPATCH[dh] as usize + (dl - 1)];
        assert_eq!(record.exits, [0x02, 0x00, 0xfd, 0x00]);
        let visible: Vec<bool> = record.exits.iter().map(|&n| n >= 0xfb).collect();
        assert_eq!(visible, vec![false, false, true, false]);
    }

    #[test]
    fn ui_click_move_room_targets() {
        // = palace entry room (location_and_room = 0x2001), exits
        //   [0x02, 0x00, 0xfd, 0x00]. compass_move_target is the render-free
        //   core of ui_click_move_room (the method itself draws, which needs a
        //   real dat_file).
        let exits = [0x02u8, 0x00, 0xfd, 0x00];

        // UP (exit 0x02): destination room — keep dh, swap the room low byte.
        assert_eq!(compass_move_target(0x2001, exits, 0), Some(0x2002));
        // RIGHT (exit 0x00): no exit -> no move.
        assert_eq!(compass_move_target(0x2001, exits, 1), None);
        // DOWN (exit 0xfd): special-exit code (0x80..0xFF) -> not ported, no move.
        assert_eq!(compass_move_target(0x2001, exits, 2), None);
        // LEFT (exit 0x00): no exit -> no move.
        assert_eq!(compass_move_target(0x2001, exits, 3), None);
    }
    // = the seg000:3a24..3a7b orni pass: with available ornithopters on a
    // first-room sky scene, draw_location_room composites parked ornis from
    // ORNY.HSQ onto the landing pad; with none the scene is unchanged. Renders
    // palace room 1 (0x2001) — the balcony view the intro parks the player's
    // orni in front of — and checks the pass actually changes pixels.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn parked_ornis_draw_on_the_room1_landing_pad() {
        use std::sync::mpsc;

        use crate::dat_file::DatFile;

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

        // No ornis available: the pass is a no-op.
        game.available_equipment.ornithopters = 0;
        game.draw_location_room(game.location_and_room, game.location_appearance);
        let without = game.active_fb_mut().pixels().to_vec();

        // One orni available: the landing pad gets a parked orni.
        game.available_equipment.ornithopters = 1;
        game.draw_location_room(game.location_and_room, game.location_appearance);
        let with = game.active_fb_mut().pixels().to_vec();

        let differing = without.iter().zip(&with).filter(|(a, b)| a != b).count();
        eprintln!("orni pass changed {differing} pixels");
        assert!(
            differing > 100,
            "expected the orni sprites to change the scene (changed {differing} pixels)"
        );

        if std::env::var_os("WRITE_PNG").is_some() {
            let fb = game.active_fb_mut();
            let fb = fb.clone();
            fb.write_png_scaled(&game.palette, "/tmp/orni_room1.png")
                .expect("write png");
            eprintln!("wrote /tmp/orni_room1.png");
        }
    }
}
