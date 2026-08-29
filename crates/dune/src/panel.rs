//! The popup panel record and the identity DOS gives it.

use crate::Rect;

/// A reference to one of the full-map view's panel records — what DOS keeps as
/// the record's seg001 offset. The two popup slots (seg001:dbe0 map_popup_ptr /
/// seg001:dbe2 map_popup2_ptr) hold one each, and the offset doubles as the
/// popup's identity, so the map mouse handlers dispatch on it (0 = the slot is
/// free). The port names the records instead of passing offsets around: every
/// value either slot can take has a variant, each record carries its own in
/// [`PanelRecord::popup`], and `GameState::map_panel_record` dereferences one.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum MapPanelRef {
    /// = 0 — the slot is free (seg000:5af0/5af3, 5938, 59b8, 5fa2, 7b94, 7d6f).
    /// Also the identity of a record no popup slot ever holds: the contact
    /// popup's head box, the GAME PAUSED window, the port-only save panel.
    #[default]
    None,
    /// = seg001:1668 location_info_panel_record, stored by the placement at
    /// seg000:5f5f.
    LocationInfo,
    /// = seg001:18df troop_info_panel_record, the same placement store.
    TroopInfo,
    /// = seg001:18e9 troop_contact_text_panel_record (seg000:7a1e).
    TroopContactText,
    /// = seg001:1940 map_equipment_location_strip (seg000:7de2) — the MODIFY EQUIPMENT
    /// location strip, the second slot only (map_place_equipment_panels).
    EquipmentLocationStrip,
    /// = seg001:194a data_0194a, the rallied-troops title popup (seg000:5bb6).
    Rallied,
    /// = seg001:4710 data_04710, the spice-density overlay panel: it takes the
    /// first slot when free, else the second (seg000:5523..5535). The overlay
    /// is the one popup whose panel is not a `PanelRecord` — its frame is
    /// sprite 0x8d, so seg001:4710 holds only a rect.
    SpiceOverlay,
}

// = the `PanelRecord` struct in the chani project: the 10-byte popup panel
// record the seg001 data holds one of per panel — the rect (+0..+7), then the
// frame (+8) and fill (+9) colours. The draw pair at seg000:7b1b fills the rect
// with `fill_color` and tail-jumps to loc_0c551, which outlines it one pixel in
// with `frame_color`. Callers that write a panel's rect per open (the info
// popups, seg000:5f4f) rewrite the first four words in place and leave the
// colours as compiled in.
//
// `popup` is not one of the ten bytes: DOS identifies a record by its own
// seg001 address, which is what the popup slots store, so the port carries that
// identity in the record and the placement reads it straight off (seg000:5f5f
// `mov [map_popup_ptr], si`). A record no slot ever names is `MapPanelRef::None`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelRecord {
    pub popup: MapPanelRef,
    pub rect: Rect,
    pub frame_color: u8,
    pub fill_color: u8,
}

pub const fn panel(popup: MapPanelRef, rect: Rect, frame_color: u8, fill_color: u8) -> PanelRecord {
    PanelRecord {
        popup,
        rect,
        frame_color,
        fill_color,
    }
}

impl PanelRecord {
    // The DOS font colour word for text drawn on this panel: cx = (bg << 8) |
    // fg over the panel's own fill colour, the way map_draw_location_popup
    // loads ch straight from the record (seg000:601f) before its label draws.
    pub const fn text_color(&self, fg: u8) -> u16 {
        ((self.fill_color as u16) << 8) | fg as u16
    }

    // The same record placed at another rect: the popup placement rewrites the
    // rect words and keeps the identity and the compiled-in colours.
    pub const fn at(&self, rect: Rect) -> PanelRecord {
        PanelRecord { rect, ..*self }
    }
}
