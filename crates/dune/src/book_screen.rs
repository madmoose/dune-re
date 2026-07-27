//! THE BOOK — the diary screen opened from the HUD's bottom-left book button
//! (ui_elements[3], seg000:aed6 callback_main_ui_element_03).
//!
//! The book's pages are the dialogue-played log (`dialogue_played_log`, the
//! cs:0xaa word list DOS grows at seg000:a07f whenever a replayable line is
//! spoken): each page word packs a DIALOGUE record index (low 11 bits) with
//! the speaker's lip-sync id (bits 11..15). A page renders as the speaker's
//! attribution phrase ("And the Duke said to Paul:", COMMAND 0xf3 + speaker)
//! followed by the sentence's PHRASE-bank text, drawn through the subtitle
//! engine's book layout (seg001:2265 in subtitle.rs) with its BOOK.HSQ page
//! background and illuminated drop cap.
//!
//! Navigation: the two page-corner arrows over the open-book frieze (prev
//! seg000:afc7 / next seg000:afb5), the topic verb menu (menu_book,
//! seg001:2032) filtering pages by the record's topic bits, and the close
//! hotspots/verb (seg000:b18b). Paging past the last entry rolls the game
//! credits inside the book (seg000:09f5 play_credits + the per-tick scroll
//! task at seg000:0a16); twelve special pages carry an HNM video behind the
//! invisible top-left hotspot ui_elements[23] (seg000:b1ee).

use crate::{
    GameState, TaskId, command_strings as cmd, gfx,
    room_game_screen::{CMD_GREY, CMD_HIGHLIGHT, CommandMenuRecord, ScreenElement, rec},
    sprite_bank,
};

/// = seg001:2032 menu_book — the book's verb menu (the leading 0xff priority
/// word lives in the MenuBuffer). book_menu_update_topic_availability rewrites
/// the topic entries' grey bits on open; the topic verbs move the 0x8000
/// highlight between the first four entries.
#[rustfmt::skip]
pub(crate) const MENU_BOOK: [CommandMenuRecord; 5] = [
    rec(cmd::ALL_TOPICS,          0xaf58), // -> menu_callback_choice_book_all_topics
    rec(cmd::TOPIC_PAUL_ON_DUNE,  0xaf60),
    rec(cmd::TOPIC_SPICE,         0xaf68),
    rec(cmd::TOPIC_THE_FREMEN,    0xaf70),
    rec(cmd::CLOSE_BOOK,          0xb18b), // -> callback_ui_element_book_close
];

/// = seg001:2426 book_video_page_words — the 12 page words whose pages carry
/// an HNM video; index i maps to video resource id 0x19+i (and camera icon
/// sprite 0x19+i-11). The Ctrl+V cheat (seg000:b270) copies the last 10 into
/// the dialogue-played log.
#[rustfmt::skip]
const BOOK_VIDEO_PAGE_WORDS: [u16; 12] = [
    0x8456, 0x8457, 0x8459, 0x0884, 0x1939, 0x845c,
    0x2199, 0x2a3a, 0x845d, 0x3a79, 0x8461, 0x08ae,
];

impl GameState {
    // = seg000:aed6 callback_main_ui_element_03 — THE BOOK hud button: open
    // the diary screen. Only from the plain room view (game_screen_mode_flags
    // == 0).
    pub(crate) fn callback_main_ui_element_03(&mut self) {
        // = seg000:aed6 cmp game_screen_mode_flags,0; jnz ret.
        if self.game_screen_mode_flags != 0 {
            return;
        }
        // = seg000:aede loc_0aede — leave the room view.
        self.ui_teardown_room_view();
        self.ui_hud_head_animate_down();
        self.clear_mouse_nav_rect();
        // = seg000:aee7 data_000c6 = 1 — the book owns the display (any
        // nonzero value also suppresses dialogue subtitles).
        self.data_000c6 = 1;
        // = seg000:aeec call suspend_game_clock.
        self.suspend_game_clock();
        // = seg000:aeef call select_room_ui_table.
        self.select_room_ui_table();
        // = seg000:aef2 call loc_0ad5e — re-pick the music for the book mode.
        self.update_room_music();
        // = seg000:aef5..aefa al = 0x34; bp = callback_transition_0af26.
        self.transition(0x34, |s| s.callback_transition_book_open());
        // = seg000:aefd jmp service_midi_music.
        self.service_midi_music();
    }

