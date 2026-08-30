//! The smuggler inventories — the seg001:10d8 table of six 0x11-byte records
//! (struct Smuggler): region, haggling attitude, two state bytes, five stock
//! counts and their prices. The new-day hook (events.rs) restocks empty
//! slots whose price byte has bit 7; the trade UI that spends them is not
//! yet ported.

use crate::GameState;

/// = one 0x11-byte record of the smugglers table (seg001:10d8).
#[derive(Clone, Copy)]
pub(crate) struct Smuggler {
    pub(crate) region: u8,
    pub(crate) willingness_to_haggle: u8,
    /// +2 — state flags; bit 3 arms the daily restock (seg000:1cae).
    pub(crate) field_2: u8,
    pub(crate) field_3: u8,
    /// +4..+8 — the five stock counts: harvesters, ornithopters, krys
    /// knives, laser guns, weirding modules.
    pub(crate) stock: [u8; 5],
    /// +9..+0xd — the matching prices; bit 7 marks a slot the daily restock
    /// may refill.
    pub(crate) prices: [u8; 5],
    /// +0xe — the open bill (spice owed to this smuggler); 0 = none.
    pub(crate) bill_value: u16,
    /// +0x10 — the in-game day the bill was raised (get_ingame_day).
    pub(crate) bill_day: u8,
}

/// = seg001:10d8 — the table's seg001 offset; record pointers are
/// SMUGGLERS_SEG001_OFS + index * 0x11 (data_0113f and room_persons[13].
/// field_c hold such pointers).
pub(crate) const SMUGGLERS_SEG001_OFS: u16 = 0x10d8;
const SMUGGLER_RECORD_SIZE: u16 = 0x11;

/// The seg001 pointer of smugglers[index].
pub(crate) fn smuggler_ptr(index: usize) -> u16 {
    SMUGGLERS_SEG001_OFS + index as u16 * SMUGGLER_RECORD_SIZE
}

/// The smugglers[] index a seg001 record pointer addresses, if it lies in
/// the table.
pub(crate) fn smuggler_index_from_ptr(ptr: u16) -> Option<usize> {
    let rel = ptr.checked_sub(SMUGGLERS_SEG001_OFS)?;
    let (index, rem) = (rel / SMUGGLER_RECORD_SIZE, rel % SMUGGLER_RECORD_SIZE);
    (rem == 0 && index < 6).then_some(index as usize)
}

const fn sm(region: u8, willingness_to_haggle: u8, stock: [u8; 5], prices: [u8; 5]) -> Smuggler {
    Smuggler {
        region,
        willingness_to_haggle,
        field_2: 0,
        field_3: 0,
        stock,
        prices,
        bill_value: 0,
        bill_day: 0,
    }
}

