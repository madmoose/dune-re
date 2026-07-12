//! The in-game PALACE PLAN overlay (seg000:18ee ui_draw_palace_plan), reached
//! from the room-screen nav-panel centre button (the compass centre,
//! ui_elements[17] / NAV_PANEL_ROOM[5], func_ptr 0x18ee).
//!
//! The handler toggles a full-screen-right overlay that shows a schematic plan
//! of the palace with a population marker per room: a coloured backing rect, a
//! four-deep bevel border, the PALPLAN.HSQ base bitmap + labels, and, per
//! palace room, up to five "ally" dots, up to five "enemy" dots, and a marker
//! on the player's current room. A click anywhere (or a second centre click)
//! closes it again.

use crate::{
    GameState, Rect,
    game_ui::MouseHandlers,
    gfx,
    room_game_screen::{MENU_DONE, ScreenElement},
    sprite_bank,
};

// = seg001:1aba mouse_handlers_01aba — the PALACE PLAN overlay's MouseHandlers
// record. idle and every button slot but the LMB press are the no-op loc_00f66;
// the LMB press is menu_callback_choice_exit_menu (0xd2e2), so a click anywhere
// over the plan closes it. ui_draw_palace_plan installs it via
// set_active_mouse_handlers (= seg000:1906).
pub(crate) static PALACE_PLAN_MOUSE_HANDLERS: MouseHandlers = MouseHandlers {
    idle: GameState::palace_plan_idle,
    lmb: GameState::palace_plan_lmb,
    rmb: GameState::palace_plan_rmb,
    release: GameState::palace_plan_release,
    rmb_release: GameState::palace_plan_rmb_release,
    drag: GameState::palace_plan_drag,
    rmb_drag: GameState::palace_plan_rmb_drag,
};

// = seg001:120b _stru_206BB_icon_list — the PALPLAN.HSQ icon list drawn by
// draw_icons_list_at_si (each entry is (sprite, x, y), 0xffff-terminated): the
// plan base bitmap (sprite 0) at the plan origin (182, 12) and the three room
// labels (sprites 3..5). The origin doubles as data_0120d/data_0120f, the base
// the per-room marker offsets are added to.
const PALACE_PLAN_ICONS: [(u16, i16, i16); 4] = [
    (0x0000, 182, 12),
    (0x0003, 266, 65),
    (0x0004, 238, 65),
    (0x0005, 193, 65),
];

// = data_0120d / data_0120f — the plan origin (x, y) the marker offsets below
// are measured from (also the first PALACE_PLAN_ICONS entry's position).
const PLAN_X: i16 = 182;
const PLAN_Y: i16 = 12;

// = seg001:1426 data_01426 — the 11 per-room marker offsets (x, y) added to the
// plan origin, one per palace room. DOS reads each word as two `lodsb` bytes
// (low = x offset, high = y offset).
const PALACE_PLAN_ROOM_OFFSETS: [(u8, u8); 11] = [
    (29, 65),
    (85, 49),
    (29, 33),
    (57, 17),
    (57, 65),
    (29, 49),
    (1, 49),
    (29, 1),
    (29, 17),
    (57, 49),
    (1, 65),
];