    // = seg000:af26 callback_transition_0af26 — the book-open builder, run
    // inside the transition (front buffer redirected to fb1).
    fn callback_transition_book_open(&mut self) {
        // = seg000:af26 call ui_set_and_draw_frieze_sides_open_book.
        self.ui_set_and_draw_frieze_sides_open_book();
        // = seg000:af29 si = ui_globe_rotation_controls[6]; call loc_0d72b —
        // install the six book controls over the nav-panel records 12..17.
        self.ui_install_nav_panel(&crate::game_ui::NAV_PANEL_BOOK);
        // = seg000:af2f call book_menu_update_topic_availability.
        self.book_menu_update_topic_availability();
        // = seg000:af32 book_topic_filter = 0 — all topics.
        self.book_topic_filter = 0;
        // = seg000:af38..af40 push menu_book (bx = fn_0d917_noop, no cleanup).
        self.screen_element_stack_push(ScreenElement::BookScreen);
        // = seg000:af43 falls through into the cover draw.
        self.callback_transition_book_cover();
    }

    // = seg000:af43 callback_transition_0af43 — draw the book cover.
    fn callback_transition_book_cover(&mut self) {
        // = seg000:af43 or data_000c6,2 — at the cover.
        self.data_000c6 |= 2;
        // = seg000:af48 call clear_game_area.
        self.clear_game_area();
        // = seg000:af4b..af4d BOOK.HSQ sprite 0 — the closed cover image.
        self.open_resource_and_draw_sprite0(sprite_bank::BOOK);
        // = seg000:af50 hide the SEE-video hotspot.
        self.ui_elements[23].flags = 0;
        // = seg000:af55 jmp ui_hud_head_draw.
        self.ui_hud_head_draw();
    }

    // = seg000:af00 book_menu_update_topic_availability — grey out empty
    // topics: probe the played log for a page carrying each topic (bh =
    // 4/8/0xc, record mask bl = 0x1c); no page sets the menu id's 0x4000
    // disabled bit, a hit clears it.
    fn book_menu_update_topic_availability(&mut self) {
        for slot in 1..=3usize {
            // = seg000:af02 bl = 0x1c, bh stepping 4, 8, 0xc.
            let filter = ((slot as u16 * 4) << 8) | 0x1c;
            // = seg000:af04 call book_find_first_page; af07..af0c ax = 0x4000
            // kept only when no page matched.
            let empty = self.book_find_first_page(filter).is_none();
            // = seg000:af17..af1b and id,0xbfff; or id,ax.
            let rec = &mut self.menu_book.records[slot];
            rec.text_id = if empty {
                rec.text_id | CMD_GREY
            } else {
                rec.text_id & !CMD_GREY
            };
        }
    }

    // = seg000:af58/af60/af68/af70 the four topic menu verbs, dispatched from
    // dispatch_command_handler: each loads its filter word (bx) and menu-entry
    // offset (bp = slot*4) and joins book_topic_select_common.
    pub(crate) fn menu_callback_choice_book_topic(&mut self, slot: usize) {
        // = seg000:af58 bx=0 bp=0 / af60 bx=0x41c bp=4 / af68 bx=0x81c bp=8 /
        // af70 bx=0xc1c bp=0xc.
        let filter = if slot == 0 {
            0
        } else {
            ((slot as u16 * 4) << 8) | 0x1c
        };
        // = seg000:af76 book_topic_select_common.
        // = seg000:af77 test data_000c6,2; jnz loc_0af86 — already at the
        // cover; otherwise flip the shown page back to the cover first.
        if self.data_000c6 & 2 == 0 {
            // = seg000:af7f loc_00a3e — remove the credits scroll task.
            self.remove_frame_task(TaskId::CreditsScroll);
            // = seg000:af82 call book_turn_back_to_cover.
            self.book_turn_back_to_cover();
        }
        // = seg000:af86 call book_find_first_page; jz ret — the topic has no
        // pages (its verb is greyed, but ALL TOPICS can also come up empty).
        if self.book_find_first_page(filter).is_none() {
            return;
        }
        // = seg000:af8c the active topic filter.
        self.book_topic_filter = filter;
        // = seg000:af90..af9d bookmark = the log base 0xa8 (0xaa when at the
        // cover) so the next-page search below starts at the first entry
        // either way (the cover path searches with dx = 0).
        self.book_bookmark_ptr = if self.data_000c6 & 2 != 0 { 0xaa } else { 0xa8 };
        // = seg000:afa0..afb1 move the 0x8000 highlight to the chosen entry.
        for rec in self.menu_book.records[..4].iter_mut() {
            rec.text_id &= !CMD_HIGHLIGHT;
        }
        self.menu_book.records[slot].text_id |= CMD_HIGHLIGHT;
        // = seg000:afb1 falls through into the next-page callback.
        self.callback_ui_element_book_next_page();
    }

