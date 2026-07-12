//! Person-dialogue entry path: clicking a character's "&Person" verb (or the
//! person sprite in the room) shows that character's talking head and starts a
//! conversation — e.g. clicking "DUKE LETO ATREIDES" shows Leto's portrait over
//! the zoomed throne room and plays "I am the Duke Leto Atreides, your father."
//!
//! Mirrors the contiguous DOS block around seg000:92f2..9472: the per-character
//! trampolines (dispatched in [`crate::room_game_screen`]'s
//! `dispatch_command_handler`), the shared setup
//! `common_code_for_ui_dialogue_related_functions` (seg000:93aa), and its
//! callees. Functions are laid out here in DOS address order.
//!
//! This is the entry path: it zooms the room in on the speaker
//! (zoom_room_to_dialogue_speaker), shows the talking head (reusing the
//! already-ported [`crate::GameState::setup_talking_head`]), records the speaker
//! and installs the dialogue verb panel (setup_npc_dialogue_menu, loc_090bd),
//! then presents the first line (menu_callback_choice_talk_to_me, seg000:9472).
//! Both the talk verb and the room-leave auto-dialogue scan share the DOS
//! present routine present_first_matching_dialogue_line (seg000:9f9e): the
//! entry walk + per-entry condition, the talking-head setup, the spoken-line
//! event callbacks + spoken mark + dialogue-played log, and the voice `.voc`
//! playback are ported. Still stubbed: the subtitle text (draw_subtitle_body
//! and the whole phrase/text engine) and the multi-part text continuation
//! (dialogue_text_continuation_ptr stays 0).

use std::io::Cursor;

use bytes_ext::ReadBytesExt;

use crate::{GameState, Rect, container, gfx};

impl GameState {
    // = seg000:cfb9 build_per_person_voc_base_table .
    pub(crate) fn build_voc_base_table(&mut self) {
        let count = container::entry_count(&self.dialogue);

        // Every person has 8 dialogue slots.
        for i in 0..count / 8 {
            // Find the first non-empty slot for this person.
            for j in 0..8 {
                let entry = container::entry(&self.dialogue, i * 8 + j);
                let mut c = Cursor::new(entry);

                if c.read_le_u16()
                    .expect("build_voc_base_table: failed to read entry")
                    == 0xffff
                {
                    continue;
                }

                assert!(
                    entry.len() >= 4,
                    "build_voc_base_table: entry {j} too short"
                );

                let word1 = c
                    .read_be_u16()
                    .expect("build_voc_base_table: failed to read entry word1");

                self.voc_bases[i as usize] = (word1 & 0x3ff) - 1;
                break;
            }
        }
    }

    // = the seg000:a708 `[bx*2 - 280ch]` read — the voc-index base for voc
    // directory id `dir_id` (the lip-sync id clamped to 0x0e at seg000:a6e7).
    pub(crate) fn voc_base(&self, dir_id: u16) -> u16 {
        self.voc_bases[dir_id.min(16) as usize]
    }

    // = seg000:a097 or byte [si], 0x80 — mark the sentence entry at `entry_offset`
    // (absolute, within `data`) spoken, so a later walk's verb-panel mask skips it
    // and the replay queue does not re-add it.
    fn mark_spoken(&mut self, entry_offset: usize) {
        if let Some(b) = self.dialogue.get_mut(entry_offset) {
            *b |= 0x80;
        }
    }

    // = the loc_09fab..loc_09fd6 walk of present_first_matching_dialogue_line
    // (seg000:9f9e) — walk the 4-byte sentence entries from absolute offset `start`
    // and return the first entry whose condition holds (its phrase id plus the
    // event id that fires when the line is spoken). Each entry is
    // [word0_le, word1_le]; a word0 of 0xffff (seg000:9fad) terminates the record
    // with no match — Err carries the terminator's offset (DOS leaves si there).
    //
    // Per-entry gate (seg000:9fb2): the entry is condition-checked unless its word0
    // low byte has bit 7 set and bit 6 clear and (low byte & `mask`) is nonzero, in
    // which case it is skipped without evaluating. `mask` is data_047c2, the dialogue
    // verb-panel mask set_dialogue_speaker primes to 0x80.
    //
    // The condition id (seg000:9fc0) is word0's high byte plus the top two bits of
    // word1's low byte: `al = word0_hi; ah = (entry[2] rol 2) & 3`. Conditions are
    // evaluated against `game`'s live state (GameState::condition_holds); with
    // CONDIT not loaded they read as always-true, matching the prior
    // always-first-entry stub.
    fn interpret_record(&self, start: usize, mask: u8) -> Result<SelectedLine, usize> {
        let mut off = start;
        let mut c = Cursor::new(&self.dialogue[off..]);
        loop {
            // = seg000:9fab mov ax,[si]; cmp ax,0ffffh; jz (no match). A walk that
            // runs off the buffer (a corrupt offset) ends like a terminator.
            let Some(word0) = c.read_le_u16().ok() else {
                return Err(off);
            };
            if word0 == 0xffff {
                return Err(off);
            }
            let lo = word0 as u8;

            let b2 = c.read_u8().unwrap();
            let b3 = c.read_u8().unwrap();

            // = seg000:9fb2..9fbe — flag-gated entries (bit7 set, bit6 clear, masked
            // by data_047c2) are skipped without evaluating their condition.
            let skip = (lo & 0x80) != 0 && (lo & 0x40) == 0 && (lo & mask) != 0;
            if !skip {
                // = seg000:9fc0 — condition id = word0_hi | top-2-bits(entry[2]) << 8.

                let cond_id = (word0 >> 8) | ((((b2 >> 6) & 3) as u16) << 8);
                let holds = self.condition_holds(cond_id);
                // = seg000:9fd1 jnz loc_09fd8 — non-zero result selects this entry.
                if holds {
                    // = seg000:9ff7 — the selected entry's phrase id: word1
                    // byteswapped, low 10 bits, phrase-marked (bit 11).
                    let word1 = u16::from_le_bytes([b2, b3]);
                    return Ok(SelectedLine {
                        phrase: (word1.swap_bytes() & 0x3ff) | 0x800,
                        // = seg000:a049 al = [si] & 0x0f — the spoken-line event id.
                        event: lo & 0x0f,
                        word0,
                        entry2: b2,
                        // = seg000:a097 `si` — this entry's absolute offset.
                        entry_offset: off,
                    });
                }
            }
            // = seg000:9fd3 add si,4 — advance to the next sentence entry.
            off += 4;
        }
    }
}

