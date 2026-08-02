use std::io::Cursor;

use bytes_ext::ReadBytesExt;

use crate::{GameState, container};

fn condit_var_name(addr: u16) -> Option<(&'static str, bool)> {
    Some(match addr {
        0x00 => ("rand_bits", true),
        0x02 => ("game_time", true),
        0x04 => ("location_and_room", true),
        0x06 => ("location_appearance", true),
        0x08 => ("data_00008", false),
        0x09 => ("data_00009", false),
        0x0a => ("bitfield_paul_events", false),
        0x0b => ("current_room", false),
        0x0c => ("pending_destination_room", false),
        0x0d => ("previous_room", false),
        0x0e => ("persons_met", true),
        0x10 => ("persons_travelling_with", true),
        0x12 => ("persons_in_room", true),
        0x14 => ("persons_talking_to", true),
        0x19 => ("line_spoken_this_conversation", false),
        0x1b => ("stay_here_come_with_me_count", false),
        0x23 => ("pending_room_action", false),
        0x2c => ("troop.offset_of_location", true),
        0x2e => ("troop.troop_id", false),
        0x2f => ("troop.occupation_low", false),
        0x30 => ("troop.occupation", false),
        0x31 => ("troop.dissatisfaction_low", false),
        0x32 => ("troop.bitfield_10", true),
        0x34 => ("troop.dissatisfaction", true),
        0x36 => ("troop.motivation_modifier", false),
        0x37 => ("troop.skill_in_occupation", false),
        0x38 => ("troop.spice_skill", false),
        0x39 => ("troop.army_skill", false),
        0x3a => ("troop.ecology_skill", false),
        0x3b => ("troop.equipment", false),
        0x3c => ("troop.population", false),
        0x40 => ("troop.days_since_ralliement", false),
        0x42 => ("troop.time_periods_since_ralliement", true),
        0x48 => ("troop.harvest_rate", true),
        0x4c => ("contacting_troops_ds_4c", false),
        0x4d => ("location.appearance", false),
        0x4e => ("location.area_and_name", true),
        0x51 => ("location.status", false),
        0x52 => ("location.spice_density", false),
        0x54 => ("location.water", false),
        0xa2 => ("area_controlled_by_atreides", true),
        0xa4 => ("area_controlled_by_harkonnen", true),
        0xa6 => ("todays_spice_production", true),
        0xa8 => ("harkonnen_spice_production", true),
        0xaa => ("data_000aa", true),
        0xae => ("previous_day_spice_production", true),
        0xb0 => ("spice_production_better_than_previous_day", true),
        0xb2 => ("spice_production_lower_than_previous_day", true),
        0xbc => ("spice_shipment_quantity", true),
        0xbe => ("spice_shipment_fulfilment", false),
        0xbf => ("spice_shipment_flags", false),
        0xc2 => ("final_attack_stage", false),
        0xc3 => ("spice_shipment_sequence_number", false),
        0xc4 => ("number_of_sietches_attacked_by_harkonnen", false),
        0xca => ("nearest_location.distance", true),
        0xcc => ("nearest_location.loc_ptr", true),
        0xce => ("nearest_location.octant", false),
        0xcf => ("days_left_until_spice_shipment", false),
        0xd0 => ("nearest_village.distance", true),
        0xd2 => ("nearest_village.loc_ptr", true),
        0xd4 => ("nearest_village.octant", false),
        0xd5 => ("contact_distance_related_ds_d5", false),
        0xd6 => ("nearest_sietch.distance", true),
        0xd8 => ("nearest_sietch.loc_ptr", true),
        0xda => ("nearest_sietch.octant", false),
        0xdc => ("nearest_atreides_area.distance", true),
        0xde => ("nearest_atreides_area.loc_ptr", true),
        0xe0 => ("nearest_atreides_area.octant", false),
        0xe2 => ("nearest_harkonnen_area.distance", true),
        0xe4 => ("nearest_harkonnen_area.loc_ptr", true),
        0xe6 => ("nearest_harkonnen_area.octant", false),
        0xf5 => ("for_condit_desert_walk_ds_f5", false),
        0xf8 => ("number_of_locations_with_illness", false),
        0xf9 => ("chani_troop_illness_cure_progress", false),
        0xfe => ("game_phase_copy_ds_fe", false),
        0x25 => ("number_of_sietches_visited", false),
        0x26 => ("entering_new_sietch", false),
        0x27 => ("discovered_sietch_count", false),
        0x28 => ("number_of_rallied_troops", false),
        0x29 => ("charisma", false),
        0x2a => ("game_phase", false),
        0x2b => ("night_attack_stage", false),
        0xac => ("data_000ac", true),
        0xc5 => ("person_marker_base", false),
        0xc6 => ("data_000c6", false),
        0xc8 => ("data_000c8", false),
        0xe1 => ("data_000e1", false),
        0xe8 => ("ui_hud_head_index", false),
        0xea => ("data_000ea", false),
        0xed => ("data_000ed", false),
        0xee => ("data_000ee", true),
        0xf4 => ("desert_walk_counter", false),
        0xfb => ("room_view_toggle", false),
        0xff => ("days_since_phase_change", false),
        // 0xfc => ("data_000fc", true),
        _ => return None,
    })
}

