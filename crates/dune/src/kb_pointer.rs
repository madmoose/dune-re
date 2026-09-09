//! Keyboard pointer control: the keyboard tail of poll_pointer_input (the
//! keypad direction keys, the confirm keys acting as the mouse button, the
//! Ctrl+arrow accelerated move and the arrow-key jump to the nearest screen
//! target) and the game_loop glide that carries the pointer to that target.
//!
//! Ported from seg000:d962 (the glide step), daaf (the clamped move),
//! df56..dfa6 (the keyboard tail) and dfb7..e26e (the target pick, its five
//! scanners and eight direction scorers, and the accelerated move). Routines
//! are in DOS address order.

use crate::{GameState, Rect, menu_defs::MenuRef};

// = seg001:271c kb_pointer_key_table — {button, dx, dy} for the keypad
// scancodes 0x47..0x53 (7 8 9 - 4 5 6 + 1 2 3 0 .). The host folds the arrow
// keys onto the same codes (DOS reads the extended-key alias at +0x12 too).
// The button words of keypad 0 and . are accumulated and then overwritten
// in DOS (seg000:df6c / df86): dead.
const KB_POINTER_KEY_TABLE: [(u16, i16, i16); 13] = [
    (0, -1, -1),
    (0, 0, -1),
    (0, 1, -1),
    (0, 0, 0),
    (0, -1, 0),
    (0, 0, 0),
    (0, 1, 0),
    (0, 0, 0),
    (0, -1, 1),
    (0, 0, 1),
    (0, 1, 1),
    (1, 0, 0),
    (2, 0, 0),
];

// = seg001:2462 map_scroll_arrow_points — the eight map scroll-arrow hot
// points around the map window centre.
const MAP_SCROLL_ARROW_POINTS: [(i16, i16); 8] = [
    (0x7f, 0x39),
    (0xa0, 0x31),
    (0xc1, 0x39),
    (0x78, 0x59),
    (0xc8, 0x59),
    (0x7f, 0x79),
    (0xa0, 0x81),
    (0xc1, 0x79),
];

// = seg001:28e9 mixer_slider_points — the mixer panel's slider knob points.
const MIXER_SLIDER_POINTS: [(i16, i16); 10] = [
    (0xbe, 0x2c),
    (0xbe, 0x33),
    (0xbe, 0x3a),
    (0xbe, 0x41),
    (0xbe, 0x48),
    (0xbe, 0x4f),
    (0xbe, 0x56),
    (0xbe, 0x72),
    (0xbe, 0x79),
    (0xbe, 0x80),
];

/// A direction scorer: (candidate x, candidate y, pointer x, pointer y) ->
/// the score (lower is better; 0xffff rejects).
type Scorer = fn(i16, i16, i16, i16) -> u16;

// = seg000:dfa7 kb_direction_scorers — by direction: up-right, right,
// down-right, up-left, left, down-left, up, down.
const KB_DIRECTION_SCORERS: [Scorer; 8] = [
    kb_score_up_right,
    kb_score_right,
    kb_score_down_right,
    kb_score_up_left,
    kb_score_left,
    kb_score_down_left,
    kb_score_up,
    kb_score_down,
];

/// The scan's running best: (score, target).
struct KbTargetScan {
    mx: i16,
    my: i16,
    scorer: Scorer,
    best: u16,
    target: (i16, i16),
}

impl KbTargetScan {
    // = the shared `call [bp]; cmp ax,[bp+2]; jnb; store` step of every scanner.
    fn offer(&mut self, px: i16, py: i16) {
        let score = (self.scorer)(px, py, self.mx, self.my);
        if score < self.best {
            self.best = score;
            self.target = (px, py);
        }
    }
}

