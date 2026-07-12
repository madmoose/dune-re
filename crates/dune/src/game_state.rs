use crate::{
    CursorMode, CursorShapeId, DatFile, Equipment, Font, FontState, FrameBuffer, InputState,
    Location, Palette, Rect, SpriteSheet, TalkingHead,
    attack::AttackState,
    blit,
    frame_slot::FrameSink,
    game_ui::{MouseHandlers, ROOM_MOUSE_HANDLERS, UI_ELEMENTS_INIT, UiElement},
    gfx::{self, palette_flush},
    hnm::hnm_id_by_name,
    input::SharedInput,
    locations::LOCATIONS,
    midi::{self, Midi},
    mouse::SharedCursor,
    pcm_player::{self, PcmPlayer},
    room_game_screen::{CommandMenuRecord, ROOM_PERSON_TABLE_INIT, RoomPerson, ScreenElement},
    settings_ui::{SETTINGS_RECORDS_INIT, SettingsRecord},
    sprite::Sprite,
    sprite_bank::Banks,
    sprite_blitter,
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
}

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
}

pub(crate) struct FrameTask {
    interval: u16,
    accumulator: u16,
    task_id: TaskId,
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

    // ---- Host/runtime state and buffers (not seg001 data-segment globals) ----
    pub dat_file: DatFile,

    pub screen: FrameBuffer,
    pub screen_pal: Palette,

    // = segvga:01a3 fb_base_ofs — the game-area top. Stored here as a row; DOS
    // keeps the row*320 byte offset and applies it to every blit.
    pub y_offset: u16,
    pub framebuffer: FrameBuffer,
    // = _word_2D08E_framebuffer_saved_seg (fb2): a clean backup of the composed
    // scene; regions are restored from here under moving overlays. (The buffer
    // itself, not the seg001 selector word at seg001:dbde that points to it.)
    pub framebuffer_saved: FrameBuffer,
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

    pub(crate) game_start: std::time::Instant,
    pub(crate) frame_sink: Box<dyn FrameSink>,

    // Where the cursor sprite gets composited. `Baked` runs the DOS
    // `vga_draw_cursor` / `vga_restore_cursor` pair on the game thread;
    // `Overlay` skips that and lets the present thread draw the cursor
    // sprite on the GPU using the freshest pointer position.
    pub(crate) cursor_mode: CursorMode,
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
    // room scene to draw".
    pub(crate) data_00008: u8,

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

    // = seg001:0023 data_00023 — the room-transition / dialogue-scan state.
    // ui_click_move_room sets it to 1 to request the room-leave auto-dialogue scan
    // (run_room_leave_dialogue_scan gates on it and clears it), CONDIT condition 0x1c tests it == 1,
    // and the committed move sets it to 5.
    pub(crate) data_00023: u8,

    // = seg001:0025 number_of_sietches_visited — counts first visits to
    // locations with a code below 0x20 (the sietches)
    pub(crate) number_of_sietches_visited: u8,

    // = seg001:0026 entering_new_sietch — 0xff while the player's first in-room
    // move inside a freshly visited location is being committed
    pub(crate) entering_new_sietch: u8,

    // = seg001:002a _byte_1F4DA_game_phase — the global story-progress counter.
    pub(crate) game_phase: u8,

    // = seg001:002b night_attack_stage.
    pub(crate) night_attack_stage: u8,

    // = seg001:00c5 person_marker_base — random base offset for arranging the
    // people standing in a room. Set to rand() at room setup (the arrival
    // handler in tick_in_game_travel, seg000:4fc6), reset to 0 on scene change
    // (seg000:02a2). sal_position_markers reads its low nibble as the `base` in
    // preferred slot = (person_id + base) % count.
    pub(crate) person_marker_base: u8,

    // = seg001:00c6 data_000c6.
    pub(crate) data_000c6: u8,

    // = seg001:00c8 data_000c8 — the smuggler-present flag for the smuggler-room
    // command verbs (build_room_command_records, bl==0x80 && dl==8). Inits to 0.
    pub(crate) data_000c8: u8,

    // = seg001:00e8 _byte_1F598_ui_hud_head_index.
    pub(crate) ui_hud_head_index: u8,

    // = seg001:00ea data_000ea (signed).
    pub(crate) data_000ea: i8,

    // = seg001:00f4 desert_walk_counter — counts compass moves taken in the
    // desert.
    pub(crate) desert_walk_counter: u8,

