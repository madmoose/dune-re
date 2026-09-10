//! Battle resolution: the attacking occupation's per-period callback and the
//! chains that follow a won or a lost battle.
//!
//! Ported from the seg000:739e..767c block (the attacking callback, the
//! won/lost tails and their per-troop callbacks) plus the charisma and
//! motivation helpers they call (seg000:6f48, 6f56, 6fb0) and the capture
//! routine (seg000:668f). Routines are in DOS address order below.

use crate::{GameState, locations};

impl GameState {
    // = seg000:1243 increase_final_attack_stage_if_more_than_10K_Fremen_near_
    // Harkonnen_palace — with at least 1000 people in atomics-equipped army
    // troops at the three locations closest to the Harkonnen palace, the
    // final attack stage advances.
    pub(crate) fn increase_final_attack_stage_if_more_than_10k_fremen_near_harkonnen_palace(
        &mut self,
    ) {
        // = seg000:1243..124a bx = 0; the accumulator callback over the
        //   three locations.
        let mut population = 0u16;
        self.for_each_hired_troop_near_harkonnen_palace(|s, ti| {
            s.callback_troop_accumulate_atomics_equipped_army_troop_population(ti, &mut population);
        });
        // = seg000:124d..1253 cmp bx,3e8h; jb; inc final_attack_stage.
        if population >= 0x3e8 {
            self.final_attack_stage = self.final_attack_stage.wrapping_add(1);
        }
    }

    // = seg000:1258 for_each_hired_troop_near_harkonnen_palace — the hired
    // troops of the three locations closest to the Harkonnen palace
    // (locations[2..=4]).
    fn for_each_hired_troop_near_harkonnen_palace(
        &mut self,
        mut callback: impl FnMut(&mut Self, usize),
    ) {
        for li in 2..5 {
            self.for_each_hired_troop_in_location(li, &mut callback);
        }
    }

    // = seg000:1269 callback_troop_accumulate_atomics_equipped_army_troop_
    // population — an army troop (occupation 4) holding atomics (equipment
    // bit 2) adds its population.
    fn callback_troop_accumulate_atomics_equipped_army_troop_population(
        &self,
        ti: usize,
        population: &mut u16,
    ) {
        let t = &self.troops[ti];
        if t.occupation == 4 && t.equipment & 4 != 0 {
            *population = population.wrapping_add(t.population as u16);
        }
    }

    // = seg000:2d2c callback_event_dialogue_line_09_Stilgar_final_attack_
    // select_troops — Stilgar's final-attack line: the stage advances, and up
    // to seven atomics-equipped army troops from the three locations closest
    // to the Harkonnen palace are sent to Arrakeen and take their first
    // travel step.
    pub(crate) fn dialogue_event_09_stilgar_final_attack_select_troops(&mut self) {
        // = seg000:2d2c inc final_attack_stage.
        self.final_attack_stage = self.final_attack_stage.wrapping_add(1);
        // = seg000:2d30..2d3b a 25-word stack list, filled by
        //   store_troop_to_array_of_troop_ptrs_if_army_and_atomics_equipped
        //   and 0-terminated.
        let mut selected: Vec<usize> = Vec::new();
        self.for_each_hired_troop_near_harkonnen_palace(|s, ti| {
            s.store_troop_to_array_of_troop_ptrs_if_army_and_atomics_equipped(ti, &mut selected);
        });
        // = seg000:2d3f..2d5c the first seven (cmp si, sp + 0eh): move order
        //   to locations[1], then one travel step.
        for &ti in selected.iter().take(7) {
            self.troop_issue_move_order(ti, 1);
            self.troop_travel_step(ti);
        }
    }

    // = seg000:2d62 store_troop_to_array_of_troop_ptrs_if_army_and_atomics_
    // equipped — an army troop (occupation 4) holding atomics (equipment bit
    // 2) joins the list.
    fn store_troop_to_array_of_troop_ptrs_if_army_and_atomics_equipped(
        &self,
        ti: usize,
        list: &mut Vec<usize>,
    ) {
        let t = &self.troops[ti];
        if t.occupation == 4 && t.equipment & 4 != 0 {
            list.push(ti);
        }
    }