    // = seg000:afb5 callback_ui_element_book_next_page — the next-page arrow
    // (and the topic verbs' tail). From the cover the search starts at the
    // bookmark itself (dx = 0), so reopening the book returns to the last
    // page read.
    pub(crate) fn callback_ui_element_book_next_page(&mut self) {
        // = seg000:afb5..afc4 cx=1, dx=2; the cover (flags bit 1) makes dx=0.
        let dx = if self.data_000c6 & 2 != 0 { 0 } else { 2 };
        self.book_page_step(dx);
    }

    // = seg000:afc7 callback_ui_element_book_prev_page — the previous-page
    // arrow. Ignored on the cover; from the rolling credits it first backs
    // the bookmark up and restores normal music.
    pub(crate) fn callback_ui_element_book_prev_page(&mut self) {
        // = seg000:afc7 test data_000c6,2; jnz ret.
        if self.data_000c6 & 2 != 0 {
            return;
        }
        // = seg000:afce test data_000c6,4 — the credits are rolling.
        if self.data_000c6 & 4 != 0 {
            // = seg000:afd5 bookmark += 2 so the backward search (which steps
            // -2 before reading) lands back on the last page.
            self.book_bookmark_ptr += 2;
            // = seg000:afda loc_00a3e — remove the credits scroll task.
            self.remove_frame_task(TaskId::CreditsScroll);
            // = seg000:afdd call loc_0ad5e — normal music service.
            self.update_room_music();
        }
        // = seg000:afe0 cx=1, dx=-2.
        self.book_page_step(-2);
    }

    // = seg000:afe6 loc_0afe6 — the shared page-step tail: search one page
    // from the bookmark in direction `dx` (0 = start at the bookmark, then
    // forward), turn to it, or handle running off either end.
    fn book_page_step(&mut self, dx: i16) {
        // = seg000:afe6..afee bx = topic filter; si = bookmark; book_find_page.
        match self.book_find_page(self.book_bookmark_ptr, dx, 1, self.book_topic_filter) {
            Some(found) => {
                // = seg000:aff3 bookmark the found page.
                self.book_bookmark_ptr = found;
                // = seg000:aff7 the page-turn animation on the HUD book.
                self.ui_draw_book_turning_page(dx < 0);
                // = seg000:affa..affd bp = book_draw_current_page; the ruffle
                // + transition 0x0e.
                self.book_page_turn_present(|s| s.book_draw_current_page());
                // = seg000:b000 call loc_09901 — drop the bubble pointer (no
                // restore; the next page draw must not put old pixels back).
                self.subtitle_bubble = None;
                // = seg000:b003 jmp redraw_active_command_menu.
                self.redraw_active_command_menu();
            }
            None => {
                // = seg000:b006..b008 backward off the front: back to the
                // cover.
                if dx < 0 {
                    self.book_turn_back_to_cover();
                    return;
                }
                // = seg000:b00a test data_000c6,4; jnz ret — the credits are
                // already rolling.
                if self.data_000c6 & 4 != 0 {
                    return;
                }
                // = seg000:b011 or data_000c6,4; b016 call play_credits —
                // paging past the last entry rolls the credits in the book.
                self.data_000c6 |= 4;
                self.book_play_credits_scroll();
                // = seg000:b019..b021 a forward page turn revealing them,
                // with only the head redraw in the transition.
                self.ui_draw_book_turning_page(false);
                self.book_page_turn_present(|s| s.ui_hud_head_draw());
            }
        }
    }

    // = seg000:b147 book_find_first_page — probe the played log from its base
    // (cs:0xa8, one forward step lands on the first entry at 0xaa) for the
    // first page matching `filter`.
    fn book_find_first_page(&self, filter: u16) -> Option<u16> {
        // = seg000:b147 si = 0xa8; dx = 2; cx = 1.
        self.book_find_page(0xa8, 2, 1, filter)
    }

