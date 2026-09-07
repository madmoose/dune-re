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
//! effects (motivation, the phase-0x64 location scan), the palace-plan
//! locked-door icon-list truncation, and the string substitution table.

use crate::{
    GameState, cmd,
    room_game_screen::{NPC_DETACH_ON_TRAVEL, NPC_STORY_BIT},
};

// = seg001:002a game_phase — the story position. Multiples of 4 are the
// callback phases: dialogue event 0x0c (seg000:a235) rounds the phase up to
// the next multiple of 4 and set_game_phase_and_trigger_callbacks runs the
// per-phase callback; event 0x0b (seg000:a219) adds 1 for the sub-steps.
// Seven code paths set a value directly (noted per constant). CONDIT tests
// the value at ds:2a; the ranges the dialogue data tests are noted too.

/// Game opens: Leto sends Paul to find the Fremen, no sietch visited yet.
pub(crate) const PHASE_00_START: u8 = 0x00;
/// First sietch visited; the bump to 1 makes Duncan's palace entry visible
/// (seg000:100b).
pub(crate) const PHASE_01_DUNCAN_AVAILABLE: u8 = 0x01;
/// Two troops rallied; Leto asks for stillsuits (cond 7, +1).
pub(crate) const PHASE_02_TWO_TROOPS_RALLIED: u8 = 0x02;
/// Looking for the stillsuit maker (cond 245).
pub(crate) const PHASE_03_STILLSUIT_QUEST: u8 = 0x03;
/// Callback 04: palace room 1 background steps back, locations 10 and 17
/// appear, Jessica moves. Book: the harvester.
pub(crate) const PHASE_04_STILLSUIT_MAKER_MET: u8 = 0x04;
/// Book: the prospecting troop. Leto says the palace may hold secrets
/// (0x05..0x07, +1).
pub(crate) const PHASE_05_PROSPECTORS_FOUND: u8 = 0x05;
/// Jessica accompanies Paul through the palace (cond 96: 0x06..0x08); the
/// hidden door in room 2 (+4).
pub(crate) const PHASE_06_JESSICA_EXPLORES_PALACE: u8 = 0x06;
/// Callback 08: room 1 west exit unlocked. Jessica in the comm room
/// (0x08..0x0c, +4).
pub(crate) const PHASE_08_HIDDEN_DOOR_FOUND: u8 = 0x08;
/// Callback 0c: rooms 6/7 exits unlocked, the comm-room gather cutscene.
/// Duncan sends Paul after harvesters (0x0c..0x0f, +1).
pub(crate) const PHASE_0C_COMM_ROOM_FOUND: u8 = 0x0c;
/// Sub-step of the harvester search (cond 248).
pub(crate) const PHASE_0D_HARVESTER_SEARCH: u8 = 0x0d;
/// Set directly when Tuono-Harg is discovered (seg000:427c). Callback 10:
/// Leto and Jessica move, Duncan sighting. EQUIPMENT and GO & SEARCH
/// unlock. Book: the Emperor.
pub(crate) const PHASE_10_TUONO_HARG_FOUND: u8 = 0x10;
/// Sub-steps 0x11..0x13 (conds 11, 162, 251, 252, 487).
pub(crate) const PHASE_11_AFTER_TUONO_HARG: u8 = 0x11;
/// Callback 14: Harah's locations appear. The idle checker waits here for
/// the first vision; the pre-vision cutscene script.
pub(crate) const PHASE_14_AWAITING_VISION: u8 = 0x14;
/// Set directly by the first vision (seg000:1076): visions enabled, Leto
/// matchable, Gurney to room 0x0b, the shipment demand armed. Gurney lies
/// wounded 0x15..0x1f.
pub(crate) const PHASE_15_FIRST_VISION: u8 = 0x15;
/// Leto: "Gurney Halleck has disappeared" (0x16..0x1b, +4).
pub(crate) const PHASE_16_GURNEY_MISSING: u8 = 0x16;
/// No-op callback; cutscene script 18. Jessica finds the hidden door in
/// room 7 (0x18..0x1c, +4).
pub(crate) const PHASE_18_GURNEY_SEARCH: u8 = 0x18;
/// Callback 1c: room 6 east exit unlocked. Leto: "Go and see Thufir Hawat"
/// (0x1c..0x20, +4).
pub(crate) const PHASE_1C_ROOM_6_OPENED: u8 = 0x1c;
/// Callback 20: Thufir visible. Book: the Mentat. Thufir disarms the
/// armory trap in room 0x0b (below 0x25, +4).
pub(crate) const PHASE_20_THUFIR_FOUND: u8 = 0x20;
/// No-op callback. Book: the Harkonnens. Thufir posts a guard (+1). The
/// early palace room rules end here.
pub(crate) const PHASE_24_ARMORY_FOUND: u8 = 0x24;
/// Sub-steps 0x25..0x2b. Harah points to Stilgar's sietch (0x26..0x28, +4).
pub(crate) const PHASE_25_ARMORY_GUARDED: u8 = 0x25;
/// Callback 28: Sihaya Clam on the map. Book: the ornithopter.
pub(crate) const PHASE_28_STILGAR_SIETCH_KNOWN: u8 = 0x28;
/// Callback 2c: Paul named Muad'Dib, +20 charisma, the household
/// restationed, the raid timer stamped. Leto: "I trust Stilgar" (+1).
pub(crate) const PHASE_2C_STILGAR_MET: u8 = 0x2c;
/// Thufir: use Gurney to train the Fremen (0x2d..0x2f, +1). A troop in army
/// training with Gurney present sets 0x30 (seg000:6c46).
pub(crate) const PHASE_2D_GURNEY_TRAINING: u8 = 0x2d;
/// Callback 30: "Something terrible has happened", the Baron sighting.
/// Book: the Krys. Cutscene script 30.
pub(crate) const PHASE_30_BARON_ATTACK_RUSE: u8 = 0x30;
/// Leto plans a punitive expedition (0x31..0x32, +1).
pub(crate) const PHASE_31_LETO_WANTS_REPRISAL: u8 = 0x31;
/// TOWARDS NEAREST PLACE appears. Thufir: too early to attack, a new
/// message (0x32..0x33, +4).
pub(crate) const PHASE_32_TOWARDS_NEAREST_PLACE: u8 = 0x32;
/// Callback 34: a sighting at location 0x28. Leto: "I'll never run away"
/// (0x34..0x35, +1).
pub(crate) const PHASE_34_LETO_DEFIANT: u8 = 0x34;
/// Saboteur events start (seg000:71bc). Duncan: get ornis from the
/// smugglers (0x35..0x3a, +4).
pub(crate) const PHASE_35_SABOTEURS_BEGIN: u8 = 0x35;
/// Callback 38: Leto hidden from the palace. Comm-room lines gate here.
pub(crate) const PHASE_38_LETO_DEPARTED: u8 = 0x38;
/// Days-since gates at 0x39 and 0x3a (conds 306, 405).
pub(crate) const PHASE_39_LETO_ABSENT: u8 = 0x39;
/// Set directly by the first smuggler deal (seg000:2388). Raids no longer
/// need the timer. Harah thanks Paul at location 0x304 (+4).
pub(crate) const PHASE_3C_SMUGGLERS_DEALT: u8 = 0x3c;
/// Callback 40: Harah detaches on travel. Stilgar: "meet somebody"
/// (0x40..0x44, +4).
pub(crate) const PHASE_40_HARAH_HOME: u8 = 0x40;
/// Callback 44: Oxtyn Tabr on the map.
pub(crate) const PHASE_44_CHANI_SIETCH_KNOWN: u8 = 0x44;
/// At Oxtyn Tabr, Chani present or not (conds 313, 314).
pub(crate) const PHASE_45_CHANI_SIETCH_VISITED: u8 = 0x45;
/// Callback 48: cutscene, +10 charisma, the Leto-killed rally threshold
/// armed. The late-game theme from here.
pub(crate) const PHASE_48_CHANI_MET: u8 = 0x48;
/// Set directly when the rallied troops reach the threshold (seg000:66e8).
/// Message 0x105, worm odds up. Jessica: "The Duke is dead" (+1).
pub(crate) const PHASE_4C_LETO_KILLED: u8 = 0x4c;
/// Thufir: "another means of transportation" (0x4d..0x4f, +1).
pub(crate) const PHASE_4D_NEED_TRANSPORT: u8 = 0x4d;
/// Alone-in-room gates (conds 315..317).
pub(crate) const PHASE_4E_ALONE_FOR_WORM: u8 = 0x4e;
/// CALL A WORM stops being greyed.
pub(crate) const PHASE_4F_WORM_CALL_UNLOCKED: u8 = 0x4f;
/// Set directly by the worm departure transition (seg000:47a8). Callback
/// 50: +40 charisma. Book: the thumper. Thufir: "the answer is in this
/// palace" (0x50..0x53, +1).
pub(crate) const PHASE_50_WORM_RIDDEN: u8 = 0x50;
/// Jessica travels with Paul again (cond 96); the hidden door in room 0x0b
/// (0x51..0x54, +4).
pub(crate) const PHASE_51_JESSICA_RETURNS: u8 = 0x51;
/// Callback 54: the greenhouse door unlocked; room 3 allowed in the palace
/// shuffle.
pub(crate) const PHASE_54_GREENHOUSE_OPENED: u8 = 0x54;
/// Kynes greets Chani (0x55..0x57, +4).
pub(crate) const PHASE_55_KYNES_QUEST: u8 = 0x55;
/// Callback 58: cutscene, Kynes' locations appear. Kynes: "Come in the next
/// room" (0x58..0x5c, +4).
pub(crate) const PHASE_58_KYNES_MET: u8 = 0x58;
/// Callback 5c: Kynes to room 5, the illness plot armed for day + 3. Book:
/// the wind-traps.
pub(crate) const PHASE_5C_BOTANICAL_STATION: u8 = 0x5c;
/// Chani cures the ill sietch by staying there (seg000:1e01). No line in
/// the CD DIALOGUE data fires the +1 event at 0x5c, so this value is only
/// reachable from a save.
pub(crate) const PHASE_5D_CURING_ILLNESS: u8 = 0x5d;
/// Set directly when the cure completes (seg000:11d0): Chani is held in the
/// Arrakeen palace, room 2.
pub(crate) const PHASE_60_FIND_CHANI: u8 = 0x60;
/// Set by talking to a Fremen troop with speech bit 0x800 during
/// 0x60..0x63 (seg000:1ed1). Motivation drops 0x64..0x67. Chani: "deliver
/// me" (+4). The callback stations Chani.
pub(crate) const PHASE_64_ENDGAME: u8 = 0x64;
/// No-op callback. The final attack preparation (Stilgar's Water of Life
/// and troop selection events).
pub(crate) const PHASE_68_CHANI_RESCUED: u8 = 0x68;
/// The last callback slot, no-op. Event 0x0e raises the attack stage.
pub(crate) const PHASE_6C_FINAL_ATTACK: u8 = 0x6c;
/// Set by the ending (seg000:16fc): the idle room loop stops, head
/// animation 5, the final sprite mapping.
pub(crate) const PHASE_C8_GAME_WON: u8 = 0xc8;

