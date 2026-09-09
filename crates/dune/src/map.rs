//! Planet-map (MAP.HSQ) position lookups.
//!
//! The desert model addresses the planet by a 16-bit longitude `x` (one full
//! circumference = 0x10000, the DOS `dx`) and a signed latitude row `lat`
//! (-98..98, the DOS `bl`; 0 is the equator). MAP.HSQ is one terrain byte per
//! map cell, stored row by row with the equator row at offset 0x62fc and the
//! rows shrinking toward the poles; TABLAT.BIN gives each row's distance from
//! the equator row (`offset`) and its cell count (`len`, the row is `2 * len`
//! bytes). A longitude maps into a row as `cell = round(x * 2*len / 0x10000)`.
//!
//! Map byte bit 0x40 marks "a location is at this cell"; the startup loop
//! (init_location_map_offsets) plants it and caches each location's byte
//! offset so the desert-walk arrival check (loc_04002) can match the cell
//! back to its `Location`.

use crate::GameState;

impl GameState {
    // = seg000:b58b map_func (+ tablat_lookup_from_bx_to_ax_bp, seg000:b5a0) —
    // the MAP.HSQ byte offset for (x = longitude, lat = latitude row). The
    // tablat entry for |lat| gives the row's start (offset from the map
    // centre, negated for southern rows) and its byte length bp = 2 * len;
    // the cell within the row is round(x * bp / 0x10000) (the DOS
    // `mul dx; shl ax,1; adc dx,0` rounding). Besides the offset (es:di),
    // the DOS routine leaves the cell index (dx) and row byte length (bp)
    // live for map_offset_and_snap_x — returned here as the second and
    // third tuple element.
    pub(crate) fn map_position_to_offset(&self, x: u16, lat: i16) -> (usize, u16, u16) {
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        // Tablat encodes rows as y = lat + 98 (0..196); its offset() applies
        // the row's distance below/above the map centre 0x62fc (= the DOS
        // res_map_ofs base, seg000:010e). (For lat == 0 it subtracts where
        // DOS adds, but the equator entry's offset is 0.)
        let y = (lat + 98) as u16;
        let row = tablat.offset(y) as usize;
        let row_len = tablat.len(y) as u32;
        let cell = (row_len * x as u32 + 0x8000) >> 16;
        (row + cell as usize, cell as u16, row_len as u16)
    }

    // = seg000:b5c5 map_offset_and_snap_x — map_position_to_offset plus the
    // `xor ax,ax; div bp` snap: the longitude is quantised back from the cell
    // (x = cell * 0x10000 / row_len), so a snapped location map_x compares
    // equal (loc_04002) when a desert walk lands on its cell.
    pub(crate) fn map_offset_and_snap_x(&self, x: u16, lat: i16) -> (usize, u16) {
        let (offset, cell, row_len) = self.map_position_to_offset(x, lat);
        let snapped = (((cell as u32) << 16) / row_len as u32) as u16;
        (offset, snapped)
    }

    // = seg000:b532 read_map_byte_at_dx_bl — the terrain byte at
    // (x = longitude, lat = latitude row).
    pub(crate) fn read_map_byte(&self, x: u16, lat: i16) -> u8 {
        self.map[self.map_position_to_offset(x, lat).0]
    }

    // = seg000:407e get_map_position — the player's map position: in a room
    // (location_appearance low byte 0x80) the current location record's
    // (map_x, map_y); in the desert, location_and_room IS the longitude and
    // the appearance low byte the (sign-extended) latitude row.
    pub(crate) fn get_map_position(&self) -> (u16, i16) {
        if self.location_appearance & 0xff == 0x80 {
            // = seg000:408b..4092 si = [current_location_ptr] — always set
            // while in a room (every scene open recomputes it).
            let location = &self.locations[self.current_location_index as usize];
            (location.map_x as u16, location.map_y)
        } else {
            // = seg000:4096..4098 xchg bx,ax; cbw; xchg bx,ax.
            (
                self.location_and_room,
                (self.location_appearance as u8) as i8 as i16,
            )
        }
    }

    // = seg000:5b5d set_zoomed_globe_pos_from_map_position — seed the
    // map/globe view centre from the player's current map position; falls
    // into set_zoomed_globe_pos (seg000:5b60, the two stores).
    pub(crate) fn set_zoomed_globe_pos_from_map_position(&mut self) {
        let (x, lat) = self.get_map_position();
        self.zoomed_globe_longitude = x;
        self.zoomed_globe_latitude = lat;
    }

    // = seg000:409a find_location_by_map_offset — scan locations[] for the
    // entry whose cached map-byte offset matches. None when no location
    // claims the cell (DOS returns the table's end sentinel with ZF clear).
    pub(crate) fn find_location_by_map_offset(&self, offset: usize) -> Option<usize> {
        self.locations
            .iter()
            .position(|l| l.map_offset as usize == offset)
    }

