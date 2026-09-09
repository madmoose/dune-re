//! Save-game files (= the DOS save/load path, seg000:b28c..b52f).
//!
//! The on-disk format matches the original `dune37s<N>.sav` files — see
//! docs/savegame-format.md for the full layout. A file is a u16 `game_time`
//! header followed by an RLE-compressed 0x567a-byte image of four memory
//! regions: the packed map overlay bits, the dialogue-played log, the
//! DIALOGUE buffer, and the seg001:0000..1260 game-state block.
//!
//! Fields of the state block the port does not model yet are written as
//! zeroes and ignored on load (project decision; DOS-side round-trip
//! compatibility is out of scope for now).

use std::{io, path::Path};

use crate::{
    GameState, container,
    locations::Equipment,
    menu_defs::{self, CMD_GREY, CMD_HIGHLIGHT, MenuItem, MenuRef},
};

/// = the uncompressed save-image size (create_save_in_memory returns cx = 0x567a).
const IMAGE_LEN: usize = 0x567a;

/// = seg000:b439 — the map region covers 0xc5fc map cells (the MAP.HSQ buffer
/// plus 3 slack bytes; the resource decompresses to 0xc5f9).
const MAP_CELLS: usize = 0xc5fc;

/// Image offsets of the four regions (= the create_save_in_memory copy order).
const OFS_MAP: usize = 0;
const OFS_LOG: usize = OFS_MAP + MAP_CELLS / 4; // 0x317f
/// = seg000:b457 — the dialogue-played log region is 0xa2 bytes.
const LOG_LEN: usize = 0xa2;
const OFS_DIALOGUE: usize = OFS_LOG + LOG_LEN; // 0x3221
const OFS_STATE: usize = OFS_DIALOGUE + 0x11f8; // 0x4419
/// = seg001:1225..1261 atreides_palace_rooms: the scene records inside the
/// state block (the 0x1261 cut lands exactly on the end of the 12th record).
const PALACE_ROOM_COUNT: usize = 12;
const STATE_LEN: usize = 0x1261;

/// = seg000:b4ea `mov dl, 0f7h` — the RLE escape byte.
const RLE_ESCAPE: u8 = 0xf7;

/// = seg001:aa76 — the DIALOGUE buffer's fixed seg001 address. The DOS image
/// carries the buffer with its leading sub-resource offset table relocated by
/// this base (adjust_sub_resource_pointers, seg000:0098); the port's buffer is
/// unrelocated, so the base is added on save and removed on load.
const DIALOGUE_BASE: u16 = 0xaa76;

/// = seg001:38a8 aDune37s0_sav — the save file name, with the digit at
/// position 7 patched to '1' + slot (seg000:b38c).
pub(crate) fn save_game_filename(slot: u8) -> String {
    format!("dune37s{}.sav", (b'1' + slot) as char)
}

// = seg000:b4ea compress_sav — byte-RLE. Output: u16 escape value, u16 block
// length (4 + stream, i.e. including this header), then the stream. Runs of
// 3..=255 equal bytes — and any run of the escape byte itself — emit
// (escape, count, value); runs of 1-2 other bytes emit the bytes literally.
// (DOS leaves garbage in the escape word's high byte and stores a 0x0000
// terminator after the stream that never reaches the file; the port writes a
// clean 0 high byte and no terminator.)
fn compress_sav(src: &[u8]) -> Vec<u8> {
    let mut out = vec![RLE_ESCAPE, 0, 0, 0];
    let mut i = 0;
    while i < src.len() {
        let value = src[i];
        let mut count = 1;
        while count < 255 && i + count < src.len() && src[i + count] == value {
            count += 1;
        }
        if value == RLE_ESCAPE || count > 2 {
            out.extend_from_slice(&[RLE_ESCAPE, count as u8, value]);
        } else {
            for _ in 0..count {
                out.push(value);
            }
        }
        i += count;
    }
    let len = out.len() as u16;
    out[2..4].copy_from_slice(&len.to_le_bytes());
    out
}

// = seg000:b4bb decompress_sav — the inverse: literals copy through, the
// escape byte introduces a (count, value) run (count may be 0). The block
// length word bounds the stream; malformed input just ends the stream early
// (the caller validates the decompressed size).
fn decompress_sav(block: &[u8]) -> Vec<u8> {
    if block.len() < 4 {
        return Vec::new();
    }
    let escape = block[0];
    let len = (u16::from_le_bytes([block[2], block[3]]) as usize).min(block.len());
    let mut out = Vec::with_capacity(IMAGE_LEN);
    let mut i = 4;
    while i < len {
        let b = block[i];
        i += 1;
        if b != escape {
            out.push(b);
            continue;
        }
        if i + 1 >= len {
            break;
        }
        let count = block[i] as usize;
        let value = block[i + 1];
        i += 2;
        out.extend(std::iter::repeat_n(value, count));
    }
    out
}

fn w8(b: &mut [u8], ofs: usize, v: u8) {
    b[ofs] = v;
}

fn w16(b: &mut [u8], ofs: usize, v: u16) {
    b[ofs..ofs + 2].copy_from_slice(&v.to_le_bytes());
}

fn r8(b: &[u8], ofs: usize) -> u8 {
    b[ofs]
}

fn r16(b: &[u8], ofs: usize) -> u16 {
    u16::from_le_bytes([b[ofs], b[ofs + 1]])
}

/// The seg001 pointer form of a `locations[]` index (base 0x100, stride 0x1c);
/// 0xffff passes through as the "no location" sentinel.
fn location_ptr_from_index(index: u16) -> u16 {
    if index == 0xffff {
        0xffff
    } else {
        0x100 + index * 0x1c
    }
}

fn location_index_from_ptr(ptr: u16) -> u16 {
    if ptr == 0xffff {
        0xffff
    } else {
        (ptr.wrapping_sub(0x100)) / 0x1c
    }
}

fn write_equipment(b: &mut [u8], ofs: usize, e: &Equipment) {
    b[ofs..ofs + 7].copy_from_slice(&[
        e.harvesters,
        e.ornithopters,
        e.krys_knives,
        e.laser_guns,
        e.weirding_modules,
        e.atomics,
        e.bulbs,
    ]);
}

fn read_equipment(b: &[u8], ofs: usize) -> Equipment {
    Equipment {
        harvesters: b[ofs],
        ornithopters: b[ofs + 1],
        krys_knives: b[ofs + 2],
        laser_guns: b[ofs + 3],
        weirding_modules: b[ofs + 4],
        atomics: b[ofs + 5],
        bulbs: b[ofs + 6],
    }
}

impl GameState {
    // = seg000:b427 create_save_in_memory — build the uncompressed 0x567a-byte
    // save image: the four memory regions back to back.
    pub(crate) fn create_save_in_memory(&self) -> Vec<u8> {
        let mut image = vec![0u8; IMAGE_LEN];

        // = seg000:b433..b450 — pack bits 5-4 (the mutable vegetation overlay)
        // of each of the 0xc5fc map cells, 4 cells per byte, the first cell in
        // bits 7-6. Cells past the buffer end (the 3 slack bytes) pack as 0.
        for (group, packed) in image[OFS_MAP..OFS_LOG].iter_mut().enumerate() {
            for k in 0..4 {
                let cell = self.map.get(group * 4 + k).copied().unwrap_or(0);
                *packed = *packed << 2 | (cell >> 4) & 3;
            }
        }

        // = seg000:b452..b45a — the dialogue-played log at cs:00aa..014b: the
        // 0-terminated word list of replayable spoken lines. The region holds
        // at most 0x50 words + the terminator. DOS snapshots the raw bytes,
        // which past the terminator are the initialize_resources code the log
        // grows over; that code is not game state, so the port writes zeros.
        for (k, word) in self.dialogue_played_log.iter().take(0x50).enumerate() {
            w16(&mut image, OFS_LOG + 2 * k, *word);
        }

        // = seg000:b45c..b463 — the DIALOGUE buffer (0x1190 bytes + slack to
        // 0x11f8), with its leading sub-resource offset table in the relocated
        // form the DOS buffer holds: each of the table's len/2 words (the
        // length word included) gets the seg001:aa76 base added.
        let n = self.dialogue.len().min(0x11f8);
        image[OFS_DIALOGUE..OFS_DIALOGUE + n].copy_from_slice(&self.dialogue[..n]);
        if n >= 2 {
            let table_len = (r16(&image, OFS_DIALOGUE) as usize & !1).min(n);
            for ofs in (0..table_len).step_by(2) {
                let w = r16(&image, OFS_DIALOGUE + ofs).wrapping_add(DIALOGUE_BASE);
                w16(&mut image, OFS_DIALOGUE + ofs, w);
            }
        }

        // = seg000:b465..b46b — the game-state block seg001:0000..1260.
        self.write_state_block(&mut image[OFS_STATE..OFS_STATE + STATE_LEN]);

        image
    }