/// = the sentence entry dialogue_interpret_record selects: the phrase id to
/// present and the event id fire_event_callbacks_from_spoken_dialogue_lines_and_
/// more (seg000:a03f) dispatches when the line is spoken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct SelectedLine {
    /// = seg000:9ff7 — word1 byteswapped, low 10 bits, phrase-marked (bit 11).
    pub(crate) phrase: u16,
    /// = seg000:a049 `al = [si] & 0x0f` — the event-callback id (0 = none).
    pub(crate) event: u8,
    /// = seg000:9ff9 dialogue_line_word0 — the entry's first word.
    pub(crate) word0: u16,
    /// The entry's third byte: condition-id top bits (6..7), the voiced-line
    /// flag (bit 4, seg000:a0f8) and the replay flags (bits 2..3, seg000:a061).
    pub(crate) entry2: u8,
    /// Byte offset of the selected sentence entry within the DIALOGUE buffer
    /// (DOS `si`, absolute), used to mark the entry spoken (= seg000:a097).
    pub(crate) entry_offset: usize,
}

impl GameState {
    // = seg000:3af9 zoom_room_to_dialogue_speaker — zoom the room scene in on the
    // speaker before the talking head is composited over it: re-render the room,
    // then 4×-zoom it around the clicked character's on-screen anchor so the head
    // sits on a close-up of where they stand. Leaves the zoomed room in fb1, which
    // common_dialogue's setup_talking_head then saves as the head's backdrop.
    pub(crate) fn dialogue_zoom_room(&mut self) {
        // = seg000:3af9 cmp night_attack_stage,0; jnz copy_game_area_to_screen_
        // fb2_to_fb1 — during the night attack just restore the game area from
        // fb2 (no zoom).
        if self.night_attack_stage != 0 {
            self.copy_game_area_fb2_to_fb1();
            return;
        }
        // = seg000:3b03 cmp room_render_flags,0; js loc_03b58 (ret) — the sign
        // bit suppresses the zoom.
        if (self.room_render_flags as i8) < 0 {
            return;
        }
        // = seg000:3b0a ax = current_lip_sync_resource_id. The special-room
        // person (al == 0x0f) adds data_0476c to pick its character_*_table slot.
        let id = self.current_lip_sync_resource_id;
        if id as u8 == 0x0f {
            // = seg000:3b11 add al,[data_0476c]. TODO: data_0476c (the special-
            // room slot offset) is not modelled; the simple person handlers never
            // reach id 0x0f, so this branch is currently unreached.
        }
        // = seg000:3b15 di = id*4; dx = character_x_table[id]; bx =
        // character_y_table[id] (the [47f8h]/[47fah] anchors sal_draw_character
        // recorded — the port's character_screen_pos).
        let (fx, fy) = self.character_screen_pos[id as usize];
        // = seg000:3b1f or dx,dx; js loc_03b58 (ret) — 0xffff (absent anchor,
        // cleared by loc_03ae9 before the room is drawn) means no zoom.
        if (fx as i16) < 0 {
            return;
        }
        // = seg000:3b28 or room_render_flags,80h — the redraw-for-zoom flag
        // draw_SAL reads (not yet modelled in draw_location_room).
        self.room_render_flags |= 0x80;
        // = seg000:3b2d call loc_037b5 — re-render the room scene into fb1 (the
        // scene-draw half of draw_room_scene, without its lip-sync reset).
        self.draw_location_room(self.location_and_room, self.location_appearance);
        // = seg000:3b32..3b40 clamp the anchor so the scale-6 (4×) source window
        // (320/4 × 152/4 = 80×38 px) stays inside the 320×152 game area:
        // 320−80 = 0xf0, 152−38 = 0x72 (DOS clamps the row to 0x71).
        let col = (fx as i16).min(0xf0);
        let row = (fy as i16).min(0x71);
        // = seg000:3b43..3b4f es=fb2, ds=fb1, bp=6, call vga_zoom_screen — zoom
        // the freshly-drawn room into fb2 at 4× around the clicked character.
        crate::zoom::vga_zoom_fb1_to_fb2(self, col, row, 6);
        // = seg000:3b55 jmp copy_game_area_to_screen_fb2_to_fb1 — copy the zoomed
        // game area from fb2 back to fb1.
        self.copy_game_area_fb2_to_fb1();
    }

    // = seg000:c43e copy_game_area_to_screen_fb2_to_fb1 — copy the game-area rect
    // (_word_20920_game_area_rect = (0,0,320,152)) from fb2 to fb1 via
    // copy_rect_fb2_to_fb1 (seg000:c446). The port's vga_copy_rect takes absolute
    // framebuffer coordinates, so apply the fb_base_ofs (y_offset) here.
    fn copy_game_area_fb2_to_fb1(&mut self) {
        let yoff = self.y_offset as i16;
        let rect = Rect {
            x0: 0,
            y0: yoff,
            x1: 320,
            y1: yoff + 152,
        };
        gfx::vga_copy_rect(&mut self.framebuffer, &self.framebuffer_saved, rect);
    }

