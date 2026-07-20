//! The dialogue/narration text engine: show_voice_subtitle (seg000:88af) and
//! everything under it — phrase-token expansion (88f1), string interpolation
//! (8944), and the subtitle renderer draw_subtitle_body (8b11) with its
//! layout (8ccd/8e16), speech-bubble painting (8f28) and restore (8c8a).
//!
//! Two dialogue presentations, selected by voice_subtitle_mode (the settings
//! screen's bottom three buttons, values 0..2):
//!   - mode 0 (text): the outlined text strip drawn over the game area just
//!     above the command panel (rect seg001:223c, the loc_0900b/09025/09046
//!     backbuffer path — the port draws it straight into fb1).
//!   - mode 1 (text + voice): a speech balloon in the game area (the 3-rect
//!     table at seg001:2224, ICONES sprite 0x1c tiled as the background).
//!   - mode 2 (voice only): no text (seg000:88da), except unvoiced lines,
//!     which adjust_subtitle_mode_for_dialogue_line downgrades to mode 1.
//!
//! Deviations, all documented in place: the mode-0 strip renders and
//! outlines in the backbuffer like DOS, but composites into fb1 immediately
//! (DOS defers to the head-present chain, seg000:9025 — same pixels,
//! different plumbing; the seg000:908c head-render re-stamp is ported); the
//! map-troop (data_046eb) and book (data_000c6) special backgrounds are
//! TODO. A top-level sentence separator (terminator != 0xff) arms the
//! multi-part continuation TALK TO ME resumes (loc_094dd).

use crate::{GameState, Rect, gfx};

/// = the live speech-bubble state: DOS keeps current_bubble_layout_ptr
/// (seg001:479e, the layout-descriptor pointer), the ui_hud_elements[18] rect
/// and the save-under pixel buffer at RESOURCE_GLOBDATA; the port bundles
/// them. `saved_fb2` holds the fb2 pixels under the rect (mode != 0 only).
pub(crate) struct SubtitleBubble {
    /// The DOS layout-descriptor identity: 0x223c is the mode-0 strip; the
    /// restore path branches on it (seg000:8c98 tests voice_subtitle_mode,
    /// which selected the descriptor).
    pub(crate) strip: bool,
    /// = seg001:479e current_bubble_layout_ptr — the seg001 address of the
    /// layout descriptor this bubble was drawn from (0x2224/0x222c/0x2234
    /// balloon sizes, 0x223c strip, 0x224c narration, 0x2275 dusk).
    pub(crate) layout: u16,
    /// Absolute framebuffer rect (y already offset by y_offset).
    pub(crate) rect: Rect,
    pub(crate) saved_fb2: Vec<u8>,
}

/// = the seg001:2224 balloon descriptor table (x, y, w, h), tried
/// smallest-first by subtitle_setup_layout.
const BALLOONS: [[i16; 4]; 3] = [
    [0x50, 0x0e, 0xc0, 0x48],
    [0x50, 0x10, 0xc8, 0x56],
    [0x50, 0x08, 0xd0, 0x61],
];

impl GameState {
    /// The live (x, y, w, h) words of a seg001 layout descriptor, keyed by
    /// its DOS address — the balloon x0 words carry the per-head patch
    /// (`balloon_x`, seg000:91d4). Used by the --log-subtitle trace, which
    /// reports the DOS (un-y_offset) geometry so it diffs against the
    /// chani_egui trace.
    fn layout_desc(&self, layout: u16) -> (i16, i16, i16, i16) {
        match layout {
            0x2224 | 0x222c | 0x2234 => {
                let b = BALLOONS[(layout - 0x2224) as usize / 8];
                (self.balloon_x, b[1], b[2], b[3])
            }
            // = seg001:223c the mode-0 strip layout.
            0x223c => (0, 0, 0x140, 0x47),
            // = seg001:224c the free-form narration layout.
            0x224c => (0x10, 0, 0x120, 0x42),
            // = seg001:2275 the dusk/night strip.
            0x2275 => (0, 0x99, 0x140, 0x2f),
            _ => (0, 0, 0, 0),
        }
    }
}

/// The text part of the trace: the interpolated glyph stream up to the
/// terminator (first byte >= 0xf0), printable ASCII verbatim, everything
/// else escaped — matching the chani_egui formatter (which caps at 160).
fn sub_trace_text(text: &[u8]) -> String {
    let mut s = String::new();
    for &b in text.iter().take(160) {
        if b >= 0xf0 {
            break;
        }
        if (0x20..0x7f).contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{b:02x}"));
        }
    }
    s
}

/// One laid-out subtitle line (an entry of the DOS per-line table at
/// ss:0a9d0, built by layout_subtitle_lines): the line's words plus the
/// justification the commit computed.
struct SubLine {
    words: Vec<Vec<u8>>,
    /// = the stored per-word advance: leftover_budget / (words-1) + 6.
    advance: u16,
    /// = the stored division remainder, distributed one pixel per gap.
    pad: u16,
}