    // = seg000:668f troop_capture — capture the troop: unless it is already
    // away or a Harkonnen troop, mark it away (occupation bit 5), strip its
    // equipment, stamp the time and lose 4 charisma.
    pub(crate) fn troop_capture(&mut self, ti: usize) {
        let t = self.troops[ti];
        // = seg000:668f..6699 test occupation,20h; test bitfield_10,80h.
        if t.occupation & 0x20 != 0 || t.bitfield_10 & 0x80 != 0 {
            return;
        }
        // = seg000:669b..66a6.
        let game_time = self.game_time;
        let t = &mut self.troops[ti];
        t.occupation |= 0x20;
        t.equipment = 0;
        t.time_period_of_ralliement = game_time;
        // = seg000:66aa/66ac al = 4; call decrease_charisma_and_decrease_
        //   troop_motivation_accordingly.
        self.decrease_charisma(4);
    }

    // = seg000:68e0 troop_finish_conversion — the tail of
    // callback_troop_convert_captured_harkonnen: clear bitfield_10 bit 4,
    // count the troop and respawn its map icon.
    pub(crate) fn troop_finish_conversion(&mut self, ti: usize, count: &mut u16) {
        self.troops[ti].bitfield_10 &= 0xffef;
        *count += 1;
        self.troop_respawn_map_icon(ti);
    }

    // = seg000:6f48 troop_increase_motivation — motivation += `by`, capped
    // at 100.
    pub(crate) fn troop_increase_motivation(&mut self, ti: usize, by: u8) {
        let t = &mut self.troops[ti];
        t.motivation = t.motivation.wrapping_add(by).min(0x64);
    }

    // = seg000:6f56 increase_motivation_for_all_active_troops — +`by`
    // motivation (capped at 100) on every troop that is neither away nor
    // unhired (occupation bits 5 and 7 clear).
    pub(crate) fn increase_motivation_for_all_active_troops(&mut self, by: u8) {
        for t in self.troops.iter_mut() {
            if t.occupation & 0xa0 == 0 {
                t.motivation = t.motivation.wrapping_add(by).min(0x64);
            }
        }
    }

    // = seg000:6fb0 decrease_charisma_and_decrease_troop_motivation_accordingly
    // — charisma -= amount, floored at 1; every 4 whole points lost take one
    // motivation point from every active troop.
    pub(crate) fn decrease_charisma(&mut self, amount: u8) {
        // = seg000:6fb0..6fb9 al = charisma - amount, or 1 on a borrow / zero.
        let old = self.charisma;
        let new = if old > amount { old - amount } else { 1 };
        self.charisma = new;
        // = seg000:6fbb..6fc8 steps = ((old & 0xfc) - (new & 0xfc)) >> 2.
        let steps = (old & 0xfc).wrapping_sub(new & 0xfc) >> 2;
        if steps == 0 {
            return;
        }
        // = seg000:6fcd..6fe1 every troop with occupation bits 5 and 7 clear.
        for ti in 0..self.troops.len() {
            if self.troops[ti].occupation & 0xa0 == 0 {
                self.troop_decrease_motivation(ti, steps);
            }
        }
    }