    // = seg000:0169..01c6 map2_resource_func (minus the troop placement pass,
    // init_troop_locations): build a 256-entry histogram of the MAP2 spice
    // layer's bytes, each count seeded with 7 (seg000:0175..018d), then for
    // every location: snap its map_x to its map cell, cache the map byte
    // offset (Location.map_offset, seg000:019e), mark the cell as holding a
    // location (map byte |= 0x40, seg000:01a1), read the MAP2 byte at that
    // offset into spice_field_id (seg000:01a5..01ac) and set spice_amount =
    // histogram[field] >> 4 (seg000:01af..01bd).
    pub(crate) fn init_location_map_offsets(&mut self) {
        // = seg000:0175..018d the seeded MAP2 histogram (data_0c5f9 bytes,
        //   the map length).
        let mut histogram = [7u16; 256];
        for &cell in self.map2.iter().take(self.map.len()) {
            histogram[cell as usize] += 1;
        }
        for i in 0..self.locations.len() {
            let (map_x, map_y) = (self.locations[i].map_x, self.locations[i].map_y);
            let (offset, snapped_x) = self.map_offset_and_snap_x(map_x as u16, map_y);
            self.locations[i].map_x = snapped_x as i16;
            self.locations[i].map_offset = offset as u16;
            self.map[offset] |= 0x40;
            // = seg000:01a5..01bd the location's spice field: the MAP2 byte
            //   at its cell, and the field's map coverage as the amount.
            let field = self.map2[offset];
            self.locations[i].spice_field_id = field;
            self.locations[i].spice_amount = (histogram[field as usize] >> 4) as u8;
        }
    }

    // = seg000:644e location_stamp_atreides_zone_on_map — bits 5-4 = 0x20 on
    // the disc of radius discoverable_at_phase around the location.
    pub(crate) fn location_stamp_atreides_zone_on_map(&mut self, li: usize) {
        let radius = self.locations[li].discoverable_at_phase as u8 as u16;
        self.map_stamp_zone_disc(li, radius, 0x20);
    }

    // = seg000:6447 location_stamp_harkonnen_zone_on_map — bits 5-4 = 0x30 on
    // the disc of radius `radius` around the location.
    pub(crate) fn location_stamp_harkonnen_zone_on_map(&mut self, li: usize, radius: u16) {
        self.map_stamp_zone_disc(li, radius, 0x30);
    }

    // = seg000:6458 map_stamp_zone_disc.
    fn map_stamp_zone_disc(&mut self, li: usize, radius: u16, fill_bits: u8) {
        let centre_x = self.locations[li].map_x as u16;
        let centre_y = self.locations[li].map_y;
        map_disc_rasterize(radius, |row, len, x_start| {
            self.map_stamp_zone_span(centre_x, centre_y, fill_bits, row, len, x_start);
        });
    }

    // = seg000:646f map_stamp_zone_span — one disc span on row centre_y + row,
    // cells x_start.. x_start + len - 1 relative to the centre cell, wrapping
    // around the row. Vegetation cells (bits 5-4 == 0x10) keep their bits.
    fn map_stamp_zone_span(
        &mut self,
        centre_x: u16,
        centre_y: i16,
        fill_bits: u8,
        row: i16,
        len: u16,
        x_start: i16,
    ) {
        let row = row.wrapping_add(centre_y);
        if !(-93..=93).contains(&row) {
            return;
        }
        let (offset, cell, row_len) = self.map_position_to_offset(centre_x, row);
        let (mut offset, mut cell, row_len) = (offset as i32, cell as i32, row_len as i32);
        offset += x_start as i32;
        cell += x_start as i32;
        while cell < 0 {
            offset += row_len;
            cell += row_len;
        }
        for _ in 0..len {
            let al = self.map[offset as usize];
            if al & 0x30 != 0x10 {
                self.map[offset as usize] = al & !0x30 | fill_bits;
            }
            offset += 1;
            cell += 1;
            if cell >= row_len {
                cell -= row_len;
                offset -= row_len;
            }
        }
    }
    // = seg000:6515 location_spread_vegetation_on_map — spread the location's
    // vegetation program: the disc of radius discoverable_at_phase around
    // (vegetation_x, vegetation_y).
    pub(crate) fn location_spread_vegetation_on_map(&mut self, li: usize) {
        self.location_mark_map_view_dirty(li);
        let mut seed_pattern = 0x44u8;
        let radius = self.locations[li].discoverable_at_phase as u8 as u16;
        let centre_x = self.locations[li].vegetation_x as u16;
        let centre_y = self.locations[li].vegetation_y as i16;
        map_disc_rasterize(radius, |row, len, x_start| {
            self.map_vegetation_span(centre_x, centre_y, &mut seed_pattern, row, len, x_start);
        });
    }

