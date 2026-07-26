//! The troop icon renderer — the DOS engine at seg000:c58d..c7d3 that keeps a
//! list of layered sprites (the troop icons clustered around the map's
//! location markers) and repaints dirty rects of the view by restoring the
//! fb2 snapshot and drawing the intersecting icons back over it in a
//! pluggable order (insertion order, or back-to-front by depth on the full
//! map). The night attack scene reuses the same DOS routines as a particle
//! system — the port keeps that scene's private copy separate (attack/mod.rs).
//!
//! Icon scripts are 3-byte (sprite, dx, dy) steps, sprite 0 terminating, with
//! byte 0 doubling as the spawn sprite; they live in the seg001:1672..1935
//! data block embedded below. troop_icon_anim_task steps them.

use crate::{GameState, Rect, TaskId, locations::location_index_from_ptr, rect::rect};

// = seg001:1672..1935 — the troop icon data block: the 16 marker-slot
// offsets (troop_icon_slot_offsets, 1672), the anim scripts, and the three
// 16-entry script-pointer tables (troop_icon_scripts_specialized 16b6,
// troop_icon_scripts_basic 179c, troop_icon_scripts_moving 18bf). Extracted
// verbatim from DNCDPRG.EXE; all script pointers are seg001 offsets into
// this same block.
const TROOP_ICON_DATA_BASE: u16 = 0x1672;
#[rustfmt::skip]
static TROOP_ICON_DATA: [u8; 0x2c4] = [
    0x00, 0x0d, 0x16, 0x0d, 0xea, 0x0d, 0x16, 0x00, 0xea, 0x00, 0x16, 0xf2,
    0xea, 0xf2, 0x00, 0xf2, 0xfa, 0xf8, 0x00, 0xf8, 0xf4, 0xf8, 0x06, 0xf8,
    0xee, 0xf8, 0xf7, 0xf6, 0x03, 0xf6, 0xf1, 0xf6, 0x02, 0x02, 0x00, 0x00,
    0x02, 0x02, 0x01, 0x00, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x1a, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x4f, 0x00, 0x00, 0x00, 0xa2, 0x16, 0xa6, 0x16,
    0xa2, 0x16, 0xa2, 0x16, 0xaa, 0x16, 0xaa, 0x16, 0xaa, 0x16, 0xaa, 0x16,
    0xae, 0x16, 0xae, 0x16, 0xae, 0x16, 0xae, 0x16, 0xb2, 0x16, 0xb2, 0x16,
    0xb2, 0x16, 0xb2, 0x16, 0x13, 0x00, 0x00, 0x14, 0x00, 0x00, 0x15, 0x00,
    0x00, 0x16, 0x00, 0x00, 0x00, 0x17, 0x00, 0x00, 0x18, 0x00, 0x00, 0x19,
    0x00, 0x00, 0x1a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2b, 0x00,
    0x00, 0x2c, 0x00, 0x00, 0x2d, 0x00, 0x00, 0x2e, 0x00, 0x00, 0x2f, 0x00,
    0x00, 0x30, 0x00, 0x00, 0x31, 0x00, 0x00, 0x32, 0x00, 0x00, 0x33, 0x00,
    0x00, 0x34, 0x00, 0x00, 0x35, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x31,
    0x00, 0x00, 0x31, 0x00, 0x00, 0x30, 0x00, 0x00, 0x31, 0x00, 0x00, 0x31,
    0x00, 0x00, 0x31, 0x00, 0x00, 0x30, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00,
    0x1b, 0x00, 0x00, 0x1b, 0x00, 0xff, 0x1b, 0x00, 0x01, 0x00, 0x27, 0x00,
    0x00, 0x27, 0xff, 0xff, 0x27, 0x01, 0x01, 0x00, 0x1f, 0x00, 0x00, 0x1f,
    0xff, 0xff, 0x1f, 0x01, 0x01, 0x00, 0x23, 0x00, 0x00, 0x23, 0x00, 0xff,
    0x23, 0x00, 0x01, 0x00, 0x47, 0x00, 0x00, 0x48, 0x00, 0x00, 0x49, 0x00,
    0x00, 0x4a, 0x00, 0x00, 0x00, 0x4b, 0x00, 0x00, 0x4c, 0x00, 0x00, 0x4d,
    0x00, 0x00, 0x4e, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x50, 0x00, 0xff,
    0x50, 0x00, 0x01, 0x00, 0x5c, 0x00, 0x00, 0x5c, 0xff, 0xff, 0x5c, 0x01,
    0x01, 0x00, 0x54, 0x00, 0x00, 0x54, 0xff, 0xff, 0x54, 0x01, 0x01, 0x00,
    0x58, 0x00, 0x00, 0x58, 0x00, 0xff, 0x58, 0x00, 0x01, 0x00, 0xd6, 0x16,
    0xe3, 0x16, 0xf0, 0x16, 0xd6, 0x16, 0xf4, 0x16, 0x19, 0x17, 0x32, 0x17,
    0x19, 0x17, 0x5a, 0x17, 0x67, 0x17, 0x67, 0x17, 0x67, 0x17, 0xb2, 0x16,
    0x74, 0x17, 0xb2, 0x16, 0xb2, 0x16, 0x64, 0x00, 0x00, 0x65, 0x00, 0x00,
    0x66, 0x00, 0x00, 0x67, 0x00, 0x00, 0x00, 0x69, 0x00, 0x00, 0x68, 0x00,
    0x00, 0x69, 0x00, 0x00, 0x6a, 0x00, 0x00, 0x69, 0x00, 0x00, 0x68, 0x00,
    0x00, 0x69, 0x00, 0x00, 0x6a, 0x00, 0x00, 0x69, 0x00, 0x01, 0x68, 0x00,
    0x01, 0x69, 0x00, 0x00, 0x6a, 0x00, 0x00, 0x69, 0x00, 0x00, 0x68, 0xff,
    0xff, 0x69, 0x00, 0x00, 0x6a, 0x00, 0x00, 0x69, 0x00, 0x00, 0x68, 0x01,
    0x00, 0x69, 0x00, 0x00, 0x6a, 0x00, 0xff, 0x00, 0x6c, 0x00, 0x00, 0x6d,
    0x00, 0x00, 0x6e, 0x00, 0x00, 0x6f, 0x00, 0x00, 0x00, 0x63, 0x00, 0x00,
    0x00, 0x68, 0x00, 0x00, 0x00, 0x6b, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00,
    0x00, 0x03, 0x00, 0x00, 0x04, 0x00, 0x00, 0x05, 0x00, 0x00, 0x06, 0x00,
    0x00, 0x00, 0x1b, 0x00, 0x00, 0x1c, 0x00, 0x00, 0x1d, 0x00, 0x00, 0x1e,
    0x00, 0x00, 0x00, 0x37, 0x00, 0x00, 0x38, 0x00, 0x00, 0x39, 0x00, 0x00,
    0x3a, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x08, 0x00, 0x00, 0x09, 0x00,
    0x00, 0x0a, 0x00, 0x00, 0x00, 0x1f, 0x00, 0x00, 0x20, 0x00, 0x00, 0x21,
    0x00, 0x00, 0x22, 0x00, 0x00, 0x00, 0x3b, 0x00, 0x00, 0x3c, 0x00, 0x00,
    0x3d, 0x00, 0x00, 0x3e, 0x00, 0x00, 0x00, 0x0b, 0x00, 0x00, 0x0c, 0x00,
    0x00, 0x0d, 0x00, 0x00, 0x0e, 0x00, 0x00, 0x00, 0x23, 0x00, 0x00, 0x24,
    0x00, 0x00, 0x25, 0x00, 0x00, 0x26, 0x00, 0x00, 0x00, 0x3f, 0x00, 0x00,
    0x40, 0x00, 0x00, 0x41, 0x00, 0x00, 0x42, 0x00, 0x00, 0x00, 0x0f, 0x00,
    0x00, 0x10, 0x00, 0x00, 0x11, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00, 0x27,
    0x00, 0x00, 0x28, 0x00, 0x00, 0x29, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x00,
    0x43, 0x00, 0x00, 0x44, 0x00, 0x00, 0x45, 0x00, 0x00, 0x46, 0x00, 0x00,
    0x00, 0x23, 0x18, 0x4a, 0x18, 0x71, 0x18, 0x98, 0x18, 0x30, 0x18, 0x57,
    0x18, 0x7e, 0x18, 0xa5, 0x18, 0x3d, 0x18, 0x64, 0x18, 0x8b, 0x18, 0xb2,
    0x18, 0x30, 0x18, 0x57, 0x18, 0x7e, 0x18, 0xa5, 0x18, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0xfb, 0xf0, 0x05, 0x00, 0x05, 0x00, 0xe8,
    0x00, 0x48, 0x00, 0xf5, 0xfb, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0xf5, 0xe4, 0x85, 0x00, 0x00, 0x86, 0x00, 0x00, 0x87, 0x00, 0x00,
    0x88, 0x00, 0x00, 0x89, 0x00, 0x00, 0x8a, 0x00, 0x00, 0x8b, 0x00, 0x00,
    0x8c, 0x00, 0x00, 0x00, 0x85, 0x00, 0x00, 0x85, 0x00, 0x00, 0x86, 0x00,
    0x00, 0x86, 0x00, 0x00, 0x87, 0x00, 0x00, 0x87, 0x00, 0x00, 0x88, 0x00,
    0x00, 0x88, 0x00, 0x00, 0x00, 0x63, 0x68, 0x71, 0x72, 0x73, 0x74, 0x77,
];