    // = seg001:00fb data_000fb — toggle between the room/dialogue view and the
    // globe/map view (static init 0xff). ui_toggle_room_view negs it each call:
    // a non-negative result shows the room view, a negative one the map.
    pub(crate) room_view_toggle: u8,

    // = seg001:00fc data_000fc — a constant early-game flag (static
    // init 1, no DOS writers); CONDIT condition 1 (`byte ds:[fc]`) gates the
    // first greeting on it.
    pub(crate) data_000fc: u8,

    // = seg001:0100 locations.
    pub(crate) locations: [Location; 70],

    // = seg001:08aa troops.
    pub(crate) troops: [Troop; 68],

    // = seg001:1152 companion_1 / seg001:1153 companion_2 — the icon state of the
    // two bottom-left HUD companions portraits.
    pub(crate) companion_1: i16,
    pub(crate) companion_2: i16,

    // = seg001:11bc data_011bc — scene flag set (|= 1) by the night-attack
    // branch of draw_room_game_screen.
    pub(crate) data_011bc: u8,

    // = seg001:11c9 game_screen_mode_flags — bitfield selecting the active
    // non-room screen/mode (book/map/dialogue/...); 0 = the plain room view.
    // draw_room_game_screen branches on bits 0..1 (mask 3) and on ==0.
    pub(crate) game_screen_mode_flags: u8,

    // = seg001:11ca data_011ca — set during a pending room-screen swap (between
    // pending_room_screen_request being raised and loc_00d8e finishing the
    // transition); loc_04f0c bails when set so it does not race the swap.
    pub(crate) data_011ca: u8,

    // = seg001:11cb data_011cb — gates the second map-mode command verb
    // (build_room_command_records, the game_screen_mode_flags & 3 branch).
    // Static-inits to 0.
    pub(crate) data_011cb: u8,

    // = seg001:1ae4 _word_20F94_ui_elements — the in-game HUD element table.
    pub(crate) ui_elements: [UiElement; 24],

    // = seg001:1f0e command_menu_buf record list, flattened. Each entry is a
    // 4-byte [text_id, handler] verb record; build_room_command_records fills it
    // and redraw_active_command_menu paints it into ui_elements rows 7..11.
    pub(crate) command_menu_records: Vec<CommandMenuRecord>,

    // = seg001:1f80's text id field (menu_NPC_actions record 0) — the TALK TO
    // ME verb's text: 0x90 ('   >>>>  TALK TO ME  <<<<') while a voice line
    // plays, 0x9f ('" TALK TO ME "') once it stops. DOS patches the static menu
    // template in place (set_talk_to_me_verb_text, seg000:d621); the flattened
    // port keeps the template value here and the live copy in
    // command_menu_records.
    pub(crate) menu_npc_actions_talk_text_id: u16,

    // = seg001:21da screen_element_stack — the z-ordered active screen-element
    // stack.
    pub(crate) screen_element_stack: Vec<ScreenElement>,

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

    // = seg001:3810 music_playlist_flags — the jukebox mode. 0 = game-relative
    // (the song follows the on-screen situation, the default set at game init);
    // bit 0 = CD-style playlist, bit 1 = shuffle.
    pub(crate) music_playlist_flags: u8,

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
    // Port-only: a forced switch is pending (the situation's song-table entry
    // had bit 0x80 set and differs from the playing song). Mirrors the DOS
    // immediate-switch path (loc_0adbe) without the gradual fade.
    pub(crate) music_force_restart: bool,

    // Music-situation classifier inputs (= loc_0aa96).
    // = seg001:dd03 data_0dd03.
    pub(crate) data_0dd03: u8,

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

    // = seg001:46df data_046df — arms the loc_03916 sky-fade task (stage 29).
    // The task stops itself when this is cleared; set by intro_29_init.
    pub(crate) sky_fade_active: bool,

    // = seg001:46e0 data_046e0 — previous sky_fade_active state; draw_room_game_
    // screen xchg's it with the current flag to decide between a fade transition
    // and a plain palette+blit when the day/night state changed.
    pub(crate) data_046e0: u8,

    // = seg001:46eb data_046eb — selects the navigation panel template in
    // ui_setup_and_draw_nav_panel: nonzero picks the alternate (ornithopter/travel)
    // panel (1cca). Set by the travel/globe routines (e.g. seg000:4323/49a6),
    // cleared back to 0 for the plain room view.
    pub(crate) data_046eb: u8,