impl GameState {
    // = seg000:88af show_voice_subtitle — record and present a voice/
    // narration subtitle: resolve the string, expand its phrase tokens,
    // interpolate the name/number placeholders, and (voice_subtitle_mode < 2)
    // lay it out and draw it.
    pub(crate) fn show_voice_subtitle(&mut self, phrase_id: u16) {
        // = seg000:88af or ax,ax; jz — a zero string id is a no-op.
        if phrase_id == 0 {
            eprintln!("show_voice_subtitle: ignoring unexpected zero phrase id");
            return;
        }
        // = seg000:88b3/88b6 record the id, clear the voiced-variant index.
        self.current_subtitle_id = phrase_id;
        self.data_047e0 = 0;
        // = seg000:88bb the data_046eb & 0x40 short-circuit drops the bubble
        //   entirely (loc_080df, the map-troop popup path). TODO: that popup
        //   context is not modelled.
        // = seg000:88ca..88cf resolve + expand (get_phrase_or_command_string_
        //   si loads the record's PHRASE bank on the way, seg000:cf78).
        self.load_phrasexx_hsq();
        let s = self.get_phrase_or_command_string(phrase_id).to_vec();
        let expanded = self.expand_phrase_tokens(&s);
        // = seg000:88d2..88d6 format into the 0xa6b0 buffer.
        let text = self.format_interpolated_string(&expanded);
        // = seg000:88da cmp voice_subtitle_mode,2; jnb loc_08888 — voice-only
        //   mode presents no text (the 8888 element juggling only re-arms the
        //   overlay bookkeeping; nothing is drawn).
        if self.voice_subtitle_mode >= 2 {
            return;
        }
        // = seg000:88e1 draw_subtitle_text_from_si.
        self.draw_subtitle_text(&text);
    }

    // = seg000:88e1 draw_subtitle_text_from_si — bail when the first byte has
    // its high bit set (nothing renderable), else lay out + draw.
    fn draw_subtitle_text(&mut self, text: &[u8]) {
        if text.first().is_none_or(|b| b & 0x80 != 0) {
            return;
        }
        self.draw_subtitle_body(text);
    }

    // = seg000:88d2 loc_088d2 — interpolate `src` into the 0xa6b0 buffer and,
    // in a text-showing subtitle mode (< 2), lay it out and draw it. The talk
    // verb's multi-part continuation re-enters the subtitle pipeline here
    // (seg000:94e1) with the pending continuation text.
    pub(crate) fn format_and_draw_subtitle(&mut self, src: &[u8]) {
        let text = self.format_interpolated_string(src);
        // = seg000:88da cmp voice_subtitle_mode,2; jnb loc_08888.
        if self.voice_subtitle_mode >= 2 {
            return;
        }
        self.draw_subtitle_text(&text);
    }

    // = seg000:88f1 expand_phrase_tokens — expand a phrase/command string
    // into the 0xa840 buffer: literal bytes copy through; 0xe0..0xfd are
    // dictionary references — 6-bit length ((word_hi & 0xc0) >> 2 | token &
    // 0xf), 14-bit offset into the PHRASE bank's last entry (the token
    // dictionary) — each followed by an implicit space unless the string
    // ends; 0xff terminates.
    pub(crate) fn expand_phrase_tokens(&self, src: &[u8]) -> Vec<u8> {
        let dict = self.phrase_dictionary();
        let mut out = Vec::new();
        let mut i = 0;
        while i < src.len() {
            let b = src[i];
            i += 1;
            // = seg000:88f9 the terminator.
            if b == 0xff {
                break;
            }
            // = seg000:88fd..8905 0xfe (the multi-part separator) and every
            //   byte below 0xe0 copy through. (The 0xa9cf overflow cap is the
            //   DOS buffer end; the Vec needs none.)
            if b == 0xfe || b < 0xe0 {
                out.push(b);
                continue;
            }
            // = seg000:8910..892d the dictionary reference.
            if i + 1 >= src.len() {
                break;
            }
            let word = u16::from_le_bytes([src[i], src[i + 1]]);
            i += 2;
            let len = ((word >> 8) as u8 >> 2 & 0x30 | (b & 0x0f)) as usize;
            let ofs = (word & 0x3fff) as usize;
            if ofs + len <= dict.len() {
                out.extend_from_slice(&dict[ofs..ofs + len]);
            }
            // = seg000:8930..8939 an implicit space between tokens, skipped
            //   right before the terminator.
            if src.get(i) != Some(&0xff) {
                out.push(0x20);
            }
        }
        // = seg000:893d the copied terminator the interpolator stops on.
        out.push(0xff);
        out
    }