// = a seg001-offset read into the data block (the DOS `[bp]` script reads).
fn icon_data(ofs: u16) -> u8 {
    TROOP_ICON_DATA[(ofs - TROOP_ICON_DATA_BASE) as usize]
}

fn icon_data_word(ofs: u16) -> u16 {
    u16::from_le_bytes([icon_data(ofs), icon_data(ofs + 1)])
}

/// = seg001:192f — the 7 equipment-type ONMAP sprite ids (harvesters ..
/// bulbs), indexed by the equipment bit slot (loc_07e3d's `[di+192fh]`).
pub(crate) fn equipment_icon_sprite(slot: usize) -> u16 {
    icon_data(0x192f + slot as u16) as u16
}

/// = seg001:18fd data_018fd — the selected-troop highlight ring's icon script
/// (the rotating sprites 0x85..0x8c), spawned by troop_0697c with flag 0x40.
pub(crate) const FOCUS_RING_SCRIPT: u16 = 0x18fd;

/// = one 0x11-byte record of the troop_icons list (seg001:3cc0): the icon's
/// screen rect (+0..+7), its ONMAP sprite (+8), the owning troop (+0xa, a
/// troop ptr in DOS, the table index here), the flags byte (+0xc: bit 0 anim
/// script armed, bit 1 no-loop, bit 6 draw-last, bit 7 hidden) and the anim
/// script cursor/base (+0xd/+0xf, seg001 offsets into TROOP_ICON_DATA).
#[derive(Clone, Copy)]
pub(crate) struct TroopIcon {
    pub(crate) rect: Rect,
    pub(crate) sprite: u16,
    pub(crate) troop_index: usize,
    pub(crate) flags: u8,
    pub(crate) script_cursor: u16,
    pub(crate) script_base: u16,
}