    // = seg001:46ff
    pub(crate) available_equipment: Equipment,

    // = seg001:4727 data_04727 — nonzero while an in-game travel sequence
    // (HNM-driven map flight) is active; loc_04f0c (the game_loop's per-pass
    // travel pump) returns immediately when this is 0. Cleared on travel
    // arrival (seg000:4fcb).
    pub(crate) data_04727: u8,

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

    // = seg001:47b6 dialogue_text_continuation_ptr (offset half of the far
    // pointer) — a pending multi-part subtitle-text continuation, armed at
    // seg000:89c8 by the subtitle text engine and cleared by
    // set_dialogue_speaker. While nonzero, menu_callback_choice_talk_to_me
    // re-presents the continuation (loc_094dd) and fire_event_callbacks skips
    // the event + spoken-mark (seg000:a042). The text engine is unported, so
    // this stays 0 in the port; the guards that read it are still modelled.
    pub(crate) dialogue_text_continuation_ptr: u16,

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
    // the active screen element and fires loc_0c868 on expiry.
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

    // = seg001:ce7a _word_2C32A_pit_timer_callback_counter — free-running PIT
    // ISR tick counter. draw_room_game_screen snapshots it into
    // game_clock_tick_base to time-stamp the room entry.
    pub(crate) pit_timer_callback_counter: u16,

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