    // = seg000:7317 menu_callback_choice_massive_attack — the MASSIVE ATTACK
    // night-attack verb: up to 16 rounds of the whole location fighting at
    // once (one roll against the battle balance picks the round callback for
    // all of them), the killed strengths into ds:98/9a for the dialogue,
    // then 20 rounds of the night-attack sky flash forced to one of its two
    // periods, and the scheduler's refresh tail.
    pub(crate) fn menu_callback_choice_massive_attack(&mut self, _text_id: u16, _index: usize) {
        // = seg000:7317 massive_attack_active = 1.
        self.massive_attack_active = 1;
        if let Some(attack) = self.attack.as_mut() {
            attack.set_massive_attack(1);
        }
        let li = self.current_location_index as usize;
        // = seg000:731c..732c stage the strengths; ds:98/9a = the strengths
        //   before the fight.
        self.condit_stage_location_strengths(li);
        self.location_condit.harkonnen_killed_ds_98 = self.location_condit.harkonnen_strength;
        self.location_condit.fremen_killed_ds_9a = self.location_condit.fremen_strength;
        // = seg000:732f..733b rand; cmp al,[battle_balance]; the round
        //   callback: below the balance the hit pass, else the loss pass.
        let al = self.rand() as u8;
        let hit = al < self.location_condit.battle_balance;
        // = seg000:733e..735d up to 16 rounds: re-stage, run the pass over
        //   the location's troops, stop once a side's staged strength is 0.
        for _ in 0..16 {
            self.condit_stage_location_strengths(li);
            self.for_each_troop_in_location(li, |s, ti| {
                if hit {
                    s.callback_troop_massive_attack_hit(ti, li);
                } else {
                    s.callback_troop_massive_attack_loss(ti, li);
                }
            });
            if self.location_condit.harkonnen_strength == 0
                || self.location_condit.fremen_strength == 0
            {
                break;
            }
        }
        // = seg000:735f..736c the strengths left come off ds:98/9a.
        self.condit_stage_location_strengths(li);
        self.location_condit.harkonnen_killed_ds_98 = self
            .location_condit
            .harkonnen_killed_ds_98
            .wrapping_sub(self.location_condit.harkonnen_strength);
        self.location_condit.fremen_killed_ds_9a = self
            .location_condit
            .fremen_killed_ds_9a
            .wrapping_sub(self.location_condit.fremen_strength);
        // = seg000:7370..738f 20 rounds: rand_masked(201h) — al picks the
        //   sky-flash period 0bh / 11h, ah (0 or 2) pads the 28h-tick wait.
        for _ in 0..20 {
            let r = self.rand_masked(0x201);
            let period: i8 = if r & 1 == 0 { 0x0b } else { 0x11 };
            if let Some(attack) = self.attack.as_mut() {
                attack.set_sky_flash_timer(period);
            }
            self.wait_interruptable(0x28 + ((r >> 8) & 0xff) as u64);
        }
        // = seg000:7391 massive_attack_active = 0; 7396 jmp events_refresh_tail.
        self.massive_attack_active = 0;
        if let Some(attack) = self.attack.as_mut() {
            attack.set_massive_attack(0);
        }
        self.events_refresh_tail();
    }

    // = seg000:7419 callback_troop_massive_attack_hit — the massive attack's
    // per-troop pass when the Fremen won the round's roll: an attacking
    // troop (occupation 6) goes to location_battle_won, anyone else to
    // attack_deal_casualties.
    fn callback_troop_massive_attack_hit(&mut self, ti: usize, li: usize) {
        if self.troops[ti].occupation == 6 {
            self.location_battle_won(ti, li);
        } else {
            self.attack_deal_casualties(ti, li);
        }
    }

    // = seg000:7399 troop_make_occupation_military_training — occupation = 4.
    fn troop_make_occupation_military_training(&mut self, ti: usize) {
        self.troops[ti].occupation = 4;
    }

    // = seg000:739e callback_troop_location_for_troop_occupation_attacking —
    // one time period of an attack by troop `ti` on its location. At
    // Arrakeen it is the final attack: the Harkonnen surrender. Elsewhere
    // the location strengths are staged; with no Harkonnen strength left the
    // battle is won, otherwise a roll against the battle balance decides
    // whether the troop deals casualties (loc_073ef) or takes them
    // (troop_attack_take_losses).
    pub(crate) fn troop_occupation_event_attacking(&mut self, ti: usize) {
        let li = locations::location_index_from_ptr(self.troops[ti].offset_of_location);
        // = seg000:739e or [harkonnen_raid_suppress_once],1.
        self.harkonnen_raid_suppress_once |= 1;
        // = seg000:73a3 cmp location,locations[1]; jnz not_final_attack.
        if li == 1 {
            // = seg000:73a9 inc final_attack_stage.
            self.final_attack_stage = self.final_attack_stage.wrapping_add(1);
            // = seg000:73ad..73b0 every hired troop here -> military training.
            self.for_each_hired_troop_in_location(li, |s, tj| {
                s.troop_make_occupation_military_training(tj);
            });
            // = seg000:73b3 call location_evict_unhired_harkonnen_troops.
            self.location_evict_unhired_harkonnen_troops(li);
            // = seg000:73b6..73cf every Harkonnen zone cell of the map (bits
            //   5-4 == 0x30) becomes an Atreides one (0x20).
            for cell in self.map.iter_mut() {
                if *cell & 0x30 == 0x30 {
                    *cell &= 0xef;
                }
            }
            // = seg000:73d1..73d6 message 0x0a "The shield is down, the
            //   Harkonnen troops here have surrendered!" for Arrakeen.
            self.queue_vision_message_f00(0x0a, 1);
            return;
        }
        // = seg000:73d9 call condit_stage_location_strengths.
        self.condit_stage_location_strengths(li);
        // = seg000:73dc cmp [for_condit_ds_94],0; jz location_battle_won.
        if self.location_condit.harkonnen_strength == 0 {
            self.location_battle_won(ti, li);
            return;
        }
        // = seg000:73e3..73ec rand; cmp al,[battle_balance]; jb
        //   attack_deal_casualties; jmp troop_attack_take_losses.
        let al = self.rand() as u8;
        if al >= self.location_condit.battle_balance {
            self.troop_attack_take_losses(ti, li);
            return;
        }
        self.attack_deal_casualties(ti, li);
    }