impl GameState {
    fn condit_ds_byte(&self, addr: u16) -> Option<u8> {
        Some(match addr {
            // = seg001:0008 data_00008 — the current room/apparence selector
            // byte (0xff = no room scene to draw).
            0x08 => self.data_00008,
            // = seg001:0009 data_00009 — the current location slot byte
            // (0xff while out in the desert).
            0x09 => self.data_00009,
            // = seg001:000a bitfield_Paul_events — Paul's story-progress bits;
            // bit 0x10 (met Stilgar) gates the army-recruit dialogue lines
            // (e.g. the WORK WITH ME refusal/acceptance conditions).
            0x0a => self.bitfield_paul_events,
            // = seg001:000b current_room.
            0x0b => self.current_room,
            // = seg001:000c pending_destination_room — condition 0x1c (Leto's
            // room-leave line) tests it == 4.
            0x0c => self.pending_destination_room,
            // = seg001:000d previous_room.
            0x0d => self.previous_room,
            // = seg001:0019 line_spoken_this_conversation — 0 = no line spoken
            // yet, 0xff once any line is presented. A fallback dialogue line's
            // condition tests it == 0, so the fallback presents only when no
            // other line was presentable.
            0x19 => self.line_spoken_this_conversation,
            // = seg001:001b related_to_stay_here_come_with_me_ds_1b — the
            // COME WITH ME / STAY HERE use counter (cleared by TALK TO ME).
            0x1b => self.data_0001b,
            // = seg001:0023 pending_room_action — the room-leave / dialogue-scan state;
            // condition 0x1c tests it == 1.
            0x23 => self.pending_room_action,
            // = seg001:0025 number_of_sietches_visited / 0026
            // entering_new_sietch — the first-visit state ui_click_move_room
            // maintains.
            0x25 => self.number_of_sietches_visited,
            0x26 => self.entering_new_sietch,
            // = seg001:0027 discovered_sietch_count — counts sietches whose
            // location has been discovered (bumped by seg000:426f).
            0x27 => self.discovered_sietch_count,
            // = seg001:0028 number_of_rallied_troops — conditions 4/5/7 gate
            // early-game Leto lines on it. The troop-rally system that bumps
            // it is not yet ported.
            0x28 => self.number_of_rallied_troops,
            // = seg001:0029 charisma.
            0x29 => self.charisma,
            // = seg001:002a game_phase.
            0x2a => self.game_phase,
            // = seg001:002b night_attack_stage.
            0x2b => self.night_attack_stage,
            // = seg001:002e..0041 the staged troop block (troop_prepare_troop_
            // data_for_condit, troops.rs).
            0x2e => self.troop_condit.troop_id,
            0x2f => self.troop_condit.occupation_low,
            0x30 => self.troop_condit.occupation,
            0x31 => self.troop_condit.dissatisfaction_low,
            0x36 => self.troop_condit.motivation_modifier,
            0x37 => self.troop_condit.skill_in_occupation,
            0x38 => self.troop_condit.spice_skill,
            0x39 => self.troop_condit.army_skill,
            0x3a => self.troop_condit.ecology_skill,
            0x3b => self.troop_condit.equipment,
            0x3c => self.troop_condit.population,
            0x40 => self.troop_condit.days_since_ralliement,
            0x41 => self.troop_condit.game_days_since_ralliement,
            // = seg001:004c related_to_contacting_troops_ds_4c — 0xff while
            // the contacted troop answers from outside the visibility range.
            0x4c => self.contacting_troops_ds_4c,
            // = seg001:004d..005b the staged location block (prepare_location_
            // data_for_condit, troops.rs).
            0x4d => self.location_condit.appearance,
            0x50 => self.location_condit.worm_event_likelihood,
            0x51 => self.location_condit.status,
            0x52 => self.location_condit.spice_density,
            0x53 => self.location_condit.unused_equipment,
            0x54 => self.location_condit.water,
            0x55..=0x5b => self.location_condit.equipment[(addr - 0x55) as usize],
            // = seg001:00be/00bf the spice-shipment fulfilment fraction and
            // flags (events.rs).
            0xbe => self.spice_shipment_fulfilment,
            0xbf => self.spice_shipment_flags,
            // = seg001:00c2 final_attack_stage_ds_c2.
            0xc2 => self.final_attack_stage,
            // = seg001:00c3 spice_shipment_sequence_number_ds_c3.
            0xc3 => self.spice_shipment_sequence_number,
            // = seg001:00c4 number_of_sietches_attacked_by_Harkonnen_ds_c4.
            0xc4 => self.number_of_sietches_attacked_by_harkonnen,
            // = seg001:00c5 person_marker_base.
            0xc5 => self.person_marker_base,
            // = seg001:00c6 data_000c6.
            0xc6 => self.data_000c6,
            // = seg001:00c8 data_000c8 — DOS's comm_sighting_count byte,
            // kept in step with comm_sightings.
            0xc8 => self.data_000c8,
            // = seg001:00ce..00e6 the nearest-location compass octants
            // (condit_scan_nearest_locations, troops.rs).
            0xce => self.nearest_location.octant,
            0xd4 => self.nearest_village.octant,
            0xda => self.nearest_sietch.octant,
            0xe0 => self.nearest_atreides_area.octant,
            0xe6 => self.nearest_harkonnen_area.octant,
            // = seg001:00e1 data_000e1 — the fly-over side flag.
            0xe1 => self.data_000e1,
            // = seg001:00e8 ui_hud_head_index.
            0xe8 => self.ui_hud_head_index,
            // = seg001:00ea data_000ea (signed).
            0xea => self.data_000ea as u8,
            // = seg001:00ed data_000ed — the overpower-captain condit byte.
            0xed => self.data_000ed,
            // = seg001:00cf days_left_until_spice_shipment.
            0xcf => self.days_left_until_spice_shipment,
            // = seg001:00d5 contact_distance_related_ds_d5.
            0xd5 => self.contact_distance_related_ds_d5,
            // = seg001:00f4 desert_walk_counter.
            0xf4 => self.desert_walk_counter,
            // = seg001:00f5 for_condit_desert_walk_related_ds_f5.
            0xf5 => self.for_condit_desert_walk_ds_f5,
            // = seg001:00f8/00f9 the illness-plot counters (events.rs).
            0xf8 => self.number_of_locations_with_illness,
            0xf9 => self.chani_troop_illness_cure_progress,
            // = seg001:00fb room_view_toggle.
            0xfb => self.room_view_toggle,
            // = seg001:00fc data_000fc.
            0xfc => self.data_000fc,
            // = seg001:00fe game_phase_copy_ds_fe.
            0xfe => self.game_phase_copy_ds_fe,
            // = seg001:00ff number_of_days_since_last_game_phase_change_ds_ff.
            0xff => self.days_since_last_game_phase_change,
            _ => return None,
        })
    }