    // = seg000:b473 restore_from_save_memory — scatter a decompressed save
    // image back into live memory.
    pub(crate) fn restore_from_save_memory(&mut self, image: &[u8]) {
        // = seg000:b473..b49c — merge the packed overlay bits back into bits
        // 5-4 of the map cells: cell = cell & 0xcf | saved bits.
        for (group, packed) in image[OFS_MAP..OFS_LOG].iter().enumerate() {
            for k in 0..4 {
                if let Some(cell) = self.map.get_mut(group * 4 + k) {
                    *cell = *cell & 0xcf | (packed >> (6 - 2 * k)) << 4 & 0x30;
                }
            }
        }

        // = seg000:b49e..b4a6 — the dialogue-played log, words up to the 0
        // terminator.
        self.dialogue_played_log.clear();
        for k in 0..0x50 {
            let word = r16(image, OFS_LOG + 2 * k);
            if word == 0 {
                break;
            }
            self.dialogue_played_log.push(word);
        }

        // = seg000:b4a8..b4b0 — the DIALOGUE buffer, with the offset table's
        // relocation (the seg001:aa76 base) removed again for the port's
        // buffer-relative form.
        let n = self.dialogue.len().min(0x11f8);
        self.dialogue[..n].copy_from_slice(&image[OFS_DIALOGUE..OFS_DIALOGUE + n]);
        if n >= 2 {
            let table_len =
                (r16(&self.dialogue, 0).wrapping_sub(DIALOGUE_BASE) as usize & !1).min(n);
            for ofs in (0..table_len).step_by(2) {
                let w = r16(&self.dialogue, ofs).wrapping_sub(DIALOGUE_BASE);
                w16(&mut self.dialogue, ofs, w);
            }
        }

        // = seg000:b4b2..b4b8 — the game-state block.
        self.read_state_block(&image[OFS_STATE..OFS_STATE + STATE_LEN]);
    }

