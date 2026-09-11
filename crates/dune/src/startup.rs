use rand::Rng;

use crate::{game_state::GameState, locations, tablat::Tablat};

impl GameState {
    // = seg000:0000 start - plays the intro and credits, sets up the in-game
    // UI, enters the room view (ui_enter_room_view), starts the game clock
    // (reset_game_suspend) and runs game_loop.
    pub fn start(&mut self, skip_intro: bool) {
        // = seg000:0006 call initialize_system.
        self.initialize_system();

        // = seg000:0009 call init_resources
        self.init_resources();

        // ESC anywhere in the intro skips straight into the game; a non-ESC key
        // or the mouse only ends the current phase. The flag threads through the
        // three calls (= the DOS ZF(esc) chained via each function's jz-at-entry).
        self.intro_skip_to_game = false;

        // = seg000:000d call play_intro_cd.
        self.play_intro_cd(skip_intro);

        // = seg000:0010 call play_credits. Skipped when the intro was ended
        // with ESC (seg000:0309 jz loc_00331).
        self.play_credits(skip_intro || self.intro_skip_to_game);

        // = seg000:0013 call play_intro_floppy.
        self.play_intro_floppy(skip_intro || self.intro_skip_to_game);

        // = seg000:0016
        self.midi.midi_reset();

        // = seg000:0019 mov [music_playlist_flags], 0
        self.music_playlist_flags = 0;
        // Port-only: the `--music` selection set_music_mode held back, landed
        // now that the reset above is out of the way. Nothing pending (every
        // caller but the CLI) leaves the music state untouched.
        self.apply_pending_music_mode();

        // = seg000:001e mov [game_time], 2.
        self.game_time = 2;

        // = seg000:0024 call init_game_ui.
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

    // = seg000:003a exit_to_dos — leave the game: the mouse reset, the memory
    // driver, the MIDI and PCM resets, the text mode and the DOS return; the
    // port silences the audio and exits the process.
    pub(crate) fn exit_to_dos(&mut self) -> ! {
        // Finalise any in-progress recording first: `std::process::exit` below
        // skips every destructor, so this is the only chance to mux the clip
        // when the player quits through the in-game EXIT GAME menu.
        self.recorder.stop();

        // = seg000:004e/0052 call MIDI_Reset / pcm_vtable_reset.
        self.midi.midi_reset();
        self.pcm_player.stop();

        // = the INT 21/4C return to DOS.
        std::process::exit(0);
    }

    // = seg000:0083 init_game_ui — configure the voice/subtitle language, then
    // draw the in-game HUD (falls through into draw_game_ui at seg000:0086).
    // Called once from start (seg000:0024) before the game loop.
    pub fn init_game_ui(&mut self) {
        // = seg000:0083 call check_amr_or_eng_language.
        self.check_amr_or_eng_language();
        // = seg000:0086 fall through into draw_game_ui.
        self.draw_game_ui();
    }

    // = seg000:0086 draw_game_ui — clear fb1, draw every HUD element offscreen,
    // then overlay the character head-and-shoulders portrait. Also entered
    // standalone from seg000:3768 to redraw the HUD.
    pub fn draw_game_ui(&mut self) {
        // = seg000:0086 set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = seg000:0089 gfx_clear_active_framebuffer.
        self.gfx_clear_active_framebuffer();
        // = seg000:008c
        self.gfx_call_bp_with_front_buffer_as_screen(|s| s.draw_all_ui_elements());
        // = seg000:0095 jmp ui_hud_head_draw.
        self.ui_hud_head_draw();
    }

    // = seg000:0098 adjust_sub_resource_pointers [not needed]

    // = seg000:00b0 init_resources
    pub fn init_resources(&mut self) {
        self.load_resources();

        // = seg000:57ec/5481 open_resource_by_index(0x3a) — the MAP2.HSQ
        // spice layer the density overlay renders (DOS loads it on demand and
        // swaps res_map_seg to it; the port keeps it alongside the terrain).
        self.map2 = self.dat_file.read("MAP2.HSQ").expect("load MAP2.HSQ");

        // = seg000:00b3
        self.init_locations_and_troops();

        // = seg000:00b6 call clear_frame_tasks.
        self.clear_frame_tasks();

        // = seg000:00b9/00bc — run the game-phase trigger record twice. Each
        // walk presents the first condition-matching unspoken entry of DIALOGUE
        // slot 135 (records 0x456..) silently (subtitles suppressed,
        // pseudo-speaker 0x10 skips the talking head) and appends it to the
        // dialogue-played log, so a new game's BOOK opens with two pages — the
        // "On Dune, the desert covers the entire planet." and "Paul Atreides
        // arrived on Dune with his father, ..." narrations, both carrying a
        // book video (HNM 0x19/0x1a via book_video_page_words[0..2]).
        self.run_game_phase_triggers();
        self.run_game_phase_triggers();

        // = seg000:00bf
        let v = rand::random::<u16>();
        self.rand_seed = v;
        self.rand_bits_seed = v;
        self.rand_iterated_seed = v;
    }

    // = seg000:00d1 load_resources
    pub fn load_resources(&mut self) {
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

        self.build_voc_base_table();
    }

    // = seg000:0169..01c6 map2_resource_func (minus the troop placement pass,
    // init_troop_locations): build a 256-entry histogram of the MAP2 spice
    // layer's bytes, each count seeded with 7 (seg000:0175..018d), then for
    // every location: snap its map_x to its map cell, cache the map byte
    // offset (Location.map_offset, seg000:019e), mark the cell as holding a
    // location (map byte |= 0x40, seg000:01a1), read the MAP2 byte at that
    // offset into spice_field_id (seg000:01a5..01ac) and set spice_amount =
    // histogram[field] >> 4 (seg000:01af..01bd).
    pub(crate) fn init_locations_and_troops(&mut self) {
        // = seg000:0175..018d the seeded MAP2 histogram (data_0c5f9 bytes,
        //   the map length).
        let mut histogram = [7u16; 256];
        for &cell in self.map2.iter().take(self.map.len()) {
            histogram[cell as usize] += 1;
        }
        for i in 0..self.locations.len() {
            let (map_x, map_y) = (self.locations[i].map_x, self.locations[i].map_y);
            let (offset, snapped_x) = self.map_offset_and_snap_x(map_x as u16, map_y);
            self.locations[i].map_x = snapped_x as i16;
            self.locations[i].map_offset = offset as u16;
            self.map[offset] |= 0x40;
            // = seg000:01a5..01bd the location's spice field: the MAP2 byte
            //   at its cell, and the field's map coverage as the amount.
            let field = self.map2[offset];
            self.locations[i].spice_field_id = field;
            self.locations[i].spice_amount = (histogram[field as usize] >> 4) as u8;
        }

        // = seg000:01c8..01df the startup troop placement pass at the end of
        // map2_resource_func: for every location, call troop_set_location_info on
        // each troop chained to it (call_callback_on_all_troops_in_location with
        // bp = the callback, dx/bx = the location's map cell).
        for loc_index in 0..self.locations.len() {
            // = seg000:01da the 0xff terminator entry ends the walk.
            if self.locations[loc_index].first_name == 0xff {
                break;
            }
            self.for_each_troop_in_location(loc_index, |game, ti| {
                game.troop_init_location(ti, loc_index);
            });
        }
    }

    // = seg000:01e0 troop_set_location_info — link one troop to a location:
    // offset_of_location = the location's seg001 pointer, the gps coordinates
    // = the location's map cell, and the dissatisfaction_and_speech low byte
    // = the voice-bank id: (first_name & 0xf) | (dissat & 0x70), with bit 7
    // (the southern Fremen bank) toggled once per threshold first_name passes
    // (> 3, > 5, > 9), so it ends up set for first_name 4..5 and >= 10.
    fn troop_init_location(&mut self, ti: usize, loc_index: usize) {
        let loc = self.locations[loc_index];
        let t = &mut self.troops[ti];
        // = seg000:01e0..01e6 the location pointer and the map cell.
        t.offset_of_location = locations::location_ptr(loc_index as u16);
        t.gps_coordinates_1 = loc.map_x as u16;
        t.gps_coordinates_2 = loc.map_y as u16;
        // = seg000:01e9..0208 the voice-bank low byte.
        let mut bank = loc.first_name & 0x0f;
        let mut high = (t.dissatisfaction_and_speech as u8) & 0x70;
        if loc.first_name > 3 {
            high ^= 0x80;
        }
        if loc.first_name > 5 {
            high ^= 0x80;
        }
        if loc.first_name > 9 {
            high ^= 0x80;
        }
        bank |= high;
        t.dissatisfaction_and_speech = (t.dissatisfaction_and_speech & 0xff00) | bank as u16;
    }
}
