//! The scripted continue-sequence — the DOS engine at seg000:1707..1774 that
//! plays a multi-line scene one " Continue…" click at a time. A byte script
//! (in cs, one action byte per step, 0xff ending it) is installed by
//! start_scripted_dialogue; each click reads the next byte and dispatches it
//! through array_callbacks_actions_in_continue_sequence.
//!
//! The path this drives today is the prospector troop's spice-map scene:
//! SPECIALIZE IN SPICE on troops[2] presents a line whose event 0x03
//! (callback_event_dialogue_line_03_trigger_cutscenes) installs the
//! phase-appropriate script and puts the " Continue…" panel up; below game
//! phase 0x14 that script is [0x0e, 0x10, 0xff] — action 7 (raise the
//! spice-density overlay, then speak "Here, take this map of the planet."),
//! action 8 (drop the overlay, then speak "You can update this map…"), end.
//!
//! GOTCHA: the script byte is a BYTE OFFSET into the action table, not an
//! index — 0x0e selects entry 7 and 0x10 entry 8 (seg000:172f `mov bx,ax`
//! then `jmp [array + bx]`).

use crate::{GameState, TaskId, menu_defs::MenuRef};

const SCRIPT_SHOW_SPICE_MAP: u8 = 0x0e; // action 7
const SCRIPT_HIDE_SPICE_MAP: u8 = 0x10; // action 8
const SCRIPT_END: u8 = 0xff; // end of script

// = seg000:12f8 cutscene_game_phase_below_14_dialogue.
#[rustfmt::skip]
static SCRIPT_PROSPECTOR_INTRO: [u8; 3] = [
    SCRIPT_SHOW_SPICE_MAP,
    SCRIPT_HIDE_SPICE_MAP,
    SCRIPT_END
];

// = seg000:134f cutscene_game_phase_14_dialogue.
#[rustfmt::skip]
static SCRIPT_PHASE_14: [u8; 33] = [
    0x02, 0x01, 0x02, 0x00, 0x02, 0x01, 0x04, 0x08, 0x02, 0x00, 0x00, 0x05, 0x04, 0x00, 0x01,
    0x03, 0x08, 0x08, 0x02, 0x00, 0x02, 0x03, 0x02, 0x00, 0x02, 0x08, 0x04, 0x08, 0x02, 0x00,
    0x02, 0x00, 0xff,
];

// = seg000:1370 cutscene_game_phase_18_dialogue.
#[rustfmt::skip]
static SCRIPT_PHASE_18: [u8; 34] = [
    0x02, 0x01, 0x00, 0x0a, 0x03, 0x01, 0x05, 0x00, 0x08, 0x02, 0x05, 0x02, 0x05, 0x02, 0x00,
    0x02, 0x05, 0x04, 0x08, 0x02, 0x05, 0x02, 0x01, 0x00, 0x0a, 0x02, 0x01, 0x05, 0x06, 0x00,
    0x08, 0x02, 0x00, 0xff,
];

// = seg000:12db cutscene_game_phase_30_dialogue.
#[rustfmt::skip]
static SCRIPT_PHASE_30: [u8; 29] = [
    0x00, 0x02, 0x05, 0x01, 0x02, 0x04, 0x05, 0x07, 0x02, 0x02, 0x02, 0x01, 0x02, 0x02, 0x02,
    0x07, 0x02, 0x05, 0x04, 0x08, 0x02, 0x04, 0x02, 0x02, 0x02, 0x01, 0x02, 0x02, 0xff,
];