    // = seg000:73ef attack_deal_casualties — the hit: the attacking troop's
    // battle strength spread over the Harkonnen troops here, +1, is the
    // casualty strength dealt to each of them; the kills go to the troop's
    // occupation tally, and with no Harkonnen survivor the battle is won.
    fn attack_deal_casualties(&mut self, ti: usize, li: usize) {
        // = seg000:73ef..73fb location_count_harkonnen_and_attacking_troops;
        //   troop_battle_strength; div cx unless cx is 0.
        let (harkonnen, _) = self.location_count_harkonnen_and_attacking_troops(li);
        let strength = self.troop_battle_strength(ti);
        let ax = strength.checked_div(harkonnen).unwrap_or(strength);
        // = seg000:73fd..7405 dl = al + 1, saturating at 0xff; dh = 0.
        let dx = ((ax as u8) as u16 + 1).min(0xff);
        // = seg000:7407..740e cx = 0 (the kills), bx = 0 (the survivors);
        //   callback_troop_harkonnen_casualties over the location.
        let mut kills = 0u16;
        let mut survivors = 0u16;
        self.for_each_troop_in_location(li, |s, tj| {
            s.callback_troop_harkonnen_casualties(tj, dx, &mut kills, &mut survivors);
        });
        // = seg000:7411 add troop->troop_occupation_dependent_C,cx — the
        //   "Harkonnen killed" tally.
        self.troops[ti].harvest_rate = self.troops[ti].harvest_rate.wrapping_add(kills);
        // = seg000:7414/7416 or bx,bx; jz location_battle_won.
        if survivors == 0 {
            self.location_battle_won(ti, li);
        }
    }

    // = seg000:7429 location_battle_won — the battle at `li` is won by troop
    // `ti`'s side: the "we won the battle" vision unless the player is
    // there; a sietch clears its in-battle bit and runs the after-battle
    // troop pass, a fortress goes through location_battle_won_for_fortress.
    pub(crate) fn location_battle_won(&mut self, ti: usize, li: usize) {
        let _ = ti;
        // = seg000:7429..7431 cmp location,[current_location_ptr]; jz; message
        //   7 "We won the battle Muad'Dib, here in ...! Yaoouuuh!".
        if li != self.current_location_index as usize {
            self.queue_vision_message_f00(7, li);
        }
        // = seg000:7434/7438 cmp appearance,28h; jnb location_battle_won_for_
        //   fortress.
        if self.locations[li].appearance >= 0x28 {
            self.location_battle_won_for_fortress(li);
            return;
        }
        // = seg000:743a and status,0fdh — clear the in-battle bit.
        self.locations[li].status &= 0xfd;
        // = seg000:743e/7441 bp = callback_troop_after_battle_won; jmp
        //   location_battle_won_tail.
        self.location_battle_won_tail(li, false);
    }