    // = seg000:b150 book_find_page — step `count` matching pages through the
    // played log from `ptr` by `dx` (2 forward, -2 backward, 0 = start at
    // `ptr` then forward). A page matches when its record's flag byte masked
    // with the filter's low byte is 0 (an untopiced line) or equals the
    // filter's high byte. Returns the last match, or None when the log fence/
    // terminator was hit first.
    fn book_find_page(&self, ptr: u16, dx: i16, count: u16, filter: u16) -> Option<u16> {
        // = seg000:b150 xor di,di.
        let mask = filter as u8; // bl
        let topic = (filter >> 8) as u8; // bh
        let mut ptr = ptr;
        let mut dx = dx;
        let mut found = None;
        let mut left = count;
        loop {
            // = seg000:b152 add si,dx; or dx,2 — after the first (possibly
            // in-place) read, always step forward/backward by 2.
            ptr = ptr.wrapping_add(dx as u16);
            dx |= 2;
            // = seg000:b157..b15e the page word's low 11 bits; 0 = the fence
            // below the log / the 0 terminator past its head.
            let w = self.book_page_word(ptr);
            let idx = (w & 0x7ff) as usize;
            if idx == 0 {
                break;
            }
            // = seg000:b160..b171 the record's flag byte (DIALOGUE entry
            // bytes are 4 apart, +2 into the entry = port offset idx*4+4).
            let flags = self.dialogue.get(idx * 4 + 4).copied().unwrap_or(0);
            if flags & mask != 0 && flags & mask != topic {
                continue;
            }
            // = seg000:b173 di = si; loop — count only matching pages.
            found = Some(ptr);
            left -= 1;
            if left == 0 {
                break;
            }
        }
        // = seg000:b177 or di,di.
        found
    }

    // = the cs:[si] page-word read of book_find_page / book_draw_current_page:
    // the dialogue-played log entry at DOS pointer `ptr` (0xaa + 2*i). The
    // fence word below 0xaa and the 0 terminator at the log head both read 0.
    fn book_page_word(&self, ptr: u16) -> u16 {
        if ptr < 0xaa {
            return 0;
        }
        let i = ((ptr - 0xaa) / 2) as usize;
        self.dialogue_played_log.get(i).copied().unwrap_or(0)
    }

    // = seg000:b024 book_turn_back_to_cover — flip back to the cover: a
    // backward page-turn animation, then the ruffle + transition into the
    // cover redraw.
    fn book_turn_back_to_cover(&mut self) {
        // = seg000:b024 mov dh,0xff — a backward turn.
        self.ui_draw_book_turning_page(true);
        // = seg000:b029 bp = callback_transition_0af43.
        self.book_page_turn_present(|s| s.callback_transition_book_cover());
    }

    // = seg000:b02c loc_0b02c — present a page turn: stop any VOC, play the
    // SN2 papers-ruffle, and run transition 0x0e with the caller's draw
    // callback (DOS bp).
    fn book_page_turn_present(&mut self, render: fn(&mut GameState)) {
        // = seg000:b02c call pcm_stop_voc.
        self.pcm_player.stop();
        // = seg000:b02f..b031 al = 2; audio_start_voc — SN2, papers ruffle.
        self.audio_start_voc("SN2.HSQ");
        // = seg000:b034..b036 al = 0x0e; jmp transition.
        self.transition(0x0e, render);
    }