    fn condit_ds_word(&self, addr: u16) -> Option<u16> {
        Some(match addr {
            // = seg001:0000 rand_bits — the rolling random-bit word (conditions
            // 0x25..0x28 pick a branch off its low bits).
            0x00 => self.rand_bits,
            // = seg001:0002 game_time — the in-game clock (16 ticks per day).
            0x02 => self.game_time,
            // = seg001:0004 location_and_room — the current scene's
            // (location << 8) | room code.
            0x04 => self.location_and_room,
            // = seg001:0006 location_appearance — the current location
            // slot/index.
            0x06 => self.location_appearance,
            // = seg001:000e persons_met / 0010 persons_travelling_with / 0012
            // persons_in_room / 0014 persons_talking_to — the person bitmasks
            // several conditions test.
            0x0e => self.persons_met,
            0x10 => self.persons_travelling_with,
            0x12 => self.persons_in_room,
            0x14 => self.persons_talking_to,
            // = seg001:002c..004a the staged troop block words.
            0x2c => self.troop_condit.offset_of_location,
            0x32 => self.troop_condit.bitfield_10,
            0x34 => self.troop_condit.dissatisfaction_and_speech,
            0x42 => self.troop_condit.time_periods_since_ralliement,
            0x44 => self.troop_condit.harvest_rate,
            0x46 => self.troop_condit.harvest_total,
            0x48 => self.troop_condit.ds_48,
            0x4a => self.troop_condit.ds_4a,
            // = seg001:004e the staged location area+name word.
            0x4e => self.location_condit.area_and_name,
            // = seg001:00a0 spice_in_stock — the player's spice, in 10 kg
            //   batches.
            0xa0 => self.spice_in_stock,
            // = seg001:00a2..00b2 the daily statistics block (events.rs).
            0xa2 => self.area_controlled_by_atreides,
            0xa4 => self.area_controlled_by_harkonnen,
            0xa6 => self.todays_spice_production,
            0xa8 => self.harkonnen_spice_production,
            // = seg001:00aa data_000aa — total population of the loyal
            // troops.
            0xaa => self.data_000aa,
            // = seg001:00ac data_000ac — total population of the
            // allegiance-flagged troops.
            0xac => self.data_000ac,
            0xae => self.previous_day_spice_production,
            0xb0 => self.spice_production_better_than_previous_day,
            0xb2 => self.spice_production_lower_than_previous_day,
            // = seg001:00bc spice_shipment_quantity.
            0xbc => self.spice_shipment_quantity,
            // = seg001:00ca..00e4 the nearest-location distances and ptrs
            // (condit_scan_nearest_locations, troops.rs); ds:d6 gates the
            // "There is a sietch very near" messages.
            0xca => self.nearest_location.distance,
            0xcc => self.nearest_location.loc_ptr,
            0xd0 => self.nearest_village.distance,
            0xd2 => self.nearest_village.loc_ptr,
            0xd6 => self.nearest_sietch.distance,
            0xd8 => self.nearest_sietch.loc_ptr,
            0xdc => self.nearest_atreides_area.distance,
            0xde => self.nearest_atreides_area.loc_ptr,
            0xe2 => self.nearest_harkonnen_area.distance,
            0xe4 => self.nearest_harkonnen_area.loc_ptr,
            // = seg001:00ee data_000ee — the overpower-captain condit word.
            0xee => self.data_000ee,
            _ => return None,
        })
    }