impl GameState {
    // = seg000:d962 kb_pointer_glide_step — one game_loop pass of the
    // keyboard pointer glide (kb_glide_steps != 0): after 6 ticks since the
    // last step, move the pointer toward the target by (delta >> shift) | 1
    // (shift 2, 1 or 0 by elapsed ticks 6/12/24; shift 0 = 3/4 of the
    // delta), clamped; on arrival the glide ends. Releases the button state
    // and falls into mouse_stuff.
    pub(crate) fn kb_pointer_glide_step(&mut self) -> u16 {
        // = seg000:d962..d96b al = tick - kb_glide_tick; cmp al,6; jb.
        let now = self.game_ticks() as u8;
        let elapsed = now.wrapping_sub(self.kb_glide_tick);
        if elapsed >= 6 {
            // = seg000:d96d..d979 cx = 2, 1 (>= 12) or 0 (>= 24).
            let shift = if elapsed < 12 {
                2
            } else if elapsed < 24 {
                1
            } else {
                0
            };
            // = seg000:d97a..d980 kb_glide_tick = tick; dec kb_glide_steps.
            self.kb_glide_tick = now;
            self.kb_glide_steps = self.kb_glide_steps.wrapping_sub(1);
            // = seg000:d984..d9ba the per-axis step: sar by cx then or 1, or
            //   delta - delta / 4 when cx is 0; zero stays zero.
            let step = |delta: i16| -> i16 {
                if delta == 0 {
                    0
                } else if shift != 0 {
                    (delta >> shift) | 1
                } else {
                    delta.wrapping_sub(delta >> 2)
                }
            };
            let dx = step(self.kb_glide_target_x.wrapping_sub(self.mouse_pos_x as i16));
            let dy = step(self.kb_glide_target_y.wrapping_sub(self.mouse_pos_y as i16));
            // = seg000:d9ba..d9c5 or ax,dx; jnz; kb_glide_steps = 0.
            if dx == 0 && dy == 0 {
                self.kb_glide_steps = 0;
            } else {
                // = seg000:d9c7/d9ca call mouse_move_clamped; mouse_button_state = 0.
                self.mouse_move_clamped(dx, dy);
                self.mouse_button_state = 0;
            }
        }
        // = seg000:d9cf jmp mouse_stuff.
        self.mouse_stuff()
    }

    // = seg000:daaf mouse_move_clamped — mouse_pos += (dx, dy), clamped to
    // the mouse clip region (0, 0)-(319, 199), the only range
    // define_mouse_range ever set (seg000:e659).
    pub(crate) fn mouse_move_clamped(&mut self, dx: i16, dy: i16) {
        let x = (self.mouse_pos_x as i16).wrapping_add(dx).clamp(0, 319);
        let y = (self.mouse_pos_y as i16).wrapping_add(dy).clamp(0, 199);
        self.mouse_pos_x = x as u16;
        self.mouse_pos_y = y as u16;
        // = seg000:dae3 falls into set_mouse_pos — push the position into
        //   the driver (InputState::set_mouse_pos; the host warps its pointer).
        self.input.lock().unwrap().set_mouse_pos(x as u16, y as u16);
    }

    // = seg000:df56 poll_pointer_input's keyboard tail (loc_0df56): sum the
    // held keypad direction keys, fold the confirm keys into the button
    // state, and with a direction held either jump to the nearest target
    // (kb_pointer_pick_target) or, with Ctrl, move the pointer
    // (kb_pointer_accelerated_move).
    pub(crate) fn poll_pointer_input_keyboard(&mut self) {
        let (keys, ctrl) = {
            let input = self.input.lock().unwrap();
            (input.kb_keys, input.kb_keys[0x1d])
        };
        // = seg000:df56..df77 the 13 keypad slots (scancodes 0x47..0x53)
        //   through kb_pointer_key_table: dx, bx = the direction sums.
        let (mut dx, mut dy) = (0i16, 0i16);
        for (i, &(_, kdx, kdy)) in KB_POINTER_KEY_TABLE.iter().enumerate() {
            if keys[0x47 + i] != 0 {
                dx = dx.wrapping_add(kdx);
                dy = dy.wrapping_add(kdy);
            }
        }
        // = seg000:df79..df94 the keyboard button: Space (0x39) | Enter
        //   (0x1c) | the extended Delete (data_0cee6; the host folds Delete
        //   onto 0x53), bit 0. The previous pass's bit is cleared from the
        //   live button state, then the current one OR-ed in.
        let kb_button = (keys[0x39] | keys[0x1c] | keys[0x53]) & 1;
        let prev = std::mem::replace(&mut self.kb_button_prev, kb_button);
        self.mouse_button_state = (self.mouse_button_state & !prev) | kb_button;
        // = seg000:df97..dfa3 no direction held: the accelerated-move run
        //   state resets.
        if dx == 0 && dy == 0 {
            self.kb_move_dist_x = 0;
            self.kb_move_dist_y = 0;
            self.kb_move_frac = [0; 2];
            return;
        }
        // = seg000:dfb7 cmp [kb_keys_ctrl],0ffh; jnz kb_pointer_pick_target.
        if ctrl != 0xff {
            self.kb_pointer_pick_target(dx as i8, dy as i8);
        } else {
            self.kb_pointer_accelerated_move(dx as i8, dy as i8);
        }
    }