    // = seg000:7443 location_battle_won_for_fortress_07443 — a Harkonnen
    // fortress falls: it becomes discoverable, the Atreides zone is stamped
    // around it (radius 5), it turns into a sietch two days later, +4
    // charisma and +1 motivation for every troop, status bit 3; then the
    // after-battle pass over the hired troops and the conversion pass over
    // the captured Harkonnen troops.
    pub(crate) fn location_battle_won_for_fortress(&mut self, li: usize) {
        // = seg000:7445/7449 discoverable_at_phase = 5; call location_stamp_
        //   atreides_zone_on_map (the radius).
        self.locations[li].discoverable_at_phase = 5;
        self.location_stamp_atreides_zone_on_map(li);
        // = seg000:744e..7453 discoverable_at_phase = day + 2 — the fortress
        //   becomes a sietch two days later.
        let day = self.get_ingame_day() as u8;
        self.locations[li].discoverable_at_phase = day.wrapping_add(2) as i8;
        // = seg000:7456..745d.
        self.increase_charisma(4);
        self.increase_motivation_for_all_active_troops(1);
        // = seg000:7460 or status,8.
        self.locations[li].status |= 8;
        // = seg000:7464..746f won_fortress_regions |= 8000h rol first_name
        //   (bit first_name - 1); the map overlay open consumes it.
        let cl = self.locations[li].first_name as u32;
        self.won_fortress_regions = 0x8000u16.rotate_left(cl);
        // = seg000:7470/7473 callback_troop_after_battle_won over the
        //   location.
        self.for_each_troop_in_location(li, |s, tj| {
            s.callback_troop_after_battle_won(tj, li);
        });
        // = seg000:7476 bp = callback_troop_convert_captured_harkonnen; falls
        //   into location_battle_won_tail.
        self.location_battle_won_tail(li, true);
    }

    // = seg000:7479 location_battle_won_tail — the shared tail of the won
    // battle: run the per-troop pass (`convert`: the captured-Harkonnen
    // conversion of the fortress path, else the after-battle pass), keep 0 or
    // 1 Harkonnen troop enslaved and remove the rest, re-accumulate the
    // Harkonnen spice production, arm final_attack_stage = 1 with the
    // atomics top-up, and mark the map view dirty.
    fn location_battle_won_tail(&mut self, li: usize, convert: bool) {
        // = seg000:7479 call call_callback_on_all_troops_in_location with the
        //   caller's bp. The conversion pass counts in cx, which DOS leaves
        //   as the zone stamp's loop counter (0 after a complete disc).
        if convert {
            let mut count = 0u16;
            self.for_each_troop_in_location(li, |s, tj| {
                s.callback_troop_convert_captured_harkonnen(tj, li, &mut count);
            });
        } else {
            self.for_each_troop_in_location(li, |s, tj| {
                s.callback_troop_after_battle_won(tj, li);
            });
        }
        // = seg000:747c..7486 dx = 1 when rand_bits & 3 == 0, else 0 — how
        //   many Harkonnen troops stay enslaved.
        let keep = if self.rand_bits & 3 == 0 { 1u16 } else { 0 };
        // = seg000:7487..7491 cx = 0; callback_troop_enslave_harkonnen over
        //   the location; repeat while cx > dx (a removal breaks the chain
        //   walk).
        loop {
            let mut count = 0u16;
            self.for_each_troop_in_location(li, |s, tj| {
                s.callback_troop_enslave_harkonnen(tj, keep, &mut count);
            });
            if count <= keep {
                break;
            }
        }
        // = seg000:7495 call accumulate_harkonnen_spice_production.
        self.accumulate_harkonnen_spice_production();
        // = seg000:7498/749b cmp dl,1; ja — dl is 0 or 1, so this always
        //   runs: final_attack_stage = 1, room persons 1 and 2 lose flag
        //   bit 1, and the atomics top-up.
        if keep <= 1 {
            self.final_attack_stage = 1;
            self.room_persons[1].flags &= 0xfd;
            self.room_persons[2].flags &= 0xfd;
            self.location_top_up_atomics(li);
        }
        // = seg000:74b3 jmp location_mark_map_view_dirty.
        self.location_mark_map_view_dirty(li);
    }

