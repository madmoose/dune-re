use crate::{
    CursorMode, CursorShapeId, DatFile, Equipment, Font, FontState, FrameBuffer, InputState,
    Location, Palette, Rect, SpriteSheet, TalkingHead,
    attack::AttackState,
    blit, cmd,
    frame_slot::FrameSink,
    game_ui::{MouseHandlers, ROOM_MOUSE_HANDLERS, UI_ELEMENTS_INIT, UiElement},
    gfx::{self, palette_flush},
    globe_renderer::GlobeRenderer,
    hnm::hnm_id_by_name,
    input::SharedInput,
    locations::LOCATIONS,
    map_renderer::MapRenderer,
    menu_defs::{self, MenuRef},
    midi::{self, Midi},
    mouse::{MOUSE_START_X, MOUSE_START_Y, SharedCursor},
    pcm_player::{self, PcmPlayer},
    recorder::Recorder,
    room_game_screen::{ROOM_PERSON_TABLE_INIT, RoomPerson},
    settings_ui::{SETTINGS_RECORDS_INIT, SettingsRecord},
    sprite::Sprite,
    sprite_bank::Banks,
    sprite_blitter,
    tablat::Tablat,
    travel_map_screen::MapLocationMarker,
    troops::{TROOPS, Troop},
};

/// Identifies one of the engine's pixel buffers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbId {
    /// = `_word_2D088_screen_buffer_seg` — the visible VGA buffer (DNVGA: 0xA000).
    Screen,
    /// = `_word_2D086_framebuffer_1_seg` — the primary offscreen compose buffer.
    Fb1,
    /// = `_word_2D08E_framebuffer_saved_seg` (fb2) — a saved clean copy of the
    /// scene, used to restore regions dirtied by sprites/cursor/the talking head.
    Saved,
    /// = `_word_2D0E2_framebuffer_back` — the globe/map scratch buffer. During a
    /// travel it holds the persistent flight minimap + trail, re-stamped over
    /// each decoded flight frame (hnm_present_flight_frame, seg000:4afd).
    Back,
}

/// = (loc_0e85c - travel_trail_ring) / 4 — the travel-trail ring capacity in
/// (longitude, latitude) pairs.
pub(crate) const TRAVEL_TRAIL_LEN: usize = (0xe85c - 0xe40c) / 4;

pub const PCM_OUTPUT_RATE: u32 = 49716;
pub const MIDI_SAMPLE_RATE: u32 = 49716;

/// Identifies a frame task. Dune identifies tasks by function pointer, but
/// function pointers aren't reliably comparable in Rust so we use an id.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum TaskId {
    // = seg000:0070c hnm frame player (intro_play_hnm_with_frame_task).
    HnmDoFrame,
    // = seg000:00b45 loc_00b45 — intro_28 night-attack particle tick.
    IntroNightAttack,
    // = seg000:099be loc_099be — talking-head idle animator.
    TalkingHeadIdle,
    // = seg000:0a7c2 lip_sync_frame_task — talking-head speech / mouth.
    TalkingHeadVoc,
    // = seg000:00826 loc_00826 — desert / midnight sky palette cycler.
    SkyPaletteCycler,
    // = seg000:03916 loc_03916 — one-shot sky palette fade (stage 29, runs
    // alongside the HNM player).
    SkyFade,
    // = seg000:0c0b6 room_frame_task - general room frame task
    Room,
    // = seg000:0ab92 frame_task_callback_0ab92 — after a ducked voice clip
    // starts, poll PCM playback each frame and release the music ducking once
    // it ends, then self-remove. In DOS this task also refilled the streaming
    // VOC (loc_0a9b9); the consolidated dnsdb driver owns the whole clip in the
    // port, so only the music-restore half remains.
    PcmVoiceMusicRestore,
    // = seg000:046b5 map_caption_frame_task — the map screen's "SELECT
    // DESTINATION ON MAP" typewriter: one glyph per firing (interval 0x18).
    MapCaption,
    // = seg000:044ab map_player_marker_blink_task — the blinking "you are
    // here" marker on the map view (interval 0x12c).
    MapPlayerMarker,
    // = seg000:0b9ae frame_task_callback_0b9ae — the globe rotation task
    // (interval 1): one outline row into fb1 per tick, present + one phase
    // step per finished pass.
    GlobeRotation,
    // = seg000:be57 results_gauge_task (interval 0xc).
    ResultsGauges,
    // = seg000:6b34 troop_icon_anim_task — the troop icon animation task on
    // the full map view (interval 15).
    TroopIconAnim,
    // = seg000:0a16 frame_task_callback_00a16 — the scrolling-credits step
    // (one CREDITS.HNM frame per tick), armed by the book's past-the-last-
    // page path (play_credits, seg000:09f5).
    CreditsScroll,
    // = seg000:176b frame_task_callback_blink — the scripted continue-
    // sequence's blink toggle (interval 0x64), installed by
    // start_scripted_dialogue.
    SequenceBlink,
}

pub(crate) struct FrameTask {
    interval: u16,
    accumulator: u16,
    task_id: TaskId,
}

/// = one of the seg001:00ca..00e6 nearest-location triples
/// condit_scan_nearest_locations (seg000:5274) maintains: the distance
/// (max(|dlon| >> 8, |dlat|), 0xffff = none found), the location's seg001
/// pointer and the compass octant toward it (0 = N .. 7 = NW).
#[derive(Clone, Copy)]
pub(crate) struct NearestLocation {
    pub(crate) distance: u16,
    pub(crate) loc_ptr: u16,
    pub(crate) octant: u8,
}

impl Default for NearestLocation {
    fn default() -> Self {
        // = the seg001 statics: distance 0xffff, ptr/octant 0.
        NearestLocation {
            distance: 0xffff,
            loc_ptr: 0,
            octant: 0,
        }
    }
}

/// Build a header-less Creative Voice File holding a single Type-1 data block.
fn build_pcm_voc(tc: u8, samples: &[u8]) -> Vec<u8> {
    let body_len = samples.len() + 2; // time-constant + codec
    let mut voc = Vec::with_capacity(4 + body_len);
    voc.push(1); // Type-1 sound-data block
    voc.push((body_len & 0xff) as u8);
    voc.push(((body_len >> 8) & 0xff) as u8);
    voc.push(((body_len >> 16) & 0xff) as u8);
    voc.push(tc);
    voc.push(0); // codec 0 = 8-bit unsigned PCM passthrough
    voc.extend_from_slice(samples);
    voc
}

pub struct GameState {
    headless: bool,

    // Port-only debug overlay: a text panel of live game state (game phase,
    // location, charisma, …) drawn over the presented frame. Toggled by the
    // backquote key (`). `debug_overlay_key_down` edge-detects the toggle.
    pub(crate) debug_overlay: bool,
    debug_overlay_key_down: bool,

    // Port-only testing hotkey: the `=`/`+` key bumps game_phase by one and
    // runs the usual phase triggers. `debug_advance_phase_key_down` edge-detects
    // the press so a held key advances only once.
    debug_advance_phase_key_down: bool,

    // Port-only: F5 opens the custom named save/load panel (save_screen.rs).
    // Edge-detects the press so a held key opens the panel only once.
    pub(crate) custom_save_key_down: bool,

    // ---- Host/runtime state and buffers (not seg001 data-segment globals) ----
    pub dat_file: DatFile,

    pub screen: FrameBuffer,
    pub screen_pal: Palette,

    // = segvga:01a3 fb_base_ofs — the game-area top. Stored here as a row; DOS
    // keeps the row*320 byte offset and applies it to every blit.
    pub y_offset: u16,
    // = seg001:2772 data_02772 — the 16-bit line pattern draw_line loads per
    // edge (seg000:c541); 0xffff = solid, 0x5555 = the overlay's dotted box.
    pub line_pattern: u16,
    pub framebuffer: FrameBuffer,
    // = _word_2D08E_framebuffer_saved_seg (fb2): a clean backup of the composed
    // scene; regions are restored from here under moving overlays. (The buffer
    // itself, not the seg001 selector word at seg001:dbde that points to it.)
    pub framebuffer_saved: FrameBuffer,
    /// = `_word_2D0E2_framebuffer_back` (FbId::Back) — the globe/map scratch
    /// buffer (the flight minimap + trail persist here during a travel).
    pub framebuffer_back: FrameBuffer,
    pub palette: Palette,
    pub palette_fade_target: Palette,
    pub global_frame_count: usize,

    // pub bank: Option<SpriteSheet>,

    // = the DNCHAR.BIN font glyphs + width tables (seg000:cfe4 loads resource
    // 0xbb into the seg001:0ceec buffer). `font_state` mirrors the seg001
    // font-draw globals (pen position, colour, selected font) the font_*
    // routines maintain. See font.rs.
    pub font: Font,
    pub font_state: FontState,

    // = _dword_23C5C_COMMANDx_BIN — the COMMAND1.BIN command-string table
    // (seg000:d003 loads resource 0xc0 + language). A head table of word offsets
    // (count = word[0]/2) followed by 0xff-terminated strings; the verb panel
    // resolves verb text from it via get_phrase_or_command_string_si.
    pub command_bin: Box<[u8]>,

    // = the active talking-head portrait (intro + dialogue lip-sync). None when
    // no head is on screen. See `talking_head.rs`.
    pub talking_head: Option<TalkingHead>,

    // The SD (digital-audio) chunk captured by the most recently decoded HNM
    // frame, awaiting wrap into a VOC by the audio orchestration. = the streaming
    // decoder's last_sd_block.
    pub(crate) hnm_sd_block: Option<Vec<u8>>,
    pub hnm_ticks_per_frame: u64,
    pub hnm_last_frame_tick: u64,
    // hnm_y_offset is not in the original, Dune decodes to an offset by
    // manipulating the frame buffer pointer.
    pub hnm_y_offset: i16,

    // PCM-driven frame timing. While the current clip carries SD audio,
    // hnm_do_frame waits until the dnsdb driver has picked up the previously
    // queued buffer (`pcm_player.queue_slot_filled()` clears) before advancing
    // — mirroring the DOS hnm_wait_for_frame loc_0caf0 path, where each HNM
    // frame advances only after the Sound Blaster has drained the previous PCM
    // buffer (the job-state byte `[si+6]`). When `hnm_audio_active` is false the
    // clip has no audio and falls back to the fixed tick-per-frame path.
    pub(crate) hnm_audio_active: bool,
    // = the time constant captured from the first frame's SD VOC; later frames
    // carry raw samples that reuse it (the persistent job-buffer header in
    // copy_sd_chunk_to_pcm_buf, seg000:aa70).
    pub(crate) hnm_audio_tc: u8,

    // `Midi` owns its CPAL stream + audio thread internally. All digital audio
    // (standalone voices and HNM video sound) runs through the single dnsdb
    // driver `pcm_player`, which owns its own CPAL output stream — matching the
    // original, where one PCM driver served both.
    pub(crate) midi: Midi,

    pub(crate) pcm_player: PcmPlayer,

    // The clip recorder, kept here so the in-game EXIT GAME path (`exit_to_dos`)
    // can finalise a recording before `std::process::exit` skips all destructors.
    pub(crate) recorder: std::sync::Arc<Recorder>,

    pub(crate) game_start: std::time::Instant,
    pub(crate) frame_sink: Box<dyn FrameSink>,

    // Where the cursor sprite gets composited. `Baked` runs the DOS
    // `vga_draw_cursor` / `vga_restore_cursor` pair on the game thread;
    // `Overlay` skips that and lets the present thread draw the cursor
    // sprite on the GPU using the freshest pointer position. This is the
    // *active* mode — it is forced to `Baked` while recording so the cursor
    // lands in the captured framebuffer (see `sync_recording_cursor_mode`).
    pub(crate) cursor_mode: CursorMode,
    // The cursor mode selected on the command line; `cursor_mode` is restored to
    // this when a recording stops.
    pub(crate) base_cursor_mode: CursorMode,
    // Shape + visibility published by `redraw_mouse` when `cursor_mode ==
    // Overlay`, sampled by the present thread once per redraw.
    pub(crate) shared_cursor: SharedCursor,

    // Shared keyboard + mouse state, written by the host event loop (the DOS
    // keyboard ISR + INT 33h driver equivalent, see `input` module) and polled
    // by any_key_pressed. A headless `GameState::new` gets its own idle
    // instance; the windowed binary hands in the same handle its event loop fills.
    pub(crate) input: SharedInput,
    // = the `si` previous-mouse-buttons value any_key_pressed edge-detects
    // against (= seg000:dd80 `xor bx,si; and bx,si`): a held button registers as
    // input only on the press transition, not every poll.
    pub(crate) prev_mouse_buttons: u8,

    // Set when a keypress during a play_intro stage requests aborting the whole
    // intro. DOS carries this as the CF returned by each stage's play function /
    // wait_for_pcm_voice_interruptable (seg000:05ef/05fb jb loc_005fd); the port
    // records it here and play_intro breaks the stage loop on it.
    pub(crate) intro_aborted: bool,

    // Set when ESC (specifically — kb_esc_was_hit) is pressed anywhere in the
    // intro sequence: it skips past play_credits and play_intro2 straight into
    // the game, whereas a non-ESC key or the mouse only ends the current phase.
    // = the DOS ZF(esc) threaded play_intro -> play_credits -> play_intro2 via
    // each function's jz-at-entry (seg000:0309/0226). start() resets it.
    pub(crate) intro_skip_to_game: bool,

    // = segvga:2768 transition_col / segvga:276a transition_frame — the
    // wipe-transition engine's running state, advanced one step per call by
    // transition_tick (gfx::transition_tick). Static-init to col=8, frame=1.
    // room_frame_task (tick_room) steps this to time the cave water-drip sound.
    pub(crate) transition_col: u16,
    pub(crate) transition_frame: u16,

    pub(crate) attack: Option<AttackState>,

    // ---- seg001 data-segment globals (sorted by address) ----

    // = seg001:0000 rand_bits — the last word `rand` returned. game_loop
    // refreshes it every pass; seg001:0000 also serves as the seg001 segment
    // base, so most `rand_bits[si]` references in the disasm are addressing
    // other globals at non-zero offsets, not reading this word.
    pub(crate) rand_bits: u16,

    // = seg001:0002 game_time — the in-game clock (16 ticks per day; the low
    // nibble is the time-of-day phase). Static-initialised to 2 (seg001:0002
    // `dw 2`), which is also the value play_intro re-seeds at its exit and
    // start re-seeds again at seg000:001e. The PIT game-clock ISR (not ported)
    // advances it. get_ingame_day_in_ax reads (game_time+3)>>4.
    pub(crate) game_time: u16,

    // = seg001:0004 location_and_room — the current scene's (location<<8)|room
    // code (the DOS `dx`). draw_location_room records it here; loc_0d41b reads
    // it back via the room navigation stack (get_location_and_room), and
    // add_room_frame_task gates on it.
    pub location_and_room: u16,

    // = seg001:0006 data_00006 — the current location slot/index (static init
    // 0x180). open_SAL_resource (loc_008f0) sets it from bx; its high byte picks
    // the location's apparence (which SAL file to draw). = `location_appearance` passed
    // to draw_location_room.
    pub location_appearance: u16,

    // = seg001:0008 data_00008 — current room/apparence selector byte (static
    // init 0x20). draw_room_scene and draw_room_game_screen treat 0xff as "no
    // room scene to draw"; the desert walk-out (loc_03fd2) sets it to 0xff and
    // the walk-in arrival (arrive_at_location) restores the location code.
    pub(crate) data_00008: u8,

    // = seg001:0009 data_00009 — the current location slot byte (the
    // location_appearance high byte), 0xff while out in the desert. Written
    // alongside data_00008 by the walk-out/arrival paths; the NPC shuffle
    // (iterate_over_allied_NPCs_and_locations, not ported) reads it.
    pub(crate) data_00009: u8,

    // = seg001:000a bitfield_Paul_events — Paul's story-progress bitfield. Bit 0x10
    // gates the person-0x0e dialogue verb (seg000:90ed: 0x96 vs 0x97).
    pub(crate) bitfield_paul_events: u8,

    // = seg001:000b current_room — the room byte of the room the player is in
    // (static init 0x0a, the palace throne room). ui_click_move_room's commit
    // (loc_04057, seg000:4060) rotates it into previous_room; its == 1 check at
    // seg000:3f72 marks "leaving the location's entry room".
    pub(crate) current_room: u8,

    // = seg001:000c pending_destination_room — the pending destination room ui_click_move_room
    // records (seg000:3faa) before the room-leave dialogue scan; CONDIT conditions
    // read it through the ds window (e.g. condition 0x1c gates Leto's "where are
    // you going so fast" on pending_destination_room == 4, the throne-room DOWN exit).
    pub(crate) pending_destination_room: u8,

    // = seg001:000d previous_room — the room byte the player came from, written
    // by the move commit (seg000:4064).
    pub(crate) previous_room: u8,

    // = seg001:000e _word_1F4BE_persons_met — heads the contiguous persons array
    // (persons_met, persons_travelling_with, persons_in_room, persons_talking_to
    // at +0/+2/+4/+6). draw_room_game_screen indexes it by data_047aa to pick the
    // speaker whose lip-sync to start.
    pub(crate) persons_met: u16,

    // = seg001:0010 persons_travelling_with — which persons travel with the
    // player.
    pub(crate) persons_travelling_with: u16,

    // = seg001:0012 persons_in_room — which persons stand in the current room.
    pub persons_in_room: u16,

    // = seg001:0014 _word_1F4C4_persons_talking_to — the person the player is
    // currently in dialogue with.
    pub(crate) persons_talking_to: u16,

    // = seg001:0019 line_spoken_this_conversation — a "has a dialogue line been
    // spoken this conversation" flag: 0 when set_dialogue_speaker starts a
    // conversation (seg000:9417), 0xff once any dialogue line is presented
    // (fire_dialogue_line_event, seg000:a092). A fallback dialogue line tests
    // this == 0 in its CONDIT condition, so the fallback presents only when no
    // other line was presentable this conversation.
    pub(crate) line_spoken_this_conversation: u8,

    // = seg001:001b related_to_stay_here_come_with_me_ds_1b — counts the
    // COME WITH ME / STAY HERE verb uses since the last TALK TO ME (which
    // clears it, seg000:947a).
    pub(crate) data_0001b: u8,

    // = seg001:0023 pending_room_action — the room-transition / dialogue-scan state.
    // ui_click_move_room sets it to 1 to request the room-leave auto-dialogue scan
    // (run_room_leave_dialogue_scan gates on it and clears it), CONDIT condition 0x1c tests it == 1,
    // and the committed move sets it to 5. The dialogue verbs also stage
    // outcome codes here for their record's conditions to read (the Fremen
    // chief's WORK WITH ME charisma check, seg000:95de: 0 pass / 2 refuse).
    pub(crate) pending_room_action: u8,

    // = seg001:0025 number_of_sietches_visited — counts first visits to
    // locations with a code below 0x20 (the sietches)
    pub(crate) number_of_sietches_visited: u8,

    // = seg001:0026 entering_new_sietch — 0xff while the player's first in-room
    // move inside a freshly visited location is being committed
    pub(crate) entering_new_sietch: u8,

    // = seg001:0027 discovered_sietch_count — counts sietches whose location
    // record lost its undiscovered bit (location_mark_discovered).
    pub(crate) discovered_sietch_count: u8,

    // = seg001:0028 number_of_rallied_troops — how many Fremen troops have
    // been rallied to the Atreides cause. The troop system that maintains it
    // (troop_rally_troop_066ce) is not yet ported, so it only changes if set
    // externally; CONDIT conditions (e.g. Leto's early-game mission lines)
    // read it.
    pub(crate) number_of_rallied_troops: u8,

    // = seg001:0029 charisma — Paul's charisma stat (capped at 0xc8 by
    // increase_charisma_and_increase_troop_motivation_accordingly).
    pub(crate) charisma: u8,

    // = seg001:002a _byte_1F4DA_game_phase — the global story-progress counter.
    pub(crate) game_phase: u8,

    // = seg001:002b night_attack_stage.
    pub(crate) night_attack_stage: u8,

    // = seg001:00a0 spice_in_stock — the palace spice stock, stored in batches
    // of 10 kg (a value of 123 is 1230 kg; Duncan's sign appends a "0" to show
    // it in kg). The mining troops pay their whole harvest into it each time
    // period, divided by 10 to convert kg to batches (seg000:701b), the
    // sub-batch kg carried in spice_harvest_remainder.
    pub(crate) spice_in_stock: u16,

    // = seg001:00a2/00a4 for_condit_area_controlled_by_Atreides/Harkonnen —
    // the map-wide territory percentages compute_area_controlled_percentages
    // (seg000:bfe3) derives from the vegetation-stage bits each new day.
    pub(crate) area_controlled_by_atreides: u16,
    pub(crate) area_controlled_by_harkonnen: u16,

    // = seg001:00a6 for_condit_todays_spice_production_ds_a6 — today's spice
    // production: stock + spice_spent_today - stock_at_last_new_day, clamped
    // at 0 (seg000:1c6e); recompute_condit_statistics keeps the running max
    // within the day.
    pub(crate) todays_spice_production: u16,

    // = seg001:00a8 for_condit_potential_spice_harvest_ds_a8 — sum of
    // spice_density/8 over Atreides locations + rand_iterated(sum/16)
    // (seg000:1cda).
    pub(crate) potential_spice_harvest: u16,

    // = seg001:00aa data_000aa — total population of the troops that are
    // neither Harkonnen nor captured/unrallied (the recompute_condit_
    // statistics scan, seg000:c049).
    pub(crate) data_000aa: u16,

    // = seg001:00ac data_000ac — total population of the Harkonnen-flagged
    // troops (the seg000:c049 scan sums troop byte +0x1a into ds:ac for
    // troops with byte +0x10 bit 0x80, else into ds:aa); static init 0x1b58
    // (7000). Gates the Fremen WORK WITH ME charisma check (seg000:95c4).
    // recompute_condit_statistics refreshes it each new day.
    pub(crate) data_000ac: u16,

    // = seg001:00ae for_condit_previous_day_spice_production_ds_ae — the
    // previous day's production total, exchanged out by the new-day hook
    // (seg000:1c87) to derive the better/lower pair.
    pub(crate) previous_day_spice_production: u16,

    // = seg001:00b0/00b2 for_condit_spice_production_better/lower_than_
    // previous_day — |production - previous|, one of the pair, the other 0
    // (seg000:1c96).
    pub(crate) spice_production_better_than_previous_day: u16,
    pub(crate) spice_production_lower_than_previous_day: u16,

    // = seg001:00bc/00be/00bf the Emperor's spice-shipment demand state:
    // ds:bc the demanded quantity, ds:be the fulfilment fraction (bit 7 =
    // none paid), ds:bf the flags (bit 7 = the shipment plot armed, bit 4 =
    // a demand pending). actions_time_in_day_3 (seg000:20a4) rolls the
    // demands; the payment flow (Duncan/CHOAM dialogue) is unported.
    pub(crate) spice_shipment_quantity: u16,
    pub(crate) spice_shipment_fulfilment: u8,
    pub(crate) spice_shipment_flags: u8,

    // = seg001:00c2 final_attack_stage_ds_c2 — the endgame attack-on-the-
    // Harkonnen staging counter; from stage 7 the per-period troop and
    // location event walks stop (seg000:1b5e). The endgame that advances it
    // is unported.
    pub(crate) final_attack_stage: u8,

    // = seg001:00c3 spice_shipment_sequence_number_ds_c3 — counts the
    // Emperor's demands; the quantity formula scales with it (seg000:20d2).
    pub(crate) spice_shipment_sequence_number: u8,

    // = seg001:00c4 number_of_sietches_attacked_by_Harkonnen_ds_c4.
    pub(crate) number_of_sietches_attacked_by_harkonnen: u8,

    // = seg001:00c5 person_marker_base — random base offset for arranging the
    // people standing in a room. Set to rand() at room setup (the arrival
    // handler in tick_in_game_travel, seg000:4fc6), reset to 0 on scene change
    // (seg000:02a2). sal_position_markers reads its low nibble as the `base` in
    // preferred slot = (person_id + base) % count.
    pub(crate) person_marker_base: u8,

    // = seg001:00c6 data_000c6 (book_flags) — the book-screen flags, doubling
    // as the subtitle-suppress gate (any nonzero value makes
    // present_first_matching_dialogue_line skip show_voice_subtitle, and
    // run_game_phase_triggers sets bit 0x80 around the phase-trigger walk).
    // Book bits: 1 = book screen active, 2 = showing the cover, 4 = credits
    // rolling past the last page.
    pub(crate) data_000c6: u8,

    // = seg001:00c8 data_000c8 — DOS's comm_sighting_count byte, kept in
    // step with comm_sightings (comm_add_person_sighting); the COMM-room
    // verbs read it (build_room_command_records, dl==8). Inits to 0.
    pub(crate) data_000c8: u8,