    // = seg000:dfc1 kb_pointer_pick_target — an arrow key without Ctrl:
    // pick the scorer by the (dx, dy) signs, then score the HUD elements,
    // the mixer slider knobs, the troop icons, the visible location markers
    // and the map scroll arrows; the best (lowest) score becomes the glide
    // target.
    fn kb_pointer_pick_target(&mut self, dl: i8, bl: i8) {
        // = seg000:dfc1..dfe4 the scorer index by the signs.
        let idx = if dl != 0 {
            let base = if dl < 0 { 4 } else { 1 };
            if bl == 0 {
                base
            } else if bl < 0 {
                base - 1
            } else {
                base + 1
            }
        } else if bl == 0 {
            // = seg000:dfe0 jz ret.
            return;
        } else if bl < 0 {
            6
        } else {
            7
        };
        // = seg000:dfe7..dff2 bx = the scorer; the scan frame; kb_clear_scancode.
        self.kb_clear_scancode();
        let mut scan = KbTargetScan {
            mx: self.mouse_pos_x as i16,
            my: self.mouse_pos_y as i16,
            scorer: KB_DIRECTION_SCORERS[idx],
            // = seg000:e002 [bp+2] = 8000h.
            best: 0x8000,
            target: (0, 0),
        };
        // = seg000:dffd..e02f the HUD elements: flag 0x80, not under the
        //   pointer (rect_contains's carry = inside), scored at their focus
        //   point.
        for e in self.ui_elements.iter() {
            if e.flags & 0x80 == 0 {
                continue;
            }
            let r = Rect {
                x0: e.x0 as i16,
                y0: e.y0 as i16,
                x1: e.x1 as i16,
                y1: e.y1 as i16,
            };
            if r.contains_interior(scan.mx, scan.my) {
                continue;
            }
            let (px, py) = rect_focus_point(r);
            scan.offer(px, py);
        }
        // = seg000:e031..e03a the four other scanners.
        self.kb_target_scan_mixer_sliders(&mut scan);
        self.kb_target_scan_troop_icons(&mut scan);
        self.kb_target_scan_location_markers(&mut scan);
        self.kb_target_scan_map_arrows(&mut scan);
        // = seg000:e03d cmp [bp+2],0; js — nothing scored below 0x8000.
        if scan.best & 0x8000 != 0 {
            return;
        }
        // = seg000:e043 data_0ceba (the Space slot) = 0; e048 or
        //   kb_keys_enter,0 (no effect).
        self.input.lock().unwrap().kb_keys[0x39] = 0;
        // = seg000:e04d..e061 the glide target, 100 steps, the tick stamp.
        self.kb_glide_target_x = scan.target.0;
        self.kb_glide_target_y = scan.target.1;
        self.kb_glide_steps = 0x64;
        self.kb_glide_tick = self.game_ticks() as u8;
    }

    // = seg000:e068 kb_target_scan_mixer_sliders — the mixer panel's slider
    // knob points while the mixer panel is the active screen element; the
    // point under the pointer is skipped.
    fn kb_target_scan_mixer_sliders(&self, scan: &mut KbTargetScan) {
        if self.get_active_menu_ref() != MenuRef::MenuMixerPanel {
            return;
        }
        for &(px, py) in MIXER_SLIDER_POINTS.iter() {
            if px == scan.mx && py == scan.my {
                continue;
            }
            scan.offer(px, py);
        }
    }