    // = seg000:74b6 location_battle_lost — the battle at `li` is lost: clear
    // the in-battle bit; with the player there request room screen 6.
    // Otherwise a sietch turns into a fortress (its room persons move to
    // room 3), every troop there is captured or, a Harkonnen one, marked
    // stationed, the Harkonnen zone is stamped around it (radius 5), status
    // bits 0 and 3 clear, and the map view is marked dirty.
    pub(crate) fn location_battle_lost(&mut self, li: usize) {
        // = seg000:74b6 and status,0fdh.
        self.locations[li].status &= 0xfd;
        // = seg000:74ba/74be cmp location,[current_location_ptr]; jz
        //   loc_07500: pending_room_screen_request = 6.
        if li == self.current_location_index as usize {
            self.pending_room_screen_request = 6;
            return;
        }
        // = seg000:74c0..74c5 a sietch (appearance < 28h) becomes a fortress.
        let appearance = self.locations[li].appearance;
        if appearance < 0x28 {
            // = seg000:74c7..74cb appearance = (appearance & 7) + 28h.
            self.locations[li].appearance = (appearance & 7) + 0x28;
            // = seg000:74ce dec discovered_sietch_count.
            self.discovered_sietch_count = self.discovered_sietch_count.wrapping_sub(1);
            // = seg000:74d3..74e8 location_entry_room_dx_bx; dl = 3: the nine
            //   room persons whose location code is this location's move to
            //   its room 3.
            let (dx, bx) = self.location_entry_room_codes(li);
            let dx = (dx & 0xff00) | 3;
            for person in self.room_persons.iter_mut().take(9) {
                if person.location_appearance == bx {
                    person.location_and_room = dx;
                }
            }
        }
        // = seg000:74eb/74ee callback_troop_after_battle_lost over the location.
        self.for_each_troop_in_location(li, |s, tj| {
            s.callback_troop_after_battle_lost(tj);
        });
        // = seg000:74f2/74f5 cx = 5; call location_stamp_harkonnen_zone_on_map.
        self.location_stamp_harkonnen_zone_on_map(li, 5);
        // = seg000:74f9 and status,0f6h.
        self.locations[li].status &= 0xf6;
        // = seg000:74fd jmp location_mark_map_and_minimap_dirty.
        self.location_mark_map_and_minimap_dirty(li);
    }

    // = seg000:7506 callback_troop_after_battle_lost — a Harkonnen troop gets
    // bitfield_10 bit 4, anyone else is marked away (occupation bit 5).
    fn callback_troop_after_battle_lost(&mut self, ti: usize) {
        let t = &mut self.troops[ti];
        if t.bitfield_10 & 0x80 != 0 {
            t.bitfield_10 |= 0x10;
        } else {
            t.occupation |= 0x20;
        }
    }

    // = seg000:751d troop_attack_take_losses — the attacking troop lost the
    // roll: casualties from the Harkonnen strength per Fremen troop here; a
    // routed troop repopulates at 0x1e..0x9d and is captured, and with no
    // attacker left the location is lost.
    fn troop_attack_take_losses(&mut self, ti: usize, li: usize) {
        // = seg000:751d..752a ax = [for_condit_ds_94] / [array_for_condit_ds_60]
        //   (the Fremen troop count; 0 leaves ax as is).
        let strength = self.location_condit.harkonnen_strength;
        let fremen = self.location_condit.troop_counts[0] as u16;
        let ax = strength.checked_div(fremen).unwrap_or(strength);
        // = seg000:752c/752e dx = ax; call troop_casualties.
        let losses = self.troop_casualties(ti, ax);
        // = seg000:7531 add troop->troop_occupation_dependent_E,ax — the
        //   "Fremen lost" tally.
        self.troops[ti].harvest_total = self.troops[ti].harvest_total.wrapping_add(losses);
        // = seg000:7534/7537 sub population,al; ja ret — survivors keep
        //   fighting.
        let pop = self.troops[ti].population.wrapping_sub(losses as u8);
        self.troops[ti].population = pop;
        if pop != 0 {
            return;
        }
        // = seg000:7539..7541 population = rand_masked(7fh) + 1eh.
        let r = self.rand_masked(0x7f) as u8;
        self.troops[ti].population = r.wrapping_add(0x1e);
        // = seg000:7544 call troop_capture.
        self.troop_capture(ti);
        // = seg000:7547..754c location_count_harkonnen_and_attacking_troops;
        //   or dx,dx; jnz ret — other attackers remain.
        let (_, attacking) = self.location_count_harkonnen_and_attacking_troops(li);
        if attacking != 0 {
            return;
        }
        // = seg000:754e call location_battle_lost.
        self.location_battle_lost(li);
    }

    // = seg000:7516 callback_troop_massive_attack_loss — the massive attack's
    // per-troop pass when the Fremen lost the round's roll: an attacking
    // troop (occupation 6) takes losses.
    fn callback_troop_massive_attack_loss(&mut self, ti: usize, li: usize) {
        if self.troops[ti].occupation == 6 {
            self.troop_attack_take_losses(ti, li);
        }
    }

