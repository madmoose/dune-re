//! The per-time-period event scheduler — the DOS engine at seg000:1b23..2164
//! that fires the game's clock-driven events: the desert-walk countdown, the
//! new-day bookkeeping (phase-change day counter, NPC relocation, the
//! phase-5c illness plot, the spice-production stats, smuggler restocks),
//! one period of troop occupation events, the location/ecology walk, the
//! time-of-day action table (the Emperor's spice-shipment demands, the
//! Harkonnen raid scheduler, the daily Harkonnen growth) and the screen
//! refresh that follows.
//!
//! game_loop's loc_01b0d block and the WAIT-verb / travel pump
//! (run_events_for_n_time_periods) call run_events_for_current_time_period
//! whenever new_time_period_pending is up.

use crate::GameState;

// = seg000:1d35 array_related_to_location_appearances — the top room number
// of each location appearance; an NPC parked in a higher room than its
// location's current appearance allows is re-homed to room 1 (seg000:1d8c).
// Extracted verbatim from DNCDPRG.EXE.
static APPEARANCE_TOP_ROOM: [u8; 49] = [
    0x02, 0x02, 0x02, 0x02, 0x02, 0x03, 0x03, 0x03, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
    0x05, 0x02, 0x02, 0x02, 0x02, 0x02, 0x03, 0x03, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
    0x0c, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03,
    0x02,
];

// = seg000:2165 data_02165 — the day-3 spice-shipment reminder table, indexed
// by 4 * days-past-the-event-day + min(fulfilment class, 2) - 4: the COMM
// sighting person id (0 = escalate to the room-screen consequence instead,
// seg000:2154).
static SHIPMENT_REMINDER_TABLE: [u8; 11] = [4, 5, 6, 0, 5, 6, 0, 0, 6, 0, 0];

impl GameState {
    // = seg000:1b23 run_events_for_current_time_period — fire the scheduled
    // events for a newly-entered time period (new_hour_flag, consumed here),
    // then refresh whichever main view is up. See the module header for the
    // full breakdown.
    pub(crate) fn run_events_for_current_time_period(&mut self) {
        // = seg000:1b23 cmp [new_hour_flag],0; jz loc_01b0c — only a newly-
        //   entered time period runs events.
        if self.new_time_period_pending == 0 {
            return;
        }
        // = seg000:1b2a new_hour_flag = 0 — consume the flag.
        self.new_time_period_pending = 0;
        // = seg000:1b2f..1b3d the desert-walk countdown: decrement; a result
        //   below 0x10 (signed) zeroes the counter and the ds:f5 CONDIT byte.
        let c = self.desert_walk_counter.wrapping_sub(1) as i8;
        self.desert_walk_counter = if c >= 0x10 {
            c as u8
        } else {
            self.for_condit_desert_walk_ds_f5 = 0;
            0
        };
        // = seg000:1b40 call loc_01a0f — repaint the date/time indicator.
        self.ui_redraw_date_and_time_indicator();
        // = seg000:1b43 call loc_038e1 — cross-fade the sky to the new
        //   time-of-day sub-palette if it changed. This arms the SkyFade frame
        //   task (it does not step the palette here); the fade then runs in the
        //   game loop *after* the caller's present. On a WAIT FOR EVENING/MORNING
        //   skip this is the sky palette fade that follows the spiral transition:
        //   run_events_for_n_time_periods advances game_time to evening (re-aiming
        //   the fade target each period), the spiral reveals the room in the old
        //   palette, then the SkyFade task morphs the sky to the new time-of-day.
        self.loc_038e1_sky_refresh();
        // = seg000:1b46..1b56 new_day_flag = the day part of game_time minus
        //   the day part the last run saw (data_01174).
        let previous = std::mem::replace(&mut self.last_event_game_time, self.game_time);
        self.new_day_flag = (self.game_time as u8 & 0xf0).wrapping_sub(previous as u8 & 0xf0);
        // = seg000:1b59/1b5b jz; call run_events_for_current_time_period_new_
        //   day_01c46.
        if self.new_day_flag != 0 {
            self.run_events_new_day();
        }
        // = seg000:1b5e/1b63 cmp final_attack_stage_ds_c2,7; jnb loc_01bb2 —
        //   from stage 7 of the final attack the event walks (and the refresh
        //   tail) stop running.
        if self.final_attack_stage >= 7 {
            self.room_redraw_request = 0;
            return;
        }
        // = seg000:1b65 call test_for_Chani_in_room_and_do_stuff_if_true.
        self.chani_cure_check();
        // = seg000:1b68/1b6c push [subst_id_06] / [data_011ce] — the events
        //   below stage other locations (and their name placeholders) for
        //   CONDIT; the entry staging is restored at 1b85..1b89.
        let saved_subst = self.string_subst_id_table[6];
        let saved_staged = self.condit_staged_location;
        // = seg000:1b70 call run_troop_occupation_events — one period of every
        //   troop's occupation.
        self.run_troop_occupation_events();
        // = seg000:1b73 call iterate_over_all_locations_upon_new_day.
        self.locations_new_day_ecology_walk();
        // = seg000:1b76..1b7d call word ptr data_01db3[2 * get_ingame_time_of_
        //   day] — the time-of-day action table; the entries not listed here
        //   share the empty handler (actions_time_in_day_0_1_2_5_6_7_9_a_b_c_
        //   d_e, seg000:1dd3).
        match self.get_ingame_time_of_day() {
            3 => self.actions_time_in_day_3(),
            4 => self.actions_time_in_day_4(),
            8 => self.actions_time_in_day_8(),
            15 => self.randomly_increase_all_harkonnen_troops_head_count(),
            _ => {}
        }
        // = seg000:1b82 call loc_01c18.
        self.redraw_period_sensitive_view_content();
        // = seg000:1b85/1b86 pop di; call prepare_location_data_for_condit —
        //   re-stage the location the entry staging referred to.
        self.prepare_location_data_for_condit(saved_staged);
        // = seg000:1b89 pop [subst_id_06].
        self.string_subst_id_table[6] = saved_subst;

        // = seg000:1b8d loc_01b8d — the refresh tail.
        // = seg000:1b8d call loc_01bec — the night-attack period step.
        self.night_attack_period_step();
        // = seg000:1b90 a pending room-screen swap owns the next present.
        if self.pending_room_screen_request != 0 {
            self.room_redraw_request = 0;
            return;
        }
        // = seg000:1b97/1b9e a dirty map view refreshes (the function no-ops
        //   unless a map mode is up).
        if self.spice_density_overlay_dirty != 0 {
            self.map_view_refresh_after_events();
        }
        // = seg000:1ba1..1ba7 no current location (out in the desert), no
        //   room to redraw.
        if self.current_location_index == 0xffff {
            self.room_redraw_request = 0;
            return;
        }
        // = seg000:1ba9..1bb0 route on the data_0473b request byte.
        let request = self.room_redraw_request;
        if request & 0x80 != 0 {
            // = seg000:1bd2 loc_01bd2 — the full re-present: clear the
            //   request (call loc_01bb2), then on the map view refresh it
            //   (loc_01be9); in the room view (unless the WAIT-verb pump owns
            //   the screen) dismiss the stacked overlays and re-present
            //   through loc_00fa7: restore the cursor, present with the
            //   spiral effect 0x2a and redraw the companion portraits.
            self.room_redraw_request = 0;
            if self.room_view_toggle & 0x80 != 0 {
                self.map_view_refresh_after_events();
                return;
            }
            if self.events_pump_active != 0 {
                return;
            }
            self.dismiss_stacked_menus();
            self.call_restore_cursor();
            self.ui_present_room_screen(0x2a);
            self.ui_hud_draw_companions();
            return;
        }
        if request != 0 {
            // = seg000:1bb8 loc_01bb8 — the plain room redraw, skipped on the
            //   map view or while the WAIT-verb pump owns the screen.
            if self.room_view_toggle & 0x80 != 0 || self.events_pump_active != 0 {
                self.room_redraw_request = 0;
                return;
            }
            // = seg000:1bc6..1bcf restore the cursor, clear the request
            //   (call loc_01bb2), clear the night-attack task (loc_00b21) and
            //   redraw the room.
            self.call_restore_cursor();
            self.room_redraw_request = 0;
            self._clear_night_attack();
            self.draw_room_game_screen();
            return;
        }
        // = seg000:1bb2 loc_01bb2 (the request == 0 fall-through).
        self.room_redraw_request = 0;
    }

