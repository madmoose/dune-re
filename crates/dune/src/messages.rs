//! The message / COMM system: the vision-message queue consumers (the queue
//! itself lives in game_phase.rs), the idle-room presenter and the full-screen
//! vision dream, and the palace communications-room message viewer.
//!
//! Mirrors the DOS block seg000:2741..29ed (the COMM console + viewer) and
//! seg000:2a51..2c8f (the queue helpers + presentation), with
//! comm_add_person_sighting / queue_vision_message in game_phase.rs.

use crate::{
    GameState,
    game_state::TaskId,
    gfx, locations,
    menu_defs::{MenuRef, item},
    rect::Rect,
    sprite_bank,
};

// = seg001:14c8 _stru_20978_icon_list — the COMM console icon list, (sprite,
// x, y): the main console (0x12) and the two flanking side panels (0x13,
// mirrored). comm_draw_flicker re-stamps the side panels with sprite
// 0x13 + (glow & 3).
const COMM_ICON_LIST: [(u16, i16, i16); 3] =
    [(0x12, 0x9c, 0x64), (0x13, 0x54, 0x5c), (0x4013, 0xdd, 0x5c)];

// = seg001:225d data_0225d — the per-person COMM face-sprite position
// (x = the word's low byte, y = its high byte); comm_draw_message_face
// draws COMM sprite person + 0x1e there.
const COMM_FACE_POS: [(i16, i16); 12] = [
    (0xf2, 0xff),
    (0x0c, 0x05),
    (0x0a, 0xff),
    (0x09, 0x0b),
    (0x00, 0x00),
    (0x00, 0x00),
    (0x40, 0x01),
    (0x98, 0x00),
    (0x0a, 0x00),
    (0x8b, 0x12),
    (0x9c, 0x10),
    (0x9d, 0x0c),
];

fn menu_cleanup_noop(_: &mut GameState) {}

impl GameState {
    // = the DOS current_location_ptr word (seg001:114e) as the vision-message
    // location words store it: the location's seg001 record pointer, 0 when
    // the player is not inside a location.
    pub(crate) fn current_location_ptr_word(&self) -> u16 {
        if (self.current_location_index as usize) < self.locations.len() {
            locations::location_ptr_from_index(self.current_location_index as usize)
        } else {
            0
        }
    }

    // = seg000:29ee queue_vision_message_without_location — di = 0.
    pub(crate) fn queue_vision_message_without_location(&mut self, message_id: u16) {
        self.queue_vision_message(message_id, 0);
    }

    // = seg000:29f0 queue_vision_message_with_location — queue a vision
    // message (shown when Paul next sleeps): only once Paul has had his first
    // vision (bitfield_Paul_events bit 0); duplicates (same id + location)
    // are dropped; at 10 messages the oldest is dequeued first.
    pub(crate) fn queue_vision_message(&mut self, message_id: u16, location: u16) {
        // = seg000:29f0 test [bitfield_Paul_events],1; jz ret.
        if self.bitfield_paul_events & 1 == 0 {
            return;
        }
        // = seg000:2a01..2a0d the dedup scan.
        if self
            .vision_messages
            .iter()
            .any(|&(m, l)| m == message_id && l == location)
        {
            return;
        }
        // = seg000:2a14..2a22 at 10 messages dequeue the oldest.
        if self.vision_messages.len() >= 10 {
            self.dequeue_vision_message();
        }
        // = seg000:2a25..2a30 append + count.
        self.vision_messages.push((message_id, location));
    }

    // = seg000:2a34 dequeue_vision_message — drop the oldest queued message.
    // (DOS also clears the byte at seg001:118f when the queue drains; nothing
    // reads it, so the port does not carry it.)
    pub(crate) fn dequeue_vision_message(&mut self) {
        if !self.vision_messages.is_empty() {
            self.vision_messages.remove(0);
        }
    }

    // = seg000:2a51 purge_vision_messages_of_class — remove queued vision
    // messages whose sender class (id high byte) is `class`, compacting the
    // queue. For class 0x0f (the location-event reports) only messages whose
    // location word equals `location` are removed. Called after the sender
    // delivers the line in person (loc_03542) or the troop is contacted
    // (seg000:9729).
    pub(crate) fn purge_vision_messages_of_class(&mut self, class: u8, location: u16) {
        self.vision_messages
            .retain(|&(id, loc)| (id >> 8) as u8 != class || (class == 0x0f && loc != location));
    }

    // = seg000:2a7f purge_message_arrived_vision_messages — remove every
    // queued message whose id low byte is 1 ("A message has arrived in the
    // palace."): the player is viewing the COMM messages, so the arrival
    // reminders are moot.
    pub(crate) fn purge_message_arrived_vision_messages(&mut self) {
        self.vision_messages.retain(|&(id, _)| id & 0xff != 1);
    }

    // = seg000:2aaf find_vision_message_from_person — the first queued
    // message from sender class `class`; a class-0x0f location report must
    // concern `location`. DOS returns carry set with ax = the id and di =
    // the location word.
    pub(crate) fn find_vision_message_from_person(
        &self,
        class: u8,
        location: u16,
    ) -> Option<(u16, u16)> {
        self.vision_messages
            .iter()
            .copied()
            .find(|&(id, loc)| (id >> 8) as u8 == class && (class != 0x0f || loc == location))
    }

    // ---- Presentation -----------------------------------------------------