    // The seg001:0000..1260 game-state block, field by field at its DOS
    // offsets. Unported fields stay zero; the CONDIT scratch mirrors at
    // 002c..005b are staged state the port does model, so they ride along.
    fn write_state_block(&self, b: &mut [u8]) {
        w16(b, 0x0000, self.rand_bits);
        w16(b, 0x0002, self.game_time);
        w16(b, 0x0004, self.location_and_room);
        w16(b, 0x0006, self.location_appearance);
        w8(b, 0x0008, self.data_00008);
        w8(b, 0x0009, self.data_00009);
        w8(b, 0x000a, self.bitfield_paul_events);
        w8(b, 0x000b, self.current_room);
        w8(b, 0x000c, self.pending_destination_room);
        w8(b, 0x000d, self.previous_room);
        w16(b, 0x000e, self.persons_met);
        w16(b, 0x0010, self.persons_travelling_with);
        w16(b, 0x0012, self.persons_in_room);
        w16(b, 0x0014, self.persons_talking_to);
        w8(b, 0x0019, self.line_spoken_this_conversation);
        w8(b, 0x001b, self.data_0001b);
        w8(b, 0x001c, self.related_to_paying_smuggler_bills_ds_1c);
        w8(b, 0x001d, self.current_smuggler_willingness_to_haggle_ds_1d);
        w8(b, 0x001f, self.related_to_paying_smuggler_bills_ds_1f);
        w8(
            b,
            0x001e,
            self.current_smuggler_number_of_days_since_previous_encounter_ds_1e,
        );
        w16(b, 0x0020, self.current_smuggler_bill_value_ds_20);
        w8(b, 0x0022, self.smuggler_bills_count_ds_22);
        w8(b, 0x0023, self.pending_room_action);
        w8(b, 0x0025, self.number_of_sietches_visited);
        w8(b, 0x0026, self.entering_new_sietch);
        w8(b, 0x0027, self.discovered_sietch_count);
        w8(b, 0x0028, self.number_of_rallied_troops);
        w8(b, 0x0029, self.charisma);
        w8(b, 0x002a, self.game_phase);
        w8(b, 0x002b, self.night_attack_stage);

        // = seg001:002c..004b the for_condit troop staging block.
        let tc = &self.troop_condit;
        w16(b, 0x002c, tc.offset_of_location);
        w8(b, 0x002e, tc.troop_id);
        w8(b, 0x002f, tc.occupation_low);
        w8(b, 0x0030, tc.occupation);
        w8(b, 0x0031, tc.dissatisfaction_low);
        w16(b, 0x0032, tc.bitfield_10);
        w16(b, 0x0034, tc.dissatisfaction_and_speech);
        w8(b, 0x0036, tc.motivation_modifier);
        w8(b, 0x0037, tc.skill_in_occupation);
        w8(b, 0x0038, tc.spice_skill);
        w8(b, 0x0039, tc.army_skill);
        w8(b, 0x003a, tc.ecology_skill);
        w8(b, 0x003b, tc.equipment);
        w8(b, 0x003c, tc.population);
        w8(b, 0x003d, tc.equipment_added_by_modify);
        w8(b, 0x003e, tc.equipment_removed_by_modify);
        w8(b, 0x003f, tc.paul_orni_given);
        w8(b, 0x0040, tc.days_since_ralliement);
        w8(b, 0x0041, tc.game_days_since_ralliement);
        w16(b, 0x0042, tc.time_periods_since_ralliement);
        w16(b, 0x0044, tc.harvest_rate);
        w16(b, 0x0046, tc.harvest_total);
        w16(b, 0x0048, tc.ds_48);
        w16(b, 0x004a, tc.ds_4a);
        w8(b, 0x004c, self.contacting_troops_ds_4c);

        // = seg001:004d..005b the for_condit location staging block.
        let lc = &self.location_condit;
        w8(b, 0x004d, lc.appearance);
        w16(b, 0x004e, lc.area_and_name);
        w8(b, 0x0050, lc.worm_event_likelihood);
        w8(b, 0x0051, lc.status);
        w8(b, 0x0052, lc.spice_density);
        w8(b, 0x0053, lc.unused_equipment);
        w8(b, 0x0054, lc.water);
        b[0x0055..0x005c].copy_from_slice(&lc.equipment);
        w16(b, 0x005c, lc.combined_bitfield_10);
        w16(b, 0x005e, lc.combined_dissatisfaction);
        w16(b, 0x0094, lc.harkonnen_strength);
        w16(b, 0x0096, lc.fremen_strength);
        w8(b, 0x009c, lc.battle_balance);

        w16(b, 0x00a0, self.spice_in_stock);
        w16(b, 0x00a2, self.area_controlled_by_atreides);
        w16(b, 0x00a4, self.area_controlled_by_harkonnen);
        w16(b, 0x00a6, self.todays_spice_production);
        w16(b, 0x00a8, self.harkonnen_spice_production);
        w16(b, 0x00aa, self.data_000aa);
        w16(b, 0x00ac, self.data_000ac);
        w16(b, 0x00ae, self.previous_day_spice_production);
        w16(b, 0x00b0, self.spice_production_better_than_previous_day);
        w16(b, 0x00b2, self.spice_production_lower_than_previous_day);
        w8(b, 0x009d, self.for_condit_smuggler_dialogue_related_ds_9d);
        w8(b, 0x009e, self.for_condit_smuggler_arguing_count_ds_9e);
        w8(b, 0x009f, self.accept_refuse_argue_choice_ds_9f);
        for (k, v) in self.spice_shipment_arguing_ds_b4.iter().enumerate() {
            w16(b, 0x00b4 + k * 2, *v);
        }
        w16(b, 0x00bc, self.spice_shipment_quantity);
        w8(b, 0x00be, self.spice_shipment_fulfilment);
        w8(b, 0x00bf, self.spice_shipment_flags);
        w16(b, 0x00c0, self.for_condit_spice_shipment_ds_c0);
        w8(b, 0x00c2, self.final_attack_stage);
        w8(b, 0x00c3, self.spice_shipment_sequence_number);
        w8(b, 0x00c4, self.number_of_sietches_attacked_by_harkonnen);
        w8(b, 0x00c5, self.person_marker_base);
        w8(b, 0x00c6, self.data_000c6);
        // = seg001:00c8 — one byte in DOS: the comm sighting count (and the
        // comms-room "message queued" flag the port carries as data_000c8).
        w8(b, 0x00c8, self.comm_sightings.len() as u8);
        // = seg001:00c9 the COMM unread badge.
        w8(b, 0x00c9, self.comm_unread_count_ds_c9);
        // = seg001:0024 / 00e7 / 00eb the COMM / vision condit bytes.
        w8(b, 0x0024, self.for_dialogue_enemies_ds_24);
        w8(b, 0x00e7, self.paul_found_unconscious_ds_e7);
        w8(b, 0x00eb, self.comm_message_needs_viewing_ds_eb);
        // = seg001:00ca..00e6 the five nearest-location triples
        // (condit_scan_nearest_locations).
        for (base, t) in [
            (0x00ca, &self.nearest_location),
            (0x00d0, &self.nearest_village),
            (0x00d6, &self.nearest_sietch),
            (0x00dc, &self.nearest_atreides_area),
            (0x00e2, &self.nearest_harkonnen_area),
        ] {
            w16(b, base, t.distance);
            w16(b, base + 2, t.loc_ptr);
            w8(b, base + 4, t.octant);
        }
        w8(b, 0x00cf, self.days_left_until_spice_shipment);
        w8(b, 0x00d5, self.contact_distance_related_ds_d5);
        w8(b, 0x00e1, self.data_000e1);
        w8(b, 0x00e8, self.ui_hud_head_index);
        w8(b, 0x00ea, self.data_000ea as u8);
        w8(b, 0x00ed, self.data_000ed);
        w16(b, 0x00ee, self.data_000ee);
        w8(b, 0x00f4, self.desert_exhaustion_counter);
        w8(
            b,
            0x00f5,
            self.for_condit_jessica_commented_on_exhaustion_ds_f5,
        );
        w8(b, 0x00f6, self.for_condit_paul_next_to_harvester_ds_f6);
        w8(b, 0x00f8, self.number_of_locations_with_illness);
        w8(b, 0x00f9, self.chani_troop_illness_cure_progress);
        w8(b, 0x00fa, self.vegetation_started_on_dune);
        w8(b, 0x00fb, self.room_view_toggle);
        w8(b, 0x00fc, self.data_000fc);
        w8(b, 0x00fe, self.game_phase_copy_ds_fe);
        w8(b, 0x00ff, self.days_since_last_game_phase_change);

        // = seg001:0100 locations[70], 0x1c bytes each.
        for (i, l) in self.locations.iter().enumerate() {
            let o = 0x100 + i * 0x1c;
            w8(b, o, l.first_name);
            w8(b, o + 0x01, l.last_name);
            w16(b, o + 0x02, l.map_x as u16);
            w16(b, o + 0x04, l.map_y as u16);
            w16(b, o + 0x06, l.map_offset);
            w8(b, o + 0x08, l.appearance);
            w8(b, o + 0x09, l.troop_id);
            w8(b, o + 0x0a, l.status);
            w8(b, o + 0x0b, l.discoverable_at_phase as u8);
            w16(b, o + 0x0c, l.vegetation_x as u16);
            w8(b, o + 0x0e, l.vegetation_y as u8);
            w8(b, o + 0x0f, l.vegetation_growth);
            w8(b, o + 0x10, l.spice_field_id);
            w8(b, o + 0x11, l.spice_amount);
            w8(b, o + 0x12, l.spice_density);
            w8(b, o + 0x13, l.field_13);
            write_equipment(b, o + 0x14, &l.equipment);
            w8(b, o + 0x1b, l.water);
        }
        // = seg001:08a8 end_of_locations sentinel (`dw -1`).
        w16(b, 0x08a8, 0xffff);

        // = seg001:08aa troops[68], 0x1b bytes each (end_of_troops at 0fd6
        // is a zero word).
        for (i, t) in self.troops.iter().enumerate() {
            let o = 0x8aa + i * 0x1b;
            w8(b, o, t.troop_id);
            w8(b, o + 0x01, t.next_troop_id);
            w8(b, o + 0x02, t.position);
            w8(b, o + 0x03, t.occupation);
            w16(b, o + 0x04, t.offset_of_location);
            w16(b, o + 0x06, t.gps_coordinates_1);
            w16(b, o + 0x08, t.gps_coordinates_2);
            w16(b, o + 0x0a, t.time_period_of_ralliement);
            w16(b, o + 0x0c, t.harvest_rate);
            w16(b, o + 0x0e, t.harvest_total);
            w16(b, o + 0x10, t.bitfield_10);
            w16(b, o + 0x12, t.dissatisfaction_and_speech);
            w8(b, o + 0x14, t.game_day_of_ralliement);
            w8(b, o + 0x15, t.motivation);
            w8(b, o + 0x16, t.spice_skill);
            w8(b, o + 0x17, t.army_skill);
            w8(b, o + 0x18, t.ecology_skill);
            w8(b, o + 0x19, t.equipment);
            w8(b, o + 0x1a, t.population);
        }

        // = seg001:0fd8 room_persons[16], 0x10 bytes each (the static-zero
        // padding word at +6 stays zero).
        for (i, p) in self.room_persons.iter().enumerate() {
            let o = 0xfd8 + i * 0x10;
            w16(b, o, p.location_and_room);
            w16(b, o + 0x02, p.location_appearance);
            w16(b, o + 0x04, p.handler);
            w16(b, o + 0x08, p.time_joined);
            w16(b, o + 0x0a, p.time_dismissed);
            w16(b, o + 0x0c, p.field_c);
            w8(b, o + 0x0e, p.person_index);
            w8(b, o + 0x0f, p.flags);
        }

        // = seg001:10d8 smugglers[6].
        for (k, s) in self.smugglers.iter().enumerate() {
            let o = 0x10d8 + k * 0x11;
            w8(b, o, s.region);
            w8(b, o + 1, s.willingness_to_haggle);
            w8(b, o + 2, s.field_2);
            w8(b, o + 3, s.field_3);
            b[o + 4..o + 9].copy_from_slice(&s.stock);
            b[o + 9..o + 0xe].copy_from_slice(&s.prices);
            w16(b, o + 0xe, s.bill_value);
            w8(b, o + 0x10, s.bill_day);
        }
        // = seg001:113f current_smuggler_ptr.
        w16(b, 0x113f, self.current_smuggler_ptr);
        // = seg001:1158 shipment_report_scene_mask.
        w16(b, 0x1158, self.shipment_report_scene_mask);

        w16(
            b,
            0x114e,
            location_ptr_from_index(self.current_location_index),
        );
        w16(
            b,
            0x1150,
            location_ptr_from_index(self.last_location_index as u16),
        );
        w8(b, 0x1152, self.companions[0] as u8);
        w8(b, 0x1153, self.companions[1] as u8);
        w16(b, 0x1154, self.harkonnen_raids_armed_after_game_time);
        w16(b, 0x1156, self.illness_plot_armed_after_ingame_day);
        w16(b, 0x1170, self.spice_stock_at_last_new_day);
        w16(b, 0x1172, self.spice_spent_today);
        w16(b, 0x1174, self.last_event_game_time);
        w16(b, 0x1176, self.location_visibility_distance);
        w8(b, 0x1178, self.number_of_rallied_troops_for_leto_killed);
        // = seg001:113e a static 0xff byte between smugglers[] and
        //   current_smuggler_ptr.
        w8(b, 0x113e, 0xff);
        // = seg001:115c/115e/116a the SEE RESULTS trend state.
        w16(b, 0x115c, self.results_stats_timestamp);
        for (k, v) in self.results_prev_values.iter().enumerate() {
            w16(b, 0x115e + 2 * k, *v);
        }
        b[0x116a..0x1170].copy_from_slice(&self.results_trend_glyphs);
        // = seg001:116b/116d/116f data_0116b/d/f — three static 3 bytes
        //   interleaved with the glyphs (the port does not model them).
        w8(b, 0x116b, 3);
        w8(b, 0x116d, 3);
        w8(b, 0x116f, 3);
        // = seg001:11b9/11ba two static bytes (7, 12h) ahead of data_011bb.
        w8(b, 0x11b9, 7);
        w8(b, 0x11ba, 0x12);
        // = seg001:11bf book_bookmark_ptr.
        w16(b, 0x11bf, self.book_bookmark_ptr);
        // = seg001:11c1/11c3 data_011c1/011c3 — the spice-density overlay's
        //   home panel origin (75, 15), constants in the port.
        w16(b, 0x11c1, 75);
        w16(b, 0x11c3, 15);
        // = seg001:11d0 troop_icon_panel_heights (88, 60, 30), constants in
        //   the port (troop_map_screen HEIGHTS).
        b[0x11d0..0x11d3].copy_from_slice(&[0x58, 0x3c, 0x1e]);
        // = seg001:11dd _stru_2068D_icon_list — two static (index, x, y)
        //   UISprite entries: (31h, 0, 4ch), (1, 0, 86h).
        for (k, w) in [0x31u16, 0, 0x4c, 1, 0, 0x86].iter().enumerate() {
            w16(b, 0x11dd + 2 * k, *w);
        }
        // = seg001:11e9 data_011e9 — static 0xffff.
        w16(b, 0x11e9, 0xffff);
        // = seg001:120b _stru_206BB_icon_list — four static entries and the
        //   0xffff terminator index: (0, b6h, 0ch), (3, 10ah, 41h),
        //   (4, 0eeh, 41h), (5, 0c1h, 41h).
        for (k, w) in [
            0u16, 0xb6, 0x0c, 3, 0x10a, 0x41, 4, 0xee, 0x41, 5, 0xc1, 0x41, 0xffff,
        ]
        .iter()
        .enumerate()
        {
            w16(b, 0x120b + 2 * k, *w);
        }

        // = seg001:1179 comm_sighting_list — the count lives at 00c8.
        for (k, word) in self.comm_sightings.iter().take(10).enumerate() {
            w16(b, 0x1179 + 2 * k, *word);
        }

        // = seg001:1190/1191 the vision-message queue.
        w8(b, 0x1190, self.vision_messages.len() as u8);
        for (k, (id, loc)) in self.vision_messages.iter().take(10).enumerate() {
            w16(b, 0x1191 + 4 * k, *id);
            w16(b, 0x1193 + 4 * k, *loc);
        }

        // = seg001:11bd dialogue_played_log_head — the cs write cursor for the
        // region-2 log (base 0xaa, one word per entry).
        let log_len = self.dialogue_played_log.len().min(0x50) as u16;
        w16(b, 0x11bd, 0xaa + 2 * log_len);

        w16(b, 0x118d, self.ingame_day_of_last_spice_shipment_event);
        w8(b, 0x11bb, self.spice_shipment_unpaid);
        w8(b, 0x11bc, self.harkonnen_raid_suppress_once);

        // = seg001:1141 the worm-event likelihood table (index 0 mutates with
        // the game phase).
        for (k, v) in self.worm_event_likelihood_by_region.iter().enumerate() {
            w8(b, 0x1141 + k, *v);
        }

        w16(b, 0x11c5, self.travel_destination_ptr);
        w8(b, 0x11c7, self.travel_heading);
        w8(b, 0x11c8, self.travel_heading_mode);
        w8(b, 0x11c9, self.game_screen_mode_flags);
        w8(b, 0x11cb, self.travel_no_location_dest);
        w16(b, 0x11cc, self.travel_step_accum);
        w16(
            b,
            0x11ce,
            location_ptr_from_index(self.condit_staged_location as u16),
        );
        for (k, ptr) in self.prospector_destinations.iter().enumerate() {
            w16(b, 0x11d3 + 2 * k, *ptr);
        }
        w16(b, 0x11db, self.latest_location_with_illness);

        // = seg001:11eb string_subst_id_table (entries 1-2 hold the staged
        // location's first/last-name ids, stage_location_name_placeholders).
        for (k, id) in self.string_subst_id_table.iter().enumerate() {
            w16(b, 0x11eb + 2 * k, *id);
        }

        // = seg001:1225 atreides_palace_rooms — the 12 palace scene records
        // (background byte + 4 exit bytes each).
        for (k, rec) in self.scene_records[..PALACE_ROOM_COUNT].iter().enumerate() {
            let o = 0x1225 + 5 * k;
            w8(b, o, rec.background);
            for (i, exit) in rec.exits.iter().enumerate() {
                w8(b, o + 1 + i, *exit);
            }
        }
    }

