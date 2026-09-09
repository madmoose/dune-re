use crate::{
    CursorMode, CursorShapeId, DatFile, Equipment, Font, FontState, FrameBuffer, InputState,
    Location, MapPanelRef, Palette, PanelRecord, Rect, SpriteSheet, TalkingHead,
    attack::AttackState,
    cmd,
    frame_slot::FrameSink,
    game_phase::{PHASE_15_FIRST_VISION, PHASE_20_THUFIR_FOUND, PHASE_C8_GAME_WON},
    game_ui::{self, MouseHandlers, NavPanel, ROOM_MOUSE_HANDLERS, UI_ELEMENTS_INIT, UiElement},
    gfx::{self, blit, globe_renderer::GlobeRenderer, map_renderer::MapRenderer, palette_flush},
    hnm::hnm_id_by_name,
    input::SharedInput,
    locations::LOCATIONS,
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
    /// = seg001:dbd8 `_word_2D088_screen_buffer_seg` — the visible VGA buffer (DNVGA: 0xA000).
    Screen,
    /// = seg001:dbd6 `_word_2D086_framebuffer_1_seg` — the primary offscreen compose buffer.
    Fb1,
    /// = seg001:dbde `_word_2D08E_framebuffer_saved_seg` (fb2) — a saved clean copy of the
    /// scene, used to restore regions dirtied by sprites/cursor/the talking head.
    Saved,
    /// = seg001:dc32 `_word_2D0E2_framebuffer_back` — the globe/map scratch buffer. During a
    /// travel it holds the persistent flight minimap + trail, re-stamped over
    /// each decoded flight frame (hnm_present_flight_frame, seg000:4afd).
    Back,
}

const GAME_CLOCK_TICKS_PER_HOUR: i32 = 12000;

/// (loc_0e85c - travel_trail_ring) / 4 — the travel-trail ring capacity in
/// (longitude, latitude) pairs.
pub(crate) const TRAVEL_TRAIL_LEN: usize = (0xe85c - 0xe40c) / 4;

/// = seg000:3f59 — desert_exhaustion_counter saturates at this many steps.
pub(crate) const DESERT_EXHAUSTION_MAX: u8 = 20;

/// = seg000:918a / seg000:1b34 — counts at or above this select Paul's gaunt
/// talking-head portrait; the hourly decrement snaps anything below it to 0.
pub(crate) const DESERT_EXHAUSTION_GAUNT_THRESHOLD: u8 = 16;

pub const PCM_OUTPUT_RATE: u32 = 49716;
pub const MIDI_SAMPLE_RATE: u32 = 49716;

/// Identifies a frame task. Dune identifies tasks by function pointer, but
/// function pointers aren't reliably comparable in Rust so we use an id.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum TaskId {
    // = seg000:070c hnm_frame_task — the HNM frame player armed by
    // intro_play_hnm_with_frame_task.
    HnmDoFrame,

    // = seg000:0b45 night_attack_frame_task — intro_28 night-attack particle tick.
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

    // = seg000:0ab92 frame_task_callback_0ab92 — the narration clip's
    // per-tick monitor: pump the streaming refill (pcm_voice_stream_refill,
    // seg000:a9b9), then, once PCM playback ends, release the music ducking
    // and self-remove.
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

    // = seg000:4bb9 desert_harvester_frame_task — the desert harvester animation.
    DesertHarvester,

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

    // = seg000:2cc7 vision_shimmer_frame_task — the vision dream's fb1
    // water-ripple present (interval 6), installed by vision_dream_backdrop.
    VisionShimmer,
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

/// = seg000:a93f read_audio_file's chunk size (seg000:a950, `mov cx, 2000h`): a streamed
/// voice reaches the dnsdb driver 0x2000 file bytes at a time.
pub(crate) const PCM_VOICE_CHUNK: usize = 0x2000;

// = the seg000 streaming-voice state: the open file handle
// (_word_22CD1_pcm_voice_file_handle) with its cursor/remaining pair
// (data_0dbc0/data_0dbc4) and the ping-pong job buffers at 3811h/3819h.
// `Some` = the handle is open and pcm_voice_stream_refill keeps feeding the
// driver; dropping it to `None` (= close_pcm_voice_file_handle, seg000:a9a1)
// starves the stream, so the driver drains its queued chunks and goes idle.
pub(crate) struct PcmVoiceStream {
    /// The .VOC file bytes DOS reads from the handle.
    data: Box<[u8]>,
    /// = seg000:dbc0 data_0dbc0 — file offset of the next unread byte.
    offset: usize,
}