impl GameState {
    // = seg000:2239 callback_event_dialogue_line_08_Duncan_Idaho — Duncan's
    // dialogue event 0x08: reset the argue state, stage the shipment
    // amounts from the spice in stock, then pick the smuggler whose open
    // bill is the oldest (skipping records with state bits 0x60) and stage
    // him for CONDIT. With no open bill anywhere, rotate current_smuggler_ptr
    // to the next record that has one; when none has, nothing is staged.
    pub(crate) fn dialogue_event_08_duncan_idaho(&mut self) {
        // = seg000:2239..2241 ds:9f = 0, ds:20 = 0, ds:1a = 0.
        self.accept_refuse_argue_choice_ds_9f = 0;
        self.current_smuggler_bill_value_ds_20 = 0;
        self.related_to_arguing_ds_1a = 0;
        // = seg000:2244..224b spice in stock -> ds:9f = 3 (ARGUE possible).
        let spice = self.spice_in_stock;
        if spice != 0 {
            self.accept_refuse_argue_choice_ds_9f = 3;
        }
        // = seg000:2250 call related_to_arguing_about_spice_amounts_with_
        //   Duncan_Idaho (ax = spice_in_stock).
        self.stage_spice_argue_amounts_with_duncan(spice);
        // = seg000:2253..227a the oldest-bill scan: dl = the best age so far
        //   (0 = none), di = its record.
        let day = self.get_ingame_day() as u8;
        let mut best_age = 0u8;
        let mut best = None;
        for (i, s) in self.smugglers.iter().enumerate() {
            // = seg000:225b cmp word [si+0eh],0; jz — no bill.
            if s.bill_value == 0 {
                continue;
            }
            // = seg000:2261 test byte [si+2],60h; jnz — excluded states.
            if s.field_2 & 0x60 != 0 {
                continue;
            }
            // = seg000:2267..2272 ah = day - bill_day; keep the largest.
            let age = day.wrapping_sub(s.bill_day);
            if age <= best_age {
                continue;
            }
            best_age = age;
            best = Some(i);
        }
        let index = match best {
            // = seg000:227c..2280 a bill was found: si = di.
            Some(i) => i,
            None => {
                // = seg000:2282..229f walk on from current_smuggler_ptr
                //   (wrapping at the table's 0xff terminator) to the next
                //   record with a bill; back at the start with none -> ret.
                let start = smuggler_index_from_ptr(self.current_smuggler_ptr).unwrap_or(0);
                let mut i = start;
                loop {
                    i = (i + 1) % self.smugglers.len();
                    if self.smugglers[i].bill_value != 0 {
                        break;
                    }
                    if i == start {
                        return;
                    }
                }
                self.current_smuggler_ptr = smuggler_ptr(i);
                i
            }
        };
        // = seg000:22a3 call loc_0235f (stage_smuggler_for_condit).
        self.stage_smuggler_for_condit(index);
        // = seg000:22a6..22ad subst_id_06 = the record's region byte — the
        //   placeholder 0x86 names the smuggler's region.
        self.string_subst_id_table[6] = self.smugglers[index].region as u16;
    }

    // = seg000:22b1 related_to_arguing_about_spice_amounts_with_Duncan_Idaho
    // — stage the four amounts the shipment argument quotes (ds:b4..ba)
    // from the stock `ax` and the demand ds:bc, and set ds:bf bits 1/2 to
    // the bracket the stock falls in: below the demand (no bits), below
    // 1.5x (bit 1), below 2x (bit 2), or at least 2x (both).
    pub(crate) fn stage_spice_argue_amounts_with_duncan(&mut self, ax: u16) {
        // = seg000:22b4 and ds:bf, 0f9h.
        self.spice_shipment_flags &= 0xf9;
        // = seg000:22b9..22d1 bx = demand; cx = bx * 1.5; dx = bx * 2;
        //   si = ax / 2; di = ax / 4 + ax / 2.
        let bx = self.spice_shipment_quantity;
        let cx = (bx >> 1).wrapping_add(bx);
        let dx = bx.wrapping_add(bx);
        let si = ax >> 1;
        let di = (ax >> 2).wrapping_add(si);
        // = seg000:22d3 cmp ax,bx; jb loc_022f1.
        let table = if ax < bx {
            // = seg000:22f1..22ff.
            [ax, di, si, di.wrapping_sub(si)]
        } else if ax < cx {
            // = seg000:22d7..22da, 2300..230b.
            self.spice_shipment_flags |= 2;
            [bx, ax, di, si]
        } else if ax < dx {
            // = seg000:230c..2317.
            self.spice_shipment_flags |= 4;
            [bx, ax, di, cx]
        } else {
            // = seg000:22e5..22f0.
            self.spice_shipment_flags |= 6;
            [bx, ax, cx, dx]
        };
        self.spice_shipment_arguing_ds_b4 = table;
    }