    // = seg001:00c8 comm_sighting_count + seg001:1179 comm_sighting_list —
    // the COMM-room person-sighting words ((location index << 8) | person
    // id), max 10, appended by comm_add_person_sighting. The COMM screen
    // that displays them is unported.
    pub(crate) comm_sightings: Vec<u16>,

    // = seg001:00cf days_left_until_spice_shipment — the CONDIT day counter
    // actions_time_in_day_3 maintains while a demand date is ahead.
    pub(crate) days_left_until_spice_shipment: u8,

    // = seg001:00d5 contact_distance_related_ds_d5 — incremented once per
    // day, but only stored back from 2 up (seg000:1c62), so it stays at its
    // initial value until something else moves it to 1.
    pub(crate) contact_distance_related_ds_d5: u8,

    // = seg001:00e1 data_000e1 — the fly-over side flag set by
    // travel_scan_nearby_location (seg000:4156): 0 when the passed location is
    // to the left of the heading, 1 when to the right. Feeds the companion's
    // fly-over dialogue line (the spoken-line tail is not ported yet).
    pub(crate) data_000e1: u8,

    // = seg001:00e8 _byte_1F598_ui_hud_head_index.
    pub(crate) ui_hud_head_index: u8,

    // = seg001:00ea data_000ea (signed).
    pub(crate) data_000ea: i8,

    // = seg001:00ed/00ee for_condit_related_to_overpowering_Harkonnen_captain
    // — seeded by the captain classification (0xff when surrendered, else the
    // troop's motivation; the pair word), consumed by the OVERPOWER THE
    // PRISONER flow (seg000:9584).
    pub(crate) data_000ed: u8,
    pub(crate) data_000ee: u16,

    // = seg001:00f4 desert_walk_counter — counts compass moves taken in the
    // desert.
    pub(crate) desert_walk_counter: u8,

    // = seg001:00f5 for_condit_desert_walk_related_ds_f5 — cleared with the
    // counter when the per-period countdown drops below 0x10 (seg000:1b36);
    // Jessica's desert dialogue reads it.
    pub(crate) for_condit_desert_walk_ds_f5: u8,

    // = seg001:00f8 number_of_locations_with_illness / seg001:00f9
    // Chani_troop_illness_cure_progress / seg001:11db PTR_Location_latest_
    // location_with_illness — the phase-5c/5d illness-cure subplot state:
    // the picker (seg000:1e43) makes the strongest non-fortress ill, Chani
    // parked there advances the cure by 8 per period until it wraps to 0
    // (seg000:1eda). The latest-ill pointer keeps the DOS location-ptr
    // encoding (0 = none).
    pub(crate) number_of_locations_with_illness: u8,
    pub(crate) chani_troop_illness_cure_progress: u8,
    pub(crate) latest_location_with_illness: u16,

    // = seg001:00fb data_000fb — toggle between the room/dialogue view and the
    // globe/map view (static init 0xff). ui_toggle_room_view negs it each call:
    // a non-negative result shows the room view, a negative one the map.
    pub(crate) room_view_toggle: u8,

    // = seg001:00fc data_000fc — a constant early-game flag (static
    // init 1, no DOS writers); CONDIT condition 1 (`byte ds:[fc]`) gates the
    // first greeting on it.
    pub(crate) data_000fc: u8,

    // = seg001:00fe game_phase_copy_ds_fe — the new-day hook's copy of
    // game_phase; a mismatch resets days_since_last_game_phase_change
    // (seg000:1c46).
    pub(crate) game_phase_copy_ds_fe: u8,

    // = seg001:00ff number_of_days_since_last_game_phase_change_ds_ff — zeroed
    // on every phase change (the event-0x0b callback and
    // set_game_phase_and_trigger_callbacks) and incremented by the new-day
    // hook (run_events_new_day, seg000:1c46).
    pub(crate) days_since_last_game_phase_change: u8,

    // = seg001:0100 locations.
    pub(crate) locations: [Location; 70],

    // = seg001:08aa troops.
    pub(crate) troops: [Troop; 68],

    // = seg001:11bf book_bookmark_ptr (data_011bf) — the book's bookmark: the
    // cs offset of the current page word in the dialogue-played log (0xaa =
    // the first entry); persists while the book is closed.
    pub(crate) book_bookmark_ptr: u16,

    // = seg001:2406 book_topic_filter (data_02406) — the active book topic
    // filter (low byte = record mask 0x1c, high byte = topic bits); 0 = all
    // topics.
    pub(crate) book_topic_filter: u16,

    // = seg001:243e book_page_video_id (data_0243e) — the HNM resource id
    // (0x19..0x24) of the bookmarked page's video, 0 when the page has none.
    pub(crate) book_page_video_id: u16,

    // = seg001:4756 fremen1_troop_ptr — the troop behind the room's Fremen-1
    // person (room_persons[14], the rallied-troop chief), as a troops index.
    pub(crate) fremen1_troop: Option<usize>,

    // = seg001:4758 fremen2_troop_ptrs — up to 8 troops behind the room's
    // Fremen-2 person (room_persons[15]), filled round-robin (data_0476a) by
    // the room-entry classification.
    pub(crate) fremen2_troops: [Option<usize>; 8],

    // = seg001:4768 harkonnen_captain_troop_ptr — the troop behind the room's
    // Harkonnen captain (room_persons[12]).
    pub(crate) harkonnen_captain_troop: Option<usize>,

    // = seg001:476c selected_fremen2_index — which fremen2_troop_ptrs slot
    // the active Fremen-2 conversation (or room draw) refers to.
    pub(crate) selected_fremen2: u8,

    // = seg001 vegetation_started_on_Dune — the ecology-victory flag the
    // motivation modifier reads; the event that sets it is not yet ported.
    pub(crate) vegetation_started_on_dune: u8,

    // = the for_condit troop staging block (seg001:002c..004b), filled by
    // troop_prepare_troop_data_for_condit.
    pub(crate) troop_condit: crate::troops::TroopCondit,

    // = the for_condit location staging block (seg001:004d..005b), filled by
    // prepare_location_data_for_condit.
    pub(crate) location_condit: crate::troops::LocationCondit,

    // = seg001:11d3 ARRAY_PTR_Location_prospector_destinations — the
    // prospector troop's (troops[2]) queue of destination location ptrs;
    // its arrival at the head shifts the queue (seg000:8347). The FIND
    // PROSPECTORS flow that fills it is not yet ported.
    pub(crate) prospector_destinations: [u16; 4],

    // = seg001:11ce data_011ce — the locations[] index whose CONDIT block is
    // currently staged (prepare_location_data_for_condit records it; static
    // init 0x100 = locations[0]). The event scheduler re-stages it after the
    // per-period events may have staged other locations (seg000:1b85).
    pub(crate) condit_staged_location: usize,

    // = seg001:114e current_location_ptr — the locations[] index of the
    // location the player is currently inside. Recomputed on every scene open
    // (loc_008f0, the port's draw_location_room) and set on walk-in arrival
    // (arrive_at_location).
    pub(crate) current_location_index: u16,

    // = seg001:1150 last_location_ptr — the locations[] index of the location
    // the player is at or last left (static init 0x100 = locations[0], the
    // Atreides palace). Set on walk-in arrival (arrive_at_location); unlike
    // current_location_ptr it is NOT cleared when walking out into the desert,
    // so the desert renderer (draw_outdoor_backdrop) can still see the nearby
    // location.
    pub(crate) last_location_index: usize,

    // = seg001:1152 ui_hud_companion_1 / seg001:1153 ui_hud_companion_2 — the
    // person index shown in each of the two bottom-left HUD companion
    // portraits (-1 = empty). Filled/cleared by npc_assign_companion_slot /
    // npc_remove_companion_slot when a dialogue closes.
    pub(crate) companions: [i16; 2],

    // = seg001:1154 harkonnen_raids_armed_after_game_time — game_time
    // snapshot taken by the phase-0x2c (met Stilgar) callback; the raid
    // scheduler (actions_time_in_day_4, seg000:1f6e) arms once game_time has
    // passed it by 0x70.
    pub(crate) harkonnen_raids_armed_after_game_time: u16,

    // = seg001:1156 illness_plot_armed_after_ingame_day — an in-game-day
    // deadline (day + 3) armed by the phase-0x5c callback; the illness
    // picker (seg000:1e43) fires from that day on.
    pub(crate) illness_plot_armed_after_ingame_day: u16,

    // = seg001:1170 spice_stock_at_last_new_day / seg001:1172
    // spice_spent_today — the new-day production diff pair (seg000:1c6e):
    // production = stock + spent - stock at last new day. Smuggler purchases
    // and shipments add what they deduct from the stock to ds:1172.
    pub(crate) spice_stock_at_last_new_day: u16,
    pub(crate) spice_spent_today: u16,

    // = seg001:118d ingame_day_of_last_spice_shipment_event — the day the
    // current shipment demand was rolled; the day-3 action measures the
    // reminder/consequence days from it.
    pub(crate) ingame_day_of_last_spice_shipment_event: u16,

    // = seg001:11bb data_011bb — the unpaid-shipment flag: nonzero routes
    // the day-3 action straight to the room-screen type-7 consequence
    // (seg000:20bc). Its writers (the payment flow) are unported.
    pub(crate) spice_shipment_unpaid: u8,

    // = seg001:11bc harkonnen_raid_suppress_once — nonzero suppresses the
    // next raid check; consumed (cleared) by actions_time_in_day_4
    // (seg000:1f83).
    pub(crate) harkonnen_raid_suppress_once: u8,

    // = seg000:65b4 ecology_lfsr_state — the persistent 16-bit LFSR state
    // (taps 0x402, static init 1) of the daily vegetation-promotion walk
    // (seg000:65b6).
    pub(crate) ecology_lfsr_state: u16,

    // = seg001:10d8 smugglers — the six smuggler inventories (region,
    // haggling, stock and prices); the new-day hook restocks them
    // (seg000:1cae).
    pub(crate) smugglers: [crate::smugglers::Smuggler; 6],

    // = seg001:1178 number_of_rallied_troops_for_Leto_being_killed — the
    // rallied-troop threshold armed by the phase-0x48 (met Chani) callback
    // (rallied + 2); 0xff (the static value) = not armed. Its reader (the
    // Leto-killed event pump) is not yet ported.
    pub(crate) number_of_rallied_troops_for_leto_killed: u8,

    // = seg001:1190 vision_message_count + seg001:1191 vision_message_queue —
    // the queued vision messages, (message id, location ptr or 0), max 10;
    // queue_vision_message appends (deduplicated, oldest dropped on
    // overflow). The vision presentation that consumes them is unported.
    pub(crate) vision_messages: Vec<(u16, u16)>,

    // = seg001:2222 ui_hud_companion_blink — per-companion-slot blink countdown
    // bytes: npc_assign_companion_slot arms 0x10 on the filled slot (8 blinks),
    // npc_remove_companion_slot clears the vacated one, and the game-loop
    // blink task (ui_hud_companion_blink_task) drains them.
    pub(crate) ui_hud_companion_blink: [u8; 2],

    // = seg001:dcf1 companion_blink_step_latch — the blink task's pacing
    // latch: the last-seen (game_ticks >> 6) & 0xff step number, so the task
    // fires once per 64 PIT ticks.
    pub(crate) companion_blink_step_latch: u8,

    // = seg001:11bc data_011bc — scene flag set (|= 1) by the night-attack
    // branch of draw_room_game_screen.
    pub(crate) data_011bc: u8,

    // = seg001:11c5 travel_destination_ptr — the pending/active travel
    // destination location (locations::location_ptr encoding; 0 = none). Set
    // by arm_pending_travel; map_screen_cleanup keeps game_screen_mode_flags
    // while it is set; the per-step re-aim (loc_051cb) and the arrival
    // (seg000:4fd8) read it — both travel-pump territory, not ported.
    pub(crate) travel_destination_ptr: u16,

    // = seg001:11c7 travel_heading — the travel compass heading (0 north,
    // clockwise, 0x20 per compass point). Seeded by arm_pending_travel;
    // re-aimed at the destination each step when travel_heading_mode == 0
    // (loc_051cb); reversed by BACK TO STARTING POINT (seg000:526a).
    pub(crate) travel_heading: u8,

    // = seg001:11c8 travel_heading_mode — 1 = fixed compass heading (a
    // desert-cell click); 0 = home toward travel_destination_ptr, re-aiming
    // each step (loc_051cb).
    pub(crate) travel_heading_mode: u8,

    // = seg001:11c9 game_screen_mode_flags — bitfield selecting the active
    // non-room screen/mode (book/map/dialogue/...); 0 = the plain room view.
    // draw_room_game_screen branches on bits 0..1 (mask 3) and on ==0.
    pub(crate) game_screen_mode_flags: u8,

    // = seg001:11ca data_011ca — set during a pending room-screen swap (between
    // pending_room_screen_request being raised and loc_00d8e finishing the
    // transition); travel_pump (seg000:4f0c) bails when set so it does not race the swap.
    pub(crate) data_011ca: u8,

    // = seg001:11cb travel_no_location_dest — 0xff when the travel has no
    // location destination: a directional flight on a fixed compass heading
    // across open desert (fly east/west/etc), where travel_destination_ptr holds
    // only the starting point (last_location_ptr). 0 for a homing flight to a
    // real location. Static-inits to 0; arm_pending_travel sets it (dec,
    // seg000:494c) when the map click misses any location, and loc_050be clears
    // it. Gates the map travel verb (BACK TO STARTING POINT vs SKIP TO
    // DESTINATION, build_room_command_records), the polar heading guard
    // (travel_update_heading) and the route hostile-zone check.
    pub(crate) travel_no_location_dest: u8,

    // = seg001:11cc travel_step_accum — the travel step's 8.8 sub-cell
    // accumulator, re-seeded to 0x80 (half a cell) by adjust_travel_heading;
    // consumed by the step math (loc_05206, travel-pump territory).
    pub(crate) travel_step_accum: u16,

    // = seg001:1225.. the scene records (palace_rooms et al) — the live,
    // runtime-mutable copy of room_scene::SCENE_RECORDS: the game-phase
    // callbacks unlock scripted palace exits (exit byte &= 0x7f) and patch
    // palace_rooms[1].background in here.
    pub(crate) scene_records: [crate::room_scene::SceneRecord; 83],

    // = seg001:1968 data_01968 — the cockpit fly-over silhouette's signed
    // relative bearing (heading - location angle) * 0x20, latched by
    // travel_flyover_detect (seg000:41e1) and by the outdoor-scene detector
    // (loc_04e12). Consumed by the fly-over overlay draw, which is not ported
    // yet, so the latch is currently write-only.
    pub(crate) data_01968: i16,

    // = seg001:196a data_0196a — the fly-over silhouette sprite id (table_196d
    // indexed by the location's SAL tier), latched alongside data_01968.
    pub(crate) data_0196a: u16,

    // = seg001:196c data_0196c — travel_flyover_detect's re-arm countdown:
    // after a fly-over is latched the detector idles for 6 probe passes
    // (decrementing this) before scanning for the next one.
    pub(crate) data_0196c: u8,

    // = seg001:1174 data_01174 — the game_time the last time-period event run
    // saw; run_events_for_current_time_period diffs it to raise new_day_flag.
    pub(crate) last_event_game_time: u16,

    // = seg001:1176 location_visibility_distance — the sietch visibility
    // radius in map cells (static init 1): sietch map markers farther than
    // this from the player draw the +5 distant sprite variant, and the
    // walk/troop range checks compare against it. Raised by the dialogue-line
    // event callback at seg000:a1ad (not ported).
    pub(crate) location_visibility_distance: u16,

    // = seg001:197c _word_20E2C_zoomed_globe_longitude / seg001:197e
    // _word_20E2E_zoomed_globe_latitude — the map/globe view centre.
    // set_zoomed_globe_pos_from_map_position seeds them from the player's map
    // position when the map screen opens; map_draw_zoomed_globe clamps the
    // latitude to the window (and the nav-panel scroll buttons move them —
    // not ported).
    pub(crate) zoomed_globe_longitude: u16,
    pub(crate) zoomed_globe_latitude: i16,

    // = the RESOURCE_GLOBDATA / RESOURCE_TABLAT / res_map_ofs buffers as one
    // owned renderer, built by setup_globe_draw (seg000:b8a7). None until the
    // first globe draw.
    pub(crate) globe_renderer: Option<GlobeRenderer>,
    // = seg001:494c _dword_23DFC (the TABLAT entry-0 fp field) — the globe
    // rotation phase in 1/398ths of a revolution (0..397): the integer word
    // of the DOS 16.16 seed. set_globe_tilt_and_rotation derives it from the
    // longitude (hi word of 398 * lng); globe_rotation_increment steps and
    // wraps it (the rotation frame task adds 1 per finished draw pass).
    pub(crate) globe_rotation: u16,
    // = seg001:2460 _word_21910_globe_tilt — the globe view tilt in map
    // latitude rows, carrying the map-row sign (negative = north, like
    // zoomed_globe_latitude); magnitude clamped to >= 0x20 by
    // set_globe_tilt_and_rotation and to <= 98 by globe_increment_tilt
    // (seg000:ba15).
    pub(crate) globe_tilt: i16,

    // = _word_2D1BF_globe_decoration_offset — the FRESK side decorations'
    // slide position on the globe screen: 0 = framing the globe, negative =
    // slid apart for the SEE RESULTS reveal (seg000:b8f3).
    pub(crate) globe_decoration_offset: i16,

    // = seg001:dd02 globe_draw_area_control_colors — nonzero (the SEE
    // RESULTS mode) selects the vga_globe_init patch that recolours every
    // globe pixel into the area-control palette blocks (0x10 plain, 0x20
    // Atreides-held, 0x30 Harkonnen-held — globe_pixel_area_control_colors,
    // segvga:1ec9); the globe keeps spinning in those colours.
    pub(crate) globe_draw_area_control_colors: u8,

    // = seg001:dd11 results_gauge_targets / seg001:dd17 results_gauge_current
    // — the SEE RESULTS gauges: results_update_gauge_targets fills the
    // targets, results_draw_text_and_icones zeroes the currents, and
    // results_gauge_task steps each current one toward its target per fire.
    pub(crate) results_gauge_targets: [u8; 6],
    pub(crate) results_gauge_current: [u8; 6],

    // = seg001:115c results_stats_timestamp — game_time & 0xfff0 at the last
    // stats refresh; the trend tail treats an equal value as "unchanged"
    // (glyph 3) only across a period change.
    pub(crate) results_stats_timestamp: u16,

    // = seg001:115e results_prev_values — each stat's last value, exchanged
    // by the loc_0bf7d trend tail.
    pub(crate) results_prev_values: [u16; 6],

    // = seg001:116a results_trend_glyphs — the trend glyph codes (1 rose /
    // 2 fell / 3 unchanged) results_gauge_task draws when a gauge lands.
    pub(crate) results_trend_glyphs: [u8; 6],

    // = seg001:1ae4 _word_20F94_ui_elements — the in-game HUD element table.
    pub(crate) ui_elements: [UiElement; 24],

    // = the seg001 command-menu record buffers, one owned mutable Menu per
    // menu exactly as DOS compiles them in and patches them in place. The
    // MenuRef identity on the menu_stack is the port's `bp`;
    // GameState::menu_buffer resolves it to the buffer. A stack pop reveals
    // the buffer as-it-is — nothing is rebuilt (= seg000:d30e).
    //
    // = seg001:1f0e command_menu_buf — the room (and map-mode) verb list
    // build_room_command_records assembles.
    pub(crate) command_menu_buf: menu_defs::Menu,

    // = seg001:1f7e menu_NPC_actions — the dialogue verb panel. Record 0's
    // text id is the TALK TO ME verb set_talk_to_me_verb_text patches in
    // place (seg000:d621): 0x90 while a voice line plays, 0x9f once it stops.
    // setup_npc_dialogue_menu splices only slot 1 (the per-NPC verb).
    pub(crate) menu_npc_actions: menu_defs::Menu,

    // = seg001:1f92 menu_go_towards_this_place — the fly-over divert menu.
    pub(crate) menu_go_towards_this_place: menu_defs::Menu,

    // = seg001:1f9e menu_change_destination_ignore_warning — the fly-over
    // hostile-zone warning menu.
    pub(crate) menu_destination_warning: menu_defs::Menu,

    // = seg001:1fae menu_prospector_troop_after_specializing_in_spice.
    pub(crate) menu_prospector_continue: menu_defs::Menu,

    // = seg001:1fba menu_multiple_provide_continue_option.
    pub(crate) menu_continue: menu_defs::Menu,

    // = seg001:1fc2 menu_dynamic.
    pub(crate) menu_dynamic: menu_defs::Menu,

    // = seg001:1ff2 menu_comms_room_messages_viewed.
    pub(crate) menu_comms_room_messages_viewed: menu_defs::Menu,

    // = seg001:1ffe menu_argue_accept_refuse.
    pub(crate) menu_argue_accept_refuse: menu_defs::Menu,

    // = seg001:2012 menu_done — the PALACE PLAN's single " Done" strip.
    pub(crate) menu_done: menu_defs::Menu,

    // = seg001:201a menu_mixer_panel — the mixer's music menu strip;
    // settings_ui_update_music_playlist_flags greys its MUSIC entries in
    // place.
    pub(crate) menu_mixer_panel: menu_defs::Menu,

    // = seg001:2032 menu_book.
    pub(crate) menu_book: menu_defs::Menu,

    // = seg001:204a menu_globe.
    pub(crate) menu_globe: menu_defs::Menu,

    // = seg001:2062 menu_globe_default_click_on_globe.
    pub(crate) menu_globe_default_click_on_globe: menu_defs::Menu,

    // = seg001:206a menu_globe_music — the CD-order submenu.
    pub(crate) menu_music: menu_defs::Menu,

    // = seg001:207a menu_globe_save_game — the save-slot submenu (records
    // restaged with slot flags/labels on every open).
    pub(crate) menu_save_game: menu_defs::Menu,

    // = seg001:208a menu_globe_load_game — the load-slot submenu.
    pub(crate) menu_load_game: menu_defs::Menu,

    // = seg001:20a2 menu_restart_load_exit_game.
    pub(crate) menu_restart_load_exit_game: menu_defs::Menu,

    // = seg001:20b6 menu_exit_game_confirmation — the EXIT GAME submenu.
    pub(crate) menu_exit_game_confirmation: menu_defs::Menu,

    // = seg001:20c2 menu_palace_mirror_room — the LOOK AT MIRROR menu.
    pub(crate) menu_palace_mirror_room: menu_defs::Menu,

    // = seg001:20da menu_multiple_move_to_location_flying_an_orni /
    // seg001:20e6 riding_a_worm — the GO THERE command menu the location
    // popup folds in; the records are set per open (map_click_location_marker).
    pub(crate) menu_go_there_flying_an_orni: menu_defs::Menu,

    // = seg001:20e6 menu_multiple_move_to_location_riding_a_worm.
    pub(crate) menu_go_there_riding_a_worm: menu_defs::Menu,

    // = seg001:20f2 menu_map_main — the SEE DUNE MAP view's verb menu (EXIT
    // MAPS / CONTACT FREMEN TROOPS / SEE SPICE DENSITY / TAKE AN ORNITHOPTER /
    // FIND PROSPECTORS). map_setup_main_menu (seg000:878c) rewrites the ids
    // and grey bits before every push.
    pub(crate) menu_map_troops: menu_defs::Menu,

    // = seg001:210a menu_map_troop_dialog — the contacted troop's order menu
    // (ASK FOR MORE INFORMATION / CHANGE TROOP OCCUPATION / MODIFY EQUIPMENT /
    // MOVE TROOP / NO MORE ORDERS). map_open_troop_contact_menu rewrites the
    // last slot's id and map_setup_troop_dialog_menu the grey bits before
    // every push.
    pub(crate) menu_troop_dialog: menu_defs::Menu,

    // = seg001:2122 menu_map_troop_contact_cycle_troops — the NEXT TROOP / NO
    // MORE ORDERS menu for a troop that cannot be ordered.
    pub(crate) menu_next_troop: menu_defs::Menu,

    // = seg001:212e menu_multiple_cancel — the map/globe main view's Cancel
    // strip (map_screen_open installs the caller's record set here).
    pub(crate) menu_cancel: menu_defs::Menu,

    // = seg001:2136 menu_map_move_prospectors — the prospector's
    // multi-destination pick menu (MOVE TROOP on troops[2]).
    pub(crate) menu_move_prospectors: menu_defs::Menu,

    // = seg001:214a menu_map_troop_moving_change_destination_next_troop — the
    // CHANGE DESTINATION / NEXT TROOP / Cancel menu for a troop on the move.
    pub(crate) menu_change_troop_destination: menu_defs::Menu,