    // = seg000:2b00 present_vision_message — present a vision message through
    // the fixed dialogue block 0x84: stage the message's location for CONDIT
    // (so the "here in ..." placeholders name it), set ds:ea to the id low
    // byte so the block's conditions select the right sentence, and present
    // with the sender class as the lip-sync speaker.
    pub(crate) fn present_vision_message(&mut self, message_id: u16, location: u16) {
        // = seg000:2b00 push word [data_011ce] — remember the staged location.
        let saved = self.condit_staged_location;
        // = seg000:2b04..2b09 stage the message's location.
        if location != 0 {
            self.prepare_location_data_for_condit(locations::location_index_from_ptr(location));
        }
        // = seg000:2b0d [vision_message_type_ds_ea] = al.
        self.data_000ea = (message_id & 0xff) as i8;
        // = seg000:2b10..2b14 al = the sender class; call loc_096d8 — the
        // fixed-block presenter. The port's presentation bundles the idle-
        // animator install (seg000:2b17 loc_09945) and the fixed-bank voc
        // start (seg000:2b1f loc_09ef1 al=1).
        self.travel_play_flyover_line((message_id >> 8) as u8);
        // = seg000:2b1a [vision_message_type_ds_ea] = 0xff.
        self.data_000ea = -1;
        // = seg000:2b24/2b25 pop di; call prepare_location_data_for_condit —
        // restore the staged location.
        self.prepare_location_data_for_condit(saved);
    }

    // = seg000:2ad8 present_queued_vision_message_if_speaker_present —
    // present the head-of-queue message when its sender can deliver it: the
    // sender-class bit must be set in persons_in_room, and a class-0x0f
    // location report also requires Paul to be at the message's location.
    // Returns DOS's carry (presented).
    fn present_queued_vision_message_if_speaker_present(&mut self) -> bool {
        let Some(&(id, loc)) = self.vision_messages.first() else {
            return false;
        };
        // = seg000:2adc..2ae6 CF = bit `class` of persons_in_room.
        let class = id >> 8;
        if (self.persons_in_room >> class) & 1 == 0 {
            return false;
        }
        // = seg000:2aec..2af5 a class-0x0f report only presents in place.
        if class == 0x0f && loc != self.current_location_ptr_word() {
            return false;
        }
        // = seg000:2afd call call_restore_cursor; falls into
        // present_vision_message (the pushed ax is discarded with add sp,2).
        self.call_restore_cursor();
        self.present_vision_message(id, loc);
        true
    }

    // = the loc_03542 tail entered from the idle checker (seg000:2b68/2b6d):
    // purge the just-delivered message class and install the speaker's
    // dialogue verb panel.
    fn vision_present_tail(&mut self) {
        // = seg000:2b68 mov byte [pending_room_action],0.
        self.pending_room_action = 0;
        // = seg000:3542..3549 purge_vision_messages_of_class(speaker, here).
        let speaker = self.current_lip_sync_resource_id as u8;
        let loc_ptr = self.current_location_ptr_word();
        self.purge_vision_messages_of_class(speaker, loc_ptr);
        // = seg000:354c mov byte [Paul_found_unconscious_in_desert_ds_e7],0.
        self.paul_found_unconscious_ds_e7 = 0;
        // = seg000:3551 loc_03551.
        self.install_pending_room_action_menu();
    }

    // = seg000:2b2a idle_room_message_check — the idle-room checker run each
    // game_loop pass (seg000:1b20): with no pending screen work, the base
    // verb menu active and the room view up, present the head-of-queue vision
    // message once the player has idled long enough.
    pub(crate) fn idle_room_message_check(&mut self) {
        // = seg000:2b2a..2b35 nothing pending, no scripted dialogue, plain
        // room mode.
        if self.pending_room_screen_request != 0
            || self.is_dialogue_active
            || self.game_screen_mode_flags != 0
        {
            return;
        }
        // = seg000:2b37..2b3e the base verb menu must be the active element.
        if self.get_active_menu_ref() != MenuRef::CommandMenuBuf {
            return;
        }
        // = seg000:2b40 room_view_toggle sign set = the map/globe view.
        if self.room_view_toggle & 0x80 != 0 {
            return;
        }
        // = seg000:2b47..2b4f ax = the PIT counter; pre-vision phases idle
        // out; phase exactly 0x14 takes the first-vision trigger instead.
        let ticks = self.game_ticks() as u16;
        if self.game_phase < 0x14 {
            return;
        }
        if self.game_phase == 0x14 {
            self.first_vision_idle_check(ticks);
            return;
        }
        // = seg000:2b53 nothing queued.
        if self.vision_messages.is_empty() {
            return;
        }
        // = seg000:2b5a..2b61 under 0x32 idle ticks.
        let idle = ticks.wrapping_sub(self.game_clock_tick_base);
        if idle < 0x32 {
            return;
        }
        // = seg000:2b63/2b66 present in-room if the sender is here, then the
        // purge + speaker-menu tail.
        if self.present_queued_vision_message_if_speaker_present() {
            self.vision_present_tail();
            return;
        }
        // = seg000:2b70 during a night attack the dream runs immediately.
        if self.night_attack_stage != 0 {
            self.present_vision_dream();
            return;
        }
        // = seg000:2b77..2b8d the idle windows: < 0x96 and 0xfa..0x15e play
        // the glance transitions (loc_02b90, vga_transition codes 0x28 /
        // 0x26) — both codes fall through to the plain copy in the port, so
        // the repaint is skipped rather than replayed every pass; 0x1c2+
        // runs the full vision dream.
        if idle >= 0x1c2 {
            self.present_vision_dream();
        }
    }