    // = seg000:1c46 run_events_for_current_time_period_new_day_01c46 — the
    // new-day hook (new_day_flag != 0).
    fn run_events_new_day(&mut self) {
        // = seg000:1c46..1c58 the phase-change day counter: a game_phase
        //   change (against the ds:fe copy) resets ds:ff, then every new day
        //   increments it.
        let previous = std::mem::replace(&mut self.game_phase_copy_ds_fe, self.game_phase);
        if previous != self.game_phase {
            self.days_since_last_game_phase_change = 0;
        }
        self.days_since_last_game_phase_change =
            self.days_since_last_game_phase_change.wrapping_add(1);
        // = seg000:1c5c call iterate_over_named_NPCs_and_location_appearances_
        //   to_modify_NPCs.
        self.relocate_npcs_after_appearance_changes();
        // = seg000:1c5f call if_game_phase_5c_find_nonfortress_with_the_most_
        //   rallied_troops_and_make_them_ill.
        self.illness_pick_location_phase_5c();
        // = seg000:1c62..1c6b the contact-distance byte: incremented, but
        //   only stored back from 2 up.
        let d = self.contact_distance_related_ds_d5.wrapping_add(1);
        if d >= 2 {
            self.contact_distance_related_ds_d5 = d;
        }
        // = seg000:1c6e..1c96 the daily spice-production diff: production =
        //   spice spent today (data_01172, consumed) + stock - stock at the
        //   last new day (data_01170, reloaded), clamped at 0.
        let spent = std::mem::take(&mut self.spice_spent_today);
        let stock = self.spice_in_stock;
        let gross = spent.wrapping_add(stock);
        let baseline = std::mem::replace(&mut self.spice_stock_at_last_new_day, stock);
        let (production, borrow) = gross.overflowing_sub(baseline);
        let production = if borrow { 0 } else { production };
        self.todays_spice_production = production;
        // = seg000:1c87..1c99 the better/lower-than-yesterday pair from the
        //   previous total (ds:ae).
        let previous_production =
            std::mem::replace(&mut self.previous_day_spice_production, production);
        if previous_production >= production {
            self.spice_production_lower_than_previous_day = previous_production - production;
            self.spice_production_better_than_previous_day = 0;
        } else {
            self.spice_production_lower_than_previous_day = 0;
            self.spice_production_better_than_previous_day = production - previous_production;
        }
        // = seg000:1c9d call iterate_over_locations_and_accumulate_...
        self.accumulate_potential_spice_harvest();
        // = seg000:1ca0 call recompute_condit_statistics (loc_0c02e).
        self.recompute_condit_statistics();
        // = seg000:1ca3 call loc_0bf26 — re-format the stats percentage
        //   strings (the globe-ornament stats display). Not ported (with its
        //   reader, loc_0bdbb). TODO.
        // = seg000:1ca6..1cd7 the smuggler restock: each smuggler with
        //   field_2 bit 3 refills empty stock slots whose price byte has bit
        //   7 with two rolled-in bits of one rand() word (0..3 units), the
        //   slots walked from weirding modules down to harvesters.
        let mut bits = self.rand();
        for i in 0..self.smugglers.len() {
            // = seg000:1cd4 cmp byte ptr [si],14h — a region byte >= 0x14
            //   ends the table (the port array is exactly the six records).
            if self.smugglers[i].field_2 & 8 == 0 {
                continue;
            }
            for slot in (0..5).rev() {
                let s = &mut self.smugglers[i];
                if s.stock[slot] != 0 || s.prices[slot] & 0x80 == 0 {
                    continue;
                }
                // = seg000:1cc3..1ccb rol bx,1 twice; stock = bl & 3.
                bits = bits.rotate_left(2);
                s.stock[slot] = (bits & 3) as u8;
            }
        }
    }