    // = seg001:215a menu_map_select_troop_occupation.
    pub(crate) menu_select_troop_occupation: menu_defs::Menu,

    // = seg001:216e menu_map_troop_change_troop_occupation_for_spice_troop.
    pub(crate) menu_occupation_for_spice_troop: menu_defs::Menu,

    // = seg001:2182 menu_map_troop_change_troop_occupation_for_army_troop.
    pub(crate) menu_occupation_for_army_troop: menu_defs::Menu,

    // = seg001:219a menu_map_troop_change_troop_occupation_for_army_troop_doing_espionage_at_harkonnen_fortress
    pub(crate) menu_occupation_for_espionage_troop: menu_defs::Menu,

    // = seg001:21a6 menu_map_troop_change_troop_occupation_for_ecology_troop
    pub(crate) menu_occupation_for_ecology_troop: menu_defs::Menu,

    // = seg001:2220 menu_ptr_02220 — which of the two the scene currently
    // shows (change_menu_to_continue_menu / ..._special_menu_after_
    // specializing_prospector_troop_in_spice). Static init = the prospector
    // panel.
    pub(crate) sequence_menu: MenuRef,

    // = seg001:477a data_0477a — the active continue-sequence script and its
    // read cursor (DOS keeps a cs pointer; the port keeps the slice plus an
    // index). None = no scene running.
    pub(crate) sequence_script: Option<&'static [u8]>,
    pub(crate) sequence_cursor: usize,

    // = seg001:4778 data_04778 — the script position action 00 records so a
    // later step can branch back to it. Its readers are the unported
    // cutscene actions.
    pub(crate) sequence_return_cursor: Option<usize>,

    // = seg001:4776 data_04776 — the (location_and_room low byte,
    // data_046e0) pair start_scripted_dialogue snapshots and the 0xff end
    // restores.
    pub(crate) sequence_saved_scene: (u8, u8),

    // = seg000:23c25 the blink toggle frame_task_callback_blink flips while
    // a scripted scene runs.
    pub(crate) sequence_blink: bool,

    // = seg001:4718 data_04718 / seg001:4738 data_04738 — the destination
    // pick's working copy of the prospector queue and its entry count: the
    // MOVE TROOP verb seeds them from prospector_destinations (three words
    // copied, four scanned — the DOS asymmetry), map clicks append, and the
    // Done verb copies them back.
    pub(crate) prospector_pick_queue: [u16; 4],
    pub(crate) prospector_pick_count: u8,

    // = seg001:21fd data_021fd — the SKIP TO DESTINATION command template's
    // flags byte (the seg001:21fc record's text-id high byte; 0x40 = greyed).
    // DOS patches the static template in place
    // (set_skip_to_destination_verb_flags); the port keeps the template const
    // and applies this byte when build_room_command_records copies it.
    pub(crate) cmd_skip_to_destination_flags: u8,

    // = seg001:21da screen_element_stack — the z-ordered stack of active
    // menus, each with its cleanup func (the DOS slot's [si+2]).
    pub(crate) menu_stack: Vec<(MenuRef, Option<menu_defs::MenuCleanupFn>)>,

    // = seg001:227d data_0227d — suppresses the secondary 240..255 sky-palette
    // span. loc_039b9 / loc_0391d / loc_0398c write+fade an extra 16 colours
    // into entries 240..255 only when this is 0..
    pub(crate) data_0227d: u8,

    // = seg001:22e3 _byte_22E3_sky_skydn_selector — the SKY/SKYDN selector.
    // open_sky_or_skydn_palette opens resource 0x28 + this (0 → SKY.HSQ day,
    // 1 → SKYDN.HSQ dusk).
    pub(crate) sky_skydn_selector: u8,

    // = seg001:2570 data_02570 — pointer to the active mouse handlers:
    // the idle/LMB/RMB handler table game_loop's click/hover dispatch invokes.
    // select_room_ui_table (seg000:d95b) swaps it as the active screen changes;
    // until that is ported it stays at the room-screen variant.
    pub(crate) active_mouse_handlers: &'static MouseHandlers,

    // = seg001:2582 cursor_image_ptr — selects the active cursor shape. The port
    // tracks it as a CursorShapeId; None until the first redraw_mouse, which then
    // always composites the cursor (DOS instead draws it during the mouse-init
    // path the port does not run).
    pub(crate) cursor_image: Option<CursorShapeId>,

    // = seg001:dc58 mouse_nav_rect_ptr — the active navigation
    // mouse hot-zone: get_mouse_cursor_image switches the cursor to the hand
    // inside it and to the four travel arrows within the scroll bands outside
    // its edges. DOS stores a pointer to a Rect (the map screen installs
    // map_view_rect_template, seg000:4331); the port copies the rect. None =
    // cleared (clear_mouse_nav_rect).
    pub(crate) mouse_nav_rect: Option<Rect>,

    // = seg001:2784 _word_21C34_active_bank_id (+ the 0d844 cache table). The
    // active sprite/resource bank and its per-index loaded-sheet cache; see
    // `bank.rs`.
    pub(crate) banks: Banks,

    // = seg001:2788 data_02788 game_suspend_count — nesting suspend counter for
    // the live game (static init 1 = suspended during load/intro). While nonzero
    // the PIT callback skips advancing the game clock (seg000:ef84) and the idle-
    // event trigger is suppressed (seg000:1b12). suspend_game_clock /
    // resume_game_clock inc/dec it; reset_game_suspend zeroes it.
    pub(crate) game_suspend_count: u8,

    // = seg001:28be settings_drag_target (data_028be) — the active mixer-panel
    // drag group: 0 = none, 1 = a volume slider, 2 = a subtitle indicator. Set on
    // an LMB grab (loc_0a594); also read by get_mouse_cursor_image (the busy hand).
    pub(crate) settings_drag_target: u8,

    pub log_condit: bool,

    // Port-only (--log-subtitle): emit the subtitle/speech-bubble "SUB" trace,
    // mirroring chani_egui --log-subtitle so the two logs diff line-for-line.
    pub log_subtitle: bool,

    // = seg001:28e7 data_028e7 — active voice/subtitle output mode (0/1/2).
    // ui_toggle_room_view restores it from voice_subtitle_mode_default on room
    // entry; ui_show_globe_map_view forces it to 1.
    pub(crate) voice_subtitle_mode: u8,

    // = seg001:28e8 data_028e8 — configured voice/subtitle mode (set by
    // check_amr_or_eng_language), copied into voice_subtitle_mode on room entry.
    pub(crate) voice_subtitle_mode_default: u8,

    // = seg001:288e..28bd the six mixer-panel records (3 volume sliders + 3
    // subtitle indicators); see settings_ui.rs. Seeded from SETTINGS_RECORDS_INIT
    // and mutated as the panel is drawn / dragged.
    pub(crate) settings_records: [SettingsRecord; 6],

    // = seg001:2943 cmd_args_memory — a byte of misc/command-line
    // flags. Bit 0x10 is the "music off" toggle: menu_callback_choice_music_off
    // sets it, the MUSIC ON verbs clear it, and service_midi_music gates playback
    // on it. The mixer's MUSIC menu pre-highlight (settings_ui_update_music_
    // playlist_flags) reads it. Init 0 (the port parses no DOS command line).
    pub(crate) cmd_args_memory: u8,

    // = seg001:35a6
    pub(crate) hnm_bytes: Option<Box<[u8]>>,

    // = the resident companion loop-bridge resource (video_id + 0x61:
    // MNT1.LOP .. PALACE.LOP, resources 0x63..0x68) DOS keeps open across the
    // flight. At every flight-clip loop point it splices four stream records
    // pointing into this resource's video chunks (seg000:cbb8..cc04) — the
    // bridge frames played across the loop seam before the body resumes. The
    // port caches the resource here and hnm_step_frame decodes one chunk per
    // pass while hnm_lop_remaining > 0.
    pub(crate) hnm_lop_bytes: Option<Box<[u8]>>,
    // Which video id hnm_lop_bytes belongs to (the cache key).
    pub(crate) hnm_lop_video_id: u16,
    // The offset of the next bridge chunk within hnm_lop_bytes.
    pub(crate) hnm_lop_cursor: usize,
    // Bridge chunks still to decode (= the DOS cx = 4 splice, seg000:cbcc).
    pub(crate) hnm_lop_remaining: u8,

    // = seg001:3810 music_playlist_flags — the jukebox mode. 0 = game-relative
    // (the song follows the on-screen situation, the default set at game init);
    // bit 0 = CD-style playlist, bit 1 = shuffle.
    pub(crate) music_playlist_flags: u8,

    // = seg001:37fa music_cd_playlist — the working CD-playlist order: 9 song
    // numbers + the 0xff terminator. STANDARD ORDER recopies music_cd_standard_
    // order over it; SHUFFLE permutes it in place (music_cd_playlist_shuffle).
    pub(crate) music_cd_playlist: [u8; 10],

    // = seg001:380e music_cd_playlist_cursor — the index of the NEXT playlist
    // entry to play (DOS keeps a pointer into the table; init = the base).
    pub(crate) music_cd_playlist_cursor: usize,

    // = seg001:dbd2 music_song_end_tick_stamp — the PIT-counter stamp of the
    // first idle-driver sighting after a CD-playlist song ends; the CD service
    // advances the playlist 0xc8 ticks later. 0 = unset; cleared when a song
    // starts (seg000:adba).
    pub(crate) music_song_end_tick_stamp: u16,

    // = _unk_2CCD8_bios_timer_count_3 — rand_iterated's LCG seed, separate
    // from rand's (0d826) and rand_masked's (0d824). DOS seeds it from the
    // BIOS tick count during startup; the shuffle also perturbs it with the
    // live PIT counter between draws.
    pub(crate) rand_iterated_seed: u16,

    // = seg001:dbc8 settings_flags (data_0dbc8) — the mixer/settings flags word.
    // bit 0x1 = PCM enabled (check_pcm_enabled), bit 0x100 = music/MIDI enabled
    // (loc_0ae28), bits 0x4/0x400 = PCM / music slider draggable, bits 0x8/0x800
    // = subtitle indicators available. DOS sets these during audio init from the
    // detected hardware; the port seeds the steady "everything present" state so
    // the full panel draws and the sliders are draggable.
    pub(crate) settings_flags: u16,

    // = seg001:dbcc data_0dbcc — the "desired song" the music scheduler plays
    // when the driver goes idle (set by update_room_music; 0 = none).
    pub(crate) music_desired_song: u8,

    // Music-situation classifier inputs (= loc_0aa96).
    // = seg001:dd03 globe_screen_active.
    pub(crate) globe_screen_active: u8,

    // = seg001:46d6 _byte_23B86_current_sky_palette — persistent state of the
    // loc_00826 sky palette cycler (TaskId::SkyPaletteCycler), kept as a global
    // across frame-task clears.
    pub(crate) current_sky_palette: u8,

    // = seg001:46d7 — the sky fade countdown paired with current_sky_palette.
    pub(crate) sky_fade_countdown: u8,

    // = seg001:46d9 pending_room_screen_request — pending room-screen request code
    // (e.g. 6, 7). When nonzero, ui_present_room_screen jumps straight to
    // draw_room_game_screen for a full redraw instead of a transition wipe.
    pub(crate) pending_room_screen_request: u8,

    // = seg001:46db data_046db — the game-clock divider countdown. The PIT ISR
    // decrements it each tick (while the clock runs) and, on underflow, reloads
    // it from data_0146e (0x2ee0) and bumps game_time. Stored as i32 so the
    // underflow compare is a plain signed test; static-inits to 0 so the first
    // unsuspended tick advances the clock. See advance_game_clock.
    pub(crate) data_046db: i32,

    // = seg001:46dd new_time_period_pending — the "a new time period elapsed"
    // flag. The PIT ISR sets it whenever it bumps game_time (the `inc byte
    // [46dd]` at seg000:ef9b); run_events_for_current_time_period (reached from
    // game_loop's loc_01b0d) consumes it to refresh the date/time indicator and
    // fire scheduled time-period events.
    pub(crate) new_time_period_pending: u8,

    // = seg001:46de new_day_flag — the day part of game_time minus the day
    // part the last time-period event run saw; non-zero on the first period of
    // a new day, gating the per-day troop and location hooks.
    pub(crate) new_day_flag: u8,

    // = seg001:46df data_046df — arms the loc_03916 sky-fade task (stage 29).
    // The task stops itself when this is cleared; set by intro_29_init.
    pub(crate) sky_fade_active: bool,

    // = seg001:46e0 data_046e0 — previous sky_fade_active state; draw_room_game_
    // screen xchg's it with the current flag to decide between a fade transition
    // and a plain palette+blit when the day/night state changed.
    pub(crate) data_046e0: u8,

    // = seg001:46e1 spice_harvest_remainder — kilograms of harvested spice not
    // yet forming a full 10 kg batch of spice_in_stock, carried into the next
    // mining period's division (seg000:701b).
    pub(crate) spice_harvest_remainder: u16,

    // = seg001:46e3 data_046e3_rect — the map window rect the map screen draws
    // the desert map into; copied from map_view_rect_template (seg001:149c,
    // (81,45)-(241,134)) when the map screen opens.
    pub(crate) map_view_rect: Rect,

    // = seg001:46eb data_046eb — selects the navigation panel template in
    // ui_setup_and_draw_nav_panel: nonzero picks the alternate (ornithopter/travel)
    // panel (1cca) and the windowed map drawing (map_draw_zoomed_globe: bit 0x80
    // = full globe, bit 0x40 = suppress the map blit). Set to 1 by
    // map_screen_open (seg000:4323) and the travel routines (seg000:49a6),
    // cleared back to 0 by map_screen_cleanup for the plain room view.
    pub(crate) data_046eb: u8,

    // = the decompressed MAP2.HSQ (idx 0x3a) spice layer, one spice-field id
    // per map cell, same geometry as `map`. The spice-density overlay renders
    // it through a per-location colour table (DOS swaps res_map_seg to it,
    // seg000:5487). Empty until initialize_resources.
    pub(crate) map2: Box<[u8]>,

    // = seg001:4710/4712 data_04710/data_04712 — the shared popup-panel
    // origin the spice-density overlay draws at, and its rect (the rect
    // doubles as the popup identity). The overlay's home entry (seg000:5406)
    // reloads it from the data_011c1/011c3 home words; the contact popup
    // parks it opposite itself (seg000:7a15) for the in-place entry.
    pub(crate) map_overlay_panel_pos: (i16, i16),
    pub(crate) map_overlay_panel_rect: Rect,

    // = seg001:4722 data_04722 — which layer the overlay renders: 0 = the
    // spice-density colours, nonzero = the alternate (ecology) table at
    // seg000:583f, which no ported caller selects.
    pub(crate) map_overlay_mode: u8,

    // = seg001:0e30/0e32 _word_20E30_globe_param_3 / _word_20E32_globe_param_4
    // — the map position the spice-density overlay is centred on, exchanged
    // with the live zoomed-globe position around its draw (loc_0b69a).
    pub(crate) globe_param_3: u16,
    pub(crate) globe_param_4: i16,

    // = seg001:46da data_046da — nonzero while the WAIT-verb / travel event
    // pump (run_events_for_n_time_periods) owns the screen; the scheduler's
    // refresh tail skips the room redraw while it is set (seg000:1bbf).
    pub(crate) events_pump_active: u8,

    // = seg001:46ec data_046ec — the map-view dirty counter: bumped when a
    // mining troop eats through more than the spice-density overlay's
    // current shade while that overlay is up (data_046eb bit 6), and by the
    // daily vegetation promotion while the full map is up (seg000:65fe);
    // the scheduler's refresh tail consumes it via
    // map_view_refresh_after_events (seg000:1b97).
    pub(crate) spice_density_overlay_dirty: u8,

    // = seg001:473b data_0473b — the scheduler tail's room-redraw request
    // (seg000:1ba9): bit 7 = re-present the whole room screen (dismissing
    // stacked overlays), else nonzero = draw_room_game_screen; cleared by
    // the tail (seg000:1bb2).
    pub(crate) room_redraw_request: u8,

    // = seg001:46ed _word_23B9D_current_main_view_drawing_function — the
    // installed main-view redraw the map/globe dispatch sites call
    // (map_refresh_main_view seg000:8853, travel_refresh_view seg000:49e6;
    // the unported sites seg000:5d7e, 86c6). Each map-mode entry installs its
    // own: map_screen_open (seg000:4346) -> map_view_redraw, the travel
    // flight (travel_minimap_setup, seg000:499a) -> travel_minimap_redraw;
    // SEE DUNE MAP (seg000:5a8f) -> ui_main_view_map_interface waits on that
    // flow. DOS never clears it (the dispatch sites are gated on data_046eb);
    // None = the initial 0 word.
    pub(crate) current_main_view_drawing_function: Option<fn(&mut GameState)>,

    // = seg001:3cbe troop_icon_count / seg001:3cc0 troop_icons — the troop
    // icon renderer's live icon list (troop_icons.rs). The night attack
    // scene's separate copy lives in attack/mod.rs.
    pub(crate) troop_icons: Vec<crate::troop_icons::TroopIcon>,

    // = seg001:2786 troop_icon_draw_order_func — which draw-order pick
    // troop_icons_update_dirty_rect uses: false = troop_icons_pick_next_fifo
    // (0xc827, insertion order), true = troop_icons_pick_next_by_depth
    // (0xc835, the full map's back-to-front layering).
    pub(crate) troop_icon_draw_by_depth: bool,

    // = seg001:4752 troop_icon_focused_ptr — the two focused-icon slots; the
    // anim task steps slot 0 every firing where the rest only step every 4th.
    pub(crate) troop_icon_focused: [Option<usize>; 2],

    // = seg001:46f6 troop_icon_anim_phase — the anim task's frame counter.
    pub(crate) troop_icon_anim_phase: u8,

    // = seg001:46f3 map_view_reentry_count — counts map-view re-entries within
    // one visit (loc_05a03 increments it when a troop dialogue path re-opens
    // the view); reset_room_scene_state zeroes it. While 0,
    // ui_show_globe_map_view shows the rallied-troops title popup.
    pub(crate) map_view_reentry_count: u8,

    // = seg001:dbe0 map_popup_ptr / seg001:dbe2 map_popup2_ptr — pointers to
    // the open popup panel records on the full map view (0 = none): the
    // rallied-troops title panel (MAP_POPUP_RALLIED = data_0194a), the troop
    // occupation panel (data_04710) or the troop info panel (data_018df).
    // The map mouse handlers dispatch on which record is open. The port keeps
    // the DOS record offsets as the identity values.
    pub(crate) map_popup_ptr: u16,
    pub(crate) map_popup2_ptr: u16,

    // = seg001:1954 data_01954 — the selected troop id on the full map view
    // (0 = none): set by the icon click (troop_0872c), shown with the
    // highlight ring; reset_room_scene_state zeroes it.
    pub(crate) map_selected_troop_id: u8,

    // = seg001:1955 data_01955 — the last id map_select_troop actually
    // contacted (the byte above data_01954, so the two are read as one word by
    // menu_callback_choice_map_main_contact_fremen_troops and cleared together
    // by reset_room_scene_state). With nothing selected, the contact verb
    // resumes this troop as long as it still has an icon on the map.
    pub(crate) map_last_selected_troop_id: u8,

    // = seg001:46fa data_046fa — the troop whose info panel (data_018df) is
    // open (a troop ptr in DOS, the table index here; None = closed).
    pub(crate) map_info_popup_troop: Option<usize>,

    // = seg001:46ef data_046ef — the troop whose contact dialogue popup is up
    // (a troop ptr in DOS, the table index here; None = no live contact).
    // map_close_troop_contact_popup marks the contact on it and clears it.
    pub(crate) map_contact_troop: Option<usize>,
    // = seg001:46f1 data_046f1 — the troop the popup is being built for.
    // map_setup_troop_contact_popup latches it before
    // map_draw_troop_contact_popup, and
    // subtitle_setup_layout rebuilds the popup from it (seg000:8cea) when a
    // line is presented with no popup up.
    pub(crate) map_contact_troop_pending: Option<usize>,
    // = the troop_contact_text_panel_record's runtime rect (seg001:18e9: x
    // (5,232) compiled in, the y pair rewritten per open) and the head box
    // (seg001:18f3, written per open) inside it.
    pub(crate) map_contact_popup_rect: Rect,
    pub(crate) map_contact_head_rect: Rect,
    // = seg001:2244/2246 — the x/y words of the contact subtitle's layout
    // descriptor (seg001:2244, size 153x63), written per open by
    // map_draw_troop_contact_popup.
    pub(crate) map_contact_subtitle_pos: (i16, i16),
    // = seg001:004c related_to_contacting_troops_ds_4c — 0xff while the
    // contacted troop answers from outside the visibility range, so the
    // dialogue record's conditions pick its "out of contact" lines; cleared
    // by map_close_troop_contact_popup.
    pub(crate) contacting_troops_ds_4c: u8,

    // The five nearest-location triples condit_scan_nearest_locations
    // (seg000:5274) refreshes from the staged location whenever
    // prepare_location_data_for_condit runs.
    // = seg001:00ca nearest_location_distance_ds_ca — the nearest other
    // location of any kind.
    pub(crate) nearest_location: NearestLocation,
    // = seg001:00d0 nearest_village_distance_ds_d0 — the nearest village
    // (appearance < 0x28, status bit 7 clear).
    pub(crate) nearest_village: NearestLocation,
    // = seg001:00d6 nearest_sietch_distance_ds_d6 — the nearest
    // phase-discoverable sietch (appearance < 0x28, bit 7 set); gates the
    // "There is a sietch very near" messages.
    pub(crate) nearest_sietch: NearestLocation,
    // = seg001:00dc nearest_Atreides_area_distance_ds_dc — the nearest
    // Atreides area (appearance >= 0x28, bit 7 clear).
    pub(crate) nearest_atreides_area: NearestLocation,
    // = seg001:00e2 nearest_Harkonnen_area_distance_ds_e2 — the nearest
    // Harkonnen area (appearance >= 0x28, bit 7 set); the ESPIONAGE
    // occupation and the Harkonnen-captain dialogue need its distance < 0x1e.
    pub(crate) nearest_harkonnen_area: NearestLocation,

    // = seg001:46d2/46d4 data_046d2/046d4 — the head-rect-relative anchor point
    // the troop-contact popup re-anchors the talking head on (staged from
    // TALKING_HEAD_POPUP_ANCHOR by map_draw_troop_contact_popup), and
    // = seg001:47d4 data_047d4 — the popup's head draw box: both the
    // destination origin and the clip rect draw_head_image_group_in_box uses.
    pub(crate) head_popup_anchor: (i16, i16),
    pub(crate) head_popup_box: Rect,

    // = the data_018df panel record's runtime rect (loc_05f25 writes the
    // record's +0..+7 next to the clicked icon each open).
    pub(crate) map_info_panel_rect: Rect,

    // = seg000:5f65/_unk_2CCC6 — the source point (the clicked icon / marker
    // position) the panel's XOR outline scale animation grows from and shrinks
    // back to (xor_rect_outline_advance / _reverse, effects al=6/8), plus the
    // panel rect it animates to. = seg001:46d8 data_046d8 — set by
    // map_select_troop to suppress the next close animation (loc_07b2b).
    pub(crate) map_popup_anim_src: (i16, i16),
    pub(crate) map_popup_anim_rect: Rect,
    pub(crate) map_popup_anim_suppress: bool,

    // = segvga data 035ea..03600 — the bracket-zoom XOR animation state, staged
    // by xor_bracket_anim_setup / xor_bracket_zoom_to_panel (the troop-contact
    // popup's open effect, al=2) and read back by xor_bracket_zoom_from_panel (its
    // close effect, al=4): the per-frame box-trail step (035ea/035ec), the
    // bracket expand step (035ee/035f0), the origin of a 20x20 box centred on
    // the panel (035f6/035f8) and the last bracket drawn (035fa..03600),
    // which the close shrinks back from.
    pub(crate) xor_bracket_anim_move_step: (i16, i16),
    pub(crate) xor_bracket_anim_expand_step: (i16, i16),
    pub(crate) xor_bracket_anim_center: (i16, i16),
    pub(crate) xor_bracket_anim_shape: (i16, i16, i16, i16),

    // = seg001:46f8 data_046f8 — the location whose info popup is open
    // (a location ptr in DOS, the table index here; None = closed), the
    // re-click gate. = seg001:46f7 data_046f7 — its class+1 (0 = closed).
    pub(crate) map_location_popup_loc: Option<usize>,
    pub(crate) map_location_popup_class: u8,
    // = the data_01668 record's runtime rect.
    pub(crate) map_location_popup_rect: Rect,