    // The inverse of write_state_block: restore exactly the fields it wrote.
    fn read_state_block(&mut self, b: &[u8]) {
        self.rand_bits = r16(b, 0x0000);
        self.game_time = r16(b, 0x0002);
        self.location_and_room = r16(b, 0x0004);
        self.location_appearance = r16(b, 0x0006);
        self.data_00008 = r8(b, 0x0008);
        self.data_00009 = r8(b, 0x0009);
        self.bitfield_paul_events = r8(b, 0x000a);
        self.current_room = r8(b, 0x000b);
        self.pending_destination_room = r8(b, 0x000c);
        self.previous_room = r8(b, 0x000d);
        self.persons_met = r16(b, 0x000e);
        self.persons_travelling_with = r16(b, 0x0010);
        self.persons_in_room = r16(b, 0x0012);
        self.persons_talking_to = r16(b, 0x0014);
        self.line_spoken_this_conversation = r8(b, 0x0019);
        self.data_0001b = r8(b, 0x001b);
        self.related_to_paying_smuggler_bills_ds_1c = r8(b, 0x001c);
        self.current_smuggler_willingness_to_haggle_ds_1d = r8(b, 0x001d);
        self.related_to_paying_smuggler_bills_ds_1f = r8(b, 0x001f);
        self.current_smuggler_number_of_days_since_previous_encounter_ds_1e = r8(b, 0x001e);
        self.current_smuggler_bill_value_ds_20 = r16(b, 0x0020);
        self.smuggler_bills_count_ds_22 = r8(b, 0x0022);
        self.pending_room_action = r8(b, 0x0023);
        self.number_of_sietches_visited = r8(b, 0x0025);
        self.entering_new_sietch = r8(b, 0x0026);
        self.discovered_sietch_count = r8(b, 0x0027);
        self.number_of_rallied_troops = r8(b, 0x0028);
        self.charisma = r8(b, 0x0029);
        self.game_phase = r8(b, 0x002a);
        self.night_attack_stage = r8(b, 0x002b);

        let tc = &mut self.troop_condit;
        tc.offset_of_location = r16(b, 0x002c);
        tc.troop_id = r8(b, 0x002e);
        tc.occupation_low = r8(b, 0x002f);
        tc.occupation = r8(b, 0x0030);
        tc.dissatisfaction_low = r8(b, 0x0031);
        tc.bitfield_10 = r16(b, 0x0032);
        tc.dissatisfaction_and_speech = r16(b, 0x0034);
        tc.motivation_modifier = r8(b, 0x0036);
        tc.skill_in_occupation = r8(b, 0x0037);
        tc.spice_skill = r8(b, 0x0038);
        tc.army_skill = r8(b, 0x0039);
        tc.ecology_skill = r8(b, 0x003a);
        tc.equipment = r8(b, 0x003b);
        tc.population = r8(b, 0x003c);
        tc.equipment_added_by_modify = r8(b, 0x003d);
        tc.equipment_removed_by_modify = r8(b, 0x003e);
        tc.paul_orni_given = r8(b, 0x003f);
        tc.days_since_ralliement = r8(b, 0x0040);
        tc.game_days_since_ralliement = r8(b, 0x0041);
        tc.time_periods_since_ralliement = r16(b, 0x0042);
        tc.harvest_rate = r16(b, 0x0044);
        tc.harvest_total = r16(b, 0x0046);
        tc.ds_48 = r16(b, 0x0048);
        tc.ds_4a = r16(b, 0x004a);
        self.contacting_troops_ds_4c = r8(b, 0x004c);

        let lc = &mut self.location_condit;
        lc.appearance = r8(b, 0x004d);
        lc.area_and_name = r16(b, 0x004e);
        lc.worm_event_likelihood = r8(b, 0x0050);
        lc.status = r8(b, 0x0051);
        lc.spice_density = r8(b, 0x0052);
        lc.unused_equipment = r8(b, 0x0053);
        lc.water = r8(b, 0x0054);
        lc.equipment.copy_from_slice(&b[0x0055..0x005c]);
        lc.combined_bitfield_10 = r16(b, 0x005c);
        lc.combined_dissatisfaction = r16(b, 0x005e);
        lc.harkonnen_strength = r16(b, 0x0094);
        lc.fremen_strength = r16(b, 0x0096);
        lc.battle_balance = r8(b, 0x009c);

        self.spice_in_stock = r16(b, 0x00a0);
        self.area_controlled_by_atreides = r16(b, 0x00a2);
        self.area_controlled_by_harkonnen = r16(b, 0x00a4);
        self.todays_spice_production = r16(b, 0x00a6);
        self.harkonnen_spice_production = r16(b, 0x00a8);
        self.data_000aa = r16(b, 0x00aa);
        self.data_000ac = r16(b, 0x00ac);
        self.previous_day_spice_production = r16(b, 0x00ae);
        self.spice_production_better_than_previous_day = r16(b, 0x00b0);
        self.spice_production_lower_than_previous_day = r16(b, 0x00b2);
        self.for_condit_smuggler_dialogue_related_ds_9d = r8(b, 0x009d);
        self.for_condit_smuggler_arguing_count_ds_9e = r8(b, 0x009e);
        self.accept_refuse_argue_choice_ds_9f = r8(b, 0x009f);
        for (k, v) in self.spice_shipment_arguing_ds_b4.iter_mut().enumerate() {
            *v = r16(b, 0x00b4 + k * 2);
        }
        self.spice_shipment_quantity = r16(b, 0x00bc);
        self.spice_shipment_fulfilment = r8(b, 0x00be);
        self.spice_shipment_flags = r8(b, 0x00bf);
        self.for_condit_spice_shipment_ds_c0 = r16(b, 0x00c0);
        self.final_attack_stage = r8(b, 0x00c2);
        self.spice_shipment_sequence_number = r8(b, 0x00c3);
        self.number_of_sietches_attacked_by_harkonnen = r8(b, 0x00c4);
        self.person_marker_base = r8(b, 0x00c5);
        self.data_000c6 = r8(b, 0x00c6);
        let comm_count = r8(b, 0x00c8);
        self.data_000c8 = comm_count;
        self.comm_unread_count_ds_c9 = r8(b, 0x00c9);
        self.for_dialogue_enemies_ds_24 = r8(b, 0x0024);
        self.paul_found_unconscious_ds_e7 = r8(b, 0x00e7);
        self.comm_message_needs_viewing_ds_eb = r8(b, 0x00eb);
        for (base, t) in [
            (0x00ca, &mut self.nearest_location),
            (0x00d0, &mut self.nearest_village),
            (0x00d6, &mut self.nearest_sietch),
            (0x00dc, &mut self.nearest_atreides_area),
            (0x00e2, &mut self.nearest_harkonnen_area),
        ] {
            t.distance = r16(b, base);
            t.loc_ptr = r16(b, base + 2);
            t.octant = r8(b, base + 4);
        }
        self.days_left_until_spice_shipment = r8(b, 0x00cf);
        self.contact_distance_related_ds_d5 = r8(b, 0x00d5);
        self.data_000e1 = r8(b, 0x00e1);
        self.ui_hud_head_index = r8(b, 0x00e8);
        self.data_000ea = r8(b, 0x00ea) as i8;
        self.data_000ed = r8(b, 0x00ed);
        self.data_000ee = r16(b, 0x00ee);
        self.desert_exhaustion_counter = r8(b, 0x00f4);
        self.for_condit_jessica_commented_on_exhaustion_ds_f5 = r8(b, 0x00f5);
        self.for_condit_paul_next_to_harvester_ds_f6 = r8(b, 0x00f6);
        self.number_of_locations_with_illness = r8(b, 0x00f8);
        self.chani_troop_illness_cure_progress = r8(b, 0x00f9);
        self.vegetation_started_on_dune = r8(b, 0x00fa);
        self.room_view_toggle = r8(b, 0x00fb);
        self.data_000fc = r8(b, 0x00fc);
        self.game_phase_copy_ds_fe = r8(b, 0x00fe);
        self.days_since_last_game_phase_change = r8(b, 0x00ff);

        for (i, l) in self.locations.iter_mut().enumerate() {
            let o = 0x100 + i * 0x1c;
            l.first_name = r8(b, o);
            l.last_name = r8(b, o + 0x01);
            l.map_x = r16(b, o + 0x02) as i16;
            l.map_y = r16(b, o + 0x04) as i16;
            l.map_offset = r16(b, o + 0x06);
            l.appearance = r8(b, o + 0x08);
            l.troop_id = r8(b, o + 0x09);
            l.status = r8(b, o + 0x0a);
            l.discoverable_at_phase = r8(b, o + 0x0b) as i8;
            l.vegetation_x = r16(b, o + 0x0c) as i16;
            l.vegetation_y = r8(b, o + 0x0e) as i8;
            l.vegetation_growth = r8(b, o + 0x0f);
            l.spice_field_id = r8(b, o + 0x10);
            l.spice_amount = r8(b, o + 0x11);
            l.spice_density = r8(b, o + 0x12);
            l.field_13 = r8(b, o + 0x13);
            l.equipment = read_equipment(b, o + 0x14);
            l.water = r8(b, o + 0x1b);
        }

        for (i, t) in self.troops.iter_mut().enumerate() {
            let o = 0x8aa + i * 0x1b;
            t.troop_id = r8(b, o);
            t.next_troop_id = r8(b, o + 0x01);
            t.position = r8(b, o + 0x02);
            t.occupation = r8(b, o + 0x03);
            t.offset_of_location = r16(b, o + 0x04);
            t.gps_coordinates_1 = r16(b, o + 0x06);
            t.gps_coordinates_2 = r16(b, o + 0x08);
            t.time_period_of_ralliement = r16(b, o + 0x0a);
            t.harvest_rate = r16(b, o + 0x0c);
            t.harvest_total = r16(b, o + 0x0e);
            t.bitfield_10 = r16(b, o + 0x10);
            t.dissatisfaction_and_speech = r16(b, o + 0x12);
            t.game_day_of_ralliement = r8(b, o + 0x14);
            t.motivation = r8(b, o + 0x15);
            t.spice_skill = r8(b, o + 0x16);
            t.army_skill = r8(b, o + 0x17);
            t.ecology_skill = r8(b, o + 0x18);
            t.equipment = r8(b, o + 0x19);
            t.population = r8(b, o + 0x1a);
        }

        for (i, p) in self.room_persons.iter_mut().enumerate() {
            let o = 0xfd8 + i * 0x10;
            p.location_and_room = r16(b, o);
            p.location_appearance = r16(b, o + 0x02);
            p.handler = r16(b, o + 0x04);
            p.time_joined = r16(b, o + 0x08);
            p.time_dismissed = r16(b, o + 0x0a);
            p.field_c = r16(b, o + 0x0c);
            p.person_index = r8(b, o + 0x0e);
            p.flags = r8(b, o + 0x0f);
        }

        // = seg001:10d8 smugglers[6].
        for (k, s) in self.smugglers.iter_mut().enumerate() {
            let o = 0x10d8 + k * 0x11;
            s.region = r8(b, o);
            s.willingness_to_haggle = r8(b, o + 1);
            s.field_2 = r8(b, o + 2);
            s.field_3 = r8(b, o + 3);
            s.stock.copy_from_slice(&b[o + 4..o + 9]);
            s.prices.copy_from_slice(&b[o + 9..o + 0xe]);
            s.bill_value = r16(b, o + 0xe);
            s.bill_day = r8(b, o + 0x10);
        }
        self.current_smuggler_ptr = r16(b, 0x113f);
        self.shipment_report_scene_mask = r16(b, 0x1158);

        self.current_location_index = location_index_from_ptr(r16(b, 0x114e));
        self.last_location_index = location_index_from_ptr(r16(b, 0x1150)) as usize;
        self.companions[0] = (r8(b, 0x1152) as i8) as i16;
        self.companions[1] = (r8(b, 0x1153) as i8) as i16;
        self.harkonnen_raids_armed_after_game_time = r16(b, 0x1154);
        self.illness_plot_armed_after_ingame_day = r16(b, 0x1156);
        self.spice_stock_at_last_new_day = r16(b, 0x1170);
        self.spice_spent_today = r16(b, 0x1172);
        self.last_event_game_time = r16(b, 0x1174);
        // = seg001:115c/115e/116a the SEE RESULTS trend state.
        self.results_stats_timestamp = r16(b, 0x115c);
        for (k, v) in self.results_prev_values.iter_mut().enumerate() {
            *v = r16(b, 0x115e + 2 * k);
        }
        self.results_trend_glyphs
            .copy_from_slice(&b[0x116a..0x1170]);
        // = seg001:11bf book_bookmark_ptr.
        self.book_bookmark_ptr = r16(b, 0x11bf);
        self.location_visibility_distance = r16(b, 0x1176);
        self.number_of_rallied_troops_for_leto_killed = r8(b, 0x1178);

        self.comm_sightings.clear();
        for k in 0..comm_count.min(10) as usize {
            self.comm_sightings.push(r16(b, 0x1179 + 2 * k));
        }

        self.vision_messages.clear();
        for k in 0..r8(b, 0x1190).min(10) as usize {
            self.vision_messages
                .push((r16(b, 0x1191 + 4 * k), r16(b, 0x1193 + 4 * k)));
        }

        self.ingame_day_of_last_spice_shipment_event = r16(b, 0x118d);
        for (k, v) in self.worm_event_likelihood_by_region.iter_mut().enumerate() {
            *v = r8(b, 0x1141 + k);
        }
        self.spice_shipment_unpaid = r8(b, 0x11bb);
        self.harkonnen_raid_suppress_once = r8(b, 0x11bc);

        self.travel_destination_ptr = r16(b, 0x11c5);
        self.travel_heading = r8(b, 0x11c7);
        self.travel_heading_mode = r8(b, 0x11c8);
        self.game_screen_mode_flags = r8(b, 0x11c9);
        self.travel_no_location_dest = r8(b, 0x11cb);
        self.travel_step_accum = r16(b, 0x11cc);
        self.condit_staged_location = location_index_from_ptr(r16(b, 0x11ce)) as usize;
        for (k, ptr) in self.prospector_destinations.iter_mut().enumerate() {
            *ptr = r16(b, 0x11d3 + 2 * k);
        }
        self.latest_location_with_illness = r16(b, 0x11db);

        for (k, id) in self.string_subst_id_table.iter_mut().enumerate() {
            *id = r16(b, 0x11eb + 2 * k);
        }

        for (k, rec) in self.scene_records[..PALACE_ROOM_COUNT]
            .iter_mut()
            .enumerate()
        {
            let o = 0x1225 + 5 * k;
            rec.background = r8(b, o);
            for (i, exit) in rec.exits.iter_mut().enumerate() {
                *exit = r8(b, o + 1 + i);
            }
        }
    }