    // = seg000:2ba1 loc_02ba1 — the first-vision trigger: at game phase
    // exactly 0x14, alone (no companions) and out in the desert
    // (current_scene == 0xff), 0x3e8 idle ticks advance the phase to 0x15
    // (Paul's first vision) and drain the queued messages it posts.
    fn first_vision_idle_check(&mut self, ticks: u16) {
        // = seg000:2ba1..2bad.
        if self.persons_travelling_with != 0 {
            return;
        }
        if self.data_00008 != 0xff {
            return;
        }
        // = seg000:2baf..2bb6.
        let idle = ticks.wrapping_sub(self.game_clock_tick_base);
        if idle < 0x3e8 {
            return;
        }
        // = seg000:2bb8 game_clock_tick_base += 0x3b6 — re-arm most of the
        // idle window (the drain below sees 0x32 elapsed ticks).
        self.game_clock_tick_base = self.game_clock_tick_base.wrapping_add(0x3b6);
        // = seg000:2bbe call loc_01071 — the phase advance.
        self.first_vision_phase_advance();
        // = seg000:2bc1 call run_game_phase_triggers.
        self.run_game_phase_triggers();
        // = seg000:2bc4..2bcc drain: re-run the checker while a message is
        // mid-presentation (ds:ea != 0xff).
        loop {
            self.idle_room_message_check();
            if self.data_000ea == -1 {
                break;
            }
        }
    }

    // ---- The full-screen vision dream -------------------------------------

    // = seg000:2bd2 present_vision_dream — the full-screen "Paul hears a
    // voice" presentation: transition into the VIS.HSQ backdrop, present the
    // head-of-queue message with its sender as the talking head, shimmer the
    // game area while it plays out, then re-present the room.
    pub(crate) fn present_vision_dream(&mut self) {
        // = seg000:2bd2/2bd5 restore the cursor and advance the room music.
        self.call_restore_cursor();
        self.update_room_music();
        // = seg000:2bd8..2bf1 stage the message's location for CONDIT; a
        // location word outside the locations table (di - 0x100 >= 0x7aa) is
        // zeroed in the queue instead.
        let Some(&(id, mut loc)) = self.vision_messages.first() else {
            return;
        };
        if loc != 0 {
            if loc.wrapping_sub(0x100) >= 0x7aa {
                self.vision_messages[0].1 = 0;
                loc = 0;
            } else {
                self.prepare_location_data_for_condit(locations::location_index_from_ptr(loc));
            }
        }
        // = seg000:2bf4..2bfe ds:ea = the id low byte; a type-1 message ("A
        // message has arrived...") also raises the needs-viewing condit flag.
        let msg_type = (id & 0xff) as u8;
        self.data_000ea = msg_type as i8;
        if msg_type == 1 {
            self.comm_message_needs_viewing_ds_eb = 1;
        }
        // = seg000:2c01..2c14 a class above the speaker range, or a troop
        // report (class 0x0e) without a location, is dropped unspoken
        // (loc_02bcf -> dequeue).
        let class = id >> 8;
        if class >= 0x10 || (class == 0x0e && loc == 0) {
            self.dequeue_vision_message();
            return;
        }
        // = seg000:2c16 the sender class speaks.
        self.current_lip_sync_resource_id = class;
        if loc != 0 {
            // = seg000:2c1d/2c20 stage the location and its name
            // placeholders.
            let li = locations::location_index_from_ptr(loc);
            self.prepare_location_data_for_condit(li);
            self.stage_location_name_placeholders(li);
            // = seg000:2c23..2c43 a class-0x0e/0x0f report speaks through the
            // location's troop: al = 3 (the prospectors) for message type
            // 0x0e, else the location's troop chain head; resolve it into
            // fremen1_troop_ptr and retarget the lip-sync to the generic
            // troop head 0x0e. A location without a troop keeps the class.
            if self.current_lip_sync_resource_id >= 0x0e {
                let troop_id = if msg_type == 0x0e {
                    3
                } else {
                    self.locations[li].troop_id
                };
                if troop_id != 0 {
                    // = seg000:2c3a call get_address_of_troop_by_ID.
                    self.fremen1_troop = Some((troop_id - 1) as usize);
                    self.current_lip_sync_resource_id = 0x0e;
                }
            }
        }
        // = seg000:2c47 call loc_02c92 — transition 6 into the vision
        // backdrop, presenting the line inside the transition render.
        self.transition(6, 0, |s| s.vision_dream_backdrop());
        // = seg000:2c4a/2c4c al = 1; call loc_09ef1 — the voc start; the
        // port's presentation inside vision_dream_backdrop bundles it.
        // = seg000:2c4f dequeue the presented message.
        self.dequeue_vision_message();
        // = seg000:2c52..2c5a blank the verb panel (the command buffer's
        // skip byte and first record are zeroed) and repaint it.
        self.command_menu_buf.skip = 0;
        self.command_menu_buf.records.clear();
        self.redraw_active_command_menu();
        // = seg000:2c5d..2c66 hold the dream for 0xbb8 ticks (or a key),
        // game clock suspended, the shimmer task running.
        self.suspend_game_clock();
        self.wait_interruptable(0xbb8);
        self.resume_game_clock();
        // = seg000:2c69..2c77 remove the shimmer task and the lip-sync state.
        self.remove_frame_task(TaskId::VisionShimmer);
        self.reset_scene_lip_sync_state();
        // = seg000:2c72..2c77 current_bubble_layout_ptr = 0 and the lip-sync
        // image-list count word — the port's talking-head teardown
        // (reset_scene_lip_sync_state) covers both.
        // = seg000:2c7a..2c8a ds:ea back to idle, the HUD head fully raised,
        // the chained-voc word cleared (data_0dc30, unmodelled).
        self.data_000ea = -1;
        self.ui_hud_head_index = 0x0a;
        // = seg000:2c8c al = 6; call ui_present_room_screen; 2c8f jmp
        // copy_active_framebuffer_to_framebuffer_2.
        self.ui_present_room_screen(6);
        self.copy_active_framebuffer_to_framebuffer_2();
    }

