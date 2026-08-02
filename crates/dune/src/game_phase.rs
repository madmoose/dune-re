//! The game-phase progression system: set_game_phase_and_trigger_callbacks
//! (seg000:121f) and the per-phase callbacks it dispatches from
//! array_callbacks_for_game_phase_change (seg000:11e9), plus their small
//! shared helpers (location-discovery lists, charisma, the vision-message
//! queue and the COMM-room sighting list).
//!
//! Mirrors the contiguous DOS block seg000:1011..123d (the callbacks and the
//! dispatcher) with the helpers it calls from further afield (seg000:26da,
//! 29ee..2a50, 40ae, 6f78). Still stubbed: start_scripted_dialogue
//! (seg000:1771, the cutscene_game_phase_* byte scripts), the troop-system
//! effects (motivation, worm-event likelihood, the phase-0x64 location scan),
//! the palace-plan locked-door icon-list truncation, and the string
//! substitution table.

use crate::{GameState, cmd};

impl GameState {
    // = seg000:121f set_game_phase_and_trigger_callbacks — raise game_phase to
    // `phase` (only upward: a lower or equal value returns), zero the
    // days-since-phase-change counter, run the game-phase trigger record, then
    // — for phases 4..0x6c — dispatch the per-phase callback. DOS re-reads
    // game_phase after the trigger record runs (seg000:1230), so a trigger
    // that bumps the phase further selects the newer callback.
    pub(crate) fn set_game_phase_and_trigger_callbacks(&mut self, phase: u8) {
        // = seg000:121f cmp al,[game_phase]; jbe ret.
        if phase <= self.game_phase {
            return;
        }
        // = seg000:1225/1228 commit the phase, ds:ff = 0.
        self.game_phase = phase;
        self.days_since_last_game_phase_change = 0;
        // = seg000:122d call run_game_phase_triggers.
        self.run_game_phase_triggers();
        // = seg000:1230..123d bl = [game_phase]; above 0x6c -> no callback;
        //   else call cs:[11e7 + phase/2] (array_callbacks_for_game_phase_
        //   change — real phases are multiples of 4, entry = phase/4 - 1).
        match self.game_phase {
            0x04 => self.phase_callback_04_tuono_tabr(),
            0x08 => self.phase_callback_08(),
            0x0c => self.phase_callback_0c(),
            0x10 => self.phase_callback_10(),
            0x14 => self.phase_callback_14(),
            // = seg000:10b7 callback_game_phase_change_18_24_3c_68_6c: ret.
            0x18 | 0x24 | 0x3c | 0x68 | 0x6c => {}
            0x1c => self.phase_callback_1c(),
            0x20 => self.phase_callback_20_make_thufir_hawat_visible(),
            0x28 => self.phase_callback_28_mark_sihaya_clam_on_map(),
            0x2c => self.phase_callback_2c_met_stilgar(),
            0x30 => self.phase_callback_30_baron_pretends_sietch_devastated(),
            0x34 => self.phase_callback_34(),
            0x38 => self.phase_callback_38(),
            0x40 => self.phase_callback_40(),
            0x44 => self.phase_callback_44_mark_oxtyn_tabr_on_map(),
            0x48 => self.phase_callback_48_met_chani(),
            0x4c => self.phase_callback_4c_leto_killed(),
            0x50 => self.phase_callback_50_after_riding_worm(),
            0x54 => self.phase_callback_54_greenhouse(),
            0x58 => self.phase_callback_58_met_liet_kynes(),
            0x5c => self.phase_callback_5c(),
            0x60 => self.phase_callback_60_go_find_chani(),
            0x64 => {
                // = seg000:11e6 -> 1f13 callback_game_phase_change_64_main_
                //   code — scan locations for the best-provisioned Atreides
                //   sietch (location_do_accumulation_on_troops, troop system)
                //   and station Chani there, recording it as a COMM sighting
                //   (0x2b0a). TODO: port with the troop system.
                println!("phase_callback_64: unported (needs the troop system)");
            }
            p if p > 0x6c => {}
            // A phase that is not a multiple of 4 would make DOS read a
            // misaligned word out of the callback table and call garbage; no
            // caller passes one.
            p => println!("set_game_phase_and_trigger_callbacks: no callback for phase 0x{p:02x}"),
        }
    }