    // = seg001:46fc data_046fc — the map screen's hover state, maintained by
    // map_mouse_hover_tracker (seg000:4586) and consumed by the LMB
    // destination click: 0 = pointer outside the map window; a location ptr
    // (see locations::location_ptr) = hovering that location's marker;
    // 0xfff0+n = aligned on desert compass ray n (0 N .. 7 NW) from the
    // player marker; 0xffff = inside the window, nothing hovered. Cleared on
    // map open.
    pub(crate) data_046fc: u16,

    // = seg001:46ff
    pub(crate) available_equipment: Equipment,

    // = seg001:4726 data_04726 — the map verbs' manual heading-adjust
    // accumulator, stepped in 0x20 (one compass point) units by TOWARDS
    // NEAREST PLACE (seg000:5031) and drained by the verb region at
    // seg000:41a7..41b8 (not ported); cleared by
    // ungrey_skip_to_destination_verb.
    pub(crate) data_04726: u8,

    // = seg001:4727 travel_active — nonzero while an in-game travel sequence
    // (HNM-driven map flight) is active; travel_pump (the game_loop's per-pass
    // hook, seg000:4f0c) returns immediately when this is 0. Set to 0xff by
    // map_confirm_travel_and_close (frame_task_callback_04ab8); cleared on
    // travel arrival (seg000:4fcb).
    pub(crate) travel_active: u8,

    // = seg001:dc16 video_decode_buf_seg (as an occupancy flag) — the HNM
    // streaming pipeline: DOS's reader decodes the NEXT video frame into the
    // target buffer as soon as the present consumes the current one
    // (loc_0caa0 -> hnm_decode_typed_chunk_video_to_bp, bp = fb1 for the
    // flight clips), and hnm_decode_video_frame consumes it with
    // `xchg bp,[video_decode_buf_seg]` (seg000:cc9f). True = a prefetched
    // frame is already decoded and waiting for its tick. For the flight clips
    // this is what keeps fb1 clean between presents: the minimap stamp only
    // lives in fb1 for the instant of hnm_present_flight_frame, so the
    // fly-over cabin's transparent windshield shows plain desert.
    pub(crate) hnm_video_frame_ready: bool,

    // = seg001:4728 travel_minimap_state — the flight minimap state: 0 normal,
    // 1 = recenter + redraw pending (set by the pump when the position leaves
    // the minimap bounds, seg000:4f8e, and by CHANGE DESTINATION at
    // seg000:4980), bit 0x80 = minimap hidden (toggled by loc_04aad).
    // map_screen_cleanup re-enters the minimap view when > 0. Reset by
    // travel_reset_trail when the map screen opens.
    pub(crate) travel_minimap_state: i8,

    // = seg000:e40c travel_trail_ring — the cs-resident travel-trail ring:
    // (longitude, latitude) pairs up to loc_0e85c ((0xe85c - 0xe40c) / 4
    // entries); empty entries hold the 0x800 sentinel in both words
    // (travel_reset_trail). travel_trail_append writes at the cursor.
    pub(crate) travel_trail_ring: [(u16, u16); TRAVEL_TRAIL_LEN],

    // = seg001:149a travel_trail_cursor — the ring's write cursor (the NEXT
    // slot travel_trail_append fills; DOS keeps a byte pointer).
    pub(crate) travel_trail_cursor: usize,

    // = seg001:4729 travel_step_tick_stamp — PIT stamp of the travel pump's
    // last step (travel_pump steps every 0x300 ticks); zeroed by
    // map_confirm_travel_and_close.
    pub(crate) travel_step_tick_stamp: u16,

    // = seg001:472b travel_step_counter — counts travel_advance_step calls;
    // every 16th runs one time period of events. Zeroed by
    // map_confirm_travel_and_close.
    pub(crate) travel_step_counter: u16,

    // = seg001:473e map_ornithopter_mode — nonzero while the map screen is in
    // ornithopter (cockpit) mode: set to 1 by TAKE AN ORNITHOPTER
    // (seg000:42f5), cleared by CALL A WORM (seg000:42b0). Selects the ORNYPAN
    // cockpit drawing and caption style on the map screen.
    pub(crate) map_ornithopter_mode: u8,

    // = seg001:473f/4741 data_0473f/data_04741 — the far pointer into the
    // COMMAND string the map caption typewriter draws next (0 = disarmed).
    // The port stores the resolved string plus an index; an empty string is
    // the disarmed state map_add/remove_select_destination_text_task and the
    // seg000:4658 idempotence check test.
    pub(crate) map_caption_text: Vec<u8>,
    pub(crate) map_caption_pos: usize,
    // = seg001:4743 data_04743 / seg001:4745 data_04745 — the caption pen
    // (x, y); the typewriter task stores the advanced pen back after each
    // glyph.
    pub(crate) map_caption_x: u16,
    pub(crate) map_caption_y: u16,
    // = seg001:4747 data_04747 — the caption colour word
    // ((bg << 8) | fg, the font_draw_fg_color/font_draw_bg_color pair).
    pub(crate) map_caption_color: u16,

    // = seg001:4749 map_player_marker_rect — the blinking "you are here"
    // marker's screen bounding rect (x0 == 0 = no marker, the player is off
    // the map window), set by map_arm_player_marker_task; the blink task
    // restores and redraws it, and the map hover tracker
    // (map_mouse_hover_tracker) aims its desert compass rays at its tip.
    pub(crate) map_player_marker_rect: Rect,

    // = seg001:4751 map_player_marker_phase — the "you are here" marker blink
    // phase, bumped each map_player_marker_blink_task firing; odd = drawn.
    pub(crate) map_player_marker_phase: u8,

    // = seg001:a5c0 visible_location_markers — one entry per location visible
    // on the map view, rebuilt by map_build_and_draw_location_markers and
    // scanned by the marker hover hit-test (find_nearest_location_marker).
    // DOS packs 6-byte entries [location ptr, screen x, screen y:u8,
    // data_046eb copy:u8] with a 0-word terminator; the port stores them
    // unpacked.
    pub(crate) visible_location_markers: Vec<MapLocationMarker>,

    // = seg001:487e travel_vehicle_mode — the vehicle for the pending map
    // travel: 1 = worm (CALL A WORM, seg000:42aa), 2 = ornithopter (TAKE AN
    // ORNITHOPTER, seg000:42ff / seg000:50db). loc_04ec6 refines it into
    // hnm_active_video_id (the day/night flight HNM variants 2..5).
    pub(crate) travel_vehicle_mode: u16,

    // = seg001:472d orni_hotspot_x / seg001:472f orni_hotspot_y — the parked-
    // ornithopter hover hotspot (the first orni's position + (0xc, 8)),
    // recorded by the draw_room_scene orni pass (seg000:3a5a..3a67) and
    // cleared (x = 0 = no ornis) at every scene draw (seg000:37b8).
    // person_hit_test's orni tail (seg000:92ab) resolves the cursor against it
    // to the 0x2f pseudo-person.
    pub(crate) orni_hotspot_x: u16,
    pub(crate) orni_hotspot_y: u16,

    // = seg001:4731 orni_anim_frame — the orni animation frame counter. 0 =
    // parked (rotor idle); the take-off sequence (loc_047fb, not ported) steps
    // it up to 0x21; 0xff = ornis hidden (draw_room_ornis skips the pass).
    // draw_orni maps it to the two animated part sprites.
    pub(crate) orni_anim_frame: u8,

    // = seg001:4732 data_04732 — room-entry flags; bit 0 requests the extra
    // location overlay SAL (loc_0488a) on the normal draw_room_game_screen path.
    pub(crate) data_04732: u8,

    // = seg001:4735 data_04735 — pending-dialogue/auto-action byte; its high bit
    // (sign) makes draw_room_game_screen run the loc_03723 auto-action handler.
    pub(crate) data_04735: u8,

    // = seg001:0fd8 room_persons — the 16-entry room-person table walked by
    // scan_matching_room_person_entries. Mutable copy of ROOM_PERSON_TABLE_INIT;
    // init_room_persons rewrites entries 12..16 (addresses data_0109a / 10aa /
    // 10ba / 10ca) and the loc_06603 classification path also touches
    // entries 12, 14, 15 plus (selectively) 13.
    pub(crate) room_persons: [RoomPerson; 16],

    // = seg001:476a data_0476a — count consumed by build_room_person_record_body
    // when the entry's person_index is 0x0f: emits `data_0476a - 1` extra chained
    // verb records (text_ids 0x88..) sharing the entry's handler. init_room_persons
    // resets this to 0; the special-room (location_appearance low byte == 0x80) path in
    // init_room_persons grows it as it classifies entries.
    pub(crate) data_0476a: u8,

    // = seg001:476b data_0476b — index of the chained record (1-based, within the
    // run of records build_room_person_record_body just emitted) whose text_id is
    // patched to 0x8f when game_phase >= 5. 0 disables the patch. Reset to 0 by
    // init_room_persons.
    pub(crate) data_0476b: u8,

    // = seg001:4774 data_04774 — nonzero while a dialogue is active; routes
    // ui_draw_room_command_panel to the dialogue renderer and suppresses the
    // auto lip-sync start.
    pub(crate) is_dialogue_active: bool,

    // = seg001:47a4 room_render_flags — scene/room render flags used by draw_SAL
    // and scene setup; draw_room_game_screen clears it before the render.
    pub(crate) room_render_flags: u8,

    // = seg001:47a5 dialogue_interrupt_gate — the room-leave interrupt gate. ui_click_move_room
    // arms it to 0xff (arm_dialogue_interrupt_gate) before the room-person dialogue scan; a spoken
    // line's event callback clears it (event 0x02 stay_here -> 0), and a non-0xff
    // value aborts the move (test_dialogue_interrupt_gate).
    pub(crate) dialogue_interrupt_gate: u8,

    // = seg001:47a6 data_047a6 — armed (0xff) at the top of draw_room_game_screen
    // and consumed by finish_room_screen_setup (loc_035ad).
    pub(crate) data_047a6: u8,

    // = seg001:47a7 data_047a7 — when nonzero, draw_room_game_screen skips the
    // dialogue/lip-sync auto-start tail. The room-leave scan also sets it as each
    // standing person speaks so only one person interrupts the move.
    pub(crate) data_047a7: u8,

    // = seg001:47aa data_047aa — index into the persons array (see persons_met)
    // of the speaker whose lip-sync to auto-start; 0 = none. Cleared on entry.
    pub(crate) data_047aa: u16,

    // = seg001:47c4 _word_23C74_current_lip_sync_resource_id — sprite-sheet
    // resource id of the current speaker's lip-sync data; 0xffff = none.
    pub(crate) current_lip_sync_resource_id: u16,

    // = seg001:4780 current_subtitle_id — the COMMAND/PHRASE id of the dialogue
    // sentence currently selected for presentation (set by show_voice_subtitle
    // from the phrase id dialogue_interpret_record pulls out of the matched
    // sentence). 0 = none.
    pub(crate) current_subtitle_id: u16,

    // = seg001:47be data_047be — the dialogue sentence cursor: person_index << 3,
    // primed by set_dialogue_speaker (seg000:93e7). menu_callback_choice_talk_to_me
    // walks the speaker's record slots starting from this base (person*8 + topic).
    pub(crate) dialogue_topic_index: u16,

    // = seg001:47c2 data_047c2 — the dialogue verb-panel sentence-eligibility mask
    // set_dialogue_speaker primes to 0x80 (seg000:9412). dialogue_interpret_record
    // masks each sentence's flag byte against it (seg000:9fbe) to skip verb-gated
    // entries; other dialogue verbs flip it to 0x20.
    pub(crate) data_047c2: u8,

    // = _dword_23C60_PHRASE_BIN + _byte_23C2E_current_phrase_bin_resource_id —
    // the resident PHRASE bank (load_PHRASExx_HSQ) and its resource id
    // (0 = none loaded).
    pub(crate) phrase_bin: Vec<u8>,
    pub(crate) current_phrase_bin_id: u8,

    // = seg001:11eb string_subst_id_table — the COMMAND/PHRASE string ids the
    // inline name placeholders 0x80..0x8f expand to (entries 1..2 alias the
    // command-menu origin in DOS; the port keeps the menu origin separate).
    pub(crate) string_subst_id_table: [u16; 16],

    // = seg001:4784/4786/4788/478a subtitle_pad_left/right/top/bottom — the
    // text insets inside the subtitle/bubble rect, staged per context
    // (prepare_dialogue_presentation, subtitle_setup_layout).
    pub(crate) subtitle_pad_left: u16,
    pub(crate) subtitle_pad_right: u16,
    pub(crate) subtitle_pad_top: u16,
    pub(crate) subtitle_pad_bottom: u16,

    // = seg001:4799 subtitle_layout_flags (data_04799) — bit 0 justify, bit 1
    // centre-line, bits 2..3 the vertical placement.
    pub(crate) subtitle_layout_flags: u8,

    // = the x0 words of the three speech-balloon descriptors (seg001:2224/
    // 222c/2234) — statically 0x50, patched per speaker from
    // talking_head_balloon_x_table (seg001:22a8) whenever the talking head
    // changes (seg000:91d4 in setup_lip_sync_data_from_sprite_sheet), so the
    // balloon clears the portrait.
    pub(crate) balloon_x: i16,

    // = seg001:479e current_bubble_layout_ptr + ui_hud_elements[18] + the
    // RESOURCE_GLOBDATA save-under — the live subtitle/bubble overlay
    // subtitle_restore_prior takes down.
    pub(crate) subtitle_bubble: Option<crate::subtitle::SubtitleBubble>,

    // = seg001:47e0 data_047e0 — the voiced-line random variant index
    // (format_interpolated_string's rand & 3 tail); its reader (the voc
    // suffix pick) is not yet ported.
    pub(crate) data_047e0: u8,

    // = seg001:47e1/47e2 data_047e1 — the speaker's "hold up a sign" overlay,
    // armed by the dialogue-line event 0x0a: the low byte is the state (1
    // armed, 0x80 shown) and the high byte (data_047e2) the portrait
    // animation index * 2 that raises the sign. Cleared when the conversation
    // is set up or torn down (seg000:93ac / 97e5).
    pub(crate) head_sign_state: u8,
    pub(crate) head_sign_anim: u8,
    // = seg001:47e4 data_047e4 — the sign table row that armed it (a seg001
    // pointer in DOS, the table index here).
    pub(crate) head_sign_record: Option<usize>,

    // = seg001:477c dialogue_current_record_ptr — byte offset of the sentence
    // entry the present walk started at (seg000:9f9e); load_PHRASExx_HSQ
    // (seg000:d00f) compares it against dialogue_phrase12_first_record_ptr (a
    // relocated pointer at offset 0x60 inside the DIALOGUE buffer, seg001:aa76)
    // to pick the PHRASE11 vs PHRASE12 phrase bank.
    pub(crate) dialogue_current_record_ptr: u16,

    // = seg001:47de dialogue_line_word0 — first word of the sentence entry being
    // presented (seg000:9ff9); the voc-replay / subtitle continuation code
    // (seg000:89d3/8a3b/8ac6, unported) tests its 0x10 flag.
    pub(crate) dialogue_line_word0: u16,

    // = seg001:47b6 dialogue_text_continuation_ptr — a pending multi-part
    // subtitle-text continuation, armed at seg000:89c8 when the interpolator
    // hits a top-level sentence separator (a terminator byte != 0xff) and
    // cleared by the final 0xff terminator or set_dialogue_speaker. While
    // set, menu_callback_choice_talk_to_me re-presents the continuation
    // (loc_094dd) with current_subtitle_id += 0x1000 (the voc variant-letter
    // step) and fire_dialogue_line_event skips the event + spoken-mark +
    // advance (seg000:a042). DOS stores a far pointer into the 0xa840
    // expansion buffer; the port owns the remaining source bytes.
    pub(crate) dialogue_text_continuation: Option<Vec<u8>>,

    // = seg001:47a8 dialogue_end_request — incremented by the spoken-line event
    // 0x06 (callback_event_dialogue_line_06_end_dialogue, seg000:a1e8); consumed
    // (xchg with 0) at seg000:a09d to force the walk's continuation pointer to
    // 0xffff so the next TALK TO ME stops resuming the record.
    pub(crate) dialogue_end_request: u8,

    // = seg001:47ba dialogue_resume_entry_ptr — the TALK TO ME resume pointer:
    // byte offset of the sentence entry the next talk action continues from
    // within the current record (0 = start at the data_047be topic cursor;
    // 0xffff = record exhausted / dialogue ended).
    pub(crate) dialogue_resume_entry_ptr: u16,

    // = the growing 0-terminated word list at cs:0xaa.. whose head pointer is
    // dialogue_played_log_head (seg001:11bd) — the dialogue-played log: one
    // packed (entry_index | lip_sync_id << 11) word per replayable spoken line,
    // appended by fire_event_callbacks (seg000:a07f) and pre-filled by the
    // Ctrl+V cheat (seg000:b270, unported). Savegames carry it.
    pub(crate) dialogue_played_log: Vec<u16>,

    // = seg001:476e npc_menu_idle_timer_base / seg001:4772
    // npc_menu_idle_timer_limit — the NPC-actions-menu inactivity timer
    // arm_npc_menu_idle_timer (seg000:c85b) arms: base = PIT counter at the last
    // spoken line, limit = 0x1770 (6000 ticks, 30 s). The room mouse hook
    // loc_01ae7 (seg000:1ae7, unported) watches them while menu_NPC_actions is
    // the active menu and fires loc_0c868 on expiry.
    pub(crate) npc_menu_idle_timer_base: u16,
    pub(crate) npc_menu_idle_timer_limit: u16,

    // = seg001:47f8 character_x_table / seg001:47fa character_y_table — the
    // per-person on-screen position markers. sal_draw_character records each
    // drawn standing person's (x, y) anchor at [id*4]; person_hit_test_at_cursor reads the
    // cursor against them so a mouseover/click on a person resolves to a person
    // index. 0x17 entries; (0xffff, 0xffff) marks an absent/off-screen person
    // (cleared by loc_03ae9 before the room is drawn).
    pub(crate) character_screen_pos: [(u16, u16); 0x17],

    // = the decompressed CONDIT resource (idx 0xbc) — the condition offset
    // table + bytecode buffer pointed at by _word_29F22_res_condit_ofs
    // (seg001:aa72). DOS loads it in initialize_resources (seg000:0126); the
    // port loads it in GameState::initialize_resources. None until then. The
    // interpreter lives in condit.rs (evaluate_condition / condition_holds).
    pub(crate) condit: Box<[u8]>,

    pub(crate) dialogue: Box<[u8]>,

    // = the decompressed MAP.HSQ (idx 0xbf) planet terrain map, one byte per
    // map cell (see map.rs for the layout). DOS keeps res_map_ofs pointing at
    // its centre (offset 0x62fc); the port stores the whole buffer and adds
    // the centre inside the tablat row offsets. Mutable: the startup loop ORs
    // the location bit 0x40 into each location's cell. Empty until
    // initialize_resources.
    pub(crate) map: Box<[u8]>,

    // = the byte-swapped TABLAT.BIN (idx 0xba, loaded at seg000:00d3) — the
    // per-latitude map row table (see tablat.rs). None until
    // initialize_resources.
    pub(crate) tablat: Option<Tablat>,

    // = segvga:1f4c vga_draw_map_zoomed's working state (the RESOURCE_GLOBDATA
    // band scratch) — the SEE DUNE MAP full-planet renderer,
    // map_draw_zoomed_globe's data_046eb bit-0x80 path.
    pub(crate) map_renderer: MapRenderer,

    // = seg001:cd9e — the buffer ui_save_head_rect (seg000:1834) grabs the head-
    // fold strip into: framebuffer-1 rect [1e76h] = (150,137,170,147), 20×10 =
    // 200 packed bytes. loc_017be's animating-down branch puts it back to fb1 to
    // restore the background revealed as the portrait folds away.
    pub(crate) ui_hud_head_saved_strip: Vec<u8>,

    // = seg001:ce66 _byte_2C316_ui_hud_head_animating_down — set for the duration
    // of ui_hud_head_animate_down's fold-down loop. While set, loc_017be restores
    // the head-fold strip from ui_head_saved_strip instead of copying the clean
    // portrait backdrop from fb2.
    pub(crate) ui_hud_head_animating_down: bool,

    // = seg001:ce7a _word_2C32A_pit_timer_callback_counter — the free-running
    // PIT ISR tick counter — has no stored field: `game_ticks() as u16` is the
    // port's live equivalent, and every reader derives it from there.

    // = seg001:ce80 data_0ce80 pause_enabled — P-key GAME PAUSED window enable
    // flag (pause_if_p_key_pressed opens the window only when nonzero). Cleared
    // around HNM cutscenes; start sets it to 0xff to allow in-game pausing.
    pub(crate) pause_enabled: u8,

    // = seg001:ceeb language_setting — the selected voice/subtitle
    // language (0 = American, 3 = English, 6 = Fremen/DUT, ...). The mixer panel's
    // language buttons update this and reload the per-language COMMAND.BIN strings
    // + DNCHAR glyph font (settings_ui_reload_language), so the verb/command text
    // switches language. Defaults to 0 (American) at startup.
    pub(crate) language_setting: u8,

    // = seg001:d7f4 per_person_voc_base_table — see build_voc_base_table.
    pub(crate) voc_bases: [u16; 17],

    // = seg001:47dc data_047dc — the shared "fixed-block" voc-bank flag: nonzero
    // while a line is presented from a fixed dialogue block whose voc numbering
    // does not belong to the speaking head's own P<X> directory. The fly-over
    // narration (travel_play_flyover_line, seg000:96db) and the fixed-block COME
    // WITH ME (seg000:95b7, unported) arm it around their present, then clear it.
    // load_voc_and_lipsync_data (seg000:a6f1) reads it: when set, the voc index
    // is rebased onto per_person_voc_base_table[0x10] + 0x3e7 instead of the
    // speaker's own base, so the fly-over line finds its .voc.
    pub(crate) data_047dc: u8,

    // = seg001:d824 _unk_2CCD4_rand_seed.
    pub(crate) rand_seed: u16,

    // = seg001:d826 _unk_2CCD6_rand_seed.
    pub(crate) rand_bits_seed: u16,

    // = seg001:dbd8 _word_2D088_screen_buffer_seg — the "front buffer" copy/
    // present target. Normally Screen; gfx_call_bp_with_front_buffer_as_screen
    // redirects it to Fb1 so a stage init renders fully offscreen.
    pub(crate) screen_buffer: FbId,

    // = seg001:dbda _word_2D08A_framebuffer_active_seg — the buffer every blit
    // primitive currently targets. Stage inits run with this == Fb1.
    pub(crate) active_fb: FbId,

    // = seg001:dbe6
    pub(crate) hnm_finished: bool,
    // = seg001:dbe7
    pub(crate) hnm_frame_counter: u16,
    // = seg001:dbea hnm_counter_2 — frame records consumed since the clip was
    // opened or last hit its loop point. DOS counts them as the streaming
    // prefetcher reads them ahead (seg000:cc26/ca44); the single-buffer port
    // counts them as they are decoded, which is the same stream position.
    // Reset by hnm_reset_counters (seg000:ce07) and at the loop rewind
    // (seg000:cb70, which saves it into the unported hnm_counter_3).
    pub(crate) hnm_counter_2: u16,
    // = seg001:dbee hnm_counter_4 — the armed loop-point frame count: when
    // hnm_counter_2 reaches it the stream treats the position as the loop
    // point (seg000:cb00), so hnm_switch_active_video can redirect into
    // another clip at an exact frame. 0xffff (= the DOS -1) = disarmed.
    pub(crate) hnm_counter_4: u16,
    // = seg001:dbfe
    pub(crate) hnm_resource_data: u16,
    // = seg001:dc00
    pub(crate) hnm_video_id: u16,
    // = seg001:dc02
    pub(crate) hnm_active_video_id: u16,
    // The live read cursor into `hnm_bytes`. The DOS reader streams the file
    // through a double-buffered scratch area (hnm_file_read_buf_ofs etc.); the
    // port keeps the whole resource resident and just indexes into it.
    pub(crate) hnm_read_offset: usize,
    // = the header size word at the head of the resource (seg000:c96b
    // hnm_read_header_size). Frame offsets are relative to the end of the
    // header, so a frame at table offset `rel` sits at `hnm_header_size + rel`.
    pub(crate) hnm_header_size: u16,
    // = the cached first-frame offset within `hnm_bytes`, computed by
    // hnm_read_header (seg000:c9c6). Mirrors the DOS body_offset/remain pair
    // (seg001:dbf6) that hnm_prefetch seeks to; here it is just a buffer index.
    pub(crate) hnm_body_offset: usize,
    // = seg001:dc12
    pub(crate) hnm_framebuffer: FbId,