    // = seg000:653a map_vegetation_span — one vegetation span (walk as in
    // map_stamp_zone_span, rows within -86..86). Vegetation cells keep their
    // bits; a location cell loses its spice density, and a non-Atreides one
    // past the first two falls to the Atreides side; the rest turn green
    // (0x20), or seed new vegetation (0x10) on every carry of the rotating
    // seed pattern over low terrain (terrain & 0x0e < 8).
    fn map_vegetation_span(
        &mut self,
        centre_x: u16,
        centre_y: i16,
        seed_pattern: &mut u8,
        row: i16,
        len: u16,
        x_start: i16,
    ) {
        let row = row.wrapping_add(centre_y);
        if !(-86..=86).contains(&row) {
            return;
        }
        let (offset, cell, row_len) = self.map_position_to_offset(centre_x, row);
        let (mut offset, mut cell, row_len) = (offset as i32, cell as i32, row_len as i32);
        offset += x_start as i32;
        cell += x_start as i32;
        while cell < 0 {
            offset += row_len;
            cell += row_len;
        }
        for _ in 0..len {
            let byte = self.map[offset as usize];
            let mut al = byte;
            if byte & 0x30 != 0x10 {
                if byte & 0x40 != 0
                    && let Some(found) = self.find_location_by_map_offset(offset as usize)
                {
                    self.locations[found].spice_density = 0;
                    if !self.location_is_atreides(found) && found >= 2 {
                        self.locations[found].status &= 0x7f;
                        // = seg000:658c call location_battle_won_for_fortress_07443.
                        self.location_battle_won_for_fortress(found);
                    }
                }
                al = al & !0x30 | 0x20;
                if byte & 0x0e < 8 {
                    let carry = *seed_pattern & 0x80 != 0;
                    *seed_pattern = seed_pattern.rotate_left(1);
                    if carry {
                        al = al & !0x30 | 0x10;
                    }
                }
            }
            self.map[offset as usize] = al;
            offset += 1;
            cell += 1;
            if cell >= row_len {
                cell -= row_len;
                offset -= row_len;
            }
        }
    }
}