    // = seg000:105b make_null_terminated_array_of_location_ptrs_discovered —
    // mark each listed location discovered (DOS walks a 0-terminated cs
    // pointer array; the port passes the location indices).
    fn mark_locations_discovered(&mut self, indices: &[usize]) {
        for &i in indices {
            // = seg000:1063 call location_mark_discovered.
            self.location_mark_discovered(i);
        }
    }

    // = seg000:101b move_Jessica_in_Atreides_palace — room_persons[1]
    // .location_and_room low byte = 9: Jessica moves to palace room 9.
    fn move_jessica_in_atreides_palace(&mut self) {
        let rp = &mut self.room_persons[1];
        rp.location_and_room = (rp.location_and_room & 0xff00) | 9;
    }

    // = seg000:1011 callback_game_phase_change_04_Tuono_Tabr_1 — the
    // stillsuit-maker stage: palace_rooms[1].background steps back one
    // sub-chunk, his locations (10, 17) appear on the map, and Jessica moves.
    fn phase_callback_04_tuono_tabr(&mut self) {
        // = seg000:1011 dec byte [palace_rooms[1]].
        self.scene_records[1].background = self.scene_records[1].background.wrapping_sub(1);
        // = seg000:1015 si = array_pointers_locations_found_by_meeting_
        //   stillsuit_maker (locations[10], locations[17]).
        self.mark_locations_discovered(&[10, 17]);
        // = seg000:1018 falls through into move_Jessica_in_Atreides_palace.
        self.move_jessica_in_atreides_palace();
    }

    // = seg000:1027 callback_game_phase_change_08 — unlock palace_rooms[1]'s
    // west exit (0x8c -> 0x0c) and refresh the compass arrows.
    fn phase_callback_08(&mut self) {
        self.scene_records[1].exits[3] &= 0x7f;
        // = seg000:102c jmp rebuild_and_draw_room_nav_panel.
        self.rebuild_and_draw_room_nav_panel();
    }

    // = seg000:102f callback_game_phase_change_0c — unlock palace_rooms[7]'s
    // east and palace_rooms[6]'s west exits, drop a palace-plan locked-door
    // icon, and play the scripted scene 0x1321.
    fn phase_callback_0c(&mut self) {
        self.scene_records[7].exits[1] &= 0x7f;
        self.scene_records[6].exits[3] &= 0x7f;
        // = seg000:1039 word [data_0121d] = 0xffff — truncate the
        //   _stru_206BB_icon_list at its sprite-5 record (the locked-door
        //   overlay icons). TODO: that icon list is not modelled.
        // = seg000:103f ax = 0x1321; jmp start_scripted_dialogue. TODO: the
        //   scripted-dialogue player (seg000:1771) is not ported.
        println!("phase_callback_0c: start_scripted_dialogue(0x1321) unported");
    }

    // = seg000:1045 callback_game_phase_change_10 — Leto moves to palace room
    // 5, Jessica to room 9, and the Emperor's whereabouts (location 1, person
    // 0x0b) reach the COMM room.
    fn phase_callback_10(&mut self) {
        let rp = &mut self.room_persons[0];
        rp.location_and_room = (rp.location_and_room & 0xff00) | 5;
        // = seg000:104a call move_Jessica_in_Atreides_palace.
        self.move_jessica_in_atreides_palace();
        // = seg000:104d ax = 0x10b; jmp comm_add_person_sighting.
        self.comm_add_person_sighting(0x10b);
    }

    // = seg000:1053 callback_game_phase_change_14 — Leto moves to palace room
    // 10 and Harah's locations (21..23) appear on the map.
    fn phase_callback_14(&mut self) {
        let rp = &mut self.room_persons[0];
        rp.location_and_room = (rp.location_and_room & 0xff00) | 0x0a;
        // = seg000:1058 si = array_pointers_locations_found_by_meeting_Harah.
        self.mark_locations_discovered(&[21, 22, 23]);
    }