    pub(crate) fn condit_ds_read(&self, addr: u16, word: bool) -> u16 {
        let value = if word {
            self.condit_ds_word(addr)
        } else {
            self.condit_ds_byte(addr).map(u16::from)
        };
        // Unmodelled addresses read as 0. Debug print when hunting gaps:
        if self.log_condit && value.is_none() {
            if let Some((name, _)) = condit_var_name(addr) {
                eprintln!("CONDIT: read of unmodelled ds:[{addr:#04x}:{name}] (word: {word})");
            } else {
                eprintln!("CONDIT: read of unmodelled ds:[{addr:#04x}] (word: {word})");
            }
        }
        value.unwrap_or(0)
    }

    // = seg000:a30b read_condit_operand.
    fn read_condit_operand(&self, c: &mut Cursor<&[u8]>) -> u16 {
        let b = c.read_u8().unwrap();

        if b < 0x80 {
            // = seg000:a311 — second byte is the ds offset of the variable.
            let addr = c.read_u8().unwrap() as u16;
            // = seg000:a31c mov ax,[bx] (16-bit var, b != 1) / seg000:a322
            // mov al,[bx]; xor ah,ah (8-bit var, b == 1).
            self.condit_ds_read(addr, b != 1)
        } else if b == 0x80 {
            // = seg000:a32c es:lodsb; xor ah,ah — 8-bit immediate.
            c.read_u8().unwrap() as u16
        } else {
            // = seg000:a331 es:lodsw — 16-bit immediate.
            c.read_le_u16().unwrap()
        }
    }