    // = seg000:b1af ui_draw_book_turning_page — the two-frame page-turn
    // animation on the little open book in the HUD frieze, drawn straight on
    // the visible screen: ICONES sprites 0x0a/0x0b at (27,158), forward
    // 0x0a→0x0b, backward 0x0b→0x0a, 10 ticks apart.
    fn ui_draw_book_turning_page(&mut self, backward: bool) {
        // = seg000:b1b0 call set_screen_as_active_framebuffer; b1b3
        // load_icones_sprites.
        self.set_screen_as_active_framebuffer();
        self.open_icones_spritesheet();
        // = seg000:b1b6..b1bd ax = 0x0b; a forward turn (dx >= 0) decrements.
        let first: u16 = if backward { 0x0b } else { 0x0a };
        // = seg000:b1be..b1c5 draw at (27,158).
        self.draw_active_bank_sprite(first, 27, 158);
        self.send_frame_to_display();
        // = seg000:b1c8..b1cb wait_a_bit(10).
        let start = self.game_ticks();
        self.sleep_ticks(start, 10);
        // = seg000:b1ce..b1d8 the second frame: 0x0a→0x0b, 0x0b→0x0a.
        let second = if first + 1 == 0x0b { 0x0b } else { first - 1 };
        self.draw_active_bank_sprite(second, 27, 158);
        self.send_frame_to_display();
        // = seg000:b1db..b1de wait_a_bit(10).
        let start = self.game_ticks();
        self.sleep_ticks(start, 10);
        // = seg000:b1e1..b1e7 redraw HUD record 1 (the date-area frieze
        // backdrop the sprites were stamped over).
        self.draw_ui_elements_list(1, 1);
        // = seg000:b1eb jmp set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
    }

    // = seg000:b039 loc_0b039 — draw the bookmarked page (run as the page
    // turn's transition callback, front buffer redirected to fb1).
    fn book_draw_current_page(&mut self) {
        // = seg000:b039 and data_000c6,0xf9 — clear the cover/credits bits.
        self.data_000c6 &= 0xf9;
        // = seg000:b046..b04a the bookmarked page word.
        let w = self.book_page_word(self.book_bookmark_ptr);
        // = seg000:b04f call book_lookup_page_video.
        self.book_lookup_page_video(w);
        // = seg000:b03e/b052..b05f build the page text at the 0xa6b0 buffer:
        // first the speaker-attribution phrase (0xf3 + the page word's
        // speaker bits 11..15 — "And the Duke said to Paul:", …).
        let mut text = Vec::new();
        self.book_append_phrase(0xf3 + (w >> 11), &mut text);
        // = seg000:b062..b06e dialogue_current_record_ptr = the record's
        // sentence bytes (port offset idx*4+4) — load_phrasexx_hsq picks the
        // PHRASE bank from it (DOS loads it inside the string getter; the
        // port hoists the call).
        let idx = (w & 0x7ff) as usize;
        self.dialogue_current_record_ptr = (idx * 4 + 4) as u16;
        self.load_phrasexx_hsq();
        // = seg000:b072..b07d the sentence phrase: bank 8 | the record flag
        // byte's low 2 bits as the high id bits | the sentence-lo byte.
        let flags = self.dialogue.get(idx * 4 + 4).copied().unwrap_or(0);
        let lo = self.dialogue.get(idx * 4 + 5).copied().unwrap_or(0);
        self.book_append_phrase(0x800 | ((flags as u16 & 3) << 8) | lo as u16, &mut text);
        // = seg000:b080..b084 replace the final trailing space with the 0xff
        // terminator.
        if text.last() == Some(&b' ') {
            text.pop();
        }
        text.push(0xff);
        // = seg000:b088 call draw_subtitle_text_from_si — the book layout,
        // BOOK.HSQ page background and drop cap live in subtitle.rs.
        self.draw_subtitle_text(&text);
        // = seg000:b08b..b08e si = book_page_border_rect; loc_0c551 — the
        // page frame: (0,0)-(319,151) in colour 0x53.
        self.draw_rect_outline(0, 0, 319, 151, 0x53);
        // = seg000:b091 call ui_hud_head_draw.
        self.ui_hud_head_draw();
        // = seg000:b094..b0b2 the BOOK.HSQ sprite-4 flourish, centred between
        // the end of the text and y=140, only when the text ends high enough
        // (the unsigned midpoint math also skips text past y=140).
        let end_y = self.font_state.y;
        let y = end_y.wrapping_add(0x8cu16.wrapping_sub(end_y) >> 1);
        if y < 0x8a {
            self.open_sprite_bank(sprite_bank::BOOK);
            self.draw_active_bank_sprite(4, 147, y as i16);
        }
        // = seg000:b0b5..b0d1 the topic tag (0x103 " " / 0x104 "Paul
        // Atreides" / 0x105 "Spice" / 0x106 "The Fremen", by the record flag
        // bits 2-3) at (250,139) in colour 0x64 — on every page but the
        // first.
        if self.book_bookmark_ptr != 0xaa {
            let tag = 0x103 + ((flags as u16 & 0x0c) >> 2);
            self.font_draw_phrase_or_command_string_with_color_at_pos(tag, 0x64, 250, 139);
        }
        // = seg000:b0d4..b100 the two-digit page number at (306,3), colour
        // 0x53, always through the small glyph func, tens digit blanked when
        // zero.
        self.font_set_draw_position(306, 3);
        self.font_state.color = 0x53;
        let page = (self.book_bookmark_ptr - 0xaa) / 2 + 1;
        let saved_size = self.font_state.size;
        self.font_select_small_font();
        let tens = (page / 10 % 10) as u8;
        self.font_draw_glyph(if tens == 0 { b' ' } else { b'0' + tens });
        self.font_draw_glyph(b'0' + (page % 10) as u8);
        self.font_state.size = saved_size;
        // = seg000:b103..b122 the SEE-video hotspot (HUD record 23, the
        // invisible (0,4)-(40,46) rect) and the three camera icons down the
        // left edge, on a video page.
        self.ui_elements[23].flags = 0;
        if self.book_page_video_id != 0 {
            self.ui_elements[23].flags = 0x80;
            // = seg000:b114..b11f icon sprite = video id - 11, stamped into
            // all three list slots.
            let icon = self.book_page_video_id - 11;
            // = seg001:2412 book_video_icons_list — (sprite,6,5) (sprite,6,17)
            // (sprite,6,29). The icons are BOOK.HSQ sprites (0x0e..0x19).
            let list = [(icon, 6, 5), (icon, 6, 17), (icon, 6, 29)];
            self.open_sprite_bank(sprite_bank::BOOK);
            self.with_active_bank_sheet(|s, sheet| s.draw_icons_list_at_si(&list, sheet));
        }
    }