impl GameState {
    // = seg000:100b callback_event_dialogue_line_0b_game_phase_01_make_Duncan_
    // Idaho_visible — mov byte [ds:100b], 1: write 1 into the high byte of
    // room_persons[3].location_slot, flipping Duncan Idaho's 0xff80
    // (never-matching, hidden) to 0x0180 so his entry matches the palace room
    // (location_and_room 0x2004) from now on.
    pub(crate) fn make_duncan_idaho_visible(&mut self) {
        let entry = &mut self.room_persons[3];
        entry.location_appearance = (entry.location_appearance & 0x00ff) | 0x0100;
    }

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
            PHASE_04_STILLSUIT_MAKER_MET => self.phase_callback_04_tuono_tabr(),
            PHASE_08_HIDDEN_DOOR_FOUND => self.phase_callback_08(),
            PHASE_0C_COMM_ROOM_FOUND => self.phase_callback_0c(),
            PHASE_10_TUONO_HARG_FOUND => self.phase_callback_10(),
            PHASE_14_AWAITING_VISION => self.phase_callback_14(),
            // = seg000:10b7 callback_game_phase_change_18_24_3c_68_6c: ret.
            PHASE_18_GURNEY_SEARCH
            | PHASE_24_ARMORY_FOUND
            | PHASE_3C_SMUGGLERS_DEALT
            | PHASE_68_CHANI_RESCUED
            | PHASE_6C_FINAL_ATTACK => {}
            PHASE_1C_ROOM_6_OPENED => self.phase_callback_1c(),
            PHASE_20_THUFIR_FOUND => self.phase_callback_20_make_thufir_hawat_visible(),
            PHASE_28_STILGAR_SIETCH_KNOWN => self.phase_callback_28_mark_sihaya_clam_on_map(),
            PHASE_2C_STILGAR_MET => self.phase_callback_2c_met_stilgar(),
            PHASE_30_BARON_ATTACK_RUSE => self.phase_callback_30_baron_pretends_sietch_devastated(),
            PHASE_34_LETO_DEFIANT => self.phase_callback_34(),
            PHASE_38_LETO_DEPARTED => self.phase_callback_38(),
            PHASE_40_HARAH_HOME => self.phase_callback_40(),
            PHASE_44_CHANI_SIETCH_KNOWN => self.phase_callback_44_mark_oxtyn_tabr_on_map(),
            PHASE_48_CHANI_MET => self.phase_callback_48_met_chani(),
            PHASE_4C_LETO_KILLED => self.phase_callback_4c_leto_killed(),
            PHASE_50_WORM_RIDDEN => self.phase_callback_50_after_riding_worm(),
            PHASE_54_GREENHOUSE_OPENED => self.phase_callback_54_greenhouse(),
            PHASE_58_KYNES_MET => self.phase_callback_58_met_liet_kynes(),
            PHASE_5C_BOTANICAL_STATION => self.phase_callback_5c(),
            PHASE_60_FIND_CHANI => self.phase_callback_60_go_find_chani(),
            PHASE_64_ENDGAME => {
                // = seg000:11e6 -> 1f13 callback_game_phase_change_64_main_
                //   code — scan locations for the best-provisioned Atreides
                //   sietch (location_do_accumulation_on_troops, troop system)
                //   and station Chani there, recording it as a COMM sighting
                //   (0x2b0a). TODO: port with the troop system.
                println!("phase_callback_64: unported (needs the troop system)");
            }
            p if p > PHASE_6C_FINAL_ATTACK => {}
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
        // = seg000:103f ax = cutscene_game_phase_0c_dialogue; jmp
        //   start_scripted_dialogue — the communication-room gather scene
        //   (Leto and Jessica take turns; sequence.rs).
        self.start_scripted_dialogue(&crate::sequence::SCRIPT_PHASE_0C);
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