    // = seg000:10a4 callback_game_phase_change_1c — unlock palace_rooms[6]'s
    // east exit, drop a locked-door icon, refresh the compass arrows.
    fn phase_callback_1c(&mut self) {
        self.scene_records[6].exits[1] &= 0x7f;
        // = seg000:10a9 word [data_01217] = 0xffff — the icon-list truncation
        //   (see phase_callback_0c). TODO: not modelled.
        // = seg000:10af jmp rebuild_and_draw_room_nav_panel.
        self.rebuild_and_draw_room_nav_panel();
    }

    // = seg000:10b2 callback_game_phase_change_20_make_Thufir_Hawat_visible —
    // ds:ffb = 1: the high byte of room_persons[2].location_slot goes 1, so
    // Thufir's entry can match a room (falls into the 18/24/... ret).
    fn phase_callback_20_make_thufir_hawat_visible(&mut self) {
        let rp = &mut self.room_persons[2];
        rp.location_appearance = (rp.location_appearance & 0x00ff) | 0x0100;
    }

    // = seg000:10b8 callback_game_phase_change_28_mark_Sihaya_Clam_on_map —
    // di = locations[64]; jmp location_mark_discovered.
    fn phase_callback_28_mark_sihaya_clam_on_map(&mut self) {
        self.location_mark_discovered(64);
    }

    // = seg000:10be callback_game_phase_change_2c_met_Stilgar — timestamp the
    // meeting, restation Gurney/Thufir/Jessica in the palace, rename Paul in
    // the string-substitution table, +20 charisma, Paul-event bit 0x10, and
    // Stilgar's locations appear on the map.
    fn phase_callback_2c_met_stilgar(&mut self) {
        // = seg000:10be data_01154 = game_time.
        self.harkonnen_raids_armed_after_game_time = self.game_time;
        // = seg000:10c4 room_persons[4].location_and_room = 0x2006 (Gurney).
        self.room_persons[4].location_and_room = 0x2006;
        // = seg000:10ca/10d0 room_persons[2] = 0x2008, slot 0x180 (Thufir).
        self.room_persons[2].location_and_room = 0x2008;
        self.room_persons[2].location_appearance = 0x180;
        // = seg000:10d6/10db Jessica to room 10, slot 0x180.
        let rp = &mut self.room_persons[1];
        rp.location_and_room = (rp.location_and_room & 0xff00) | 0x0a;
        rp.location_appearance = 0x180;
        // = seg000:10e1 subst_id_0b = 0x109 — the 0x8b name placeholder
        //   becomes COMMAND string 0x109 ("Muad'Dib").
        self.string_subst_id_table[0x0b] = cmd::MUAD_DIB;
        // = seg000:10e7 al = 0x14; call increase_charisma...
        self.increase_charisma(0x14);
        // = seg000:10ec or [bitfield_Paul_events], 10h.
        self.bitfield_paul_events |= 0x10;
        // = seg000:10f1 si = array_pointers_locations_found_by_meeting_Stilgar.
        self.mark_locations_discovered(&[45, 44, 46, 48, 49]);
    }

    // = seg000:1103 callback_game_phase_change_30_Baron_Harkonnen_pretends_
    // sietch_devastated — vision message 4 ("Something terrible has happened
    // in the palace!") and the Baron's whereabouts (location 0x14, person 9)
    // reach the COMM room.
    fn phase_callback_30_baron_pretends_sietch_devastated(&mut self) {
        self.queue_vision_message_without_location(4);
        // = seg000:1109 ax = 0x1409; jmp comm_add_person_sighting.
        self.comm_add_person_sighting(0x1409);
    }

    // = seg000:110f callback_game_phase_change_34 — unless Jessica is in room
    // 8, move her to room 10 (slot 0x180); Feyd-Rautha's whereabouts
    // (location 0x28, person 0x0a) reach the COMM room.
    fn phase_callback_34(&mut self) {
        let rp = &mut self.room_persons[1];
        if rp.location_and_room & 0xff != 8 {
            rp.location_and_room = (rp.location_and_room & 0xff00) | 0x0a;
            rp.location_appearance = 0x180;
        }
        // = seg000:1121 ax = 0x280a; jmp comm_add_person_sighting.
        self.comm_add_person_sighting(0x280a);
    }