    // = seg000:b126 book_append_phrase_to_text — append phrase `id` to the
    // page text: expand its tokens, and unless it opens with a space (the
    // blank filler entries), interpolate it and end it with a single trailing
    // space in place of the terminator.
    fn book_append_phrase(&mut self, id: u16, out: &mut Vec<u8>) {
        // = seg000:b129 call get_phrase_or_command_string_si; b131 call
        // expand_phrase_tokens.
        let s = self.get_phrase_or_command_string(id).to_vec();
        let expanded = self.expand_phrase_tokens(&s);
        // = seg000:b135 cmp byte [si],20h; jz — a leading space appends
        // nothing.
        if expanded.first() == Some(&b' ') {
            return;
        }
        // = seg000:b13a call format_interpolated_string; b13d..b140 dec di;
        // stosb ' ' — the terminator becomes a trailing space.
        let mut t = self.format_interpolated_string(&expanded);
        if t.last().is_some_and(|&b| b >= 0xf0) {
            t.pop();
        }
        t.push(b' ');
        out.extend_from_slice(&t);
    }

    // = seg000:b254 book_lookup_page_video — scan the 12 video page words for
    // `w`; a hit at index i stores video resource id 0x19+i, a miss stores 0.
    fn book_lookup_page_video(&mut self, w: u16) {
        // = seg000:b258..b26a repnz scasw over data_02426; ax = 0x24 - cx.
        self.book_page_video_id = BOOK_VIDEO_PAGE_WORDS
            .iter()
            .position(|&v| v == w)
            .map_or(0, |i| 0x19 + i as u16);
    }

    // = seg000:b18b callback_ui_element_book_close — close the book (the
    // frieze hotspot, the close-button sprite and the " Close book" verb all
    // land here) and return to the room view.
    pub(crate) fn callback_ui_element_book_close(&mut self) {
        // = seg000:b18b call midi_begin_song_fade_out (seg000:adbe): start a
        // 300-tick fade to silence unless a ramp is already running.
        if !self.midi.is_fading() {
            self.midi.set_ducking(0x12c, 0, 0);
        }
        // = seg000:b18e game_suspend_count = 0 — dropped wholesale, not
        // decremented.
        self.game_suspend_count = 0;
        // = seg000:b193/b198 the book flags and the SEE-video hotspot off.
        self.data_000c6 = 0;
        self.ui_elements[23].flags = 0;
        // = seg000:b19d call hnm_close_resource — a rolling credits clip.
        self.hnm_close();
        // = seg000:b1a0 loc_00a3e — remove the credits scroll task.
        self.remove_frame_task(TaskId::CreditsScroll);
        // = seg000:b1a3 loc_09901 — drop the bubble pointer.
        self.subtitle_bubble = None;
        // = seg000:b1a6..b1a9 reopen PERS.HSQ (the room portrait bank).
        self.open_sprite_bank(sprite_bank::PERS);
        // = seg000:b1ac jmp loc_01877 — the shared enter-room tail, entered
        // past the neg at seg000:186e so room_view_toggle keeps pointing at
        // the room view.
        self.ui_enter_room_view_tail();
    }