    // = seg001:dc36 mouse_pos_x / seg001:dc38 mouse_pos_y — the cursor position
    // get_mouse_pos_etc latches each poll. The port copies it from the shared
    // InputState (already mapped into 320x200 game coordinates by the host)
    // instead of reading INT 33,3 and applying the mickey scalers.
    pub(crate) mouse_pos_x: u16,
    pub(crate) mouse_pos_y: u16,

    // = seg001:dc62 data_0dc62 / seg001:dc64 data_0dc64 — the pointer position
    // latched on the previous game_loop pass. Each pass xchg's the live position
    // in and subtracts to derive the per-frame motion delta (di = X, cx = Y) the
    // drag handler ([si+0ah]) consumes.
    pub(crate) mouse_prev_drag_x: u16,
    pub(crate) mouse_prev_drag_y: u16,

    // = seg001:dc5c data_0dc5c — the HUD element a press has armed for held
    // auto-repeat / release dispatch (set when the press lands on a record with
    // the 0x4000 flag; di in DOS, an index here). game_loop's drag path re-fires
    // it on the 0x32-PIT-tick interval and the release path fires + clears it.
    pub(crate) drag_armed_element: Option<usize>,

    // = seg001:d10e _word_2D10E_mouse_last_click_time — the PIT counter snapshot
    // taken each time an element handler fires (= seg000:d935). The held-button
    // auto-repeat gate (= seg000:d8da) re-fires only once >= 0x32 ticks elapse.
    pub(crate) mouse_last_click_time: u16,

    // = seg001:ceba data_0ceba — a keyboard-latch byte cleared alongside the
    // Enter key whenever an element click fires (= seg000:d930), so a queued
    // keyboard action does not also trigger after the mouse click.
    pub(crate) data_0ceba: u8,

    // = seg001:dc42 mouse_draw_pos_x / seg001:dc44 mouse_draw_pos_y — where the
    // cursor was last composited; redraw_mouse restores this region before
    // drawing at a new position so the pointer leaves no trail.
    pub(crate) mouse_draw_pos_x: u16,
    pub(crate) mouse_draw_pos_y: u16,

    // = seg001:dc46 cursor_hide_counter — a sign bit means the cursor is hidden;
    // redraw_mouse then skips the background restore. call_restore_cursor /
    // draw_mouse bracket screen updates that land under the software cursor,
    // nudging this negative (hidden, erased) then back to 0 (shown, redrawn);
    // redraw_mouse resets it to 0 each game-loop pass.
    pub(crate) cursor_hide_counter: i8,

    // = seg001:dc47 _byte_2D0F7_mouse_cursor_restore_needed — negative while
    // restore_mouse_if_rect_intersects has lifted the cursor off a dirty rect
    // and draw_mouse_cursor_if_needed owes the balancing re-show.
    pub(crate) mouse_cursor_restore_needed: i8,

    // = seg001:dce7 index_of_last_hovered_action_item — the verb slot
    // currently shown with the 0x8000 highlight, 0xff if none.
    // redraw_active_command_menu resets to 0xff at entry, then
    // highlight_hovered_text_action_item diffs against it each frame to know
    // which slot to un-highlight before painting the new hover.
    pub(crate) index_of_last_hovered_action_item: u8,

    // = the segvga A000:FA00 cursor-background save area and the geometry
    // vga_draw_cursor records (cs:[cursor_fb_pos/_width/_height]). The port keeps
    // `screen` exactly 320x200, so the save lives here rather than past the
    // visible framebuffer; vga_restore_cursor writes it back.
    pub(crate) cursor_save: Vec<u8>,
    pub(crate) cursor_save_pos: usize,
    pub(crate) cursor_save_w: u16,
    pub(crate) cursor_save_h: u16,

    // = seg001:dc5a game_clock_tick_base — PIT-counter reference snapshot
    // (`game_ticks() as u16`), taken when the room screen is presented and on
    // every mouse-button edge (seg000:d893); elapsed ticks are derived by
    // subtracting this base from the current `game_ticks() as u16`.
    pub(crate) game_clock_tick_base: u16,

    // = seg001:dc68 frame_tasks_last_tick — the PIT tick at the previous
    // process_frame_tasks pass; the elapsed delta drives the task accumulators.
    pub(crate) last_task_tick: u64,

    // Port-only: the game_ticks() value at the previous advance_game_clock pass
    // (mirrors last_task_tick). DOS has no equivalent — its PIT ISR advances the
    // clock per hardware tick; the port consumes the elapsed-tick delta once per
    // game_loop pass instead.
    pub(crate) game_clock_last_tick: u64,

    // = seg001:dc6a task_count / seg001:dc6c frame_tasks[] — the frame-task
    // table (DOS: up to 20 { interval:u16, accumulator:u16, callback:near }
    // entries). See add_frame_task / remove_frame_task / remove_all_frame_tasks.
    pub(crate) frame_tasks: Vec<FrameTask>,

    // = seg001:dce6 _byte_2D196_in_transition? — set while a screen transition /
    // deferred-task drain is in progress; draw_room_game_screen clears it before
    // the render. See dismiss_stacked_menus.
    pub(crate) in_transition: u8,

    // = seg001:dc4b data_0dc4b — set by the post-arrival path
    // (seg000:4fe8 / 5046) to request one game_loop pass through the idle
    // animation chooser (loc_0d962) instead of the regular mouse poll. Reset
    // to 0 at game_loop entry (seg000:d81b).
    pub(crate) idle_anim_trigger: u8,
}

impl GameState {
    /// Construct a `GameState` with its own idle input state. Suitable for
    /// headless renders/tests where no events ever arrive.
    pub fn new(dat_file: DatFile, frame_sink: impl FrameSink + 'static) -> Self {
        Self::new_with_input(dat_file, frame_sink, InputState::shared())
    }

    /// Construct a `GameState` polling `input` for keyboard/mouse. The windowed
    /// binary passes the same handle its winit event loop writes to.
    pub fn new_with_input(
        dat_file: DatFile,
        frame_sink: impl FrameSink + 'static,
        input: SharedInput,
    ) -> Self {
        Self::new_with_input_and_cursor(
            dat_file,
            frame_sink,
            input,
            CursorMode::Baked,
            SharedCursor::new(),
            std::sync::Arc::new(Recorder::new()),
        )
    }

    /// Construct a `GameState` choosing whether the cursor is baked into the
    /// framebuffer (DOS-faithful) or published for a present-time GPU
    /// overlay.
    pub fn new_with_input_and_cursor(
        dat_file: DatFile,
        frame_sink: impl FrameSink + 'static,
        input: SharedInput,
        cursor_mode: CursorMode,
        shared_cursor: SharedCursor,
        recorder: std::sync::Arc<Recorder>,
    ) -> Self {
        let mut dat_file = dat_file;
        let font = Font::new(&dat_file.read("DNCHAR.BIN").expect("load DNCHAR.BIN"));
        let command_bin = dat_file.read("COMMAND1.HSQ").expect("load COMMAND1.HSQ");
        let frame_tasks = Vec::<FrameTask>::with_capacity(20);
        let pcm_player = PcmPlayer::new(PCM_OUTPUT_RATE, std::sync::Arc::clone(&recorder));
        let midi = midi::Midi::new(std::sync::Arc::clone(&recorder));
        Self {
            headless: false,
            debug_overlay: false,
            debug_overlay_key_down: false,
            debug_advance_phase_key_down: false,
            custom_save_key_down: false,
            // ---- Host/runtime state and buffers ----
            dat_file,

            screen: FrameBuffer::new(320, 200),
            screen_pal: Palette::new(),

            y_offset: 24,
            line_pattern: 0xffff,
            framebuffer: FrameBuffer::new(320, 200),
            framebuffer_saved: FrameBuffer::new(320, 200),
            framebuffer_back: FrameBuffer::new(320, 200),
            palette: Palette::new(),
            palette_fade_target: Palette::new(),
            global_frame_count: 0,

            font,
            font_state: FontState::default(),

            command_bin,

            talking_head: None,

            hnm_sd_block: None,
            hnm_ticks_per_frame: 0,
            hnm_last_frame_tick: 0,
            hnm_y_offset: 0,

            hnm_audio_active: false,
            hnm_audio_tc: 0,

            midi,

            pcm_player,

            recorder,

            game_start: std::time::Instant::now(),
            frame_sink: Box::new(frame_sink),

            cursor_mode,
            base_cursor_mode: cursor_mode,
            shared_cursor,

            input,
            prev_mouse_buttons: 0,
            intro_aborted: false,
            intro_skip_to_game: false,

            // = segvga:2768/276a static init `dw 8` / `dw 1`.
            transition_col: 8,
            transition_frame: 1,

            // Placeholder; intro_28_init re-creates it seeded with the live
            // palette when the night attack starts.
            attack: None,

            // ---- seg001 data-segment globals (sorted by address) ----
            rand_bits: 0,
            game_time: 2,
            location_and_room: 0x200a,
            location_appearance: 0x180,
            data_00008: 0x20,
            data_00009: 0,
            bitfield_paul_events: 0,
            current_room: 0x0a,
            pending_destination_room: 0,
            previous_room: 0,
            persons_met: 0,
            persons_travelling_with: 0,
            persons_in_room: 0,
            persons_talking_to: 0,
            line_spoken_this_conversation: 0,
            data_0001b: 0,
            pending_room_action: 0,
            spice_in_stock: 0,
            area_controlled_by_atreides: 0,
            area_controlled_by_harkonnen: 0,
            todays_spice_production: 0,
            // = the seg001:00a8 static init 0x186 (390 -> "3900" in the SEE
            // RESULTS ×10 display), shown until the first new-day recompute
            // (accumulate_potential_spice_harvest).
            potential_spice_harvest: 0x186,
            data_000aa: 0,
            data_000ac: 0x1b58,
            previous_day_spice_production: 0,
            spice_production_better_than_previous_day: 0,
            spice_production_lower_than_previous_day: 0,
            spice_shipment_quantity: 0,
            spice_shipment_fulfilment: 0,
            spice_shipment_flags: 0,
            final_attack_stage: 0,
            spice_shipment_sequence_number: 0,
            number_of_sietches_attacked_by_harkonnen: 0,
            person_marker_base: 0,
            data_000c6: 0,
            days_left_until_spice_shipment: 0,
            contact_distance_related_ds_d5: 0,
            number_of_sietches_visited: 0,
            number_of_rallied_troops: 0,
            number_of_rallied_troops_for_leto_killed: 0xff,
            // = seg001:1154/1156 both static init 0xffff — disarmed until
            // their phase callbacks stamp them.
            harkonnen_raids_armed_after_game_time: 0xffff,
            illness_plot_armed_after_ingame_day: 0xffff,
            spice_stock_at_last_new_day: 0,
            spice_spent_today: 0,
            ingame_day_of_last_spice_shipment_event: 0,
            spice_shipment_unpaid: 0,
            harkonnen_raid_suppress_once: 0,
            ecology_lfsr_state: 1,
            smugglers: crate::smugglers::SMUGGLERS,
            for_condit_desert_walk_ds_f5: 0,
            number_of_locations_with_illness: 0,
            chani_troop_illness_cure_progress: 0,
            latest_location_with_illness: 0,
            game_phase_copy_ds_fe: 0,
            events_pump_active: 0,
            room_redraw_request: 0,
            map2: Box::new([]),
            map_overlay_panel_pos: (0, 0),
            map_overlay_panel_rect: Rect::default(),
            map_overlay_mode: 0,
            globe_param_3: 0,
            globe_param_4: 0,
            vision_messages: Vec::new(),
            comm_sightings: Vec::new(),
            scene_records: crate::room_scene::SCENE_RECORDS,
            charisma: 0,
            discovered_sietch_count: 0,
            entering_new_sietch: 0,
            game_phase: 0,
            days_since_last_game_phase_change: 0,
            night_attack_stage: 0,
            // = seg001:11bd/11bf both init dw 0aah (the log head lives in
            // dialogue_played_log's length).
            book_bookmark_ptr: 0xaa,
            book_topic_filter: 0,
            book_page_video_id: 0,
            data_000c8: 0,
            ui_hud_head_index: 0,
            data_000ea: 0,
            data_000e1: 0,
            desert_walk_counter: 0,
            room_view_toggle: 0xff,
            data_000fc: 1,
            locations: LOCATIONS,
            troops: TROOPS,
            fremen1_troop: None,
            fremen2_troops: [None; 8],
            harkonnen_captain_troop: None,
            selected_fremen2: 0,
            data_000ed: 0,
            data_000ee: 0,
            vegetation_started_on_dune: 0,
            troop_condit: Default::default(),
            location_condit: Default::default(),
            prospector_destinations: [0; 4],
            condit_staged_location: 0,
            current_location_index: 0xffff,
            last_location_index: 0,
            companions: [-1, -1],
            ui_hud_companion_blink: [0, 0],
            companion_blink_step_latch: 0,
            data_011bc: 0,
            travel_destination_ptr: 0,
            travel_heading: 0,
            travel_heading_mode: 0,
            game_screen_mode_flags: 0,
            data_011ca: 0,
            travel_no_location_dest: 0,
            travel_step_accum: 0,
            data_01968: 0,
            data_0196a: 0,
            data_0196c: 0,
            last_event_game_time: 0,
            location_visibility_distance: 1,
            // = the seg001:197c/197e compiled-in statics (0x1964, -4) — the
            // orientation the intro2 globe scene renders before anything
            // re-seeds the view centre.
            zoomed_globe_longitude: 0x1964,
            zoomed_globe_latitude: -4,
            troop_icons: Vec::new(),
            troop_icon_draw_by_depth: false,
            troop_icon_focused: [None; 2],
            troop_icon_anim_phase: 0,
            map_view_reentry_count: 0,
            map_popup_ptr: 0,
            map_popup2_ptr: 0,
            map_selected_troop_id: 0,
            map_last_selected_troop_id: 0,
            map_info_popup_troop: None,
            map_contact_troop: None,
            map_contact_troop_pending: None,
            map_contact_popup_rect: crate::troop_map_screen::TROOP_CONTACT_POPUP_RECT,
            map_contact_head_rect: Rect::default(),
            map_contact_subtitle_pos: (0, 0),
            contacting_troops_ds_4c: 0,
            nearest_location: NearestLocation::default(),
            nearest_village: NearestLocation::default(),
            nearest_sietch: NearestLocation::default(),
            nearest_atreides_area: NearestLocation::default(),
            nearest_harkonnen_area: NearestLocation::default(),
            head_popup_anchor: (0, 0),
            head_popup_box: Rect::default(),
            map_info_panel_rect: Rect::default(),
            map_popup_anim_src: (0, 0),
            map_popup_anim_rect: Rect::default(),
            map_popup_anim_suppress: false,
            xor_bracket_anim_move_step: (0, 0),
            xor_bracket_anim_expand_step: (0, 0),
            xor_bracket_anim_center: (0, 0),
            xor_bracket_anim_shape: (0, 0, 0, 0),
            map_location_popup_loc: None,
            map_location_popup_class: 0,
            map_location_popup_rect: Rect::default(),
            map_renderer: MapRenderer::new(),
            globe_renderer: None,
            globe_rotation: 0,
            globe_tilt: 0,
            globe_decoration_offset: 0,
            globe_draw_area_control_colors: 0,
            results_gauge_targets: [0; 6],
            results_gauge_current: [0; 6],
            results_stats_timestamp: 0,
            results_prev_values: [0; 6],
            results_trend_glyphs: [0; 6],
            ui_elements: UI_ELEMENTS_INIT,
            // = the static seg001 menu buffers, initialized to their compiled-in
            // contents (priority byte + records; command_menu_buf and
            // menu_multiple_cancel start empty and are filled by their builders).
            command_menu_buf: menu_defs::COMMAND_MENU_BUF.into(),
            menu_npc_actions: menu_defs::MENU_NPC_ACTIONS.into(),
            menu_go_towards_this_place: menu_defs::MENU_GO_TOWARDS_THIS_PLACE.into(),
            menu_destination_warning: menu_defs::MENU_DESTINATION_WARNING.into(),
            menu_prospector_continue: menu_defs::MENU_PROSPECTOR_CONTINUE.into(),
            menu_continue: menu_defs::MENU_CONTINUE.into(),
            menu_dynamic: menu_defs::MENU_DYNAMIC.into(),
            menu_comms_room_messages_viewed: menu_defs::MENU_COMMS_ROOM_MESSAGES_VIEWED.into(),
            menu_argue_accept_refuse: menu_defs::MENU_ARGUE_ACCEPT_REFUSE.into(),
            menu_done: menu_defs::MENU_DONE.into(),
            menu_mixer_panel: menu_defs::MENU_MIXER_PANEL.into(),
            menu_book: menu_defs::MENU_BOOK.into(),
            menu_globe: menu_defs::MENU_GLOBE.into(),
            menu_globe_default_click_on_globe: menu_defs::MENU_GLOBE_DEFAULT_CLICK_ON_GLOBE.into(),
            menu_music: menu_defs::MENU_MUSIC.into(),
            menu_save_game: menu_defs::MENU_SAVE_GAME.into(),
            menu_load_game: menu_defs::MENU_LOAD_GAME.into(),
            menu_restart_load_exit_game: menu_defs::MENU_RESTART_LOAD_EXIT_GAME.into(),
            menu_exit_game_confirmation: menu_defs::MENU_EXIT_GAME_CONFIRMATION.into(),
            menu_palace_mirror_room: menu_defs::MENU_PALACE_MIRROR_ROOM.into(),
            menu_go_there_flying_an_orni: menu_defs::MENU_GO_THERE_FLYING_AN_ORNI.into(),
            menu_go_there_riding_a_worm: menu_defs::MENU_GO_THERE_RIDING_A_WORM.into(),
            menu_map_troops: menu_defs::MENU_MAP_TROOPS.into(),
            menu_troop_dialog: menu_defs::MENU_TROOP_DIALOG.into(),
            menu_next_troop: menu_defs::MENU_NEXT_TROOP.into(),
            menu_cancel: menu_defs::MENU_CANCEL.into(),
            menu_move_prospectors: menu_defs::MENU_MOVE_PROSPECTORS.into(),
            menu_change_troop_destination: menu_defs::MENU_CHANGE_TROOP_DESTINATION.into(),
            menu_select_troop_occupation: menu_defs::MENU_SELECT_TROOP_OCCUPATION.into(),
            menu_occupation_for_spice_troop: menu_defs::MENU_OCCUPATION_FOR_SPICE_TROOP.into(),
            menu_occupation_for_army_troop: menu_defs::MENU_OCCUPATION_FOR_ARMY_TROOP.into(),
            menu_occupation_for_espionage_troop: menu_defs::MENU_OCCUPATION_FOR_ESPIONAGE_TROOP
                .into(),
            menu_occupation_for_ecology_troop: menu_defs::MENU_OCCUPATION_FOR_ECOLOGY_TROOP.into(),

            // = seg001:2220 dw menu_prospector_troop_after_specializing_in_
            //   spice — the static initial value.
            sequence_menu: MenuRef::MenuProspectorContinue,
            sequence_script: None,
            sequence_cursor: 0,
            sequence_return_cursor: None,
            sequence_saved_scene: (0, 0),
            sequence_blink: false,
            prospector_pick_queue: [0; 4],
            prospector_pick_count: 0,
            cmd_skip_to_destination_flags: 0,
            menu_stack: vec![(MenuRef::CommandMenuBuf, None)],
            data_0227d: 1,
            sky_skydn_selector: 0,
            active_mouse_handlers: &ROOM_MOUSE_HANDLERS,
            cursor_image: None,
            mouse_nav_rect: None,
            banks: Banks::new(),
            game_suspend_count: 1,
            settings_drag_target: 0,
            log_condit: false,
            log_subtitle: false,
            voice_subtitle_mode: 0,
            voice_subtitle_mode_default: 0,
            settings_records: SETTINGS_RECORDS_INIT,
            cmd_args_memory: 0,
            hnm_bytes: None,
            hnm_lop_bytes: None,
            hnm_lop_video_id: 0,
            hnm_lop_cursor: 0,
            hnm_lop_remaining: 0,
            music_playlist_flags: 0,
            music_cd_playlist: crate::music::MUSIC_CD_STANDARD_ORDER,
            music_cd_playlist_cursor: 0,
            music_song_end_tick_stamp: 0,
            rand_iterated_seed: 0,
            settings_flags: 0x1 | 0x4 | 0x8 | 0x100 | 0x400 | 0x800,
            music_desired_song: 0,
            globe_screen_active: 0,
            current_sky_palette: 0,
            sky_fade_countdown: 0,
            pending_room_screen_request: 0,
            data_046db: 0,
            new_time_period_pending: 0,
            new_day_flag: 0,
            sky_fade_active: false,
            data_046e0: 0,
            spice_harvest_remainder: 0,
            map_view_rect: Rect::default(),
            data_046eb: 0,
            spice_density_overlay_dirty: 0,
            current_main_view_drawing_function: None,
            data_046fc: 0,
            available_equipment: Equipment::default(),
            data_04726: 0,
            travel_active: 0,
            hnm_video_frame_ready: false,
            travel_minimap_state: 0,
            travel_trail_ring: [(0x800, 0x800); TRAVEL_TRAIL_LEN],
            travel_trail_cursor: 0,
            travel_step_tick_stamp: 0,
            travel_step_counter: 0,
            orni_hotspot_x: 0,
            orni_hotspot_y: 0,
            map_ornithopter_mode: 0,
            map_caption_text: Vec::new(),
            map_caption_pos: 0,
            map_caption_x: 0,
            map_caption_y: 0,
            map_caption_color: 0,
            map_player_marker_rect: Rect::default(),
            map_player_marker_phase: 0,
            visible_location_markers: Vec::new(),
            travel_vehicle_mode: 0,
            orni_anim_frame: 0,
            data_04732: 0,
            data_04735: 0,
            room_persons: ROOM_PERSON_TABLE_INIT,
            data_0476a: 0,
            data_0476b: 0,
            is_dialogue_active: false,
            room_render_flags: 0,
            dialogue_interrupt_gate: 0,
            data_047a6: 0,
            data_047a7: 0,
            data_047aa: 0,
            current_lip_sync_resource_id: 0,
            current_subtitle_id: 0,
            dialogue_topic_index: 0,
            data_047c2: 0,
            dialogue_current_record_ptr: 0,
            phrase_bin: Vec::new(),
            current_phrase_bin_id: 0,
            // = the seg001:11eb statics: identity COMMAND ids, except 0x8b
            // (0x108 "Paul Atreides"; the met-Stilgar callback rewrites it to
            // 0x109 "Muad'Dib"). Entries 1-2 are the staged location's
            // first/last-name ids once stage_location_name_placeholders runs.
            string_subst_id_table: [
                1,
                1,
                2,
                3,
                4,
                5,
                6,
                7,
                8,
                9,
                0x0a,
                cmd::PAUL_ATREIDES_108,
                0x0c,
                0x0d,
                0x0e,
                0x0f,
            ],
            subtitle_pad_left: 0,
            subtitle_pad_right: 0,
            subtitle_pad_top: 0,
            subtitle_pad_bottom: 0,
            subtitle_layout_flags: 9,
            balloon_x: 0x50,
            subtitle_bubble: None,
            data_047e0: 0,
            head_sign_state: 0,
            head_sign_anim: 0,
            head_sign_record: None,
            dialogue_line_word0: 0,
            dialogue_text_continuation: None,
            dialogue_end_request: 0,
            dialogue_resume_entry_ptr: 0,
            dialogue_played_log: Vec::new(),
            npc_menu_idle_timer_base: 0,
            npc_menu_idle_timer_limit: 0,
            character_screen_pos: [(0xffff, 0xffff); 0x17],
            dialogue: Default::default(),
            condit: Default::default(),
            map: Default::default(),
            tablat: None,
            ui_hud_head_saved_strip: vec![0; 20 * 10],
            ui_hud_head_animating_down: false,
            pause_enabled: 0,
            language_setting: 0,
            voc_bases: [0; 17],
            data_047dc: 0,
            rand_seed: 1,
            rand_bits_seed: 1,
            screen_buffer: FbId::Screen,
            active_fb: FbId::Fb1,
            hnm_finished: false,
            hnm_frame_counter: 0,
            hnm_counter_2: 0,
            hnm_counter_4: 0xffff,
            hnm_resource_data: 0,
            hnm_video_id: 0,
            hnm_active_video_id: 0,
            hnm_read_offset: 0,
            hnm_header_size: 0,
            hnm_body_offset: 0,
            hnm_framebuffer: FbId::Fb1,
            // = seg000:e65c..e662 initialize_system warps the pointer to its
            // startup position (237, 171) via warp_mouse_cursor (seg000:db03).
            mouse_pos_x: MOUSE_START_X,
            mouse_pos_y: MOUSE_START_Y,
            mouse_prev_drag_x: 0,
            mouse_prev_drag_y: 0,
            drag_armed_element: None,
            mouse_last_click_time: 0,
            data_0ceba: 0,
            mouse_draw_pos_x: 0,
            mouse_draw_pos_y: 0,
            // = seg000:e64a mov [cursor_hide_counter], 0ffh — the cursor starts
            // hidden; the first redraw_mouse pass (game_loop) clears the counter
            // and shows it.
            cursor_hide_counter: -1,
            mouse_cursor_restore_needed: 0,
            index_of_last_hovered_action_item: 0xff,
            cursor_save: Vec::new(),
            cursor_save_pos: 0,
            cursor_save_w: 0,
            cursor_save_h: 0,
            game_clock_tick_base: 0,
            last_task_tick: 0,
            game_clock_last_tick: 0,
            frame_tasks,
            in_transition: 0,
            idle_anim_trigger: 0,
        }
    }