    // = seg000:2c9a vision_dream_backdrop — the dream's transition render:
    // fold the HUD head away, draw the VIS.HSQ backdrop, present the message
    // line, and swap the room frame task for the shimmer task.
    fn vision_dream_backdrop(&mut self) {
        // = seg000:2c9a ui_hud_head_index = 0.
        self.ui_hud_head_index = 0;
        // = seg000:2c9f/2ca1 open VIS.HSQ and draw sprite 0 full-screen.
        self.open_resource_and_draw_sprite0(sprite_bank::VIS);
        // = seg000:2ca4 snapshot the backdrop into fb2.
        self.copy_active_framebuffer_to_framebuffer_2();
        // = seg000:2ca7/2caa present the line through loc_096d8 (the class
        // was staged in current_lip_sync_resource_id; the fixed block 0x84
        // supplies the sentence, ds:ea the selector). The port's presenter
        // also plays the voc (= the loc_09ef1 the DOS caller issues after).
        let class = self.current_lip_sync_resource_id as u8;
        self.travel_play_flyover_line(class);
        // = seg000:2cad..2cb5 drop the bubble/head-ornament elements and the
        // sky fade.
        self.ui_elements[18].flags = 0;
        self.ui_elements[19].flags = 0;
        self.sky_fade_active = false;
        // = seg000:2cb8..2cc4 swap the room frame task for the shimmer task
        // (interval 6).
        self.remove_frame_task(TaskId::Room);
        self.add_frame_task(6, TaskId::VisionShimmer);
    }

    // = seg000:2cc7 vision_shimmer_frame_task — blit fb1 to the screen with
    // the water-ripple effect (0x0a) over the game-area rect (data_01478).
    pub(crate) fn tick_vision_shimmer(&mut self) {
        let rect = Rect {
            x0: 8,
            y0: 0,
            x1: 312,
            y1: 148,
        };
        self.blit_fb1_to_screen_effect(0x0a, rect);
    }

    // ---- The COMM room console --------------------------------------------

    // = seg000:274e comm_draw_panels — draw the COMM console icon list and
    // the idle glow frame onto the visible screen.
    fn comm_draw_panels(&mut self) {
        // = seg000:274e call set_screen_as_active_framebuffer.
        self.set_screen_as_active_framebuffer();
        // = seg000:2751/2754 open COMM.HSQ.
        self.open_sprite_bank(sprite_bank::COMM);
        // = seg000:2757/275a draw the console icon list.
        self.with_active_bank_sheet(|s, sheet| s.draw_icons_list_at_si(&COMM_ICON_LIST, sheet));
        // = seg000:275d al = 1; falls into comm_draw_glow.
        self.comm_draw_glow(1);
    }

    // = seg000:275f comm_draw_glow — draw glow sprite (al & 7) + 0x0b at
    // (100, 86) on the visible screen, then reselect fb1.
    fn comm_draw_glow(&mut self, frame: u16) {
        self.set_screen_as_active_framebuffer();
        // = seg000:2762..276d sprite (al & 7) + 0x0b at dx=0x64, bx=0x56.
        self.draw_active_bank_sprite((frame & 7) + 0x0b, 0x64, 0x56);
        self.send_frame_to_display();
        // = seg000:2770 jmp set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
    }

    // = seg000:27b6 comm_draw_glow_index — map the glow counter onto the
    // 2..6..2 triangle wave, draw that glow frame, then fall into
    // comm_draw_flicker.
    fn comm_draw_glow_index(&mut self) {
        // = seg000:27b6..27c4 ax = index & 7; >= 5 mirrors down (8 - ax);
        // + 2.
        let mut frame = self.comm_glow_index & 7;
        if frame >= 5 {
            frame = 8 - frame;
        }
        self.comm_draw_glow(frame + 2);
        self.comm_draw_flicker();
    }

    // = seg000:27c9 comm_draw_flicker — the console static: four random
    // sparkle sprites down the screen strip, then the two side panels
    // re-stamped with the (glow & 3) frame.
    fn comm_draw_flicker(&mut self) {
        // = seg000:27c9 draw on the visible screen.
        self.set_screen_as_active_framebuffer();
        // = seg000:27cc..27ee bx = 0x67..0x70 step 3; per row a rand & 0x0f
        // re-rolled while it repeats the previous roll; sprite 0x17 + r at
        // dx = 0xa3.
        let mut prev = 0xffff;
        for y in (0x67..=0x70i16).step_by(3) {
            let mut r = self.rand_masked(0x0f);
            while r == prev {
                r = self.rand_masked(0x0f);
            }
            prev = r;
            self.draw_active_bank_sprite(r + 0x17, 0xa3, y);
        }
        // = seg000:27f0..2800 re-stamp the side panels: entries [1]/[2] of
        // the icon list get sprite 0x13 + (glow & 3) (the mirror flag on [2]
        // survives the low-byte write).
        let s = 0x13 + (self.comm_glow_index & 3);
        let panels = [
            (s, COMM_ICON_LIST[1].1, COMM_ICON_LIST[1].2),
            (0x4000 | s, COMM_ICON_LIST[2].1, COMM_ICON_LIST[2].2),
        ];
        self.with_active_bank_sheet(|st, sheet| st.draw_icons_list_at_si(&panels, sheet));
        self.send_frame_to_display();
        // = seg000:2803 jmp set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
    }