    // = seg000:09f5 play_credits (the in-book scrolling entry, distinct from
    // the blocking startup credits at seg000:0309) — load the first
    // CREDITS.HNM frame into fb1, start the WORMSUIT score unless the CD
    // playlist owns the music, and install the per-tick scroll task.
    fn book_play_credits_scroll(&mut self) {
        // = seg000:09ef play_CREDITS_HNM2 — hnm_load_first_frame(0x14).
        self.hnm_load_first_frame("CREDITS.HNM", 0);
        // = seg000:09f8 call update_screen_palette.
        self.update_screen_palette();
        // = seg000:09fb..0a09 WORMSUIT unless the day sky is up and the CD
        // playlist mode owns the music.
        if self.data_0227d != 0 || self.music_playlist_flags & 1 == 0 {
            self.midi.play_music_wormsuit_hsq(&mut self.dat_file);
        }
        // = seg000:0a0c..0a11 add frame_task_callback_00a16 (bp = 0: every
        // tick).
        self.add_frame_task(0, TaskId::CreditsScroll);
    }

    // = seg000:0a16 frame_task_callback_00a16 — the credits scroll step: one
    // HNM frame per firing, with the active framebuffer preserved around it.
    pub(crate) fn credits_scroll_frame_task(&mut self) {
        // = seg000:0a16 push [framebuffer_active_seg].
        let saved = self.active_fb();
        // = seg000:0a23 loc_00a23.
        if self.data_0227d != 0 {
            // = seg000:0a2a the intro path: decode straight onto the screen.
            self.set_screen_as_active_framebuffer();
            if self.hnm_do_frame() {
                self.send_frame_to_display();
            }
        } else {
            // = seg000:0a30..0a3b the in-book path: decode into fb1, and on
            // an advanced frame present the game area and the mouse.
            self.set_fb1_as_active_framebuffer();
            if self.hnm_do_frame() {
                self.present_game_area();
                self.draw_mouse();
            }
        }
        // = seg000:0a1d pop [framebuffer_active_seg].
        self.active_fb = saved;
    }

    // = seg000:b1ee callback_main_ui_element_23 — the SEE-verb on a video
    // page: play the page's HNM full screen, skippable, then restore the
    // page.
    pub(crate) fn callback_main_ui_element_23(&mut self) {
        // = seg000:b1ee call [gfx_vtable_vga_save_palette_to_fade_target] —
        // keep the book palette to swap back after the clip.
        self.palette_fade_target = self.palette.clone();
        // = seg000:b1f2 call midi_reset.
        self.midi.midi_reset();
        // = seg000:b1f5 is_voc_pcm_playing = 1 — parks the CD-playlist
        // service under the clip; the port's service checks the live PCM
        // player instead.
        // = seg000:b1fa..b1ff transition 0x34 into a cleared framebuffer +
        // the video's first frame.
        self.transition(0x34, |s| s.callback_transition_book_video_load());
        // = seg000:b202 call set_screen_as_active_framebuffer.
        self.set_screen_as_active_framebuffer();
        // = seg000:b205 pause_enabled = 0; b20a kb_clear_scancode.
        self.pause_enabled = 0;
        self.kb_clear_scancode();
        // = seg000:b20d..b215 the skippable frame loop (hnm_do_frame_
        // skippable + check_if_hnm_complete).
        loop {
            if self.hnm_do_frame() {
                self.send_frame_to_display();
            }
            if self.any_key_pressed() {
                self.kb_clear_scancode();
                break;
            }
            if self.hnm_is_complete() {
                break;
            }
            let now = self.game_ticks();
            self.sleep_ticks(now, 1);
        }
        // = seg000:b217 call hnm_close_resource; b21a inc pause_enabled.
        self.hnm_close();
        self.pause_enabled = self.pause_enabled.wrapping_add(1);
        // = seg000:b21e call pcm_stop_voc; b221 set_fb1; b224 the palette
        // swap back to the book's.
        self.pcm_player.stop();
        self.set_fb1_as_active_framebuffer();
        gfx::vga_swap_palettes(self);
        // = seg000:b228..b22d transition 0x34 back into the redrawn page.
        self.transition(0x34, |s| s.callback_transition_book_video_done());
        // = seg000:b230 snapshot fb1 to fb2; b233 jmp set_voc_pcm_is_not_
        // playing (see the is_voc_pcm_playing note above).
        self.copy_active_framebuffer_to_framebuffer_2();
    }