    // = seg000:235f stage_smuggler_for_condit — make smugglers[index] the
    // Smugglers room-person's record (room_persons[13].field_c) and copy its
    // state byte, bill, bill age and haggling attitude into the CONDIT bytes
    // ds:1c / ds:20 / ds:1f / ds:1d.
    pub(crate) fn stage_smuggler_for_condit(&mut self, index: usize) {
        let s = self.smugglers[index];
        // = seg000:235f mov [room_persons[13].field_c], si.
        self.room_persons[13].field_c = smuggler_ptr(index);
        // = seg000:2363/2369 ds:1c = state byte; ds:20 = the bill.
        self.related_to_paying_smuggler_bills_ds_1c = s.field_2;
        self.current_smuggler_bill_value_ds_20 = s.bill_value;
        // = seg000:236f..237e ds:1f = 0, or the bill's age in days.
        self.related_to_paying_smuggler_bills_ds_1f = 0;
        if s.bill_value != 0 {
            self.related_to_paying_smuggler_bills_ds_1f =
                (self.get_ingame_day() as u8).wrapping_sub(s.bill_day);
        }
        // = seg000:2381 ds:1d = the haggling attitude.
        self.current_smuggler_willingness_to_haggle_ds_1d = s.willingness_to_haggle;
    }

    // = seg000:2318 smuggler_stage_encounter — entering a smuggler den
    // (init_room_persons, seg000:3166): find the smugglers[] record whose
    // region byte is the den location's first_name, stage it for CONDIT,
    // set ds:1e to the days since the last visit (1 on the first visit,
    // which also arms state bit 3), clear the offer price ds:9d, pick the
    // offered equipment slot from rand_bits wrapped modulo the worm-event
    // base byte (seg000:2347 reads seg001:1141, as event 0x08 does), and
    // reset the ACCEPT/REFUSE/ARGUE choice. DOS walks the table unbounded;
    // a den whose region no smuggler serves stages nothing here.
    pub(crate) fn smuggler_stage_encounter(&mut self, loc_index: usize) {
        // = seg000:2318 al = [di] — the location's region (first_name).
        let region = self.locations[loc_index].first_name;
        // = seg000:231a..2322 walk from smugglers[0] for a matching region.
        let Some(index) = self.smugglers.iter().position(|s| s.region == region) else {
            return;
        };
        // = seg000:2324 call stage_smuggler_for_condit.
        self.stage_smuggler_for_condit(index);
        // = seg000:2327..2339 al = today - [si+3]; a first visit (state bit
        //   3 clear) reads as 1 and arms the bit.
        let day = self.get_ingame_day() as u8;
        let s = &mut self.smugglers[index];
        let mut days = day.wrapping_sub(s.field_3);
        if s.field_2 & 8 == 0 {
            days = 1;
            s.field_2 |= 8;
        }
        self.current_smuggler_number_of_days_since_previous_encounter_ds_1e = days;
        // = seg000:233c ds:9d = 0.
        self.for_condit_smuggler_dialogue_related_ds_9d = 0;
        // = seg000:2341..2351 ax = rand_bits & 7, wrapped modulo the byte at
        //   seg001:1141 (worm_event_likelihood_by_region[0]).
        let modulus = self.worm_event_likelihood_by_region[0];
        let mut al = (self.rand_bits & 7) as u8;
        if modulus != 0 {
            while al >= modulus {
                al -= modulus;
            }
        }
        // = seg000:2353/2356 subst_id_03 = slot + 0xe8.
        self.string_subst_id_table[3] = al as u16 + 0xe8;
        // = seg000:2359 ds:9f = 0.
        self.accept_refuse_argue_choice_ds_9f = 0;
    }

    // = seg000:23d5 smuggler_haggle_price_down — a successful ARGUE knocks
    // one eighth off the offer price: ds:9d -= ds:9d >> 3.
    pub(crate) fn smuggler_haggle_price_down(&mut self) {
        let price = self.for_condit_smuggler_dialogue_related_ds_9d;
        self.for_condit_smuggler_dialogue_related_ds_9d = price.wrapping_sub(price >> 3);
    }