impl GameState {
    // = seg000:c60b troop_icon_spawn — append a troop icon: centre the ONMAP
    // sprite on (cx, cy), fill a new record and bump troop_icon_count.
    pub(crate) fn troop_icon_spawn(
        &mut self,
        sprite: u16,
        cx: i16,
        cy: i16,
        troop_index: usize,
    ) -> Option<usize> {
        // = seg000:c60c call open_onmap_resource.
        self.open_onmap_spritesheet();
        // = seg000:c610 call center_sprite_coordinates; c634..c647 the
        //   bottom-right corner from the sprite header dims.
        let (mut w, mut h) = (0i16, 0i16);
        self.with_active_bank_sheet(|_, sheet| {
            if let Some(s) = sheet.get_sprite(sprite) {
                w = s.width() as i16;
                h = s.height() as i16;
            }
        });
        if w == 0 {
            return None;
        }
        let x0 = cx - w / 2;
        let y0 = cy - h / 2;
        // = seg000:c61b inc [troop_icon_count]; c622..c647 fill the record.
        self.troop_icons.push(TroopIcon {
            rect: rect(x0, y0, x0 + w, y0 + h),
            sprite,
            troop_index,
            flags: 0,
            script_cursor: 0,
            script_base: 0,
        });
        Some(self.troop_icons.len() - 1)
    }

    // = seg000:c5cf troop_icon_spawn_with_anim — spawn a troop icon from an
    // icon script (`script` = its seg001 offset; byte 0 = the sprite, byte 3
    // nonzero = animated): plain troop_icon_spawn, then attach the script at
    // a random step and arm flag bit 0.
    pub(crate) fn troop_icon_spawn_with_anim(
        &mut self,
        script: u16,
        cx: i16,
        cy: i16,
        troop_index: usize,
    ) -> Option<usize> {
        // = seg000:c5cf al = [bp]; the anim gate at [bp+3].
        let sprite = icon_data(script) as u16;
        let i = self.troop_icon_spawn(sprite, cx, cy, troop_index)?;
        if icon_data(script + 3) == 0 {
            return Some(i);
        }
        // = seg000:c5df [di+0fh] = bp — the script base.
        // = seg000:c5e5..c5f1 count the extra steps to the sprite-0 fence.
        let mut extra = 0u16;
        while icon_data(script + 3 + 3 * extra) != 0 {
            extra += 1;
            if extra > 0x40 {
                break;
            }
        }
        // = seg000:c5f3 or bx,bx; jz — a single-step script stays unarmed
        //   (unreachable: byte 3 was nonzero, so extra >= 1).
        // = seg000:c5f7..c605 cursor = base + 3 * rand_iterated(extra) —
        //   rand_iterated's range is inclusive, so any step can start — and
        //   arm flag bit 0.
        let step = self.rand_iterated(extra);
        let icon = &mut self.troop_icons[i];
        icon.script_base = script;
        icon.script_cursor = script + 3 * step;
        icon.flags |= 1;
        Some(i)
    }

