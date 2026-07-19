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
// VILG -> 0x7f, HARK -> DF1 (0x76) / DH0 (0x84). Standing in the desert on the
// location's longitude adds the fine latitude (the walked distance) to the
// base, so the view shrinks into the distance step by step (DP1 -> DP0 -> DP2
// -> DP3 for the palace).
const ROOM1_BACKDROP_BASE: [u8; 5] = [0x3c, 0x72, 0x7f, 0x76, 0x84];

// = seg001:1977 room1_backdrop_threshold — how far (in fine-latitude steps)
// each location type stays in view; past it draw_outdoor_backdrop falls back
// to the position-hashed desert terrain tile (seg000:3834 jnb loc_0384a).
const ROOM1_BACKDROP_THRESHOLD: [u8; 5] = [5, 4, 5, 4, 4];

// = chani struct PalaceRoom (seg001:1225 palace_rooms et al). The first byte
// selects the room's SAL sub-chunk and sprite sheet (see draw_SAL); the four
// `exits` bytes are the scene's per-direction exits resolved by
// ui_click_move_room (seg000:3f27) and rebuild_and_draw_room_nav_panel
// (seg000:2ffb).
#[derive(Clone, Copy)]
pub(crate) struct SceneRecord {
    pub(crate) background: u8,
    // One byte per compass direction; index i maps to the bottom-right HUD
    // compass arrow at i = 0..3 (UP / RIGHT / DOWN / LEFT, i.e. N/E/S/W). The
    // byte at index i is the exit reachable in that direction:
    //   0x00:        no exit in this direction
    //   0x01..0x7F:  destination room number — ui_click_move_room stores it as
    //                the new `location_and_room` low byte (seg000:3faa).
    //   0x80..0xFF:  special exit: walk out of the location into the desert
    //                (seg000:3fd2), stepped by the desert_step_deltas word at
    //                index -exit. The 0xFB..0xFF subrange is what
    //                rebuild_and_draw_room_nav_panel renders as a visible
    //                HUD arrow; the rest are in-scene/scripted exits.
    // Runtime-mutable: the game-phase callbacks (seg000:1027/102f/10a4/1188)
    // clear an exit's bit 7 to unlock scripted palace doors, and phase 4
    // decrements palace_rooms[1].background — GameState owns the live copy
    // (scene_records); this const table is the seg001 static initializer.
    pub(crate) exits: [u8; 4],
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
pub(crate) const SCENE_RECORDS: [SceneRecord; 83] = [
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

// = seg001:1454 desert_step_deltas — per-direction desert step words, indexed
// by bp = the compass direction (1..4, UP/RIGHT/DOWN/LEFT) or the negated
// special-exit byte (0xff..0xfb -> 1..5). Each word is a packed step: the low
// byte is a signed longitude delta, the high byte a signed fine-latitude
// delta, applied by desert_apply_step_delta. Index 5 (exit 0xfb) is "no
// movement" — step onto the map in place. Index 0 is unused (the DOS word
// there, 0x0209, is unrelated data).
const DESERT_STEP_DELTAS: [u16; 6] = [0x0209, 0xff00, 0x0001, 0x0100, 0x00ff, 0x0000];

// = seg000:b5cf desert_apply_step_delta — apply a desert_step_deltas word to
// the desert position: `x` is the longitude (0..0xffff around the planet, the
// DOS dx), `latfine` packs the signed latitude row in its low byte (the DOS
// bl, -97..97) and the fine sub-row position in its high byte (bh, 0..255
// between latitude rows). The step's high byte moves the fine latitude,
// carrying into the row clamped to row < 0x62 going one way and row > -0x62
// going the other (a blocked step is undone); the low byte (sign-extended)
// is added to the longitude.
fn desert_apply_step_delta(step: u16, x: u16, latfine: u16) -> (u16, u16) {
    let ah = (step >> 8) as u8;
    let mut bl = (latfine & 0xff) as u8;
    let mut bh = (latfine >> 8) as u8;
    if (ah as i8) > 0 {
        // = seg000:b5d5 add bh,ah; jnb loc_0b5f5 — no carry: done.
        let (sum, carry) = bh.overflowing_add(ah);
        bh = sum;
        if carry {
            // = seg000:b5d9 inc bl; cmp bl,62h; jl — clamp at row 0x62,
            // undoing the whole step when it would cross.
            bl = bl.wrapping_add(1);
            if bl as i8 >= 0x62 {
                bl = bl.wrapping_sub(1);
                bh = bh.wrapping_sub(ah);
            }
        }
    } else if (ah as i8) < 0 {
        // = seg000:b5e6 add bh,ah; jb loc_0b5f5 — carry set: done.
        let (sum, carry) = bh.overflowing_add(ah);
        bh = sum;
        if !carry {
            // = seg000:b5ea dec bl; cmp bl,9eh; jg — clamp at row -0x62.
            bl = bl.wrapping_sub(1);
            if bl as i8 <= -0x62 {
                bl = bl.wrapping_add(1);
                bh = bh.wrapping_sub(ah);
            }
        }
    }
    // = seg000:b5f5 cbw; add dx,ax — the sign-extended longitude delta.
    let x = x.wrapping_add((step as u8) as i8 as u16);
    (x, ((bh as u16) << 8) | bl as u16)
}

// = seg000:5e4f calc_SAL_index. Maps a location's `apparence` byte to a SAL
// index via ascending thresholds (0=SIET 1=PALACE 2=VILG 3/4=HARK). Also the
// map-marker sprite tier (calc_location_marker_sprite falls into it).
pub(crate) fn calc_sal_index(apparence: u8) -> usize {
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
    //   into the desert; ui_click_move_room takes that branch before calling
    //   here, so this guard only backstops direct callers (tests).
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
        if idx >= self.scene_records.len() {
            return None;
        }
        Some(self.scene_records[idx].exits)
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
    // Three paths: an in-room destination-room exit commits a plain room move;
    // a special exit (0x80..0xFF) walks out of the location into the desert
    // (loc_03fd2); and while already in the desert (location_appearance low
    // byte != 0x80) every compass click steps the desert position. The two
    // desert paths meet in the desert-position dispatch (loc_03ff5 ->
    // desert_position_dispatch).
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
            // = seg000:3f59..3f60 cap desert_walk_counter at 20.
            if self.desert_walk_counter < 20 {
                self.desert_walk_counter += 1;
            }
            // = seg000:3f64 jmp loc_03ff5 — the desert-position dispatch, with
            //   bp = the compass direction and the current desert position in
            //   (location_and_room, location_appearance).
            self.desert_position_dispatch(
                direction,
                self.location_and_room,
                self.location_appearance,
            );
            return;
        }

        let Some(exits) = self.current_scene_exits() else {
            return;
        };
        let exit = exits[direction - 1];
        // = seg000:3f6c or dl,dl; 3f6e jz loc_03f14 — no exit in this
        //   direction (also re-checked by compass_move_target below).
        if exit == 0 {
            return;
        }
        // = seg000:3f70 js loc_03fd2 — a special exit (0x80..0xFF): walk out
        //   of the location into the desert.
        if exit & 0x80 != 0 {
            // = seg000:3fd2 mov [data_000e7],0 — a write-only desert-redraw
            //   counter (no DOS reader); not modelled.
            // = seg000:3fd7/3fd9 xor dh,dh; neg dl — bp = -exit, the
            //   desert_step_deltas index (0xff..0xfb -> 1..5).
            let bp = exit.wrapping_neg() as usize;
            // = seg000:3fdd/3fdf xor si,si; xchg si,[current_location_ptr] —
            //   detach the current location record (the desert has no verb
            //   list); the record itself stays in last_location_ptr. (DOS
            //   writes the pointer 0; the port's index form uses the 0xffff
            //   "no location" sentinel.)
            let loc_index = self.current_location_index as usize;
            self.current_location_index = 0xffff;
            let Some(location) = self.locations.get(loc_index) else {
                return;
            };
            // = seg000:3fe3/3fe6/3fe9 seed the desert position from the
            //   location's map cell: dx = map_x, bl = map_y, bh (the fine
            //   latitude) = 0.
            let x = location.map_x as u16;
            let latfine = (location.map_y as u16) & 0xff;
            // = seg000:3feb/3ff0 current_scene / data_00009 = 0xff — no room
            //   scene; draw_room_scene now takes its desert branch.
            self.data_00008 = 0xff;
            self.data_00009 = 0xff;
            // Exit bytes 0x80..0xFA would index past desert_step_deltas (DOS
            // reads unrelated seg001 data there); only the compass-arrow
            // exits 0xFB..0xFF are clickable on the HUD.
            if bp >= DESERT_STEP_DELTAS.len() {
                return;
            }
            self.desert_position_dispatch(bp, x, latfine);
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

        // = seg000:3f84 mov si,[current_location_ptr] — the current location
        //   record.
        let loc_index = self.current_location_index as usize;
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

        // = seg000:3fcf jmp loc_04057 — bx (the location appearance) is
        //   unchanged on the in-room path.
        self.commit_room_move(new_room, self.location_appearance);
    }

    // = seg000:3ff5 loc_03ff5 — the desert-position dispatch shared by the
    // walk-out special exit and the in-desert compass moves: apply the
    // direction's step to the desert position (dx = longitude, latfine =
    // (fine << 8) | latitude row) and commit it; a step that lands exactly on
    // a latitude row (fine == 0) first resolves whether a location occupies
    // that map cell and enters its room 1 instead.
    fn desert_position_dispatch(&mut self, direction: usize, x: u16, latfine: u16) {
        // = seg000:3ff5/3ff7 ax = desert_step_deltas[bp]; 3ffb call
        //   desert_apply_step_delta.
        let step = DESERT_STEP_DELTAS[direction];
        let (x, latfine) = desert_apply_step_delta(step, x, latfine);
        // = seg000:3ffe or bh,bh; jnz loc_04057 — between latitude rows: just
        //   commit the desert position.
        if latfine & 0xff00 != 0 {
            self.commit_room_move(x, latfine);
            return;
        }
        // = seg000:4002 loc_04002 — on a latitude row: check for a location.
        self.desert_check_arrival(x, latfine);
    }

    // = seg000:4002 loc_04002 — the desert arrival check: a location is here
    // when the map byte has bit 0x40, find_location_by_map_offset finds its
    // record and the walked longitude equals the location's snapped map_x;
    // then arrive_at_location enters its room 1. Otherwise commit the position
    // as a plain desert view. Reached from the desert-walk step above and from
    // the travel arrival (travel_pump, seg000:4ff8, with the destination's
    // (map_x, map_y)).
    pub(crate) fn desert_check_arrival(&mut self, x: u16, latfine: u16) {
        // = seg000:4002..4005 bx = the sign-extended latitude row.
        let lat = (latfine as u8) as i8 as i16;
        // = seg000:4007 call read_map_byte_at_dx_bl (then 400a xor bh,bh —
        //   bh is already 0 on this path).
        let offset = self.map_position_to_offset(x, lat).0;
        let map_byte = self.map[offset];
        // = seg000:400c test al,40h; jz loc_04057 — no location at this cell.
        if map_byte & 0x40 == 0 {
            self.commit_room_move(x, latfine);
            return;
        }
        // = seg000:4010 call find_location_by_map_offset; jnz loc_04057.
        let Some(loc_index) = self.find_location_by_map_offset(offset) else {
            self.commit_room_move(x, latfine);
            return;
        };
        // = seg000:4015 cmp dx,[si+2]; jnz loc_04057 — the walked longitude
        //   must equal the location's snapped map_x exactly.
        if self.locations[loc_index].map_x as u16 != x {
            self.commit_room_move(x, latfine);
            return;
        }
        // = seg000:401f arrive_at_location, falling through into loc_04057.
        let (new_room, new_appearance) = self.arrive_at_location(loc_index);
        self.commit_room_move(new_room, new_appearance);
    }

    // = seg000:401f arrive_at_location — walk-in arrival at a location from
    // the desert. Returns the (location_and_room, location_appearance) pair
    // for its entry room, which DOS falls through into loc_04057 to commit.
    fn arrive_at_location(&mut self, loc_index: usize) -> (u16, u16) {
        // = seg000:401f mov [data_04735],0 — reset the desert step counter
        //   (and disarm the auto-action dispatch its high bit requests).
        self.data_04735 = 0;
        // = seg000:4024/4028 current_location_ptr / last_location_ptr = si — the
        //   arrived-at location is the current location record again.
        self.current_location_index = loc_index as u16;
        self.last_location_index = loc_index;
        // = seg000:402e call location_related_to_dying_if_arriving_at_
        //   fortress_0503c — the occupation check (arms the night attack when
        //   the location is enemy-held). TODO: needs the troop system.
        // = seg000:4031/4037 data_0009a/data_00098 = 0 — the massive-attack
        //   troop accumulators (seg000:7326); not modelled, no ported reader.
        // = seg000:403d call location_mark_discovered.
        self.location_mark_discovered(loc_index);
        // = seg000:4040 call location_entry_room_dx_bx — dx = the location's
        //   entry room 1, bx = the in-room appearance for its 1-based slot.
        let new_room = ((self.locations[loc_index].appearance as u16) << 8) | 1;
        let new_appearance = ((loc_index as u16 + 1) << 8) | 0x80;
        // = seg000:4043/4047 current_scene = dh; data_00009 = bh.
        self.data_00008 = (new_room >> 8) as u8;
        self.data_00009 = (new_appearance >> 8) as u8;
        // = seg000:404b cmp dh,20h; jb loc_04054; or [si+0ah],10h — arriving
        //   at a non-sietch (city/palace) marks it visited immediately.
        if new_room >> 8 >= 0x20 {
            self.locations[loc_index].status |= 0x10;
        }
        // = seg000:4054 call iterate_over_allied_NPCs_and_locations — the
        //   companion/NPC room shuffle on arrival. TODO: not ported.
        (new_room, new_appearance)
    }

    // = seg000:425b location_mark_discovered — first arrival at an
    // undiscovered location (status bit 0x80 set): clear the bit, zero
    // discoverable_at_phase, count sietches in discovered_sietch_count, and
    // advance the game phase when the location is Tuono-Harg.
    pub(crate) fn location_mark_discovered(&mut self, loc_index: usize) {
        let location = &mut self.locations[loc_index];
        // = seg000:425b test [di+0ah],80h; jz ret.
        if location.status & 0x80 == 0 {
            return;
        }
        // = seg000:4261/4265 clear the bit, discoverable_at_phase = 0.
        location.status &= 0x7f;
        location.discoverable_at_phase = 0;
        // = seg000:4269 cmp [di+8],20h; jnb ret — only sietches count.
        if location.appearance >= 0x20 {
            return;
        }
        let (first_name, last_name) = (location.first_name, location.last_name);
        // = seg000:426f inc [discovered_sietch_count].
        self.discovered_sietch_count += 1;
        // = seg000:4273 cmp word [di],603h — first_name 3 / last_name 6 is
        //   Tuono-Harg; discovering it advances the story to phase 0x10.
        if first_name == 3 && last_name == 6 {
            // = seg000:427e call set_game_phase_and_trigger_callbacks(0x10).
            self.set_game_phase_and_trigger_callbacks(0x10);
        }
    }

    // = seg000:4057 loc_04057 — commit a move: move companion NPCs to the
    // destination, record location_and_room / location_appearance, rotate
    // current_room into previous_room, then redraw. Reached from the in-room
    // move (seg000:3fcf), the desert-position dispatch, and the walk-in
    // arrival fall-through.
    fn commit_room_move(&mut self, new_room: u16, new_appearance: u16) {
        // = seg000:4057 call move_all_NPCs_whose_bit_6_of_flags_is_set —
        //   companions in the room being left follow the player to the
        //   destination. Runs before location_and_room is updated: the scan
        //   matches against the room being left.
        self.move_all_npcs_whose_bit_6_of_flags_is_set(new_room, new_appearance);
        // = seg000:405a mov [location_and_room],dx — commit the destination
        //   (re-recorded by draw_location_room when the redraw runs below, but
        //   the no-redraw return paths still need it committed).
        self.location_and_room = new_room;
        // = seg000:405e..4064 rotate current_room into previous_room.
        self.previous_room = std::mem::replace(&mut self.current_room, (new_room & 0xff) as u8);
        // = seg000:4067 mov [location_appearance],bx — unchanged on the
        //   in-room path; the desert paths commit a new appearance (the
        //   (fine << 8) | latitude position, or the arrival's room form).
        self.location_appearance = new_appearance;
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
        // = seg000:407b jmp loc_02dbf — re-enter draw_room_game_screen at its
        //   scene-reload entry (NOT the seg000:2db1 top: the nav-panel
        //   template is not re-installed on a move); its draw_room_scene
        //   renders the destination (room or desert view) from the
        //   just-committed globals.
        self.draw_room_game_screen_scene_reload();
        self.send_frame_to_display();
    }

    // = seg000:37eb loc_037eb — the outdoor view composite, reached from
    // draw_room_scene's desert branch (loc_037dc, current_scene == 0xff) and
    // its first-room dispatch (loc_03a13): the sky + backdrop sprite, then the
    // neighbouring-location horizon pass. The port calls it only from the
    // desert branch; draw_location_room keeps its direct draw_outdoor_backdrop
    // call for the first-room case.
    pub(crate) fn draw_desert_view(&mut self) {
        // = seg000:37c1 call clear_game_area — done in draw_room_scene's
        // prologue in DOS (loc_037b5, shared with the SAL path where the
        // port's draw_location_room clears); the desert branch needs it too so
        // a backdrop sprite narrower than the game area leaves no leftovers.
        self.clear_game_area();
        self.draw_outdoor_backdrop();
        // = seg000:37ee call loc_04e12 — resolve (and draw via loc_04ded) a
        //   neighbouring location's entrance on the horizon when within ±4
        //   longitude cells of it. TODO: not ported.
        // = seg000:37f1 jmp loc_04d06 — the location-entrance proximity pass:
        //   standing on a location's longitude within its visibility distance
        //   animates the walk-up (loc_04d57/loc_04bdf) and plays SN5.VOC.
        //   TODO: not ported (data_04733 stays 0, which disables it in DOS
        //   too).
    }

    // = seg000:40c3 move_all_NPCs_whose_bit_6_of_flags_is_set — via
    // scan_matching_room_person_entries with the NPC_move_if_flag_bit_6_set_040c9
    // callback (seg000:40c9): every room-person entry matching the room being
    // left whose flags carry bit 0x40 is rewritten to the destination, so a
    // companion follows the player into the new room. The scan matches the
    // entries against the *memory* (location_and_room, location_appearance) —
    // the room being left — while the callback receives the caller's dx/bx (the
    // destination, restored around the call at seg000:3707..370a).
    pub(crate) fn move_all_npcs_whose_bit_6_of_flags_is_set(
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
        // = seg000:08f8/08fc (loc_008f0) — record the scene being drawn so
        // get_location_and_room / add_room_frame_task can read it back.
        self.location_and_room = location_and_room;
        self.location_appearance = location_appearance;
        // = seg000:0900 mov [current_scene],dh.
        self.data_00008 = (location_and_room >> 8) as u8;

        // = seg000:37b8 orni_hotspot_x = 0 — every DOS room draw runs the
        // loc_037b5 prologue (draw_room_scene, and the zoom re-render via
        // seg000:3b2d), clearing the parked-orni hover hotspot until this
        // draw's orni pass records one. Notably the dialogue-zoom re-render
        // clears it and its orni pass skips the re-record (render flags 0x81),
        // so the orni is not clickable behind a talking head.
        self.orni_hotspot_x = 0;

        let dh = (location_and_room >> 8) as usize;
        let dl = (location_and_room & 0xff) as usize;
        let bh = (location_appearance >> 8) as usize;

        // = seg000:0904..090b (loc_008f0) — every scene open recomputes the
        // current-location record from the 1-based location slot bh.
        self.current_location_index = bh as u16 - 1;

        // = loc_008f0 / open_SAL_resource / calc_SAL_index: locations[bh-1]
        //   .apparence picks the SAL. open_SAL_resource maps a calc result of
        //   4 back to 3, so SAL indices clamp to the four SAL files.
        let apparence = self.locations[bh - 1].appearance;
        // open_SAL_resource clamps a calc result of 4 back to 3 for the four
        // SAL files (draw_outdoor_backdrop keeps the unclamped 0..4 index).
        let sal_name = SAL_NAMES[calc_sal_index(apparence).min(3)];

        // = loc_03efe: pick scene record (dl-1) in the table starting at
        //   SCENE_DISPATCH[dh]. The record's `background` byte drives draw_SAL.
        let record = &self.scene_records[SCENE_DISPATCH[dh] as usize + (dl - 1)];
        let background = record.background;

        // = draw_SAL (seg000:3b59): split the background byte into a SAL room
        //   sub-chunk and a sprite-sheet resource.
        let room = ((background - 1) & 0x0f) as usize;
        let sheet_index = ((background - 1) >> 4) as usize;
        let sheet_name = ROOM_SHEET_NAMES[sheet_index];

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
            // the backdrop, so the backdrop runs for all first rooms. The
            // loc_037eb tail — loc_04e12 + loc_04d06, the neighbouring-location
            // horizon sprites — also runs here in DOS; see draw_desert_view.)
            self.draw_outdoor_backdrop();
        }

        self.draw_sal_room(sal_name, room, sheet_name, sheet_index != 0);

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
            let (x, y) = self.get_orni_position();
            // = seg000:3a5a..3a67 record the first orni's hover hotspot
            // (position + (0xc, 8)) for person_hit_test's orni tail
            // (seg000:92ab) — hovering/clicking the parked orni resolves to
            // the 0x2f pseudo-person (the TAKE AN ORNITHOPTER verb).
            self.orni_hotspot_x = (x + 12) as u16;
            self.orni_hotspot_y = (y + 8) as u16;
            // = seg000:3a6a draw_ornis_loop — one orni per available
            // ornithopter.
            self.draw_ornis_loop(count, x, y);
        }
        // = seg000:3a41 push 388dh — the pass exits through set_sky_palette so
        // the sky-gradient palette entries ORNY's bank palette overwrote are
        // restored.
        self.set_sky_palette();
    }

    // = seg000:3a6a draw_ornis_loop — draw `count` parked ornis from the
    // active bank starting at (x, y), each stepped down-right by (0x46, 0x0a).
    pub(crate) fn draw_ornis_loop(&mut self, count: u8, mut x: i16, mut y: i16) {
        for _ in 0..count {
            self.draw_orni(x, y);
            x += 70;
            y += 10;
        }
    }

    // = seg000:3a73 draw_ornis — the loop's step-first entry: advance one pad
    // slot and loop-decrement BEFORE drawing, so `count - 1` ornis draw at
    // slots 2..count. The takeoff frames enter here to leave the departing
    // orni's first slot empty.
    pub(crate) fn draw_ornis(&mut self, count: u8, x: i16, y: i16) {
        self.draw_ornis_loop(count.saturating_sub(1), x + 70, y + 10);
    }

    // = seg000:3a95 get_orni_position — the landing-pad screen position for the
    // current location: (149, 57) for location codes (location_and_room high
    // byte) below 0x20 (the sietches), (202, 73) for the rest (the palace /
    // city views).
    pub(crate) fn get_orni_position(&self) -> (i16, i16) {
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
    pub(crate) fn draw_orni(&mut self, x: i16, y: i16) {
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

    // = seg000:380c draw_outdoor_backdrop (reached via loc_037eb, from both
    // draw_room_scene's first-room dispatch at seg000:3a02..3a16 and the
    // desert-view branch loc_037dc). The outdoor view drawn as (or behind) the
    // scene: install the sky-gradient palette, pick a full-screen backdrop
    // sprite sheet, open it and draw its sprite 0 (loc_0c2f2, =
    // open_spritesheet + draw_active_bank_sprite). For a location's first room
    // the SAL architecture (e.g. BALCON.HSQ for palace room 0x2001) composites
    // over it; in the desert it IS the scene.
    //
    // The backdrop selection (al = the distance the location backdrop is seen
    // from): in a room (location_appearance low byte 0x80) al = 0; in the
    // desert on the location's longitude al = the fine latitude walked; other
    // desert cells (or past room1_backdrop_threshold) fall back to the
    // position-hashed terrain tile.
    fn draw_outdoor_backdrop(&mut self) {
        // = seg000:380c mov [sky_skydn_selector],1; 3811 call set_sky_palette —
        // the backdrop sits under the sky gradient, so install it first.
        self.sky_skydn_selector = 1;
        self.set_sky_palette();

        // = seg000:3814 si = [last_location_ptr]; 3818/381b bx = 1972h +
        // calc_SAL_index — the nearby location picks the backdrop family.
        let location = &self.locations[self.last_location_index];
        let sal_index = calc_sal_index(location.appearance);

        // = seg000:3820/3824 dx = location_and_room, ax = location_appearance.
        let bl = (self.location_appearance & 0xff) as u8;
        let bh = (self.location_appearance >> 8) as u8;
        // = seg000:3827..3832 — al = 0 in a room (bl == 0x80); in the desert
        // al = the fine latitude (bh) when standing on the location's
        // longitude (dx == [si+2]), else the terrain-tile branch (loc_0384a).
        let distance = if bl == 0x80 {
            Some(0u8)
        } else if self.location_and_room == location.map_x as u16 {
            Some(bh)
        } else {
            None
        };

        let resource = match distance {
            // = seg000:3834 cmp al,room1_backdrop_threshold[bx]; jnb loc_0384a.
            Some(al) if al < ROOM1_BACKDROP_THRESHOLD[sal_index] => {
                // = seg000:3839 add al,room1_backdrop_base[bx].
                let resource = ROOM1_BACKDROP_BASE[sal_index].wrapping_add(al);
                // = seg000:383b..3845 the VILG base at distance 0 (al == 7fh)
                // varies the village view by the location's first_name:
                // al += first_name/2 - 5 (VIL1.. 0x7a..0x80).
                if resource == 0x7f {
                    (resource
                        .wrapping_add(location.first_name >> 1)
                        .wrapping_sub(5)) as i16
                } else {
                    resource as i16
                }
            }
            _ => self.desert_tile_resource(),
        };

        // = seg000:3847 jmp loc_0c2f2 — open the backdrop bank (applies its
        // palette) and draw sprite 0 at (0,0).
        self.open_resource_and_draw_sprite0(resource);
    }

    // = seg000:384a loc_0384a — the desert terrain tile: the DN20..DN38 dune
    // set, or the VG01..VG10 rock set when the nearby location's status bit 0
    // is set or any of the four map bytes around the player's cell is rock
    // terrain ((byte & 0x30) == 0x10). The tile within the set is a hash of
    // the desert position, so a cell always shows the same view.
    fn desert_tile_resource(&mut self) -> i16 {
        // = seg000:384a di = [last_location_ptr]; 384e test [di+0ah],1.
        let location = &self.locations[self.last_location_index];
        let rock_set = if location.status & 1 != 0 {
            true
        } else {
            // = seg000:3854/3857 get_map_position + map_func; 385a dec di;
            // 385b..3868 scan the 4 bytes at cell-1 .. cell+2.
            let (x, lat) = self.get_map_position();
            let offset = self.map_position_to_offset(x, lat).0;
            (offset - 1..offset + 3).any(|o| self.map[o] & 0x30 == 0x10)
        };
        // = seg000:386a/386d dunes: 0x13 tiles from 0x42 (DN20);
        // = seg000:3872/3875 rock: 0x0a tiles from 0x88 (VG01).
        let (count, base) = if rock_set { (0x0a, 0x88) } else { (0x13, 0x42) };
        // = seg000:3878..3888 hash: (swapped location_appearance ^
        // location_and_room + 1) % count + base.
        let hash = (self.location_appearance.swap_bytes() ^ self.location_and_room).wrapping_add(1);
        (hash % count + base) as i16
    }

    // = seg000:3b59 draw_SAL (inner work). Open the sprite-sheet resource
    // (applying its palette, mirroring open_spritesheet ->
    // apply_sprite_sheet_palette), read one room sub-chunk from a .SAL room
    // sheet, and blit its sprites / polygons / lines into the active
    // framebuffer at the current fb_base_ofs (state.y_offset), landing in the
    // game-area rect (rows 24..175). The recursive sprite/polygon/line decode
    // lives in RoomSheet/RoomRenderer.
    fn draw_sal_room(
        &mut self,
        sal_name: &str,
        room: usize,
        sprite_sheet_name: &str,
        apply_sheet_palette: bool,
    ) {
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
        // entries the room draws with. NOT for sheet index 0 (GENERIC.HSQ):
        // draw_SAL skips the open_resource_by_index for it entirely
        // (seg000:3b62..3b68 `shr ax,4; jz loc_03b70`) — GENERIC stays
        // resident through the font path (font_draw_glyph_func, seg000:d176)
        // and its [240..254] palette chunk (the sand UI tint) must not stamp
        // over the sky palette's time-of-day UI span here.
        if apply_sheet_palette {
            sprite_sheet
                .apply_palette_update(&mut self.palette)
                .expect("failed to apply palette");
        }

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
        // sheet's palette update; DOS restores the previously active bank after
        // each character (seg000:3d7f/3d80 pop ax; jmp open_resource_by_index),
        // so re-apply the room palette last to keep its entries winning on any
        // overlap — except for the never-opened GENERIC sheet (see above),
        // whose id is not the pushed active bank in DOS.
        let character_sheet = if markers.iter().any(|&m| m != -1) {
            let pers = self
                .dat_file
                .read("PERS.HSQ")
                .expect("failed to read PERS.HSQ");
            let sheet = SpriteSheet::from_slice(&pers).expect("failed to parse PERS.HSQ");
            sheet
                .apply_palette_update(&mut self.palette)
                .expect("failed to apply PERS palette");
            if apply_sheet_palette {
                sprite_sheet
                    .apply_palette_update(&mut self.palette)
                    .expect("failed to re-apply room palette");
            }
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
    // = the desert_step_deltas/desert_apply_step_delta pair driving the walk:
    // stepping DOWN (south) off a cell bumps the fine latitude; stepping UP
    // undoes it; the fine-latitude carry into the row clamps at ±0x62.
    #[test]
    fn desert_steps_move_and_clamp() {
        // Palace exit 0xfd = DOWN = index 3: fine latitude +1, longitude kept.
        assert_eq!(
            desert_apply_step_delta(DESERT_STEP_DELTAS[3], 6421, 0x00fc),
            (6421, 0x01fc)
        );
        // UP (index 1) steps straight back onto the row.
        assert_eq!(
            desert_apply_step_delta(DESERT_STEP_DELTAS[1], 6421, 0x01fc),
            (6421, 0x00fc)
        );
        // RIGHT/LEFT move only the longitude.
        assert_eq!(
            desert_apply_step_delta(DESERT_STEP_DELTAS[2], 6421, 0x01fc),
            (6422, 0x01fc)
        );
        assert_eq!(
            desert_apply_step_delta(DESERT_STEP_DELTAS[4], 6421, 0x01fc),
            (6420, 0x01fc)
        );
        // Index 5 (exit 0xfb) steps onto the map in place.
        assert_eq!(
            desert_apply_step_delta(DESERT_STEP_DELTAS[5], 6421, 0x00fc),
            (6421, 0x00fc)
        );
        // The southern clamp: row 0x61 fine 0xff refuses to cross into 0x62.
        assert_eq!(
            desert_apply_step_delta(DESERT_STEP_DELTAS[3], 0, 0xff61),
            (0, 0xff61)
        );
        // The northern clamp: row -0x61 fine 0 refuses to cross into -0x62.
        assert_eq!(
            desert_apply_step_delta(DESERT_STEP_DELTAS[1], 0, 0x009f),
            (0, 0x009f)
        );
    }

    // = the walk-out special exit (loc_03fd2) + the desert-position dispatch
    // (loc_03ff5) + the walk-in arrival (loc_04002/arrive_at_location), end to
    // end: clicking DOWN in palace room 1 (exit 0xfd) walks one step south
    // into the desert (the outdoor palace view), clicking UP walks back onto
    // the palace's map cell and re-enters its entry room.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn walking_out_of_the_palace_and_back() {
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
        // = the seg000:02b8 -> loc_008f0 startup: the throne room is the
        // palace, so the centre palace-plan button [17] shows from the very
        // first nav-panel rebuild (regression: the rebuild runs before the
        // first draw_location_room, so intro2's tail must have recorded the
        // current location already).
        assert_eq!(game.current_location_index, 0);
        assert_eq!(game.ui_elements[17].flags, 0x80);
        // Stand in the palace entry room (0x2001), the room with the DOWN
        // compass arrow (exit 0xfd).
        game.location_and_room = 0x2001;
        game.location_appearance = 0x180;
        game.current_room = 1;
        game.draw_room_game_screen();

        // DOWN: the special exit walks one step south of the palace cell.
        game.ui_click_move_down();
        assert_eq!(game.location_and_room, 6421, "longitude = palace map_x");
        assert_eq!(
            game.location_appearance, 0x01fc,
            "fine latitude 1, latitude row -4"
        );
        assert_eq!(game.data_00008, 0xff, "no room scene in the desert");
        assert_eq!(
            game.current_location_index, 0xffff,
            "the walk-out detaches the current location"
        );
        // = the loc_03073 alt rebuild leaves the centre button [17] as the
        // entry room set it (hidden) — the move commit enters at loc_02dbf,
        // past the nav-panel template copy.
        assert_eq!(game.ui_elements[17].flags, 0x20);
        let desert = game.active_fb_mut().pixels().to_vec();

        // UP: back onto the palace cell -> the walk-in arrival puts us in the
        // palace entry room.
        game.ui_click_move_up();
        assert_eq!(game.location_and_room, 0x2001);
        assert_eq!(game.location_appearance, 0x0180);
        assert_eq!(game.data_00008, 0x20);
        assert_eq!(game.current_room, 1);
        assert_eq!(game.current_location_index, 0);
        assert_eq!(game.last_location_index, 0);
        // = seg000:3039..304e the centre palace-plan button [17]: hidden in
        // the palace entry room, visible in its inner rooms.
        assert_eq!(game.ui_elements[17].flags, 0x20);
        game.ui_click_move_up(); // room 1 -> room 2 (exit 0x02)
        assert_eq!(game.location_and_room, 0x2002);
        assert_eq!(game.ui_elements[17].flags, 0x80);
        game.ui_click_move_down(); // room 2 -> room 1 (exit 0x01)
        assert_eq!(game.location_and_room, 0x2001);
        let room = game.active_fb_mut().pixels().to_vec();

        let differing = desert.iter().zip(&room).filter(|(a, b)| a != b).count();
        assert!(
            differing > 10_000,
            "the desert view and the room render must differ (changed {differing} pixels)"
        );

        // Walk four steps south: fine latitudes 1..3 show the receding palace
        // backdrops, fine latitude 4 crosses room1_backdrop_threshold and
        // renders the position-hashed terrain tile (desert_tile_resource).
        for _ in 0..4 {
            game.ui_click_move_down();
        }
        assert_eq!(game.location_appearance, 0x04fc);
        // A sideways step moves only the longitude (still the tile branch,
        // now off the palace's longitude).
        game.ui_click_move_left();
        assert_eq!(game.location_and_room, 6420);
        assert_eq!(game.location_appearance, 0x04fc);
        if std::env::var_os("WRITE_PNG").is_some() {
            let fb = game.active_fb_mut().clone();
            fb.write_png_scaled(&game.palette, "/tmp/desert_walk_far.png")
                .expect("write png");
            eprintln!("wrote /tmp/desert_walk_far.png");
        }

        // Retrace the steps: back east onto the palace longitude, then four
        // steps north re-enter the palace entry room.
        game.ui_click_move_right();
        for _ in 0..4 {
            game.ui_click_move_up();
        }
        assert_eq!(game.location_and_room, 0x2001);
        assert_eq!(game.location_appearance, 0x0180);

        if std::env::var_os("WRITE_PNG").is_some() {
            game.ui_click_move_down();
            let fb = game.active_fb_mut().clone();
            fb.write_png_scaled(&game.palette, "/tmp/desert_walk.png")
                .expect("write png");
            eprintln!("wrote /tmp/desert_walk.png");
        }
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