    // = seg000:8944 format_interpolated_string — expand the placeholder
    // controls into the final glyph stream (the DOS 0xa6b0 buffer):
    //   < 0x80        literal glyphs/controls, copied.
    //   0x80          inline big-endian string id, included in place.
    //   0x81..0x8f    string_subst_id_table[N] (the name placeholders).
    //   0x90..0x9f    a decimal number read live from ds:[next byte]
    //                 (word-sized for 0x92, else a byte).
    //   0xa0..0xcf    extended glyphs, copied.
    //   0xd0..0xef    display controls, copied with their operands.
    //   >= 0xf0       pop the include stack; at top level, terminate.
    // Includes nest through a 12-frame stack (the sub sp,32h frame).
    pub(crate) fn format_interpolated_string(&mut self, src: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut stack: Vec<(Vec<u8>, usize)> = Vec::new();
        let mut cur: Vec<u8> = src.to_vec();
        let mut pos = 0;
        // = seg000:894b skip leading spaces.
        while cur.get(pos) == Some(&0x20) {
            pos += 1;
        }
        loop {
            let Some(&b) = cur.get(pos) else {
                // A source exhausted without a terminator behaves like one
                // (DOS strings always carry theirs).
                let Some(frame) = stack.pop() else { break };
                (cur, pos) = frame;
                continue;
            };
            pos += 1;
            match b {
                // = seg000:8954 literal.
                0x00..=0x7f => out.push(b),
                // = seg000:8970 the inline big-endian id / 0x81..0x8f the
                //   subst-table id: include that string.
                0x80..=0x8f => {
                    let id = if b == 0x80 {
                        let hi = cur.get(pos).copied().unwrap_or(0);
                        let lo = cur.get(pos + 1).copied().unwrap_or(0);
                        pos += 2;
                        u16::from_be_bytes([hi, lo])
                    } else {
                        self.string_subst_id_table[(b & 0x0f) as usize]
                    };
                    // = seg000:8984 push (si, ds); loc_08a3b; the lookup.
                    stack.push((std::mem::take(&mut cur), pos));
                    cur = self.get_phrase_or_command_string(id).to_vec();
                    pos = 0;
                }
                // = seg000:89e4 the live-number placeholder: ds var at the
                //   operand byte, word-wide for 0x92; decimal digits with
                //   leading zeros skipped (the loc_08a05..8a1f digit loop).
                0x90..=0x9f => {
                    let addr = cur.get(pos).copied().unwrap_or(0) as u16;
                    pos += 1;
                    let value = self.condit_ds_read(addr, b == 0x92);
                    let mut started = false;
                    for div in [10000u16, 1000, 100, 10, 1] {
                        let digit = (value / div % 10) as u8;
                        if digit != 0 || started || div == 1 {
                            started = true;
                            out.push(b'0' + digit);
                        }
                    }
                }
                // = seg000:89ad extended glyphs copy through.
                0xa0..=0xcf => out.push(b),
                // = seg000:899b..89ab display controls with operands: 0xd0
                //   carries 2, 0xd1 carries 4, 0xd2.. carry 1.
                0xd0..=0xef => {
                    out.push(b);
                    let operands = match b {
                        0xd0 => 2,
                        0xd1 => 4,
                        _ => 1,
                    };
                    for _ in 0..operands {
                        if let Some(&o) = cur.get(pos) {
                            out.push(o);
                            pos += 1;
                        }
                    }
                }
                // = seg000:89b0 pop an include, or terminate at top level.
                0xf0..=0xff => {
                    if let Some(frame) = stack.pop() {
                        (cur, pos) = frame;
                        continue;
                    }
                    // = seg000:89c1..89c8 the terminator byte is stored, and
                    //   the continuation pointer is (re)written: a sentence
                    //   separator (any terminator != 0xff) arms it with the
                    //   text that follows — the talk verb's multi-part resume
                    //   (loc_094dd) — while the final 0xff clears it.
                    out.push(b);
                    self.dialogue_text_continuation = if b != 0xff {
                        Some(cur[pos..].to_vec())
                    } else {
                        None
                    };
                    // = seg000:89d3..89e0 a voiced line (dialogue_line_word0
                    //   bit 4) picks a random spoken-variant index
                    //   (data_047e0 = rand & 3) and consumes the flag.
                    if self.dialogue_line_word0 & 0x10 != 0 {
                        self.data_047e0 = (self.rand() & 3) as u8;
                        self.dialogue_line_word0 &= !0x10;
                    }
                    break;
                }
            }
        }
        out
    }

