//! The worm ride (CALL A WORM / GO THERE RIDING A WORM): VER.BIN's sprite-list
//! animation, drawn from VER.HSQ over the DFL2.HNM flight clip.
//!
//! Ported from seg000:42d1 (the verb), 4285 (the setup), 4aeb (the view
//! redraw), 4d6c (the script step) and 4da0 (the list draw). DOS loads
//! VER.BIN over its startup code at cs:015f; the port parses it into
//! `WormAnim`. Routines are in DOS address order.

use crate::{GameState, Rect, gfx, rect::rect, sprite_bank};

/// = the VER.BIN blob at cs:015f: the view rect, the sprite lists and the
/// frame script, with the script cursor (worm_script_cursor).
pub(crate) struct WormAnim {
    // = seg000:015f worm_anim_rect / seg001:aa66 worm_view_rect.
    rect: Rect,
    // = cs:0169 the sprite-list table (2-based indices): (sprite, dx, dy)
    //   byte triples per list.
    lists: Vec<Vec<(u16, i16, i16)>>,
    // = the script bytes from the script start to the 0xff end marker.
    script: Vec<u8>,
    // = seg001:aa6e worm_script_cursor — an index into `script`.
    cursor: usize,
}

impl WormAnim {
    // = the VER.BIN layout worm_ride_setup relies on: 4 words of rect, the
    // script pointer word (relative to blob + 8; the script starts at that
    // word plus its value), then at blob + 10 the table of list offsets
    // (relative to blob + 10, the first offset bounding the table).
    fn parse(data: &[u8]) -> Option<WormAnim> {
        let w = |o: usize| -> Option<u16> {
            Some(u16::from_le_bytes([*data.get(o)?, *data.get(o + 1)?]))
        };
        let r = rect(w(0)? as i16, w(2)? as i16, w(4)? as i16, w(6)? as i16);
        let script_ptr = 8 + w(8)? as usize;
        let script_start = script_ptr + w(script_ptr)? as usize;
        let mut script = Vec::new();
        let mut i = script_start;
        loop {
            let b = *data.get(i)?;
            script.push(b);
            if b == 0xff {
                break;
            }
            i += 1;
        }
        let table = 10;
        let first = w(table)? as usize;
        let mut lists = Vec::new();
        for k in 0..first / 2 {
            let mut o = table + w(table + 2 * k)? as usize;
            let mut list = Vec::new();
            loop {
                let sprite = *data.get(o)?;
                if sprite == 0 {
                    break;
                }
                let dx = *data.get(o + 1)? as i16;
                let dy = *data.get(o + 2)? as i16;
                list.push((sprite as u16 - 1, dx, dy));
                o += 3;
            }
            lists.push(list);
        }
        Some(WormAnim {
            rect: r,
            lists,
            script,
            cursor: 0,
        })
    }
}

impl GameState {
    // = seg000:42d1 menu_callback_choice_call_a_worm — the CALL A WORM room
    // verb: take down a lingering talking head, set the worm ride up and
    // open the map screen with the Cancel menu.
    pub(crate) fn menu_callback_choice_call_a_worm(&mut self, _text_id: u16, _index: usize) {
        // = seg000:42d1 call tear_down_prior_talking_head_overlay.
        self.tear_down_prior_talking_head_overlay();
        // = seg000:42d4 call worm_ride_setup.
        self.worm_ride_setup();
        // = seg000:42d7 jmp map_screen_open_with_cancel_menu.
        self.map_screen_open_with_cancel_menu();
    }