    // = seg000:1127 callback_game_phase_change_38 — ds:fdb = 0xff: the high
    // byte of room_persons[0].location_slot goes 0xff, hiding Duke Leto.
    fn phase_callback_38(&mut self) {
        let rp = &mut self.room_persons[0];
        rp.location_appearance = (rp.location_appearance & 0x00ff) | 0xff00;
    }

    // = seg000:112d callback_game_phase_change_40 — room_persons[8] (Harah)
    // gains flags bit 2.
    fn phase_callback_40(&mut self) {
        self.room_persons[8].flags |= 2;
    }

    // = seg000:1133 callback_game_phase_change_44_mark_Oxtyn_Tabr_on_map —
    // di = 0x3d8 = locations[26]; jmp location_mark_discovered.
    fn phase_callback_44_mark_oxtyn_tabr_on_map(&mut self) {
        self.location_mark_discovered(26);
    }

    // = seg000:1139 callback_game_phase_change_48_met_Chani — +10 charisma,
    // the phase-0x48 scripted scene, Chani's room-person flags (set 0x10,
    // clear 0x02), arm the Leto-killed rallied-troop threshold, and her
    // locations appear on the map.
    fn phase_callback_48_met_chani(&mut self) {
        self.increase_charisma(0x0a);
        // = seg000:113e ax = cutscene_game_phase_48_dialogue (seg000:1313);
        //   call start_scripted_dialogue. TODO: unported (seg000:1771).
        println!("phase_callback_48_met_chani: start_scripted_dialogue unported");
        // = seg000:1144..114b room_persons[7].flags = (flags | 0x10) & ~0x02.
        let rp = &mut self.room_persons[7];
        rp.flags = (rp.flags | 0x10) & !0x02;
        // = seg000:114e..1153 the Leto-killed threshold = rallied + 2.
        self.number_of_rallied_troops_for_leto_killed =
            self.number_of_rallied_troops.wrapping_add(2);
        // = seg000:1156 si = array_pointers_locations_found_by_meeting_Chani.
        self.mark_locations_discovered(&[27, 28, 25, 69]);
    }

    // = seg000:1166 callback_game_phase_change_4c_leto_killed — worm events
    // become likelier, Jessica moves to room 2, and vision message 0x105
    // ("Oh Paul, how I would like you to be here at a time like this!").
    fn phase_callback_4c_leto_killed(&mut self) {
        // = seg000:1166 inc byte [array_likelihood_of_worm_related_spice_
        //   mining_troop_events_by_region]. TODO: the troop-event system is
        //   not modelled.
        println!("phase_callback_4c_leto_killed: worm-event likelihood bump unported");
        // = seg000:116a/116f Jessica to room 2, slot 0x180.
        let rp = &mut self.room_persons[1];
        rp.location_and_room = (rp.location_and_room & 0xff00) | 2;
        rp.location_appearance = 0x180;
        // = seg000:1175 ax = 0x105; jmp queue_vision_message_without_location.
        self.queue_vision_message_without_location(0x105);
    }

    // = seg000:117b callback_game_phase_change_50_after_riding_worm — Paul-
    // event bit 0x40, +40 charisma, and Jessica moves back to room 9.
    fn phase_callback_50_after_riding_worm(&mut self) {
        self.bitfield_paul_events |= 0x40;
        self.increase_charisma(0x28);
        // = seg000:1185 jmp move_Jessica_in_Atreides_palace.
        self.move_jessica_in_atreides_palace();
    }