    // = seg000:93aa common_code_for_ui_dialogue_related_functions — the shared
    // tail every per-character trampoline (seg000:92f2..9371) jumps to with
    // `al` = the speaker's lip-sync resource index. Open the speaker's portrait,
    // zoom the room to them, show the talking head, then run the dialogue.
    pub(crate) fn common_dialogue(&mut self, person_index: u8) {
        // = seg000:93aa xor ah,ah — ax = the lip-sync resource index (0..0xd).
        // = seg000:93ac data_047e1 = 0. TODO: data_047e1 not modelled.
        // = seg000:93b3 current_lip_sync_resource_id = ax.
        self.current_lip_sync_resource_id = person_index as u16;
        // = seg000:93b9 call zoom_room_to_dialogue_speaker — zoom the room to the
        // speaker first, so the head composites over the zoomed backdrop. DOS opens
        // the portrait sheet (setup_lip_sync_data_from_sprite_sheet, 91a0) at 93b6,
        // just before this; the port bundles that open+parse into setup_talking_head
        // below, which only reads the sheet (not the framebuffer), so deferring it
        // past the zoom is harmless.
        self.dialogue_zoom_room();
        // = seg000:93b6 setup_lip_sync_data_from_sprite_sheet (91a0, open+parse);
        //   93bc setup_lip_sync_data_from_current; 93bf loc_09908 (install the
        //   idle animator frame task); 93cc loc_09bac (first head render) — the
        //   port's setup_talking_head bundles all four into one call.
        self.setup_talking_head(person_index, 0);
        // = seg000:93cf call ui_save_head_rect.
        self.ui_hud_head_save_rect();
        // = seg000:93d2 call update_screen_palette.
        self.update_screen_palette();
        // = seg000:93d5 call loc_0c4dd — present the head rect to the screen.
        self.present_dialogue_head();
        // = seg000:93d9 call set_dialogue_speaker — record the speaker and arm
        // the dialogue verb panel.
        self.set_dialogue_speaker(person_index);
        // = seg000:93dc jmp menu_callback_choice_talk_to_me — run the dialogue.
        // Its tail reveals the staged verb panel (play_pending_panel_fold) when
        // a line was presented, or pops it (menu_callback_choice_exit_menu)
        // when the speaker has nothing to say.
        self.menu_callback_choice_talk_to_me();
    }

    // = seg000:d280 play_pending_panel_fold — when a verb-panel transition is pending (screen_overlay_request_transition
    // armed in_transition to a small positive), reveal the fb1-staged panel with
    // the 17-frame accordion fold (panel_anim) and clear the flag. The dialogue
    // paths jmp here (talk_to_me 94da, come_with_me/stay_here 9652). in_transition
    // < 0 (0x80, a full-screen transition already underway) or 0 takes no action.
    //
    // Each fold frame is paced to 6 PIT ticks, and lip_sync_frame_task runs during
    // that wait so the speaker's mouth keeps moving through the reveal.
    pub(crate) fn play_pending_panel_fold(&mut self) {
        // = seg000:d280 cmp in_transition,0; jle ret. <=0 (idle, or a full-screen
        //   transition already underway) takes no action.
        if (self.in_transition as i8) <= 0 {
            return;
        }
        // = seg000:d28a in_transition = 0 — consume the pending flag now.
        self.in_transition = 0;
        // = seg000:d28f call ui_hud_open_hands — close the ICONES doors over the
        //   panel sides (ui_elements[1]/[2].sprite_id -> %3 == 2) before folding.
        self.ui_hud_open_hands();

        // = seg000:d292 mov cx,0x11; d2b4 loop loc_0d295 — 17 fold frames, cx 0x11..1.
        for frame in (1..=17u16).rev() {
            // = seg000:d296 push pit_timer — the step-start tick, captured BEFORE the
            //   fold blit so the d2a4 pace below spans the whole step.
            let start = self.game_ticks();

            // = seg000:d29a..d2a0 si=framebuffer_saved_seg; al=0x18; call
            //   blit_fb1_to_screen_effect — render one fold frame straight to screen.
            gfx::panel_anim_play_step(self, frame);
            self.send_frame_to_display();

            // = seg000:d2a4 loc_0d2a4 — keep the speaker's mouth moving by re-running
            //   lip_sync_frame_task until 6 PIT ticks have elapsed since `start`.
            loop {
                // = seg000:d2a5 call lip_sync_frame_task — advance the talking-head
                //   voice mouth one step (a no-op once the voice has drained).
                self.tick_talking_head_voc();
                // = seg000:d2a9 mov ax,pit_timer; sub ax,bx; cmp ax,6; jb loc_0d2a4.
                if self.game_ticks().saturating_sub(start) >= 6 {
                    break;
                }
                // DOS busy-spins re-calling lip_sync_frame_task; the port yields one
                // PIT tick between calls so the game thread does not spin-wait.
                let now = self.game_ticks();
                self.sleep_ticks(now, 1);
            }
        }

        // = seg000:d2b6 call ui_hud_close_hands — reopen the doors (%3 == 0),
        //   revealing the new panel and restoring the date/time indicator.
        self.ui_hud_close_hands();
    }

    // = seg000:93df set_dialogue_speaker — record the active dialogue speaker:
    // mark them as met and as the current conversation partner, prime the dialogue
    // sentence cursor + verb mask, and push the per-NPC dialogue verb panel.
    pub(crate) fn set_dialogue_speaker(&mut self, person_index: u8) {
        // = seg000:93e1 data_047be = person_index << 3 — the dialogue sentence
        // cursor base (menu_callback_choice_talk_to_me indexes the record table by
        // it: person*8 + topic).
        self.dialogue_topic_index = (person_index as u16) << 3;
        // = seg000:93ea ax = 1 << person_index.
        let bit = 1u16 << person_index;
        // = seg000:93ef or [persons_met], ax.
        self.persons_met |= bit;
        // = seg000:93f3 or [persons_talking_to], ax.
        self.persons_talking_to |= bit;
        // = seg000:93f7 data_047a2 = &room_persons[person_index] — the active-
        // speaker pointer; only the unported loc_094f3 (the for_condit_ds_16
        // timestamp seed) reads it. setup_npc_dialogue_menu takes the index
        // directly.
        // = seg000:9403 dialogue_resume_entry_ptr = 0 — start the talk walk at
        // the topic cursor, not inside a previous record.
        self.dialogue_resume_entry_ptr = 0;
        // = seg000:9409 call setup_npc_dialogue_menu — select the per-NPC verb and
        // push the dialogue verb panel.
        self.setup_npc_dialogue_menu(person_index);
        // = seg000:940c dialogue_text_continuation_ptr = 0 — drop any pending
        // multi-part subtitle continuation.
        self.dialogue_text_continuation_ptr = 0;
        // = seg000:9412 data_047c2 = 0x80 — prime the verb-panel sentence mask
        // dialogue_interpret_record applies to each sentence's flag byte.
        self.data_047c2 = 0x80;
        // = seg000:9417 data_00019 = 0 — write-only in the binary (set to 0xff at
        // seg000:a092, never read); not modelled.
    }