    // = seg000:23e6 smuggler_sell_equipment — ACCEPT on the smuggler's
    // offer: clear his REFUSED/ARGUED state bits (5/6), move the offer price
    // ds:9d onto his bill (ds:20 and the record's +0xe; a bill that was zero
    // counts a new debtor in ds:22), stamp today as the bill day, take one
    // of the offered equipment (subst_id_03 - 0xe8) from his stock and add
    // it to the current location's equipment row.
    pub(crate) fn smuggler_sell_equipment(&mut self, index: usize) {
        // = seg000:23e6 and byte [di+2], 9fh.
        self.smugglers[index].field_2 &= 0x9f;
        // = seg000:23ea..23fc ax = xchg(ds:9d, 0); ds:20 += ax; bill += ax;
        //   a bill equal to ax was zero before -> ds:22 += 1.
        let price = std::mem::take(&mut self.for_condit_smuggler_dialogue_related_ds_9d) as u16;
        self.current_smuggler_bill_value_ds_20 =
            self.current_smuggler_bill_value_ds_20.wrapping_add(price);
        let s = &mut self.smugglers[index];
        s.bill_value = s.bill_value.wrapping_add(price);
        if s.bill_value == price {
            self.smuggler_bills_count_ds_22 = self.smuggler_bills_count_ds_22.wrapping_add(1);
        }
        // = seg000:2400..2403 [di+10h] = today.
        self.smugglers[index].bill_day = self.get_ingame_day() as u8;
        // = seg000:2406..2418 the offered slot: stock -1, the current
        //   location's equipment row +1.
        let slot = (self.string_subst_id_table[3].wrapping_sub(0xe8) & 0xff) as usize;
        if let Some(stock) = self.smugglers[index].stock.get_mut(slot) {
            *stock = stock.wrapping_sub(1);
        }
        let li = self.current_location_index as usize;
        if slot < 7 && li < self.locations.len() {
            let e = self.locations[li].equipment.slot_mut(slot);
            *e = e.wrapping_add(1);
        }
    }

    // = seg000:2388 callback_event_dialogue_line_08_Smugglers — the
    // smuggler's dialogue event 0x08: advance the game phase to 0x3c, roll
    // the haggling rounds, stamp today into the staged record's +3 byte,
    // and pick the next stocked equipment slot after the one subst_id_03
    // names as the offer (subst_id_03 = slot + 0xe8, ds:9d = its price bits
    // 0..6 doubled).
    pub(crate) fn dialogue_event_08_smugglers(&mut self) {
        // = seg000:2388 al = 0x3c; call set_game_phase_and_trigger_callbacks.
        self.set_game_phase_and_trigger_callbacks(0x3c);
        // = seg000:238d..2393 ds:9e = rand_masked(3).
        self.for_condit_smuggler_arguing_count_ds_9e = self.rand_masked(3) as u8;
        // = seg000:2396..239d [record + 3] = today (di = room_persons[13].
        //   field_c, the record stage_smuggler_for_condit set).
        let day = self.get_ingame_day() as u8;
        let Some(index) = smuggler_index_from_ptr(self.room_persons[13].field_c) else {
            return;
        };
        self.smugglers[index].field_3 = day;
        // = seg000:23a0 ds:1a = 0.
        self.related_to_arguing_ds_1a = 0;
        // = seg000:23a5..23bb ax = subst_id_03 - 0xe8; up to two wraps
        //   (cx = 2): step to the next slot, reducing al modulo the byte at
        //   seg001:1141 — array_likelihood_of_worm_related_spice_mining_
        //   troop_events_by_region[0] (the bytes 3A 06 41 11 really address
        //   the worm-event table, so the wrap modulus is 3 at the start of
        //   the game and grows with the phases that raise it); once both
        //   wraps are spent the search gives up.
        let modulus = self.worm_event_likelihood_by_region[0];
        let mut ax = self.string_subst_id_table[3].wrapping_sub(0xe8);
        let mut cx = 2u16;
        loop {
            // = seg000:23ae inc ax.
            ax = ax.wrapping_add(1);
            // = seg000:23af..23bb the modulo loop.
            loop {
                let al = ax as u8;
                if al < modulus {
                    break;
                }
                ax = (ax & 0xff00) | al.wrapping_sub(modulus) as u16;
                cx = cx.wrapping_sub(1);
                if cx == 0 {
                    return;
                }
            }
            // = seg000:23bc..23c2 bx = ax; a stocked slot ends the search.
            let bx = ax as usize;
            let stocked = self.smugglers[index].stock.get(bx).is_some_and(|&n| n != 0);
            if stocked {
                break;
            }
        }
        // = seg000:23c4..23d1 subst_id_03 = slot + 0xe8; ds:9d = (price &
        //   0x7f) << 1.
        self.string_subst_id_table[3] = ax.wrapping_add(0xe8);
        let price = self.smugglers[index].prices[ax as usize];
        self.for_condit_smuggler_dialogue_related_ds_9d = (price & 0x7f) << 1;
    }
}