    // = seg000:7552 callback_troop_harkonnen_casualties — the attack hit's
    // per-troop callback: each Harkonnen troop takes troop_casualties(dx)
    // (`kills` accumulates them, `survivors` counts the troops still
    // standing); a wiped-out troop is marked away (occupation bit 5,
    // bitfield_10 bit 4), loses its atomics on a 1-in-4 roll, and drops its
    // map icon.
    fn callback_troop_harkonnen_casualties(
        &mut self,
        ti: usize,
        dx: u16,
        kills: &mut u16,
        survivors: &mut u16,
    ) {
        // = seg000:7552/7556 test bitfield_10,80h; jz ret.
        if self.troops[ti].bitfield_10 & 0x80 == 0 {
            return;
        }
        // = seg000:7558..755e inc bx; casualties; add cx,ax; sub population,al.
        *survivors += 1;
        let al = self.troop_casualties(ti, dx);
        *kills = kills.wrapping_add(al);
        let pop = self.troops[ti].population.wrapping_sub(al as u8);
        self.troops[ti].population = pop;
        // = seg000:7561 ja ret — still standing.
        if pop != 0 {
            return;
        }
        // = seg000:7563..756b away, stationed, and not a survivor after all.
        self.troops[ti].occupation |= 0x20;
        self.troops[ti].bitfield_10 |= 0x10;
        *survivors -= 1;
        // = seg000:756f..757e rand_masked(3); jnz — a 1-in-4 roll strips the
        //   troop's atomics from it and from the location's equipment.
        if self.rand_masked(3) == 0 {
            let li = locations::location_index_from_ptr(self.troops[ti].offset_of_location);
            self.troops[ti].equipment &= 0xfb;
            self.troop_unregister_equipment_from_location(ti, li);
        }
        // = seg000:7581..7586 troop_find_icon; jnz; troop_icon_remove.
        if let Some(icon) = self.troop_find_icon(ti) {
            self.troop_icon_remove(icon);
        }
    }

    // = seg000:758d troop_casualties — the casualties strength `dx` deals to
    // troop `ti`: (dx * (0xff - 2 * army_skill)) >> 8, saturated at 0xff and
    // capped at the troop's population.
    fn troop_casualties(&self, ti: usize, dx: u16) -> u16 {
        let t = &self.troops[ti];
        // = seg000:758e..7599 dl = 0xff - army_skill - army_skill; mul dx.
        let factor = 0xffu8.wrapping_sub(t.army_skill).wrapping_sub(t.army_skill) as u32;
        let product = dx as u32 * factor;
        // = seg000:759b..75a3 al = ah of the low word, 0xff when the high
        //   word is non-zero.
        let mut al = if product >> 16 != 0 {
            0xff
        } else {
            ((product >> 8) & 0xff) as u8
        };
        // = seg000:75a6..75ab cap at the population.
        if al > t.population {
            al = t.population;
        }
        al as u16
    }

    // = seg000:75af callback_troop_after_battle_won — per hired troop after a
    // won battle: the prospector troop goes back to prospecting (occupation
    // bit 5 cleared, viability re-tested); a troop that was away becomes
    // 0x22; anyone else gets bitfield_10 bit 10 (bit 5 too at a fortress),
    // +4 motivation, +3 army skill and occupation 4 (military training).
    fn callback_troop_after_battle_won(&mut self, ti: usize, li: usize) {
        // = seg000:75af jnb ret — hired troops only (the walk's carry).
        if self.troops[ti].occupation & 0x80 != 0 {
            return;
        }
        // = seg000:75b1/75b5 cmp troop,troops[2]; jz loc_075e3.
        if ti == 2 {
            // = seg000:75e3/75e7 and occupation,0dfh; jmp troop_location_test_
            //   for_location_area_prospected.
            self.troops[ti].occupation &= 0xdf;
            self.troop_location_test_for_location_area_prospected(ti, li);
            return;
        }
        // = seg000:75b7/75bb test occupation,20h; jnz loc_075de: occupation = 22h.
        if self.troops[ti].occupation & 0x20 != 0 {
            self.troops[ti].occupation = 0x22;
            return;
        }
        // = seg000:75bd..75c8 bitfield_10 |= 0x400, and 0x20 at a fortress.
        self.troops[ti].bitfield_10 |= 0x400;
        if self.locations[li].appearance >= 0x28 {
            self.troops[ti].bitfield_10 |= 0x20;
        }
        // = seg000:75cc..75db +4 motivation, +3 army skill, occupation 4.
        self.troop_increase_motivation(ti, 4);
        self.troop_raise_skill(ti, 1, 3);
        self.troop_set_occupation(ti, 4);
    }