    // = seg000:8c8a subtitle_restore_prior — take down the previously drawn
    // subtitle: consume the bubble state; for a balloon put the saved fb2
    // pixels back, restore fb1 from fb2 over the covered rect (the whole game
    // area for the mode-0 strip, = the si preset at seg000:8c95), redraw a
    // live talking-head overlay, and present.
    pub(crate) fn subtitle_restore_prior(&mut self) {
        // = seg000:8c8a xor ax,ax; xchg [current_bubble_layout_ptr]; cmp 2 —
        //   nothing (or only the armed marker 1) to restore.
        let Some(bubble) = self.subtitle_bubble.take() else {
            return;
        };
        // = the chani_egui trace hook at seg000:8c8a (which logs only a live
        // layout ptr >= 2, i.e. exactly when a bubble is up).
        if self.log_subtitle {
            println!("SUB restore prior layout={:04x}", bubble.layout);
        }
        let rect = if bubble.strip {
            // = seg000:8c95 si = _word_20920_game_area_rect — the strip path
            //   restores the whole game area.
            let yoff = self.y_offset as i16;
            Rect {
                x0: 0,
                y0: yoff,
                x1: 320,
                y1: yoff + 152,
            }
        } else {
            // = seg000:8c9f..8cb0 put the saved pixels back into fb2 and
            //   clear the overlay element.
            self.put_rect_pixels(&bubble);
            bubble.rect
        };
        // = seg000:8cb5 call copy_rect_fb2_to_fb1.
        gfx::vga_copy_rect(&mut self.framebuffer, &self.framebuffer_saved, rect);
        // = seg000:8cb8..8cc6 a live talking-head overlay (data_047c8) is
        //   re-rendered over the restored area (loc_09bac). The port drops
        //   the head's incremental-draw cache so its next idle tick repaints
        //   the full frame over the restored backdrop.
        if let Some(head) = self.talking_head.as_mut() {
            head.prev_images.clear();
        }
        // = seg000:8cc9 call present_game_area.
        self.present_game_area();
    }