    // = seg000:1d66 iterate_over_named_NPCs_and_location_appearances_to_
    // modify_NPCs — walk the 12 named-NPC room_persons records; each NPC
    // parked at a location (location_slot low byte 0x80, high byte the
    // 1-based location index, not 0xff) is re-homed to room 1 of the
    // location's current appearance when the stored appearance no longer
    // matches, or its room exceeds the appearance's top room.
    fn relocate_npcs_after_appearance_changes(&mut self) {
        for i in 0..12 {
            let slot = self.room_persons[i].location_appearance;
            // = seg000:1d6f/1d73 the parked-at-a-location filter.
            if slot & 0xff != 0x80 || slot >> 8 == 0xff {
                continue;
            }
            // = seg000:1d78..1d7f di = 0x1c * high byte + 0xe4 — the 1-based
            //   location record. (The static 0x7f80 reset value indexes past
            //   the table in DOS; the port skips it.)
            let li = (slot >> 8) as usize - 1;
            let Some(loc) = self.locations.get(li) else {
                continue;
            };
            // = seg000:1d81..1d91 keep the record when the appearance still
            //   matches and the room is within the appearance's top room.
            let stored = self.room_persons[i].location_and_room;
            let appearance = loc.appearance;
            if stored >> 8 == appearance as u16
                && (stored & 0xff) as u8 <= APPEARANCE_TOP_ROOM[appearance as usize]
            {
                continue;
            }
            // = seg000:1d93..1d97 re-home to room 1 of the current appearance.
            self.room_persons[i].location_and_room = ((appearance as u16) << 8) | 1;
        }
    }

    // = seg000:1e43 if_game_phase_5c_find_nonfortress_with_the_most_rallied_
    // troops_and_make_them_ill — the illness-plot picker: from the armed day
    // on, during phase 0x5c, away from location 62, make every hired troop
    // of the sietch with the most non-ecology troops ill, and queue the
    // "strange disease" vision message.
    fn illness_pick_location_phase_5c(&mut self) {
        // = seg000:1e43..1e51 the day and phase gates.
        if self.get_ingame_day_in_ax() < self.illness_plot_armed_after_ingame_day
            || self.game_phase != 0x5c
        {
            return;
        }
        // = seg000:1e53 cmp [current_location_ptr],7c8h — not while the
        //   player is at locations[62].
        if self.current_location_index == 62 {
            return;
        }
        // = seg000:1e5b..1e88 scan for the candidate with the most hired
        //   non-ecology (occupation < 8) troops; sietches only (appearance
        //   < 0x28), not hidden (status bit 7), skipping locations[16]
        //   (Tuono-Tabr).
        let mut best = None;
        let mut best_count = 0u16;
        for li in 0..self.locations.len() {
            let loc = &self.locations[li];
            if loc.appearance >= 0x28 || loc.status & 0x80 != 0 || li == 16 {
                continue;
            }
            // = seg000:1e72..1e77 callback_troop_accumulate_troop_fremen_non_
            //   ecology_troops over the hired troops: cmp occupation,8; adc.
            let mut count = 0u16;
            self.for_each_hired_troop_in_location(li, |s, ti| {
                if s.troops[ti].occupation < 8 {
                    count += 1;
                }
            });
            if count > best_count {
                best_count = count;
                best = Some(li);
            }
        }
        // = seg000:1e8a jcxz loc_01ea8.
        let Some(li) = best else {
            return;
        };
        // = seg000:1e8e/1e92 latch the location and bump the counter.
        self.latest_location_with_illness = crate::locations::location_ptr_from_index(li);
        self.number_of_locations_with_illness =
            self.number_of_locations_with_illness.wrapping_add(1);
        // = seg000:1e96..1e99 callback_troop_make_troop_ill_and_stop_working
        //   on every hired troop: dissatisfaction bit 0x400 + stop working.
        self.for_each_hired_troop_in_location(li, |s, ti| {
            s.troops[ti].dissatisfaction_and_speech |= 0x400;
            s.troop_make_stop_working(ti);
        });
        // = seg000:1e9c/1e9e message 8 = "There is a strange disease here in
        //   .... We all are ill... very ill."
        self.queue_vision_message_f00(8, li);
    }

    // = seg000:1e24 location_does_location_house_an_ill_troop — walk the
    // location's troop chain; carry set when any troop has the illness bit
    // (dissatisfaction_and_speech & 0x400).
    fn location_houses_ill_troop(&self, li: usize) -> bool {
        let mut id = self.locations[li].troop_id;
        while id != 0 {
            let t = &self.troops[(id - 1) as usize];
            if t.dissatisfaction_and_speech & 0x400 != 0 {
                return true;
            }
            id = t.next_troop_id;
        }
        false
    }

    // = seg000:1d9f test_for_Chani_in_room_and_do_stuff_if_true — while
    // Chani is away (not in the room with Paul) and parked at an ill
    // location during phase 0x5d, her cure progresses.
    fn chani_cure_check(&mut self) {
        // = seg000:1d9f test [persons_in_room],80h — Chani (person 7) in the
        //   room means she is with Paul, not curing.
        if self.persons_in_room & 0x80 != 0 {
            return;
        }
        // = seg000:1da7/1daa si = room_persons[7]; the phase-5d ill-troop
        //   check; jb -> the cure step.
        if let Some(li) = self.chani_parked_at_ill_location() {
            self.chani_troop_cure_progress_step(li);
        }
    }