    // = seg000:e0a2 kb_target_scan_troop_icons — the troop icons' focus
    // points on the full map view (data_046eb bit 7), skipping hidden and
    // highlight icons (flags 0xc0) and the one under the pointer.
    fn kb_target_scan_troop_icons(&self, scan: &mut KbTargetScan) {
        if self.data_046eb & 0x80 == 0 {
            return;
        }
        for ic in self.troop_icons.iter() {
            if ic.flags & 0xc0 != 0 || ic.rect.contains_interior(scan.mx, scan.my) {
                continue;
            }
            let (px, py) = rect_focus_point(ic.rect);
            scan.offer(px, py);
        }
    }

    // = seg000:e0db kb_target_scan_location_markers — the visible location
    // markers while a map view is up (data_046eb != 0); the marker under the
    // pointer is skipped. DOS reads the y as a byte.
    fn kb_target_scan_location_markers(&self, scan: &mut KbTargetScan) {
        if self.data_046eb == 0 {
            return;
        }
        for m in self.visible_location_markers.iter() {
            let (px, py) = (m.x, m.y & 0xff);
            if px == scan.mx && py == scan.my {
                continue;
            }
            scan.offer(px, py);
        }
    }

    // = seg000:e11c kb_target_scan_map_arrows — the eight map scroll-arrow
    // points in the windowed map view (data_046eb bit 0) while the player
    // marker is drawn (map_player_marker_rect.x0 != 0); the point under the
    // pointer is skipped.
    fn kb_target_scan_map_arrows(&self, scan: &mut KbTargetScan) {
        if self.data_046eb & 1 == 0 || self.map_player_marker_rect.x0 == 0 {
            return;
        }
        for &(px, py) in MAP_SCROLL_ARROW_POINTS.iter() {
            if px == scan.mx && py == scan.my {
                continue;
            }
            scan.offer(px, py);
        }
    }

    // = seg000:e1d1 kb_pointer_accelerated_move — Ctrl + arrow: move the
    // pointer by the key direction, doubled at each of three distance
    // thresholds of the run so far, times the elapsed ticks (at most 8),
    // with a 3-bit sub-pixel fraction per axis.
    fn kb_pointer_accelerated_move(&mut self, dl: i8, bl: i8) {
        // = seg000:e1d1..e1f1 x: |kb_move_dist_x| against 4 / 12 / 36.
        let mut dl = dl;
        if dl != 0 {
            let a = self.kb_move_dist_x.unsigned_abs();
            for threshold in [4, 12, 0x24] {
                if a < threshold {
                    break;
                }
                dl = dl.wrapping_add(dl);
            }
        }
        // = seg000:e1f3..e211 y against 3 / 10 / 28. The `jns` at e1fa
        //   tests the flags of `or bl,bl`, so the distance is negated when
        //   the KEY direction is negative, not when the distance is.
        let mut bl = bl;
        if bl != 0 {
            let dist = self.kb_move_dist_y;
            let a = if bl < 0 { dist.wrapping_neg() } else { dist } as u16;
            for threshold in [3, 10, 0x1c] {
                if a < threshold {
                    break;
                }
                bl = bl.wrapping_add(bl);
            }
        }
        // = seg000:e213..e224 cl = min(tick - kb_move_tick, 8); kb_move_tick = tick.
        let now = self.game_ticks() as u8;
        let elapsed = now
            .wrapping_sub(std::mem::replace(&mut self.kb_move_tick, now))
            .min(8);
        // = seg000:e226..e23c the two axes through kb_pointer_accel_axis,
        //   accumulated into the run distances.
        let mx = kb_pointer_accel_axis(&mut self.kb_move_frac[0], dl, elapsed);
        self.kb_move_dist_x = self.kb_move_dist_x.wrapping_add(mx);
        let my = kb_pointer_accel_axis(&mut self.kb_move_frac[1], bl, elapsed);
        self.kb_move_dist_y = self.kb_move_dist_y.wrapping_add(my);
        // = seg000:e240 jmp mouse_move_clamped.
        self.mouse_move_clamped(mx, my);
    }
}

// = seg000:e159 rect_focus_point — the keyboard focus point of a rect:
// x = (x0 + x1) / 2, y = y1 - (y1 - y0) / 4.
fn rect_focus_point(r: Rect) -> (i16, i16) {
    let x = ((r.x0 as u16).wrapping_add(r.x1 as u16) >> 1) as i16;
    let y =
        r.y1.wrapping_sub(((r.y1.wrapping_sub(r.y0)) as u16 >> 2) as i16);
    (x, y)
}