    // = the seg000:8ca5/8fb7 vga_put_rect/vga_grab_rect pair on the
    // RESOURCE_GLOBDATA save buffer, applied to fb2.
    fn grab_rect_pixels(&mut self, rect: Rect) -> Vec<u8> {
        let mut saved = Vec::new();
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                saved.push(self.framebuffer_saved.get(x as u16, y as u16));
            }
        }
        saved
    }

    fn put_rect_pixels(&mut self, bubble: &SubtitleBubble) {
        let rect = bubble.rect;
        let mut i = 0;
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                if let Some(&p) = bubble.saved_fb2.get(i) {
                    self.framebuffer_saved.set(x as u16, y as u16, p);
                }
                i += 1;
            }
        }
    }

    // = seg000:8b11 draw_subtitle_body — restore any prior subtitle, pick the
    // layout, paint the bubble background, then render the laid-out lines.
    pub(crate) fn draw_subtitle_body(&mut self, text: &[u8]) {
        // = seg000:8b12 call subtitle_restore_prior.
        self.subtitle_restore_prior();
        // = seg000:8b16 call subtitle_setup_layout; jb ret.
        let Some((layout, rect, mut lines)) = self.subtitle_setup_layout(text) else {
            return;
        };
        // The --log-subtitle trace, mirroring the chani_egui hook at
        // seg000:8f7f (inside draw_speech_bubble, once the elem-18 rect,
        // pens and budgets are computed, before anything paints). It reports
        // the DOS descriptor geometry — not the port's y_offset rect — so
        // the two logs diff line-for-line.
        if self.log_subtitle {
            let (dx, dy, dw, dh) = self.layout_desc(layout);
            println!(
                "SUB draw_speech_bubble id={:#06x} mode={} lipsync={:04x} \
                 ctx[troop_popup={:02x} book={:02x} dusk={:02x} pending={:02x}]",
                self.current_subtitle_id,
                self.voice_subtitle_mode,
                self.current_lip_sync_resource_id,
                self.data_046eb,
                self.data_000c6,
                self.data_0227d,
                self.pending_room_screen_request,
            );
            println!(
                "SUB   layout={layout:04x} desc=({dx},{dy},{dw}x{dh}) elem18=({dx},{dy})-({},{}) \
                 pads l/r/t/b={}/{}/{}/{} flags={:02x} lines={}",
                (dx + dw).min(320),
                dy + dh,
                self.subtitle_pad_left,
                self.subtitle_pad_right,
                self.subtitle_pad_top,
                self.subtitle_pad_bottom,
                self.subtitle_layout_flags,
                lines.len(),
            );
            println!(
                "SUB   pen=({},{}) budget={}x{} text=\"{}\"",
                dx as u16 + self.subtitle_pad_left,
                dy as u16 + self.subtitle_pad_top,
                dw as u16 - self.subtitle_pad_left - self.subtitle_pad_right,
                dh as u16 - self.subtitle_pad_top - self.subtitle_pad_bottom,
                sub_trace_text(text),
            );
        }
        // = seg000:8b1b call draw_speech_bubble — also records the bubble
        //   state and the pen origin.
        let strip = self.draw_speech_bubble(layout, rect, &lines);
        // = seg000:8b1e loc_08df0 — drop full justification when any line's
        //   inter-word advance stretched past 0x1e (over-spread looks bad).
        if self.subtitle_layout_flags & 1 != 0
            && lines
                .iter()
                .any(|l| !l.words.is_empty() && l.advance >= 0x1e)
        {
            self.subtitle_layout_flags &= !0x01;
        }
        // = seg000:8b21..8b88 the pen start: rect origin + padding, then the
        //   vertical placement by subtitle_layout_flags bits 2..3 (the
        //   default 9 has 8 = centre vertically; the mode-0 strip's 1 = top).
        let pen_x = rect.x0 as u16 + self.subtitle_pad_left;
        let mut pen_y = rect.y0 as u16 + self.subtitle_pad_top;
        let line_h = 10u16;
        let height_budget =
            (rect.y1 - rect.y0) as u16 - self.subtitle_pad_top - self.subtitle_pad_bottom;
        let total_h = lines.len() as u16 * line_h;
        match self.subtitle_layout_flags & 0x0c {
            // = seg000:8b3c..8b53 flags 4: spread the lines over the budget.
            // Only the book layout uses it; line spacing stays 10 here. TODO.
            0x08 => {
                // = seg000:8b66..8b7e centre vertically (leftover / 2).
                pen_y += height_budget.saturating_sub(total_h) / 2;
            }
            0x0c => {
                // = the same path without the halving: bottom-anchored.
                pen_y += height_budget.saturating_sub(total_h);
            }
            _ => {}
        }
        // = seg000:8b8b..8c67 the per-line render loop.
        let n = lines.len();
        for (li, line) in lines.iter_mut().enumerate() {
            // = seg000:8bd1..8be2 the last line, or a layout without the
            //   justify bit, uses the plain 6-pixel inter-word gap.
            let justify = self.subtitle_layout_flags & 1 != 0 && li + 1 != n;
            let (advance, mut pad) = if justify {
                (line.advance, line.pad)
            } else {
                (6, 0)
            };
            self.font_set_draw_position(pen_x, pen_y);
            for (wi, word) in line.words.iter().enumerate() {
                if wi != 0 {
                    // = seg000:8c26..8c3e the inter-word advance (the stored
                    //   stretch + 6), +1 while the remainder lasts.
                    let mut adv = advance;
                    if pad > 0 {
                        adv += 1;
                        pad -= 1;
                    }
                    let (x, y) = self.font_get_draw_position();
                    self.font_set_draw_position(x + adv, y);
                }
                // = seg000:8be9..8c45 the in-word byte loop with the display
                //   controls: 0x01 swaps fg/bg, 0x06 selects the small font,
                //   0x08 the tall font; anything else is a glyph.
                let mut wi2 = 0;
                while wi2 < word.len() {
                    let c = word[wi2];
                    wi2 += 1;
                    match c {
                        0x01 => self.font_state.color = self.font_state.color.rotate_left(8),
                        0x06 => self.font_select_small_font(),
                        0x08 => self.font_select_tall_font(),
                        _ => self.font_draw_glyph(c),
                    }
                }
            }
            // = seg000:8c47..8c60 next line: x back to the start, y += the
            //   line spacing (data_0479a, 10 by default).
            pen_y += line_h;
        }
        // = seg000:8c7b the mode-0 strip finish: the glyph outline pass on
        //   the backbuffer copy (loc_09046), the transparent composite into
        //   fb1 (DOS defers it to the present chain, loc_09025 with the
        //   colour-0 skip), then 8c86 jmp set_fb1_as_active_framebuffer.
        if strip {
            self.outline_subtitle_strip(rect);
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    let p = self.framebuffer_back.get(x as u16, y as u16);
                    if p != 0 {
                        self.framebuffer.set(x as u16, y as u16, p);
                    }
                }
            }
            self.set_fb1_as_active_framebuffer();
            // = the chani_egui trace hook at seg000:9025 (the strip present
            // blit, logged once per placement): the DOS pen_y at present
            // time equals the strip height (pad_top + lines*10 = the
            // relocated rect's height), dest_y = 0x92 - pen_y.
            if self.log_subtitle {
                let pen_y = (rect.y1 - rect.y0) as u16;
                println!("SUB strip blit pen_y={} -> dest_y={}", pen_y, 0x92 - pen_y);
            }
        }
    }

    // = seg000:8ccd subtitle_setup_layout — pick the layout rect, colour,
    // paddings and flags for the current presentation context, and word-wrap
    // the text into it. Returns the picked descriptor's seg001 address (the
    // DOS bp) alongside; None for the DOS carry-set bail (a single-word
    // overflow that fits no rect).
    fn subtitle_setup_layout(&mut self, text: &[u8]) -> Option<(u16, Rect, Vec<SubLine>)> {
        // = seg000:8ccd flags = 9 (justify + vertical centre), fg = 0xf0.
        self.subtitle_layout_flags = 9;
        self.font_state.color = 0x00f0;
        let yoff = self.y_offset as i16;
        // = seg000:8cd8 data_046eb != 0: the map-troop popup bubble
        //   (seg001:2244). TODO: that context is not modelled; fall through.
        // = seg000:8cfb speaker 0xffff: the free-form narration rect
        //   (seg001:224c = 16,0,288,66), pads 0x48/0x10/8/8.
        if self.current_lip_sync_resource_id == 0xffff {
            self.subtitle_pad_left = 0x48;
            self.subtitle_pad_right = 0x10;
            self.subtitle_pad_top = 8;
            self.subtitle_pad_bottom = 8;
            let rect = Rect {
                x0: 0x10,
                y0: yoff,
                x1: 0x10 + 0x120,
                y1: yoff + 0x42,
            };
            let lines = self.layout_lines(text, rect)?;
            return Some((0x224c, rect, lines));
        }
        // = seg000:8d1b data_000c6 != 0: the book/document layout
        //   (seg001:2265). TODO: the BOOK background and its spread-line
        //   flags (loc_0d082) are not ported; fall through to the plain
        //   layouts so the text still shows.
        // = seg000:8d43 suppress_sky != 0: the dusk narration strip
        //   (seg001:2275 = 0,153,320,47), fg 6, no padding.
        if self.data_0227d != 0 {
            self.font_state.color = 6;
            self.subtitle_pad_left = 0;
            self.subtitle_pad_right = 0;
            self.subtitle_pad_top = 0;
            self.subtitle_pad_bottom = 0;
            let rect = Rect {
                x0: 0,
                y0: 153,
                x1: 320,
                y1: 200,
            };
            let lines = self.layout_lines(text, rect)?;
            return Some((0x2275, rect, lines));
        }
        // = seg000:8d62 voice_subtitle_mode == 0: the text-mode strip
        //   (seg001:223c = 0,0,320,71), flags 1 (justify, top-anchored), fg
        //   0x0f, pads 0x10/0x10/1/0. DOS lays it out at the backbuffer top
        //   and blits it to y = 0x92 - height at present time (loc_09025);
        //   the port relocates the rect there up front and draws into fb1.
        if self.voice_subtitle_mode == 0 {
            self.subtitle_layout_flags = 1;
            self.font_state.color = 0x000f;
            self.subtitle_pad_left = 0x10;
            self.subtitle_pad_right = 0x10;
            self.subtitle_pad_top = 1;
            self.subtitle_pad_bottom = 0;
            let rect = Rect {
                x0: 0,
                y0: 0,
                x1: 320,
                y1: 0x47,
            };
            let lines = self.layout_lines(text, rect)?;
            // = loc_09025 bx = 0x92 - subtitle_pen_y — the strip sits
            //   directly above the command panel.
            let h = (lines.len() as u16 * 10 + self.subtitle_pad_top) as i16;
            let rect = Rect {
                x0: 0,
                y0: yoff + 0x92 - h,
                x1: 320,
                y1: yoff + 0x92,
            };
            return Some((0x223c, rect, lines));
        }
        // = seg000:8d8a the voice-mode balloon: try the 3 rects at
        //   seg001:2224 smallest-first, keeping the first whose height fits
        //   the wrapped line count — with a random skip to the next size for
        //   variety (seg000:8dd3 rand_masked(1)). The last rect is the
        //   fallback with the vertical pads dropped (seg000:8dbe..8dcc, the
        //   carry-set bail when even that overflows).
        for (i, b) in BALLOONS.iter().enumerate() {
            let layout = 0x2224 + 8 * i as u16;
            // = the descriptors' x0 words carry the per-head patch
            //   (seg000:91d4), not the static 0x50.
            let rect = Rect {
                x0: self.balloon_x,
                y0: b[1] + yoff,
                x1: self.balloon_x + b[2],
                y1: b[1] + b[3] + yoff,
            };
            let lines = self.layout_lines(text, rect)?;
            let text_h = lines.len() as u16 * 10 + self.subtitle_pad_top + self.subtitle_pad_bottom;
            if text_h < b[3] as u16 {
                // = seg000:8dcd..8dd8 not the largest: a set random bit
                //   bumps to the next size anyway.
                if i + 1 < BALLOONS.len() && self.rand() & 1 != 0 {
                    continue;
                }
                return Some((layout, rect, lines));
            }
            if i + 1 == BALLOONS.len() {
                // = seg000:8dbe..8dcc the overflow fallback: drop the
                //   vertical pads; bail (carry set) when the pads were
                //   already zero.
                self.subtitle_pad_top = 0;
                let bottom = std::mem::take(&mut self.subtitle_pad_bottom);
                if bottom == 0 {
                    return None;
                }
                let lines = self.layout_lines(text, rect)?;
                return Some((layout, rect, lines));
            }
        }
        None
    }

    // = seg000:8e16 layout_subtitle_lines — word-wrap the glyph stream into
    // the per-line table: words separated by spaces, hard breaks on 0x0d,
    // 6-pixel nominal gaps, each committed line recording the justification
    // (leftover / (n-1) + 6 advance and the remainder). Returns None for the
    // loc_08e97 overflow marker (a first word wider than the whole budget —
    // DOS reports 200 lines so every rect rejects it).
    fn layout_lines(&mut self, text: &[u8], rect: Rect) -> Option<Vec<SubLine>> {
        let budget = (rect.x1 - rect.x0) as u16 - self.subtitle_pad_left - self.subtitle_pad_right;
        let mut lines = Vec::new();
        let mut words: Vec<Vec<u8>> = Vec::new();
        let mut remaining = budget as i32;
        let mut size = self.font_state.size;
        let mut i = 0;
        loop {
            let b = text.get(i).copied().unwrap_or(0xff);
            // = seg000:8e2a the high-bit terminator.
            if b & 0x80 != 0 {
                if !words.is_empty() {
                    lines.push(commit_line(&mut words, remaining));
                }
                break;
            }
            i += 1;
            match b {
                // = seg000:8e39 the hard line break.
                0x0d => {
                    lines.push(commit_line(&mut words, remaining));
                    remaining = budget as i32;
                }
                // = seg000:8e32 the word separator.
                0x20 => {}
                _ => {
                    // = seg000:8e4b call measure_word_width (which tracks the
                    //   0x06/0x08 font switches, seg000:8f0d..8f23).
                    i -= 1;
                    let start = i;
                    let mut width = 0u16;
                    while i < text.len() {
                        let c = text[i];
                        if c == 0x20 || c == 0x0d || c & 0x80 != 0 {
                            break;
                        }
                        i += 1;
                        match c {
                            0x06 => size = crate::font::TextSize::Small,
                            0x08 => size = crate::font::TextSize::Large,
                            0x01..=0x05 | 0x07 | 0x09..=0x1f => {}
                            _ => width += self.font.glyph_width(c, size) as u16,
                        }
                    }
                    if width == 0 {
                        continue;
                    }
                    let word = text[start..i].to_vec();
                    // = seg000:8e52..8e72 fit the word (+6 gap) into the
                    //   remaining budget, breaking the line when it spills.
                    let need = width as i32 + 6;
                    if remaining - need >= 0 {
                        remaining -= need;
                        words.push(word);
                    } else if remaining - need + 6 >= 0 {
                        // = seg000:8e5d..8e64 it fits without the gap: it
                        //   ends the line.
                        words.push(word);
                        lines.push(commit_line(&mut words, remaining - need + 6));
                        remaining = budget as i32;
                    } else if !words.is_empty() {
                        // = seg000:8e69..8e72 spill to a fresh line.
                        lines.push(commit_line(&mut words, remaining));
                        remaining = budget as i32 - need;
                        if remaining + 6 < 0 {
                            // = seg000:8e97 a word wider than the budget.
                            return None;
                        }
                        words.push(word);
                    } else {
                        // = seg000:8e97 the first word alone overflows.
                        return None;
                    }
                }
            }
        }
        if lines.is_empty() {
            return None;
        }
        Some(lines)
    }

    // = seg000:8f28 draw_speech_bubble — record the bubble state and paint
    // its background. `layout` is the picked descriptor's seg001 address
    // (= the DOS bp, stored in current_bubble_layout_ptr). Returns whether
    // this is the mode-0 strip (whose finishing outline pass the caller
    // runs).
    fn draw_speech_bubble(&mut self, layout: u16, rect: Rect, _lines: &[SubLine]) -> bool {
        // = seg000:8f73..8f7b the ui_hud_elements[18] rect clamps x1 to 0x140:
        //   the widest per-head balloons (balloon_x 0x7e + w 0xd0) reach past
        //   the screen edge. Only the painted/saved rect clamps — the pen and
        //   budgets upstream keep the descriptor width.
        let rect = Rect {
            x1: rect.x1.min(320),
            ..rect
        };
        // = seg000:8f8d/8f94 the dusk strip and pending room transitions
        //   paint no background at all (loc_08fd0).
        if self.data_0227d != 0 {
            return false;
        }
        // = seg000:8f9b voice_subtitle_mode == 0 -> loc_0900b: the text-mode
        //   strip renders into the (cleared) backbuffer; the port clears the
        //   strip rows in fb1 to colour 0 is NOT done — DOS composites the
        //   strip transparently (colour 0 skipped, seg000:9032 ch=0xff), so
        //   the port draws the glyphs straight over the fb1 game area at the
        //   loc_09025 placement instead.
        if self.voice_subtitle_mode == 0 && self.current_lip_sync_resource_id != 0xffff {
            // = the chani_egui trace hook at seg000:900b (loc_0900b).
            if self.log_subtitle {
                println!("SUB mode-0 strip path (no bubble bg)");
            }
            self.subtitle_bubble = Some(SubtitleBubble {
                strip: true,
                layout,
                rect,
                saved_fb2: Vec::new(),
            });
            // = seg000:900b..901f clear the strip area and render into the
            //   backbuffer — the outline pass needs the zero background, and
            //   the composite skips zero pixels.
            gfx::vga_clear_rect(
                self,
                crate::FbId::Back,
                rect.x0 as u16,
                rect.y0 as u16,
                rect.x1 as u16,
                rect.y1 as u16,
            );
            self.set_backbuffer_as_frame_buffer();
            return true;
        }
        // = seg000:8fa2..8fcf the balloon: grab the fb2 save-under
        //   (vga_grab_rect into RESOURCE_GLOBDATA), then tile ICONES sprite
        //   0x1c across the rect in fb1 (blit_repeated_x clears the rect
        //   first).
        let saved_fb2 = self.grab_rect_pixels(rect);
        self.subtitle_bubble = Some(SubtitleBubble {
            strip: false,
            layout,
            rect,
            saved_fb2,
        });
        // = the chani_egui trace hook at seg000:c370 (blit_repeated_x
        // entry); the rect logged in DOS is the elem-18 one, so report the
        // un-y_offset descriptor geometry here too.
        if self.log_subtitle {
            let (dx, dy, dw, dh) = self.layout_desc(layout);
            println!(
                "SUB blit_repeated_x sprite=0x1c rect=({dx},{dy})-({},{})",
                (dx + dw).min(320),
                dy + dh,
            );
        }
        gfx::vga_clear_rect(
            self,
            crate::FbId::Fb1,
            rect.x0 as u16,
            rect.y0 as u16,
            rect.x1 as u16,
            rect.y1 as u16,
        );
        let saved_bank = self.open_icones_spritesheet();
        // = seg000:8fcc/c370 blit_repeated_x — tile ICONES sprite 0x1c across
        //   the rect in fb1, clamped to it (the tiles must not spill past the
        //   rect, or the overflow is never saved to fb2 or cleaned on
        //   restore, leaving debris when a smaller balloon replaces a larger
        //   one).
        self.blit_repeated_x(0x1c, rect);
        self.open_sprite_bank(saved_bank as i16);
        false
    }

    // = seg000:908c loc_0908c — after a head render dirties fb1 inside the
    // live mode-0 strip (current_bubble_layout_ptr == 0x223c), re-composite
    // the strip's backbuffer pixels over it before the rect presents.
    pub(crate) fn restamp_subtitle_strip(&mut self, dirty: Rect) {
        let Some(bubble) = self.subtitle_bubble.as_ref() else {
            return;
        };
        // = seg000:9095 only the strip layout; = seg000:908f only when the
        //   dirty rect reaches into it.
        if !bubble.strip || dirty.y1 <= bubble.rect.y0 {
            return;
        }
        let rect = bubble.rect;
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                let p = self.framebuffer_back.get(x as u16, y as u16);
                if p != 0 {
                    self.framebuffer.set(x as u16, y as u16, p);
                }
            }
        }
    }

    // = seg000:9046 loc_09046 — outline the mode-0 strip's glyphs: every fg
    // pixel (0x0f) paints its 4 zero neighbours with the outline colour
    // (0xf0, or 8 while narration ducking is active), so the text reads over
    // any backdrop once composited.
    fn outline_subtitle_strip(&mut self, rect: Rect) {
        // = seg000:9055 ax = 0f00fh / ah = 8 under narration ducking.
        let target = 0x0f;
        let outline = if self.data_000ea > 0 { 8 } else { 0xf0 };
        let fb = &mut self.framebuffer_back;
        for y in rect.y0.max(1)..rect.y1.min(199) {
            for x in rect.x0.max(1)..rect.x1.min(319) {
                if fb.get(x as u16, y as u16) == target {
                    for (dx, dy) in [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)] {
                        let (nx, ny) = ((x + dx) as u16, (y + dy) as u16);
                        if fb.get(nx, ny) == 0 {
                            fb.set(nx, ny, outline);
                        }
                    }
                }
            }
        }
    }
}

// = seg000:8e9e commit_subtitle_layout_line — close the current line: the
// leftover budget spreads over the word gaps (advance = leftover/(n-1) + 6,
// remainder distributed a pixel at a time).
fn commit_line(words: &mut Vec<Vec<u8>>, remaining: i32) -> SubLine {
    let n = words.len() as u16;
    let leftover = remaining.max(0) as u16;
    let (advance, pad) = if n > 1 {
        (leftover / (n - 1) + 6, leftover % (n - 1))
    } else {
        (leftover + 6, 0)
    };
    SubLine {
        words: std::mem::take(words),
        advance,
        pad,
    }
}