    pub(crate) fn is_headless(&self) -> bool {
        self.headless
    }

    pub fn set_headless(&mut self) {
        self.headless = true;
        // Port-only: headless runs (tests, renders) default to music off — the
        // same cmd_args_memory bit 4 the MUSIC OFF verb sets (seg000:aeaf), so
        // check_music_enabled gates every music path; a MUSIC ON verb can still
        // clear it.
        self.cmd_args_memory |= 0x10;
        // Likewise default digital sound (PCM / voices) off — clear
        // settings_flags bit 0x1, the flag check_pcm_enabled reads. The headless
        // rigs have no audio drain, so a started narration clip would otherwise
        // spin out its 1000-tick timeout (duck_music_and_start_narration_voice_
        // clip / wait_for_narration_voice_clip both gate on check_pcm_enabled). A
        // caller that wants PCM can re-enable it with set_pcm_enabled(true).
        self.settings_flags &= !0x1;
        // Silence the audio backends outright (no card): PCM playback is refused
        // so voice waits skip up front, and the MIDI output is muted (song timing
        // still advances). This covers the intro/direct-call paths that bypass
        // the game-logic gates above.
        self.pcm_player.set_enabled(false);
        self.midi.set_enabled(false);
    }

    // = seg000:0000 start (the startup sequence after parse_command_line /
    // initialize_system / initialize_resources). Plays the intro and credits,
    // sets up the in-game UI, enters the room view (ui_enter_room_view) and
    // starts the game clock (reset_game_suspend). play_intro2's WORMSUIT
    // cutscenes, create_save_cl and game_loop are not ported yet.
    //
    // `skip_intro` is a port-only convenience (no DOS equivalent): when set it
    // jumps straight to the in-game UI, skipping the intro/credits/intro2.
    pub fn start(&mut self, skip_intro: bool) {
        // = initialize_system → initialize_resources, run before start in DOS
        // (the port front-loads the constructor's DNCHAR/COMMAND loads and defers
        // the rest; this brings in the resources interpreted at runtime).
        self.initialize_resources();

        // ESC anywhere in the intro skips straight into the game; a non-ESC key
        // or the mouse only ends the current phase. The flag threads through the
        // three calls (= the DOS ZF(esc) chained via each function's jz-at-entry).
        self.intro_skip_to_game = false;

        // = seg000:000d call play_intro.
        self.play_intro(skip_intro);

        // = seg000:0010 call play_CREDITS_HNM. Skipped when the intro was ended
        // with ESC (seg000:0309 jz loc_00331).
        self.play_credits(skip_intro || self.intro_skip_to_game);

        // = seg000:0013 call play_intro2. It self-skips its WORMSUIT cutscenes
        // when `skip_intro` is set (or ESC ended an earlier phase, seg000:0226 jz);
        // its tail sets the game up at the palace throne room (location_and_room
        // 0x200a / location_appearance 0x180) and resets fb_base_ofs to 0 for the
        // in-game screen.
        self.play_intro2(skip_intro || self.intro_skip_to_game);

        // = seg000:0016
        self.midi.midi_reset();

        // = seg000:0019 mov [music_playlist_flags], 0
        self.music_playlist_flags = 0;

        // = seg000:001e mov [game_time], 2 — start the in-game clock at 2 (the
        // PIT game-clock ISR that advances it is not ported yet).
        self.game_time = 2;

        // = seg000:0024 call init_game_ui (loc_00083).
        self.init_game_ui();

        // = seg000:0027 cl=0xff; call create_save_cl — not ported yet.
        // TODO

        // = seg000:002c call ui_enter_room_view (loc_01860).
        self.ui_enter_room_view();

        // = seg000:002f mov [pause_enabled], 0ffh — allow the P-key GAME PAUSED
        // window now that gameplay has begun.
        self.pause_enabled = 0xff;

        // = seg000:0034 call reset_game_suspend (loc_0b2be) — zero the suspend
        // counter so the in-game clock and idle animations start running.
        self.reset_game_suspend();

        // = seg000:0037 call game_loop — the in-game per-frame loop. The port
        // invokes it from the windowed runtime (bin/dune.rs) right after start()
        // returns, so headless setup renders/tests that call start() do not enter
        // its infinite loop.
    }

    // = seg000:00b0 initialize_resources (its seg000:00d1 initialize_resources2
    // body). DOS loads TABLAT (0xba), MAP (0xbf), DIALOGUE (0xbd) and CONDIT
    // (0xbc) here, then bump-allocates the COMMANDx/PHRASE buffers. The port
    // loads most of those lazily or in the constructor; this ports the CONDIT
    // load (seg000:0126) — the one resource interpreted purely at runtime.
    pub fn initialize_resources(&mut self) {
        self.dialogue = self
            .dat_file
            .read("DIALOGUE.HSQ")
            .expect("load DIALOGUE.HSQ");

        self.condit = self.dat_file.read("CONDIT.HSQ").expect("load CONDIT.HSQ");

        // = seg000:00d3..00e5 load TABLAT.BIN and byte-swap its words (Tablat
        // reads big-endian, the equivalent). The seg000:00e7 loop's derived
        // per-row table (data_04880, 0x10000 / row length) has no ported
        // reader yet.
        let tablat = self.dat_file.read("TABLAT.BIN").expect("load TABLAT.BIN");
        let tablat: &[u8; 792] = tablat[..792].try_into().expect("TABLAT.BIN size");
        self.tablat = Some(Tablat::new(tablat));

        // = seg000:0106..0114 load MAP.HSQ (idx 0xbf); res_map_ofs = its centre
        // (the port keeps the whole buffer, see map.rs).
        self.map = self.dat_file.read("MAP.HSQ").expect("load MAP.HSQ");

        // = seg000:57ec/5481 open_resource_by_index(0x3a) — the MAP2.HSQ
        // spice layer the density overlay renders (DOS loads it on demand and
        // swaps res_map_seg to it; the port keeps it alongside the terrain).
        self.map2 = self.dat_file.read("MAP2.HSQ").expect("load MAP2.HSQ");

        // = seg000:018f..01c6 cache each location's map cell (also marks the
        // cell's map byte with the location bit 0x40).
        self.init_location_map_offsets();

        // = seg000:01c8..01df link every troop to its location (offset, map
        // cell, voice bank).
        self.init_troop_locations();

        self.build_voc_base_table();
    }

    // = seg000:d815 game_loop — the in-game per-frame loop.
    pub(crate) fn exit_to_dos(&mut self) -> ! {
        // Finalise any in-progress recording first: `std::process::exit` below
        // skips every destructor, so this is the only chance to mux the clip
        // when the player quits through the in-game EXIT GAME menu.
        self.recorder.stop();
        // = seg000:004e/0052 call MIDI_Reset / pcm_vtable_reset — silence audio
        //   before the process exits so the device is released cleanly.
        self.midi.midi_reset();
        self.pcm_player.stop();
        // = the INT 21/4C return to DOS.
        std::process::exit(0);
    }

    // Port-only: keep `cursor_mode` in sync with the recorder. While recording,
    // force `Baked` so `redraw_mouse` composites the cursor into the framebuffer
    // (which is what the recorder captures); restore the configured mode when it
    // stops. Called at the top of each loop pass's cursor work, on the game
    // thread, where the front buffer is the screen and no cursor state is
    // mid-flight — so the Baked save/restore invariants stay intact across the
    // switch.
    fn sync_recording_cursor_mode(&mut self) {
        let desired = if self.recorder.is_recording() {
            CursorMode::Baked
        } else {
            self.base_cursor_mode
        };
        if desired == self.cursor_mode {
            return;
        }

        if self.cursor_mode == CursorMode::Baked {
            // Leaving Baked: erase the baked cursor so it doesn't leave a stuck
            // imprint in the framebuffer, then present the cleaned frame.
            if self.cursor_save_h != 0 {
                gfx::vga_restore_cursor(self);
                self.send_frame_to_display();
            }
        } else if desired == CursorMode::Baked {
            // Entering Baked: zero the save footprint so the first
            // `vga_restore_cursor` is a no-op (no stale region gets repainted),
            // and invalidate the drawn position so the cursor is composited
            // fresh on this pass even if the pointer has not moved.
            self.mouse_draw_pos_x = u16::MAX;
            self.mouse_draw_pos_y = u16::MAX;
        }
        self.cursor_save_w = 0;
        self.cursor_save_h = 0;
        self.cursor_mode = desired;
    }

    pub fn game_loop(&mut self) {
        // = seg000:d815..d818 frame_tasks_last_tick = pit_timer_callback_counter
        //   — anchor process_frame_tasks's elapsed-since-last delta to "now".
        self.last_task_tick = self.game_ticks();
        // Anchor the game-clock delta to "now" as well (port-only; the DOS PIT
        // ISR needs no anchor since it advances the clock per hardware tick).
        self.game_clock_last_tick = self.game_ticks();
        // = seg000:d81b mov byte ptr [data_0dc4b], 0 — clear the idle-anim
        //   request so the first pass takes the normal mouse path.
        self.idle_anim_trigger = 0;
        loop {
            // = seg000:d820 loc_0d820 — the loop top.

            // Port-only: toggle the debug overlay on a backquote (`) key edge.
            // Read the raw key state so this does not consume the buffered
            // scancode the game's own key handling uses.
            self.poll_debug_overlay_toggle();

            // Port-only testing hotkey: `=`/`+` steps game_phase forward by one,
            // firing the usual phase triggers. Also reads the raw key state.
            self.poll_debug_advance_game_phase();

            // Port-only: F5 opens the custom named save/load panel (a blocking
            // modal loop in save_screen.rs). Also reads the raw key state.
            self.poll_custom_save_panel();

            // = seg000:d820..d82e — the Ctrl+V (scancode 0x2f + kb_keys[0x1d]
            // held; chani labels [0x1d] "_w" but 0x1d is Left Ctrl, not W)
            // one-shot debug cheat. handle_ctrl_v_once (seg000:b270) copies
            // 10 pre-canned `(dialogue_record_index, lip_sync_id<<3)` packed
            // words from seg001:242a into the dialogue-played log at
            // seg000:0xaa+ (the growing buffer whose head pointer lives in
            // dialogue_played_log_head, the port's dialogue_played_log Vec —
            // fire_event_callbacks at seg000:a07f appends one entry per
            // replayable spoken line), bumps the head, writes a 0-word
            // terminator, then self-modifies its own first instruction to
            // 0xc3 (RET) so it can fire only once per session.
            // TODO: port the cheat; it retroactively marks 10 specific
            //   dialogues as heard so gating that consumes the log behaves
            //   as if the player had already encountered them.

            // = seg000:d831 pending_room_screen_request == 0 -> run the
            // pre-swap hooks: ui_hud_companion_blink_task (seg000:d7b7, the
            // new-companion portrait blink) and loc_01b0d (seg000:1b0d), which
            // advances post-voice game state.
            if self.pending_room_screen_request == 0 {
                // = seg000:d838 call ui_hud_companion_blink_task.
                self.ui_hud_companion_blink_task();

                // = seg000:1b0d loc_01b0d -> run_events_for_current_time_period
                // (seg000:1b23). DOS gates the call on is_voc_pcm_playing /
                // game_suspend_count / [2a] < 0xc8; the port checks
                // game_suspend_count (the rest is unported state). The routine
                // itself consumes new_time_period_pending, so the flag pre-check
                // here just skips the call when nothing is pending.
                if self.new_time_period_pending != 0 && self.game_suspend_count == 0 {
                    self.run_events_for_current_time_period();
                }
            }

            // = seg000:d83e — process_frame_tasks also steps the per-frame
            // music pump (music_cd_playlist_service, seg000:d9d2). DOS's game
            // loop does NOT call service_midi_music: its mid-ramp switch
            // (status bit 0x40) would restart the playing song whenever a
            // narration duck or its end-of-line volume restore is ramping.
            self.process_frame_tasks();

            // Advance the in-game clock.
            let now = self.game_ticks();
            let elapsed = now.saturating_sub(self.game_clock_last_tick);
            self.game_clock_last_tick = now;
            self.advance_game_clock(elapsed);

            // = seg000:d841 if pending_room_screen_request != 0 apply the
            // swap. loc_00d8e (seg000:0d8e) is the actual room-screen
            // transition handler (reset_scene_lip_sync_state, frame-task
            // clear, voice/subtitle, then draw_room_game_screen via the
            // 0x80 | request byte). TODO: port; without it a request stays
            // pending and the room never swaps.
            if self.pending_room_screen_request != 0 {
                // TODO: port loc_00d8e (apply_pending_room_screen_request).
            }

            // = seg000:d84b call rand; mov [rand_bits], ax.
            self.rand_bits = self.rand();

            // = seg000:d851 call travel_pump — the in-game travel pump: while a
            //   flight is active (travel_active) it drives the flight HNM and a
            //   travel step every 0x300 ticks (travel_map_screen.rs).
            self.travel_pump();

            // = seg000:d854 if data_0dc4b != 0 take the idle-anim path
            //   (loc_0d962, seg000:d962); else the normal mouse poll +
            //   button-edge latch.
            let ax = if self.idle_anim_trigger != 0 {
                // TODO: port loc_0d962 — the post-arrival idle/glance animation
                //   chooser. Until then fall through to the mouse path so the
                //   pointer keeps tracking.
                self.idle_anim_trigger = 0;
                self.get_mouse_pos_etc();
                self.mouse_stuff()
            } else {
                // = seg000:d860 call get_mouse_pos_etc; call mouse_stuff.
                self.get_mouse_pos_etc();
                self.mouse_stuff()
            };

            // Port-only: while recording, force the software (baked) cursor so it
            // lands in the captured framebuffer. Switched here, before the pass's
            // cursor work, where the front buffer is the screen.
            self.sync_recording_cursor_mode();

            // = seg000:d866 call redraw_mouse — composite the cursor at its
            //   new position. DOS draws straight to VGA; the port presents
            //   only when the screen actually changed.
            if self.redraw_mouse() {
                self.send_frame_to_display();
            }

            // = seg000:d869..d87b latch the per-frame pointer motion delta:
            //   di = curX - prevX, cx = curY - prevY (the `xchg [data_0dc62/64];
            //   sub; neg` sequence). The drag handlers consume these.
            let drag_dx = self.mouse_pos_x.wrapping_sub(self.mouse_prev_drag_x) as i16;
            let drag_dy = self.mouse_pos_y.wrapping_sub(self.mouse_prev_drag_y) as i16;
            self.mouse_prev_drag_x = self.mouse_pos_x;
            self.mouse_prev_drag_y = self.mouse_pos_y;

            // = seg000:d87d mov si, [active_mouse_handlers] — the active screen
            //   record. = seg000:d881 and ax,0fh — keep the four button bits
            //   mouse_stuff produced: bit0 LMB-down, bit1 RMB-down, bit2 LMB-edge,
            //   bit3 RMB-edge.
            let handlers = self.active_mouse_handlers;
            let nibble = (ax & 0x0f) as u8;
            // = seg000:d884 jnz loc_0d893 — any button bit set takes the button
            //   branch; otherwise the idle/hover branch.
            if nibble == 0 {
                // = seg000:d886 call highlight_hovered_text_action_item.
                if self.highlight_hovered_text_action_item() {
                    self.send_frame_to_display();
                }
                // = seg000:d889..d88f the (cx|di) motion test only chooses between
                //   two equivalent fall-throughs; both reach call [si], the
                //   record's idle handler.
                (handlers.idle)(self);
            } else {
                // = seg000:d893 button branch. = seg000:d893..d897 stamp the
                //   interaction time (game_clock_tick_base = the PIT counter).
                self.game_clock_tick_base = self.game_ticks() as u16;

                // = seg000:d89b cmp data_04774,0; jnz — while a dialogue is on
                //   screen the only recognised input is a fresh LMB press (down +
                //   edge = bits 0|2 both set); it advances/skips the line.
                if self.is_dialogue_active {
                    // = seg000:d8a2 and al,5; cmp al,5; jnz loc_0d8d7.
                    if nibble & 0x05 == 0x05 {
                        // = seg000:d8a8 call call_restore_cursor; call loc_01707.
                        self.call_restore_cursor();
                        self.menu_callback_choice_continue_for_sequence(0, 0);
                    }
                } else {
                    // = seg000:d8b1 test al,5; jnz loc_0d8ba — if the LMB is not
                    //   involved (neither down nor edged) the event is the right
                    //   button: DOS biases the record base by one word (add si,2 ->
                    //   the rmb/rmb_release/rmb_drag slots) and shifts the RMB bits
                    //   down into the LMB positions (shr ax,1). The port selects the
                    //   RMB handler fields instead of biasing a pointer.
                    let rmb = nibble & 0x05 == 0;
                    let primary = if rmb {
                        (nibble >> 1) & 0x05
                    } else {
                        nibble & 0x05
                    };
                    // let button = self.prev_mouse_buttons;

                    // = seg000:d8ba and al,5; dec al; jnz loc_0d8f4 — al&5 is now 1
                    //   (down, no edge = held drag), 5 (down + edge = press), or 4
                    //   (edge up = release).
                    match primary {
                        // = seg000:d8c0 the held-button (drag) path.
                        0x01 => {
                            if let Some(armed) = self.drag_armed_element {
                                // = seg000:d8da an element is armed (a press landed
                                //   on a record with the 0x4000 repeat flag): re-fire
                                //   it once >= 0x32 PIT ticks have passed since the
                                //   last fire and the pointer is still over it. This
                                //   is the held-button auto-repeat (e.g. a +/- knob).
                                let elapsed = (self.game_ticks() as u16)
                                    .wrapping_sub(self.mouse_last_click_time);
                                if elapsed >= 0x32 && self.hit_test_ui_elements() == Some(armed) {
                                    // = seg000:d8ef call call_restore_cursor; jmp
                                    //   loc_0d92b.
                                    self.call_restore_cursor();
                                    self.dispatch_element_with_latch(armed);
                                }
                                // = seg000:d8e4/d8e9/d8ed otherwise (too soon, or the
                                //   pointer moved off the element) nothing fires.
                            } else if drag_dx != 0 || drag_dy != 0 {
                                // = seg000:d8c8..d8d4 nothing armed and the pointer
                                //   moved: dispatch the drag handler ([si+0ah], or
                                //   [si+0ch] for the right button) with the delta.
                                self.call_restore_cursor();
                                if rmb {
                                    (handlers.rmb_drag)(self, drag_dx, drag_dy);
                                } else {
                                    (handlers.drag)(self, drag_dx, drag_dy);
                                }
                            }
                        }
                        // = seg000:d8f4 the click path: a button edge (press at
                        //   al&5==5, release at al&5==4 — loc_0e26f is a no-op ret,
                        //   so `sub al,3; jz` selects release for the 4 case).
                        _ => {
                            // = seg000:d8f4 call call_restore_cursor — lift the
                            //   software cursor before a handler repaints under it;
                            //   redraw_mouse re-composites it next pass.
                            self.call_restore_cursor();
                            if primary == 0x04 {
                                // = seg000:d944 release: if a press armed an element,
                                //   clear the arm and fire the element one last time
                                //   ([di+0ch]); otherwise call the record's release
                                //   handler ([si+6], or [si+8] for the right button).
                                if let Some(armed) = self.drag_armed_element.take() {
                                    self.dispatch_element_with_latch(armed);
                                } else if rmb {
                                    (handlers.rmb_release)(self);
                                } else {
                                    (handlers.release)(self);
                                }
                            } else {
                                // = seg000:d8fe cmp si,[active_mouse_handlers]; jnz
                                //   loc_0d90e.
                                if rmb {
                                    (handlers.rmb)(self);
                                } else {
                                    self.game_loop_dispatch_lmb_press();
                                }
                            }
                        }
                    }
                }
            }

            // DOS does not sleep; the port paces to one PIT tick (~5 ms) so
            // the game thread does not burn a CPU.
            let start = self.game_ticks();
            self.sleep_ticks(start, 1);
        }
    }

    // = seg000:c085 set_backbuffer_as_frame_buffer — make the back buffer
    // (_word_2D0E2_framebuffer_back) the active framebuffer; drawing
    // primitives land there until set_fb1_as_active_framebuffer restores fb1.
    pub(crate) fn set_backbuffer_as_frame_buffer(&mut self) {
        self.active_fb = FbId::Back;
    }

    // = seg000:ef84..ef9b the game-clock tail of pit_timer_callback. While the
    // clock runs (game_suspend_count == 0) each PIT tick decrements data_046db;
    // on underflow it reloads from data_0146e (0x2ee0) and bumps game_time. The
    // reload period is 0x2ee0 + 1 ticks (the extra tick is the underflow that
    // goes negative) — ~60 s per game_time unit at 200 Hz, so ~16 min per
    // in-game day (16 ticks/day). Each bump also sets new_time_period_pending,
    // the flag game_loop's loc_01b0d consumes to refresh the date/time indicator
    // (run_events_for_current_time_period).
    //
    // `elapsed_ticks` is the number of PIT ticks since the previous call (DOS
    // runs it once per tick; the port batches a game_loop pass's worth).
    fn advance_game_clock(&mut self, elapsed_ticks: u64) {
        // = seg000:ef84 cmp byte ptr [game_suspend_count], 0; jnz loc_0ef9f.
        if self.game_suspend_count != 0 {
            return;
        }
        // = seg000:ef91 data_0146e — the divider reload value.
        const GAME_CLOCK_DIVIDER: i32 = 12000;
        // = seg000:ef8b dec word ptr [46dbh]; jns (skip while still >= 0).
        self.data_046db -= elapsed_ticks as i32;
        // = seg000:ef91..ef9b reload, inc game_time, and set
        // new_time_period_pending on each underflow.
        while self.data_046db < 0 {
            self.data_046db += GAME_CLOCK_DIVIDER + 1;
            self.game_time = self.game_time.wrapping_add(1);
            // = seg000:ef9b inc byte ptr [46ddh] — flag a new time period.
            self.new_time_period_pending = 1;
        }
    }

    // = seg000:0fd9 run_events_for_n_time_periods — advance the game clock by
    // `count` time periods, firing one period of scheduled events per step. Used
    // by the WAIT verbs (seg000:0f95) and the travel step clock (seg000:4b4a).
    // [46da] = 1 marks the pump active for the duration; the scheduler's
    // refresh tail (seg000:1bbf/1bdc) skips the room redraw while it is set —
    // the pump's caller presents once at the end instead.
    pub(crate) fn run_events_for_n_time_periods(&mut self, count: i16) {
        // = seg000:0fd9 data_046da = 1.
        self.events_pump_active = 1;
        // = seg000:0fde call reset_game_suspend — the pump runs the clock.
        self.reset_game_suspend();
        // = seg000:0fe1 or cx,cx; jle — nothing to do for a non-positive
        //   count (loc_01005 still clears the pump flag).
        if count <= 0 {
            self.events_pump_active = 0;
            return;
        }
        for _ in 0..count {
            // = seg000:0fe6 reload the clock divider so the free-running PIT
            //   clock does not also bump game_time mid-pump (data_0146e = 12000).
            self.data_046db = 12000;
            // = seg000:0fec cmp [new_hour_flag],0; jz — a period was already
            //   pending from the live clock, so run its events before the next.
            if self.new_time_period_pending != 0 {
                self.run_events_for_current_time_period();
            }
            // = seg000:0ff6 inc [game_time]; new_hour_flag = 1; run the events
            //   for the newly-entered period.
            self.game_time = self.game_time.wrapping_add(1);
            self.new_time_period_pending = 1;
            self.run_events_for_current_time_period();
        }
        // = seg000:1005 data_046da = 0.
        self.events_pump_active = 0;
    }

    // = seg000:390a drain_sky_fade — drain an in-flight sky cross-fade to completion,
    // running its frame task (loc_03916) back-to-back until the countdown hits 0.
    // Called before a time skip so the fade does not bleed into the new scene.
    pub(crate) fn drain_sky_fade(&mut self) {
        // = seg000:390a cmp [sky_fade_countdown],0; jz — nothing to drain.
        // = seg000:3911 call frame_task_callback_03916; jmp drain_sky_fade — loop.
        //   tick_sky_fade zeroes the countdown itself once the fade is disarmed,
        //   so the loop always terminates.
        while self.sky_fade_countdown != 0 {
            self.tick_sky_fade();
        }
    }