    // = seg000:6946 loc_06946 — hit-test a screen point against the troop
    // icons, top-most (last-listed) first, skipping the flag-0x40 highlight
    // ring. A hit on an unrallied troop's icon (occupation bit 7) reports as
    // a miss (seg000:6977 cmp [di+3],80h — CF only below 0x80). Returns the
    // (icon index, troop index) pair (DOS si/di).
    pub(crate) fn troop_icon_hit_test(&self, x: i16, y: i16) -> Option<(usize, usize)> {
        for i in (0..self.troop_icons.len()).rev() {
            let ic = &self.troop_icons[i];
            // = seg000:6965 test [si+0ch],40h — the highlight ring is not
            //   clickable.
            if ic.flags & 0x40 != 0 {
                continue;
            }
            // = seg000:6957..696e the strict point-in-rect test.
            if ic.rect.x0 < x && ic.rect.y0 < y && x < ic.rect.x1 && y < ic.rect.y1 {
                let ti = ic.troop_index;
                if self.troops[ti].occupation & 0x80 != 0 {
                    return None;
                }
                return Some((i, ti));
            }
        }
        None
    }

    // = seg000:6917 troop_leaf_fn_06917 (the full-map branch) — find the
    // troop's plain icon (skipping the flag-0x40 highlight ring). The room
    // branch (seg000:6938, the data_03caf person scan) is not modelled.
    pub(crate) fn troop_find_icon(&self, troop_index: usize) -> Option<usize> {
        (0..self.troop_icons.len()).find(|&i| {
            let ic = &self.troop_icons[i];
            ic.troop_index == troop_index && ic.flags & 0x40 == 0
        })
    }

    // = seg000:c58d troop_icon_remove — remove the troop icon at `index`:
    // mark it hidden, repaint its rect, compact the list and fix up the
    // focused-icon slots.
    pub(crate) fn troop_icon_remove(&mut self, index: usize) {
        if index >= self.troop_icons.len() {
            return;
        }
        // = seg000:c59f flags |= 0x80; c5a7 repaint the rect (the hidden flag
        //   keeps this very icon out of the repaint).
        self.troop_icons[index].flags |= 0x80;
        let r = self.troop_icons[index].rect;
        self.troop_icons_update_dirty_rect(r);
        // = seg000:c5ad..c5b8 compact the list over the record.
        self.troop_icons.remove(index);
        // = seg000:c5bd..c5cc the focused-slot fixups: a pointer at or past
        //   the removed record shifts down one record.
        for slot in self.troop_icon_focused.iter_mut() {
            if let Some(j) = slot {
                if *j >= index {
                    if *j == 0 {
                        *slot = None;
                    } else {
                        *j -= 1;
                    }
                }
            }
        }
    }

    // = seg000:c661 troop_icon_move_and_redraw — move the troop icon at
    // `index` by (dx, dy) and repaint the union of its old and new rects.
    pub(crate) fn troop_icon_move_and_redraw(&mut self, index: usize, dx: i16, dy: i16) {
        // = seg000:c661 call open_onmap_resource.
        self.open_onmap_spritesheet();
        // = seg000:c666..c66f the old rect into a stack temp; c677..c67f move
        //   the live rect.
        let old = self.troop_icons[index].rect;
        let icon = &mut self.troop_icons[index];
        icon.rect = rect(old.x0 + dx, old.y0 + dy, old.x1 + dx, old.y1 + dy);
        let new = icon.rect;
        // = seg000:c682..c6a2 the dirty union: the moved-toward edge comes
        //   from the new rect, the moved-away edge stays the old one.
        let union = rect(
            if dx >= 0 { old.x0 } else { new.x0 },
            if dy >= 0 { old.y0 } else { new.y0 },
            if dx >= 0 { new.x1 } else { old.x1 },
            if dy >= 0 { new.y1 } else { old.y1 },
        );
        // = seg000:c6a6 call troop_icons_update_dirty_rect.
        self.troop_icons_update_dirty_rect(union);
    }