    // = seg000:9f40 loc_09f40 — per-presentation setup shared by the talk verb
    // (seg000:9472) and the auto-dialogue present chain (seg000:9713).
    fn prepare_dialogue_presentation(&mut self) {
        // = seg000:9f43..9f51 — current_lip_sync_resource_id == 2 (Stilgar)
        //   during final-attack stage 4 (final_attack_stage_ds_c2 == 4) re-runs
        //   increase_final_attack_stage_if_more_than_10K_Fremen_near_Harkonnen_
        //   palace; the final-attack model is unported.
        // = seg000:9f56 data_047a2 = &room_persons[id] — the active-speaker
        //   entry pointer; only the unported loc_094f3 reads it.
        // = seg000:9f60 cmp data_046eb,0; jnz loc_09f82 — in the room view,
        //   draws target fb1 and the subtitle box gets the in-room pads.
        if self.data_046eb == 0 {
            // = seg000:9f67 call set_fb1_as_active_framebuffer.
            self.set_fb1_as_active_framebuffer();
            // = seg000:9f6a..9f7c subtitle_pad_left/right/top/bottom =
            //   0x28/0x10/0x10/0x10 — layout for draw_subtitle_body (the
            //   subtitle text engine, unported).
        }
        // = seg000:9f82 loc_09f82 font_draw_fg_color = 0xf0 + font_select_tall_
        //   font — subtitle font setup (text engine unported).
    }

    // = seg000:9472 menu_callback_choice_talk_to_me — present one dialogue line:
    // resume inside the current record (dialogue_resume_entry_ptr) or walk the
    // speaker's topic records (data_047be cursor, person*8 + 0..3) and present
    // the first condition-matching sentence (present_first_matching_dialogue_
    // line, 9f9e). Only ONE sentence is presented per talk action; a presented
    // line reveals the staged verb panel (94da jmp play_pending_panel_fold), an
    // exhausted walk pops it (94c0 jmp menu_callback_choice_exit_menu).
    pub(crate) fn menu_callback_choice_talk_to_me(&mut self) {
        // = seg000:9472 call loc_09f40.
        self.prepare_dialogue_presentation();
        // = seg000:9475 data_0226d = 0x0a and 947a data_0001b = 0 — not modelled.
        // = seg000:947f cmp dialogue_text_continuation_ptr,0; jnz loc_094dd — a
        //   pending multi-part subtitle continuation is re-presented (loc_088d2,
        //   current_subtitle_id += 0x1000) and its events re-fired instead of
        //   walking a new line. The text engine that arms the pointer is
        //   unported, so the branch never runs.

        // = seg000:9486 si = dialogue_resume_entry_ptr — resume inside the
        //   current record; 948e zero -> start at the data_047be topic cursor's
        //   record (loc_09492: si = [cursor*2 - 558ah]).
        let mut ofs = self.dialogue_resume_entry_ptr;
        if ofs == 0 {
            ofs = container::entry_offset(&self.dialogue, self.dialogue_topic_index);
        }

        println!(
            "\n\nmenu_callback_choice_talk_to_me: starting at ofs {ofs:#06x} (cursor {:#04x})",
            self.dialogue_topic_index
        );

        // The person-0xd restart (loc_094cc) re-enters the topic walk with the
        // auto mask; if nothing matches then either, DOS would loop forever, so
        // the port latches the restart to one attempt.
        let mut retried_with_auto_mask = false;
        loop {
            // = seg000:949a cmp si,0ffffh; jz loc_094b9 — empty slot / ended record.
            if ofs != 0xffff {
                // = seg000:949f call loc_09b49 — the data_047e1-gated portrait
                //   part-2 animation wait; data_047e1 (armed by the per-character
                //   trampolines, seg000:93ac) is not modelled.
                // = seg000:94a2 call present_first_matching_dialogue_line.
                let (next, presented) = self.present_first_matching_dialogue_line(ofs as usize);

                println!(
                    "menu_callback_choice_talk_to_me: present_first_matching_dialogue_line({ofs:#06x}) -> next {next:#06x}, presented {presented}",
                );

                // = seg000:94a5 dialogue_resume_entry_ptr = si — the next TALK TO
                //   ME continues from the entry after the presented one.
                self.dialogue_resume_entry_ptr = next;
                // = seg000:94a9 jnb loc_094da — a line was presented: reveal the
                //   staged verb panel and stop.
                if presented {
                    self.play_pending_panel_fold();
                    return;
                }
                // = seg000:94ab..94b7 — advance the topic cursor; while it stays
                //   inside the person's 4 talk topics (& 3 != 0), walk the next
                //   record (loc_09492).
                self.dialogue_topic_index = self.dialogue_topic_index.wrapping_add(1);
                ofs = self.dialogue_topic_index;
                if ofs & 3 != 0 {
                    ofs = container::entry_offset(&self.dialogue, self.dialogue_topic_index);
                    continue;
                }
            }
            // = seg000:94b9 loc_094b9 — topics exhausted (or the resume pointer
            //   hit the record end).
            if self.current_lip_sync_resource_id != 0x0d || retried_with_auto_mask {
                // = seg000:94c0 jmp menu_callback_choice_exit_menu — nothing to
                //   say: pop the dialogue verb panel.
                self.menu_callback_choice_exit_menu();
                return;
            }
            // = seg000:94c3..94d8 — the special-room person (0xd): restart at
            //   the person's topic 0 with the auto mask and walk again.
            retried_with_auto_mask = true;
            // = seg000:94c3 cmp si,0ffffh; jnz loc_094cc; 94c8 si = data_047be.
            if ofs == 0xffff {
                ofs = self.dialogue_topic_index;
            }
            // = seg000:94cc and si,0fff8h; data_047be = si; data_047c2 = 0x20.
            ofs &= 0xfff8;
            self.dialogue_topic_index = ofs;
            self.data_047c2 = 0x20;
            // = seg000:94d8 jmp loc_09492 — the record-table lookup.
            ofs = container::entry_offset(&self.dialogue, ofs);
        }
    }