/// Build a header-less Creative Voice File holding a single Type-2
/// continuation block: raw samples that reuse the playing block's time
/// constant and codec. = the prefab header dnsdb_queue_next_impl writes over
/// a queued refill buffer (seg001:01db..01e9: type byte at ptr+2, its 24-bit
/// length = the job's byte count, data at ptr+6).
fn build_pcm_voc_continuation(samples: &[u8]) -> Vec<u8> {
    let mut voc = Vec::with_capacity(4 + samples.len());
    voc.push(2); // Type-2 continuation block
    voc.push((samples.len() & 0xff) as u8);
    voc.push(((samples.len() >> 8) & 0xff) as u8);
    voc.push(((samples.len() >> 16) & 0xff) as u8);
    voc.extend_from_slice(samples);
    voc
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

    pub(crate) ctrl_v_cheat_done: bool,

    pub log_condit: bool,

    // Port-only (--log-subtitle): emit the subtitle/speech-bubble "SUB" trace,
    // mirroring chani_egui --log-subtitle so the two logs diff line-for-line.
    pub log_subtitle: bool,

    // ---- Host/runtime state and buffers (not seg001 data-segment globals) ----
    pub dat_file: DatFile,

    pub screen: FrameBuffer,
    pub screen_pal: Palette,

    // = segvga:01a3 fb_base_ofs — the game-area top. Stored here as a row; DOS
    // keeps the row*320 byte offset and applies it to every blit.
    pub y_offset: u16,

    pub framebuffer: FrameBuffer,

    // = seg001:dbde _word_2D08E_framebuffer_saved_seg (fb2): a clean backup of the composed
    // scene; regions are restored from here under moving overlays. (The buffer
    // itself, not the seg001 selector word at seg001:dbde that points to it.)
    pub framebuffer_saved: FrameBuffer,

    /// = seg001:dc32 `_word_2D0E2_framebuffer_back` (FbId::Back) — the globe/map scratch
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

    // = seg001:47ac _dword_23C5C_COMMANDx_BIN — the COMMAND1.BIN command-string table
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

    // Port-only, NOT the original behaviour: when set, a sound effect will not
    // start while the mixer is still playing a voice, so an audible line is
    // never cut short by one. The game gates on voc_pcm_playing above, which a
    // teardown clears early — that is why a companion's refusal line is cut by
    // the ornithopter engine loop, and how much survives depends on machine
    // speed. This makes the gate mean what it looks like it means instead.
    //
    // It only covers effects. An explicit pcm_stop_voc still stops a clip, so
    // routes that reach seg000:478c quickly (the map menu's GO THERE verb,
    // which never arms the takeoff animation) still truncate the line.
    // Wired to --let-voices-finish; off by default.
    pub let_voices_finish: bool,

    pub(crate) audio_current_sfx: Option<String>,
    pub(crate) audio_current_sfx_data: Vec<u8>,

    // The streamed voice clip being fed to the driver in PCM_VOICE_CHUNK
    // pieces; see [`PcmVoiceStream`].
    pub(crate) pcm_voice_stream: Option<PcmVoiceStream>,

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

    pub(crate) attack: Option<AttackState>,

    // = the growing 0-terminated word list at cs:0xaa.. whose head pointer is
    // dialogue_played_log_head (seg001:11bd) — the dialogue-played log: one
    // packed (entry_index | lip_sync_id << 11) word per replayable spoken line,
    // appended by fire_event_callbacks (seg000:a07f) and pre-filled by the
    // Ctrl+V cheat (seg000:b270, unported). Savegames carry it.
    pub(crate) dialogue_played_log: Vec<u16>,

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
    // advances it. get_ingame_day_3_periods_later reads (game_time+3)>>4.
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
    // (npc_shuffle_on_arrival) reads it as "where the player is now".
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
    pub(crate) persons_in_room: u16,

    // = seg001:0014 _word_1F4C4_persons_talking_to — the person the player is
    // currently in dialogue with.
    pub(crate) persons_talking_to: u16,

    // = seg001:0016/0018 for_condit_ds_16 / for_condit_ds_18 — the
    // per-presented-line speaker seeds (loc_094f3, seg000:94f3): ds:16 =
    // game_time minus the speaker's room-person travel timestamp, ds:18 = the
    // speaker's room-person flags byte. Conditions test ds:18 bit 0x40
    // (travelling with Paul — Jessica's "I feel nothing particular in this
    // room" palace-search lines) and bit 0x04 (left in the desert).
    pub(crate) for_condit_ds_16: u16,
    pub(crate) for_condit_ds_18: u8,

    // = seg001:0019 line_spoken_this_conversation — a "has a dialogue line been
    // spoken this conversation" flag: 0 when set_dialogue_speaker starts a
    // conversation (seg000:9417), 0xff once any dialogue line is presented
    // (fire_dialogue_line_event, seg000:a092). A fallback dialogue line tests
    // this == 0 in its CONDIT condition, so the fallback presents only when no
    // other line was presentable this conversation.
    pub(crate) line_spoken_this_conversation: u8,

    // = seg001:001a related_to_arguing_ds_1a — cleared per finish_room_screen_
    // setup pass and set to 1 while its room-entry scan is live; the arguing
    // logic around seg000:2241..24fe (unported) reads and steps it.
    pub(crate) related_to_arguing_ds_1a: u8,

    // = seg001:001c related_to_paying_smuggler_bills_ds_1c — the staged
    // smuggler's state byte (Smuggler +2), 001d current_smuggler_willingness_
    // to_haggle_ds_1d (+1), 001f related_to_paying_smuggler_bills_ds_1f — the
    // age in days of his open bill, 0020 current_smuggler_bill_value_ds_20 —
    // the bill (+0xe). All staged by stage_smuggler_for_condit (seg000:235f).
    pub(crate) related_to_paying_smuggler_bills_ds_1c: u8,
    pub(crate) current_smuggler_willingness_to_haggle_ds_1d: u8,
    pub(crate) related_to_paying_smuggler_bills_ds_1f: u8,
    pub(crate) current_smuggler_bill_value_ds_20: u16,
    // = seg001:001e current_smuggler_number_of_days_since_previous_encounter_
    // ds_1e — days since the den's last visit (1 on the first, seg000:2327..
    // 2339); 0022 smuggler_bills_count_ds_22 — how many smugglers hold an
    // open bill.
    pub(crate) current_smuggler_number_of_days_since_previous_encounter_ds_1e: u8,
    pub(crate) smuggler_bills_count_ds_22: u8,

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

    // = seg001:0024 for_dialogue_enemies_ds_24 — the location-index byte of
    // the COMM message being viewed (the sighting's high byte), staged for
    // the message dialogue conditions; cleared by comm_return_to_room.
    pub(crate) for_dialogue_enemies_ds_24: u8,

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
    // = seg001:11dd _stru_2068D_icon_list[0].index — the ATTACK.HSQ backdrop
    // sprite of the night attack, set by location_arrival_hostility_check
    // from the location type (0x2f sietch, 0x30 village, 0x33 fortress);
    // static 0x31.
    pub(crate) night_attack_backdrop_sprite: u16,

    // = seg001:004c related_to_contacting_troops_ds_4c — 0xff while the
    // contacted troop answers from outside the visibility range, so the
    // dialogue record's conditions pick its "out of contact" lines; cleared
    // by map_close_troop_contact_popup.
    pub(crate) contacting_troops_ds_4c: u8,

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

    // = seg001:00a8 for_condit_harkonnen_spice_production_ds_a8 — the
    // Harkonnen spice production (the SEE RESULTS Harkonnen SPICE PRODUCTION
    // column, ×10 in kg): sum of spice_density/8 over the locations
    // location_is_Atreides_05d36 REJECTS (everything the Harkonnens still
    // exploit) + rand_iterated(sum/16), recomputed each new day
    // (seg000:1cda). Static init 390 (3900 kg at game start).
    pub(crate) harkonnen_spice_production: u16,

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
    // none paid; static init 0x80, so the first demand announces as the
    // fresh-demand sighting 0x20b rather than the "last shipment wasn't
    // what I demanded" 0x30b), ds:bf the flags (bit 7 = the shipment plot
    // armed, bit 4 = a demand pending). actions_time_in_day_3 (seg000:20a4)
    // rolls the demands; the payment flow (Duncan/CHOAM dialogue) is
    // unported.
    pub(crate) spice_shipment_quantity: u16,
    pub(crate) spice_shipment_fulfilment: u8,
    pub(crate) spice_shipment_flags: u8,

    // = seg001:00b4..00ba for_condit_spice_shipment_arguing_related_ds_b4..ba
    // — the four spice amounts Duncan's shipment argument quotes, staged by
    // stage_spice_argue_amounts_with_duncan (seg000:22b1) from the stock and
    // the demand; ds:bf bits 1/2 record which bracket the stock fell in.
    pub(crate) spice_shipment_arguing_ds_b4: [u16; 4],

    // = seg001:009d for_condit_smuggler_dialogue_related_ds_9d — (price & 0x7f)
    // << 1 of the equipment the smuggler offers; 009e for_condit_smuggler_
    // arguing_count_ds_9e — rand_masked(3) haggling rounds; 009f accept_
    // refuse_argue_choice_ds_9f — the ACCEPT/REFUSE/ARGUE verb state (3 =
    // Paul has spice to argue with, 1 = accepted).
    pub(crate) for_condit_smuggler_dialogue_related_ds_9d: u8,
    pub(crate) for_condit_smuggler_arguing_count_ds_9e: u8,
    pub(crate) accept_refuse_argue_choice_ds_9f: u8,
    // = seg001:476d argue_menu_with_smuggler — which talk the ACCEPT/REFUSE/
    // ARGUE menu belongs to: 0 = Duncan's shipment offer (dialogue event
    // 0x04), 1 = the smuggler's bill (event 0x05). Event 0x09 reads it.
    pub(crate) argue_menu_with_smuggler: u8,
    // = seg001:1158 shipment_report_scene_mask — 0xffff once Paul has
    // answered Duncan's offer (seg000:2510); the room-entry scan masks ds:c0
    // with it to run the dining-hall shipment-report scene (seg000:35cf,
    // unported).
    pub(crate) shipment_report_scene_mask: u16,

    // = seg001:00c0 for_condit_spice_shipment_related_ds_c0 — Duncan's
    // shipment-mission report state: zeroed when his dialogue-line event
    // 0x0f sends him off (seg000:24b3), set from the ds:b4 table when he
    // returns (seg000:250d). The room-entry scan tests it (masked by
    // data_01158) to run the dining-hall shipment-report scene
    // (seg000:35cf); that scene is unported.
    pub(crate) for_condit_spice_shipment_ds_c0: u16,

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
    // id), max 10, appended by comm_add_person_sighting. Bit 7 of the low
    // byte marks an entry viewed (menu_callback_comms_message_selected); the
    // COMM message list (messages.rs) filters on it.
    pub(crate) comm_sightings: Vec<u16>,

    // = seg001:00c9 for_condit_comms_room_message_count_ds_c9 — the COMM
    // unread badge: incremented per new sighting (seg000:2713), decremented
    // when a new message is viewed (seg000:2941). The COMM verbs grey off it
    // and comm_return_to_room mirrors it into ds:eb.
    pub(crate) comm_unread_count_ds_c9: u8,

    // The five nearest-location triples condit_scan_nearest_locations
    // (seg000:5274) refreshes from the staged location whenever
    // prepare_location_data_for_condit runs.
    // = seg001:00ca nearest_location_distance_ds_ca — the nearest other
    // location of any kind.
    pub(crate) nearest_location: NearestLocation,

    // = seg001:00cf days_left_until_spice_shipment — the CONDIT day counter
    // actions_time_in_day_3 maintains while a demand date is ahead.
    pub(crate) days_left_until_spice_shipment: u8,

    // = seg001:00d0 nearest_village_distance_ds_d0 — the nearest village
    // (appearance < 0x28, status bit 7 clear).
    pub(crate) nearest_village: NearestLocation,

    // = seg001:00d5 contact_distance_related_ds_d5 — incremented once per
    // day, but only stored back from 2 up (seg000:1c62), so it stays at its
    // initial value until something else moves it to 1.
    pub(crate) contact_distance_related_ds_d5: u8,

    // = seg001:00d6 nearest_sietch_distance_ds_d6 — the nearest
    // phase-discoverable sietch (appearance < 0x28, bit 7 set); gates the
    // "There is a sietch very near" messages.
    pub(crate) nearest_sietch: NearestLocation,

    // = seg001:00db comm_list_filter_seen_ds_db — the COMM message-list
    // filter: 0 while viewing new messages (rows with sighting bit 7 clear),
    // 0xff while re-viewing already-seen ones.
    pub(crate) comm_list_filter_seen: u8,

    // = seg001:00dc nearest_Atreides_area_distance_ds_dc — the nearest
    // Atreides area (appearance >= 0x28, bit 7 clear).
    pub(crate) nearest_atreides_area: NearestLocation,

    // = seg001:00e1 data_000e1 — the fly-over side flag set by
    // travel_scan_nearby_location (seg000:4156): 0 when the passed location is
    // to the left of the heading, 1 when to the right. Feeds the companion's
    // fly-over dialogue line (the spoken-line tail is not ported yet).
    pub(crate) data_000e1: u8,

    // = seg001:00e2 nearest_Harkonnen_area_distance_ds_e2 — the nearest
    // Harkonnen area (appearance >= 0x28, bit 7 set); the ESPIONAGE
    // occupation and the Harkonnen-captain dialogue need its distance < 0x1e.
    pub(crate) nearest_harkonnen_area: NearestLocation,

    // = seg001:00e7 Paul_found_unconscious_in_desert_ds_e7 — cleared by the
    // desert walk-out (seg000:3fd2) and after an auto-dialogue line
    // (seg000:354c).
    pub(crate) paul_found_unconscious_ds_e7: u8,

    // = seg001:00e8 _byte_1F598_ui_hud_head_index.
    pub(crate) ui_hud_head_index: u8,

    // = seg001:00e9 for_condit_ds_e9 — the person id of the COMM message
    // being presented (0 between messages); the message dialogue records'
    // conditions read it.
    pub(crate) for_condit_ds_e9: u8,

    // = seg001:00ea data_000ea (signed).
    pub(crate) data_000ea: i8,

    // = seg001:00eb for_condit_presence_of_comms_room_message_which_needs_
    // viewing_there_ds_eb — comm_return_to_room and the vision dream mirror
    // the unread state here for CONDIT.
    pub(crate) comm_message_needs_viewing_ds_eb: u8,

    // = seg001:00ed/00ee for_condit_related_to_overpowering_Harkonnen_captain
    // — seeded by the captain classification (0xff when surrendered, else the
    // troop's motivation; the pair word), consumed by the OVERPOWER THE
    // PRISONER flow (seg000:9584).
    pub(crate) data_000ed: u8,
    pub(crate) data_000ee: u16,

    // = seg001:00f4 desert_exhaustion_counter — Paul's desert-exhaustion
    // latch: +1 per compass step outdoors, saturating at
    // DESERT_EXHAUSTION_MAX; the hourly decrement (run_events_for_current_
    // time_period) snaps any value below DESERT_EXHAUSTION_GAUNT_THRESHOLD
    // to 0, so it holds either "recently marched hard" (16..=20) or 0.
    pub(crate) desert_exhaustion_counter: u8,

    // = seg001:00f5 for_condit_desert_walk_related_ds_f5 — cleared with the
    // counter when the per-period countdown drops below
    // DESERT_EXHAUSTION_GAUNT_THRESHOLD (seg000:1b36); Jessica's desert
    // dialogue reads it.
    pub(crate) for_condit_jessica_commented_on_exhaustion_ds_f5: u8,

    // = seg001:00f2 for_condit_Chani_prisoner_location_area_and_name_ds_f2 —
    // (first_name << 8) | last_name of the sietch Chani is held prisoner in,
    // set by the phase-0x64 callback.
    pub(crate) for_condit_chani_prisoner_location_area_and_name_ds_f2: u16,

    // = seg001:00f6 for_condit_Paul_next_to_harvester_ds_f6 — set by
    // desert_harvester_check while the player stands at a location whose
    // spice-mining troop has a working harvester.
    pub(crate) for_condit_paul_next_to_harvester_ds_f6: u8,

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

    // = seg001:00fd for_condit_battle_related_ds_fd — the night attack's
    // battle gauge byte (location_seed_battle_gauge: the gauge | 1).
    pub(crate) for_condit_battle_related_ds_fd: u8,

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

    // = seg001:0e30/0e32 _word_20E30_globe_param_3 / _word_20E32_globe_param_4
    // — the map position the spice-density overlay is centred on, exchanged
    // with the live zoomed-globe position around its draw (loc_0b69a).
    pub(crate) globe_param_3: u16,
    pub(crate) globe_param_4: i16,

    // = seg001:0fd8 room_persons — the 16-entry room-person table walked by
    // scan_current_room_npcs. Mutable copy of ROOM_PERSON_TABLE_INIT;
    // init_room_persons rewrites entries 12..16 (addresses data_0109a / 10aa /
    // 10ba / 10ca) and its special-room branch (init_room_persons_special)
    // also touches entries 12, 14, 15 plus (selectively) 13.
    pub(crate) room_persons: [RoomPerson; 16],

    // = seg001:10d8 smugglers — the six smuggler inventories (region,
    // haggling, stock and prices); the new-day hook restocks them
    // (seg000:1cae).
    pub(crate) smugglers: [crate::smugglers::Smuggler; 6],
    // = seg001:113f current_smuggler_ptr — the smugglers[] record Duncan's
    // bill scan rotates through (seg000:2282..229f); static init = the
    // table's first record. Kept as the DOS seg001 pointer (see
    // smugglers::smuggler_ptr) so the save image carries it verbatim.
    pub(crate) current_smuggler_ptr: u16,

    // = seg001:1141 array_likelihood_of_worm_related_spice_mining_troop_
    // events_by_region — [0] is the base event probability (incremented by
    // the phase-0x4c and 0x5c callbacks; the smuggler dialogue also reads
    // it), [1..12] the per-region base indexed by Location.first_name.
    pub(crate) worm_event_likelihood_by_region: [u8; 13],

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

    // = seg001:1170 spice_stock_at_last_new_day / seg001:1172
    // spice_spent_today — the new-day production diff pair (seg000:1c6e):
    // production = stock + spent - stock at last new day. Smuggler purchases
    // and shipments add what they deduct from the stock to ds:1172.
    pub(crate) spice_stock_at_last_new_day: u16,
    pub(crate) spice_spent_today: u16,

    // = seg001:1174 data_01174 — the game_time the last time-period event run
    // saw; run_events_for_current_time_period diffs it to raise new_day_flag.
    pub(crate) last_event_game_time: u16,

    // = seg001:1176 location_visibility_distance — the sietch visibility
    // radius in map cells (static init 1): sietch map markers farther than
    // this from the player draw the +5 distant sprite variant, and the
    // walk/troop range checks compare against it. Raised by the dialogue-line
    // event callback at seg000:a1ad (not ported).
    pub(crate) location_visibility_distance: u16,

    // = seg001:1178 number_of_rallied_troops_for_Leto_being_killed — the
    // rallied-troop threshold armed by the phase-0x48 (met Chani) callback
    // (rallied + 2); 0xff (the static value) = not armed. Its reader (the
    // Leto-killed event pump) is not yet ported.
    pub(crate) number_of_rallied_troops_for_leto_killed: u8,

    // = seg001:118d ingame_day_of_last_spice_shipment_event — the day the
    // current shipment demand was rolled; the day-3 action measures the
    // reminder/consequence days from it.
    pub(crate) ingame_day_of_last_spice_shipment_event: u16,

    // = seg001:1190 vision_message_count + seg001:1191 vision_message_queue —
    // the queued vision messages, (message id, location ptr or 0), max 10;
    // queue_vision_message appends (deduplicated, oldest dropped on
    // overflow). Consumed by the idle-room presenter and the vision dream
    // (messages.rs) and purged when the sender delivers in person.
    pub(crate) vision_messages: Vec<(u16, u16)>,

    // = seg001:11bb data_011bb — the unpaid-shipment flag: nonzero routes
    // the day-3 action straight to the room-screen type-7 consequence
    // (seg000:20bc). Its writers (the payment flow) are unported.
    pub(crate) spice_shipment_unpaid: u8,

    // = seg001:11bc harkonnen_raid_suppress_once — nonzero suppresses the
    // next raid check; consumed (cleared) by actions_time_in_day_4
    // (seg000:1f83).
    pub(crate) harkonnen_raid_suppress_once: u8,

    // = seg001:11bc data_011bc — scene flag set (|= 1) by the night-attack
    // branch of draw_room_game_screen.
    pub(crate) data_011bc: u8,

    // = seg001:11bf book_bookmark_ptr (data_011bf) — the book's bookmark: the
    // cs offset of the current page word in the dialogue-played log (0xaa =
    // the first entry); persists while the book is closed.
    pub(crate) book_bookmark_ptr: u16,

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

    // = seg001:11ce data_011ce — the locations[] index whose CONDIT block is
    // currently staged (prepare_location_data_for_condit records it; static
    // init 0x100 = locations[0]). The event scheduler re-stages it after the
    // per-period events may have staged other locations (seg000:1b85).
    pub(crate) condit_staged_location: usize,
    // = seg001:47e6 staged_name_location_ptr — the location whose name the
    // 0x81/0x82 placeholders were staged for (stage_location_name_placeholders);
    // the dialogue-line-0x0d callback zooms the map inset on it.
    pub(crate) staged_name_location: usize,

    // = seg001:11d3 ARRAY_PTR_Location_prospector_destinations — the
    // prospector troop's (troops[2]) queue of destination location ptrs;
    // its arrival at the head shifts the queue (seg000:8347). The FIND
    // PROSPECTORS flow that fills it is not yet ported.
    pub(crate) prospector_destinations: [u16; 4],

    // = seg001:11eb string_subst_id_table — the COMMAND/PHRASE string ids the
    // inline name placeholders 0x80..0x8f expand to (entries 1..2 alias the
    // command-menu origin in DOS; the port keeps the menu origin separate).
    pub(crate) string_subst_id_table: [u16; 16],

    // = seg001:1225.. the scene records (palace_rooms et al) — the live,
    // runtime-mutable copy of room_scene::SCENE_RECORDS: the game-phase
    // callbacks unlock scripted palace exits (exit byte &= 0x7f) and patch
    // palace_rooms[1].background in here.
    pub(crate) scene_records: [crate::room_scene::SceneRecord; 83],

    // = seg001:149a travel_trail_cursor — the ring's write cursor (the NEXT
    // slot travel_trail_append fills; DOS keeps a byte pointer).
    pub(crate) travel_trail_cursor: usize,

    // = seg001:1668 record's runtime rect.
    pub(crate) map_location_info_panel: PanelRecord,

    // = seg001:18df
    pub(crate) map_troop_info_panel: PanelRecord,

    // = seg001:18e9
    pub(crate) map_troop_contact_text_panel: PanelRecord,

    // = seg001:18f3
    pub(crate) map_troop_contact_head_panel: PanelRecord,

    // = seg001:1936 map_equipment_troop_row_box — the MODIFY EQUIPMENT troop-row box inside
    // the contact popup; map_place_equipment_panels places it. Never in a popup slot.
    pub(crate) map_equipment_troop_row_box: PanelRecord,

    // = seg001:1940 map_equipment_location_strip — the MODIFY EQUIPMENT location strip
    // (the location's unused equipment), placed by map_place_equipment_panels and held
    // by the second popup slot while the spinners are up.
    pub(crate) map_equipment_location_strip: PanelRecord,

    // = seg001:194a
    pub(crate) data_0194a: PanelRecord,

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

    // = seg001:1ae4 _word_20F94_ui_elements — the in-game HUD element table.
    pub(crate) ui_elements: [UiElement; 24],

    // = seg001:1c76 ui_nav_panel_room.
    pub(crate) nav_panel_room: NavPanel,

    // = seg001:1cca ui_nav_panel_map_scroll.
    pub(crate) nav_panel_alt: NavPanel,

    // = seg001:1d1e ui_nav_panel_blank.
    pub(crate) nav_panel_blank: NavPanel,

    // = seg001:1d72 ui_nav_panel_flight.
    pub(crate) nav_panel_flight: NavPanel,

    // = seg001:1dc6 ui_globe_rotation_controls[0..6].
    pub(crate) nav_panel_globe: NavPanel,

    // = seg001:1e1a ui_globe_rotation_controls[6..12].
    pub(crate) nav_panel_book: NavPanel,

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

    // = seg001:1fae menu_continue_or_what.
    pub(crate) menu_continue_or_what: menu_defs::Menu,

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

    // = seg001:21da screen_element_stack — the z-ordered stack of active
    // menus, each with its cleanup func (the DOS slot's [si+2]).
    pub(crate) menu_stack: Vec<(MenuRef, Option<menu_defs::MenuCleanupFn>)>,

    // = seg001:21fd data_021fd — the SKIP TO DESTINATION command template's
    // flags byte (the seg001:21fc record's text-id high byte; 0x40 = greyed).
    // DOS patches the static template in place
    // (set_skip_to_destination_verb_flags); the port keeps the template const
    // and applies this byte when build_room_command_records copies it.
    pub(crate) cmd_skip_to_destination_flags: u8,

    // = seg001:2220 menu_ptr_02220 — which of the two the scene currently
    // shows (change_menu_to_continue_menu / ..._special_menu_after_
    // specializing_prospector_troop_in_spice). Static init = the prospector
    // panel.
    pub(crate) sequence_menu: MenuRef,

    // = seg001:2222 ui_hud_companion_blink — per-companion-slot blink countdown
    // bytes: npc_assign_companion_slot arms 0x10 on the filled slot (8 blinks),
    // npc_remove_companion_slot clears the vacated one, and the game-loop
    // blink task (ui_hud_companion_blink_task) drains them.
    pub(crate) ui_hud_companion_blink: [u8; 2],

    // = seg001:2244/2246 — the x/y words of the contact subtitle's layout
    // descriptor (seg001:2244, size 153x63), written per open by
    // map_draw_troop_contact_popup.
    pub(crate) map_contact_subtitle_pos: (i16, i16),

    // = seg001:224a — the descriptor's live height word (63; the move-order
    // caption narrows it to 0x19 around its draw, seg000:80fd).
    pub(crate) map_contact_subtitle_h: i16,

    // = seg001:227d data_0227d — suppresses the secondary 240..255 sky-palette
    // span. loc_039b9 / loc_0391d / loc_0398c write+fade an extra 16 colours
    // into entries 240..255 only when this is 0..
    pub(crate) data_0227d: u8,

    // = seg001:22e3 _byte_22E3_sky_skydn_selector — the SKY/SKYDN selector.
    // open_sky_or_skydn_palette opens resource 0x28 + this (0 → SKY.HSQ day,
    // 1 → SKYDN.HSQ dusk).
    pub(crate) sky_skydn_selector: u8,

    // = seg001:2406 book_topic_filter (data_02406) — the active book topic
    // filter (low byte = record mask 0x1c, high byte = topic bits); 0 = all
    // topics.
    pub(crate) book_topic_filter: u16,

    // = seg001:243e book_page_video_id (data_0243e) — the HNM resource id
    // (0x19..0x24) of the bookmarked page's video, 0 when the page has none.
    pub(crate) book_page_video_id: u16,

    // = seg001:2460 _word_21910_globe_tilt — the globe view tilt in map
    // latitude rows, carrying the map-row sign (negative = north, like
    // zoomed_globe_latitude); magnitude clamped to >= 0x20 by
    // set_globe_tilt_and_rotation and to <= 98 by globe_increment_tilt
    // (seg000:ba15).
    pub(crate) globe_tilt: i16,

    // = seg001:dd0f _word_2D1BF_globe_decoration_offset — the FRESK side decorations'
    // slide position on the globe screen: 0 = framing the globe, negative =
    // slid apart for the SEE RESULTS reveal (seg000:b8f3).
    pub(crate) globe_decoration_offset: i16,

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

    // = seg001:2772 data_02772 — the 16-bit line pattern draw_line loads per
    // edge (seg000:c541); 0xffff = solid, 0x5555 = the overlay's dotted box.
    pub line_pattern: u16,

    // = seg001:2784 _word_21C34_active_bank_id (+ the 0d844 cache table). The
    // active sprite/resource bank and its per-index loaded-sheet cache; see
    // `bank.rs`.
    pub(crate) banks: Banks,

    // = seg001:2786 troop_icon_draw_order_func — which draw-order pick
    // troop_icons_update_dirty_rect uses: false = troop_icons_pick_next_fifo
    // (0xc827, insertion order), true = troop_icons_pick_next_by_depth
    // (0xc835, the full map's back-to-front layering).
    pub(crate) troop_icon_draw_by_depth: bool,

    // = seg001:2788 data_02788 game_suspend_count — nesting suspend counter for
    // the live game (static init 1 = suspended during load/intro). While nonzero
    // the PIT callback skips advancing the game clock (seg000:ef84) and the idle-
    // event trigger is suppressed (seg000:1b12). suspend_game_clock /
    // resume_game_clock inc/dec it; reset_game_suspend zeroes it.
    pub(crate) game_suspend_count: u8,

    // = seg001:288e..28bd the six mixer-panel records (3 volume sliders + 3
    // subtitle indicators); see settings_ui.rs. Seeded from SETTINGS_RECORDS_INIT
    // and mutated as the panel is drawn / dragged.
    pub(crate) settings_records: [SettingsRecord; 6],

    // = seg001:28be settings_drag_target (data_028be) — the active mixer-panel
    // drag group: 0 = none, 1 = a volume slider, 2 = a subtitle indicator. Set on
    // an LMB grab (loc_0a594); also read by get_mouse_cursor_image (the busy hand).
    pub(crate) settings_drag_target: u8,

    // = seg001:28e7 data_028e7 — active voice/subtitle output mode (0/1/2).
    // ui_toggle_room_view restores it from voice_subtitle_mode_default on room
    // entry; ui_show_globe_map_view forces it to 1.
    pub(crate) voice_subtitle_mode: u8,

    // = seg001:28e8 data_028e8 — configured voice/subtitle mode (set by
    // check_amr_or_eng_language), copied into voice_subtitle_mode on room entry.
    pub(crate) voice_subtitle_mode_default: u8,

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

    // = seg001:37da voc_filename — the 14-byte voice filename buffer,
    // template "PF\PF001I .VOC". create_voc_file_name (seg000:a8bc) writes
    // bytes 1/4 (speaker letter), 5..7 (hex voc index), 8 (I/O acoustics
    // suffix, the a74a retry-flip target) and 9 (variant letter or blank);
    // bytes 0, 2..3 and 10..13 stay fixed.
    pub(crate) voc_filename: [u8; 14],

    // = seg000:a6d3 data_0a6d3 — the self-modifying immediate of
    // load_voc_and_lipsync_data's game-over branch (current_lip_sync_resource_id
    // == 0xffff): the voc index of the mocking line, 0x0fff / 0x1fff
    // (P<head>FFF / P<head>FFF..B), its bit 12 toggled after every load.
    pub(crate) game_over_voc_index: u16,
    // = seg001:dc30 chained_narration_clip — the narration voc index the
    // dialogue-line-0x0d callback queues; the voice task plays it through the
    // player's PO bank once the spoken line drains (seg000:a789).
    pub(crate) chained_narration_clip: u16,

    // Port-only stand-in for dune37s0.sav: the save image create_save_cl
    // writes at seg000:0029 (cl = 0xff, slot '0') right after init_game_ui.
    // RESTART GAME restores it from memory instead of reading the file.
    pub(crate) initial_game_image: Option<Vec<u8>>,

    // = seg001:37fa music_cd_playlist — the working CD-playlist order: 9 song
    // numbers + the 0xff terminator. STANDARD ORDER recopies music_cd_standard_
    // order over it; SHUFFLE permutes it in place (music_cd_playlist_shuffle).
    pub(crate) music_cd_playlist: [u8; 10],

    // = seg001:380e music_cd_playlist_cursor — the index of the NEXT playlist
    // entry to play (DOS keeps a pointer into the table; init = the base).
    pub(crate) music_cd_playlist_cursor: usize,

    // = seg001:3810 music_playlist_flags — the jukebox mode. 0 = game-relative
    // (the song follows the on-screen situation, the default set at game init);
    // bit 0 = CD-style playlist, bit 1 = shuffle.
    pub(crate) music_playlist_flags: u8,

    // Port-only (no DOS equivalent, which has no music command line): the
    // `--music` selection, held from set_music_mode until start() lands it
    // through apply_pending_music_mode. It has to wait: start() zeroes
    // music_playlist_flags at seg000:0019, between the intro and the in-game
    // setup, so a mode set before start() would be wiped. `None` = no
    // selection made, and start() leaves the music state alone.
    pub(crate) pending_music_mode: Option<crate::MusicMode>,

    // = seg001:3cbe troop_icon_count / seg001:3cc0 troop_icons — the troop
    // icon renderer's live icon list (troop_icons.rs). The night attack
    // scene's separate copy lives in attack/mod.rs.
    pub(crate) troop_icons: Vec<crate::troop_icons::TroopIcon>,

    // = seg001:46d2/46d4 data_046d2/046d4 — the head-rect-relative anchor point
    // the troop-contact popup re-anchors the talking head on (staged from
    // TALKING_HEAD_POPUP_ANCHOR by map_draw_troop_contact_popup), and
    // = seg001:47d4 data_047d4 — the popup's head draw box: both the
    // destination origin and the clip rect draw_head_image_group_in_box uses.
    pub(crate) head_popup_anchor: (i16, i16),
    pub(crate) head_popup_box: Rect,

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

    // = seg001:46da data_046da — nonzero while the WAIT-verb / travel event
    // pump (run_events_for_n_time_periods) owns the screen; the scheduler's
    // refresh tail skips the room redraw while it is set (seg000:1bbf).
    pub(crate) events_pump_active: u8,

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

    // = seg001:46ec data_046ec — the map-view dirty counter: bumped when a
    // mining troop eats through more than the spice-density overlay's
    // current shade while that overlay is up (data_046eb bit 6), and by the
    // daily vegetation promotion while the full map is up (seg000:65fe);
    // the scheduler's refresh tail consumes it via
    // map_view_refresh_after_events (seg000:1b97).
    pub(crate) spice_density_overlay_dirty: u8,

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

    // = seg001:46f4 map_troop_equipment_row_up — 1 while the contact popup shows the troop's
    // equipment row (a line whose event armed the hand-over drew it,
    // seg000:7c47); map_open_troop_contact_dialogue,
    // map_close_troop_contact_popup and troop_equipment_changed clear it.
    pub(crate) map_troop_equipment_row_up: u8,

    // = seg001:46f5 map_modify_equipment_mode — 1 while the MODIFY EQUIPMENT spinner
    // sub-mode is up (menu_callback_choice_map_troop_dialogue_modify_
    // equipment pushes the DONE strip; its cleanup map_modify_equipment_done
    // clears it). Routes the popup clicks to the spinners and makes any other
    // map click DONE.
    pub(crate) map_modify_equipment_mode: u8,

    // = seg001:46f3 map_view_reentry_count — counts map-view re-entries within
    // one visit (loc_05a03 increments it when a troop dialogue path re-opens
    // the view); reset_room_scene_state zeroes it. While 0,
    // ui_show_globe_map_view shows the rallied-troops title popup.
    pub(crate) map_view_reentry_count: u8,

    // = seg001:46f6 troop_icon_anim_phase — the anim task's frame counter.
    pub(crate) troop_icon_anim_phase: u8,

    // = seg001:46f8 data_046f8 — the location whose info popup is open
    // (a location ptr in DOS, the table index here; None = closed), the
    // re-click gate. = seg001:46f7 data_046f7 — its class+1 (0 = closed).
    pub(crate) map_location_popup_loc: Option<usize>,
    pub(crate) map_location_popup_class: u8,

    // = seg001:46fa data_046fa — the troop whose info panel (data_018df) is
    // open (a troop ptr in DOS, the table index here; None = closed).
    pub(crate) map_info_popup_troop: Option<usize>,

    // = seg001:46fc data_046fc — the map screen's hover state, maintained by
    // map_mouse_hover_tracker (seg000:4586) — and, with the spice-density
    // overlay open, by map_main_mouse_idle (seg000:5c4d) over the overlay's
    // own markers — and consumed by the LMB
    // destination click: 0 = pointer outside the map window; a location ptr
    // (see locations::location_ptr) = hovering that location's marker;
    // 0xfff0+n = aligned on desert compass ray n (0 N .. 7 NW) from the
    // player marker; 0xffff = inside the window, nothing hovered. Cleared on
    // map open.
    pub(crate) data_046fc: u16,

    // = seg001:46ff
    pub(crate) available_equipment: Equipment,

    // = seg001:4705 troop_equipment_flags_location_style_ds_4705 — a troop's
    // equipment as 7 per-type 0/1 bytes (harvesters .. bulbs), the row the
    // troop info panel draws and the MODIFY EQUIPMENT spinners edit; packed
    // back by troop_update_troop_equipment_from_location_style_equipment.
    pub(crate) troop_equipment_flags: [u8; 7],

    // = seg001:4c60 the GLOBDATA-slot scratch draw_equipment_row fills: per
    // equipment column the [x0, x1) it drew, for the spinner click test
    // (equipment_column_at); (0, 0) for a column it did not draw.
    pub(crate) map_equipment_column_x_ranges: [(i16, i16); 7],

    // = seg001:4c7c map_equipment_troop_column_x_ranges — the troop row's copy of the scratch
    // (seg000:7d1e), taken before the location row overwrites it.
    pub(crate) map_equipment_troop_column_x_ranges: [(i16, i16); 7],

    // = seg001:4710/4712 data_04710/data_04712 — the shared popup-panel
    // origin the spice-density overlay draws at, and its rect (the rect
    // doubles as the popup identity). The overlay's home entry (seg000:5406)
    // reloads it from the data_011c1/011c3 home words; the contact popup
    // parks it opposite itself (seg000:7a15) for the in-place entry.
    pub(crate) map_overlay_panel_pos: (i16, i16),
    pub(crate) map_overlay_panel_rect: Rect,

    // = seg001:4718 data_04718 / seg001:4738 data_04738 — the destination
    // pick's working copy of the prospector queue and its entry count: the
    // MOVE TROOP verb seeds them from prospector_destinations (three words
    // copied, four scanned — the DOS asymmetry), map clicks append, and the
    // Done verb copies them back.
    pub(crate) prospector_pick_queue: [u16; 4],
    pub(crate) prospector_pick_count: u8,

    // = seg001:4720 data_04720 — the overlay-open flourish source: callers
    // stage a rect origin (DOS stores a record address — seg001:18f3 the
    // contact head box, seg001:1e6e ui_hud_head_rect) and
    // map_draw_spice_density_overlay consumes it (seg000:553c) as the start
    // point of the effect-6 XOR-outline scale-in to the panel rect.
    pub(crate) map_overlay_anim_src: Option<(i16, i16)>,

    // = seg001:4722 map_overlay_mode — which layer the overlay renders: 0 =
    // the spice-density colours (ramp legend), 0xff = the troop-occupation
    // class-mix colours (seg000:583f). Toggled by a click on the overlay
    // footer (seg000:5970); reset to 0 by the SEE SPICE DENSITY verb and the
    // contact-scene entry.
    pub(crate) map_overlay_mode: u8,

    // = seg001:4724 map_overlay_hover_tick — the density-ramp tick the
    // overlay hover readout has XOR-drawn on the legend: the hovered region
    // shade - 0x50 (0..15), or 0xff for none. Reset to 0xff whenever the
    // footer strip repaints (seg000:5630).
    pub(crate) map_overlay_hover_tick: u8,

    // = seg001:4725 map_overlay_footer_label_color — change detector for the
    // overlay footer label colour: the fg byte last drawn (0xfe normal, 0xf5
    // inverted while the cursor is over the footer). Seeded by
    // map_overlay_draw_legend (seg000:5647), swapped by the seg000:57b5
    // redraw.
    pub(crate) map_overlay_footer_label_color: u8,

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

    // = seg001:4728 travel_minimap_state — the flight minimap state: 0 normal,
    // 1 = recenter + redraw pending (set by the pump when the position leaves
    // the minimap bounds, seg000:4f8e, and by CHANGE DESTINATION at
    // seg000:4980), bit 0x80 = minimap hidden (toggled by travel_toggle_minimap,
    // seg000:4aad).
    // map_screen_cleanup re-enters the minimap view when > 0. Reset by
    // travel_reset_trail when the map screen opens.
    pub(crate) travel_minimap_state: i8,

    // = seg001:4729 travel_step_tick_stamp — PIT stamp of the travel pump's
    // last step (travel_pump steps every 0x300 ticks); zeroed by
    // map_confirm_travel_and_close.
    pub(crate) travel_step_tick_stamp: u16,

    // = seg001:472b travel_step_counter — counts travel_advance_step calls;
    // every 16th runs one time period of events. Zeroed by
    // map_confirm_travel_and_close.
    pub(crate) travel_step_counter: u16,

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

    // = seg001:4733 spice_mining_troops_with_harvester_in_location — low byte:
    // the hired spice-mining troops with a harvester at the location the
    // player stands at, high byte: the location's own harvester count. Staged
    // by desert_entrance_pass, read by desert_harvester_check.
    pub(crate) spice_mining_troops_with_harvester_in_location: u16,

    // = seg001:485e..487c the sprite animation record (the desert harvester).
    pub(crate) sprite_anim: crate::room_scene::SpriteAnim,

    // = seg001:4732 data_04732 — room-entry flags; bit 0 requests the extra
    // location overlay SAL (loc_0488a) on the normal draw_room_game_screen path.
    pub(crate) data_04732: u8,
    // = seg001:144c _byte_208FC_loaded_SAL_index — which of the four .SAL
    // files open_sal_resource has loaded (0xff: none yet).
    pub(crate) loaded_sal_index: u8,
    // = seg001:bc6e _work_2B11E_SAL_data — the loaded .SAL, parsed.
    pub(crate) sal_sheet: Option<crate::RoomSheet>,

    // = seg001:4735 desert_step_counter — low 7 bits count the desert-walk
    // steps since the last room-entry; msb is set by ui_click_move_room when the counter is updated,
    // and reset by draw_room_game_screen (loc_03a3e) after it consumes the
    // change.
    pub(crate) desert_step_counter: u8,

    // = seg001:473b data_0473b — the scheduler tail's room-redraw request
    // (seg000:1ba9): bit 7 = re-present the whole room screen (dismissing
    // stacked overlays), else nonzero = draw_room_game_screen; cleared by
    // the tail (seg000:1bb2).
    pub(crate) room_redraw_request: u8,

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

    // = seg001:4752 troop_icon_focused_ptr — the two focused-icon slots; the
    // anim task steps slot 0 every firing where the rest only step every 4th.
    pub(crate) troop_icon_focused: [Option<usize>; 2],

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

    // = seg001:476c selected_fremen2_index — which fremen2_troop_ptrs slot
    // the active Fremen-2 conversation (or room draw) refers to.
    pub(crate) selected_fremen2: u8,

    // = seg001:00fa vegetation_started_on_Dune — the ecology-victory flag the
    // motivation modifier reads: troop_irrigated_this_period as the last
    // per-period troop walk left it.
    pub(crate) vegetation_started_on_dune: u8,

    // = seg001:4737 troop_irrigated_this_period.
    pub(crate) troop_irrigated_this_period: u8,
    // = seg001:00ec bulb_growing_progress — the bulb-growing counter: a
    // bulb-growing troop at a location without bulbs bumps it each period;
    // when it wraps to 0 the location gets 16 bulbs (seg000:767d).
    pub(crate) bulb_growing_progress: u8,
    // = seg001:473c gurney_location_ptr — the location record Gurney
    // (room_persons[4]) stands in, or 0 when he is not placed. Refreshed at
    // the start of every troop walk; army training there is much faster.
    pub(crate) gurney_location_ptr: u16,

    // = the for_condit troop staging block (seg001:002c..004b), filled by
    // troop_prepare_troop_data_for_condit.
    pub(crate) troop_condit: crate::troops::TroopCondit,

    // = the for_condit location staging block (seg001:004d..005b), filled by
    // prepare_location_data_for_condit.
    pub(crate) location_condit: crate::troops::LocationCondit,

    // = seg001:476e npc_menu_idle_timer_base / seg001:4772
    // npc_menu_idle_timer_limit — the NPC-actions-menu inactivity timer
    // arm_npc_menu_idle_timer (seg000:c85b) arms: base = PIT counter at the last
    // spoken line, limit = 0x1770 (6000 ticks, 30 s). The room mouse hook
    // room_idle_npc_menu_zoom (seg000:1ae7) watches them while menu_NPC_actions
    // is the active menu and fires loc_0c868 on expiry.
    pub(crate) npc_menu_idle_timer_base: u16,
    pub(crate) npc_menu_idle_timer_limit: u16,
    // = seg001:4770 npc_menu_idle_last_tick — the PIT counter value
    // room_idle_npc_menu_zoom last evaluated, so the timer is checked once per
    // tick rather than once per game-loop pass.
    pub(crate) npc_menu_idle_last_tick: u16,

    // = seg001:4774 data_04774 — nonzero while a dialogue is active; routes
    // ui_draw_room_command_panel to the dialogue renderer and suppresses the
    // auto lip-sync start.
    pub(crate) is_dialogue_active: bool,

    // = seg001:4776 data_04776 — the (location_and_room low byte,
    // data_046e0) pair start_scripted_dialogue snapshots and the 0xff end
    // restores.
    pub(crate) sequence_saved_scene: (u8, u8),

    // = seg001:4778 data_04778 — the script position action 00 records so a
    // later step can branch back to it. Its readers are the unported
    // cutscene actions.
    pub(crate) sequence_return_cursor: Option<usize>,

    // = seg001:477a data_0477a — the active continue-sequence script and its
    // read cursor (DOS keeps a cs pointer; the port keeps the slice plus an
    // index). None = no scene running.
    pub(crate) sequence_script: Option<&'static [u8]>,
    pub(crate) sequence_cursor: usize,

    // = seg001:477c dialogue_current_record_ptr — byte offset of the sentence
    // entry the present walk started at (seg000:9f9e); load_PHRASExx_HSQ
    // (seg000:d00f) compares it against dialogue_phrase12_first_record_ptr (a
    // relocated pointer at offset 0x60 inside the DIALOGUE buffer, seg001:aa76)
    // to pick the PHRASE11 vs PHRASE12 phrase bank.
    pub(crate) dialogue_current_record_ptr: u16,

    // = seg001:4780 current_subtitle_id — the COMMAND/PHRASE id of the dialogue
    // sentence currently selected for presentation (set by show_voice_subtitle
    // from the phrase id dialogue_interpret_record pulls out of the matched
    // sentence). 0 = none.
    pub(crate) current_subtitle_id: u16,

    // = seg001:478c data_0478c — word count of the last laid-out subtitle
    // (zeroed by layout_subtitle_lines, summed per committed line at
    // seg000:8ea3). loc_09908 seeds the talking head's lively idle budget with
    // 4 × this; the intro stores 0x1e (loc_009c7) and 1 (loc_00965) directly.
    pub(crate) subtitle_word_count: u8,

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

    // = seg001:47a8 dialogue_end_request — incremented by the spoken-line event
    // 0x06 (callback_event_dialogue_line_06_end_dialogue, seg000:a1e8); consumed
    // (xchg with 0) at seg000:a09d to force the walk's continuation pointer to
    // 0xffff so the next TALK TO ME stops resuming the record.
    pub(crate) dialogue_end_request: u8,

    // = seg001:47a9 comm_displayed_message_person — person id of the COMM
    // message face currently displayed over the room view (0 = none). A
    // game-area click while nonzero runs the Viewed action, and
    // build_room_command_records shows the comm console sprite 0x28.
    pub(crate) comm_displayed_message_person: u8,

    // = seg001:47aa data_047aa — index into the persons array (see persons_met)
    // of the speaker whose lip-sync to auto-start; 0 = none. Cleared on entry.
    pub(crate) data_047aa: u16,

    // = seg001:47b0 / seg001:47b4
    // the resident PHRASE bank (load_PHRASExx_HSQ) and its resource id
    // (0 = none loaded).
    pub(crate) phrase_bin: Vec<u8>,
    pub(crate) current_phrase_bin_id: u8,

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

    // = seg001:47ba dialogue_resume_entry_ptr — the TALK TO ME resume pointer:
    // byte offset of the sentence entry the next talk action continues from
    // within the current record (0 = start at the data_047be topic cursor;
    // 0xffff = record exhausted / dialogue ended).
    pub(crate) dialogue_resume_entry_ptr: u16,

    // = seg001:47be data_047be — the dialogue sentence cursor: person_index << 3,
    // primed by set_dialogue_speaker (seg000:93e7). menu_callback_choice_talk_to_me
    // walks the speaker's record slots starting from this base (person*8 + topic).
    pub(crate) dialogue_topic_index: u16,

    // = seg001:47c2 data_047c2 — the dialogue verb-panel sentence-eligibility mask
    // set_dialogue_speaker primes to 0x80 (seg000:9412). dialogue_interpret_record
    // masks each sentence's flag byte against it (seg000:9fbe) to skip verb-gated
    // entries; other dialogue verbs flip it to 0x20.
    pub(crate) data_047c2: u8,

    // = seg001:47c4 _word_23C74_current_lip_sync_resource_id — sprite-sheet
    // resource id of the current speaker's lip-sync data; 0xffff = none.
    pub(crate) current_lip_sync_resource_id: u16,

    // = seg001:47dc data_047dc — the shared "fixed-block" voc-bank flag: nonzero
    // while a line is presented from a fixed dialogue block whose voc numbering
    // does not belong to the speaking head's own P<X> directory. The fly-over
    // narration (travel_play_flyover_line, seg000:96db) and the fixed-block COME
    // WITH ME (seg000:95b7, unported) arm it around their present, then clear it.
    // load_voc_and_lipsync_data (seg000:a6f1) reads it: when set, the voc index
    // is rebased onto per_person_voc_base_table[0x10] + 0x3e7 instead of the
    // speaker's own base, so the fly-over line finds its .voc.
    pub(crate) data_047dc: u8,

    // = seg001:47dd last_line_voc_bank_flag — data_047dc as it was when the
    // current subtitle line's voice last played (seg000:9f00). The WHAT verb
    // replays through play_dialogue_voc_with_bank_flag with this value.
    pub(crate) last_line_voc_bank_flag: u8,

    // = seg001:227e post_voice_hook — a one-shot routine the line presenter
    // runs right after the voice starts (seg000:a0d6: xchg with nullsub_00f66,
    // call). Only the Stilgar branch of dialogue event 0x08 arms it
    // (seg000:a13a). None = nullsub_00f66.
    pub(crate) post_voice_hook: Option<fn(&mut GameState)>,

    // = seg001:47de dialogue_line_word0 — first word of the sentence entry being
    // presented (seg000:9ff9); the voc-replay / subtitle continuation code
    // (seg000:89d3/8a3b/8ac6, unported) tests its 0x10 flag.
    pub(crate) dialogue_line_word0: u16,

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

    // = seg001:487e travel_vehicle_mode — the vehicle for the pending map
    // travel: 1 = worm (CALL A WORM, seg000:42aa), 2 = ornithopter (TAKE AN
    // ORNITHOPTER, seg000:42ff / seg000:50db). loc_04ec6 refines it into
    // hnm_active_video_id (the day/night flight HNM variants 2..5).
    pub(crate) travel_vehicle_mode: u16,
    // = the VER.BIN blob worm_ride_setup loads over cs:015f, with
    // worm_view_rect (seg001:aa66) and worm_script_cursor (seg001:aa6e).
    pub(crate) worm_anim: Option<crate::worm_ride::WormAnim>,

    // = seg001:494c _dword_23DFC (the TABLAT entry-0 fp field) — the globe
    // rotation phase in 1/398ths of a revolution (0..397): the integer word
    // of the DOS 16.16 seed. set_globe_tilt_and_rotation derives it from the
    // longitude (hi word of 398 * lng); globe_rotation_increment steps and
    // wraps it (the rotation frame task adds 1 per finished draw pass).
    pub(crate) globe_rotation: u16,

    // = seg001:a5c0 visible_location_markers — one entry per location visible
    // on the map view, rebuilt by map_build_and_draw_location_markers and
    // scanned by the marker hover hit-test (find_nearest_location_marker).
    // DOS packs 6-byte entries [location ptr, screen x, screen y:u8,
    // data_046eb copy:u8] with a 0-word terminator; the port stores them
    // unpacked.
    pub(crate) visible_location_markers: Vec<MapLocationMarker>,

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

    // = seg001:d10e _word_2D10E_mouse_last_click_time — the PIT counter snapshot
    // taken each time an element handler fires (= seg000:d935). The held-button
    // auto-repeat gate (= seg000:d8da) re-fires only once >= 0x32 ticks elapse.
    pub(crate) mouse_last_click_time: u16,

    // = seg001:d7f4 per_person_voc_base_table — see build_voc_base_table.
    pub(crate) voc_bases: [u16; 17],

    // = seg001:d824 _unk_2CCD4_rand_seed.
    pub(crate) rand_seed: u16,

    // = seg001:d826 _unk_2CCD6_rand_seed.
    pub(crate) rand_bits_seed: u16,

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

    // = seg001:dbd2 music_song_end_tick_stamp — the PIT-counter stamp of the
    // first idle-driver sighting after a CD-playlist song ends; the CD service
    // advances the playlist 0xc8 ticks later. 0 = unset; cleared when a song
    // starts (seg000:adba).
    pub(crate) music_song_end_tick_stamp: u16,

    // = seg001:d828 _unk_2CCD8_bios_timer_count_3 — rand_iterated's LCG seed, separate
    // from rand's (0d826) and rand_masked's (0d824). DOS seeds it from the
    // BIOS tick count during startup; the shuffle also perturbs it with the
    // live PIT counter between draws.
    pub(crate) rand_iterated_seed: u16,

    // = seg001:dbd8 _word_2D088_screen_buffer_seg — the "front buffer" copy/
    // present target. Normally Screen; gfx_call_bp_with_front_buffer_as_screen
    // redirects it to Fb1 so a stage init renders fully offscreen.
    pub(crate) screen_buffer: FbId,

    // = seg001:dbda _word_2D08A_framebuffer_active_seg — the buffer every blit
    // primitive currently targets. Stage inits run with this == Fb1.
    pub(crate) active_fb: FbId,

    // = seg001:dbe0 map_popup_ptr — which popup panel record is open on the
    // full map view.
    pub(crate) map_popup: MapPanelRef,

    // == seg001:dbe2 map_popup2_ptr
    pub(crate) map_popup2: MapPanelRef,

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

    // = seg001:376a _byte_22C1A_audio_current_sfx_id / seg001:3811 _dword_22CC1_pcm_voc_
    // resource_offset — which sound effect audio_start_voc (seg000:ab15) last
    // opened, and its loaded bytes. Re-requesting the same effect replays the
    // resident resource (seg000:ab23 cmp al,[audio_current_sfx_id]; jz
    // loc_0ab35) instead of stopping the voice and opening it again. DOS keys
    // this on the resource index; the port keys it on the resource name, which
    // is what its callers pass.
    // = seg001:dc2b _byte_2D0DB_is_voc_pcm_playing — "a voice was started and
    // has not yet been declared finished". Deliberately NOT a mirror of the
    // mixer: lip_sync_stop clears it (seg000:a7b9) while the clip is still
    // being transferred, and pcm_stop_voc (seg000:ac14) leaves it set. Set at
    // the talking-head voice start (seg000:a768); DOS also sets it on two
    // paths the port does not have (seg000:a567's blocking narration and the
    // seg000:b1f5 video transition). audio_start_voc and
    // start_narration_voice_clip gate on THIS, not on real playback, which is
    // why a sound effect can cut a voice that is still audible.
    pub(crate) voc_pcm_playing: bool,

    // = seg001:dc36 mouse_pos_x / seg001:dc38 mouse_pos_y — the cursor position
    // poll_pointer_input latches each poll. The port copies it from the shared
    // InputState (already mapped into 320x200 game coordinates by the host)
    // instead of reading INT 33,3 and applying the mickey scalers.
    pub(crate) mouse_pos_x: u16,
    pub(crate) mouse_pos_y: u16,

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

    // = seg001:dc34 mouse_button_state — the button state of this pass:
    // the live buttons (poll_pointer_input) with the keyboard's confirm keys
    // folded in (poll_pointer_input_keyboard); mouse_stuff reads it.
    pub(crate) mouse_button_state: u8,

    // = seg001:dc4a..dc4e the keyboard pointer glide (kb_pointer.rs):
    // kb_glide_tick (the tick byte of the last step), kb_glide_steps (steps
    // left; 0 = idle, game_loop polls the mouse; cleared at game_loop entry,
    // seg000:d81b), kb_glide_target_x/y.
    pub(crate) kb_glide_tick: u8,
    pub(crate) kb_glide_steps: u8,
    pub(crate) kb_glide_target_x: i16,
    pub(crate) kb_glide_target_y: i16,

    // = seg001:dc50..dc57 the Ctrl+arrow accelerated move: kb_move_tick (the
    // tick byte of the last move), kb_move_dist_x/y (the run so far, the
    // acceleration measure), kb_move_frac (the 3-bit sub-pixel fraction per
    // axis) and kb_button_prev (the keyboard button bit of the previous
    // pass).
    pub(crate) kb_move_tick: u8,
    pub(crate) kb_move_dist_x: i16,
    pub(crate) kb_move_dist_y: i16,
    pub(crate) kb_move_frac: [u8; 2],
    pub(crate) kb_button_prev: u8,

    // = seg001:dc58 mouse_nav_rect_ptr — the active navigation
    // mouse hot-zone: get_mouse_cursor_image switches the cursor to the hand
    // inside it and to the four travel arrows within the scroll bands outside
    // its edges. DOS stores a pointer to a Rect (the map screen installs
    // map_view_rect_template, seg000:4331); the port copies the rect. None =
    // cleared (clear_mouse_nav_rect).
    pub(crate) mouse_nav_rect: Option<Rect>,

    // = seg001:dc5a game_clock_tick_base — PIT-counter reference snapshot
    // (`game_ticks() as u16`), taken when the room screen is presented and on
    // every mouse-button edge (seg000:d893); elapsed ticks are derived by
    // subtracting this base from the current `game_ticks() as u16`.
    pub(crate) game_clock_tick_base: u16,

    // = seg001:dc5c data_0dc5c — the HUD element a press has armed for held
    // auto-repeat / release dispatch (set when the press lands on a record with
    // the 0x4000 flag; di in DOS, an index here). game_loop's drag path re-fires
    // it on the 0x32-PIT-tick interval and the release path fires + clears it.
    pub(crate) drag_armed_element: Option<usize>,

    // = seg001:dc62 data_0dc62 / seg001:dc64 data_0dc64 — the pointer position
    // latched on the previous game_loop pass. Each pass xchg's the live position
    // in and subtracts to derive the per-frame motion delta (di = X, cx = Y) the
    // drag handler ([si+0ah]) consumes.
    pub(crate) mouse_prev_drag_x: u16,
    pub(crate) mouse_prev_drag_y: u16,

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

    // = seg001:dc66 frame_task_dispatch_sp — Some while process_frame_tasks
    // is inside a callback: the array index the walk visits next. DOS
    // publishes the loop's saved {si, cx} through this cell so
    // remove_frame_task can shift the walk back when the compaction moves
    // the entries at or before the cursor (seg000:da87); see
    // process_frame_tasks for why the index alone carries that.
    pub(crate) frame_task_walk_next: Option<usize>,

    // = seg001:dce4 data_0dce4 — the active menu's skip byte as sampled by
    // redraw_active_command_menu, with bit 0x80 OR'd in when more records
    // follow the visible window. read_command_menu_record_for_slot decides
    // what the " Others..." row does from it: sign set = advance a page,
    // else positive = rewind to the first page. The port keeps the skip in
    // record units (DOS stores a byte offset, 4 bytes per record).
    pub(crate) command_menu_more_state: u8,

    // = seg001:dce5 data_0dce5 — the slot the " Others..." (0xa0) row was
    // painted into by the last redraw, 0xff = none.
    pub(crate) command_menu_more_slot: u8,

    // = seg001:dce6 _byte_2D196_in_transition? — set while a screen transition /
    // deferred-task drain is in progress; draw_room_game_screen clears it before
    // the render. See dismiss_stacked_menus.
    pub(crate) in_transition: u8,

    // = seg001:dce7 index_of_last_hovered_action_item — the verb slot
    // currently shown with the 0x8000 highlight, 0xff if none.
    // redraw_active_command_menu resets to 0xff at entry, then
    // highlight_hovered_text_action_item diffs against it each frame to know
    // which slot to un-highlight before painting the new hover.
    pub(crate) index_of_last_hovered_action_item: u8,

    // = seg001:dce8 data_0dce8 — how many non-blank slots the last
    // redraw_active_command_menu painted (records plus the " Others..." row);
    // the hover highlight walks only these.
    pub(crate) command_menu_slot_count: u8,

    // = the segvga A000:FA00 cursor-background save area and the geometry
    // vga_draw_cursor records (cs:[cursor_fb_pos/_width/_height]). The port keeps
    // `screen` exactly 320x200, so the save lives here rather than past the
    // visible framebuffer; vga_restore_cursor writes it back.
    pub(crate) cursor_save: Vec<u8>,
    pub(crate) cursor_save_pos: usize,
    pub(crate) cursor_save_w: u16,
    pub(crate) cursor_save_h: u16,

    // = seg001:dcf1 companion_blink_step_latch — the blink task's pacing
    // latch: the last-seen (game_ticks >> 6) & 0xff step number, so the task
    // fires once per 64 PIT ticks.
    pub(crate) companion_blink_step_latch: u8,

    // = seg001:dd02 globe_draw_area_control_colors — nonzero (the SEE
    // RESULTS mode) selects the vga_globe_init patch that recolours every
    // globe pixel into the area-control palette blocks (0x10 plain, 0x20
    // Atreides-held, 0x30 Harkonnen-held — globe_pixel_area_control_colors,
    // segvga:1ec9); the globe keeps spinning in those colours.
    pub(crate) globe_draw_area_control_colors: u8,

    // Music-situation classifier inputs (= loc_0aa96).
    // = seg001:dd03 globe_screen_active.
    pub(crate) globe_screen_active: u8,

    // = seg001:dd11 results_gauge_targets / seg001:dd17 results_gauge_current
    // — the SEE RESULTS gauges: results_update_gauge_targets fills the
    // targets, results_draw_text_and_icones zeroes the currents, and
    // results_gauge_task steps each current one toward its target per fire.
    pub(crate) results_gauge_targets: [u8; 6],
    pub(crate) results_gauge_current: [u8; 6],

    // = seg001:2ccc6 _unk_2CCC6_comm_glow_index — the COMM console glow /
    // flicker animation frame counter.
    pub(crate) comm_glow_index: u16,

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

    // = seg000:65b4 ecology_lfsr_state — the persistent 16-bit LFSR state
    // (taps 0x402, static init 1) of the daily vegetation-promotion walk
    // (seg000:65b6).
    pub(crate) ecology_lfsr_state: u16,

    // = seg000:e40c travel_trail_ring — the cs-resident travel-trail ring:
    // (longitude, latitude) pairs up to loc_0e85c ((0xe85c - 0xe40c) / 4
    // entries); empty entries hold the 0x800 sentinel in both words
    // (travel_reset_trail). travel_trail_append writes at the cursor.
    pub(crate) travel_trail_ring: [(u16, u16); TRAVEL_TRAIL_LEN],

    // = seg001:4775 _byte_23C25_blink — the toggle frame_task_callback_blink flips while
    // a scripted scene runs.
    pub(crate) sequence_blink: bool,

    // = segvga:1f4c vga_draw_map_zoomed's working state (the RESOURCE_GLOBDATA
    // band scratch) — the SEE DUNE MAP full-planet renderer,
    // map_draw_zoomed_globe's data_046eb bit-0x80 path.
    pub(crate) map_renderer: MapRenderer,

    // = segvga:2768 transition_col / segvga:276a transition_frame — the
    // wipe-transition engine's running state, advanced one step per call by
    // transition_tick (gfx::transition_tick). Static-init to col=8, frame=1.
    // room_frame_task (tick_room) steps this to time the cave water-drip sound.
    pub(crate) transition_col: u16,
    pub(crate) transition_frame: u16,

    // = segvga:34fc data_segvga_034fc — the water-ripple row counter the
    // vision-dream shimmer (vga effect 0x0a) advances one row per pass.
    pub(crate) vision_shimmer_phase: u16,
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
            ctrl_v_cheat_done: false,
            log_condit: false,
            log_subtitle: false,

            // ---- Host/runtime state and buffers ----
            dat_file,
            screen: FrameBuffer::new(320, 200),
            screen_pal: Palette::new(),
            y_offset: 24,
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
            let_voices_finish: false,
            audio_current_sfx: None,
            audio_current_sfx_data: Vec::new(),
            pcm_voice_stream: None,
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

            // Placeholder; intro_28_init re-creates it seeded with the live
            // palette when the night attack starts.
            attack: None,
            dialogue_played_log: Vec::new(),

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
            for_condit_ds_16: 0,
            for_condit_ds_18: 0,
            line_spoken_this_conversation: 0,
            related_to_arguing_ds_1a: 0,
            related_to_paying_smuggler_bills_ds_1c: 0,
            current_smuggler_willingness_to_haggle_ds_1d: 0,
            related_to_paying_smuggler_bills_ds_1f: 0,
            current_smuggler_bill_value_ds_20: 0,
            current_smuggler_number_of_days_since_previous_encounter_ds_1e: 0,
            smuggler_bills_count_ds_22: 0,
            data_0001b: 0,
            pending_room_action: 0,
            for_dialogue_enemies_ds_24: 0,
            number_of_sietches_visited: 0,
            entering_new_sietch: 0,
            // = seg001:0027 static init 3.
            discovered_sietch_count: 3,
            number_of_rallied_troops: 0,
            charisma: 0,
            game_phase: 0,
            night_attack_stage: 0,
            night_attack_backdrop_sprite: 0x31,
            contacting_troops_ds_4c: 0,
            spice_in_stock: 0,
            area_controlled_by_atreides: 0,
            area_controlled_by_harkonnen: 0,
            todays_spice_production: 0,
            harkonnen_spice_production: 390,
            data_000aa: 0,
            data_000ac: 0x1b58,
            previous_day_spice_production: 0,
            spice_production_better_than_previous_day: 0,
            spice_production_lower_than_previous_day: 0,
            spice_shipment_quantity: 0,
            spice_shipment_fulfilment: 0x80,
            spice_shipment_flags: 0,
            spice_shipment_arguing_ds_b4: [0; 4],
            for_condit_smuggler_dialogue_related_ds_9d: 0,
            for_condit_smuggler_arguing_count_ds_9e: 0,
            accept_refuse_argue_choice_ds_9f: 0,
            argue_menu_with_smuggler: 0,
            shipment_report_scene_mask: 0,
            for_condit_spice_shipment_ds_c0: 0,
            final_attack_stage: 0,
            spice_shipment_sequence_number: 0,
            number_of_sietches_attacked_by_harkonnen: 0,
            person_marker_base: 0,
            data_000c6: 0,
            data_000c8: 0,
            comm_sightings: Vec::new(),
            comm_unread_count_ds_c9: 0,
            nearest_location: NearestLocation::default(),
            days_left_until_spice_shipment: 0,
            nearest_village: NearestLocation::default(),
            contact_distance_related_ds_d5: 0,
            nearest_sietch: NearestLocation::default(),
            comm_list_filter_seen: 0,
            nearest_atreides_area: NearestLocation::default(),
            data_000e1: 0,
            nearest_harkonnen_area: NearestLocation::default(),
            paul_found_unconscious_ds_e7: 0,
            ui_hud_head_index: 0,
            for_condit_ds_e9: 0,
            data_000ea: 0,
            comm_message_needs_viewing_ds_eb: 0,
            data_000ed: 0,
            data_000ee: 0,
            desert_exhaustion_counter: 0,
            for_condit_jessica_commented_on_exhaustion_ds_f5: 0,
            for_condit_paul_next_to_harvester_ds_f6: 0,
            for_condit_chani_prisoner_location_area_and_name_ds_f2: 0,
            number_of_locations_with_illness: 0,
            chani_troop_illness_cure_progress: 0,
            latest_location_with_illness: 0,
            room_view_toggle: 0xff,
            data_000fc: 1,
            for_condit_battle_related_ds_fd: 0,
            game_phase_copy_ds_fe: 0,
            days_since_last_game_phase_change: 0,
            locations: LOCATIONS,
            troops: TROOPS,
            globe_param_3: 0,
            globe_param_4: 0,
            room_persons: ROOM_PERSON_TABLE_INIT,
            smugglers: crate::smugglers::SMUGGLERS,
            current_smuggler_ptr: crate::smugglers::SMUGGLERS_SEG001_OFS,

            // = the seg001:1141 static initializer.
            worm_event_likelihood_by_region: [
                0x03, 0x0d, 0x0f, 0x32, 0x64, 0x80, 0x28, 0x14, 0x28, 0x23, 0x32, 0x46, 0x80,
            ],
            current_location_index: 0xffff,
            last_location_index: 0,
            companions: [-1, -1],

            // = seg001:1154/1156 both static init 0xffff — disarmed until
            // their phase callbacks stamp them.
            harkonnen_raids_armed_after_game_time: 0xffff,
            illness_plot_armed_after_ingame_day: 0xffff,
            // = seg001:115c static init 0xffff.
            results_stats_timestamp: 0xffff,
            // = seg001:115e static init 14h, 1, 0, 0, 0, 0.
            results_prev_values: [0x14, 1, 0, 0, 0, 0],
            results_trend_glyphs: [0; 6],
            spice_stock_at_last_new_day: 0,
            spice_spent_today: 0,
            // = seg001:1174 static init 2 (the start-of-game clock).
            last_event_game_time: 2,
            location_visibility_distance: 1,
            number_of_rallied_troops_for_leto_killed: 0xff,
            ingame_day_of_last_spice_shipment_event: 0,
            vision_messages: Vec::new(),
            spice_shipment_unpaid: 0,
            harkonnen_raid_suppress_once: 0,
            data_011bc: 0,

            // = seg001:11bd/11bf both init dw 0aah (the log head lives in
            // dialogue_played_log's length).
            book_bookmark_ptr: 0xaa,
            travel_destination_ptr: 0,
            travel_heading: 0,
            travel_heading_mode: 0,
            game_screen_mode_flags: 0,
            data_011ca: 0,
            travel_no_location_dest: 0,
            travel_step_accum: 0,
            condit_staged_location: 0,
            staged_name_location: 0,
            prospector_destinations: [0; 4],

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
            scene_records: crate::room_scene::SCENE_RECORDS,
            travel_trail_cursor: 0,
            map_location_info_panel: crate::troop_map_screen::LOCATION_INFO_PANEL,
            map_troop_info_panel: crate::troop_map_screen::TROOP_INFO_PANEL,
            map_troop_contact_text_panel: crate::troop_map_screen::TROOP_CONTACT_POPUP_PANEL,
            map_troop_contact_head_panel: crate::troop_map_screen::TROOP_CONTACT_HEAD_PANEL,
            map_equipment_troop_row_box: crate::troop_map_screen::EQUIPMENT_TROOP_ROW_BOX,
            map_equipment_location_strip: crate::troop_map_screen::EQUIPMENT_LOCATION_STRIP,
            data_0194a: crate::troop_map_screen::RALLIED_POPUP_PANEL,
            map_selected_troop_id: 0,
            map_last_selected_troop_id: 0,
            data_01968: 0,
            data_0196a: 0,
            data_0196c: 0,

            // = the seg001:197c/197e compiled-in statics (0x1964, -4) — the
            // orientation the intro2 globe scene renders before anything
            // re-seeds the view centre.
            zoomed_globe_longitude: 0x1964,
            zoomed_globe_latitude: -4,
            globe_renderer: None,
            ui_elements: UI_ELEMENTS_INIT,

            // The only reads of the NAV_PANEL_* consts: the templates are
            // mutable state from here on.
            nav_panel_room: game_ui::NAV_PANEL_ROOM,
            nav_panel_alt: game_ui::NAV_PANEL_ALT,
            nav_panel_blank: game_ui::NAV_PANEL_BLANK,
            nav_panel_flight: game_ui::NAV_PANEL_FLIGHT,
            nav_panel_globe: game_ui::NAV_PANEL_GLOBE,
            nav_panel_book: game_ui::NAV_PANEL_BOOK,

            // = the static seg001 menu buffers, initialized to their compiled-in
            // contents (priority byte + records; command_menu_buf and
            // menu_multiple_cancel start empty and are filled by their builders).
            command_menu_buf: menu_defs::COMMAND_MENU_BUF.into(),
            menu_npc_actions: menu_defs::MENU_NPC_ACTIONS.into(),
            menu_go_towards_this_place: menu_defs::MENU_GO_TOWARDS_THIS_PLACE.into(),
            menu_destination_warning: menu_defs::MENU_DESTINATION_WARNING.into(),
            menu_continue_or_what: menu_defs::MENU_CONTINUE_OR_WHAT.into(),
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
            menu_stack: vec![(MenuRef::CommandMenuBuf, None)],
            cmd_skip_to_destination_flags: 0,

            // = seg001:2220 dw menu_prospector_troop_after_specializing_in_
            //   spice — the static initial value.
            sequence_menu: MenuRef::MenuContinueOrWhat,
            ui_hud_companion_blink: [0, 0],
            map_contact_subtitle_pos: (0, 0),
            map_contact_subtitle_h: 0x3f,
            data_0227d: 1,
            sky_skydn_selector: 0,
            book_topic_filter: 0,
            book_page_video_id: 0,
            globe_tilt: 0,
            globe_decoration_offset: 0,
            active_mouse_handlers: &ROOM_MOUSE_HANDLERS,
            cursor_image: None,
            line_pattern: 0xffff,
            banks: Banks::new(),
            troop_icon_draw_by_depth: false,
            game_suspend_count: 1,
            settings_records: SETTINGS_RECORDS_INIT,
            settings_drag_target: 0,
            voice_subtitle_mode: 0,
            voice_subtitle_mode_default: 0,
            cmd_args_memory: 0,
            hnm_bytes: None,
            hnm_lop_bytes: None,
            hnm_lop_video_id: 0,
            hnm_lop_cursor: 0,
            hnm_lop_remaining: 0,
            voc_filename: *b"PF\\PF001I .VOC",
            chained_narration_clip: 0,
            game_over_voc_index: 0x0fff,
            initial_game_image: None,
            music_cd_playlist: crate::music::MUSIC_CD_STANDARD_ORDER,
            music_cd_playlist_cursor: 0,
            music_playlist_flags: 0,
            pending_music_mode: None,
            troop_icons: Vec::new(),
            head_popup_anchor: (0, 0),
            head_popup_box: Rect::default(),
            current_sky_palette: 0,
            sky_fade_countdown: 0,
            pending_room_screen_request: 0,
            events_pump_active: 0,
            data_046db: 0,
            new_time_period_pending: 0,
            new_day_flag: 0,
            sky_fade_active: false,
            data_046e0: 0,
            spice_harvest_remainder: 0,
            map_view_rect: Rect::default(),
            data_046eb: 0,
            map2: Box::new([]),
            spice_density_overlay_dirty: 0,
            current_main_view_drawing_function: None,
            map_contact_troop: None,
            map_troop_equipment_row_up: 0,
            map_modify_equipment_mode: 0,
            map_contact_troop_pending: None,
            map_view_reentry_count: 0,
            troop_icon_anim_phase: 0,
            map_location_popup_loc: None,
            map_location_popup_class: 0,
            map_info_popup_troop: None,
            data_046fc: 0,
            available_equipment: Equipment::default(),
            troop_equipment_flags: [0; 7],
            map_equipment_column_x_ranges: [(0, 0); 7],
            map_equipment_troop_column_x_ranges: [(0, 0); 7],
            map_overlay_panel_pos: (0, 0),
            map_overlay_panel_rect: Rect::default(),
            prospector_pick_queue: [0; 4],
            prospector_pick_count: 0,
            map_overlay_anim_src: None,
            map_overlay_mode: 0,
            map_overlay_hover_tick: 0,
            map_overlay_footer_label_color: 0,
            data_04726: 0,
            travel_active: 0,
            travel_minimap_state: 0,
            travel_step_tick_stamp: 0,
            travel_step_counter: 0,
            orni_hotspot_x: 0,
            orni_hotspot_y: 0,
            orni_anim_frame: 0,
            spice_mining_troops_with_harvester_in_location: 0,
            sprite_anim: crate::room_scene::SpriteAnim::default(),
            data_04732: 0,
            loaded_sal_index: 0xff,
            sal_sheet: None,
            desert_step_counter: 0,
            room_redraw_request: 0,
            map_ornithopter_mode: 0,
            map_caption_text: Vec::new(),
            map_caption_pos: 0,
            map_caption_x: 0,
            map_caption_y: 0,
            map_caption_color: 0,
            map_player_marker_rect: Rect::default(),
            map_player_marker_phase: 0,
            troop_icon_focused: [None; 2],
            fremen1_troop: None,
            fremen2_troops: [None; 8],
            harkonnen_captain_troop: None,
            data_0476a: 0,
            data_0476b: 0,
            selected_fremen2: 0,
            vegetation_started_on_dune: 0,
            troop_irrigated_this_period: 0,
            bulb_growing_progress: 0,
            gurney_location_ptr: 0,
            troop_condit: Default::default(),
            location_condit: Default::default(),
            npc_menu_idle_timer_base: 0,
            npc_menu_idle_timer_limit: 0,
            npc_menu_idle_last_tick: 0,
            is_dialogue_active: false,
            sequence_saved_scene: (0, 0),
            sequence_return_cursor: None,
            sequence_script: None,
            sequence_cursor: 0,
            dialogue_current_record_ptr: 0,
            current_subtitle_id: 0,
            subtitle_word_count: 0,
            subtitle_pad_left: 0,
            subtitle_pad_right: 0,
            subtitle_pad_top: 0,
            subtitle_pad_bottom: 0,
            subtitle_layout_flags: 9,
            balloon_x: 0x50,
            subtitle_bubble: None,
            room_render_flags: 0,
            dialogue_interrupt_gate: 0,
            data_047a6: 0,
            data_047a7: 0,
            dialogue_end_request: 0,
            comm_displayed_message_person: 0,
            data_047aa: 0,
            phrase_bin: Vec::new(),
            current_phrase_bin_id: 0,
            dialogue_text_continuation: None,
            dialogue_resume_entry_ptr: 0,
            dialogue_topic_index: 0,
            data_047c2: 0,
            current_lip_sync_resource_id: 0,
            data_047dc: 0,
            last_line_voc_bank_flag: 0,
            post_voice_hook: None,
            dialogue_line_word0: 0,
            data_047e0: 0,
            head_sign_state: 0,
            head_sign_anim: 0,
            head_sign_record: None,
            character_screen_pos: [(0xffff, 0xffff); 0x17],
            condit: Default::default(),
            dialogue: Default::default(),
            map: Default::default(),
            tablat: None,
            worm_anim: None,
            travel_vehicle_mode: 0,
            globe_rotation: 0,
            visible_location_markers: Vec::new(),
            ui_hud_head_saved_strip: vec![0; 20 * 10],
            ui_hud_head_animating_down: false,
            pause_enabled: 0,
            language_setting: 0,
            mouse_last_click_time: 0,
            voc_bases: [0; 17],
            rand_seed: 1,
            rand_bits_seed: 1,
            settings_flags: 0x1 | 0x4 | 0x8 | 0x100 | 0x400 | 0x800,
            music_desired_song: 0,
            music_song_end_tick_stamp: 0,
            rand_iterated_seed: 0,
            screen_buffer: FbId::Screen,
            active_fb: FbId::Fb1,
            map_popup: MapPanelRef::None,
            map_popup2: MapPanelRef::None,
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
            hnm_video_frame_ready: false,
            voc_pcm_playing: false,

            // Seeded to the startup position so the shared input and the
            // host pointer agree before initialize_system runs its warp.
            mouse_pos_x: MOUSE_START_X,
            mouse_pos_y: MOUSE_START_Y,
            mouse_draw_pos_x: 0,
            mouse_draw_pos_y: 0,
            // Starts hidden; initialize_system sets the DOS value.
            cursor_hide_counter: -1,
            mouse_cursor_restore_needed: 0,
            mouse_button_state: 0,
            kb_glide_tick: 0,
            kb_glide_steps: 0,
            kb_glide_target_x: 0,
            kb_glide_target_y: 0,
            kb_move_tick: 0,
            kb_move_dist_x: 0,
            kb_move_dist_y: 0,
            kb_move_frac: [0; 2],
            kb_button_prev: 0,
            mouse_nav_rect: None,
            game_clock_tick_base: 0,
            drag_armed_element: None,
            mouse_prev_drag_x: 0,
            mouse_prev_drag_y: 0,
            last_task_tick: 0,
            game_clock_last_tick: 0,
            frame_tasks,
            frame_task_walk_next: None,
            command_menu_more_state: 0,
            command_menu_more_slot: 0xff,
            in_transition: 0,
            index_of_last_hovered_action_item: 0xff,
            command_menu_slot_count: 0,
            cursor_save: Vec::new(),
            cursor_save_pos: 0,
            cursor_save_w: 0,
            cursor_save_h: 0,
            companion_blink_step_latch: 0,
            globe_draw_area_control_colors: 0,
            globe_screen_active: 0,
            results_gauge_targets: [0; 6],
            results_gauge_current: [0; 6],
            comm_glow_index: 0,
            map_popup_anim_src: (0, 0),
            map_popup_anim_rect: Rect::default(),
            map_popup_anim_suppress: false,
            xor_bracket_anim_move_step: (0, 0),
            xor_bracket_anim_expand_step: (0, 0),
            xor_bracket_anim_center: (0, 0),
            xor_bracket_anim_shape: (0, 0, 0, 0),
            ecology_lfsr_state: 1,
            travel_trail_ring: [(0x800, 0x800); TRAVEL_TRAIL_LEN],
            sequence_blink: false,
            map_renderer: MapRenderer::new(),

            // = segvga:2768/276a static init `dw 8` / `dw 1`.
            transition_col: 8,
            transition_frame: 1,
            vision_shimmer_phase: 0,
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
    // cutscenes and game_loop are not ported yet.
    //
    // `skip_intro` is a port-only convenience (no DOS equivalent): when set it
    // jumps straight to the in-game UI, skipping the intro/credits/intro2.
    pub fn start(&mut self, skip_intro: bool) {
        // = seg000:0006 call initialize_system.
        self.initialize_system();
        // = seg000:0009 call initialize_resources (the port front-loads the
        // constructor's DNCHAR/COMMAND loads and defers the rest; this brings
        // in the resources interpreted at runtime).
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

        // = seg000:0013 call play_intro_floppy. It self-skips its WORMSUIT
        // cutscenes when `skip_intro` is set (or ESC ended an earlier phase,
        // seg000:0226 jz); its tail sets the game up at the palace throne room
        // (location_and_room 0x200a / location_appearance 0x180) and resets
        // fb_base_ofs to 0 for the in-game screen.
        self.intro_floppy_play(skip_intro || self.intro_skip_to_game);

        // = seg000:0016
        self.midi.midi_reset();

        // = seg000:0019 mov [music_playlist_flags], 0
        self.music_playlist_flags = 0;
        // Port-only: the `--music` selection set_music_mode held back, landed
        // now that the reset above is out of the way. Nothing pending (every
        // caller but the CLI) leaves the music state untouched.
        self.apply_pending_music_mode();

        // = seg000:001e mov [game_time], 2 — start the in-game clock at 2 (the
        // PIT game-clock ISR that advances it is not ported yet).
        self.game_time = 2;

        // = seg000:0024 call init_game_ui (loc_00083).
        self.init_game_ui();

        // = seg000:0027/0029 cl=0xff; call create_save_cl — DOS writes the
        //   fresh game as dune37s0.sav, the image RESTART GAME reloads. The
        //   port keeps that image in memory (initial_game_image) instead of
        //   writing a file.
        self.initial_game_image = Some(self.create_save_in_memory());

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

    // = seg000:e594 initialize_system — the DOS startup: clear the data
    // segment, load the VGA driver, allocate the framebuffers, hook the
    // interrupts, probe the input and audio devices, then leave fb1 active,
    // cleared and copied to fb2. Most of it is DOS machinery the port has
    // no use for; the steps that touch game state are kept in DOS order.
    pub fn initialize_system(&mut self) {
        // = seg000:e599..e5b1 [not needed] — zero seg001 from _word_2316C_error_msg
        //   up, seed the bump allocator and compute the back-buffer segment.
        // = seg000:e5c2..e5da [not needed] — INT 21h default-drive and
        //   Ctrl-Break queries.
        // = seg000:e5dc call open_dune_dat — DatFile::open, run by the binary
        //   before the constructor.
        // = seg000:e57b load_driver_ax_with_vtable_at_si [not needed] — load
        //   DNVGA.BIN or DN386.BIN (cmd arg bit 0) and bind its vtable; the
        //   gfx module is that driver compiled in.
        // = seg000:e5ee..e5f2 vga_get_framebuffer_info [not needed] — the
        //   screen buffer segment and size come from the driver.
        // = seg000:e5f5 call set_screen_as_active_framebuffer.
        self.set_screen_as_active_framebuffer();
        // = seg000:e5fc..e60d [not needed] — bump-allocate fb1 (and the front
        //   buffer when the driver has none); the port's framebuffers are
        //   fields.
        // = seg000:e610 vga_set_mode_13h [not needed].
        // = seg000:e614..e61e language_setting = (cmd_args >> 2) & 7 — the
        //   port has no language switch yet; the field stays 0.
        // = seg000:e622..e62f initialize_joystick / initialize_mouse
        //   [not needed] — device probes (cmd arg bits 7 and 6).
        // = seg000:e632 initialize_pit_timer [not needed] — hook INT 8 and
        //   calibrate the timer; the port's clock is the frame sink's.
        // = seg000:e635 init_extended_memory_allocator [not needed].
        // = seg000:e638..e640 vga_set_grayscale_mode [not needed] — cmd arg
        //   bit 1.
        // = seg000:e644 mov [kb_pointer_key_table_ptr], 271ch [not needed] —
        //   the keypad direction table is a constant in kb_pointer.rs.
        // = seg000:e64a mov [cursor_hide_counter], 0ffh — the cursor starts
        // hidden; the first redraw_mouse pass (game_loop) clears the counter
        // and shows it.
        self.cursor_hide_counter = -1;
        // = seg000:e64f..e659 define_mouse_range (0,0)-(319,199) [not needed]
        //   — the window bounds the pointer.
        // = seg000:e65c..e662 call warp_mouse_cursor — the startup pointer
        // position (237, 171).
        self.warp_mouse_cursor(MOUSE_START_X, MOUSE_START_Y);
        // = seg000:e665 initialize_audio [not needed] — the driver pick and
        //   FREQ.HSQ timing test.
        // = seg000:e668 hnm_initialize_memory_handler [not needed].
        // = seg000:e66b call set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = seg000:e66e call gfx_clear_active_framebuffer.
        self.gfx_clear_active_framebuffer();
        // = seg000:e671 jmp copy_active_framebuffer_to_framebuffer_2.
        self.copy_active_framebuffer_to_framebuffer_2();
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

        // = seg000:00b9/00bc — after initialize_resources2 returns, run the
        // game-phase trigger record twice. Each walk presents the first
        // condition-matching unspoken entry of DIALOGUE slot 135 (records
        // 0x456..) silently (subtitles suppressed, pseudo-speaker 0x10 skips
        // the talking head) and appends it to the dialogue-played log, so a
        // new game's BOOK opens with two pages — the "On Dune, the desert
        // covers the entire planet." and "Paul Atreides arrived on Dune with
        // his father, ..." narrations, both carrying a book video (HNM
        // 0x19/0x1a via book_video_page_words[0..2]).
        self.run_game_phase_triggers();
        self.run_game_phase_triggers();
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

        // = seg000:d81b mov byte ptr [kb_glide_steps], 0 — no keyboard
        //   pointer glide in progress: the first pass polls the mouse.
        self.kb_glide_steps = 0;
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

            // = seg000:d820..d82e — the Ctrl+V one-shot cheat: the buffered
            // key-press scancode is 'V' (0x2f) while Left Ctrl (kb_keys[0x1d];
            // chani labels it "_w" but 0x1d is Left Ctrl, not W) is held.
            // DOS only compares the scancode buffer here — it does not consume
            // it — and relies on handle_ctrl_v_once's self-patch (RET, ported
            // as ctrl_v_cheat_done) to make the repeated passes no-ops.
            let ctrl_v = {
                let input = self.input.lock().unwrap();
                input.key_hit_scancode == 0x2f && input.kb_keys[0x1d] != 0
            };
            if ctrl_v {
                self.handle_ctrl_v_once();
            }

            // = seg000:d831 pending_room_screen_request == 0 -> run the
            // pre-swap hooks: ui_hud_companion_blink_task (seg000:d7b7, the
            // new-companion portrait blink) and loc_01b0d (seg000:1b0d), which
            // advances post-voice game state.
            if self.pending_room_screen_request == 0 {
                // = seg000:d838 call ui_hud_companion_blink_task.
                self.ui_hud_companion_blink_task();

                // = seg000:1b0d game_loop_sub_01b0d — gated on no voice
                // playing (is_voc_pcm_playing), the clock not suspended and
                // the game not ended (game_phase < 0xc8): run the idle-room
                // message check (loc_02b2a), then the per-period events
                // (seg000:1b23). run_events itself consumes
                // new_time_period_pending, so the flag pre-check just skips
                // the call when nothing is pending.
                if !self.talking_head.as_ref().is_some_and(|h| h.speaking)
                    && self.game_suspend_count == 0
                    && self.game_phase < PHASE_C8_GAME_WON
                {
                    self.idle_room_message_check();
                    if self.new_time_period_pending != 0 {
                        self.run_events_for_current_time_period();
                    }
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

            // = seg000:d841/d848 a pending room-screen request is applied
            // here (apply_pending_room_screen_request, seg000:0d8e): the
            // game-over presenter for the positive codes, a no-op once the
            // byte has been flipped to 0x80.
            if self.pending_room_screen_request != 0 {
                self.apply_pending_room_screen_request();
            }

            // = seg000:d84b call rand; mov [rand_bits], ax.
            self.rand_bits = self.rand();

            // = seg000:d851 call travel_pump — the in-game travel pump: while a
            //   flight is active (travel_active) it drives the flight HNM and a
            //   travel step every 0x300 ticks (travel_map_screen.rs).
            self.travel_pump();

            // = seg000:d854 cmp [kb_glide_steps],0; jz — a keyboard pointer
            //   glide in progress steps the pointer instead of polling the
            //   mouse (kb_pointer_glide_step ends in mouse_stuff).
            let ax = if self.kb_glide_steps != 0 {
                self.kb_pointer_glide_step()
            } else {
                // = seg000:d860 call poll_pointer_input; call mouse_stuff.
                self.poll_pointer_input();
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

        // = seg000:ef8b dec word ptr [46dbh]; jns (skip while still >= 0).
        self.data_046db -= elapsed_ticks as i32;

        // = seg000:ef91..ef9b reload, inc game_time, and set
        // new_time_period_pending on each underflow.
        while self.data_046db < 0 {
            self.data_046db += GAME_CLOCK_TICKS_PER_HOUR + 1;
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
            //   clock does not also bump game_time mid-pump.
            self.data_046db = GAME_CLOCK_TICKS_PER_HOUR;

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

    /// One game tick: the 200Hz PIT the game clock and every wait loop run on.
    const TICK_NANOS: u64 = 4_992_530; // 4.99253ms

    /// Returns the number of game ticks since game start (200Hz, 4.99253ms per tick)
    pub fn game_ticks(&self) -> u64 {
        let elapsed_nanos = self.game_start.elapsed().as_nanos() as u64;
        elapsed_nanos / Self::TICK_NANOS
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
        // The DOS wait loops spin on the PIT counter, so they resume on the
        // tick edge. Sleep to that absolute deadline rather than for a whole
        // tick span measured from now: `start` is read mid-tick, so a relative
        // sleep overshoots by the part of the tick already gone, and in a long
        // wait loop the error compounds (the 410-batch LFSR dissolve ran a
        // quarter longer than the original before this).
        let deadline = std::time::Duration::from_nanos((start + ticks) * Self::TICK_NANOS);
        if let Some(remaining) = deadline.checked_sub(self.game_start.elapsed()) {
            std::thread::sleep(remaining);
        }
    }

    // = seg000:da25 add_frame_task — append a per-frame callback. DOS never
    // dedupes: the same callback can sit in the array twice (a troop-contact
    // line arms the voice from both seg000:a0c9 and seg000:7c36), and
    // remove_frame_task takes one entry per call.
    pub(crate) fn add_frame_task(&mut self, interval: u16, task_id: TaskId) {
        // = seg000:da2a..da30 inc count; cmp ax,14h; ja ret — at most 20
        //   entries; a full array drops the add.
        if self.frame_tasks.len() >= 20 {
            return;
        }
        self.frame_tasks.push(FrameTask {
            interval,
            accumulator: 0,
            task_id,
        });
        // = seg000:da47..da4f inside a dispatch, inc the saved cx so the new
        //   entry is reached this pass. The walk runs against the live length,
        //   so nothing to patch here.
    }

    // = seg000:da5f remove_frame_task — drop the FIRST entry with this
    // callback (seg000:da69 scans and stops at a match) and compact the tail
    // down over it.
    pub(crate) fn remove_frame_task(&mut self, id: TaskId) {
        let Some(idx) = self.frame_tasks.iter().position(|t| t.task_id == id) else {
            return;
        };
        // = seg000:da76 dec count; da9b..daa0 rep movsw.
        self.frame_tasks.remove(idx);
        // = seg000:da7a..da8d inside a dispatch: an entry after the cursor
        //   (`cmp di,[bp]; ja`) only shortens the saved cx, which the live
        //   length already reflects; an entry at or before it (`sub [bp],6`)
        //   shifted the rest down one slot, so the walk steps back to land on
        //   the entry that moved into the cursor's slot.
        if let Some(next) = &mut self.frame_task_walk_next {
            if idx < *next {
                *next -= 1;
            }
        }
    }

    pub(crate) fn has_frame_task(&self, id: TaskId) -> bool {
        self.frame_tasks.iter().any(|t| t.task_id == id)
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

        // = seg000:d9e3..da21 walk the LIVE array: si steps one entry per
        // pass with cx = the entries left, and a callback runs with that pair
        // published through frame_task_dispatch_sp so add_frame_task /
        // remove_frame_task can patch it. Every patch keeps (walked + cx) equal
        // to the live count — add bumps both (seg000:da4f), remove drops both
        // (seg000:da8d, or da87 with the cursor) — so the bound is simply the
        // live length, and the one patch that changes where the walk goes next
        // is a removal at or before the cursor (frame_task_walk_next). A
        // snapshot of the due ids would not do: a task removed from inside a
        // callback (lip_sync_stop under a contact cut) still got one run.
        let mut i = 0;
        while i < self.frame_tasks.len() {
            let task = &mut self.frame_tasks[i];
            // = seg000:d9ee..d9f4 ax = elapsed + accumulator (wrapping); due
            //   when it reaches the interval (>=). = seg000:da04 interval 0
            //   fires every pass and keeps its accumulator.
            let acc = elapsed.wrapping_add(task.accumulator);
            if task.interval != 0 {
                if acc < task.interval {
                    // = seg000:d9f6..d9fb not due: store it, next entry.
                    task.accumulator = acc;
                    i += 1;
                    continue;
                }
                // = seg000:da0a `div bp` — carry the remainder so the period
                //   stays exact.
                task.accumulator = acc % task.interval;
            }
            let task_id = task.task_id;
            // = seg000:da11..da14 push {bx, cx, si}; frame_task_dispatch_sp =
            //   sp. A dispatch nested through wait_processing_frame_tasks
            //   republishes its own walk; DOS leaves the cell cleared when it
            //   returns (seg000:d9fd), so the outer callback's later removals
            //   go unpatched there — the port restores the outer walk instead.
            let outer = self.frame_task_walk_next.replace(i + 1);
            self.run_frame_task(task_id);
            // = seg000:da1b..da21 pop {si, cx, bx} as patched; add si,6; loop.
            i = self.frame_task_walk_next.take().unwrap_or(i + 1);
            self.frame_task_walk_next = outer;
        }
        // = seg000:d9fd frame_task_dispatch_sp = 0.
    }

    // = seg000:da18 `call word ptr [si+4]` — the task's callback.
    fn run_frame_task(&mut self, task_id: TaskId) {
        match task_id {
            TaskId::HnmDoFrame => {
                self.hnm_frame_task();
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
            TaskId::DesertHarvester => {
                self.desert_harvester_frame_task();
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
            TaskId::VisionShimmer => {
                self.tick_vision_shimmer();
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
    // = segvga:2572 transition_frame_wait — the wait between transition frames.
    pub fn present_transition_frame(&mut self) {
        // = segvga:2572 loc_segvga_02572 `sub ax,bx; cmp ax,3; jb` — 3 ticks (~15ms).
        self.present_transition_frame_ticks(3);
    }

    // The `ticks`-per-step form of `present_transition_frame`. Not every effect
    // paces on loc_segvga_02572: the LFSR dissolve (dissolve_lfsr_body,
    // segvga:2a5a) has its own `cmp bx,[bp]; jz` spin, which waits only for the
    // PIT counter to change — one tick per batch, not three.
    pub fn present_transition_frame_ticks(&mut self, ticks: u64) {
        let start = self.game_ticks();
        self.send_frame_to_display();
        self.sleep_ticks(start, ticks);
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

    // = seg000:a90b open_pcm_voice_file + voc_get_lipsync_data's stream
    // setup — begin streaming a voice .VOC to the driver in PCM_VOICE_CHUNK
    // pieces. The first chunk is the file's own first 0x2000 bytes (the VOC
    // header and the type-5/type-1 blocks sit inside it; the block engine
    // walks past them), started as job A; the second chunk is queued right
    // away (= the pcm_voice_stream_refill call at seg000:a879 / ab7a). Later
    // chunks arrive through the per-tick pumps: lip_sync_frame_task's tail
    // (seg000:a811) for a talking head, the ab92 monitor for narration.
    //
    // A clip that fits inside the first chunk starts with the stop-at-end
    // flag and leaves no stream behind (= read_audio_file going negative on
    // the first read: seg000:a99d ORs 80h into +7 and falls into
    // close_pcm_voice_file_handle).
    //
    // The caller runs pcm_stop_voc first, exactly as DOS does (seg000:a84a /
    // ab6d), so start_playback is only ever refused when no PCM card is
    // modelled — in which case nothing is left to starve.
    pub(crate) fn pcm_voice_stream_start(&mut self, data: Box<[u8]>) {
        // = seg000:a910..a916 — open_pcm_voice_file zeroes both ping-pong job
        // state words before the new stream. A chunk left queued when the
        // previous clip was cut is still chain-eligible (voc_blk0_terminator
        // checks the queued state, seg001:068c) and would play as a snippet
        // of the old line right after this clip's first chunk.
        self.pcm_player.clear_queued();
        let first = data.len().min(PCM_VOICE_CHUNK);
        let last = first >= data.len();
        let flags = if last { pcm_player::VOC_STOP_AT_END } else { 0 };
        if !self.pcm_player.start_playback(&data[..first], flags) {
            return;
        }
        if !last {
            self.pcm_voice_stream = Some(PcmVoiceStream {
                data,
                offset: first,
            });
            self.pcm_voice_stream_refill();
        }
    }

    // = seg000:a9b9 pcm_voice_stream_refill — feed the driver the next
    // PCM_VOICE_CHUNK file bytes. Returns if the handle is closed
    // (check_pcm_voice_file_open == 0) or both ping-pong jobs are still
    // queued (seg000:a9c1/a9c8; the port's single queued slot is the free
    // buffer). The chunk reaches the driver as a type-2 continuation block
    // (= the header dnsdb_queue_next_impl writes, seg001:01db..01e9), so the
    // driver chains to it at the current chunk's end — or auto-starts it
    // from ENDED after an underrun. The chunk that exhausts the file gets
    // the stop-at-end flag and closes the handle (= seg000:a99d falling into
    // close_pcm_voice_file_handle).
    pub(crate) fn pcm_voice_stream_refill(&mut self) {
        let Some(stream) = self.pcm_voice_stream.as_mut() else {
            return;
        };
        if self.pcm_player.queue_slot_filled() {
            return;
        }
        let end = (stream.offset + PCM_VOICE_CHUNK).min(stream.data.len());
        let last = end >= stream.data.len();

        // = seg001:01d0..01da — the driver shortens a stop-at-end job by one
        // byte: the file's last byte is the VOC terminator, which must not
        // reach the DAC as a sample (0x00 is a full-scale negative click).
        let data_end = if last {
            end.saturating_sub(1).max(stream.offset)
        } else {
            end
        };
        let chunk = build_pcm_voc_continuation(&stream.data[stream.offset..data_end]);
        stream.offset = end;
        let flags = if last { pcm_player::VOC_STOP_AT_END } else { 0 };
        self.pcm_player.queue_next(&chunk, flags);
        if last {
            self.pcm_voice_stream = None;
        }
    }

    // = seg000:ac14 pcm_stop_voc — the one routine that cuts a clip
    // instantly: remove the ab92 refill/music-restore monitor, close the
    // streaming handle (so nothing re-feeds the driver) and stop the driver
    // itself.
    pub(crate) fn pcm_stop_voc(&mut self) {
        // = seg000:ac1b/ac1e remove_frame_task(frame_task_callback_0ab92).
        self.remove_frame_task(TaskId::PcmVoiceMusicRestore);

        // = seg000:ac21 call close_pcm_voice_file_handle.
        self.pcm_voice_stream = None;

        // = seg000:ac24 call [pcm_vtable_stop].
        self.pcm_player.stop();
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
        // previous clip (a still-streaming voice included) before queueing
        // this clip's first buffer.
        self.pcm_stop_voc();

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

    // = seg000:cc85 check_if_hnm_complete — finished once the clip has played
    // its last frame (hnm_finished) or been closed.
    pub fn hnm_is_complete(&self) -> bool {
        self.hnm_finished || !self.hnm_is_open()
    }

    // = seg000:c9f4 hnm_do_frame_and_check_if_frame_advanced / seg000:cad4 hnm_wait_for_frame / seg000:ca59 hnm_stamp_frame_tick
    // — decode the next HNM frame into the framebuffer iff the per-clip tick interval has
    // elapsed. Returns true when a frame was actually decoded. The screen
    // is NOT updated here; the foreground play loop calls
    // `gfx_copy_whole_framebuf_to_screen` after a successful advance
    // (mirroring `gfx_copy_whole_framebuf_to_screen` at seg000:0632).
    pub fn hnm_do_frame(&mut self) -> bool {
        // = seg000:ca60 cmp word ptr [35a6h], 0; jz loc_0ca9a. Once a
        // non-looping clip runs out of frames it is closed/finished. From then on
        // hnm_do_frame is a no-op: hnm_frame_task keeps ticking (clc

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

        // = seg000:ca80..ca8c: decode the next frame (into framebuffer_active = active_fb)
        // and advance. hnm_step_frame returns false if it stepped onto the
        // end-of-stream marker without decoding.
        if !std::mem::take(&mut self.hnm_video_frame_ready) && !self.hnm_step_frame() {
            return false;
        }

        palette_flush(self);

        self.hnm_queue_sd_block();

        true
    }
}

// = seg001:27b6 per-scene zoom focal points (col, row), indexed by the talking
// head id. These line up with the 17 talking-head characters (LETO=0, JESS=1,
// …, CHAN=7, …). A (0, 0) entry means "no zoom for this character".
#[rustfmt::skip]
const ZOOM_FOCAL_POINTS: [(i16, i16); 17] = [
    (0x4c, 0x2f), (0x4b, 0x49), (0x00, 0x00), (0x53, 0x25),
    (0x4c, 0x3e), (0x53, 0x3e), (0x4d, 0x4e), (0x58, 0x3f), // [7] = Chani
    (0x47, 0x41), (0x56, 0x1b), (0x69, 0x5b), (0x00, 0x00),
    (0x4a, 0x29), (0x00, 0x00), (0x5e, 0x57), (0x00, 0x00),
    (0x00, 0x00),
];

// = seg001:279a per-scale source-rect half-extents (col, row), indexed by the
// scale selector 1..7. The source rect is centred on the focal point, so its
// top-left corner is `focal − half_extent`. Each pair is (src_w/2, src_h/2) for
// that scale's kernel. Index 0 is unused (0 terminates a sequence).
#[rustfmt::skip]
const ZOOM_HALF_EXTENTS: [(i16, i16); 8] = [
    (  0,  0), // [0] unused
    (140, 66), // [1] 8/7
    (120, 57), // [2] 4/3
    (106, 50), // [3] 3/2
    ( 80, 38), // [4] 2×
    ( 53, 25), // [5] 3×
    ( 40, 19), // [6] 4×
    ( 20,  9), // [7] 8×
];

// = the zoom step sequences. Positive = scale step; -1 = a long pause on the
// current frame; the trailing 0 terminator is dropped here (the loop ends at
// slice end). The intro uses ZOOM_SEQ_FULL because [227dh] is 1.
const ZOOM_SEQ_FULL: [i8; 7] = [6, -1, 5, 4, 3, 2, 1]; // = seg001:2792
const ZOOM_SEQ_RAND_A: [i8; 4] = [5, -1, 4, 3]; // = seg001:2789
const ZOOM_SEQ_RAND_B: [i8; 3] = [4, -1, 3]; // = seg001:278e

// = seg000:dbe6 data_0dbe6 (set to 6 at seg000:0790): the minimum number of timer ticks
// each zoom step is held (the loc_0c8ed frame-rate gate). game_ticks() is the
// port's PIT counter equivalent.
const ZOOM_STEP_TICKS: u64 = 6;

// = seg000:e387 wait_a_bit(0x12c) at seg000:c8aa — the pause held on a -1 sequence entry.
const ZOOM_PAUSE_TICKS: u64 = 300;

impl GameState {
    // = seg000:c868 loc_0c868 / seg000:c8c1 loc_0c8c1 — the cinematic zoom-in
    // reveal, driving the segvga vga_zoom_screen primitive (gfx/zoom.rs):
    //
    //   - `loc_0c8c1` (seg000:c8c1): one zoom step. The source rectangle is
    //     centred on a per-scene focal point — top-left = focal − half-extent,
    //     clamped ≥ 0 — then the step holds for `data_0dbe6` (= 6) timer ticks.
    //
    //   - `loc_0c868` (seg000:c868): the sequencer. The "scene id" is the talking
    //     head id (`[22a6h]` = `_word_21756_talking_head_id`); it indexes the
    //     focal-point table (seg001:27b6). A (0,0) focal point, an id ≥ 0x11, or a
    //     voice already playing skips the zoom. The step sequence is a list of
    //     signed bytes: a positive value is a scale step, −1 is a long pause on
    //     the current (close-up) frame, 0 terminates. With `[227dh] != 0` (its
    //     intro value, 1) the full sequence seg001:2792 is used; otherwise one of
    //     two shorter sequences is chosen at random. After the sequence the scene
    //     is redrawn 1:1 (present_game_area).
    // = seg000:c8c1 loc_0c8c1 — render one zoom step. Centre the `scale`-sized
    // source rect on `focal` (top-left = focal − half_extent, clamped ≥ 0),
    // blit it to the screen, then hold for ZOOM_STEP_TICKS.
    fn zoom_reveal_step(&mut self, focal: (i16, i16), scale: u8) {
        let (hx, hy) = ZOOM_HALF_EXTENTS[scale as usize];
        // = sub dx,[si+2796h] / sub bx,[si+2798h], each clamped ≥ 0.
        let col = (focal.0 - hx).max(0);
        let row = (focal.1 - hy).max(0);

        gfx::zoom::vga_zoom_screen(self, col, row, scale);
        self.send_frame_to_display();

        // = seg000:c8ed loc_0c8ed: spin until at least data_0dbe6 (6) ticks have elapsed.
        let start = self.game_ticks();
        self.sleep_ticks(start, ZOOM_STEP_TICKS);
    }

    // = seg000:c868 loc_0c868 — the cinematic zoom-in reveal of the current
    // talking-head scene. Runs synchronously (no frame tasks) before the head
    // starts talking; the static composited frame in fb1 is the source.
    pub fn scene_zoom_in_reveal(&mut self) {
        // = call is_voc_pcm_playing; jnz ret — don't zoom over a playing voice.
        let Some(scene) = self
            .talking_head
            .as_ref()
            .filter(|h| !h.speaking)
            .map(|h| h.talking_head_id as usize)
        else {
            return;
        };

        // = mov si,[22a6h]; cmp si,11h; jnb ret — scene id = talking head id.
        if scene >= 0x11 {
            return;
        }

        // = mov dx,[si+27b6h]; mov bx,[si+27b8h]; or ax; jz ret — (0,0) = none.
        let focal = ZOOM_FOCAL_POINTS[scene];
        if focal == (0, 0) {
            return;
        }

        // = seg000:c889..c8a0 select the step sequence on suppress_sky_240_255
        //   (data_0227d): non-zero (the intro, and the cutscene brackets that
        //   inc/dec it) plays the full pull-back sequence seg001:2792; in the
        //   game (seg000:029d zeroes it at start) the idle handler's call from
        //   room_idle_npc_menu_zoom picks one of the two short close-ups at
        //   random: 5,hold,4,3 or 4,hold,3.
        let seq: &[i8] = if self.data_0227d != 0 {
            &ZOOM_SEQ_FULL
        } else if self.rand_masked(1) == 0 {
            &ZOOM_SEQ_RAND_A
        } else {
            &ZOOM_SEQ_RAND_B
        };

        // = seg000:c8a3 loc_0c8a3: lodsb; or al,al; jz end; jns step; (negative) pause.
        for &step in seq {
            if step == 0 {
                break;
            } else if step < 0 {
                // = mov ax,12ch; call wait_a_bit — hold the close-up.
                let start = self.game_ticks();
                self.send_frame_to_display();
                self.sleep_ticks(start, ZOOM_PAUSE_TICKS);
            } else {
                self.zoom_reveal_step(focal, step as u8);
            }
        }

        // = seg000:c8bd loc_0c8bd: call present_game_area — final 1:1 reveal of the whole scene.
        self.gfx_copy_whole_framebuf_to_screen();
        self.send_frame_to_display();
    }
}

impl GameState {
    // = seg000:c8fb loc_0c8fb — foreground-play an HNM clip (DOS ax = the
    // video id) to completion in the game area: open it into fb1, reveal the
    // first frame through the `bp` present callback, then pump frames,
    // presenting the game area after each advance and servicing the CD
    // playlist. Ends with the last frame snapshotted to fb2 and the clip
    // closed.
    pub(crate) fn play_hnm_to_completion(&mut self, video_id: u16, bp: fn(&mut GameState)) {
        // = seg000:c8fb call set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();

        // = seg000:c8ff call hnm_load_first_frame — the in-game fb row offset is 0.
        self.hnm_load_first_frame_by_id(video_id, 0);

        // = seg000:c902/c905 present the game area and flush the header palette.
        self.present_game_area();
        self.update_screen_palette();

        // = seg000:c909 call bp — the caller's first-frame reveal.
        bp(self);

        // = seg000:c90b loc_0c90b — pump to completion. DOS spins on
        // hnm_do_frame_and_check_if_frame_advanced; the port paces on ticks.
        while !self.hnm_is_complete() {
            if self.hnm_do_frame() {
                // = seg000:c910/c913 present the game area + the CD playlist service.
                self.present_game_area();
                self.music_cd_playlist_service();
            }
            self.tick_one_frame();
        }

        // = seg000:c91b snapshot the last frame to fb2; c91e jmp hnm_close_resource.
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

        let (mut fb, pal) = (self.screen.clone(), self.screen_pal.clone());
        if self.debug_overlay {
            // Port-only: composite the debug overlay onto a copy of the screen so
            // the game's own framebuffers stay clean (the overlay must never be
            // baked into fb1/fb2, which the render restores from).
            self.draw_debug_overlay(&mut fb);
        }
        self.frame_sink.publish(fb, pal);
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

        // let day = self.get_ingame_day();
        // (label, value) rows. The value column is placed at a fixed pixel x
        // past the widest label, so the values line up even though the glyph
        // font is proportional (space-padding would not align them).
        let rows: &[(&str, String)] = &[
            ("PHASE", format!("{}", self.game_phase)),
            // (
            //     "LOC",
            //     format!("{:#06x} room {}", self.location_and_room, self.current_room),
            // ),
            // ("APPEAR", format!("{:#06x}", self.location_appearance)),
            // ("DAY", format!("{}  time {:#06x}", day, self.game_time)),
            ("CHARISMA", format!("{}", self.charisma)),
            ("SIETCHES", format!("{}", self.number_of_sietches_visited)),
            ("RALLIED", format!("{}", self.number_of_rallied_troops)),
            // ("MET", format!("{:#06x}", self.persons_met)),
            // ("TRAVEL", format!("{:#06x}", self.persons_travelling_with)),
            // ("IN ROOM", format!("{:#06x}", self.persons_in_room)),
            // ("EXHAUSTION", format!("{}", self.desert_exhaustion_counter)),
            // ("DESERT STEPS", format!("{}", self.desert_step_counter)),
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

    // = seg000:c4cd gfx_copy_whole_framebuf_to_screen / segvga:1b7c vga_copy_screen. Plain memcpy from fb1
    // to the front buffer (`screen_buffer`) — does NOT apply `fb_base_ofs`
    // (matching the DOS `vga_copy_screen_2` behaviour). The y-offset is applied
    // to incoming draws, not to this outgoing copy.
    //
    // When `screen_buffer` is redirected to fb1 (inside
    // gfx_call_bp_with_front_buffer_as_screen during a stage init), the copy is
    // fb1 → fb1, i.e. a no-op — the visible screen is left untouched until the
    // transition reveals fb1.
    pub(crate) fn gfx_copy_whole_framebuf_to_screen(&mut self) {
        // Front buffer redirected to fb1: the copy would be fb1 → fb1.
        if self.front_buffer_is_fb1() {
            return;
        }
        self.screen.copy_from(&self.framebuffer);
    }

    // = seg000:c4dd present_game_area / segvga:1be7 vga_copy_partial — present the game-area rect (0,0)-
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

    // = seg000:c474 copy_game_rect_fb1_to_fb2 — snapshot the game-area rect
    // (game_area_rect) from fb1 into fb2.
    pub(crate) fn copy_game_rect_fb1_to_fb2(&mut self) {
        let yoff = self.y_offset as i16;
        let game_area = crate::rect::rect(0, yoff, 320, 152 + yoff);
        self.gfx_copy_rect_fb1_to_fb2(game_area);
    }

    // = seg000:c477 gfx_copy_rect_fb1_to_fb2 — copy `rect` from fb1 into fb2
    // (the clean scene backup), so a later fb2 restore keeps what was drawn.
    // An empty rect does nothing.
    pub(crate) fn gfx_copy_rect_fb1_to_fb2(&mut self, rect: Rect) {
        // = seg000:c482..c488 sub bp,dx / sub ax,bx — bail on a zero-area rect.
        if rect.x1 <= rect.x0 || rect.y1 <= rect.y0 {
            return;
        }
        // = seg000:c48a..c493 es = fb2, ds = fb1; vga_copy_rect.
        gfx::vga_copy_rect(&mut self.framebuffer_saved, &self.framebuffer, rect);
    }

    // = seg000:c49a gfx_copy_screen_to_framebuffer_1 — fb1 = the visible
    // screen (vga_copy_screen with ds = the screen buffer, es = fb1).
    pub(crate) fn gfx_copy_screen_to_framebuffer_1(&mut self) {
        self.framebuffer.copy_from(&self.screen);
    }

    // = seg000:127c is_Gurney_Halleck_and_between_game_phases_15_and_20 — true
    // when `npc` is Gurney (4) and the story phase is in [0x15, 0x20): the
    // wounded Gurney, found lying in a sietch. The PALACE PLAN tally drops him
    // (seg000:196a), sal_draw_character draws the lying sprite (seg000:3d5b)
    // and his talking head gets no idle animator (seg000:9940).
    pub(crate) fn is_gurney_between_phases_15_and_20(&self, npc: u8) -> bool {
        // = seg000:127c cmp npc,4; jnz clc/ret.
        if npc != 4 {
            return false;
        }

        // = seg000:1280 cmp [game_phase],15h; jb; cmp [game_phase],20h; ret —
        //   carry (the caller's skip) iff 0x15 <= game_phase < 0x20.
        (PHASE_15_FIRST_VISION..PHASE_20_THUFIR_FOUND).contains(&self.game_phase)
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

    // = seg000:c551 draw_panel_outline
    pub(crate) fn draw_panel_outline(&mut self, panel: MapPanelRef) {
        let Some(panel) = self.map_panel_record(panel) else {
            return;
        };
        self.draw_rect_outline(
            panel.rect.x0,
            panel.rect.y0,
            panel.rect.x1 - 1,
            panel.rect.y1 - 1,
            panel.frame_color,
        );
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

        // = segvga:276c transition_tick. Draws this frame's ripple band into the screen
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{GameState, TaskId, dat_file::DatFile};

    // = seg000:da87 — a task that removes itself from inside its callback
    // shifts the walk back one slot, so the entry that moved into its slot
    // still runs this pass and the entry after it is neither skipped nor run
    // twice. tick_sky_fade with the fade disarmed is such a task; the map
    // caption task with a long interval is the marker: its accumulator shows
    // whether the walk reached it. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored frame_task_walk
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn frame_task_walk_survives_removals_from_inside_a_callback() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        game.remove_all_frame_tasks();
        game.sky_fade_active = false;
        game.add_frame_task(0, TaskId::SkyFade);
        game.add_frame_task(0, TaskId::SkyFade);
        game.add_frame_task(1000, TaskId::MapCaption);
        game.add_frame_task(0, TaskId::SkyFade);
        game.add_frame_task(1000, TaskId::MapCaption);
        // Five ticks elapsed since the last pass (a little more by the time
        // the walk reads the clock).
        game.last_task_tick = game.game_ticks().saturating_sub(5);
        game.process_frame_tasks();

        let left: Vec<(TaskId, u16)> = game
            .frame_tasks
            .iter()
            .map(|t| (t.task_id, t.accumulator))
            .collect();
        assert_eq!(left.len(), 2, "every self-removing task went: {left:?}");
        for (id, acc) in left {
            assert_eq!(id, TaskId::MapCaption);
            assert!(
                (5..100).contains(&acc),
                "the marker was visited once: {acc}"
            );
        }
        assert_eq!(game.frame_task_walk_next, None, "the dispatch is over");
    }
}