    // = seg000:1e01 NPC_is_Chani_during_game_phase_5d_find_ill_troops_at_her_
    // location — during phase 0x5d, when room_persons[7] is Chani parked at a
    // location (location_slot low byte 0x80), pin her to room 2 there and
    // return the location if it houses an ill troop (the DOS carry).
    fn chani_parked_at_ill_location(&mut self) -> Option<usize> {
        if self.game_phase != 0x5d {
            return None;
        }
        let p = self.room_persons[7];
        // = seg000:1e08 cmp npc->person_index,7 — Chani.
        if p.person_index != 7 {
            return None;
        }
        // = seg000:1e0e..1e14 parked at a location?
        let slot = p.location_appearance;
        if slot & 0xff != 0x80 {
            return None;
        }
        // = seg000:1e16 npc->location_and_room low byte = 2 — she works from
        //   room 2.
        self.room_persons[7].location_and_room = (p.location_and_room & 0xff00) | 2;
        // = seg000:1e1b..1e22 di = the 1-based location record, falling into
        //   location_does_location_house_an_ill_troop. (An out-of-table index
        //   — the 0x7f80 reset value — reads garbage in DOS; the port skips.)
        let li = ((slot >> 8) as usize).wrapping_sub(1);
        if li >= self.locations.len() {
            return None;
        }
        self.location_houses_ill_troop(li).then_some(li)
    }

    // = seg000:1eda chani_troop_cure_progress_step — +8 per period; on the
    // wrap to 0 every troop at the location is cured, the "I've managed to
    // cure everybody" message is queued, and the plot either moves to the
    // next ill location or fires the phase-0x60 go-find-Chani callback.
    fn chani_troop_cure_progress_step(&mut self, li: usize) {
        // = seg000:1eda add [Chani_troop_illness_cure_progress],8; jnz ret.
        self.chani_troop_illness_cure_progress =
            self.chani_troop_illness_cure_progress.wrapping_add(8);
        if self.chani_troop_illness_cure_progress != 0 {
            return;
        }
        // = seg000:1ee3..1ee6 callback_troop_make_troop_cured_from_illness on
        //   ALL troops at the location: clear bit 0x400, set the was-cured
        //   speech bit 0x800.
        self.for_each_troop_in_location(li, |s, ti| {
            let t = &mut s.troops[ti];
            t.dissatisfaction_and_speech = (t.dissatisfaction_and_speech & !0x400) | 0x800;
        });
        // = seg000:1ee9/1eec message 9 = "Paul, I'm so happy! I've managed to
        //   cure everybody, here in ...."
        self.queue_vision_message(0x709, crate::locations::location_ptr_from_index(li));
        // = seg000:1eef dec [number_of_locations_with_illness].
        self.number_of_locations_with_illness =
            self.number_of_locations_with_illness.wrapping_sub(1);
        // = seg000:1ef3..1f05 rescan for the next ill location (0 = none).
        let next = (0..self.locations.len()).find(|&i| self.location_houses_ill_troop(i));
        self.latest_location_with_illness = match next {
            Some(i) => crate::locations::location_ptr_from_index(i),
            None => 0,
        };
        // = seg000:1f09..1f0d nothing left ill: the go-find-Chani phase
        //   callback (seg000:11cb).
        if next.is_none() {
            self.phase_callback_60_go_find_chani();
        }
    }

    // = seg000:63f0 iterate_over_all_locations_upon_new_day — the per-new-day
    // location/ecology walk: grow each vegetation-program location's water by
    // the nearby stage-1 vegetation, then promote a pseudo-random set of
    // stage-1 map cells to stage 2 (the drawable tufts).
    fn locations_new_day_ecology_walk(&mut self) {
        // = seg000:63f0 cmp [new_day_flag],0; jz ret.
        if self.new_day_flag == 0 {
            return;
        }
        // = seg000:63fe..6428 pass 1: locations with status bit 0x20 and
        //   water below 0xfa grow by 1 + half the stage-1 count in the 6 map
        //   cells around their map offset.
        for li in 0..self.locations.len() {
            let loc = &self.locations[li];
            if loc.status & 0x20 == 0 || loc.water >= 0xfa {
                continue;
            }
            // = seg000:640e/6411 si = [di+6]; call count_stage1_vegetation_
            //   cells_near_offset — the 6 cells at map offset si-1 .. si+4.
            let base = (loc.map_offset as usize).saturating_sub(1);
            let nearby = self.map[base..self.map.len().min(base + 6)]
                .iter()
                .filter(|&&c| c & 0x30 == 0x10)
                .count() as u8;
            // = seg000:6414..641f water += nearby/2 + 1, capped at 0xfa.
            self.locations[li].water = (loc.water + nearby / 2 + 1).min(0xfa);
        }
        // = seg000:65b6 loc_065b6 pass 2: a 0x46-step LFSR walk (taps 0x402)
        //   over map columns of stride 0x800; every visited stage-1 cell
        //   ((cell & 0x30) == 0x10) is promoted to stage 2.
        let mut state = self.ecology_lfsr_state;
        let mut promoted = 0u16;
        for _ in 0..0x46 {
            // = seg000:65c4..65c8 shr si,1; jnb; xor si,402h.
            let carry = state & 1 != 0;
            state >>= 1;
            if carry {
                state ^= 0x402;
            }
            // = seg000:65cc..65ea the column: offsets state, state + 0x7ff,
            //   ... below the map end (data_0c5f9).
            let mut ofs = state as usize;
            while ofs < self.map.len() {
                let cell = self.map[ofs];
                if cell & 0x30 == 0x10 {
                    self.map[ofs] = (cell & 0xcf) | 0x20;
                    promoted += 1;
                }
                ofs += 0x7ff;
            }
        }
        // = seg000:65ee the persistent LFSR state.
        self.ecology_lfsr_state = state;
        // = seg000:65f3..65fe promotions while the full map view is up dirty
        //   the view (data_046ec).
        if promoted != 0 && self.data_046eb & 0x80 != 0 {
            self.spice_density_overlay_dirty = self.spice_density_overlay_dirty.wrapping_add(1);
        }
    }