    // = seg000:1188 callback_game_phase_change_54_greenhouse — unlock
    // palace_rooms[10]'s east exit (the greenhouse door), drop a locked-door
    // icon, refresh the compass arrows.
    fn phase_callback_54_greenhouse(&mut self) {
        self.scene_records[10].exits[1] &= 0x7f;
        // = seg000:118d word [data_01211] = 0xffff — the icon-list truncation
        //   (see phase_callback_0c). TODO: not modelled.
        // = seg000:1193 jmp rebuild_and_draw_room_nav_panel.
        self.rebuild_and_draw_room_nav_panel();
    }

    // = seg000:1196 callback_game_phase_change_58_met_Liet_Kynes — Paul-event
    // bit 0x20, the phase-0x58 scripted scene, and Kynes' locations appear on
    // the map.
    fn phase_callback_58_met_liet_kynes(&mut self) {
        self.bitfield_paul_events |= 0x20;
        // = seg000:119b ax = 0x12fb (cutscene_game_phase_58_dialogue); call
        //   start_scripted_dialogue. TODO: unported (seg000:1771).
        println!("phase_callback_58_met_liet_kynes: start_scripted_dialogue unported");
        // = seg000:11a1 si = array_pointers_locations_found_by_meeting_Liet_
        //   Kynes.
        self.mark_locations_discovered(&[63, 60, 61, 67, 65]);
    }

    // = seg000:11b3 callback_game_phase_change_5c — Liet Kynes moves to room
    // 5, spice-mining pressure rises, and a day+3 deadline is armed.
    fn phase_callback_5c(&mut self) {
        // = seg000:11b3 room_persons[6].location_and_room low byte = 5.
        let rp = &mut self.room_persons[6];
        rp.location_and_room = (rp.location_and_room & 0xff00) | 5;
        // = seg000:11b8 add byte [data_011d0], 0x0c — a byte of the region
        //   table read at seg000:5f15 (troop events). TODO: not modelled.
        // = seg000:11c6 inc byte [array_likelihood_of_worm_related_...].
        //   TODO: not modelled (see phase_callback_4c_leto_killed).
        println!("phase_callback_5c: troop-event pressure bumps unported");
        // = seg000:11bd..11c3 data_01156 = get_ingame_day + 3.
        self.illness_plot_armed_after_ingame_day = self.get_ingame_day_in_ax().wrapping_add(3);
    }

    // = seg000:11cb callback_game_phase_change_60_go_find_chani — Chani is
    // stationed in the Arrakeen (Harkonnen) palace, room 2. Also called
    // directly by the cure step (chani_troop_cure_progress_step, seg000:1f0d)
    // once nothing is left ill, hence the redundant phase/counter writes.
    pub(crate) fn phase_callback_60_go_find_chani(&mut self) {
        // = seg000:11cb/11d0 ds:ff = 0; game_phase = 0x60.
        self.days_since_last_game_phase_change = 0;
        self.game_phase = 0x60;
        // = seg000:11d5 di = locations[1]; call location_entry_room_dx_bx;
        //   11db dl = 2 — room 2 instead of the entry room 1.
        let (dx, bx) = self.location_entry_room_codes(1);
        self.room_persons[7].location_and_room = (dx & 0xff00) | 2;
        self.room_persons[7].location_appearance = bx;
    }

    // = seg000:40ae location_entry_room_dx_bx — build the arrival scene codes
    // for location `index`: dx = (appearance << 8) | 1 (the location's entry
    // room 1), bx = ((index + 1) << 8) | 0x80 (the location_appearance
    // in-room form).
    pub(crate) fn location_entry_room_codes(&self, index: usize) -> (u16, u16) {
        let dx = ((self.locations[index].appearance as u16) << 8) | 1;
        let bx = ((index as u16 + 1) << 8) | 0x80;
        (dx, bx)
    }

    // = seg000:6f78 increase_charisma_and_increase_troop_motivation_accordingly
    // — charisma += amount, capped at 0xc8; every 4 whole points gained feed
    // troop motivation.
    pub(crate) fn increase_charisma(&mut self, amount: u8) {
        // = seg000:6f78..6f84 the capped add.
        let old = self.charisma;
        let sum = old.wrapping_add(amount);
        self.charisma = if sum > 0xc8 { 0xc8 } else { sum };
        // = seg000:6f87..6f8e al = ((new & 0xfc) - (old & 0xfc)) >> 2.
        let steps = (self.charisma & 0xfc).wrapping_sub(old & 0xfc) >> 2;
        if steps != 0 {
            // = seg000:6f90 jnz increase_motivation_for_all_active_troops —
            //   +steps motivation on every active troop. TODO: the troop
            //   system is not ported.
            println!("increase_charisma: +{steps} troop motivation unported");
        }
    }