    // = seg001:dc5a game_clock_tick_base — PIT-counter reference snapshot taken
    // when the room screen is presented; elapsed ticks are derived by
    // subtracting this base.
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
    // the render. See dismiss_stacked_overlays.
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
    ) -> Self {
        let mut dat_file = dat_file;
        let font = Font::new(&dat_file.read("DNCHAR.BIN").expect("load DNCHAR.BIN"));
        let command_bin = dat_file.read("COMMAND1.HSQ").expect("load COMMAND1.HSQ");
        let frame_tasks = Vec::<FrameTask>::with_capacity(20);
        let pcm_player = PcmPlayer::new(PCM_OUTPUT_RATE);
        let midi = midi::Midi::new();
        Self {
            headless: false,
            // ---- Host/runtime state and buffers ----
            dat_file,

            screen: FrameBuffer::new(320, 200),
            screen_pal: Palette::new(),

            y_offset: 24,
            framebuffer: FrameBuffer::new(320, 200),
            framebuffer_saved: FrameBuffer::new(320, 200),
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

            game_start: std::time::Instant::now(),
            frame_sink: Box::new(frame_sink),

            cursor_mode,
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
            bitfield_paul_events: 0,
            current_room: 0x0a,
            pending_destination_room: 0,
            previous_room: 0,
            persons_met: 0,
            persons_travelling_with: 0,
            persons_in_room: 0,
            persons_talking_to: 0,
            data_00023: 0,
            number_of_sietches_visited: 0,
            entering_new_sietch: 0,
            game_phase: 0,
            night_attack_stage: 0,
            person_marker_base: 0,
            data_000c6: 0,
            data_000c8: 0,
            ui_hud_head_index: 0,
            data_000ea: 0,
            desert_walk_counter: 0,
            room_view_toggle: 0xff,
            data_000fc: 1,
            locations: LOCATIONS,
            troops: TROOPS,
            companion_1: -1,
            companion_2: -1,
            data_011bc: 0,
            game_screen_mode_flags: 0,
            data_011ca: 0,
            data_011cb: 0,
            ui_elements: UI_ELEMENTS_INIT,
            command_menu_records: Vec::new(),
            menu_npc_actions_talk_text_id: 0x90,
            screen_element_stack: vec![ScreenElement::RoomCommandMenu],
            data_0227d: 1,
            sky_skydn_selector: 0,
            active_mouse_handlers: &ROOM_MOUSE_HANDLERS,
            cursor_image: None,
            banks: Banks::new(),
            game_suspend_count: 1,
            settings_drag_target: 0,
            voice_subtitle_mode: 0,
            voice_subtitle_mode_default: 0,
            settings_records: SETTINGS_RECORDS_INIT,
            cmd_args_memory: 0,
            hnm_bytes: None,
            music_playlist_flags: 0,
            settings_flags: 0x1 | 0x4 | 0x8 | 0x100 | 0x400 | 0x800,
            music_desired_song: 0,
            music_force_restart: false,
            data_0dd03: 0,
            current_sky_palette: 0,
            sky_fade_countdown: 0,
            pending_room_screen_request: 0,
            data_046db: 0,
            new_time_period_pending: 0,
            sky_fade_active: false,
            data_046e0: 0,
            data_046eb: 0,
            available_equipment: Equipment::default(),
            data_04727: 0,
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
            dialogue_line_word0: 0,
            dialogue_text_continuation_ptr: 0,
            dialogue_end_request: 0,
            dialogue_resume_entry_ptr: 0,
            dialogue_played_log: Vec::new(),
            npc_menu_idle_timer_base: 0,
            npc_menu_idle_timer_limit: 0,
            character_screen_pos: [(0xffff, 0xffff); 0x17],
            dialogue: Default::default(),
            condit: Default::default(),
            ui_hud_head_saved_strip: vec![0; 20 * 10],
            ui_hud_head_animating_down: false,
            pit_timer_callback_counter: 0,
            pause_enabled: 0,
            language_setting: 0,
            voc_bases: [0; 17],
            rand_seed: 1,
            rand_bits_seed: 1,
            screen_buffer: FbId::Screen,
            active_fb: FbId::Fb1,
            hnm_finished: false,
            hnm_frame_counter: 0,
            hnm_resource_data: 0,
            hnm_video_id: 0,
            hnm_active_video_id: 0,
            hnm_read_offset: 0,
            hnm_header_size: 0,
            hnm_body_offset: 0,
            hnm_framebuffer: FbId::Fb1,
            mouse_pos_x: 0,
            mouse_pos_y: 0,
            mouse_prev_drag_x: 0,
            mouse_prev_drag_y: 0,
            drag_armed_element: None,
            mouse_last_click_time: 0,
            data_0ceba: 0,
            mouse_draw_pos_x: 0,
            mouse_draw_pos_y: 0,
            cursor_hide_counter: 0,
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

    pub fn set_headless(&mut self) {
        self.headless = true;
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

        self.build_voc_base_table();
    }

    // = seg000:d815 game_loop — the in-game per-frame loop.
    pub(crate) fn exit_to_dos(&mut self) -> ! {
        // = seg000:004e/0052 call MIDI_Reset / pcm_vtable_reset — silence audio
        //   before the process exits so the device is released cleanly.
        self.midi.midi_reset();
        self.pcm_player.stop();
        // = the INT 21/4C return to DOS.
        std::process::exit(0);
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
            // pre-swap hooks. loc_0d7b7 (seg000:d7b7) hot-reloads the icones
            // sprite bank on the 4-PIT-tick edge; loc_01b0d (seg000:1b0d)
            // advances post-voice game state.
            if self.pending_room_screen_request == 0 {
                // TODO: port loc_0d7b7 (icones bank reload).

                // = seg000:1b0d loc_01b0d -> run_events_for_current_time_period
                // (seg000:1b23). DOS gates the call on is_voc_pcm_playing /
                // game_suspend_count / [2a] < 0xc8; the port checks
                // game_suspend_count (the rest is unported state). When the game
                // clock has flagged a new time period (new_time_period_pending),
                // consume the flag and refresh the date/time indicator.
                if self.new_time_period_pending != 0 && self.game_suspend_count == 0 {
                    // = seg000:1b2a mov byte ptr [46dd], 0 — consume the flag.
                    self.new_time_period_pending = 0;
                    // = seg000:1b40 call loc_01a0f — repaint the indicator.
                    self.ui_redraw_date_and_time_indicator();
                    // = seg000:1b43 call loc_038e1 — cross-fade the sky to the
                    // new time-of-day sub-palette if it changed.
                    self.loc_038e1_sky_refresh();
                    // TODO: port the rest of run_events_for_current_time_period
                    //   (the day-change hook loc_01c46, map_func_qq, and the
                    //   scheduled time-period events).
                }
            }

            // = seg000:d83e
            self.process_frame_tasks();

            // = seg000:ae04 service_midi_music.
            self.service_midi_music();

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

            // = seg000:d851 call loc_04f0c — the in-game travel pump. It
            //   early-returns unless data_04727 != 0 && data_011ca == 0, i.e.
            //   only while an HNM-driven map flight is active; the port has no
            //   travel state yet so this is a guarded no-op.
            self.tick_in_game_travel();

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
                        self.dialogue_advance_on_click();
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

    // = seg000:4f0c loc_04f0c — the in-game travel pump game_loop calls each
    // pass. Returns immediately unless an HNM map-flight is active
    // (data_04727 != 0) and no room swap is pending (data_011ca == 0). On the
    // active branch DOS drives hnm_do_frame, map_func/get_map_position, the
    // arrival-ornithopter blit and post-arrival cleanup; none of that is
    // ported yet, so the guarded no-op here matches the steady-state
    // behaviour (data_04727 stays 0 outside travel).
    fn tick_in_game_travel(&mut self) {
        // = seg000:4f0c cmp byte ptr [data_04727], 0; jz ret.
        // = seg000:4f13 cmp byte ptr [data_011ca], 0; jnz ret.
        if self.data_04727 != 0 && self.data_011ca == 0 {
            // TODO: port the active-travel branch (hnm_do_frame, map_func,
            //   loc_04b3b/4a1a/4a00, the ornithopter sprite blit, the arrival
            //   handler at seg000:4fc3+). The arrival handler seeds
            //   person_marker_base from rand (seg000:4fc3 call rand;
            //   seg000:4fc6 mov [person_marker_base], al).
        }
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

    // = seg000:e353 wait_processing_frame_tasks_interruptable — run the
    // driver for a fixed number of PIT ticks, breaking early on user input.
    // Used for `stage.wait` style timed pauses, which the player can skip.
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
        let video_id = hnm_id_by_name(name);
        self.hnm_last_frame_tick = self.game_ticks();
        self.hnm_y_offset = y_offset;
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

        // = ca80..ca8c: decode the next frame (into framebuffer_active = active_fb)
        // and advance. hnm_step_frame returns false if it stepped onto the
        // end-of-stream marker without decoding.
        if !self.hnm_step_frame() {
            return false;
        }

        palette_flush(self);

        self.hnm_queue_sd_block();

        true
    }

    // The buffer `id` resolves to. = dereferencing one of the segment globals.
    pub fn fb_mut(&mut self, id: FbId) -> &mut FrameBuffer {
        match id {
            FbId::Screen => &mut self.screen,
            FbId::Fb1 => &mut self.framebuffer,
            FbId::Saved => &mut self.framebuffer_saved,
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
    // area (_word_20920_game_area_rect = 0,0,0x140,0x98); without it the tall
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
            x1: 0x140,
            y1: 0x98 + yoff,
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
    // (_word_20920_game_area_rect = {0,0,0x140,0x98}, offset by fb_base_ofs) of
    // the active framebuffer to colour 0 (segvga vga_clear_rect). The rect spans
    // the full 320px width across rows fb_base_ofs..fb_base_ofs+0x98 (the in-game
    // viewport), so it is a contiguous row band. draw_SAL (loc_037b5) calls this
    // before drawing a room, so a scene's unpainted/dithered pixels show black
    // rather than the previous stage's leftover framebuffer.
    pub fn clear_game_area(&mut self) {
        let y0 = self.y_offset as usize;
        let fb = self.active_fb_mut();
        let w = fb.w() as usize;
        let h = fb.h() as usize;
        let y1 = (y0 + 0x98).min(h);
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

        self.frame_sink
            .publish(self.screen.clone(), self.screen_pal.clone());
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
    // blit) rather than routing through the generic Bresenham vga_draw_line; the
    // 16-bit line pattern (data_02772, solid here) and clip rect (data_0276a)
    // are not modelled.
    pub(crate) fn draw_rect_outline(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
        let yoff = self.y_offset as i16;
        let fb = self.active_fb_mut();
        let w = fb.w() as i16;
        let h = fb.h() as i16;
        let mut plot = |x: i16, y: i16| {
            let py = y + yoff;
            if (0..w).contains(&x) && (0..h).contains(&py) {
                fb.set(x as u16, py as u16, color);
            }
        };
        // = seg000:c569/c573 the top and bottom edges.
        for x in x0..=x1 {
            plot(x, y0);
            plot(x, y1);
        }
        // = seg000:c57d/c583 the left and right edges.
        for y in y0..=y1 {
            plot(x0, y);
            plot(x1, y);
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
            // = the remaining 11 vga_effect_dispatch effects are unported; this
            //   dispatcher only serves the PALACE PLAN's two reveal effects.
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