impl GameState {
    // = seg000:a1f7 callback_event_dialogue_line_03_trigger_cutscenes — the
    // spoken-line event that starts a scripted scene: the script is picked by
    // game_phase (below 0x14 / 0x14 / 0x18 / 0x30 and up).
    pub(crate) fn dialogue_event_trigger_cutscene(&mut self) {
        println!(
            "continue-sequence: starting scripted scene for game phase {:#04x}",
            self.game_phase
        );
        let script: &'static [u8] = match self.game_phase {
            // = seg000:a1fb/a1fe.
            p if p < 20 => &SCRIPT_PROSPECTOR_INTRO,
            // = seg000:a203/a206.
            p if p < 0x18 => &SCRIPT_PHASE_14,
            // = seg000:a20b/a20e.
            p if p < 0x30 => &SCRIPT_PHASE_18,
            // = seg000:a213.
            _ => &SCRIPT_PHASE_30,
        };
        // = seg000:a216 jmp start_scripted_dialogue.
        self.start_scripted_dialogue(script);
    }

    // = seg000:1771 start_scripted_dialogue — install `script` as the active
    // continue-sequence: the cursor at its head, the dialogue-active flag up
    // (the room loop routes the command panel to the dialogue renderer), the
    // game clock suspended, the current scene snapshotted for the teardown,
    // and the blink frame task installed.
    pub(crate) fn start_scripted_dialogue(&mut self, script: &'static [u8]) {
        // = seg000:1771/1774 data_0477a = the script, data_04778 = 0.
        self.sequence_script = Some(script);
        self.sequence_cursor = 0;
        self.sequence_return_cursor = None;
        // = seg000:177a inc data_04774.
        self.is_dialogue_active = true;
        // = seg000:177e call suspend_game_clock.
        self.suspend_game_clock();
        // = seg000:1781 call loc_0ad5e — the room music refresh (update_room_
        //   music). Not ported.
        // = seg000:1784..178b data_04776 = (location_and_room low, data_046e0)
        //   — what the teardown puts back.
        self.sequence_saved_scene = (self.location_and_room as u8, self.data_046e0);
        // = seg000:178e..1794 add_frame_task(frame_task_callback_blink, 0x64).
        self.add_frame_task(0x64, TaskId::SequenceBlink);
    }

    // = seg000:176b frame_task_callback_blink — the scripted scene's blink
    // toggle (data_23c25), flipped every 0x64 PIT ticks. Its only reader is
    // the unported dialogue-panel renderer, so the toggle is kept and nothing
    // else happens.
    pub(crate) fn tick_sequence_blink(&mut self) {
        self.sequence_blink = !self.sequence_blink;
    }

    // = seg000:2ebf loc_02ebf — push the scene's " Continue…" panel (the menu
    // menu_ptr_02220 currently points at) with a no-op cleanup. The
    // occupation verbs' prospector branch and the dialogue tail both end here.
    pub(crate) fn sequence_push_continue_menu(&mut self) {
        // = seg000:2ebf/2ec3/2ec6 bp = [menu_ptr_02220]; bx = nullsub_00f66;
        //   jmp screen_element_stack_push.
        let element = self.sequence_menu;
        self.menu_stack_push(element, None);
    }

    // = seg000:1392 change_menu_to_continue_menu — menu_ptr_02220 =
    // menu_multiple_provide_continue_option (seg001:1fba).
    fn change_menu_to_continue_menu(&mut self) {
        self.sequence_menu = MenuRef::MenuContinue;
    }

    // = seg000:1399 change_menu_to_special_menu_after_specializing_prospector_
    // troop_in_spice — menu_ptr_02220 =
    // menu_prospector_troop_after_specializing_in_spice (seg001:1fae).
    fn change_menu_to_prospector_menu(&mut self) {
        self.sequence_menu = MenuRef::MenuProspectorContinue;
    }

    // = seg000:1707 menu_callback_choice_continue_for_sequence — the
    // " Continue…" slot. While the prospector panel is the active one a click
    // inside the talking-head HUD element (8) replays the line instead
    // (menu_callback_choice_what); anything else steps the script.
    pub(crate) fn menu_callback_choice_continue_for_sequence(&mut self) {
        // = seg000:1707/170d cmp [menu_ptr_02220],1faeh; jnz continue.
        if self.sequence_menu == MenuRef::MenuProspectorContinue {
            // = seg000:170f..1715 di = ui_hud_elements[8]; rect_contains.
            let e = self.ui_elements[8];
            let (x, y) = (self.mouse_pos_x, self.mouse_pos_y);
            if x > e.x0 && x < e.x1 && y > e.y0 && y < e.y1 {
                // = seg000:1717 jmp menu_callback_choice_what.
                self.menu_callback_choice_what();
                return;
            }
        }
        self.menu_callback_choice_continue();
    }

    // = seg000:171a menu_callback_choice_continue — step the script: read the
    // next action byte (0xff ends the scene) and dispatch it. The byte is a
    // byte offset into array_callbacks_actions_in_continue_sequence, so the
    // action index is byte / 2.
    pub(crate) fn menu_callback_choice_continue(&mut self) {
        // = seg000:171a kb_keys_enter = 0 — consume the Enter that may have
        //   triggered the slot. The port's keyboard state lives behind the
        //   shared input mutex and no reader latches Enter here, so nothing
        //   to clear.
        // = seg000:171f/1723 si = [data_0477a]; cs:lodsb.
        let Some(script) = self.sequence_script else {
            return;
        };
        let Some(&byte) = script.get(self.sequence_cursor) else {
            self.sequence_end();
            return;
        };
        // = seg000:1725 cmp al,0ffh; jz loc_01736.
        if byte == SCRIPT_END {
            self.sequence_end();
            return;
        }
        // = seg000:1729 store the advanced cursor.
        self.sequence_cursor += 1;
        // = seg000:172d..1731 xor ah,ah; mov bx,ax; jmp [table + bx].
        match byte {
            // = seg000:13a0 callback_action_in_continue_sequence_07.
            SCRIPT_SHOW_SPICE_MAP => self.sequence_action_07_show_spice_map(),
            // = seg000:13aa callback_action_in_continue_sequence_08.
            SCRIPT_HIDE_SPICE_MAP => self.sequence_action_08_hide_spice_map(),
            other => {
                // The cutscene-script actions (the phase 0x14/0x18/0x30
                // scripts): scene swaps, room re-renders, the time skip and
                // the Chani kiss (seg000:13c8/13db/13e4/140b/1422/1442/148d/
                // 14c9/167c). Their scripts are only reached from the
                // unported phase callbacks. TODO.
                println!("continue-sequence: unported action {other} (byte {byte:#04x})");
                self.change_menu_to_continue_menu();
                self.sequence_push_continue_menu();
            }
        }
    }

    // = seg000:13a0 callback_action_in_continue_sequence_07 — set the
    // data_046eb bit-6 one-shot and speak the next line ("Here, take this map
    // of the planet."). The overlay is not drawn here: presenting the line
    // runs show_voice_subtitle, whose bit-6 short-circuit (seg000:88bb)
    // routes the text through move_troop_show_instruction_caption, whose
    // tail raises the spice-density overlay.
    fn sequence_action_07_show_spice_map(&mut self) {
        // = seg000:13a0 call change_menu_to_special_menu_...
        self.change_menu_to_prospector_menu();
        // = seg000:13a3 or data_046eb,40h — the one-shot show_voice_subtitle
        //   consumes.
        self.data_046eb |= 0x40;
        // = seg000:13a8 jmp sequence_present_line_and_voice.
        self.sequence_present_line();
    }

    // = seg000:13aa callback_action_in_continue_sequence_08 — drop the
    // overlay, repaint the contact panel and speak the closing line ("You can
    // update this map…").
    fn sequence_action_08_hide_spice_map(&mut self) {
        // = seg000:13aa call loc_058fa — leave the sub-mode.
        self.map_leave_spice_density_overlay();
        // = seg000:13ad..13b3 si = troop_contact_text_panel_record; call
        //   loc_0c551 — repaint the contact popup's panel outline on screen.
        self.set_screen_as_active_framebuffer();
        let r = self.map_contact_popup_rect;
        self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, 0xf5);
        // = falls into sequence_present_line_and_voice.
        self.sequence_present_line();
    }

    // = seg000:13b6 sequence_present_line_and_voice — the shared line tail of
    // the sequence actions:
    // present the speaker-0x0f topic-7 line into the screen buffer, play its
    // voice, drop the bubble pointer and return to fb1.
    fn sequence_present_line(&mut self) {
        // = seg000:13b6/13b9 ax = 0x0f; call set_screen_as_active_framebuffer.
        self.set_screen_as_active_framebuffer();
        // = seg000:13bc call loc_09761.
        self.sequence_present_topic7_line(0x0f);
        // = seg000:13bf call loc_09efd — the voice.
        self.play_dialogue_voc();
        // = seg000:13c2 call loc_09901 — drop the bubble pointer.
        self.subtitle_bubble = None;
        // = seg000:13c5 jmp set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // The panel the action installed becomes the active element again so
        // the next " Continue…" click steps the script (DOS leaves it up from
        // the loc_02ebf push that opened the scene).
        self.sequence_push_continue_menu();
    }

    // = seg000:9761 loc_09761 — present the scene's next line: the speaker
    // (0x0f here) selects the dialogue record (speaker * 8) | 7, presented
    // with the verb mask 0x80. Speaker 0x0e additionally stages the Fremen-1
    // troop's CONDIT block.
    fn sequence_present_topic7_line(&mut self, speaker: u16) {
        // = seg000:9761 current_lip_sync_resource_id = ax.
        self.current_lip_sync_resource_id = speaker;
        // = seg000:9764..976f the 0x0e troop staging.
        if speaker == 0x0e {
            if let Some(ti) = self.fremen1_troop {
                self.troop_prepare_troop_data_for_condit(ti);
            }
        }
        // = seg000:9772..977f si = DIALOGUE[(speaker << 3) | 7].
        let ofs = crate::container::entry_offset(&self.dialogue, (speaker << 3) | 7);
        if ofs == 0xffff {
            return;
        }
        // = seg000:9783 call loc_09f40; 9786 data_047c2 = 0x80.
        self.prepare_dialogue_presentation();
        self.data_047c2 = 0x80;
        // = seg000:978b jmp present_first_matching_dialogue_line.
        self.present_first_matching_dialogue_line(ofs as usize);
    }

    // = seg000:1736 loc_01736 — the script's 0xff end: drop the blink task,
    // restore the snapshotted scene, clear the dialogue-active flag, fade the
    // music out (except in game phase 0x48), resume the clock and put the
    // view back — the room screen, or on the map the contacted troop's order
    // menu.
    fn sequence_end(&mut self) {
        // = seg000:1736/1739 remove_frame_task(frame_task_callback_blink).
        self.remove_frame_task(TaskId::SequenceBlink);
        self.sequence_script = None;
        self.sequence_cursor = 0;
        // = seg000:173c..1746 location_and_room / data_046e0 = data_04776.
        let (scene, sky) = self.sequence_saved_scene;
        self.location_and_room = (self.location_and_room & 0xff00) | scene as u16;
        self.data_046e0 = sky;
        // = seg000:1748 data_04774 = 0.
        self.is_dialogue_active = false;
        // = seg000:174b..1752 the music fade, skipped in phase 0x48.
        if self.game_phase != 0x48 {
            // = seg000:1752 call midi_begin_song_fade_out. The port's music
            //   layer has no fade-out entry point yet. TODO.
        }
        // = seg000:1755 call reset_game_suspend.
        self.reset_game_suspend();
        // = seg000:1758..175f cmp room_view_toggle,0; js loc_01762 — on the
        //   map view the contacted troop's menu comes back; in the room the
        //   screen is re-presented (loc_00fa7).
        if self.room_view_toggle & 0x80 != 0 {
            // = seg000:1762 call loc_0ad5e (music, unported); 1765 call
            //   contact_verb_troop; 1768 jmp map_open_troop_contact_menu.
            if let Some(ti) = self.contact_verb_troop() {
                self.map_open_troop_contact_menu(ti);
            }
            return;
        }
        // = seg000:0fa7 loc_00fa7 — restore the cursor, present with the
        //   spiral effect and redraw the companion portraits.
        self.call_restore_cursor();
        self.ui_present_room_screen(0x2a);
        self.ui_hud_draw_companions();
    }
}