    /// Returns the number of game ticks since game start (200Hz, 4.99253ms per tick)
    pub fn game_ticks(&self) -> u64 {
        const TICK_NANOS: u64 = 4_992_530; // 4.99253ms
        let elapsed_nanos = self.game_start.elapsed().as_nanos() as u64;
        elapsed_nanos / TICK_NANOS
    }

    /// Sleeps until at least `ticks` have elapsed since `start`
    ///
    /// # Arguments
    /// * `start` - The starting tick count
    /// * `ticks` - Number of ticks to wait from start
    ///
    /// # Example
    /// ```ignore
    /// let start = game_state.game_ticks();
    /// // ... do work ...
    /// game_state.sleep_ticks(start, 4); // Sleep until 4 ticks have passed since start
    /// ```
    pub fn sleep_ticks(&self, start: u64, ticks: u64) {
        // println!("Sleeping {ticks} ticks from {start}");
        const TICK_NANOS: u64 = 4_992_530; // 4.99253ms

        let target_tick = start + ticks;
        let current_tick = self.game_ticks();

        if current_tick >= target_tick {
            // Already past target time, no need to sleep
            return;
        }

        let ticks_remaining = target_tick - current_tick;
        let sleep_duration = std::time::Duration::from_nanos(ticks_remaining * TICK_NANOS);

        std::thread::sleep(sleep_duration);
    }

    // = seg000:da25 add_frame_task — append a per-frame callback.
    pub(crate) fn add_frame_task(&mut self, interval: u16, task_id: TaskId) {
        self.frame_tasks.push(FrameTask {
            interval,
            accumulator: 0,
            task_id,
        })
    }

    // = seg000:da5f remove_frame_task — remove by id.
    pub(crate) fn remove_frame_task(&mut self, id: TaskId) {
        self.frame_tasks.retain(|t| t.task_id != id);
    }

    // = seg000:3a7c add_room_frame_task — (re)install the in-room frame task
    // (room_frame_task, interval 0x0c), but only for an actual in-game room: the
    // guard installs only when location_and_room has low byte 4 and high byte
    // < 0x20 — i.e. the cave/water rooms (confirmed: the dripping-cave scene
    // enters here with location_and_room = 0x0804). play_intro calls this after
    // each stage transition too, but its rooms (0x2002/0x2004/0x803/0x802) all
    // fail the guard, so the task installs only in gameplay.
    pub fn add_room_frame_task(&mut self) {
        // = seg000:3a7c call remove_room_frame_task — never install a duplicate.
        self.remove_room_frame_task();

        // = seg000:3a7f mov ax,[4]; cmp al,4; jnz / cmp ah,20h; jnb — install
        // only when location_and_room ([4], seg001:0004) has low byte 4 and
        // high byte < 0x20.
        let location_and_room = self.location_and_room;
        if (location_and_room & 0xff) == 4 && (location_and_room >> 8) < 0x20 {
            // = seg000:3a8b si=room_frame_task; bp=0ch; call add_frame_task.
            self.add_frame_task(0x0c, TaskId::Room);
        }
    }

    // = seg000:39e6 remove_room_frame_task.
    pub fn remove_room_frame_task(&mut self) {
        self.remove_frame_task(TaskId::Room);
    }

    // = seg000:0911 remove_all_frame_tasks.
    pub fn remove_all_frame_tasks(&mut self) {
        self.frame_tasks.clear();
        self.sky_fade_countdown = 0;
        // = seg000:0920 mov [_byte_22E3_sky_skydn_selector], 1.
        self.sky_skydn_selector = 1;
    }

    pub fn has_frame_tasks(&self) -> bool {
        !self.frame_tasks.is_empty()
    }

    // = seg000:d9d2 process_frame_tasks.
    pub fn process_frame_tasks(&mut self) {
        // = seg000:d9d2 call music_cd_playlist_service — step the CD-playlist
        // music streamer before polling the task array.
        self.music_cd_playlist_service();

        let now = self.game_ticks();
        let elapsed_raw = now.saturating_sub(self.last_task_tick);
        let elapsed = elapsed_raw.min(u16::MAX as u64) as u16;
        self.last_task_tick = now;

        let mut due = Vec::new();
        for task in &mut self.frame_tasks {
            if task.interval == 0 {
                due.push(task.task_id);
                continue;
            }

            // = seg000:d9f4 `cmp ax,bp; jnb` — fire when elapsed+accumulator
            // reaches the interval (>=), not strictly past it. The modulo (=
            // seg000:da0a `div bp`) carries the remainder so the period stays
            // exact; when not firing acc < interval, so `acc % interval == acc`,
            // matching DOS's plain `mov [si],ax` store on the not-due path.
            let acc = elapsed + task.accumulator;
            let fire = acc >= task.interval;
            task.accumulator = acc % task.interval;

            if fire {
                due.push(task.task_id);
            }
        }

        // Each task may call add/remove_frame_task during its callback (e.g. a
        // task removing itself when its clip ends, = lip_sync_stop's
        // remove_frame_task(0a7c2)); `due` was collected above so the mutation
        // doesn't disturb the in-flight scan.
        for task_id in due {
            match task_id {
                TaskId::HnmDoFrame => {
                    // = seg000:0070c
                    if self.hnm_do_frame() {
                        self.gfx_copy_whole_framebuf_to_screen();
                        self.send_frame_to_display();
                    }
                }
                TaskId::IntroNightAttack => {
                    self.tick_intro_night_attack();
                }
                TaskId::TalkingHeadIdle => {
                    self.tick_talking_head_idle();
                }
                TaskId::TalkingHeadVoc => {
                    self.tick_talking_head_voc();
                }
                TaskId::SkyPaletteCycler => {
                    self.tick_sky_palette_cycler();
                }
                TaskId::SkyFade => {
                    self.tick_sky_fade();
                }
                TaskId::Room => {
                    self.tick_room();
                }
                TaskId::PcmVoiceMusicRestore => {
                    self.tick_pcm_voice_music_restore();
                }
                TaskId::MapCaption => {
                    self.tick_map_caption();
                }
                TaskId::MapPlayerMarker => {
                    self.tick_map_player_marker();
                }
                TaskId::GlobeRotation => {
                    self.tick_globe_rotation();
                }
                TaskId::ResultsGauges => {
                    self.tick_results_gauges();
                }
                TaskId::TroopIconAnim => {
                    self.tick_troop_icon_anim();
                }
                TaskId::CreditsScroll => {
                    self.credits_scroll_frame_task();
                }
                TaskId::SequenceBlink => {
                    self.tick_sequence_blink();
                }
            }
        }
    }

    // = seg000:e3a0 wait_processing_frame_tasks.
    pub fn tick_one_frame(&mut self) {
        let start = self.game_ticks();
        self.process_frame_tasks();
        // `cmp ax,[0ce7a]; jz` spin — sleep on PIT tick instead of spinning.
        self.sleep_ticks(start, 1);
    }

    // === Input poll layer (the DOS keyboard helpers + any_key_pressed) ===

    // Present one frame during a screen transition: emit the current screen and
    // pace one frame interval, WITHOUT running frame tasks. DOS transitions
    // (segvga) step under their own vsync wait (`loc_segvga_02572`) and never
    // call `process_frame_tasks` — tasks resume only in the post-transition
    // wait loops — so the transition must not advance them here.
    //
    // loc_segvga_02572's vsync_polarity==0 path (the one taken when not polling
    // CRT retrace) spins until `[bp] - bx >= 3`, i.e. 3 PIT ticks per step. The
    // PIT runs at the same ~200Hz the port models, so this is 3 game ticks.
    pub fn present_transition_frame(&mut self) {
        let start = self.game_ticks();
        self.send_frame_to_display();
        // = loc_segvga_02572 `sub ax,bx; cmp ax,3; jb` — 3 ticks (~15ms).
        self.sleep_ticks(start, 3);
    }

    // = seg000:e387 wait_a_bit — run the driver for a fixed number of PIT
    // ticks, servicing the frame tasks (e3ae calls process_frame_tasks) and
    // breaking early on user input. Used for `stage.wait` style timed pauses,
    // which the player can skip.
    //
    // NOT seg000:e353 (wait_processing_frame_tasks_interruptable), despite the
    // similar shape: e353 only services tasks while suppress_sky_240_255
    // (data_0227d) is non-zero — the intro/cutscene state its callers bracket
    // themselves into. In-game that byte is 0 and e353 degenerates to a plain
    // timed spin; head_sign_lower depends on exactly that.
    pub fn wait_frame_tasks_for_ticks(&mut self, ticks: u64) {
        let deadline = self.game_ticks() + ticks;
        while self.game_ticks() < deadline {
            // = seg000:e36a call any_key_pressed; jb loc_0e386 — break out of
            // the timed wait as soon as a key/mouse press arrives.
            if self.any_key_pressed() {
                break;
            }
            self.tick_one_frame();
        }
    }

    // Run the driver until every registered task has signalled `Done`.
    pub fn wait_until_no_frame_tasks(&mut self) {
        while !self.frame_tasks.is_empty() {
            self.tick_one_frame();
        }
    }

    // = seg000:0704 intro_play_hnm_with_frame_task — install an HNM frame
    // task that decodes one frame whenever the per-clip tick interval has
    // elapsed. The task self-removes when the clip ends.
    pub fn play_hnm_with_frame_task(&mut self) {
        self.add_frame_task(5, TaskId::HnmDoFrame)
    }

    // = seg000:ca1b hnm_load_first_frame — open an HNM resource and decode its
    // first frame into the active framebuffer. Backed by the single-buffer
    // GameState decoder (crate::hnm); `name` resolves to a video id.
    pub fn hnm_load_first_frame(&mut self, name: &str, y_offset: i16) {
        self.hnm_load_first_frame_by_id(hnm_id_by_name(name), y_offset);
    }

    // = seg000:ca1b hnm_load_first_frame, the id form — DOS receives the video
    // id in ax (e.g. the travel flight open at seg000:3802 passes
    // travel_vehicle_mode directly).
    pub fn hnm_load_first_frame_by_id(&mut self, video_id: u16, y_offset: i16) {
        self.hnm_last_frame_tick = self.game_ticks();
        self.hnm_y_offset = y_offset;
        // A fresh clip starts with an empty pipeline (video_decode_buf_seg 0).
        self.hnm_video_frame_ready = false;
        // Reset audio-driven timing state. decode_sd_block below sets
        // hnm_audio_active when this clip carries SD chunks; clips without audio
        // leave it false and fall back to tick timing.
        self.hnm_audio_active = false;

        // = open + decode frame 0 into the active buffer (hnm_decode_frame targets
        // framebuffer_active and captures the frame's SD chunk).
        self.hnm_open_and_decode_first_frame(video_id);

        // = seg000:cae5 cmp al, [data_0dbff]: the per-frame tick interval for
        // clips without SD audio is the high byte of the resource flag word
        // (hnm_resource_data >> 8) — data_0dbff is that high byte (it overlaps
        // current_hnm_resource_flag at seg001:dbff). Audio clips pace on the
        // dnsdb queue instead and ignore this (hnm_audio_active).
        self.hnm_ticks_per_frame = (self.hnm_resource_data >> 8) as u64;

        // = seg000:ca37 call decode_sd_block — initialise the streaming audio
        // from the first SD chunk of the clip. The DOS engine only calls
        // decode_sd_block here; subsequent frames' SD chunks ride along via
        // copy_sd_chunk_to_pcm_buf from inside the HNM playback loop.
        self.decode_sd_block();

        self.global_frame_count += 1;
    }

    /// = seg001:0115 dnsdb_set_volume (vtable[7]) — set the master digital
    /// audio volume on the single dnsdb driver. Drives all PCM (voices + HNM
    /// video sound). The mixer panel's VOICES slider uses this; headless render
    /// examples set 0 to stay silent while the sample clock still advances.
    pub fn set_pcm_volume(&self, volume: u8) {
        self.pcm_player.set_volume(volume);
    }

    // = seg000:aa0f decode_sd_block — kick off PCM playback from the first
    // SD chunk of an HNM clip. The chunk's payload is a complete Creative
    // Voice File: a 0x1a-byte VOC header followed by a 6-byte Type-1 data
    // block header and then raw 8-bit unsigned mono samples. DOS strips a
    // fixed 0x20 (= 0x1a + 6) bytes off the front (seg000:aa30); the sample
    // rate comes from the Type-1 header's time-constant byte.
    //
    // = seg000:aa48..aa64 — DOS builds a same-sized silent lead-in buffer (job
    // 0x3819) and starts it FIRST, then queues the real first chunk (job
    // 0x3811). The silent lead-in keeps the dnsdb driver fed while the game
    // thread refills later chunks. We mirror that exactly: start_playback a
    // silence VOC, then queue_next the audio VOC, both on the single dnsdb
    // driver `pcm_player`.
    fn decode_sd_block(&mut self) {
        let Some(sd_block) = self.hnm_take_sd_block() else {
            // = seg000:aa12 inc ax; jz loc_0aa0e — no 'sd' chunk in this frame.
            return;
        };

        // = seg000:aa1a call pcm_stop_voc — drop any audio left over from a
        // previous clip before queueing this clip's first buffer.
        self.pcm_player.stop();

        if sd_block.len() < 0x20 || &sd_block[..19] != b"Creative Voice File" {
            // Not a VOC payload — bail rather than feed garbage to the driver.
            self.hnm_audio_active = false;
            return;
        }

        // Capture the time constant from the Type-1 data block (offset 4 within
        // the 6-byte header at 0x1a..0x20). Later frames carry raw samples that
        // reuse it (copy_sd_chunk_to_pcm_buf reuses the persistent job header).
        let tc = sd_block[0x1a + 4];
        self.hnm_audio_tc = tc;

        // = seg000:aa30 sub word ptr [_word_22CC5_res_remaining], 20h
        let samples = &sd_block[0x20..];

        let silence = build_pcm_voc(tc, &vec![0x80u8; samples.len()]);
        let audio = build_pcm_voc(tc, samples);
        // The lead-in plays once and chains to the queued audio (the terminator
        // prefers a queued job over a loop); the audio chunk loops if the queue
        // under-runs, matching the DOS loop flag 0x41 on each buffer.
        self.pcm_player.start_playback(&silence, 0);
        self.pcm_player
            .queue_next(&audio, pcm_player::VOC_LOOP_WHOLE);
        self.hnm_audio_active = true;
    }

    // = seg000:a9f4 (loc_0a9f4) / copy_sd_chunk_to_pcm_buf — every subsequent
    // HNM frame that carries an SD chunk refills the next ping-pong buffer and
    // hands it to the driver (driven from hnm_wait_for_frame at seg000:cafb).
    // The chunk body is raw samples reusing the captured time constant; wrap it
    // as a Type-1 VOC and queue_next it for gapless playback. The driver's
    // current/queued slots are the two ping-pong buffers (0x3811/0x3819).
    fn hnm_queue_sd_block(&mut self) {
        if !self.hnm_audio_active {
            return;
        }
        if let Some(sd_block) = self.hnm_take_sd_block() {
            let voc = build_pcm_voc(self.hnm_audio_tc, &sd_block);
            // = seg000:aa91 `mov byte ptr [si+6], 1; mov byte ptr [si+7], 41h` —
            // every HNM SD buffer is queued with the loop-whole flag (0x40), so
            // the last chunk loops if nothing replaces it; the play loop stops
            // the driver explicitly when the clip ends (e.g. seg000:cf3f).
            self.pcm_player.queue_next(&voc, pcm_player::VOC_LOOP_WHOLE);
        }
    }

    pub fn hnm_is_complete(&self) -> bool {
        // = check_if_hnm_complete: finished once the clip has played its last
        // frame (hnm_finished) or been closed.
        self.hnm_finished || !self.hnm_is_open()
    }

    // = seg000:c9f4 hnm_do_frame_and_check_if_frame_advanced — decode the next HNM
    // frame into the framebuffer iff the per-clip tick interval has
    // elapsed. Returns true when a frame was actually decoded. The screen
    // is NOT updated here; the foreground play loop calls
    // `gfx_copy_whole_framebuf_to_screen` after a successful advance
    // (mirroring `gfx_copy_whole_framebuf_to_screen` at seg000:0632).
    pub fn hnm_do_frame(&mut self) -> bool {
        // = seg000:ca60 cmp word ptr [35a6h], 0; jz loc_0ca9a. Once a
        // non-looping clip runs out of frames it is closed/finished. From then on
        // hnm_do_frame is a no-op: the frame task at loc_0070c keeps ticking (clc
        // = stay scheduled) but decodes nothing, so the screen holds the last
        // frame until play_intro's wait elapses.
        if !self.hnm_is_open() || self.hnm_finished {
            return false;
        }

        // = seg000:cad4 hnm_wait_for_frame. When the clip is carrying SD audio,
        // gate the frame advance on the dnsdb job-state byte — the DOS engine
        // takes the loc_0caf0 branch and waits (`[si+6]==1`) for the SB to pick
        // up the previously queued buffer. Here that is `queue_slot_filled`:
        // hold while a queued chunk has not yet been promoted to playing. When
        // there's no audio, fall back to the fixed [data_0dbff] tick path.
        if self.hnm_audio_active {
            if self.pcm_player.queue_slot_filled() {
                return false;
            }
        } else {
            let current_tick = self.game_ticks();
            let next_frame_tick = self.hnm_last_frame_tick + self.hnm_ticks_per_frame;
            if current_tick < next_frame_tick {
                return false;
            }
            self.hnm_last_frame_tick = current_tick;
        }

        // = seg000:cc9f xchg bp,[video_decode_buf_seg] — a frame the streaming
        // pipeline already decoded (hnm_present_flight_frame's loc_0caa0
        // prefetch) is consumed as-is; otherwise decode one now.
        // = ca80..ca8c: decode the next frame (into framebuffer_active = active_fb)
        // and advance. hnm_step_frame returns false if it stepped onto the
        // end-of-stream marker without decoding.
        if !std::mem::take(&mut self.hnm_video_frame_ready) && !self.hnm_step_frame() {
            return false;
        }

        palette_flush(self);

        self.hnm_queue_sd_block();

        true
    }

    // = seg000:c8fb loc_0c8fb — foreground-play an HNM clip (DOS ax = the
    // video id) to completion in the game area: open it into fb1, reveal the
    // first frame through the `bp` present callback, then pump frames,
    // presenting the game area after each advance and servicing the CD
    // playlist. Ends with the last frame snapshotted to fb2 and the clip
    // closed.
    pub(crate) fn play_hnm_to_completion(&mut self, video_id: u16, bp: fn(&mut GameState)) {
        // = c8fb call set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = c8ff call hnm_load_first_frame — the in-game fb row offset is 0.
        self.hnm_load_first_frame_by_id(video_id, 0);
        // = c902/c905 present the game area and flush the header palette.
        self.present_game_area();
        self.update_screen_palette();
        // = c909 call bp — the caller's first-frame reveal.
        bp(self);
        // = c90b loc_0c90b — pump to completion. DOS spins on
        // hnm_do_frame_and_check_if_frame_advanced; the port paces on ticks.
        while !self.hnm_is_complete() {
            if self.hnm_do_frame() {
                // = c910/c913 present the game area + the CD playlist service.
                self.present_game_area();
                self.music_cd_playlist_service();
            }
            self.tick_one_frame();
        }
        // = c91b snapshot the last frame to fb2; c91e jmp hnm_close_resource.
        self.copy_active_framebuffer_to_framebuffer_2();
        self.hnm_close();
    }

    // The buffer `id` resolves to. = dereferencing one of the segment globals.
    pub fn fb_mut(&mut self, id: FbId) -> &mut FrameBuffer {
        match id {
            FbId::Screen => &mut self.screen,
            FbId::Fb1 => &mut self.framebuffer,
            FbId::Saved => &mut self.framebuffer_saved,
            FbId::Back => &mut self.framebuffer_back,
        }
    }

    // Mutable references to two *distinct* framebuffers at once — the borrow
    // checker can't prove disjointness through fb_mut. Used where one buffer is
    // the source and another the destination, e.g. the HNM checkerboard 2x blit
    // reads the staging buffer (bp) and writes framebuffer_active. Panics if the
    // two ids are equal.
    pub fn fb_pair_mut(&mut self, a: FbId, b: FbId) -> (&mut FrameBuffer, &mut FrameBuffer) {
        use FbId::*;
        match (a, b) {
            (Screen, Fb1) => (&mut self.screen, &mut self.framebuffer),
            (Screen, Saved) => (&mut self.screen, &mut self.framebuffer_saved),
            (Fb1, Screen) => (&mut self.framebuffer, &mut self.screen),
            (Fb1, Saved) => (&mut self.framebuffer, &mut self.framebuffer_saved),
            (Saved, Screen) => (&mut self.framebuffer_saved, &mut self.screen),
            (Saved, Fb1) => (&mut self.framebuffer_saved, &mut self.framebuffer),
            _ => panic!("fb_pair_mut requires distinct framebuffers, got {a:?} and {b:?}"),
        }
    }

    // The current render target. = the buffer `_word_2D08A_framebuffer_active_seg`
    // points at. Drawing primitives blit here.
    pub fn active_fb_mut(&mut self) -> &mut FrameBuffer {
        self.fb_mut(self.active_fb)
    }

    pub fn active_fb(&self) -> FbId {
        self.active_fb
    }

    // True while the front buffer is redirected to fb1 (inside a stage init run
    // through gfx_call_bp_with_front_buffer_as_screen): "copy to screen" is then
    // a no-op so the visible screen stays untouched until the transition.
    pub fn front_buffer_is_fb1(&self) -> bool {
        self.screen_buffer == FbId::Fb1
    }

    // = seg000:c07c set_fb1_as_active_framebuffer.
    pub fn set_fb1_as_active_framebuffer(&mut self) {
        self.active_fb = FbId::Fb1;
    }

    // = seg000:c08e set_screen_as_active_framebuffer — active follows the
    // front-buffer pointer (Screen normally, Fb1 while redirected by
    // gfx_call_bp_with_front_buffer_as_screen).
    pub fn set_screen_as_active_framebuffer(&mut self) {
        self.active_fb = self.screen_buffer;
    }

    // = seg000:c097 gfx_call_bp_with_front_buffer_as_screen. Run `f` (a stage
    // init) with fb1 as the active target AND as the front buffer, so any draw
    // — including "copy to screen" — lands in fb1. The visible screen is left
    // untouched until the following transition reveals fb1. DOS does not
    // restore `active` afterward (it stays Fb1).
    pub fn gfx_call_bp_with_front_buffer_as_screen(&mut self, f: fn(&mut GameState)) {
        self.set_fb1_as_active_framebuffer();
        let saved = self.screen_buffer;
        self.screen_buffer = FbId::Fb1;
        f(self);
        self.screen_buffer = saved;
    }

    // = seg000:c412 copy_active_framebuffer_to_framebuffer_2. Snapshot the
    // active buffer into fb2 (the clean scene backup).
    pub fn copy_active_framebuffer_to_framebuffer_2(&mut self) {
        match self.active_fb {
            FbId::Screen => self.framebuffer_saved.copy_from(&self.screen),
            FbId::Fb1 => self.framebuffer_saved.copy_from(&self.framebuffer),
            FbId::Back => self.framebuffer_saved.copy_from(&self.framebuffer_back),
            FbId::Saved => {}
        }
    }

    // = seg000:0579 clear_global_y_offset. `xor ax,ax; call vga_set_fb_row`
    // — resets the framebuffer row offset used by
    // `gfx_copy_whole_framebuf_to_screen` to 0 so the next blit starts at
    // the top of the screen. The seg000 wrapper just calls the segvga
    // vtable primitive `vga_set_fb_row`.
    pub fn clear_global_y_offset(&mut self) {
        gfx::vga_set_fb_row(self, 0);
    }

    // = seg000:b2be reset_game_suspend — zero game_suspend_count, fully resuming
    // the in-game clock and idle animations. Called from start once gameplay
    // begins and after scene/menu transitions.
    pub fn reset_game_suspend(&mut self) {
        self.game_suspend_count = 0;
    }

    // = seg000:c0ad gfx_clear_active_framebuffer. Clears the buffer
    // `_word_2D08A_framebuffer_active_seg` points at (via the segvga
    // `vga_clear_screen` primitive).
    pub fn gfx_clear_active_framebuffer(&mut self) {
        gfx::vga_clear_screen(self);
    }