    // = seg000:2795 comm_fade_in_glow — pulse the glow in: 13 frames of the
    // glow triangle + flicker, 9 ticks each.
    pub(crate) fn comm_fade_in_glow(&mut self) {
        // = seg000:2795/2798 open COMM.HSQ.
        self.open_sprite_bank(sprite_bank::COMM);
        self.comm_glow_index = 0;
        // = seg000:27a1..27b3 per frame: comm_draw_glow_index via the wait's
        // bp callback, 9 ticks.
        while self.comm_glow_index < 0x0d {
            self.comm_draw_glow_index();
            self.wait_frame_tasks_for_ticks(9);
            self.comm_glow_index += 1;
        }
    }

    // = seg000:2773 comm_fade_out_glow — the computer beeps (SN9.VOC) while
    // the glow steps back down, then the idle console repaints.
    fn comm_fade_out_glow(&mut self) {
        // = seg000:2773/2776 open COMM.HSQ; 2779/277b play SN9.VOC.
        self.open_sprite_bank(sprite_bank::COMM);
        self.audio_start_voc("SN9.VOC");
        // = seg000:277e..2791 glow_index 4 down through 0.
        for i in (0..=4u16).rev() {
            self.comm_glow_index = i;
            self.comm_draw_glow_index();
            self.wait_frame_tasks_for_ticks(9);
        }
        // = seg000:2793 jmp comm_draw_panels.
        self.comm_draw_panels();
    }

    // = seg000:281c comm_animate_incoming_call — `frames` frames of console
    // static, 9 ticks each, the glow counter advancing.
    fn comm_animate_incoming_call(&mut self, frames: u16) {
        self.open_sprite_bank(sprite_bank::COMM);
        self.comm_glow_index = 0;
        // = seg000:2828..2837.
        for _ in 0..frames {
            self.comm_draw_flicker();
            self.wait_frame_tasks_for_ticks(9);
            self.comm_glow_index += 1;
        }
    }

    // = seg000:2806 comm_show_incoming_call — the console lights up: panels +
    // glow 2, the incoming-call flicker for 0x14 frames, wait out the
    // narration clip, settle the glow to frame 1.
    fn comm_show_incoming_call(&mut self) {
        self.comm_draw_panels();
        self.comm_draw_glow(2);
        self.comm_animate_incoming_call(0x14);
        self.wait_for_narration_voice_clip();
        self.comm_draw_glow(1);
    }

    // ---- The message viewer -----------------------------------------------

    // = seg000:283a menu_callback_choice_comms_room_view_new_messages — the
    // VIEW NEW MESSAGES verb: open the message list filtered to unread rows.
    pub(crate) fn menu_callback_choice_comms_room_view_new_messages(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:283a xor ax,ax; jmp loc_02841.
        self.comm_open_message_list(0);
    }

    // = seg000:283e menu_callback_choice_comms_room_messages_already_seen —
    // the companion verb: the list filtered to already-viewed rows.
    pub(crate) fn menu_callback_choice_comms_room_messages_already_seen(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:283e xor ax,ax; dec ax; falls into loc_02841.
        self.comm_open_message_list(0xff);
    }

    // = seg000:2841 loc_02841 — the shared list opener: al (0 = new, 0xff =
    // seen) picks the filter and the narration clip (0x2a / 0x2b), the
    // console animates the incoming call, and the sender rows build into
    // menu_dynamic.
    fn comm_open_message_list(&mut self, filter: u8) {
        // = seg000:2841 [comm_list_filter_seen_ds_db] = al.
        self.comm_list_filter_seen = filter;
        // = seg000:2844..2848 clip = (-al) + 0x2a: 0x2a for new, 0x2b for
        // seen.
        let clip = if filter == 0 { 0x2a } else { 0x2b };
        self.duck_music_and_start_narration_voice_clip(clip);
        // = seg000:284b call tear_down_prior_talking_head_overlay.
        self.tear_down_prior_talking_head_overlay();
        // = seg000:284e call comm_show_incoming_call.
        self.comm_show_incoming_call();
        // = seg000:2851 call purge_message_arrived_vision_messages.
        self.purge_message_arrived_vision_messages();
        // = seg000:2854 call loc_03ae9 — clear the character x/y tables (no
        // room people are click-targetable over the console).
        self.character_screen_pos = [(0xffff, 0xffff); 0x17];
        // = seg000:2857 [for_condit_ds_e9] = 0.
        self.for_condit_ds_e9 = 0;
        // = seg000:285c..2863 menu_dynamic's skip byte = 0.
        self.menu_dynamic.skip = 0;
        // = seg000:2864..288b walk comm_sighting_list from the newest entry
        // down; a row passes the filter when (low byte ^ filter) keeps bit 7
        // clear. Each row is the sender's "&Person" name (low & 0x3f + 0x78)
        // bound to the row-click callback (loc_0290b).
        let mut records = Vec::new();
        for &word in self.comm_sightings.iter().rev() {
            if (word as u8 ^ filter) & 0x80 != 0 {
                continue;
            }
            records.push(item(
                (word & 0x3f) + 0x78,
                0x290b,
                GameState::menu_callback_comms_message_selected,
            ));
        }
        // = seg000:288d..2894 the trailing "  Cancel" (0xa3) row.
        records.push(item(
            0xa3,
            0x29d4,
            GameState::menu_callback_comms_messages_done,
        ));
        self.menu_dynamic.records = records;
        // = seg000:2898..289e bp = menu_dynamic; bx = nullsub_00f66; jmp
        // loc_0d323 — stage the list with the panel fold.
        self.stage_command_submenu(MenuRef::MenuDynamic, menu_cleanup_noop);
    }