// = seg000:e173 the straight scorers' tail (loc_0e173): si = |si|; reject
// unless ax >= 6 (signed) and si < 50 (unsigned); score = ax + 2 * si.
fn kb_score_straight(ax: i16, si: i16) -> u16 {
    let si = if si < 0 { si.wrapping_neg() } else { si };
    if ax < 6 || (si as u16) >= 0x32 {
        return 0xffff;
    }
    (ax as u16).wrapping_add((si as u16).wrapping_mul(2))
}

// = seg000:e1a8 the diagonal scorers' tail (loc_0e1a8): reject unless both
// ax and si >= 6 (signed); score = |ax - si|.
fn kb_score_diagonal(ax: i16, si: i16) -> u16 {
    if ax < 6 || si < 6 {
        return 0xffff;
    }
    let d = ax.wrapping_sub(si);
    (if d < 0 { d.wrapping_neg() } else { d }) as u16
}

// = seg000:e16f kb_score_right — ax = px - mx, si = py - my.
fn kb_score_right(px: i16, py: i16, mx: i16, my: i16) -> u16 {
    kb_score_straight(px.wrapping_sub(mx), py.wrapping_sub(my))
}

// = seg000:e18c kb_score_left — ax = mx - px.
fn kb_score_left(px: i16, py: i16, mx: i16, my: i16) -> u16 {
    kb_score_straight(mx.wrapping_sub(px), py.wrapping_sub(my))
}

// = seg000:e192 kb_score_up — ax = my - py, si = px - mx.
fn kb_score_up(px: i16, py: i16, mx: i16, my: i16) -> u16 {
    kb_score_straight(my.wrapping_sub(py), px.wrapping_sub(mx))
}

// = seg000:e19b kb_score_down — ax = py - my, si = px - mx.
fn kb_score_down(px: i16, py: i16, mx: i16, my: i16) -> u16 {
    kb_score_straight(py.wrapping_sub(my), px.wrapping_sub(mx))
}

// = seg000:e1a2 kb_score_up_right — ax = px - mx, si = my - py.
fn kb_score_up_right(px: i16, py: i16, mx: i16, my: i16) -> u16 {
    kb_score_diagonal(px.wrapping_sub(mx), my.wrapping_sub(py))
}

// = seg000:e1b9 kb_score_up_left — ax = mx - px, si = my - py.
fn kb_score_up_left(px: i16, py: i16, mx: i16, my: i16) -> u16 {
    kb_score_diagonal(mx.wrapping_sub(px), my.wrapping_sub(py))
}

// = seg000:e1c3 kb_score_down_left — ax = mx - px, si = py - my.
fn kb_score_down_left(px: i16, py: i16, mx: i16, my: i16) -> u16 {
    kb_score_diagonal(mx.wrapping_sub(px), py.wrapping_sub(my))
}

// = seg000:e1cb kb_score_down_right — ax = px - mx, si = py - my.
fn kb_score_down_right(px: i16, py: i16, mx: i16, my: i16) -> u16 {
    kb_score_diagonal(px.wrapping_sub(mx), py.wrapping_sub(my))
}

// = seg000:e243 kb_pointer_accel_axis — one axis of the accelerated move:
// ax = dir * elapsed ticks; its magnitude plus the axis's 3-bit fraction
// byte gives the new fraction (low 3 bits) and the whole part (>> 3, with
// the byte sign-extended first), returned with the sign of the product.
fn kb_pointer_accel_axis(frac: &mut u8, dir: i8, elapsed: u8) -> i16 {
    // = seg000:e243 imul cl.
    let ax = (dir as i16).wrapping_mul(elapsed as i8 as i16);
    // = seg000:e245/e247 or ax,ax; js loc_0e25a.
    let negative = ax < 0;
    let mag = if negative { ax.wrapping_neg() } else { ax };
    // = seg000:e249..e257 / e25c..e26a al += [si]; [si] = al & 7; cbw;
    //   shr ax,1 x3.
    let al = (mag as u8).wrapping_add(*frac);
    *frac = al & 7;
    let whole = (((al as i8) as i16) as u16 >> 3) as i16;
    if negative {
        whole.wrapping_neg()
    } else {
        whole
    }
}