    // = seg000:6b34 troop_icon_anim_task — the troop icon animation task
    // (interval 15, TaskId::TroopIconAnim, armed by the SEE DUNE MAP open):
    // every 4th firing steps every armed icon's script, the other firings
    // only the focused one.
    pub(crate) fn tick_troop_icon_anim(&mut self) {
        // = seg000:6b34 inc troop_icon_anim_phase.
        self.troop_icon_anim_phase = self.troop_icon_anim_phase.wrapping_add(1);
        if self.troop_icon_anim_phase & 3 != 0 {
            // = seg000:6b3f..6b4a only the focused icon (slot 0).
            if let Some(i) = self.troop_icon_focused[0] {
                self.troop_icon_anim_step(i);
            }
            return;
        }
        // = seg000:6b4b..6b87 the full walk.
        for i in 0..self.troop_icons.len() {
            self.troop_icon_anim_step(i);
        }
    }

    // = seg000:6b55..6b82 one icon's script step: read (sprite, dx, dy) at
    // the cursor — a sprite-0 fence loops back to the script base unless flag
    // bit 1 — store the new sprite, advance the cursor, move + repaint.
    fn troop_icon_anim_step(&mut self, i: usize) {
        let icon = self.troop_icons[i];
        // = seg000:6b55 test [di+0ch],1 — no script armed.
        if icon.flags & 1 == 0 {
            return;
        }
        let mut cur = icon.script_cursor;
        let mut sprite = icon_data(cur);
        if sprite == 0 {
            // = seg000:6b63..6b6b the loop-back, gated on flag bit 1.
            if icon.flags & 2 != 0 {
                return;
            }
            cur = icon.script_base;
            sprite = icon_data(cur);
        }
        // = seg000:6b6d..6b7a store the sprite, read the deltas, advance.
        let dx = icon_data(cur + 1) as i8 as i16;
        let dy = icon_data(cur + 2) as i8 as i16;
        let icon = &mut self.troop_icons[i];
        icon.sprite = sprite as u16;
        icon.script_cursor = cur + 3;
        // = seg000:6b7f call troop_icon_move_and_redraw.
        self.troop_icon_move_and_redraw(i, dx, dy);
    }