    // = seg000:88af show_voice_subtitle (reached at loc_0a034) — record the
    // matched phrase id as the current subtitle. DOS also resolves the phrase
    // string and draws the subtitle bubble (get_phrase_or_command_string ->
    // expand_phrase_tokens -> format_interpolated_string -> the loc_08b11 draw);
    // that text rendering is unported, so this only stores the id. DOS does this
    // BEFORE firing the spoken-line event (loc_0a03f), so the event sees it set.
    pub(crate) fn show_voice_subtitle(&mut self, phrase_id: u16) {
        // = seg000:88af or ax,ax; jz — a zero string id is a no-op. Phrase ids are
        //   always phrase-marked (>= 0x800), so a zero here means an upstream bug.
        if phrase_id == 0 {
            eprintln!("show_voice_subtitle: ignoring unexpected zero phrase id");
            return;
        }
        self.current_subtitle_id = phrase_id;
    }

    // = loc_0a0c9 -> loc_09efd — load and play the current subtitle line's voice
    // `.voc` over the lip-sync engine. Reads current_subtitle_id, which
    // show_voice_subtitle set. DOS runs this AFTER the spoken-line event fires.
    pub(crate) fn play_dialogue_voc(&mut self) {
        // = seg000:9efd data_047dd = data_047dc (the come-with-me voc-bank flag,
        //   armed at seg000:95b7/96db — unmodelled, reads 0); ax =
        //   current_subtitle_id; bx = current_lip_sync_resource_id; call
        //   load_voc_and_lipsync_data (a6cc). Its index transform:
        // = seg000:a6e7 bl = min(speaker, 0x0e) — the voc directory id;
        // = seg000:a6ee ah &= 0xf3 — strip the phrase-marker bits.
        let dir_id = self.current_lip_sync_resource_id.min(0x0e);
        let mut voc_index = self.current_subtitle_id & 0xf3ff;
        // = seg000:a6f1 data_047dc != 0 -> ax = ax - [data_0d814] + 0x3e7 — the
        //   come-with-me voc bank; unmodelled (data_047dc is always 0 here).
        // = seg000:a701 cmp suppress_sky_240_255,0; jnz — HNM/cutscene contexts
        //   skip the per-person rebase.
        if self.data_0227d == 0 {
            // = seg000:a708 sub ax,[bx*2 - 280ch] — rebase the global phrase
            //   index onto the speaker's 001-based P<X>\ voc numbering (the
            //   per_person_voc_base_table built at startup by seg000:cfb9).
            //   Leto's base is 0 (his first phrase index is 1); Jessica's is
            //   0x31, so her first line (phrase 0x836) plays PB005, not PB036.
            // if let Some(records) = self.dialogue_records.as_ref() {
            voc_index = voc_index.wrapping_sub(self.voc_base(dir_id));
            // }
        }
        // = seg000:a710..a726 — the dir_id == 0x0e troop special (voc index
        //   0x2c/0x2d retargets the lip-sync id to 0x0c) is not modelled.

        // = create_voc_file_name_from_bx suffix (a8e1..a8fa): 'I' in the early-game
        // special-room context, else 'O'.
        let suffix = if self.data_000ea <= 0
            && (self.location_appearance & 0xff) == 0x80
            && (self.location_and_room & 0xff) != 1
        {
            'I'
        } else {
            'O'
        };

        // = loc_0a0c9 -> loc_09efd: load and play the voice .voc + lip-sync.
        self.play_talking_head_voc(voc_index, suffix);
    }

    // = seg000:96f1 present_room_person_dialogue -> loc_09702 -> loc_0970b ->
    // present_dialogue_line_with_auto_mask (loc_09f8b) — present a standing
    // room-person's auto-dialogue line. room_person_present_auto_dialogue
    // reaches here during the room-leave scan: the person's topic-4 record
    // (loc_09702 forces topic 4 via `or ax,4`) is walked with the verb mask
    // 0x20, and on a condition match present_first_matching_dialogue_line shows
    // the talking head over the zoomed room, fires the line's event callback,
    // and plays the voice. For Duke Leto in the early game this selects phrase
    // 0x81f ("Where are you going so fast? I have to talk to you.") whose
    // stay_here event interrupts the move.
    //
    // Returns whether a line was presented — DOS signals this with the carry
    // flag, which room_person_present_auto_dialogue tests at seg000:3531 (`jnb`)
    // to decide whether to install the dialogue verb menu.
    pub(crate) fn present_room_person_line(&mut self, person_index: u8) -> bool {
        // = seg000:96f1 mov [_word_23C74_current_lip_sync_resource_id], ax — the
        //   lip-sync resource id is the person index. (seg000:96f4's al == 0x0e
        //   troop special-case — troop_prepare_troop_data_for_condit on the
        //   data_04756 troop — is not modelled.)
        self.current_lip_sync_resource_id = person_index as u16;

        // = seg000:9702 ax = person*8 | 4.
        let ofs = container::entry_offset(&self.dialogue, ((person_index as u16) << 3) + 4);
        if ofs == 0xffff {
            return false;
        }
        // = seg000:9713 call loc_09f40.
        self.prepare_dialogue_presentation();
        // = seg000:9716 jmp present_dialogue_line_with_auto_mask (seg000:9f8b) —
        //   present with the verb mask data_047c2 forced to 0x20, preserving the
        //   caller's mask around the call.
        let saved_mask = self.data_047c2;
        self.data_047c2 = 0x20;
        let (_, presented) = self.present_first_matching_dialogue_line(ofs as usize);
        self.data_047c2 = saved_mask;
        presented
    }