    // = seg000:1cda iterate_over_locations_and_accumulate_number_of_Atreides_
    // locations_and_value_related_to_spice_density — ds:a8 = sum of
    // spice_density/8 over Atreides locations + rand_iterated(sum/16). (The
    // Atreides count accumulates in dx but has no store.)
    fn accumulate_potential_spice_harvest(&mut self) {
        let mut sum = 0u16;
        for li in 0..self.locations.len() {
            if self.location_is_atreides(li) {
                sum = sum.wrapping_add((self.locations[li].spice_density >> 3) as u16);
            }
        }
        // = seg000:1cfc..1d09 the random topping.
        let extra = self.rand_iterated(sum >> 4);
        self.potential_spice_harvest = sum.wrapping_add(extra);
    }

    // = seg000:bfe3 compute_area_controlled_percentages — the territory
    // percentages from the map vegetation bits: cells with (cell & 0x30) ==
    // 0x30 count as Harkonnen area, other nonzero stages as Atreides;
    // ds:a2 = round(atreides * 100 / cells) + 1, ds:a4 = round(harkonnen *
    // 100 / cells), with the 0x187-byte polar tail excluded from the total.
    fn compute_area_controlled_percentages(&mut self) {
        let mut flagged = 0u32;
        let mut atreides = 0u32;
        for &cell in self.map.iter() {
            match cell & 0x30 {
                0 => {}
                0x30 => flagged += 1,
                _ => {
                    flagged += 1;
                    atreides += 1;
                }
            }
        }
        let harkonnen = flagged - atreides;
        // = seg000:c002..c006 si = the scan end - 0x188 + 1.
        let cells = (self.map.len() - 0x187) as u32;
        // = seg000:c007..c010 (count * 65536 / cells) * 100, rounded by the
        //   doubled low word's carry.
        let pct = |count: u32| -> u16 {
            let q = ((count << 16) / cells) * 100;
            ((q >> 16) + ((q >> 15) & 1)) as u16
        };
        // = seg000:c023 inc dx — the Atreides side gets + 1.
        self.area_controlled_by_atreides = pct(atreides).wrapping_add(1);
        self.area_controlled_by_harkonnen = pct(harkonnen);
    }

    // = seg000:c02e recompute_condit_statistics — the area percentages, the
    // running max of today's spice production, and the troop population sums
    // (ds:ac Harkonnen, ds:aa loyal).
    fn recompute_condit_statistics(&mut self) {
        self.compute_area_controlled_percentages();
        // = seg000:c031..c046 the same production formula as the new-day
        //   diff, kept as a running max within the day.
        let gross = self.spice_in_stock.wrapping_add(self.spice_spent_today);
        let (production, borrow) = gross.overflowing_sub(self.spice_stock_at_last_new_day);
        let production = if borrow { 0 } else { production };
        if production >= self.todays_spice_production {
            self.todays_spice_production = production;
        }
        // = seg000:c049..c079 the population sums: Harkonnen troops
        //   (bitfield_10 bit 7) into ds:ac; troops neither captured
        //   (occupation bit 5) nor unrallied (bit 7) into ds:aa.
        self.data_000aa = 0;
        self.data_000ac = 0;
        for t in self.troops.iter() {
            // = seg000:c076 cmp byte ptr [si],0 — troop_id 0 ends the table.
            if t.troop_id == 0 {
                break;
            }
            let pop = t.population as u16;
            if t.occupation & 0x20 != 0 {
                continue;
            }
            if t.bitfield_10 & 0x80 != 0 {
                self.data_000ac = self.data_000ac.wrapping_add(pop);
            } else if t.occupation & 0x80 == 0 {
                self.data_000aa = self.data_000aa.wrapping_add(pop);
            }
        }
    }

    // = seg000:1d10 randomly_increase_all_harkonnen_troops_head_count — the
    // end-of-day (time-of-day 0xf) Harkonnen growth: on a set rolling rand
    // bit, every Harkonnen troop (bitfield_10 bit 7) with population
    // 1..0xc7 gains one man (cap 0xc8).
    fn randomly_increase_all_harkonnen_troops_head_count(&mut self) {
        // = seg000:1d10 rol [rand_bits],1; jnb ret.
        if !self.roll_rand_bit() {
            return;
        }
        // = seg000:1d19..1d32 the walk ends at troops[66].
        for t in self.troops.iter_mut().take(66) {
            if t.bitfield_10 & 0x80 == 0 {
                continue;
            }
            // = seg000:1d1f..1d26 dec al; cmp al,0c7h; jnb — skips 0 and
            //   anything at the 0xc8 cap.
            if t.population.wrapping_sub(1) >= 0xc7 {
                continue;
            }
            t.population += 1;
        }
    }

    // = the `rol [rand_bits],1; jb` idiom (seg000:1d10, 1f8b, ...): rotate
    // the rolling random-bit word left and report the bit carried out.
    pub(crate) fn roll_rand_bit(&mut self) -> bool {
        let carry = self.rand_bits & 0x8000 != 0;
        self.rand_bits = self.rand_bits.rotate_left(1);
        carry
    }

    // = seg000:24d2 loc_024d2 — the shipment-fulfilment class: how many of
    // the thresholds {1, 0x40, 0x80, 0x90, 0xff} ds:be sits below (0 = fully
    // paid 0xff .. 5 = nothing paid).
    fn shipment_fulfilment_class(&self) -> u8 {
        let v = self.spice_shipment_fulfilment;
        [1u8, 0x40, 0x80, 0x90, 0xff]
            .iter()
            .filter(|&&t| v < t)
            .count() as u8
    }