    // = seg000:26da comm_add_person_sighting — record a person-sighting word
    // ((location index << 8) | person id) in the COMM-room message list:
    // duplicates are ignored; at 10 entries the oldest is dropped first
    // (comm_drop_oldest_sighting, seg000:272f). From game phase 0x38, when
    // not already in the COMM room (room 8), vision message 0x201 ("A message
    // has arrived in the palace.") is queued.
    pub(crate) fn comm_add_person_sighting(&mut self, sighting: u16) {
        // = seg000:26dd..26ea the dedup scan.
        if self.comm_sightings.contains(&sighting) {
            return;
        }
        // = seg000:26f8..2706 at 10 entries drop the oldest and append as
        //   the 10th.
        if self.comm_sightings.len() >= 10 {
            self.comm_sightings.remove(0);
        }
        // = seg000:270d/270f store + count — data_000c8 is DOS's
        //   comm_sighting_count byte (seg001:00c8), kept in step with the
        //   list (build_room_command_records reads it for the COMM verbs).
        self.comm_sightings.push(sighting);
        self.data_000c8 = self.comm_sightings.len() as u8;
        // = seg000:2713 inc byte [RES_SMUG_HSQ] — the COMM unread badge
        //   (DOS keeps it in the byte the SMUG resource-table entry starts
        //   with); its reader, the COMM screen, is unported.
        // = seg000:2717..2728 the arrival notification.
        if self.game_phase >= 0x38 && self.current_room != 8 {
            self.queue_vision_message_without_location(0x201);
        }
    }

    // = seg000:71b2 or_message_ID_with_F00_and_queue_vision_message_with_
    // location — the location-event messages: class byte 0x0f over the low
    // message id, the location as the message's subject.
    pub(crate) fn queue_vision_message_f00(&mut self, message_low: u8, loc_index: usize) {
        // = seg000:71b2 mov ah,0fh; call queue_vision_message_with_location.
        self.queue_vision_message(
            0x0f00 | message_low as u16,
            crate::locations::location_ptr_from_index(loc_index),
        );
    }

    // = seg000:29ee queue_vision_message_without_location — di = 0.
    pub(crate) fn queue_vision_message_without_location(&mut self, message_id: u16) {
        self.queue_vision_message(message_id, 0);
    }

    // = seg000:29f0 queue_vision_message_with_location — queue a vision
    // message (shown when Paul next sleeps): only once Paul has had his first
    // vision (bitfield_Paul_events bit 0); duplicates (same id + location)
    // are dropped; at 10 messages the oldest is dequeued first.
    pub(crate) fn queue_vision_message(&mut self, message_id: u16, location: u16) {
        // = seg000:29f0 test [bitfield_Paul_events],1; jz ret.
        if self.bitfield_paul_events & 1 == 0 {
            return;
        }
        // = seg000:2a01..2a0d the dedup scan.
        if self
            .vision_messages
            .iter()
            .any(|&(m, l)| m == message_id && l == location)
        {
            return;
        }
        // = seg000:2a14..2a22 at 10 messages dequeue the oldest.
        if self.vision_messages.len() >= 10 {
            self.dequeue_vision_message();
        }
        // = seg000:2a25..2a30 append + count.
        self.vision_messages.push((message_id, location));
    }

    // = seg000:2a34 dequeue_vision_message — drop the oldest queued message.
    // DOS also clears the byte at seg001:118f when the queue drains; its
    // reader (the vision presentation) is unported.
    pub(crate) fn dequeue_vision_message(&mut self) {
        if !self.vision_messages.is_empty() {
            self.vision_messages.remove(0);
        }
    }
}