    // = seg000:1071 loc_01071 — Paul's first vision, fired by the idle
    // checker at game phase exactly 0x14 (first_vision_idle_check): advance
    // to phase 0x15, restation the household (the Duke's presence becomes
    // matchable, Gurney to room 0x0b of location 0x20, Jessica to room 10),
    // arm the spice-shipment plot with a fresh demand, and — with visions now
    // enabled (Paul-event bit 0) — queue message 1 ("A message has arrived in
    // the palace.").
    pub(crate) fn first_vision_phase_advance(&mut self) {
        // = seg000:1071/1076 ds:ff = 0; game_phase = 0x15.
        self.days_since_last_game_phase_change = 0;
        self.game_phase = PHASE_15_FIRST_VISION;
        // = seg000:107b data_00fdb = 1 — room_persons[0].location_appearance
        //   high byte (the visibility byte, cf. phase_callback_20).
        let rp = &mut self.room_persons[0];
        rp.location_appearance = (rp.location_appearance & 0x00ff) | 0x0100;
        // = seg000:1080/1086 room_persons[4] to (0x200b, slot 0x180).
        self.room_persons[4].location_and_room = 0x200b;
        self.room_persons[4].location_appearance = 0x180;
        // = seg000:108c room_persons[1].location_and_room low byte = 0x0a.
        let rp = &mut self.room_persons[1];
        rp.location_and_room = (rp.location_and_room & 0xff00) | 0x0a;
        // = seg000:1091 [contact_distance_related_ds_d5] = 0xff.
        self.contact_distance_related_ds_d5 = 0xff;
        // = seg000:1096 call loc_02090 — stamp today as the shipment event
        //   day and roll the first demand.
        self.ingame_day_of_last_spice_shipment_event = self.get_ingame_day();
        self.spice_shipment_roll_new_demand();
        // = seg000:1099 or [bitfield_Paul_events], 1 — visions enabled.
        self.bitfield_paul_events |= 1;
        // = seg000:109e/10a1 queue message 1.
        self.queue_vision_message_without_location(1);
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
        self.room_persons[8].flags |= NPC_DETACH_ON_TRAVEL;
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
        rp.flags = (rp.flags | NPC_STORY_BIT) & !NPC_DETACH_ON_TRAVEL;
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
        //   mining_troop_events_by_region] — raise the base worm-event
        //   probability.
        self.worm_event_likelihood_by_region[0] =
            self.worm_event_likelihood_by_region[0].wrapping_add(1);
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
        println!("phase_callback_5c: seg000:5f15 pressure bump unported");
        // = seg000:11c6 inc byte [array_likelihood_of_worm_related_...] —
        //   raise the base worm-event probability.
        self.worm_event_likelihood_by_region[0] =
            self.worm_event_likelihood_by_region[0].wrapping_add(1);
        // = seg000:11bd..11c3 data_01156 = get_ingame_day + 3.
        self.illness_plot_armed_after_ingame_day = self.get_ingame_day().wrapping_add(3);
    }

    // = seg000:11cb callback_game_phase_change_60_go_find_chani — Chani is
    // stationed in the Arrakeen (Harkonnen) palace, room 2. Also called
    // directly by the cure step (chani_troop_cure_progress_step, seg000:1f0d)
    // once nothing is left ill, hence the redundant phase/counter writes.
    pub(crate) fn phase_callback_60_go_find_chani(&mut self) {
        // = seg000:11cb/11d0 ds:ff = 0; game_phase = 0x60.
        self.days_since_last_game_phase_change = 0;
        self.game_phase = PHASE_60_FIND_CHANI;
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
        // = seg000:2713 inc byte [for_condit_comms_room_message_count_ds_c9]
        //   — the COMM unread badge (viewing the message decrements it). An
        //   unread entry dropped by the overflow path above leaves the badge
        //   high — mirroring DOS, whose comm_drop_oldest_sighting does not
        //   touch ds:c9 either.
        self.comm_unread_count_ds_c9 = self.comm_unread_count_ds_c9.wrapping_add(1);
        // = seg000:2717..2728 the arrival notification.
        if self.game_phase >= PHASE_38_LETO_DEPARTED && self.current_room != 8 {
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
}