    // = seg000:20a4 actions_time_in_day_3 — the Emperor's spice-shipment day
    // counter: keep days_left updated ahead of the event day, roll a new
    // demand on it, and post escalating reminders (or the room-screen
    // consequence) once one is pending.
    fn actions_time_in_day_3(&mut self) {
        // = seg000:20a4 test ds:bf,80h — the shipment plot is armed by the
        //   phase progression.
        if self.spice_shipment_flags & 0x80 == 0 {
            return;
        }
        // = seg000:20ab call get_ingame_day.
        let day = self.get_ingame_day_in_ax();
        let event_day = self.ingame_day_of_last_spice_shipment_event;
        // = seg000:20ae/20b3 during the final attack only the days_left
        //   counter is maintained (loc_02098).
        if self.final_attack_stage != 0 {
            let diff = day.wrapping_sub(event_day);
            if diff != 0 {
                self.days_left_until_spice_shipment = (diff as u8).wrapping_neg();
            }
            return;
        }
        // = seg000:20b5/20ba a pending demand posts reminders (loc_02131).
        if self.spice_shipment_flags & 0x10 != 0 {
            let diff = day.wrapping_sub(event_day);
            if diff == 0 {
                return;
            }
            // = seg000:2137/213a 4 days past: the room-screen type-7
            //   consequence (loc_0215f).
            if diff >= 4 {
                self.pending_room_screen_request = 7;
                return;
            }
            // = seg000:213c..2152 the reminder person id from the 0x2161
            //   table: index 4 * days + min(fulfilment class, 2).
            let idx = 4 * diff as usize + self.shipment_fulfilment_class().min(2) as usize;
            let person = SHIPMENT_REMINDER_TABLE[idx - 4];
            // = seg000:2154/2156 a zero entry escalates to the consequence.
            if person == 0 {
                self.pending_room_screen_request = 7;
                return;
            }
            // = seg000:2158..215c comm_add_person_sighting((person << 8) |
            //   0x0b).
            self.comm_add_person_sighting(((person as u16) << 8) | 0x0b);
            return;
        }
        // = seg000:20bc/20c3 an unpaid shipment goes straight to the
        //   consequence (loc_0215f).
        if self.spice_shipment_unpaid != 0 {
            self.pending_room_screen_request = 7;
            return;
        }
        // = seg000:20c6..20ce ahead of the event day: days_left = event day -
        //   today.
        let diff = day.wrapping_sub(event_day);
        if diff != 0 {
            self.days_left_until_spice_shipment = (diff as u8).wrapping_neg();
            return;
        }
        // = seg000:20d2 loc_020d2 — roll a new demand. Base quantity =
        //   (sequence * 150 + 100), saturating to 0xffff on overflow.
        let seq = self.spice_shipment_sequence_number;
        self.spice_shipment_sequence_number = seq.wrapping_add(1);
        let base = (seq as u32) * 0x96 + 0x64;
        // = seg000:20e9..20f2 scaled by (rand(0..0x3f) + 0xe0) / 256.
        let factor = self.rand_masked(0x3f) as u32 + 0xe0;
        let mut quantity = if base > 0xffff {
            0xffff
        } else {
            let product = base * factor;
            if product >> 24 != 0 {
                0xffff
            } else {
                (product >> 8) as u16
            }
        };
        // = seg000:20fc..210f with any of the last demand paid (ds:be bit 7
        //   clear), scale by (0x100 + !(be * 2)) / 256, saturating.
        if quantity != 0xffff && self.spice_shipment_fulfilment & 0x80 == 0 {
            let scale = 0x100 + (!(self.spice_shipment_fulfilment << 1) as u32);
            let product = quantity as u32 * scale;
            quantity = if product >> 24 != 0 {
                0xffff
            } else {
                (product >> 8) as u16
            };
        }
        // = seg000:2114..211d store the demand, reset days_left, arm bits
        //   4 + 7.
        self.spice_shipment_quantity = quantity;
        self.days_left_until_spice_shipment = 0;
        self.spice_shipment_flags |= 0x90;
        // = seg000:2122..212e the announcement sighting: person 2 with a
        //   fully-unpaid history (ds:be bit 7), person 3 otherwise.
        let person: u16 = if self.spice_shipment_fulfilment & 0x80 != 0 {
            2
        } else {
            3
        };
        self.comm_add_person_sighting((person << 8) | 0x0b);
    }

    // = seg000:1f64 actions_time_in_day_4 — the Harkonnen sietch-raid
    // scheduler: arms once game_time has passed the phase-0x2c stamp by 0x70
    // (or unconditionally from phase 0x3c), fires on even days only, can be
    // suppressed for one firing, and needs a set rolling rand bit.
    fn actions_time_in_day_4(&mut self) {
        // = seg000:1f64..1f77 the arming gates.
        if self.game_phase < 0x3c {
            let (diff, borrow) = self
                .game_time
                .overflowing_sub(self.harkonnen_raids_armed_after_game_time);
            if borrow || diff < 0x70 {
                return;
            }
        }
        // = seg000:1f79 test [game_time],10h — even days only.
        if self.game_time & 0x10 != 0 {
            return;
        }
        // = seg000:1f81..1f89 the one-shot suppress byte.
        if std::mem::take(&mut self.harkonnen_raid_suppress_once) != 0 {
            return;
        }
        // = seg000:1f8b rol [rand_bits],1; jb loc_01f92.
        if !self.roll_rand_bit() {
            return;
        }
        // = seg000:1f92..2016 the raid: pick a target sietch
        //   (harkonnen_pick_attack_target, seg000:2017), move up to two
        //   troops from the source area onto it (troop_location_084a6 +
        //   troop_arrive_at_destination), battle-flag it, queue the "The
        //   Harkonnens are attacking ...!" message and enter the night
        //   attack when it is the current location. The target picker and
        //   the re-home helper (seg000:84a6) are not ported. TODO.
        println!("actions_time_in_day_4: Harkonnen raid launch (seg000:1f92) not ported");
    }