    // = seg000:b2aa suspend_clock_and_push_menu — suspend_game_clock, then
    // loc_0d323: push `menu` with resume_game_clock as its cleanup and request
    // the overlay transition.
    pub(crate) fn suspend_clock_and_push_menu(&mut self, menu: MenuRef) {
        self.suspend_game_clock();
        self.screen_overlay_request_transition();
        self.menu_stack_push(menu, Some(GameState::resume_game_clock));
    }

    // = seg000:b389 create_save_cl — write slot `slot`'s save file: the u16
    // game_time header, then the RLE-compressed image.
    pub(crate) fn save_game(&self, slot: u8) -> io::Result<()> {
        self.save_game_to(Path::new(&save_game_filename(slot)))
    }

    // = seg000:f27c write_file_from_esdi — the create + write + close that
    // create_save_cl jumps into (seg000:b3ad).
    pub(crate) fn save_game_to(&self, path: &Path) -> io::Result<()> {
        let image = self.create_save_in_memory();
        let mut data = Vec::with_capacity(2 + image.len());
        // = seg000:b393..b39d — stamp game_time at file offset 0.
        data.extend_from_slice(&self.game_time.to_le_bytes());
        // = seg000:b39e call compress_sav; b3aa..f299 create + write the file.
        data.extend_from_slice(&compress_sav(&image));
        std::fs::write(path, &data)
    }