    // = seg000:290b menu_callback_comms_message_selected — a message row was
    // clicked: resolve the sighting, mark it viewed (in view-new mode), show
    // the sender's face and let them speak, then offer the Viewed verb.
    pub(crate) fn menu_callback_comms_message_selected(&mut self, _text_id: u16, index: usize) {
        // = seg000:290b..292d cx = the clicked slot + the skip byte / 4, + 1
        // (the port's dispatch already folds the skip into `index`): count
        // that many filter-passing rows back from the end of the list.
        let filter = self.comm_list_filter_seen;
        let mut remaining = index + 1;
        let mut pos = None;
        for (i, &word) in self.comm_sightings.iter().enumerate().rev() {
            if (word as u8 ^ filter) & 0x80 != 0 {
                continue;
            }
            remaining -= 1;
            if remaining == 0 {
                pos = Some(i);
                break;
            }
        }
        let Some(pos) = pos else { return };
        let word = self.comm_sightings[pos];
        // = seg000:292f/2931 ds:24 = the sighting's location-index byte.
        self.for_dialogue_enemies_ds_24 = (word >> 8) as u8;
        // = seg000:2935 al &= 0x3f — the sender person.
        let person = (word & 0x3f) as u8;
        // = seg000:2937..2954 in view-new mode: mark the row viewed (bit 7),
        // drop the unread badge, and a shipment reminder from the Emperor's
        // envoy (person 0x0b, location byte 2/3) acknowledges the demand
        // (ds:bf bit 0x20).
        if self.comm_list_filter_seen == 0 {
            self.comm_sightings[pos] |= 0x80;
            self.comm_unread_count_ds_c9 = self.comm_unread_count_ds_c9.wrapping_sub(1);
            if person == 0x0b && ((word >> 8) as u8).wrapping_sub(2) < 2 {
                self.spice_shipment_flags |= 0x20;
            }
        }
        // = seg000:2956..296f a different sender than the displayed face:
        // dismiss the old face, start the message voice clip (person + 0x1a)
        // and show the new face.
        if person != self.comm_displayed_message_person {
            self.comm_dismiss_message_face();
            self.duck_music_and_start_narration_voice_clip(person as u16 + 0x1a);
            self.comm_show_message_face(person);
        }
        // = seg000:2970 [for_condit_ds_e9] = the sender — the message
        // dialogue conditions select the text from it (and ds:24).
        self.for_condit_ds_e9 = person;
        // = seg000:2973..2979 park pending_room_screen_request across the
        // dialogue + menu push.
        let saved_request = std::mem::take(&mut self.pending_room_screen_request);
        // = seg000:297a call present_room_person_dialogue — the sender
        // delivers the message line.
        self.present_room_person_line(person);
        // = seg000:297d/2980 clear elements 18..20, then re-arm the game-area
        // hotspot (a click there acts as Viewed).
        self.main_ui_elements_clear_flags_18_19_20();
        self.ui_elements[20].flags = 0x80;
        // = seg000:2985..298b push the Viewed menu (cleanup loc_02997).
        self.stage_command_submenu(
            MenuRef::MenuCommsRoomMessagesViewed,
            GameState::comm_viewed_cleanup,
        );
        // = seg000:298e/298f restore pending_room_screen_request.
        self.pending_room_screen_request = saved_request;
    }

    // = seg000:28a1 comm_show_message_face — record the sender, pulse the
    // glow in, wait out the message clip, then transition 8 to the face
    // still.
    fn comm_show_message_face(&mut self, person: u8) {
        // = seg000:28a1 [comm_displayed_message_person] = al.
        self.comm_displayed_message_person = person;
        // = seg000:28a4/28a7 the glow pulse over the current screen.
        self.comm_fade_in_glow();
        gfx::gfx_copy_whole_framebuf_to_screen(self);
        // = seg000:28aa wait out the person + 0x1a voice clip.
        self.wait_for_narration_voice_clip();
        // = seg000:28ad..28b2 transition 8 to comm_draw_message_face.
        self.transition(8, 0, |s| s.comm_draw_message_face());
    }

    // = seg000:28b5 comm_draw_message_face — draw the sender's face: COMM
    // sprite person + 0x1e at the data_0225d position, play SNA.VOC, and
    // store the position into the character x/y tables so the face is
    // click-targetable.
    fn comm_draw_message_face(&mut self) {
        // = seg000:28b5/28b8 open COMM.HSQ.
        self.open_sprite_bank(sprite_bank::COMM);
        // = seg000:28bb/28bd play SNA (the DAT carries it HSQ-compressed).
        self.audio_start_voc("SNA.HSQ");
        // = seg000:28c0..28cf the per-person position (x = low byte, y =
        // high byte).
        let person = self.comm_displayed_message_person as usize;
        let (x, y) = COMM_FACE_POS.get(person).copied().unwrap_or((0, 0));
        // = seg000:28d4..28da the character x/y tables (47f8/47fa).
        if let Some(slot) = self.character_screen_pos.get_mut(person) {
            *slot = (x as u16, y as u16);
        }
        // = seg000:28d1/28de sprite person + 0x1e at (x, y).
        self.draw_active_bank_sprite(person as u16 + 0x1e, x, y);
    }