    // = seg000:9f9e present_first_matching_dialogue_line — walk the dialogue
    // record's sentence entries from absolute offset `start` and present the
    // first entry whose condition holds: show the talking head (loc_09fd8),
    // record the subtitle, then fall through into fire_event_callbacks_from_
    // spoken_dialogue_lines_and_more (event callback + spoken mark + voice).
    //
    // Returns DOS's (si, !carry) exit: `(_, false)` when no entry matched (si at
    // the terminator), `(next, true)` after presenting a line, with `next` the
    // entry after the presented one (or 0xffff when dialogue_end_request fired)
    // — the talk verb stores it as dialogue_resume_entry_ptr.
    pub(crate) fn present_first_matching_dialogue_line(&mut self, start: usize) -> (u16, bool) {
        // = seg000:9f9e mov [dialogue_current_record_ptr], si — the phrase-bank
        //   selector load_PHRASExx_HSQ (seg000:d00f) consults.
        self.dialogue_current_record_ptr = start as u16;
        // = seg000:9fa2 call loc_094f3 — seed for_condit_ds_16 with game_time
        //   minus the speaker's room-person timestamp (entry word +8 or +0xa) and
        //   run the illness-location menu-origin hook. The room-person runtime
        //   timestamps are not modelled, so time-gated conditions read ds:0x16
        //   as 0. TODO when the room-person runtime fields land.
        // = seg000:9fa5 data_047bc = 0xa6b0 — reset the subtitle string-buffer
        //   write cursor; condition evaluation can leave override text there
        //   (the seg000:a005..a02c draw_subtitle_body path). Text engine
        //   unported, so the cursor never moves and loc_0a034 is always taken.

        // = the verb-panel sentence mask; the per-entry condition evaluation
        //   reads its memory operands straight off the live game state
        //   (GameState::condition_holds).
        let mask = self.data_047c2;
        let selected = {
            // let Some(records) = self.dialogue_records.as_ref() else {
            //     return (0xffff, false);
            // };
            // = seg000:9fab..9fd6 the entry walk (loc_09fab).
            self.interpret_record(start, mask)
        };
        let line = match selected {
            // = seg000:9f9c stc; ret — no condition matched; si is left at the
            //   record terminator.
            Err(terminator) => return (terminator as u16, false),
            Ok(line) => line,
        };

        // = seg000:9fd8 loc_09fd8 — show the talking head, only for a real room
        //   speaker: data_046eb == 0 (room view) and resource id < 0x10.
        if self.data_046eb == 0 && self.current_lip_sync_resource_id < 0x10 {
            // = seg000:9fe9 call adjust_subtitle_mode_for_dialogue_line (a0f1).
            self.adjust_subtitle_mode_for_dialogue_line(line.entry2);
            // = seg000:9fec call ui_hud_head_animate_down — fold the small HUD
            //   head ornament out of view
            self.ui_hud_head_animate_down();
            // = seg000:9fef call loc_03af9 zoom_room_to_dialogue_speaker.
            self.dialogue_zoom_room();
            // = seg000:9ff3 call setup_lip_sync_data_from_sprite_sheet (91a0) —
            //   open + parse the speaker's portrait sheet. The port's
            //   setup_talking_head bundles that with the backdrop save, the
            //   first idle render and the idle-task install that DOS performs
            //   later via start_room_lip_sync (seg000:a0b9).
            self.setup_talking_head(self.current_lip_sync_resource_id as u8, 0);
        }

        // = seg000:9ff7 lodsw — dialogue_line_word0 = the entry's first word
        //   (the voc-replay / multi-part flags the subtitle engine reads).
        self.dialogue_line_word0 = line.word0;
        // = seg000:9ffc..a002 the phrase id (already extracted by the walk).
        // = seg000:a005..a02c — when condition evaluation left override text at
        //   0xa6b0 (data_047bc moved), format + draw it via draw_subtitle_body;
        //   unported (see above), so the port always takes loc_0a034.
        // = seg000:a034 cmp data_000c6,0; jnz — a suppressed presentation skips
        //   the subtitle.
        if self.data_000c6 == 0 {
            // = seg000:a03b call show_voice_subtitle.
            self.show_voice_subtitle(line.phrase);
        }
        // = seg000:a03e falls through into fire_event_callbacks_from_spoken_
        //   dialogue_lines_and_more — event callback, spoken mark, head present
        //   and voice; carry-clear: a line was presented.
        let next = self.fire_dialogue_line_event(line.entry_offset);
        (next, true)
    }

    // = seg000:a0f1 adjust_subtitle_mode_for_dialogue_line — in
    // voice_subtitle_mode 2 only, the selected sentence entry decides the mode
    // for this line. `entry2` is the entry's third byte.
    fn adjust_subtitle_mode_for_dialogue_line(&mut self, entry2: u8) {
        // = seg000:a0f1 cmp voice_subtitle_mode,2; jnz ret.
        if self.voice_subtitle_mode != 2 {
            return;
        }
        if entry2 & 0x10 != 0 {
            // = seg000:a0fe — the voiced-line flag: voice_subtitle_mode = 1.
            self.voice_subtitle_mode = 1;
        }
        // = seg000:a104 jmp subtitle_restore_prior — an unvoiced line restores
        //   the prior subtitle layout. TODO: subtitle_restore_prior is unported
        //   (subtitle text engine).
    }

    // = seg000:c85b arm_npc_menu_idle_timer — (re)arm the NPC-actions-menu
    // inactivity timer: base = the PIT counter now, limit = 0x1770 (6000 ticks,
    // 30 s). The room mouse hook loc_01ae7 (seg000:1ae7, unported) watches the
    // pair while menu_NPC_actions is the active screen element and fires
    // loc_0c868 on expiry.
    fn arm_npc_menu_idle_timer(&mut self) {
        self.npc_menu_idle_timer_base = self.game_ticks() as u16;
        self.npc_menu_idle_timer_limit = 0x1770;
    }