    // = seg000:c305 draw_sprite_clipped — blit sprite `id` from `sheet` top-left
    // at (x, y), clipped to `clip`.
    pub(crate) fn draw_sprite_from_sheet_clipped(
        &mut self,
        sheet: &SpriteSheet,
        id: u16,
        x: i16,
        y: i16,
        clip: Rect,
    ) {
        if let Some(sprite) = sheet.get_sprite(id) {
            self.draw_sprite_at_clipped(sprite, x, y, clip);
        }
    }

    // = seg000:c327 j_vga_blit_clipped — blit one parsed sprite into the active
    // framebuffer at (x, y) with the game-area clip rect.
    fn draw_sprite_at_clipped(&mut self, sprite: &Sprite, x: i16, y: i16, clip: Rect) {
        let fb = self.active_fb_mut();
        let _ = blit::Blitter::new(sprite.data(), fb)
            .at(x, y)
            .size(sprite.width(), sprite.height())
            .pal_offset(sprite.pal_offset())
            .rle(sprite.rle())
            .clip_rect(Some(clip))
            .draw();
    }

    // = seg000:c32f draw_sprite_list — like draw_icons_list_at_si, but each
    // sprite is clipped to the rect at [0d834h]. The intro guard list runs after
    // copy_game_area_rect_to_clip_rect (seg000:089f), so the clip is the game
    // area (_word_20920_game_area_rect = 0,0,320,152); without it the tall
    // guard sprites run past the game-area bottom (below Feyd). DOS clips in
    // fb_base_ofs-relative space then adds fb_base_ofs in calc_fb_offset; the
    // port carries fb_base_ofs in the draw position, so the clip rect gets it
    // too.
    pub(crate) fn draw_sprite_list_clipped_to_game_area(
        &mut self,
        list: &[(u16, i16, i16)],
        sheet: &SpriteSheet,
    ) {
        let yoff = self.y_offset as i16;
        let clip = Rect {
            x0: 0,
            y0: yoff,
            x1: 320,
            y1: 152 + yoff,
        };
        for &(idx, x, y) in list {
            let flip_x = idx & 0x4000 != 0;
            let flip_y = idx & 0x2000 != 0;
            if let Some(sprite) = sheet.get_sprite(idx & 0x1ff) {
                let _ = sprite_blitter(sprite, self.active_fb_mut())
                    .at(x, y + yoff)
                    .flip_x(flip_x)
                    .flip_y(flip_y)
                    .clip_rect(clip)
                    .draw();
            }
        }
    }

    // = seg000:c343 loc_0c343 — blit sprite `id` CENTERED on (x, y) (= seg000:c355
    // sub dx,width/2 ; seg000:c361 sub bx,height/2), clipped to `clip`.
    pub(crate) fn draw_sprite_centered_clipped(
        &mut self,
        sheet: &SpriteSheet,
        id: u16,
        x: i16,
        y: i16,
        clip: Rect,
    ) {
        if let Some(sprite) = sheet.get_sprite(id) {
            let cx = x.wrapping_sub((sprite.width() / 2) as i16);
            let cy = y.wrapping_sub((sprite.height() / 2) as i16);
            self.draw_sprite_at_clipped(sprite, cx, cy, clip);
        }
    }

    // = seg000:c432 clear_game_area — clear the game-area rect
    // (_word_20920_game_area_rect = {0,0,320,152}, offset by fb_base_ofs) of
    // the active framebuffer to colour 0 (segvga vga_clear_rect). The rect spans
    // the full 320px width across rows fb_base_ofs..fb_base_ofs+152 (the in-game
    // viewport), so it is a contiguous row band. draw_SAL (loc_037b5) calls this
    // before drawing a room, so a scene's unpainted/dithered pixels show black
    // rather than the previous stage's leftover framebuffer.
    pub fn clear_game_area(&mut self) {
        let y0 = self.y_offset as usize;
        let fb = self.active_fb_mut();
        let w = fb.w() as usize;
        let h = fb.h() as usize;
        let y1 = (y0 + 152).min(h);
        let start = (y0 * w).min(fb.pixels().len());
        let end = (y1 * w).min(fb.pixels().len());
        fb.pixels_mut()[start..end].fill(0);
    }

    // = seg000:c4cd gfx_copy_whole_framebuf_to_screen. Plain memcpy from
    // fb1 to the screen buffer — does NOT apply the y-offset (that is
    // applied to incoming blits inside the gfx module). Delegates to the
    // gfx-layer implementation.
    pub fn gfx_copy_whole_framebuf_to_screen(&mut self) {
        gfx::gfx_copy_whole_framebuf_to_screen(self);
    }

    // = seg000:c0f4 update_screen_palette — flush the live `palette` into the
    // displayed `screen_pal` (DOS uploads it to the VGA DAC). DOS skips the
    // flush while the front buffer is redirected to fb1 (seg000:c0f7 cmp
    // framebuffer_1_seg, screen_buffer_seg; jz ret) — an offscreen render must
    // not disturb the visible palette, which the following transition uploads
    // at the right moment. The flush itself (vga_palette_flush, segvga:0b0c,
    // the `call [3935h]` j_vga_palette_flush target) carries its own
    // dirty-version compare (`[0dbd6h]` vs `[0dbd8h]`) to skip redundant DAC
    // uploads; the port omits only that inner redundant-upload check, always
    // flushing via palette_flush. Call this after changing `palette` outside a
    // stage transition (play_intro flushes for transition stages) so
    // send_frame_to_display presents the new colours — see intro_21_play.
    pub fn update_screen_palette(&mut self) {
        // = seg000:c0f7 jz — while rendering offscreen (front buffer = fb1),
        // leave the visible palette untouched.
        if self.front_buffer_is_fb1() {
            return;
        }
        palette_flush(self);
    }

    /// Emit the current `(screen, screen_pal)` to the display thread.
    /// Used by foreground play loops that block on `hnm_do_frame` directly
    /// (the frame-task driver emits frames on its own).
    pub fn send_frame_to_display(&self) {
        if self.headless {
            return;
        }

        // Port-only presentation care: while a rect bracket has the software
        // cursor lifted for a screen update (restore_mouse_if_rect_intersects
        // left mouse_cursor_restore_needed negative and the balancing
        // draw_mouse_cursor_if_needed has not run yet), the framebuffer is
        // missing its baked cursor. Publishing now would flash a cursor-less
        // frame that DOS never showed — its mid-bracket VGA writes were
        // followed by the re-draw within microseconds. Skip the publish; the
        // bracket close (or the next redraw_mouse pass, which consumes a
        // bracket left open across passes) publishes the completed frame.
        // Deliberate hides (cutscenes, transitions, the per-click hide) go
        // through cursor_hide_counter alone and never set this flag, so their
        // presents flow unhindered.
        if self.cursor_mode == CursorMode::Baked && self.mouse_cursor_restore_needed < 0 {
            return;
        }

        // Port-only: composite the debug overlay onto a copy of the screen so
        // the game's own framebuffers stay clean (the overlay must never be
        // baked into fb1/fb2, which the render restores from).
        if self.debug_overlay {
            let mut fb = self.screen.clone();
            self.draw_debug_overlay(&mut fb);
            self.frame_sink.publish(fb, self.screen_pal.clone());
        } else {
            self.frame_sink
                .publish(self.screen.clone(), self.screen_pal.clone());
        }
    }

    // Port-only: flip `debug_overlay` on a backquote (`, scancode 0x29) key
    // press edge. Reads the raw kb_keys state (not the one-shot scancode
    // buffer) so it never steals a keypress from the game.
    pub(crate) fn poll_debug_overlay_toggle(&mut self) {
        const SCANCODE_BACKQUOTE: usize = 0x29;
        let down = self.input.lock().unwrap().kb_keys[SCANCODE_BACKQUOTE] != 0;
        if down && !self.debug_overlay_key_down {
            self.debug_overlay = !self.debug_overlay;
            // Push a frame right away so the overlay appears / disappears at
            // once, even on an otherwise static screen where nothing else
            // would trigger a present.
            self.send_frame_to_display();
        }
        self.debug_overlay_key_down = down;
    }

    // Port-only testing hotkey: on a `=`/`+` (scancode 0x0d) key-press edge,
    // raise game_phase by one through set_game_phase_and_trigger_callbacks so it
    // fires the usual per-phase triggers and callback, letting a tester step the
    // phase progression forward. Reads the raw kb_keys state (not the one-shot
    // scancode buffer) so it never steals a keypress from the game.
    pub(crate) fn poll_debug_advance_game_phase(&mut self) {
        const SCANCODE_EQUAL: usize = 0x0d;
        let down = self.input.lock().unwrap().kb_keys[SCANCODE_EQUAL] != 0;
        if down && !self.debug_advance_phase_key_down {
            let next = self.game_phase.saturating_add(1);
            self.set_game_phase_and_trigger_callbacks(next);
        }
        self.debug_advance_phase_key_down = down;
    }

    // Port-only: draw the debug overlay — a small panel of live game state in
    // the top-left corner — onto `fb` (a copy of the screen). Uses the glyph
    // font directly so it does not disturb the font pen/colour state the game
    // relies on.
    pub(crate) fn draw_debug_overlay(&self, fb: &mut FrameBuffer) {
        use crate::font::TextSize;

        // let day = self.get_ingame_day_in_ax();
        // (label, value) rows. The value column is placed at a fixed pixel x
        // past the widest label, so the values line up even though the glyph
        // font is proportional (space-padding would not align them).
        let rows: [(&str, String); 4] = [
            (
                "PHASE",
                // format!("{:#04x} ({})", self.game_phase, self.game_phase),
                format!("{}", self.game_phase),
            ),
            // (
            //     "LOC",
            //     format!("{:#06x} room {}", self.location_and_room, self.current_room),
            // ),
            // ("APPEAR", format!("{:#06x}", self.location_appearance)),
            // ("DAY", format!("{}  time {:#06x}", day, self.game_time)),
            // ("CHARISMA", format!("{}", self.charisma)),
            ("SIETCHES", format!("{}", self.number_of_sietches_visited)),
            ("RALLIED", format!("{}", self.number_of_rallied_troops)),
            // ("MET", format!("{:#06x}", self.persons_met)),
            // ("TRAVEL", format!("{:#06x}", self.persons_travelling_with)),
            // ("IN ROOM", format!("{:#06x}", self.persons_in_room)),
            ("DESERT WALK", format!("{}", self.desert_walk_counter)),
        ];

        let pad = 2u16;
        let line_h = 8u16;
        // fg 0x0f (bright), bg 0 (transparent).
        let color = 0x000f;

        // The small font's pixel width of a string (the sum of glyph advances,
        // = what draw_glyph steps by).
        let width = |s: &str| -> u16 {
            s.bytes()
                .map(|b| {
                    let c = if b & 0x80 != 0 { 0x40 } else { b };
                    self.font.glyph_width(c, TextSize::Small) as u16
                })
                .sum()
        };
        // Value column: past the widest label + a gap.
        let value_x = pad + rows.iter().map(|(l, _)| width(l)).max().unwrap_or(0) + 6;
        let box_w = rows
            .iter()
            .map(|(_, v)| value_x + width(v))
            .max()
            .unwrap_or(0)
            + pad;
        let box_h = pad * 2 + line_h * rows.len() as u16;

        // Background panel: a dithered dark box behind the text for legibility.
        for y in 0..box_h.min(fb.h()) {
            for x in 0..box_w.min(fb.w()) {
                if (x + y) & 1 == 0 {
                    fb.set(x, y, 0);
                }
            }
        }

        for (i, (label, value)) in rows.iter().enumerate() {
            let y = pad + i as u16 * line_h;
            let mut x = pad;
            for &b in label.as_bytes() {
                let c = if b & 0x80 != 0 { 0x40 } else { b };
                x += self.font.draw_glyph(fb, x, y, c, TextSize::Small, color);
            }
            let mut x = value_x;
            for &b in value.as_bytes() {
                let c = if b & 0x80 != 0 { 0x40 } else { b };
                x += self.font.draw_glyph(fb, x, y, c, TextSize::Small, color);
            }
        }
    }

    // = seg000:c4dd present_game_area — present the game-area rect (0,0)-
    // (320,152) from fb1 to the visible screen. Used wherever a screen redraws
    // its game area directly (the talking-head composite, the map screen, the
    // message viewer, ...).
    pub(crate) fn present_game_area(&mut self) {
        // = seg000:c4dd cmp mouse_pos_y,98h; jnb +; call call_restore_cursor —
        // repaint the saved background under the cursor when it sits in the game
        // area, so a stale cursor image is not baked into the pushed rect.
        if self.mouse_pos_y < 152 {
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

    // = seg000:c4f0 present_screen_rect — the tail of the presentation chain
    // (present_game_area jumps here, as does the settings-panel repaint).
    // Redraw the HUD head into fb1 when `rect` overlaps the head box (c4fb),
    // then push `rect` from fb1 to the visible screen (copy_rect_fb1_to_screen).
    pub(crate) fn present_screen_rect(&mut self, rect: Rect) {
        // = seg000:c4fb the head-redraw half — redraw the HUD head when the
        // 240..255 sky is not suppressed and `rect` overlaps the head box (x in
        // [0x7e,0xc2), bottom edge >= 0x89). The head must land in fb1 so the
        // copy below carries it, so force fb1 active around the draw (DOS's
        // callers already have fb1 active here).
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
    // visible screen. Called on its own (e.g. the night-attack particles,
    // seg000:c7cc) as well as via the present_screen_rect fall-through. An
    // empty rect does nothing; the copy is skipped while the front buffer is
    // redirected to fb1 (offscreen render, where DOS's copy targets fb1 and the
    // real screen must stay untouched) or the mixer panel owns the mouse
    // handlers (loc_0c526).
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

    // = seg000:127c is_Gurney_Halleck_and_between_game_phases_15_and_20 — true
    // when `npc` is Gurney (4) and the story phase is in [0x15, 0x20). The
    // PALACE PLAN tally drops Gurney during those phases (he is not yet a palace
    // resident).
    pub(crate) fn is_gurney_between_phases_15_and_20(&self, npc: u8) -> bool {
        // = seg000:127c cmp npc,4; jnz clc/ret.
        if npc != 4 {
            return false;
        }
        // = seg000:1280 cmp [game_phase],15h; jb; cmp [game_phase],20h; ret —
        //   carry (the caller's skip) iff 0x15 <= game_phase < 0x20.
        (0x15..0x20).contains(&self.game_phase)
    }

    // = seg000:5b6e loc_05b6e — draw a 4-deep bevelled rectangle border. Starting
    // from the inner rect (x0, y0)-(x1, y1) and colour `color`, paint four
    // concentric outlines growing outward by one pixel per ring, each two colour
    // indices lighter. The PALACE PLAN frames its right-side area with it.
    pub(crate) fn draw_nested_rect_outline(
        &mut self,
        mut x0: i16,
        mut y0: i16,
        mut x1: i16,
        mut y1: i16,
        mut color: u8,
    ) {
        // = seg000:5b79 bp=4 — four rings.
        for _ in 0..4 {
            // = seg000:5b7e dec dx; dec bx — the top-left grows up/left each ring.
            x0 -= 1;
            y0 -= 1;
            // = seg000:5b80 call draw_rect_outline.
            self.draw_rect_outline(x0, y0, x1, y1, color);
            // = seg000:5b85 inc di; inc cx — the bottom-right grows down/right.
            x1 += 1;
            y1 += 1;
            // = seg000:5b87 sub al,2 — step the colour.
            color = color.wrapping_sub(2);
        }
    }

    // = seg000:c560 draw_rect_outline — outline the rectangle (x0, y0)-(x1, y1)
    // in `color` as four vga_draw_line edges (top, bottom, left, right). The
    // bevel is axis-aligned, so the port fills the four edge runs directly into
    // the active framebuffer (applying fb_base_ofs / y_offset like every segvga
    // blit) rather than routing through the generic Bresenham vga_draw_line.
    // Each edge reloads the 16-bit line pattern (data_02772, seg000:c541) and
    // rotates it per pixel, plotting on the rotated-out bit (segvga:1a6d) —
    // 0xffff draws solid, the spice overlay's 0x5555 the dotted you-are-here
    // box. The clip rect (data_0276a) is not modelled.
    pub(crate) fn draw_rect_outline(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
        let yoff = self.y_offset as i16;
        let pattern = self.line_pattern;
        let fb = self.active_fb_mut();
        let w = fb.w() as i16;
        let h = fb.h() as i16;
        let mut plot = |x: i16, y: i16, pat: &mut u16| {
            let bit = *pat & 0x8000 != 0;
            *pat = pat.rotate_left(1);
            let py = y + yoff;
            if bit && (0..w).contains(&x) && (0..h).contains(&py) {
                fb.set(x as u16, py as u16, color);
            }
        };
        // = seg000:c569/c573 the top and bottom edges.
        let (mut top, mut bottom) = (pattern, pattern);
        for x in x0..=x1 {
            plot(x, y0, &mut top);
            plot(x, y1, &mut bottom);
        }
        // = seg000:c57d/c583 the left and right edges.
        let (mut left, mut right) = (pattern, pattern);
        for y in y0..=y1 {
            plot(x0, y, &mut left);
            plot(x1, y, &mut right);
        }
    }

    // = seg000:c0d5 blit_fb1_to_screen_effect — present fb1 to the visible screen
    // through the segvga vga_effect_dispatch vtable (effect = `al`). The full
    // dispatcher (vga_effect_dispatch, segvga:3200) reduces `effect` mod 0x1a and
    // jumps through blit_mode_dispatch_table (segvga:31e6) to one of 13 effects;
    // only the two the PALACE PLAN drives are wired here (every other effect —
    // transition_tick 0x0c, panel_anim 0x18, … — is invoked from its own ported
    // site). DOS scrolls live VGA memory, so the motion is visible as it runs;
    // the port renders each outer pass into `screen`, presents it, and paces one
    // PIT tick per pass (DOS has no explicit timer here — the scroll is paced
    // implicitly by CPU speed — so the 1-tick cadence is a port-side stand-in
    // that makes the reveal perceptible without pegging a core).
    pub(crate) fn blit_fb1_to_screen_effect(&mut self, effect: u8, rect: Rect) {
        match effect {
            // = blit_mode_dispatch_table[8] (segvga:31e6 → segvga:33ca)
            //   blit_scroll_rect_down: the open reveal. The source origin steps
            //   from y2-2 up to y1 (si -= 0x280 per pass), each pass redrawing a
            //   taller bottom-anchored window of fb1 at the rect top.
            0x10 => {
                let mut src_row = rect.y1 - 2;
                loop {
                    let start = self.game_ticks();
                    gfx::scroll_rect_down_pass(
                        &mut self.screen,
                        &self.framebuffer,
                        self.y_offset,
                        rect,
                        src_row,
                    );
                    self.send_frame_to_display();
                    self.sleep_ticks(start, 1);
                    // = jnb loc_033ef: the outer loop ends once the source origin
                    //   reaches the rect top (si -= 0x280 would borrow).
                    if src_row <= rect.y0 {
                        break;
                    }
                    src_row -= 2;
                }
                // = jmp vga_copy_rect: the final clean full-rect copy (identical
                //   to the last pass, mirroring the DOS tail jump).
                let yoff = self.y_offset as i16;
                let r = Rect {
                    x0: rect.x0,
                    y0: rect.y0 + yoff,
                    x1: rect.x1,
                    y1: rect.y1 + yoff,
                };
                gfx::vga_copy_rect(&mut self.screen, &self.framebuffer, r);
                self.send_frame_to_display();
            }
            // = blit_mode_dispatch_table[9] (segvga:31e6 → segvga:3429)
            //   blit_scroll_rect_up: the close reveal. The block height bx steps
            //   down by six per pass (110, 104, …, 2, then a final 0 pass);
            //   blit_scroll_rect_up has no tail vga_copy_rect (its fill blocks
            //   lay down every row of fb1).
            0x12 => {
                let mut bx = (rect.y1 - rect.y0) - 6;
                loop {
                    let start = self.game_ticks();
                    gfx::scroll_rect_up_pass(
                        &mut self.screen,
                        &self.framebuffer,
                        self.y_offset,
                        rect,
                        bx,
                    );
                    self.send_frame_to_display();
                    self.sleep_ticks(start, 1);
                    // = bx -= 6; jnb loc_03445 / cmp bx,-6; mov bx,0; jnz — a
                    //   borrow that lands on -6 ends the loop; any other borrow
                    //   runs one last pass at bx = 0.
                    let next = bx - 6;
                    if next >= 0 {
                        bx = next;
                    } else if next == -6 {
                        break;
                    } else {
                        bx = 0;
                    }
                }
            }
            // = blit_mode_dispatch_table[0] (segvga:31e6 → segvga:3581)
            //   blit_zoom_shimmer: blit the rect's interior from the clean
            //   fb1 source (ds, per the c0d6/c0da buffer setup) into the
            //   screen (es) at 2x scale around the rect top-left, cycling
            //   the 2x2 sub-pixel source offsets (zoom_tile_offsets,
            //   segvga:2fb7), until the caller's tick budget runs out (cx —
            //   the globe zoom box, globe_zoom_box_shimmer_step, passes 10).
            //   Every pass rewrites the whole interior from fb1, so anything
            //   drawn over the screen inside the rect (the previous zoom-box
            //   outline) is erased each pass.
            0x00 => {
                // = segvga:358b..359e half width/height; nothing on a flat
                //   rect.
                let half_w = ((rect.x1 - rect.x0) / 2) as usize;
                let half_h = ((rect.y1 - rect.y0) / 2) as usize;
                if half_w == 0 || half_h == 0 {
                    return;
                }
                let yoff = self.y_offset as usize;
                let w = self.screen.w() as usize;
                let origin = (rect.y0 as usize + yoff) * w + rect.x0 as usize;
                // = segvga:35a0 the entry tick, segvga:35bb..35c4 the loop
                //   until cx (10) ticks elapse.
                let start = self.game_ticks();
                let mut jitter = [0usize, 321, 1, 320].iter().copied().cycle();
                loop {
                    // = segvga:35c8 fb_blit_2x_scaled — lodsb from ds (fb1)
                    //   every other byte/row, stosw doubled into es (screen).
                    let off = jitter.next().unwrap();
                    let src = self.framebuffer.pixels();
                    let dst = self.screen.pixels_mut();
                    for j in 0..half_h {
                        let di = origin + 2 * j * w;
                        let si = di + off;
                        for i in 0..half_w {
                            let c = src[si + 2 * i];
                            dst[di + 2 * i] = c;
                            dst[di + 2 * i + 1] = c;
                            dst[di + w + 2 * i] = c;
                            dst[di + w + 2 * i + 1] = c;
                        }
                    }
                    self.send_frame_to_display();
                    // DOS repeats at CPU speed; pace one PIT tick per pass so
                    // the shimmer is perceptible without pegging a core.
                    self.sleep_ticks(self.game_ticks(), 1);
                    if self.game_ticks() - start >= 10 {
                        break;
                    }
                }
            }
            // = the remaining vga_effect_dispatch effects are unported; this
            //   dispatcher only serves the PALACE PLAN and GLOBE effects.
            other => {
                eprintln!("blit_fb1_to_screen_effect: unhandled effect 0x{other:02x}");
            }
        }
    }

    // = seg000:c0b6 room_frame_task — the general in-room frame task (interval
    // 0x0c). Advance the wipe-transition engine one step (vga_effect_dispatch
    // effect 0x0c = transition_tick); when its column reaches 0x18, fire the
    // cave water-drip sound (SN4.HSQ). No drip in rooms 0x2012 / 0x201a.
    pub fn tick_room(&mut self) {
        // = seg000:c0b6 call loc_0d41b — bp = current location_and_room.
        let location_and_room = self.get_location_and_room();
        // = seg000:c0b9/c0bf cmp bp,2012h / 201ah; jz ret.
        if location_and_room == 0x2012 || location_and_room == 0x201a {
            return;
        }
        // = seg000:c0c5 mov al,0ch; call blit_fb1_to_screen_effect → vga_effect_dispatch index 6
        // = transition_tick. Draws this frame's ripple band into the screen
        // buffer and returns the engine's new wipe column.
        let cx = gfx::transition_tick(self);
        // DOS draws straight to VGA memory, so the ripple is visible as it is
        // drawn; the port renders into `screen`, so present it after each band.
        self.send_frame_to_display();
        // = seg000:c0ca cmp cx,18h; jnz ret — only when the column hits 0x18.
        if cx != 0x18 {
            return;
        }
        // = seg000:c0cf mov al,4; jmp audio_start_voc — SN4.HSQ "drip in cave".
        self.audio_start_voc("SN4.HSQ");
    }

    // = seg000:d41b loc_0d41b — bp = *[21dah], the current location_and_room
    // (the top of the room navigation stack; the live value is mirrored at
    // seg001:0004). The port keeps it in `location_and_room`, written by
    // draw_location_room.
    pub fn get_location_and_room(&self) -> u16 {
        self.location_and_room
    }
}