/// = seg001:10d8 smugglers — the static initializer, extracted verbatim from
/// DNCDPRG.EXE (six records; the byte after the table is the 0xff region
/// terminator the walk at seg000:1cd4 stops on).
pub(crate) const SMUGGLERS: [Smuggler; 6] = [
    sm(0x01, 0x00, [1, 2, 0, 2, 2], [0x9e, 0xcb, 0x0a, 0xa8, 0xfd]),
    sm(0x03, 0x01, [1, 2, 0, 2, 1], [0x9e, 0xcb, 0x0a, 0xa8, 0xe4]),
    sm(0x05, 0x03, [1, 1, 0, 0, 1], [0xb2, 0xe3, 0x0a, 0x28, 0xe4]),
    sm(0x06, 0x02, [0, 2, 3, 2, 2], [0x28, 0xd0, 0x8f, 0xb2, 0xfd]),
    sm(0x09, 0x03, [2, 1, 0, 0, 1], [0xb2, 0xd0, 0x0a, 0x28, 0xee]),
    sm(0x0b, 0x06, [1, 1, 2, 1, 0], [0xbc, 0xda, 0x8a, 0xa8, 0x64]),
];

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::dat_file::DatFile;

    fn asset_game() -> Option<GameState> {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return None;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        Some(game)
    }

    // Duncan's event 0x08 (seg000:2239): the smuggler with the oldest open
    // bill is staged (state bits 0x60 exclude a record), ds:9f = 3 with
    // spice in stock, and subst_id_06 names his region.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn duncan_event_08_stages_the_oldest_bill() {
        let Some(mut game) = asset_game() else { return };
        game.game_time = 20 << 4; // day 20
        game.spice_in_stock = 7;
        game.spice_shipment_quantity = 10;
        game.smugglers[1].bill_value = 30;
        game.smugglers[1].bill_day = 15; // 5 days old
        game.smugglers[4].bill_value = 12;
        game.smugglers[4].bill_day = 11; // 9 days old — the oldest
        game.smugglers[2].bill_value = 99;
        game.smugglers[2].bill_day = 1; // older still, but excluded (0x20)
        game.smugglers[2].field_2 |= 0x20;
        game.dialogue_event_08_duncan_idaho();
        assert_eq!(game.accept_refuse_argue_choice_ds_9f, 3, "= seg000:224b");
        assert_eq!(
            game.room_persons[13].field_c,
            smuggler_ptr(4),
            "= seg000:235f"
        );
        assert_eq!(game.current_smuggler_bill_value_ds_20, 12);
        assert_eq!(
            game.related_to_paying_smuggler_bills_ds_1f, 9,
            "= seg000:237b"
        );
        assert_eq!(game.current_smuggler_willingness_to_haggle_ds_1d, 3);
        assert_eq!(game.string_subst_id_table[6], 9, "= seg000:22ad region");
        // 7 < 10: the below-demand bracket (seg000:22f1), flags bits 1/2 clear.
        assert_eq!(game.spice_shipment_arguing_ds_b4, [7, 3 + 1, 3, 1]);
        assert_eq!(game.spice_shipment_flags & 6, 0);
    }

    // With no open bill the scan rotates current_smuggler_ptr to the next
    // record that has one (seg000:2282..229f), wrapping at the table end;
    // with none at all nothing is staged (seg000:2288 -> ret).
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn duncan_event_08_rotates_to_the_next_billed_smuggler() {
        let Some(mut game) = asset_game() else { return };
        game.current_smuggler_ptr = smuggler_ptr(5);
        game.dialogue_event_08_duncan_idaho();
        assert_eq!(
            game.room_persons[13].field_c, 0,
            "nothing staged without a bill"
        );
        assert_eq!(game.current_smuggler_ptr, smuggler_ptr(5));
        // Record 1 carries a bill but its state excludes it from the oldest
        // scan; the rotation from record 5 wraps to it anyway (the walk only
        // tests +0xe).
        game.smugglers[1].bill_value = 5;
        game.smugglers[1].field_2 |= 0x40;
        game.dialogue_event_08_duncan_idaho();
        assert_eq!(game.current_smuggler_ptr, smuggler_ptr(1));
        assert_eq!(game.room_persons[13].field_c, smuggler_ptr(1));
    }

    // The shipment-argument brackets (seg000:22b1): 1.5x and 2x the demand
    // select ds:bf bits 1 / 2 / both.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn argue_amounts_follow_the_demand_brackets() {
        let Some(mut game) = asset_game() else { return };
        game.spice_shipment_quantity = 100;
        game.spice_shipment_flags = 0xff;
        game.stage_spice_argue_amounts_with_duncan(120);
        assert_eq!(game.spice_shipment_arguing_ds_b4, [100, 120, 90, 60]);
        assert_eq!(game.spice_shipment_flags, 0xf9 | 2);
        game.stage_spice_argue_amounts_with_duncan(180);
        assert_eq!(game.spice_shipment_arguing_ds_b4, [100, 180, 135, 150]);
        assert_eq!(game.spice_shipment_flags, 0xf9 | 4);
        game.stage_spice_argue_amounts_with_duncan(200);
        assert_eq!(game.spice_shipment_arguing_ds_b4, [100, 200, 150, 200]);
        assert_eq!(game.spice_shipment_flags, 0xf9 | 6);
    }

    // The smuggler's event 0x08 (seg000:2388): the offer steps to the next
    // stocked slot after subst_id_03, wrapping modulo the worm-event base
    // byte (3 at game start — seg000:23af reads seg001:1141), and gives up
    // after two wraps.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn smuggler_event_08_picks_the_next_stocked_slot() {
        let Some(mut game) = asset_game() else { return };
        game.game_time = 4 << 4;
        game.game_phase = 0x3c; // already there: no phase side effects
        game.stage_smuggler_for_condit(0); // stock [1, 2, 0, 2, 2]
        game.string_subst_id_table[3] = 0xe8; // slot 0 offered last
        game.dialogue_event_08_smugglers();
        assert_eq!(game.string_subst_id_table[3], 0xe9, "slot 1 is stocked");
        assert_eq!(
            game.for_condit_smuggler_dialogue_related_ds_9d,
            (0xcb & 0x7f) << 1
        );
        assert_eq!(game.smugglers[0].field_3, 4, "= seg000:239d today");
        assert_eq!(game.related_to_arguing_ds_1a, 0);
        // From slot 1: slot 2 is empty, 3 == modulus wraps to 0 (stocked).
        game.dialogue_event_08_smugglers();
        assert_eq!(game.string_subst_id_table[3], 0xe8);
        // Nothing stocked within the modulus range: two wraps, then give up.
        game.smugglers[0].stock = [0, 0, 0, 2, 2];
        game.string_subst_id_table[3] = 0xe8;
        game.dialogue_event_08_smugglers();
        assert_eq!(game.string_subst_id_table[3], 0xe8, "= seg000:23bb ret");
    }
}