    // = seg000:75ea callback_troop_convert_captured_harkonnen — per unhired
    // troop after a fortress is won: a Harkonnen troop is relinked into the
    // location as a Fremen troop (bit 7 cleared) with random population
    // 0x64..0xe3, motivation 0x14..0x23, skills 0x0a..0x29 and no equipment;
    // past the eighth conversion (`count`) it is removed instead.
    fn callback_troop_convert_captured_harkonnen(&mut self, ti: usize, li: usize, count: &mut u16) {
        // = seg000:75ea jb ret — unhired troops only (the walk's carry).
        if self.troops[ti].occupation & 0x80 == 0 {
            return;
        }
        // = seg000:75ec/75f0 test bitfield_10,80h; jz ret.
        if self.troops[ti].bitfield_10 & 0x80 == 0 {
            return;
        }
        // = seg000:75f2..75f9 unlink, clear bit 7, relink (a Fremen slot).
        self.troop_unlink_from_location_chain(ti);
        self.troops[ti].bitfield_10 &= 0xff7f;
        self.troop_link_into_location(ti, li);
        // = seg000:75fc/75ff cmp cl,8; jnb loc_07655 — remove and count.
        if (*count as u8) >= 8 {
            self.troop_remove_from_play(ti);
            *count += 1;
            return;
        }
        // = seg000:7601..7623 occupation 0xa0; population/motivation =
        //   rand_masked(0f7fh) + 1464h; spice/army skill = rand_masked(1f1fh)
        //   + 0a0ah; equipment 0.
        let ax = self.rand_masked(0x0f7f).wrapping_add(0x1464);
        let skills = self.rand_masked(0x1f1f).wrapping_add(0x0a0a);
        let t = &mut self.troops[ti];
        t.occupation = 0xa0;
        t.population = ax as u8;
        t.motivation = (ax >> 8) as u8;
        t.spice_skill = skills as u8;
        t.army_skill = (skills >> 8) as u8;
        t.equipment = 0;
        // = seg000:7627 jmp troop_finish_conversion.
        self.troop_finish_conversion(ti, count);
    }

    // = seg000:762a callback_troop_enslave_harkonnen — per unhired Harkonnen
    // troop after a won battle: the first `keep` become enslaved (occupation
    // 0xac, bitfield_10 bit 4, population 0, no equipment); the rest are
    // removed from play. `count` counts both.
    fn callback_troop_enslave_harkonnen(&mut self, ti: usize, keep: u16, count: &mut u16) {
        // = seg000:762a jb ret — unhired troops only (the walk's carry).
        if self.troops[ti].occupation & 0x80 == 0 {
            return;
        }
        // = seg000:762c/7630 test bitfield_10,80h; jz ret.
        if self.troops[ti].bitfield_10 & 0x80 == 0 {
            return;
        }
        // = seg000:7632/7634 cmp cx,dx; jnb loc_07655 — remove and count.
        if *count >= keep {
            self.troop_remove_from_play(ti);
            *count += 1;
            return;
        }
        // = seg000:7636..764b.
        let t = &mut self.troops[ti];
        t.occupation = 0xac;
        t.bitfield_10 |= 0x10;
        t.population = 0;
        t.harvest_rate = 0;
        t.equipment = 0;
        *count += 1;
    }

    // = seg000:765e location_top_up_atomics — sum the atomics of every
    // location; when the total is at least 10, add (total - 10) to `li`'s
    // atomics.
    fn location_top_up_atomics(&mut self, li: usize) {
        let total = self
            .locations
            .iter()
            .fold(0u8, |acc, l| acc.wrapping_add(l.equipment.atomics));
        // = seg000:7671/7674 sub cl,0ah; jb ret.
        if total < 10 {
            return;
        }
        let a = &mut self.locations[li].equipment.atomics;
        *a = a.wrapping_add(total - 10);
    }
}
