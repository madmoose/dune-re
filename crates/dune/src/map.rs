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

    // = seg000:018f..01c6, the location loop of map2_resource_func: for every
    // location, snap its map_x to its map cell, cache the map byte offset
    // (Location.map_offset, seg000:019e), and mark the cell as holding a
    // location (map byte |= 0x40, seg000:01a1). The rest of the DOS loop —
    // spice_field_id/spice_amount from the MAP2 spice layer (seg000:01a5..
    // 01bd) and the per-location troop callback pass (seg000:01c8..01dd) —
    // is not ported yet (the port's static location table already carries
    // spice values).
    pub(crate) fn init_location_map_offsets(&mut self) {
        for i in 0..self.locations.len() {
            let (map_x, map_y) = (self.locations[i].map_x, self.locations[i].map_y);
            let (offset, snapped_x) = self.map_offset_and_snap_x(map_x as u16, map_y);
            self.locations[i].map_x = snapped_x as i16;
            self.locations[i].map_offset = offset as u16;
            self.map[offset] |= 0x40;
        }
    }
}