impl GameState {
    // = seg000:18ee ui_draw_palace_plan — the nav-panel centre button
    // (ui_elements[17]). Toggles the PALACE PLAN overlay: a second activation
    // while it is up closes it; otherwise, when standing in a palace location
    // other than room 1, it composes the plan into fb1 and reveals it. `button`
    // is the DOS click byte, which this handler ignores.
    pub(crate) fn ui_draw_palace_plan(&mut self) {
        // = seg000:18ee call get_active_screen_element; cmp bp,2012h (menu_done);
        //   jz menu_callback_choice_exit_menu — a second click closes the plan.
        //   menu_done (0x2012) is shared with the unported on-map troop screen;
        //   PalacePlan is the only identity the port maps to it.
        if self.get_active_screen_element() == ScreenElement::PalacePlan {
            self.menu_callback_choice_exit_menu();
            return;
        }
        // = seg000:18fa cmp ah,20h; jnz ret / cmp al,1; jz ret — only open over a
        //   palace location (location_and_room high byte 0x20) other than room 1.
        let lr = self.location_and_room;
        if (lr >> 8) != 0x20 || (lr & 0xff) == 1 {
            return; // = seg000:1947 ret.
        }
        // = seg000:1906 ax=mouse_handlers_01aba; set_active_mouse_handlers — a
        //   click anywhere over the plan now closes it.
        self.active_mouse_handlers = &PALACE_PLAN_MOUSE_HANDLERS;
        // = seg000:190c call dismiss_stacked_overlays.
        self.dismiss_stacked_overlays();
        // = seg000:190f call set_fb1_as_active_framebuffer — compose offscreen.
        self.set_fb1_as_active_framebuffer();
        // = seg000:1912 si=data_0143c; al=0f1h; vga_fill_rect — the plan's
        //   right-side backing rect.
        gfx::vga_fill_rect(self, 160, 0, 320, 116, 0xf1);
        // = seg000:191f si=data_01444; al=0f7h; loc_05b6e — the 4-deep bevel.
        self.draw_nested_rect_outline(164, 4, 316, 112, 0xf7);
        // = seg000:1927 ax=21h (PALPLAN.HSQ); open_spritesheet.
        self.open_sprite_bank(sprite_bank::PALPLAN);
        // = seg000:192d si=_stru_206BB_icon_list; draw_icons_list_at_si — the plan
        //   base bitmap (sprite 0 at the origin) and the three labels.
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_icons_list_at_si(&PALACE_PLAN_ICONS, sheet);
        });
        // = seg000:1933 call iterate_over_all_NPCs_and_do_quite_a_bit_more — stamp
        //   the per-room population markers.
        self.iterate_over_all_npcs_and_draw_population();
        // = seg000:1936 si=data_0143c; al=10h; blit_fb1_to_screen_effect — scroll
        //   the composed plan onto the visible screen.
        self.blit_fb1_to_screen_effect(0x10, palace_plan_rect());
        // = seg000:193e bp=menu_done; bx=loc_019fc; jmp loc_0d323 — push the
        //   overlay as the active screen element (cleanup = palace_plan_cleanup),
        //   fold the " Done" command strip on, then highlight the cursor's slot.
        self.screen_overlay_request_transition();
        self.screen_element_stack_push(ScreenElement::PalacePlan, MENU_DONE.to_vec());
        self.play_pending_panel_fold();
        self.highlight_hovered_text_action_item();
    }

    // = seg000:19fc loc_019fc — the PALACE PLAN overlay's cleanup callback (the
    // DOS bx passed to the screen-element push). Restore the room screen the plan
    // covered: clear the active mouse hotspot, copy the plan's backing rect back
    // from fb2 (the clean composed screen) into fb1, scroll-reveal it, and
    // reselect the room mouse-handler table. screen_element_stack_pop_and_cleanup
    // invokes it by the PalacePlan identity.
    pub(crate) fn palace_plan_cleanup(&mut self) {
        // = seg000:19fc call clear_some_mouse_rect.
        self.clear_some_mouse_rect();
        // = seg000:19ff si=143ch; call copy_rect_fb2_to_fb1 — restore the right-
        //   side rect from the clean fb2 backup into fb1. The port's vga_copy_rect
        //   takes absolute coordinates, so apply fb_base_ofs (y_offset) here.
        let yoff = self.y_offset as i16;
        let r = Rect {
            x0: 160,
            y0: yoff,
            x1: 320,
            y1: 116 + yoff,
        };
        gfx::vga_copy_rect(&mut self.framebuffer, &self.framebuffer_saved, r);
        // = seg000:1a07 al=12h; call blit_fb1_to_screen_effect — scroll the
        //   restored area back onto the screen.
        self.blit_fb1_to_screen_effect(0x12, palace_plan_rect());
        // = seg000:1a0c jmp select_room_ui_table — restore the room mouse handlers.
        self.select_room_ui_table();
    }

    // = seg000:1948 iterate_over_all_NPCs_and_do_quite_a_bit_more — tally every
    // present NPC into a per-room histogram, then stamp each palace room's
    // markers onto the composed plan. The histogram has three 0xc-entry banks:
    // bank A (flag 0x40 clear) and bank B (flag 0x40 set) hold the ally/enemy
    // counts indexed by room-1, and bank C marks the player's current room.
    fn iterate_over_all_npcs_and_draw_population(&mut self) {
        // = seg000:1948 sub sp,24h; rep stosb — a zeroed 36-byte histogram.
        let mut hist = [0u8; 0x24];
        // = seg000:195e dh=[data_00007] — the high byte of the current location's
        //   appearance word (location_appearance, data_00006).
        let cur_appearance_hi = (self.location_appearance >> 8) as u8;
        // = seg000:1956 si=room_persons; cx=10h — tally the 16 room-person slots.
        for i in 0..16 {
            let rp = self.room_persons[i];
            // = seg000:1962 cmp dh,[si+3] — match on the appearance high byte.
            if (rp.location_appearance >> 8) as u8 != cur_appearance_hi {
                continue;
            }
            // = seg000:1967 al=[si+0eh]; is_Gurney_Halleck_and_between_game_
            //   phases_15_and_20; jb — drop Gurney during phases 0x15..0x20.
            if self.is_gurney_between_phases_15_and_20(rp.person_index) {
                continue;
            }
            // = seg000:196f bx=[si]; dec bl — index by the entry's room number-1.
            let mut idx = ((rp.location_and_room & 0xff) as i32) - 1;
            // = seg000:1975 test [si+0fh],40h — flag 0x40 selects the second bank.
            if rp.flags & 0x40 != 0 {
                idx += 0xc;
            }
            // = seg000:197e inc [bx+di]. DOS room numbers stay within 1..0xc, so
            //   idx is always in-bounds; guard rather than corrupt the stack.
            if (0..hist.len() as i32).contains(&idx) {
                hist[idx as usize] += 1;
            }
        }
        // = seg000:1985 bx=[location_and_room]; xor bh,bh; cmp bl,0ch; ja skip;
        //   add bl,17h; inc [bx+di] — mark the player's current room in bank C.
        let cur_room = (self.location_and_room & 0xff) as i32;
        if cur_room <= 0xc {
            let idx = cur_room + 0x17;
            if (0..hist.len() as i32).contains(&idx) {
                hist[idx as usize] += 1;
            }
        }

        // = seg000:1995 inc di; cx=0bh; si=1426h — draw the 11 room markers. After
        //   the `inc di`, room k (0..10, = palace room k+2) reads bank A at
        //   hist[1+k], bank B at hist[0xd+k], and bank C at hist[0x19+k].
        for (k, &(ox, oy)) in PALACE_PLAN_ROOM_OFFSETS.iter().enumerate() {
            // = seg000:199c dx=[data_0120d]; bx=[data_0120f]; lodsb/add — the plan
            //   origin plus this room's marker offset.
            let mut x = PLAN_X + ox as i16;
            let mut y = PLAN_Y + oy as i16;
            // = seg000:19af add bx,2; add dx,3.
            x += 3;
            y += 2;
            // = seg000:19b5 cl=[di]; loc_019df — bank A (ally) dots.
            self.draw_population_dots(hist[1 + k], x, y);
            // = seg000:19ba add bx,7; cl=[di+0ch]; loc_019df — bank B (enemy) dots.
            y += 7;
            self.draw_population_dots(hist[0xd + k], x, y);
            // = seg000:19c3 sub bx,4; add dx,9.
            x += 9;
            y -= 4;
            // = seg000:19c9 cmp [di+18h],0; jz skip; ax=1;
            //   draw_sprite_clobbering_bx_dx — the player's-current-room marker.
            if hist[0x19 + k] != 0 {
                self.draw_active_bank_sprite(1, x, y);
            }
        }
    }

    // = seg000:19df loc_019df — draw up to five PALPLAN.HSQ marker dots (sprite 2)
    // in a horizontal row at (x, y), one per `count`, capped at five, advancing
    // 4 px per dot.
    fn draw_population_dots(&mut self, count: u8, x: i16, y: i16) {
        // = seg000:19df xor ch,ch; jcxz ret — nothing to draw.
        // = seg000:19e5 cmp cl,5; jbe; else cl=5 — cap the row at five dots.
        let n = (count as i16).min(5);
        let mut x = x;
        for _ in 0..n {
            // = seg000:19ed ax=2; call draw_sprite.
            self.draw_active_bank_sprite(2, x, y);
            // = seg000:19f4 add dx,4.
            x += 4;
        }
    }

    // = seg000:0f66 nullsub_00f66 — the PALACE PLAN's idle handler ([si] of the
    // mouse_handlers_01aba record, seg001:1aba), a no-op.
    fn palace_plan_idle(&mut self) {}

    // = [si+2] of mouse_handlers_01aba — the LMB press is menu_callback_choice_
    // exit_menu (0xd2e2): a click anywhere over the plan closes it.
    fn palace_plan_lmb(&mut self) {
        self.menu_callback_choice_exit_menu();
    }

    // = [si+4]/[si+6]/[si+8]/[si+0ah]/[si+0ch] of mouse_handlers_01aba — the RMB,
    // release and drag slots are all the no-op loc_00f66.
    fn palace_plan_rmb(&mut self) {}
    fn palace_plan_release(&mut self) {}
    fn palace_plan_rmb_release(&mut self) {}
    fn palace_plan_drag(&mut self, _dx: i16, _dy: i16) {}
    fn palace_plan_rmb_drag(&mut self, _dx: i16, _dy: i16) {}
}

// = data_0143c — the PALACE PLAN's right-side rect (x0, y0, x1, y1), the region
// vga_fill_rect backs and blit_fb1_to_screen_effect reveals.
fn palace_plan_rect() -> Rect {
    Rect {
        x0: 160,
        y0: 0,
        x1: 320,
        y1: 116,
    }
}