    // = seg000:c6ad troop_icons_update_dirty_rect — repaint a dirty rect of a
    // troop-icon view: lift the cursor when it overlaps `r`, clip `r` to the
    // sprite clip rect (the map window while the map view is up), restore the
    // clipped rect from the fb2 snapshot, draw the intersecting icons back
    // over it in troop_icon_draw_order_func order (fifo, or back-to-front by
    // depth on the map), then redraw the HUD head and preserve any open popup
    // the rect touches by copying its pixels back over the compose
    // (loc_0c7d4).
    pub(crate) fn troop_icons_update_dirty_rect(&mut self, r: Rect) {
        // = seg000:c6ad call open_onmap_resource.
        self.open_onmap_spritesheet();
        // = seg000:c6b0..c6e3 the cursor bracket: restore the cursor backdrop
        //   when the drawn cursor overlaps the rect, and re-draw it after the
        //   repaint (DOS pushes draw_mouse as the return address).
        self.restore_mouse_if_rect_intersects(r);
        // = seg000:c6e4..c717 clip the rect to the sprite clip rect; an empty
        //   intersection returns.
        let clip = self.map_view_clip_rect();
        let yoff = self.y_offset as i16;
        let clipped = rect(
            r.x0.max(clip.x0),
            (r.y0 + yoff).max(clip.y0),
            r.x1.min(clip.x1),
            (r.y1 + yoff).min(clip.y1),
        );
        if clipped.x1 <= clipped.x0 || clipped.y1 <= clipped.y0 {
            self.draw_mouse_cursor_if_needed();
            return;
        }
        // = seg000:c7a2..c7bd + loc_0c7d4 the open-popup preservation: DOS
        //   composes in fb1 and, before the fb1-to-screen publish, copies the
        //   open popup's rect back from the still-intact visible screen. The
        //   port composes in place on the front buffer, so it grabs the
        //   popup's pixels before the fb2 restore and lays them back after
        //   the icon draws. map_popup2_ptr (seg000:c7b0) never opens in the
        //   port (its writer, the give-equipment panel seg000:7de2, is not
        //   ported).
        let popup_save = self.map_open_popup_rect().and_then(|p| {
            let pr = rect(
                p.x0.max(clipped.x0),
                (p.y0 + yoff).max(clipped.y0),
                p.x1.min(clipped.x1),
                (p.y1 + yoff).min(clipped.y1),
            );
            if pr.x1 <= pr.x0 || pr.y1 <= pr.y0 {
                return None;
            }
            let src = match self.screen_buffer {
                crate::FbId::Fb1 => &self.framebuffer,
                _ => &self.screen,
            };
            Some((pr, crate::gfx::vga_grab_rect(src, pr)))
        });
        // = seg000:c718 call copy_clip_rect_to_screen_from_fb2 — restore the
        //   clipped rect from the fb2 snapshot; the front buffer honours the
        //   fb1 redirection like every screen push.
        match self.screen_buffer {
            crate::FbId::Fb1 => {
                crate::gfx::vga_copy_rect(&mut self.framebuffer, &self.framebuffer_saved, clipped)
            }
            _ => crate::gfx::vga_copy_rect(&mut self.screen, &self.framebuffer_saved, clipped),
        }
        // = seg000:c721..c759 collect the visible icons intersecting the
        //   clipped rect.
        let mut order: Vec<usize> = (0..self.troop_icons.len())
            .filter(|&i| {
                let ic = &self.troop_icons[i];
                ic.flags & 0x80 == 0
                    && ic.rect.x0 < clipped.x1
                    && ic.rect.y0 + yoff < clipped.y1
                    && ic.rect.x1 > clipped.x0
                    && ic.rect.y1 + yoff > clipped.y0
            })
            .collect();
        // = seg000:c763..c77d draw them in troop_icon_draw_order_func order:
        //   fifo keeps the list order; the map's by-depth policy (0xc835)
        //   layers back-to-front by ascending x1 + y1, flag-0x40 icons last.
        if self.troop_icon_draw_by_depth {
            order.sort_by_key(|&i| {
                let ic = &self.troop_icons[i];
                if ic.flags & 0x40 != 0 {
                    i32::MAX
                } else {
                    ic.rect.x1 as i32 + ic.rect.y1 as i32
                }
            });
        }
        // The icon draws land on the front buffer, like the restore above.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        for i in order {
            let ic = self.troop_icons[i];
            self.with_active_bank_sheet(|s, sheet| {
                s.draw_sprite_from_sheet_clipped(
                    sheet,
                    ic.sprite,
                    ic.rect.x0,
                    ic.rect.y0 + yoff,
                    clipped,
                );
            });
        }
        // = seg000:c780..c79f redraw the HUD head when the rect overlaps the
        //   head box (the same bounds present_screen_rect tests).
        if self.data_0227d == 0 && clipped.y1 >= 137 && clipped.x1 >= 126 && clipped.x0 < 194 {
            self.ui_hud_head_draw();
        }
        // = seg000:c7a2..c7bd lay the preserved popup pixels back on top.
        if let Some((pr, pixels)) = popup_save {
            let dst = match self.screen_buffer {
                crate::FbId::Fb1 => &mut self.framebuffer,
                _ => &mut self.screen,
            };
            crate::gfx::vga_put_rect(dst, &pixels, pr);
        }
        self.active_fb = saved;
        // Close the cursor bracket, then publish the touched screen (DOS
        // wrote straight to the visible A000 buffer).
        self.draw_mouse_cursor_if_needed();
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:686e troop_icon_screen_pos — the troop's icon screen position
    // (None = no icon: not in full-map mode, or off the window). `marker` is
    // the visible-location marker entry DOS receives in bp; a moving troop
    // (occupation bit 0x40) ignores it and projects its gps position.
    pub(crate) fn troop_icon_screen_pos(
        &self,
        troop_index: usize,
        marker: Option<&crate::travel_map_screen::MapLocationMarker>,
    ) -> Option<(i16, i16)> {
        // = seg000:686e cmp data_046eb,80h; jb ret (CF).
        if self.data_046eb < 0x80 {
            return None;
        }
        let t = &self.troops[troop_index];
        if t.occupation & 0x40 != 0 {
            // = seg000:68af..68d1 the moving troop: project the gps position;
            //   visible iff -15..327 x -15..159.
            let (sx, sy) =
                self.map_position_to_screen(t.gps_coordinates_1, t.gps_coordinates_2 as i16);
            if sx > -16 && sy > -16 && sx < 0x148 && sy < 0xa0 {
                return Some((sx, sy));
            }
            return None;
        }
        // = seg000:687b..68ad the stationed troop: the marker position plus
        //   the position-slot offset. bl = position - 1, XOR 8 when the
        //   location status has bit 1 (mirrored slot bank), & 0xf.
        let m = marker?;
        // = seg000:68a4 cmp bh,80h; jb ret — the entry's mode byte must have
        //   bit 0x80 (a full-map-mode entry).
        if m.mode & 0x80 == 0 {
            return None;
        }
        let mut slot = t.position.wrapping_sub(1);
        let li = location_index_from_ptr(t.offset_of_location);
        if let Some(loc) = self.locations.get(li) {
            if loc.status & 2 != 0 {
                slot ^= 8;
            }
        }
        // = seg000:6892..68ab the (dx, dy) pair at troop_icon_slot_offsets.
        let ofs = TROOP_ICON_DATA_BASE + 2 * (slot & 0xf) as u16;
        let dx = icon_data(ofs) as i8 as i16;
        let dy = icon_data(ofs + 1) as i8 as i16;
        Some((m.x + dx, m.y + dy))
    }