    // = seg000:a396 evaluate_condition.
    fn evaluate_condition(&self, index: u16) -> u16 {
        if index == 0 {
            return 0;
        }

        // = seg000:a39d les si,[res_condit]; add si,index*2; mov si,es:[si-2] —
        let entry = container::entry(&self.condit, index - 1);
        let mut c = Cursor::new(entry);

        // The scratch stack of (value, operator) frames the loose (0x80)
        // operators push (seg000:a3c0).
        let mut stack: Vec<(u16, u16)> = Vec::new();

        // = seg000:a3a7 — read the left operand into dx.
        let mut value = self.read_condit_operand(&mut c);

        // = seg000:a3ac loop — consume operator/operand pairs until 0xff.
        loop {
            let opcode = c.read_u8().unwrap();

            // = seg000:a3ae cmp al,0ffh; jz — end of expression.
            if opcode == 0xff {
                break;
            }

            if opcode & 0x80 != 0 {
                // = seg000:a3c0 — loose operator: push the accumulated value and
                // the operator, then start a fresh tight chain.
                stack.push((value, opcode as u16));
                value = self.read_condit_operand(&mut c);
            } else {
                // = seg000:a3b6 — tight operator: apply it immediately.
                let ax = self.read_condit_operand(&mut c);
                value = apply_operator(opcode as u16, value, ax);
            }
        }

        // = seg000:a3cb.
        if let Some(&(first, _)) = stack.first() {
            let mut acc = first;
            for i in 0..stack.len() {
                let op = stack[i].1;
                let rhs = stack.get(i + 1).map(|f| f.0).unwrap_or(value);
                acc = apply_operator(op, acc, rhs);
            }
            value = acc;
        }

        value
    }

    /// True when condition `index` holds (DOS: the `or dx,dx` non-zero test).
    /// With CONDIT not loaded, conditions read as always-true, matching the
    /// prior always-first-entry dialogue stub.
    pub(crate) fn condition_holds(&self, index: u16) -> bool {
        let holds = self.evaluate_condition(index) != 0;
        if self.log_condit {
            println!(
                "COND {:3} {}: {}",
                index,
                if holds { "HOLDS" } else { "FAILS" },
                self.format_condition(index)
            );
        }
        holds
    }