    // = seg000:28e1 comm_dismiss_message_face — take the displayed face down:
    // the Harkonnen attack report (ds:24 == 0x0c) first arms the type-7 room
    // request; then transition 8 back to the room scene and fade the glow
    // out.
    fn comm_dismiss_message_face(&mut self) {
        // = seg000:28e1..28e8 the loc_0215f consequence hook.
        if self.for_dialogue_enemies_ds_24 == 0x0c {
            self.pending_room_screen_request = 7;
        }
        // = seg000:28eb no face up: nothing to dismiss.
        if self.comm_displayed_message_person == 0 {
            return;
        }
        // = seg000:28f2..28ff snapshot, then transition 8 through the room
        // redraw (callback_transition_02dd3).
        gfx::gfx_copy_whole_framebuf_to_screen(self);
        self.data_047a6 = 0xff;
        self.transition(8, 0, |s| s.draw_room_scene_and_present());
        // = seg000:2902/2907 clear the face and fade the glow out.
        self.comm_displayed_message_person = 0;
        self.comm_fade_out_glow();
    }

    // = seg000:2993 menu_callback_choice_comms_room_message_viewed — the
    // Viewed verb (also reached from a game-area click over the face):
    // return to the room with room action 6.
    pub(crate) fn menu_callback_choice_comms_room_message_viewed(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:2993 al = 6; jmp loc_02999.
        self.comm_return_to_room(6);
    }

    // = seg000:2997 comm_viewed_cleanup — the Viewed menu's cleanup func:
    // return with no room action.
    fn comm_viewed_cleanup(&mut self) {
        self.comm_return_to_room(0);
    }

    // = seg000:2999 comm_return_to_room — shared tail of the Viewed verb
    // (action = 6) and the menu cleanup (action = 0): mirror the unread
    // count into ds:eb and, if a face is up, tear the COMM view down and
    // rebuild the room verb panel.
    fn comm_return_to_room(&mut self, action: u8) {
        // = seg000:2999/299d ds:eb = ds:c9.
        self.comm_message_needs_viewing_ds_eb = self.comm_unread_count_ds_c9;
        // = seg000:29a1 no face displayed: done.
        if self.comm_displayed_message_person == 0 {
            return;
        }
        // = seg000:29a9..29af stop the sender's voice and drop the overlay
        // elements; draw on the visible screen.
        self.menu_npc_actions_cleanup();
        self.main_ui_elements_clear_flags_18_19_20();
        self.set_screen_as_active_framebuffer();
        // = seg000:29b2 re-draw the face still (straight to the screen this
        // time) and dismiss it back to the room scene.
        self.comm_draw_message_face();
        self.comm_dismiss_message_face();
        // = seg000:29b8 ds:24 = 0.
        self.for_dialogue_enemies_ds_24 = 0;
        // = seg000:29be record the room action (6 = messages viewed) for the
        // room-screen conditions.
        self.pending_room_action = action;
        // = seg000:29c1..29c7 rebuild the verb panel offscreen and re-present
        // the room scene.
        self.screen_overlay_request_transition();
        self.ui_draw_room_command_panel();
        self.draw_room_scene_and_present();
        // = seg000:29ca..29d1 unless someone spoke during the redraw, fold
        // the rebuilt panel in.
        if self.data_047a7 == 0 {
            self.play_pending_panel_fold();
        }
    }