    // = seg000:b3ba..b3ef — the file half of the load path: read the slot's
    // file, decompress the block past the 2-byte header, scatter it back.
    pub(crate) fn load_game(&mut self, slot: u8) -> io::Result<()> {
        self.load_game_from(Path::new(&save_game_filename(slot)))
    }

    // = seg000:f255 open_nonres_file / seg000:f204 open_file_and_get_size / seg000:f260 read_ffff_to_esdi_and_close
    // — open the loose save file, read it whole and close it.
    pub(crate) fn load_game_from(&mut self, path: &Path) -> io::Result<()> {
        let data = std::fs::read(path)?;
        if data.len() < 6 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated save"));
        }
        // = seg000:b3d4..b3e8 — the game_time header is skipped (it only feeds
        // the load menu); decompress_sav rebuilds the 0x567a-byte image.
        let image = decompress_sav(&data[2..]);
        if image.len() != IMAGE_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad save image size {:#x}", image.len()),
            ));
        }
        // = seg000:b3ed call restore_from_save_memory.
        self.restore_from_save_memory(&image);
        Ok(())
    }

    // = seg000:b30f load_save_game_timestamp (one slot's read): the u16
    // game_time header of the slot's file, None when the file is missing or
    // short.
    pub(crate) fn save_game_timestamp(slot: u8) -> Option<u16> {
        Self::save_game_timestamp_path(Path::new(&save_game_filename(slot)))
    }

    // Port-only: the u16 game_time header of any save file (custom saves
    // included), None when the file is missing or short.
    pub(crate) fn save_game_timestamp_path(path: &Path) -> Option<u16> {
        let data = std::fs::read(path).ok()?;
        Some(u16::from_le_bytes([*data.first()?, *data.get(1)?]))
    }

    // = seg000:b2cd loc_0b2cd — patch a save-slot label ("Log N: DAY  d /
    // hh.mm x.m.") in the COMMAND.BIN buffer in place: the day into the 3-wide
    // field ending at the label's second digit run, then the 10-char
    // time-of-day text (string 0x117's slot time & 0xf) three chars later.
    // Labels without a second digit run (LAST ENTERING ...) are left alone.
    pub(crate) fn patch_save_slot_label(&mut self, text_id: u16, time: u16) {
        // = seg000:b2d1 and ax,0fffh; call get_phrase_or_command_string_si.
        let id = text_id & 0xfff;
        if id == 0 {
            return;
        }
        // = seg000:b2f5..b307 — the time-of-day source: 16 ten-char slots in
        // string 0x117, indexed by the time-of-day nibble.
        let time_of_day = {
            let table = self.get_phrase_or_command_string(0x117);
            let lo = (time & 0xf) as usize * 10;
            if table.len() < lo + 10 {
                return;
            }
            table[lo..lo + 10].to_vec()
        };
        let (ofs, end) = container::entry_byte_range(&self.command_bin, id - 1);
        let s = &mut self.command_bin[ofs as usize..end as usize];
        // = seg000:b2d9/b2dc find_last_numeric_digit_in_str_at_es_si twice —
        // one past the second digit run (the DAY field).
        let mut si = 0usize;
        for _ in 0..2 {
            let Some(first) = s[si..].iter().position(u8::is_ascii_digit) else {
                return;
            };
            si += first;
            si += s[si..]
                .iter()
                .position(|c| !c.is_ascii_digit())
                .unwrap_or(s.len() - si);
        }
        if si < 3 || s.len() < si + 13 {
            return;
        }
        // = seg000:b2df..b2ef — day = ((game_time + 3) >> 4) + 1, written by
        // string_replace_number_ending_at_es_si's 3-wide space-padded field.
        let day = (time.wrapping_add(3) >> 4).wrapping_add(1).min(999);
        let digits = [day / 100, day / 10 % 10, day % 10];
        let mut leading = true;
        for (i, d) in digits.iter().enumerate() {
            leading &= *d == 0;
            s[si - 3 + i] = if leading && i < 2 {
                b' '
            } else {
                b'0' + *d as u8
            };
        }
        // = seg000:b2f2 lea di,[si+3]; b304 rep movsb — the time-of-day text.
        s[si + 3..si + 13].copy_from_slice(&time_of_day);
    }

    // = seg000:b30f load_save_game_timestamp — refresh a save/load submenu's
    // slot rows from the slot files: patch each row's day/time label from the
    // file's header word, and fold the file-exists result into the row flags
    // (`flag_mask` = DOS's cx): 0x8000 marks existing saves on the save menu,
    // 0x4000 greys missing slots on the load menu (the seg000:b33c..b345
    // `sbb/not/and` combination).
    fn refresh_save_slot_rows(&mut self, records: &mut [MenuItem], flag_mask: u16) {
        for rec in records.iter_mut() {
            let id = rec.text_id & 0xfff;
            if !(0x10f..=0x112).contains(&id) {
                continue;
            }
            let slot = (id - 0x10f) as u8;
            match Self::save_game_timestamp(slot) {
                Some(time) => {
                    self.patch_save_slot_label(rec.text_id, time);
                    if flag_mask == CMD_HIGHLIGHT {
                        rec.text_id |= flag_mask;
                    }
                }
                None => {
                    if flag_mask == CMD_GREY {
                        rec.text_id |= flag_mask;
                    }
                }
            }
        }
    }

    // = seg000:b28c menu_callback_choice_mirror_room_save_game — the SAVE GAME
    // verb: suspend the game clock, stage the two-slot save submenu (existing
    // slots highlighted, labels showing each save's day/time) and fold it in.
    pub(crate) fn menu_callback_choice_mirror_room_save_game(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:b292/b295 cx = 8000h, si = the slot rows; call
        //   load_save_game_timestamp.
        let mut records = menu_defs::MENU_SAVE_GAME.records.to_vec();
        self.refresh_save_slot_rows(&mut records, CMD_HIGHLIGHT);
        self.menu_save_game.records = records;
        // = seg000:b28f call suspend_clock_and_push_menu, then fold it onto
        //   the screen (the b29b redraw_active_command_menu tail).
        self.suspend_clock_and_push_menu(MenuRef::MenuSaveGame);
        self.play_pending_panel_fold();
    }

    // = seg000:b29e menu_callback_choice_mirror_room_load_game — the LOAD GAME
    // verb: same staging with the four-slot load submenu (the two manual slots
    // plus the LAST ENTERING autosaves), missing slots greyed.
    pub(crate) fn menu_callback_choice_mirror_room_load_game(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:b29e/b2a1 cx = 4000h, si = the slot rows.
        let mut records = menu_defs::MENU_LOAD_GAME.records.to_vec();
        self.refresh_save_slot_rows(&mut records, CMD_GREY);
        self.menu_load_game.records = records;
        // = seg000:b2a7 bp = menu_globe_load_game; falls into suspend_clock_and_push_menu.
        self.suspend_clock_and_push_menu(MenuRef::MenuLoadGame);
        self.play_pending_panel_fold();
    }

    // = seg000:b35a menu_callback_choice_globe_save_game — a save-slot row:
    // save the game to that slot, show the status line, and close the submenu
    // on success. `index` is DOS's cx (the menu row = the save slot),
    // `text_id` its ax (the row's label).
    pub(crate) fn menu_callback_choice_globe_save_game(&mut self, text_id: u16, index: usize) {
        let slot = index as u8;
        // = seg000:b35a — record game_time as the last-save stamp (_unk_2CCC6,
        //   what loc_0b2cd formats); the port passes it straight through.
        // = seg000:b362 call call_restore_cursor.
        self.call_restore_cursor();
        // = seg000:b365 call loc_0b2cd; b369 call draw_command_menu_item —
        //   stamp the fresh day/time into this slot's label and redraw the row.
        self.patch_save_slot_label(text_id, self.game_time);
        self.draw_command_menu_item(slot, text_id);
        // = seg000:b36d call create_save_cl.
        let result = self.save_game(slot);
        // = seg000:b371..b379 ax = 0x113 + CF — " SAVE SUCCESSFUL" or
        //   " *** SAVE ERROR " on row 4; b37c wait 300 ticks (interruptable).
        let status = if result.is_ok() { 0x113 } else { 0x114 };
        self.draw_command_menu_item(4, status);
        self.wait_interruptable(300);
        // = seg000:b382/b385 — close the submenu on success; on error leave it
        //   up (the seg000:b388 ret).
        if result.is_ok() {
            self.menu_callback_choice_exit_menu(0, 0);
        }
    }

    // = seg000:b3b0..b3cd — the pre-load half shared by the slot rows and the
    // custom save panel: drain a pending room-screen request and hand back the
    // view toggle to preserve across the restore.
    pub(crate) fn pre_load_fixups(&mut self) -> u8 {
        // = seg000:b3b0..b3b7 loc_00e49 — drain a pending room-screen request
        //   first: play DEAD2.HNM (video 0dh) over the game-over screen
        //   (play_hnm_after_lip_sync_stop), then clear the request and the
        //   lip-sync id.
        if self.pending_room_screen_request != 0 {
            self.play_hnm_after_lip_sync_stop(0x0d);
            self.pending_room_screen_request = 0;
            self.current_lip_sync_resource_id = 0;
        }
        // = seg000:b3cd push room_view_toggle — the load keeps the current
        //   view toggle (the saved byte is discarded at seg000:b401).
        self.room_view_toggle
    }

    // = seg000:b3f1..b424 — the post-load half: restore the view toggle, clear
    // the per-scene transients, release the suspend nesting, and rebuild the
    // active screen.
    pub(crate) fn post_load_fixups(&mut self, toggle: u8) {
        self.room_view_toggle = toggle;
        // = seg000:b3f1 call loc_03ae9 — clear the person screen-pos markers.
        self.character_screen_pos = [(0xffff, 0xffff); 0x17];
        // = seg000:b3f4 call clear_frame_tasks.
        self.remove_all_frame_tasks();
        // = seg000:b3f7 talking_head_id = 0xffff — drop any active head.
        self.talking_head = None;
        // = seg000:b3fd call reset_game_suspend (also releases the save
        //   menu's suspend_game_clock nesting).
        self.reset_game_suspend();
        // = seg000:b404 or al,al; jns loc_0b41b — rebuild the view the player
        //   was in when they loaded.
        if (toggle as i8) >= 0 {
            // = seg000:b41b..b424 the room path: drop the transient overlays,
            //   refresh the date/time indicator, fade the song out
            //   (midi_begin_song_fade_out is not ported; draw_room_game_screen
            //   restarts the room music), and rebuild the room screen.
            self.dismiss_stacked_menus();
            self.ui_redraw_date_and_time_indicator();
            self.draw_room_game_screen();
        } else {
            // = seg000:b408..b418 the globe path (globe_slide_decorations_
            //   close + see_results + the rotation frame task); the port's
            //   map-view rebuild is the closest ported equivalent.
            self.ui_show_globe_map_view();
        }
    }

    // = seg000:b3b0 menu_callback_choice_globe_load_game — a load-slot row:
    // restore the slot's file and rebuild the active screen. `index` is DOS's
    // cx (the menu row = the load slot).
    pub(crate) fn menu_callback_choice_globe_load_game(&mut self, _text_id: u16, index: usize) {
        let slot = index as u8;
        let toggle = self.pre_load_fixups();
        // = seg000:b3c1..b3ed read + decompress + restore.
        if let Err(e) = self.load_game(slot) {
            println!("load_game (slot {slot}): {e}");
            return;
        }
        self.post_load_fixups(toggle);
    }

    // Port-only: format a save header word as "DAY nnn hh.mm x.m." for the
    // custom save panel's list rows. Day = ((time + 3) >> 4) + 1 (the
    // seg000:b2df math); the time-of-day text is COMMAND.BIN string 0x117's
    // ten-char slot indexed by time & 0xf (the seg000:b2f5 lookup). Returns
    // raw COMMAND.BIN bytes; trailing spaces are trimmed.
    pub(crate) fn format_save_day_time(&self, time: u16) -> Vec<u8> {
        let mut out = Vec::with_capacity(18);
        out.extend_from_slice(b"DAY ");
        let day = (time.wrapping_add(3) >> 4).wrapping_add(1).min(999);
        let mut leading = true;
        for d in [day / 100, day / 10 % 10, day % 10] {
            leading &= d == 0;
            if !leading {
                out.push(b'0' + d as u8);
            }
        }
        if leading {
            out.push(b'0');
        }
        out.push(b' ');
        let table = self.get_phrase_or_command_string(0x117);
        let lo = (time & 0xf) as usize * 10;
        if table.len() >= lo + 10 {
            out.extend_from_slice(&table[lo..lo + 10]);
        }
        while out.last() == Some(&b' ') {
            out.pop();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::{dat_file::DatFile, game_phase::PHASE_28_STILGAR_SIETCH_KNOWN};

    // Port-only diagnostic: diff the port's fresh-game image against a
    // dune37s0.sav written by the original (DUNE37S0-ORIGINAL.SAV in the
    // workspace root). Run with:
    //   cargo test -p dune --bin dune -- --ignored diff_initial_save --nocapture
    #[test]
    #[ignore = "needs assets/DUNE.DAT and DUNE37S0-ORIGINAL.SAV"]
    fn diff_initial_save_against_original() {
        use std::sync::mpsc;

        use crate::dat_file::DatFile;

        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let sav_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../DUNE37S0-ORIGINAL.SAV");
        let (Ok(dat_file), Ok(data)) = (DatFile::open(dat_path), std::fs::read(sav_path)) else {
            eprintln!("skipping: {dat_path} or {sav_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        let ours = game.initial_game_image.clone().expect("no initial image");
        let header = u16::from_le_bytes([data[0], data[1]]);
        let theirs = decompress_sav(&data[2..]);
        // Expected differences: seg001:0000 rand_bits (runtime random), the
        // packed map byte 0x317e (DOS packs 3 cells past the MAP resource),
        // and the dialogue-played log region past its terminator (DOS
        // snapshots initialize_resources' code bytes there; the port zeros).
        println!(
            "original: header game_time={header:#x} image={:#x} bytes",
            theirs.len()
        );
        println!(
            "port:     game_time={:#x} image={:#x} bytes",
            game.game_time,
            ours.len()
        );
        assert_eq!(theirs.len(), IMAGE_LEN);
        assert_eq!(ours.len(), IMAGE_LEN);
        let regions = [
            ("map overlay bits", 0usize, 0x317f, 0usize, "map"),
            ("dialogue-played log", 0x317f, 0xa2, 0xaa, "cs"),
            ("DIALOGUE snapshot", 0x3221, 0x11f8, 0xaa76, "seg001"),
            ("game-state block", 0x4419, 0x1261, 0, "seg001"),
        ];
        for (name, start, len, base, seg) in regions {
            let a = &theirs[start..start + len];
            let b = &ours[start..start + len];
            let diffs = a.iter().zip(b).filter(|(x, y)| x != y).count();
            println!("== {name}: {diffs} differing bytes of {len:#x}");
            let mut i = 0;
            let mut shown = 0;
            while i < len && shown < 60 {
                if a[i] != b[i] {
                    let run_start = i;
                    while i < len && a[i] != b[i] && i - run_start < 16 {
                        i += 1;
                    }
                    let addr = base + run_start;
                    println!(
                        "  {seg}:{addr:04x} (+{run_start:#06x}) orig={:02x?} port={:02x?}",
                        &a[run_start..i],
                        &b[run_start..i]
                    );
                    shown += 1;
                } else {
                    i += 1;
                }
            }
        }
    }

    #[test]
    fn rle_matches_dos_encoding() {
        // Runs of 1-2 stay literal; runs of >= 3 and the escape byte escape.
        assert_eq!(compress_sav(&[5, 5])[4..], [5, 5]);
        assert_eq!(compress_sav(&[1, 1, 1, 1])[4..], [RLE_ESCAPE, 4, 1]);
        assert_eq!(
            compress_sav(&[RLE_ESCAPE])[4..],
            [RLE_ESCAPE, 1, RLE_ESCAPE]
        );
        // Header: escape word (clean high byte), block length including the
        // 4-byte header itself.
        let block = compress_sav(&[9]);
        assert_eq!(block, [RLE_ESCAPE, 0, 5, 0, 9]);
        // A run longer than 255 splits.
        let long = vec![7u8; 300];
        assert_eq!(
            compress_sav(&long)[4..],
            [RLE_ESCAPE, 255, 7, RLE_ESCAPE, 45, 7]
        );
    }

    #[test]
    fn rle_round_trip() {
        let mut data = Vec::new();
        for i in 0..2000usize {
            // A mix of runs, literals and escape bytes.
            let b = match i % 7 {
                0..3 => 0xaa,
                3 => RLE_ESCAPE,
                _ => (i * 13) as u8,
            };
            data.push(b);
        }
        data.extend_from_slice(&[0; 400]);
        let block = compress_sav(&data);
        assert_eq!(decompress_sav(&block), data);
    }

    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn save_image_layout_and_round_trip() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.initialize_resources();

        // Mutate a spread of state the save must carry.
        game.game_time = 0x123;
        game.charisma = 77;
        game.spice_in_stock = 4321;
        game.game_phase = PHASE_28_STILGAR_SIETCH_KNOWN;
        game.troops[3].occupation = 0x12;
        game.troops[3].population = 111;
        game.locations[7].spice_amount = 55;
        game.locations[7].status |= 0x20;
        game.dialogue_played_log.push(0x1234);
        game.dialogue_played_log.push(0x0872);
        game.comm_sightings.push(0x280a);
        game.vision_messages.push((0x105, 0));
        // A vegetation overlay mark (bits 5-4 of a map cell).
        game.map[0x1000] = game.map[0x1000] & 0xcf | 0x10;
        // A spoken-line flag inside the DIALOGUE buffer.
        let spoken_ofs = container::entry_offset(&game.dialogue, 4) as usize;
        game.dialogue[spoken_ofs] |= 0x80;
        // The palace scene records the phase callbacks patch: the stillsuit
        // room's background (callback 4) and its unlocked west door
        // (callback 8).
        assert_eq!(game.scene_records[1].background, 0x3a);
        assert_eq!(game.scene_records[1].exits[3], 0x8c);
        game.scene_records[1].background = 0x39;
        game.scene_records[1].exits[3] = 0x0c;

        let image = game.create_save_in_memory();
        assert_eq!(image.len(), IMAGE_LEN);
        // game_time at state offset 2, the locations sentinel, troop 0's id.
        assert_eq!(r16(&image, OFS_STATE + 0x0002), 0x123);
        assert_eq!(r16(&image, OFS_STATE + 0x08a8), 0xffff);
        assert_eq!(r8(&image, OFS_STATE + 0x08aa), game.troops[0].troop_id);
        // palace_rooms[1] at seg001:122a: background, then the N/E/S/W exits.
        assert_eq!(
            &image[OFS_STATE + 0x122a..OFS_STATE + 0x122f],
            &[0x39, 0x07, 0x00, 0x01, 0x0c]
        );
        // The packed vegetation mark: cell 0x1000 is group 0x400, slot 0
        // (bits 7-6).
        assert_eq!(image[0x1000 / 4] >> 6 & 3, 1);
        // The DIALOGUE region carries the relocated offset table.
        assert_eq!(
            r16(&image, OFS_DIALOGUE),
            r16(&game.dialogue, 0).wrapping_add(DIALOGUE_BASE)
        );

        // Round-trip through a file into a fresh GameState.
        let path = std::env::temp_dir().join("dune37s_test.sav");
        game.save_game_to(&path).expect("save");
        let raw = std::fs::read(&path).expect("read back");
        assert_eq!(u16::from_le_bytes([raw[0], raw[1]]), 0x123);

        let dat_file = DatFile::open(dat_path).expect("reopen DUNE.DAT");
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut fresh = GameState::new(dat_file, tx);
        fresh.set_headless();
        fresh.initialize_resources();
        fresh.load_game_from(&path).expect("load");
        std::fs::remove_file(&path).ok();

        assert_eq!(fresh.game_time, 0x123);
        assert_eq!(fresh.charisma, 77);
        assert_eq!(fresh.spice_in_stock, 4321);
        assert_eq!(fresh.game_phase, PHASE_28_STILGAR_SIETCH_KNOWN);
        assert_eq!(fresh.troops[3].occupation, 0x12);
        assert_eq!(fresh.troops[3].population, 111);
        assert_eq!(fresh.locations[7].spice_amount, 55);
        assert_eq!(fresh.locations[7].status, game.locations[7].status);
        // The first two entries are the startup narration pages initialize_
        // resources seeds (= seg000:00b9/00bc), ahead of the two test pushes.
        assert_eq!(
            fresh.dialogue_played_log,
            vec![0x8456, 0x8457, 0x1234, 0x0872]
        );
        assert_eq!(fresh.comm_sightings, vec![0x280a]);
        assert_eq!(fresh.data_000c8, 1);
        assert_eq!(fresh.vision_messages, vec![(0x105, 0)]);
        assert_eq!(fresh.map[0x1000] & 0x30, 0x10);
        assert_eq!(fresh.scene_records[1].background, 0x39, "stillsuits shown");
        assert_eq!(fresh.scene_records[1].exits[3], 0x0c, "west door unlocked");
        assert_eq!(fresh.dialogue[spoken_ofs] & 0x80, 0x80);
        // The whole dialogue buffer survives the relocate/unrelocate pair.
        assert_eq!(fresh.dialogue, game.dialogue);
        // And a re-save of the loaded state reproduces the image bit for bit.
        assert_eq!(fresh.create_save_in_memory(), image);
    }
}