    // = seg000:a03f fire_event_callbacks_from_spoken_dialogue_lines_and_more —
    // the tail of present_first_matching_dialogue_line (which falls through into
    // it at a03e) and of the talk verb's multi-part continuation (seg000:94ee):
    // re-arm the NPC-menu idle timer, dispatch the spoken line's event callback,
    // append the line to the dialogue-played log, mark the sentence entry (at
    // `entry_offset`, absolute within the DIALOGUE buffer) spoken, then present
    // the head and start the voice.
    //
    // Returns DOS's si exit: the entry after the spoken one, or 0xffff when
    // dialogue_end_request (event 0x06) fired — the talk verb's resume pointer.
    pub(crate) fn fire_dialogue_line_event(&mut self, entry_offset: usize) -> u16 {
        // = seg000:a03f call arm_npc_menu_idle_timer (loc_0c85b).
        self.arm_npc_menu_idle_timer();

        let mut si = entry_offset as u16;
        // = seg000:a042 cmp dialogue_text_continuation_ptr,0; jnz loc_0a0aa — a
        //   pending multi-part continuation already fired its event and spoken
        //   mark on the line's first part; skip straight to the present tail.
        if self.dialogue_text_continuation_ptr == 0 {
            // let b0: u8;
            // let b2: u8;
            // todo!();
            let (b0, b2) = (
                // todo()
                // self.byte(entry_offset).unwrap_or(0),
                // self.byte(entry_offset + 2).unwrap_or(0),
                self.dialogue[entry_offset],
                self.dialogue[entry_offset + 2],
            );
            // = seg000:a049..a05d — dispatch the event callback (al = [si] &
            //   0x0f; 0 = none) via the table at seg000:a107.
            let event = b0 & 0x0f;
            if event != 0 {
                println!(
                    "fire_dialogue_line_event: dispatching event {event:#04x} for entry {entry_offset:#06x}\n"
                );
                self.dispatch_dialogue_line_event(event, b0);
            }
            // = seg000:a05e..a08d — append the line to the dialogue-played log
            //   when it is replayable (entry byte 2 has a replay flag, bits
            //   0x0c) and not yet spoken (word0 bit 0x80 clear): the packed word
            //   is the entry's index among the buffer's 4-byte entries
            //   (ax = (si - 0aa78h) >> 2, i.e. (offset - 2) / 4 past the table's
            //   leading length word) with the speaker in bits 11.. (bl =
            //   lip_sync_id << 3, or'ed into ah). DOS stores it at
            //   cs:[dialogue_played_log_head] and re-terminates with a 0 word.
            if b2 & 0x0c != 0 && b0 & 0x80 == 0 {
                let packed =
                    (((entry_offset - 2) >> 2) as u16) | (self.current_lip_sync_resource_id << 11);
                self.dialogue_played_log.push(packed);
            }
            // = seg000:a092 data_00019 = 0xff — write-only in the binary (only
            //   set_dialogue_speaker clears it back); not modelled.
            // = seg000:a097 or byte [si], 0x80 — mark the entry spoken (so a
            //   later verb-panel walk's mask skips it and the replay log does
            //   not re-add it).
            self.mark_spoken(entry_offset);
            // = seg000:a09a add si,4 — the talk verb resumes after this entry.
            si = (entry_offset + 4) as u16;
            // = seg000:a09d..a0a7 — consume dialogue_end_request (event 0x06):
            //   xchg with 0; nonzero forces si = 0xffff, ending the record.
            if std::mem::take(&mut self.dialogue_end_request) != 0 {
                si = 0xffff;
            }
        }

        // = seg000:a0aa loc_0a0aa — present the head for a real room speaker
        //   (the same data_046eb == 0 / id < 0x10 gate as loc_09fd8).
        if self.data_046eb == 0 && self.current_lip_sync_resource_id < 0x10 {
            // = seg000:a0b9 call start_room_lip_sync (978e) — sheet parse, idle
            //   task and first head render (already bundled into the port's
            //   setup_talking_head); mirror its visible tail: 97c8 call
            //   update_screen_palette, 97cb jmp loc_0c4dd.
            self.update_screen_palette();
            self.present_dialogue_head();
            // = seg000:a0bd cmp data_04774,0; jnz -> a0c5 call loc_02ebf — while
            //   a dialogue is active, push the screen element at [data_02220].
            //   TODO: that element (the dialogue text window) is not modelled.
        }
        // = seg000:a0c9 loc_0a0c9 — start the voice unless suppressed.
        if self.data_000ea <= 0 {
            // = seg000:a0d0 save_regs; a0d3 call loc_09efd — load and play the
            //   subtitle line's .voc + lip-sync.
            self.play_dialogue_voc();
            // = seg000:a0d6..a0dd — run the one-shot post-voice hook: ax =
            //   nullsub_00f66; xchg ax,[data_0227e]; call ax. Only the unported
            //   event-0x08 Stilgar branch (seg000:a13a) arms it, so it is
            //   always the nullsub here; not modelled.
        }
        // = seg000:a0e2 loc_0a0e2 — in the room view (room_view_toggle >= 0),
        //   restore the default voice/subtitle mode for the next line.
        if (self.room_view_toggle as i8) >= 0 {
            self.voice_subtitle_mode = self.voice_subtitle_mode_default;
        }
        // = seg000:a0ef clc; ret.
        si
    }

    // = seg000:a049..a05d + the callback table array_ptrs_callback_for_event_
    // fired_by_speaking_dialogue_line (seg000:a107) — dispatch one spoken-line
    // event. `word0_lo` is the entry's flag byte BEFORE the spoken mark, so the
    // first-time-only callbacks (0x0b/0x0c/0x0e test `[si], 0x80`) can check it.
    fn dispatch_dialogue_line_event(&mut self, event: u8, word0_lo: u8) {
        match event {
            // = seg000:a1d0 callback_event_dialogue_line_01_follow_me.
            1 => self.dialogue_interrupt_gate = 0xff,
            // = seg000:a1d6 callback_event_dialogue_line_02_stay_here.
            2 => self.dialogue_interrupt_gate = 0,
            // = seg000:a1e8 callback_event_dialogue_line_06_end_dialogue —
            //   request the end of the talk walk (consumed at seg000:a09d).
            6 => self.dialogue_end_request = self.dialogue_end_request.wrapping_add(1),
            // = seg000:a1dc callback_event_dialogue_line_07_show_equipment_in_map.
            7 => self.dialogue_interrupt_gate = 0x80,
            // = seg000:a219 callback_event_dialogue_line_0b_increase_game_phase_
            //   by_1_and_do_more — first time only (the spoken bit gates repeats).
            0x0b if word0_lo & 0x80 == 0 => {
                self.game_phase = self.game_phase.wrapping_add(1);
                // = seg000:a222 data_000ff = 0; a227 call loc_0b17a; a22a
                //   game_phase == 1 -> make Duncan Idaho visible (seg000:100b) —
                //   none of that state is modelled yet.
                println!(
                    "dialogue event 0x0b: game_phase -> 0x{:02x} (loc_0b17a tail unported)",
                    self.game_phase
                );
            }
            // = seg000:a235 callback_event_dialogue_line_0c_increase_game_phase_
            //   by_4_if_dialogue_bit_set — first time only.
            0x0c if word0_lo & 0x80 == 0 => {
                // = seg000:a23a..a241 set_game_phase_and_trigger_callbacks(
                //   (game_phase & 0xfc) + 4); the phase-change trigger chain
                //   (seg000:121f) is unported.
                self.game_phase = (self.game_phase & 0xfc).wrapping_add(4);
                println!(
                    "dialogue event 0x0c: game_phase -> 0x{:02x} (set_game_phase triggers unported)",
                    self.game_phase
                );
            }
            // = the already-spoken no-ops of 0x0b/0x0c (test [si],80h; jnz ret).
            0x0b | 0x0c => {}
            // = seg000:a1f7 (0x03) trigger_cutscenes, a244/a248 (0x04/0x05) the
            //   accept/refuse/argue menu, a1ed (0x0e) increase_final_attack_stage
            //   (ds:c2 not modelled), a125/a157/a172 (0x08/0x09/0x0f) the
            //   speaker-dependent effects, a25b (0x0a) the bubble-layout tweak,
            //   a28e (0x0d) the command-menu/PALPLAN redraw — all unported.
            _ => println!("dispatch_dialogue_line_event: unported event 0x{event:02x}"),
        }
    }