    // = seg000:b236 callback_transition_0b236 — clear the framebuffer and
    // load the page video's first frame.
    fn callback_transition_book_video_load(&mut self) {
        self.gfx_clear_active_framebuffer();
        let id = self.book_page_video_id;
        self.hnm_load_first_frame_by_id(id, 0);
    }

    // = seg000:b23f callback_transition_0b23f — rebuild the book screen after
    // the clip: clear, redraw the 0x12 HUD records, the verb menu and the
    // page, then drop the bubble pointer.
    fn callback_transition_book_video_done(&mut self) {
        self.gfx_clear_active_framebuffer();
        // = seg000:b242..b248 cx = 0x12; draw HUD records 0..17.
        self.draw_ui_elements_list(0, 0x12);
        // = seg000:b24b call redraw_active_command_menu.
        self.redraw_active_command_menu();
        // = seg000:b24e call book_draw_current_page.
        self.book_draw_current_page();
        // = seg000:b251 jmp loc_09901.
        self.subtitle_bubble = None;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::BOOK_VIDEO_PAGE_WORDS;
    use crate::{GameState, dat_file::DatFile, room_game_screen::ScreenElement};

    // Open THE BOOK from the starting throne room with the played log seeded
    // the way the Ctrl+V cheat does (seg000:b270: the ten video page words at
    // data_0242a), page forward twice and back to the cover, and close it.
    // Asset-gated (needs assets/DUNE.DAT); writes book_cover.png /
    // book_page_1.png / book_page_2.png. Run with:
    //   cargo test -p dune --bin dune -- --ignored book_screen
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn book_screen_opens_pages_and_closes() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        // = seg000:b270 handle_ctrl_v_once — the cheat appends the ten words
        // at data_0242a (= BOOK_VIDEO_PAGE_WORDS[2..]) to the played log.
        game.dialogue_played_log
            .extend_from_slice(&BOOK_VIDEO_PAGE_WORDS[2..]);

        // The book button: the cover comes up over the open-book frieze with
        // menu_book as the active screen element.
        game.callback_main_ui_element_03();
        assert_eq!(game.data_000c6, 3, "book open + cover bits");
        assert_eq!(game.get_active_screen_element(), ScreenElement::BookScreen);
        game.screen
            .write_png_scaled(&game.palette, "book_cover.png")
            .expect("write book_cover.png");

        // Next page from the cover shows the first logged page: bookmark on
        // the first entry, page word 0x8459 = video 0x19 (record flags carry
        // no topic bits for it, so the ALL TOPICS filter matches), the
        // SEE-video hotspot armed.
        game.callback_ui_element_book_next_page();
        assert_eq!(game.data_000c6, 1, "cover bit cleared after the turn");
        assert_eq!(game.book_bookmark_ptr, 0xaa);
        assert_eq!(game.book_page_video_id, 0x19 + 2);
        assert_eq!(game.ui_elements[23].flags, 0x80);
        game.screen
            .write_png_scaled(&game.palette, "book_page_1.png")
            .expect("write book_page_1.png");

        // Page 2.
        game.callback_ui_element_book_next_page();
        assert_eq!(game.book_bookmark_ptr, 0xac);
        game.screen
            .write_png_scaled(&game.palette, "book_page_2.png")
            .expect("write book_page_2.png");

        // Back past the front flips to the cover (bit 1 again), keeping the
        // bookmark on the first entry.
        game.callback_ui_element_book_prev_page();
        game.callback_ui_element_book_prev_page();
        assert_eq!(game.data_000c6 & 2, 2, "back past the front = the cover");

        // Close: flags drop, the room base menu replaces menu_book in place.
        game.callback_ui_element_book_close();
        assert_eq!(game.data_000c6, 0);
        assert_eq!(
            game.get_active_screen_element(),
            ScreenElement::RoomCommandMenu
        );
    }
}