    // = seg000:6770 troop_icon_pick_script — pick the troop's icon script
    // (the seg001 offset of its 3-byte-step record; 0 = no icon).
    fn troop_icon_pick_script(&self, troop_index: usize) -> u16 {
        let t = &self.troops[troop_index];
        // = seg000:6770 test bitfield_10,10h — hidden troop.
        if t.bitfield_10 & 0x10 != 0 {
            return 0;
        }
        let occ = t.occupation;
        // = seg000:6779..678d occupation bit 7 (with bitfield_10 bit 7
        //   clear): script 0x181f only when the location status has bit 4.
        if t.bitfield_10 & 0x80 == 0 && occ & 0x80 != 0 {
            let li = location_index_from_ptr(t.offset_of_location);
            if let Some(loc) = self.locations.get(li) {
                if loc.status & 0x10 != 0 {
                    return 0x181f;
                }
            }
            return 0;
        }
        // = seg000:6792 occupation bit 6 — a moving troop.
        if occ & 0x40 != 0 {
            return self.troop_icon_pick_script_moving(troop_index);
        }
        // = seg000:6799..67c4 occupation bits 4-5 set: the specialized table,
        //   with the occ & 0xf == 0 slot routed by equipment bits 6-7.
        if occ & 0x30 != 0 {
            let low = (occ & 0x0f) as u16;
            if low == 0 {
                match t.equipment & 0xc0 {
                    0x80 => return 0x1813,
                    0x40 => return 0x1817,
                    0xc0 => return 0x181b,
                    _ => {}
                }
            }
            return icon_data_word(0x16b6 + 2 * low);
        }
        // = seg000:67c5..6826 the basic table. occ & 0xf == 0 with equipment
        //   bits 6-7 set picks the 17bc/17c9/1806 specials instead.
        let low = (occ & 0x0f) as u16;
        if low == 0 {
            match t.equipment & 0xc0 {
                0x80 => return 0x17bc,
                0x40 => return 0x17c9,
                0xc0 => return 0x1806,
                _ => {}
            }
        }
        let mut script = icon_data_word(0x179c + 2 * low);
        // = seg000:67d8..67ea script 0x1732 swaps to 0x16aa when the location
        //   status has bit 1; scripts 0x1774 / 0x1732 then step per position
        //   band.
        if script == 0x1732 {
            let li = location_index_from_ptr(t.offset_of_location);
            if let Some(loc) = self.locations.get(li) {
                if loc.status & 2 != 0 {
                    return 0x16aa;
                }
            }
        } else if script != 0x1774 {
            return script;
        }
        // = seg000:67ed..6809 the position banding: (position - 1) & 7 —
        //   < 3 -> +0, 3 -> +10, 4 -> +20, > 4 -> +30.
        let band = match (t.position.wrapping_sub(1)) & 7 {
            0..=2 => 0,
            3 => 0x0a,
            4 => 0x14,
            _ => 0x1e,
        };
        script += band;
        script
    }

    // = seg000:6827 troop_icon_pick_script_moving — a moving troop's icon
    // script: facing (the dominant axis + sign of the location-minus-gps
    // delta) plus 4 * the occupation class indexes
    // troop_icon_scripts_moving.
    fn troop_icon_pick_script_moving(&self, troop_index: usize) -> u16 {
        let t = &self.troops[troop_index];
        let li = location_index_from_ptr(t.offset_of_location);
        let Some(loc) = self.locations.get(li) else {
            return 0;
        };
        // = seg000:682d..6839 the coarse longitude delta: the high byte of
        //   (location map_x - gps), sign-extended.
        let dlng = ((loc.map_x as u16).wrapping_sub(t.gps_coordinates_1) as i16) >> 8;
        // = seg000:6841 the latitude delta.
        let dlat = loc.map_y.wrapping_sub(t.gps_coordinates_2 as i16);
        // = seg000:684a..6858 the facing: bp = 2 (vertical); a dominant
        //   horizontal axis makes it 1 and swaps the deltas; a negative
        //   dominant delta flips with XOR 2.
        let mut facing: u16 = 2;
        let dominant = if dlng.unsigned_abs() >= dlat.unsigned_abs() {
            facing -= 1;
            dlng
        } else {
            dlat
        };
        if dominant < 0 {
            facing ^= 2;
        }
        // = seg000:685b..6866 + 4 * the occupation class (bits 2-3), then the
        //   16-entry table.
        let class = ((t.occupation >> 2) & 3) as u16;
        icon_data_word(0x18bf + 2 * (facing + 4 * class))
    }