    // = seg000:98b2 tear_down_prior_talking_head_overlay — before a new dialogue
    // line (the room-leave scan at seg000:36da, the worm/ornithopter verbs, the
    // portrait reload at 91c2), tear down a prior talking-head overlay so the
    // new head does not composite over a stale one.
    pub(crate) fn tear_down_prior_talking_head_overlay(&mut self) {
        // = seg000:98b2 cmp data_047c3,0; jnz ret — the bubble/no-head subtitle
        //   presenter owns the overlay (armed at seg000:0ebb/0f35); that
        //   presenter is unported, so the gate always passes.
        // = seg000:98b9.._word_239F0_copy_of_non_pcm_lip_sync_data = 0 and
        //   data_047d1 &= 0x3f — pending lip-sync frame state; the port keeps
        //   the equivalents inside TalkingHead.
        // = seg000:98c3 xchg ax,[data_047c8]; jz ret — consume the head-overlay
        //   element pointer (seg001:1bf0, armed by the head render at
        //   seg000:992a/99fa); zero means no overlay is up. The port's
        //   equivalent of a live overlay is the TalkingHead itself.
        if self.talking_head.is_none() {
            return;
        }
        // = seg000:98cb..98d3 si = 1bf0h; [si+8] = 0; ui_hud_elements[20].flags
        //   = 0 — retire the overlay's screen-element entries.
        self.ui_elements[19].flags = 0;
        self.ui_elements[20].flags = 0;
        // = seg000:98d9 call copy_rect_fb2_to_fb1 — restore the game area under
        //   the head from the clean backdrop. DOS restores just the overlay's
        //   rect (the element-19 rect); the port restores the whole game area, a
        //   clean superset of it.
        self.copy_game_area_fb2_to_fb1();
        // = seg000:98dc..98df si = 1bf0h; call draw_hud_head_if_needed_and_
        //   update_screen_rect_at_si — push the restored area to the screen
        //   (present_dialogue_head pushes the game-area rect through the same
        //   c4f0 chain).
        self.present_dialogue_head();
        // = seg000:98e2 jmp stop_lip_sync_and_remove_idle_head_task (loc_09b8b).
        self.stop_lip_sync_and_remove_idle_head_task();
    }

    // = seg000:c4dd loc_0c4dd — present the freshly-composited talking head and
    // its zoomed backdrop to the visible screen.
    pub(crate) fn present_dialogue_head(&mut self) {
        // = seg000:c4dd cmp mouse_pos_y,98h; jnb +; call call_restore_cursor —
        // repaint the saved background under the cursor when it sits in the game
        // area, so a stale cursor image is not baked into the pushed rect.
        if self.mouse_pos_y < 0x98 {
            self.restore_cursor_over_panel();
        }
        // = seg000:c4e8 si = _word_20920_game_area_rect (0,0,320,152); jmp
        // present_screen_rect.
        let yoff = self.y_offset as i16;
        self.present_screen_rect(Rect {
            x0: 0,
            y0: yoff,
            x1: 320,
            y1: yoff + 152,
        });
    }

    // = seg000:c4f0 present_screen_rect — the
    // tail of the head-presentation chain (present_dialogue_head jumps here, as
    // does the settings-panel repaint). Redraw the HUD head into fb1 when `rect`
    // overlaps the head box (c4fb), then push `rect` from fb1 to the visible
    // screen (copy_rect_fb1_to_screen).
    pub(crate) fn present_screen_rect(&mut self, rect: Rect) {
        // = seg000:c4fb draw_hud_head_if_needed_and_update_screen_rect — redraw
        // the HUD head when the 240..255 sky is not suppressed and `rect` overlaps
        // the head box (x in [0x7e,0xc2), bottom edge >= 0x89). The head must land
        // in fb1 so the copy below carries it, so force fb1 active around the draw
        // (DOS's callers already have fb1 active here).
        if self.data_0227d == 0 && rect.y1 >= 137 && rect.x1 >= 126 && rect.x0 < 194 {
            let saved = self.active_fb();
            self.set_fb1_as_active_framebuffer();
            self.ui_hud_head_draw();
            self.active_fb = saved;
        }
        // = seg000:c4fb falls through into c51e.
        self.copy_rect_fb1_to_screen(rect);
    }

    // = seg000:c51e copy_rect_fb1_to_screen — copy `rect` from fb1 to the
    // visible screen. Called on its own (e.g. seg000:c7cc) as well as via the
    // present_screen_rect fall-through. An empty rect does nothing; the copy is
    // skipped while the front buffer is redirected to fb1 (offscreen render,
    // where DOS's copy targets fb1 and the real screen must stay untouched) or
    // the mixer panel owns the mouse handlers (loc_0c526).
    pub(crate) fn copy_rect_fb1_to_screen(&mut self, rect: Rect) {
        // = seg000:c51e sub bp,dx / sub ax,bx — bail on a zero-area rect.
        if rect.x1 <= rect.x0 || rect.y1 <= rect.y0 {
            return;
        }
        // = seg000:c526 cmp active_mouse_handlers,1ad6h; jz ret.
        if self.front_buffer_is_fb1()
            || std::ptr::eq(
                self.active_mouse_handlers,
                &crate::game_ui::MIXER_MOUSE_HANDLERS,
            )
        {
            return;
        }
        gfx::vga_copy_rect(&mut self.screen, &self.framebuffer, rect);
        self.send_frame_to_display();
    }
}