    pub fn format_condition(&self, index: u16) -> String {
        if index == 0 {
            return "<condition 0: inert, always 0>".into();
        }

        fn operand(c: &mut Cursor<&[u8]>) -> String {
            let b = c.read_u8().unwrap();
            if b < 0x80 {
                let addr = c.read_u8().unwrap() as u16;

                // = seg000:a318 — type byte 1 reads a byte, every other b <
                // 0x80 a word.
                let is_word = b != 1;
                let width = if is_word { "word" } else { "byte" };
                match condit_var_name(addr) {
                    Some((name, _)) => format!("{width}[{addr:04x}:{name}]"),
                    None => format!("{width}[{addr:#04x}]"),
                }
            } else if b == 0x80 {
                let v = c.read_u8().unwrap();
                if v <= 9 {
                    format!("{v}")
                } else {
                    format!("{v:#x}")
                }
            } else {
                let v = c.read_le_u16().unwrap();
                if v <= 9 {
                    format!("{v}")
                } else {
                    format!("{v:#x}")
                }
            }
        }

        fn op_symbol(opcode: u8) -> String {
            let sym = match opcode & 0x1f {
                0x00 => "==",
                0x02 => "<",
                0x04 => ">",
                0x06 => "!=",
                0x08 => "<=",
                0x0a => ">=",
                0x0c => "+",
                0x0e => "-",
                0x10 => "&",
                0x12 => "|",
                // The 0x14..0x1e slots fall into condit_operator_return_0.
                _ => return format!("?{:#04x}", opcode),
            };
            if opcode & 0x80 != 0 {
                format!("{sym}.")
            } else {
                sym.into()
            }
        }

        let entry = container::entry(&self.condit, index - 1);
        let mut c = Cursor::new(entry);

        let mut chains: Vec<(String, usize)> = Vec::new(); // (text, term count)
        let mut loose_ops: Vec<String> = Vec::new();
        let mut chain = operand(&mut c);
        let mut terms = 1usize;

        loop {
            let opcode = c.read_u8().unwrap();

            if opcode == 0xff {
                break;
            }
            if opcode & 0x80 != 0 {
                chains.push((std::mem::take(&mut chain), terms));
                loose_ops.push(op_symbol(opcode));
                chain = operand(&mut c);
                terms = 1;
            } else {
                chain = format!("{chain} {} {}", op_symbol(opcode), operand(&mut c));
                terms += 1;
            }
        }
        chains.push((chain, terms));

        let parenthesize = chains.len() > 1;
        let mut out = String::new();
        for (i, (text, terms)) in chains.iter().enumerate() {
            if i > 0 {
                out.push_str(&format!(" {} ", loose_ops[i - 1]));
            }
            if parenthesize && *terms >= 2 {
                out.push_str(&format!("({text})"));
            } else {
                out.push_str(text);
            }
        }
        out
    }
}

// = seg000:a334 evaluate_operator_bx_on_dx_and_ax.
fn apply_operator(op: u16, a: u16, b: u16) -> u16 {
    const TRUE: u16 = 0xffff;
    const FALSE: u16 = 0;
    match op & 0x1f {
        // = seg000:a348 cmpeq (jz).
        0x00 => {
            if a == b {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a34f cmple (jb — unsigned below).
        0x02 => {
            if a < b {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a356 cmpge (ja — unsigned above).
        0x04 => {
            if a > b {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a35d cmpne (jnz).
        0x06 => {
            if a != b {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a364 cmplt (jle — signed less-or-equal).
        0x08 => {
            if (a as i16) <= (b as i16) {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a36b cmpgt (jge — signed greater-or-equal).
        0x0a => {
            if (a as i16) >= (b as i16) {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a33c addition.
        0x0c => a.wrapping_add(b),
        // = seg000:a33f subtraction.
        0x0e => a.wrapping_sub(b),
        // = seg000:a342 and.
        0x10 => a & b,
        // = seg000:a345 or.
        0x12 => a | b,
        // = seg000:a36f condit_operator_return_0 (codes 0x14..0x1e).
        _ => 0,
    }
}