// = seg000:64b2 map_disc_rasterize — emit the spans of a disc of `radius`
// around the origin as (row, len, x_start), each span on two rows (bx and its
// mirror si, seg000:64ef map_disc_emit_span_pair). A register-for-register
// transcription: the DOS walk emits some rows twice.
pub(crate) fn map_disc_rasterize(radius: u16, mut emit: impl FnMut(i16, u16, i16)) {
    let mut cx = radius as i16;
    let mut bx = 0i16.wrapping_sub(cx);
    let mut si = cx.wrapping_sub(1);
    let mut ax = cx;
    let mut bp = 0i16;
    let mut dx = 0i16;
    let mut di = 0i16;
    let mut emit_pair = |bx: i16, si: i16, di: i16, dx: i16| {
        let len = dx.wrapping_sub(di).wrapping_add(1) as u16;
        emit(bx, len, di);
        emit(si, len, di);
    };
    // = seg000:64c1..64d3
    loop {
        ax = ax.wrapping_sub(bp);
        if ax >= 0 {
            bp = bp.wrapping_add(1);
            dx = dx.wrapping_add(1);
            di = di.wrapping_sub(1);
            continue;
        }
        emit_pair(bx, si, di, dx);
        bx = bx.wrapping_add(1);
        si = si.wrapping_sub(1);
        ax = ax.wrapping_add(cx);
        if ax < 0 {
            break;
        }
        cx = cx.wrapping_sub(1);
        if cx == 0 {
            break;
        }
        bp = bp.wrapping_add(1);
        dx = dx.wrapping_add(1);
        di = di.wrapping_sub(1);
    }
    // = seg000:64d5..64ec
    bp = bp.wrapping_add(1);
    cx = cx.wrapping_sub(1);
    loop {
        loop {
            emit_pair(bx, si, di, dx);
            bx = bx.wrapping_add(1);
            si = si.wrapping_sub(1);
            let (sum, carry) = (ax as u16).overflowing_add(cx as u16);
            ax = sum as i16;
            if carry {
                break;
            }
            cx = cx.wrapping_sub(1);
            if cx == 0 {
                break;
            }
        }
        cx = cx.wrapping_sub(1);
        if cx < 0 {
            return;
        }
        dx = dx.wrapping_add(1);
        di = di.wrapping_sub(1);
        ax = ax.wrapping_sub(bp);
        bp = bp.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::map_disc_rasterize;
    use crate::{GameState, dat_file::DatFile};

    fn spans(radius: u16) -> Vec<(i16, u16, i16)> {
        let mut v = Vec::new();
        map_disc_rasterize(radius, |row, len, x| v.push((row, len, x)));
        v
    }

    // Hand-traced from seg000:64b2..64ec.
    #[test]
    fn disc_spans_match_the_dos_walk() {
        assert_eq!(
            spans(1),
            vec![
                (-1, 5, -2),
                (0, 5, -2),
                (0, 5, -2),
                (-1, 5, -2),
                (1, 5, -2),
                (-2, 5, -2),
            ]
        );
        assert_eq!(
            spans(2),
            vec![
                (-2, 5, -2),
                (1, 5, -2),
                (-1, 7, -3),
                (0, 7, -3),
                (0, 7, -3),
                (-1, 7, -3),
                (1, 7, -3),
                (-2, 7, -3),
            ]
        );
        assert_eq!(
            spans(3),
            vec![
                (-3, 7, -3),
                (2, 7, -3),
                (-2, 9, -4),
                (1, 9, -4),
                (-1, 9, -4),
                (0, 9, -4),
            ]
        );
    }

    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn atreides_zone_covers_the_disc_and_spares_vegetation() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        let li = 0;
        let (x, lat) = (game.locations[li].map_x as u16, game.locations[li].map_y);
        let (centre, cell, row_len) = game.map_position_to_offset(x, lat);
        assert!(
            cell >= 4 && cell + 4 < row_len,
            "centre cell away from the row wrap"
        );
        let row_above = game.map_position_to_offset(x, lat - 2).0;
        let row_below = game.map_position_to_offset(x, lat + 2).0;
        for o in [
            centre,
            centre - 2,
            centre + 3,
            centre + 4,
            row_above + 4,
            row_above + 3,
            row_below,
        ] {
            game.map[o] &= !0x30;
        }
        game.map[centre - 2] |= 0x10;
        game.locations[li].discoverable_at_phase = 2;

        game.location_stamp_atreides_zone_on_map(li);

        // Radius 2: rows -2..1 span -3..3 (the second pass widens the outer rows).
        assert_eq!(game.map[centre] & 0x30, 0x20);
        assert_eq!(game.map[centre - 2] & 0x30, 0x10, "vegetation kept");
        assert_eq!(game.map[centre + 3] & 0x30, 0x20);
        assert_eq!(game.map[centre + 4] & 0x30, 0x00, "outside the disc");
        assert_eq!(game.map[row_above + 3] & 0x30, 0x20);
        assert_eq!(game.map[row_above + 4] & 0x30, 0x00);
        assert_eq!(game.map[row_below] & 0x30, 0x00, "row +2 is outside");
    }

    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn vegetation_spread_greens_the_disc_and_seeds_low_terrain() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        let li = 0;
        let loc = game.locations[li];
        game.locations[li].vegetation_x = loc.map_x;
        game.locations[li].vegetation_y = loc.map_y as i8;
        game.locations[li].discoverable_at_phase = 2;
        game.locations[li].spice_density = 7;
        let (x, lat) = (loc.map_x as u16, loc.map_y);
        // Radius 2 covers rows -2..1, cells -3..3; clear them, keep one
        // vegetation cell, and clear a probe cell just outside.
        let mut disc = Vec::new();
        for dy in -2..=1 {
            let o = game.map_position_to_offset(x, lat + dy).0;
            for dx in -3..=3 {
                disc.push((o as i32 + dx) as usize);
            }
        }
        for &o in &disc {
            game.map[o] &= !0x30;
        }
        let centre = game.map_position_to_offset(x, lat).0;
        game.map[centre - 1] |= 0x10;
        let outside = centre + 4;
        game.map[outside] &= !0x30;

        game.location_spread_vegetation_on_map(li);

        assert_eq!(
            game.locations[li].spice_density, 0,
            "own cell inside the disc"
        );
        assert_eq!(game.map[centre - 1] & 0x30, 0x10, "vegetation kept");
        assert_eq!(game.map[outside] & 0x30, 0x00);
        let mut seeds = 0;
        let mut low_terrain = 0;
        for &o in &disc {
            let b = game.map[o];
            assert!(
                b & 0x30 == 0x20 || b & 0x30 == 0x10,
                "cell {o:#x} = {b:#04x}"
            );
            if o != centre - 1 && b & 0x0e < 8 {
                low_terrain += 1;
            }
            if o != centre - 1 && b & 0x30 == 0x10 {
                seeds += 1;
            }
        }
        assert!(low_terrain >= 2, "test needs low terrain around Arrakeen");
        assert!(
            seeds >= 1 && seeds < low_terrain,
            "seeds {seeds} of {low_terrain}"
        );
    }
}