    // = seg000:4285 worm_ride_setup — load VER.BIN once, then set the worm
    // travel mode: vehicle 1 (DFL2.HNM), no cockpit, screen mode 8, the
    // script at its start, VER.HSQ open with its palette flushed.
    pub(crate) fn worm_ride_setup(&mut self) {
        // = seg000:4285..42a5 cmp cs:[worm_anim_rect + 2],0; jnz — load
        //   resource 0beh (VER.BIN) over cs:015f once, copy the rect to
        //   worm_view_rect and rebase the script pointer.
        if self.worm_anim.is_none() {
            let data = self
                .dat_file
                .read("VER.BIN")
                .expect("failed to read VER.BIN");
            self.worm_anim = WormAnim::parse(&data);
        }
        // = seg000:42aa..42b5.
        self.travel_vehicle_mode = 1;
        self.map_ornithopter_mode = 0;
        self.game_screen_mode_flags = 8;
        // = seg000:42ba..42c2 worm_script_cursor = the script start.
        if let Some(anim) = self.worm_anim.as_mut() {
            anim.cursor = 0;
        }
        // = seg000:42c6..42cc ax = 39h (VER.HSQ); open_resource_by_index;
        //   vga_palette_flush.
        self.open_sprite_bank(sprite_bank::VER);
        gfx::palette_flush(self);
    }

    // = seg000:4aeb worm_view_redraw — redraw the worm view over the flight
    // frame in fb1: VER.HSQ open, the next animation frame, the minimap rect
    // restored, the game area presented.
    pub(crate) fn worm_view_redraw(&mut self) {
        // = seg000:4aeb/4aee ax = 39h; call open_resource_by_index.
        self.open_sprite_bank(sprite_bank::VER);
        // = seg000:4af1 call set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = seg000:4af4 call worm_anim_step.
        self.worm_anim_step();
        // = seg000:4af7 call travel_restore_minimap_rect.
        self.travel_restore_minimap_rect();
        // = seg000:4afa jmp present_game_area.
        self.present_game_area();
    }

    // = seg000:4d6c worm_anim_step — draw one frame of the VER.BIN script:
    // 0xff restarts at the script start; each byte is a sprite-list index
    // (1 = escape, the next byte + 0x100) until a 0 ends the frame.
    fn worm_anim_step(&mut self) {
        let Some(anim) = self.worm_anim.as_ref() else {
            return;
        };
        let script = anim.script.clone();
        let mut cursor = anim.cursor;
        // = seg000:4d70..4d7e cmp cs:[si],0ffh; jnz; si = the script start.
        if script.get(cursor).copied().unwrap_or(0xff) == 0xff {
            cursor = 0;
        }
        // = seg000:4d84..4d97 the frame's lists.
        while let Some(&b) = script.get(cursor) {
            cursor += 1;
            if b == 0 {
                break;
            }
            let mut list = b as usize;
            if b == 1 {
                let Some(&lo) = script.get(cursor) else {
                    break;
                };
                cursor += 1;
                list = 0x100 | lo as usize;
            }
            self.worm_anim_draw_list(list);
        }
        // = seg000:4d9b mov [worm_script_cursor],si.
        if let Some(anim) = self.worm_anim.as_mut() {
            anim.cursor = cursor;
        }
    }

    // = seg000:4da0 worm_anim_draw_list — draw sprite list `list` (2-based)
    // of VER.BIN: each (sprite, dx, dy) blitted clipped at the view rect's
    // origin + (dx, dy).
    fn worm_anim_draw_list(&mut self, list: usize) {
        let Some(anim) = self.worm_anim.as_ref() else {
            return;
        };
        // = seg000:4da0..4daa si = the table entry (ax - 2).
        let Some(entries) = list.checked_sub(2).and_then(|i| anim.lists.get(i)) else {
            return;
        };
        let entries = entries.clone();
        let r = anim.rect;
        let yoff = self.y_offset as i16;
        let clip = rect(r.x0, r.y0 + yoff, r.x1, r.y1 + yoff);
        // = seg000:4dad..4dea per triple: dx/bx = worm_view_rect.x0/y0 +
        //   the offsets; the VER.HSQ sprite (bp - 1) through
        //   vga_blit_clipped with bp = worm_view_rect.
        for (sprite, dx, dy) in entries {
            self.with_active_bank_sheet(|s, sheet| {
                s.draw_sprite_from_sheet_clipped(sheet, sprite, r.x0 + dx, r.y0 + dy + yoff, clip);
            });
        }
    }
}