    // = seg000:1dda actions_time_in_day_8 — the mid-day shipment reminder:
    // while a demand is pending (ds:bf bit 4), the person-3 companion is not
    // travelling with Paul, the COMM room is not current and the final
    // attack has not started, queue vision message 0x30b ("Paul! Don't
    // forget the spice shipments for the Emperor.").
    fn actions_time_in_day_8(&mut self) {
        if self.spice_shipment_flags & 0x10 == 0
            || self.persons_travelling_with & 8 != 0
            || self.current_room == 8
            || self.final_attack_stage != 0
        {
            return;
        }
        self.queue_vision_message_without_location(0x30b);
    }

    // = seg000:1c18 redraw_period_sensitive_view_content — refresh the view
    // content showing period-dependent numbers: on the full map view the
    // open troop info panel and location popup; in the room view with the
    // globe ornament up, the day/charisma stats (loc_0bdbb, unported).
    pub(crate) fn redraw_period_sensitive_view_content(&mut self) {
        // = seg000:1c18/1c1d the data_046eb routing: bit 7 = the full map.
        if self.data_046eb & 0x80 != 0 {
            // = seg000:1c1f call open_onmap_resource.
            self.open_onmap_spritesheet();
            // = seg000:1c22..1c2a an open troop info panel redraws its
            //   content.
            if let Some(ti) = self.map_info_popup_troop {
                self.map_draw_troop_info_panel_content(ti);
            }
            // = seg000:1c2d..1c35 an open location popup redraws.
            if let Some(li) = self.map_location_popup_loc {
                self.map_draw_location_popup(li);
            }
            return;
        }
        // = seg000:1c39..1c42 the room view (data_046eb == 0) with the
        //   map/globe ornament up (room_view_toggle bit 7): the globe-
        //   ornament day/charisma stats redraw (loc_0bdbb). Not ported. TODO.
        if self.data_046eb == 0 && self.room_view_toggle & 0x80 != 0 {
            println!("redraw_period_sensitive_view_content: globe stats (loc_0bdbb) not ported");
        }
    }

    // = seg000:1bec night_attack_period_step — while the night attack is up,
    // re-check the arrival consequence for the current location; once the
    // stage has cleared, clear the attack task and request a room redraw.
    fn night_attack_period_step(&mut self) {
        // = seg000:1bec cmp [night_attack_stage],0; jz ret.
        if self.night_attack_stage == 0 {
            return;
        }
        // = seg000:1bf3/1bf7 call location_related_to_dying_if_arriving_at_
        //   fortress_0503c on the current location — the attack resolution /
        //   death check. Not ported. TODO.
        println!("night_attack_period_step: attack resolution (seg000:503c) not ported");
        // = seg000:1bfa..1c01 a pending room-screen request is forced to
        //   type 6.
        if self.pending_room_screen_request != 0 {
            self.pending_room_screen_request = 6;
        }
        // = seg000:1c06..1c12 once the stage cleared: drop the attack task
        //   (loc_00b21) and request the room redraw (data_0473b bit 0).
        if self.night_attack_stage == 0 {
            self._clear_night_attack();
            self.room_redraw_request |= 1;
        }
    }