    // = seg000:29d4 menu_callback_comms_messages_done — the list's trailing
    // Cancel row: narration clip 0x29, redraw the room scene into the front
    // buffer and fold back to the room verbs.
    pub(crate) fn menu_callback_comms_messages_done(&mut self, _text_id: u16, _index: usize) {
        // = seg000:29d4/29d7 narration clip 0x29.
        self.duck_music_and_start_narration_voice_clip(0x29);
        // = seg000:29da data_047a6 = 0xff — a full room redraw.
        self.data_047a6 = 0xff;
        // = seg000:29df/29e2 bp = loc_02dbf; render the scene reload with the
        // front buffer redirected to fb1.
        self.gfx_call_bp_with_front_buffer_as_screen(|s| s.draw_room_game_screen_scene_reload());
        // = seg000:29e5/29e8 fold the room verb panel back in.
        self.screen_overlay_request_transition();
        self.play_pending_panel_fold();
        // = seg000:29eb jmp wait_for_narration_voice_clip.
        self.wait_for_narration_voice_clip();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{GameState, dat_file::DatFile, menu_defs::MenuRef};

    fn asset_game() -> Option<GameState> {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return None;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        Some(game)
    }

    // Move the started game (palace entry hall) into the COMM room (palace
    // room 8) and seed `sightings` into the message list.
    fn enter_comm_room(game: &mut GameState, sightings: &[u16]) {
        for &s in sightings {
            game.comm_add_person_sighting(s);
        }
        game.location_and_room = (game.location_and_room & 0xff00) | 8;
        game.current_room = 8;
        game.ui_draw_room_command_panel();
    }

    // The COMM verbs (build_room_command_records, seg000:2f58..2f97) and the
    // full viewer flow: VIEW NEW MESSAGES -> a sender row -> Viewed.
    // Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored comm_message_viewer
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn comm_message_viewer_flow() {
        let Some(mut game) = asset_game() else { return };
        // Two sightings: the Baron seen at location 0x14, then the Emperor's
        // envoy at location 1 (the newest entry lists first).
        enter_comm_room(&mut game, &[0x1409, 0x010b]);
        assert_eq!(game.data_000c8, 2);
        assert_eq!(game.comm_unread_count_ds_c9, 2);
        // = seg000:2f6d..2f83 the console shows the new-message sprite.
        assert_eq!(game.scene_records[7].background, 0x27);
        // The verbs: VIEW NEW MESSAGES live, Messages already seen greyed
        // (nothing viewed yet).
        let recs = game.command_menu_buf.records.clone();
        assert!(
            recs.iter()
                .any(|r| r.text_id == crate::cmd::VIEW_NEW_MESSAGES)
        );
        assert!(
            recs.iter().any(
                |r| r.text_id == crate::cmd::MESSAGES_ALREADY_SEEN | crate::menu_defs::CMD_GREY
            )
        );

        // VIEW NEW MESSAGES: the sender list builds into menu_dynamic —
        // newest first, then the "  Cancel" row.
        game.menu_callback_choice_comms_room_view_new_messages(0, 0);
        assert_eq!(game.get_active_menu_ref(), MenuRef::MenuDynamic);
        let rows: Vec<u16> = game
            .menu_dynamic
            .records
            .iter()
            .map(|r| r.text_id)
            .collect();
        assert_eq!(rows, vec![0x78 + 0x0b, 0x78 + 0x09, 0xa3]);

        // Click the first row (the envoy, sighting 0x010b): viewed mark +
        // badge drop, the face display, and the Viewed menu.
        game.dispatch_command_menu_slot(0);
        assert_eq!(game.comm_sightings, vec![0x1409, 0x018b]);
        assert_eq!(game.comm_unread_count_ds_c9, 1);
        assert_eq!(game.for_dialogue_enemies_ds_24, 1);
        assert_eq!(game.for_condit_ds_e9, 0x0b);
        assert_eq!(game.comm_displayed_message_person, 0x0b);
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuCommsRoomMessagesViewed
        );

        // Viewed: back to the room verbs, the face down, room action 6.
        game.dispatch_command_menu_slot(0);
        assert_eq!(game.get_active_menu_ref(), MenuRef::CommandMenuBuf);
        assert_eq!(game.comm_displayed_message_person, 0);
        assert_eq!(game.pending_room_action, 6);
        assert_eq!(game.comm_message_needs_viewing_ds_eb, 1);
        // With a viewed entry on file, Messages already seen is offered live.
        let recs = game.command_menu_buf.records.clone();
        assert!(
            recs.iter()
                .any(|r| r.text_id == crate::cmd::MESSAGES_ALREADY_SEEN)
        );
    }

    // The " Others..." scroll row (seg000:d3bb..d3d5 / d475..d489): eleven
    // list records page by four with a rewind back to the top.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn comm_message_list_scrolls_with_others_row() {
        let Some(mut game) = asset_game() else { return };
        // Ten sightings (persons 1..10 at locations 1..10) + Cancel = 11 rows.
        let sightings: Vec<u16> = (1..=10u16).map(|i| (i << 8) | i).collect();
        enter_comm_room(&mut game, &sightings);
        game.menu_callback_choice_comms_room_view_new_messages(0, 0);
        assert_eq!(game.menu_dynamic.records.len(), 11);
        // Page one: slots 0..3 are rows, slot 4 the " Others..." arrow with
        // more records ahead.
        assert_eq!(game.command_menu_more_slot, 4);
        assert_ne!(game.command_menu_more_state & 0x80, 0);
        // Clicking it advances a page (skip += 4)...
        game.dispatch_command_menu_slot(4);
        assert_eq!(game.menu_dynamic.skip, 4);
        game.dispatch_command_menu_slot(4);
        assert_eq!(game.menu_dynamic.skip, 8);
        // ...page three holds records 8..10 and the wrap-around arrow
        // (records exhausted, skip != 0), which rewinds to the top.
        assert_eq!(game.command_menu_more_slot, 3);
        assert_eq!(game.command_menu_more_state & 0x80, 0);
        game.dispatch_command_menu_slot(3);
        assert_eq!(game.menu_dynamic.skip, 0);
        assert_eq!(game.command_menu_more_slot, 4);
    }

    // The idle-room vision delivery (loc_02b2a -> messages_02ad8 ->
    // present_vision_message -> the loc_03542 purge): a queued message whose
    // sender stands in the room is delivered and purged after 0x32 idle
    // ticks.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn vision_message_idle_delivery() {
        let Some(mut game) = asset_game() else { return };
        // Visions enabled, past the first-vision phase.
        game.bitfield_paul_events |= 1;
        game.game_phase = 0x18;
        // Queue "A message has arrived in the palace." (0x201, sender class
        // 2) and put the sender in the room.
        game.queue_vision_message_without_location(0x201);
        assert_eq!(game.vision_messages, vec![(0x201, 0)]);
        game.persons_in_room |= 1 << 2;
        // Rewind the idle base past the 0x32-tick threshold.
        game.game_clock_tick_base = (game.game_ticks() as u16).wrapping_sub(0x40);
        game.idle_room_message_check();
        // Delivered: the queue is purged of the sender's messages and the
        // speaker's verb menu install ran (the data_047a7 latch).
        assert!(game.vision_messages.is_empty());
        assert_eq!(game.data_047a7, 1);
        assert_eq!(game.current_lip_sync_resource_id, 2);
    }
}