    // = seg000:6757 map_spawn_troop_icon — spawn one troop icon: resolve the
    // position and the script, bail when either says no icon.
    fn map_spawn_troop_icon(
        &mut self,
        troop_index: usize,
        marker: Option<&crate::travel_map_screen::MapLocationMarker>,
    ) {
        // = seg000:675a call troop_icon_screen_pos; jb ret.
        let Some((x, y)) = self.troop_icon_screen_pos(troop_index, marker) else {
            return;
        };
        // = seg000:675f call troop_icon_pick_script; 6762 cmp bp,1; jb ret.
        let script = self.troop_icon_pick_script(troop_index);
        if script == 0 {
            return;
        }
        // = seg000:6768 call troop_icon_spawn_with_anim.
        self.troop_icon_spawn_with_anim(script, x, y, troop_index);
    }

    // = seg000:6715 map_spawn_troop_icons — spawn the troop icons for the
    // full map view: every visible location marker's troop chain, then a
    // second pass for the moving troops (occupation bit 0x40), which sit at
    // their gps position rather than a marker slot.
    pub(crate) fn map_spawn_troop_icons(&mut self) {
        // = seg000:6718..6735 the marker walk.
        let markers = self.visible_location_markers.clone();
        for m in &markers {
            // = seg000:671f al = [di+9] — the location's troop chain head.
            let Some(loc) = self.locations.get(m.location_index as usize) else {
                continue;
            };
            let mut troop_id = loc.troop_id;
            // = seg000:6726..6730 the chain walk (next id at troop+1).
            while troop_id != 0 {
                let Some(t) = self.troops.get((troop_id - 1) as usize) else {
                    break;
                };
                let next = t.next_troop_id;
                self.map_spawn_troop_icon((troop_id - 1) as usize, Some(m));
                troop_id = next;
            }
        }
        // = seg000:6737..6754 the moving-troop pass: bitfield_10 bit 4 clear
        //   and occupation bit 6 set.
        for ti in 0..self.troops.len() {
            let t = &self.troops[ti];
            if t.bitfield_10 & 0x10 == 0 && t.occupation & 0x40 != 0 {
                self.map_spawn_troop_icon(ti, None);
            }
        }
    }

    // = seg000:8461 troop_refresh_icon (+ troop_0846c) — refresh the troop's map
    // icon: its sprite and animation script follow its occupation, so drop the
    // old icon and spawn a fresh one. A stationed troop needs a visible marker
    // at its location to have an icon at all; a moving one (occupation bit 6)
    // sits at its gps position and always respawns.
    pub(crate) fn troop_refresh_icon(&mut self, ti: usize) {
        // = seg000:8461/8467 call troop_find_icon; jz -> troop_icon_remove.
        if let Some(icon) = self.troop_find_icon(ti) {
            self.troop_icon_remove(icon);
        }
        // = seg000:846c..8479 a stationed troop with no visible marker gets no
        //   icon back.
        let t = self.troops[ti];
        let marker = if t.occupation & 0x40 != 0 {
            None
        } else {
            let li = location_index_from_ptr(t.offset_of_location);
            let Some(m) = self
                .visible_location_markers
                .iter()
                .find(|m| m.location_index as usize == li)
                .copied()
            else {
                return;
            };
            Some(m)
        };
        // = seg000:847b..8485 spawn it, then repaint its rect.
        let before = self.troop_icons.len();
        self.map_spawn_troop_icon(ti, marker.as_ref());
        if self.troop_icons.len() > before {
            let r = self.troop_icons[before].rect;
            self.troop_icons_update_dirty_rect(r);
        }
    }

    // = the SEE DUNE MAP open/close hooks for the icon system.

    // = seg000:5a66..5a6c add_frame_task(troop_icon_anim_task, 15).
    pub(crate) fn arm_troop_icon_anim_task(&mut self) {
        self.add_frame_task(15, TaskId::TroopIconAnim);
    }
}