    // = seg000:661d call_callback_on_hired_troops_in_location — the troop
    // chain walk of for_each_troop_in_location, calling back only for hired
    // troops (the get_address_of_troop_by_ID carry: occupation < 0x80).
    pub(crate) fn for_each_hired_troop_in_location(
        &mut self,
        loc_index: usize,
        mut callback: impl FnMut(&mut Self, usize),
    ) {
        let mut id = self.locations[loc_index].troop_id;
        while id != 0 {
            let ti = (id - 1) as usize;
            if self.troops[ti].occupation & 0x80 == 0 {
                callback(self, ti);
            }
            id = self.troops[ti].next_troop_id;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{GameState, dat_file::DatFile};

    // One full in-game day through the event pump: the desert-walk countdown,
    // the new-day bookkeeping (phase-change day counter, production stats,
    // area percentages, smuggler restock), the ecology walk, the Harkonnen
    // end-of-day growth and the day-3 spice-shipment demand. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored time_period_events
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn time_period_events_fire_across_a_day() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // The desert-walk countdown: 0x15 decrements past the 0x10 floor and
        // zeroes itself and the ds:f5 CONDIT byte within the day.
        game.desert_walk_counter = 0x15;
        game.for_condit_desert_walk_ds_f5 = 7;
        // The production diff inputs: 100 in stock, 60 at the last new day,
        // 5 spent -> production 45.
        game.spice_in_stock = 100;
        game.spice_stock_at_last_new_day = 60;
        game.spice_spent_today = 5;
        // A vegetation-program location: 6 stage-1 cells around its map
        // offset grow its water by 6/2 + 1.
        let li = 12;
        game.locations[li].status |= 0x20;
        game.locations[li].water = 10;
        let base = game.locations[li].map_offset as usize - 1;
        for k in 0..6 {
            game.map[base + k] = (game.map[base + k] & !0x30) | 0x10;
        }
        // A stage-1 cell on the ecology LFSR's first column (state 1 shifts
        // to 0x402): promoted to stage 2 on the new day.
        game.map[0x402] = (game.map[0x402] & !0x30) | 0x10;
        // A restock-armed smuggler with empty stock: the bit-7-priced slots
        // may refill (0..3), the 0x0a-priced krys slot stays empty.
        game.smugglers[0].field_2 |= 8;
        game.smugglers[0].stock = [0; 5];
        // A Harkonnen troop grows by one at the day's 0xf period when the
        // rolling rand bit carries.
        game.rand_bits = 0x8000;
        let hark = game
            .troops
            .iter()
            .position(|t| {
                t.troop_id != 0 && t.bitfield_10 & 0x80 != 0 && (1..0xc7).contains(&t.population)
            })
            .expect("a Harkonnen troop");
        let hark_pop = game.troops[hark].population;
        // The spice-shipment plot armed with the demand day = day 1: the
        // day-3 period of day 1 rolls the demand.
        game.spice_shipment_flags = 0x80;
        game.ingame_day_of_last_spice_shipment_event = 1;

        // game_time starts at 2; 18 periods cross 0xf (Harkonnen growth),
        // 0x10 (the new day) and 0x13 (day 1's day-3 action).
        game.run_events_for_n_time_periods(18);
        while rx.try_recv().is_ok() {}

        assert_eq!(game.desert_walk_counter, 0, "the walk countdown drained");
        assert_eq!(game.for_condit_desert_walk_ds_f5, 0, "ds:f5 cleared");
        assert_eq!(game.days_since_last_game_phase_change, 1, "one new day");
        assert_eq!(game.todays_spice_production, 45, "stock 100 - 60 + spent 5");
        assert_eq!(
            game.spice_production_better_than_previous_day, 45,
            "better than yesterday's 0"
        );
        assert_eq!(game.spice_production_lower_than_previous_day, 0);
        assert_eq!(game.spice_spent_today, 0, "the spent accumulator consumed");
        assert_eq!(game.spice_stock_at_last_new_day, 100, "the new baseline");
        assert!(
            game.area_controlled_by_atreides >= 1,
            "the Atreides percentage carries the +1"
        );
        assert!(
            game.data_000ac > 0,
            "the Harkonnen population sum refreshed"
        );
        assert_eq!(game.locations[li].water, 14, "water 10 + 6 tufts/2 + 1");
        assert_eq!(game.map[0x402] & 0x30, 0x20, "the LFSR cell promoted");
        assert_ne!(game.ecology_lfsr_state, 1, "the LFSR state advanced");
        assert_eq!(
            game.smugglers[0].stock[2], 0,
            "a bit-7-less price never restocks"
        );
        assert!(game.smugglers[0].stock.iter().all(|&s| s <= 3));
        assert_eq!(
            game.troops[hark].population,
            hark_pop + 1,
            "the Harkonnen troop grew at the day's end"
        );
        assert_eq!(
            game.spice_shipment_flags & 0x10,
            0x10,
            "a shipment demand is pending"
        );
        assert_eq!(game.spice_shipment_sequence_number, 1);
        // The first demand: base 100 * (224..288)/256, then the fulfilment
        // scaling (ds:be = 0, bit 7 clear) multiplies by 0x1ff/256.
        assert!(
            (170..=225).contains(&game.spice_shipment_quantity),
            "the first demand is ~2 * 100 * (224..288)/256 spice (got {})",
            game.spice_shipment_quantity
        );
        assert!(
            game.comm_sightings.contains(&0x030b),
            "the demand's COMM sighting was posted (got {:x?})",
            game.comm_sightings
        );
    }

    // The phase-5c/5d illness subplot: the new-day picker makes the sietch
    // with the most hired troops ill, and Chani parked there cures it, ending
    // in the phase-0x60 go-find-Chani transition. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored illness_plot
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn illness_plot_and_chani_cure() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // Vision messages queue only once Paul has had his first vision.
        game.bitfield_paul_events |= 1;
        // A rallied spice-prospecting troop chained to an eligible sietch —
        // the only hired troop anywhere, so the picker must choose it.
        let li = (0..game.locations.len())
            .find(|&i| {
                game.locations[i].appearance < 0x28
                    && game.locations[i].status & 0x80 == 0
                    && i != 16
            })
            .expect("an eligible sietch");
        let li_ptr = crate::locations::location_ptr_from_index(li);
        game.locations[li].troop_id = 1;
        game.troops[0].occupation = 0x01;
        game.troops[0].next_troop_id = 0;
        game.troops[0].offset_of_location = li_ptr;
        // Phase 0x5c with the day gate armed; cross one new day.
        game.game_phase = 0x5c;
        game.illness_plot_armed_after_ingame_day = 0;
        game.run_events_for_n_time_periods(15);
        while rx.try_recv().is_ok() {}

        assert_ne!(
            game.troops[0].dissatisfaction_and_speech & 0x400,
            0,
            "the troop fell ill"
        );
        assert_ne!(game.troops[0].occupation & 0x10, 0, "and stopped working");
        assert_eq!(game.number_of_locations_with_illness, 1);
        assert_eq!(game.latest_location_with_illness, li_ptr);
        assert!(
            game.vision_messages.contains(&(0xf08, li_ptr)),
            "the strange-disease vision was queued (got {:x?})",
            game.vision_messages
        );

        // Phase 0x5d: Chani parked at the ill location, one cure step from
        // done. The next period wraps the progress to 0 and cures.
        game.game_phase = 0x5d;
        game.room_persons[7].location_appearance = ((li as u16 + 1) << 8) | 0x80;
        game.persons_in_room &= !0x80;
        game.chani_troop_illness_cure_progress = 0xf8;
        game.run_events_for_n_time_periods(1);
        while rx.try_recv().is_ok() {}

        assert_eq!(
            game.troops[0].dissatisfaction_and_speech & 0x400,
            0,
            "the troop was cured"
        );
        assert_ne!(
            game.troops[0].dissatisfaction_and_speech & 0x800,
            0,
            "and carries the was-cured speech bit"
        );
        assert!(
            game.vision_messages.contains(&(0x709, li_ptr)),
            "the cured-everybody vision was queued (got {:x?})",
            game.vision_messages
        );
        assert_eq!(game.number_of_locations_with_illness, 0);
        assert_eq!(game.latest_location_with_illness, 0, "nothing left ill");
        assert_eq!(game.game_phase, 0x60, "the go-find-Chani phase fired");
    }
}
