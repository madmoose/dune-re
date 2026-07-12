//! Background-music control — the game-relative "jukebox" that picks and starts
//! songs from the current on-screen situation.
//!
//! This is a port of the contiguous seg000 music-control block
//! (`loc_0aa96`..`service_midi_music` at seg000:ae27). The original game does
//! not hard-code "play song X" calls during gameplay (those exist only for the
//! intro). Instead:
//!
//! 1. On every room-scene draw, [`GameState::update_room_music`] (= `loc_0ad5e`)
//!    classifies the current game state into a *situation index* via
//!    [`GameState::music_situation_index`] (= `loc_0aa96`), looks the index up
//!    in the song table at seg001:375c, and stores the chosen song in
//!    `music_desired_song` (= `data_0dbcc`).
//! 2. Every frame, [`GameState::service_midi_music`] (= seg000:ae04) starts
//!    `music_desired_song` whenever the driver is idle — so a song begins at
//!    game start and loops/advances as situations change.
//!
//! Only the game-relative mode (`music_playlist_flags == 0`, the default) is
//! ported; the CD-style playlist / shuffle modes (`loc_0ace6`, the order tables
//! at seg001:37fa/3804) are deferred.

use crate::GameState;

/// = seg001:375c — the game-relative song table, indexed by the situation index
/// from [`GameState::music_situation_index`]. The low 6 bits are the song number
/// (1-based, see [`crate::midi`] `song_name`); bit 0x80 means "switch to this
/// song immediately even if another is playing" (the situation changed), while
/// an entry without it simply queues the song to start when the current one ends.
/// An entry of 0 means "no music for this situation".
const SITUATION_SONG_TABLE: [u8; 14] = [
    0x82, 0x82, 0x01, 0x82, 0x84, 0x04, 0x85, 0x85, 0x87, 0x88, 0x86, 0x89, 0x83, 0x83,
];

impl GameState {
    // = seg000:aec6 loc_0aec6 — gate the music service on the MIDI-enabled
    // settings flag (loc_0ae28 = settings_flags bit 0x100). The DOS
    // cmd_args_memory bit-4 menu-interaction guard is not modelled.
    fn music_service_enabled(&self) -> bool {
        self.settings_flags & 0x100 != 0
    }

    // = seg000:aa96 loc_0aa96 — classify the current game state into a music
    // situation index 0..0x0d for the SITUATION_SONG_TABLE lookup. Earlier
    // checks win: special states and UI modes select fixed indices; otherwise
    // the index is derived from the location/room and the palace screen mode.
    fn music_situation_index(&self) -> u8 {
        // = aa98 cmp [data_04774],0; jnz — a special state overrides everything,
        // yielding index 0x0a only during game_phase 0x48.
        if self.is_dialogue_active {
            return if self.game_phase == 0x48 { 0x0a } else { 0 };
        }
        // = loc_0aaa7 — the normal cascade; each test that fires returns its index.
        if self.pending_room_screen_request != 0 {
            return 0x0d; // = aaa7
        }
        if self.data_0dd03 != 0 {
            return 1; // = aaaf
        }
        if (self.room_view_toggle as i8) < 0 {
            return 2; // = aab7 (map view, not room view)
        }
        if self.data_000c6 != 0 {
            return 3; // = aabf
        }
        if self.data_000ea > 0 {
            return 4; // = aac6 (signed compare)
        }
        // = aacd — index 5 base, refined by the scene below.
        let location_and_room = self.location_and_room;
        let room = (location_and_room & 0xff) as u8; // dl
        let location = (location_and_room >> 8) as u8; // dh
        let appearance = (self.location_appearance & 0xff) as u8; // bl

        // = aad5: appearance.lo == 0x80 && room != 1 takes the location-based
        // branch (loc_0aaef); everything else takes the palace/room branch.
        if appearance == 0x80 && room != 1 {
            // = loc_0aaef — desert/location music keyed on the location byte.
            if location >= 0x20 {
                // = loc_0ab08.
                if location != 0x20 {
                    return 0x0c;
                }
                // location == 0x20.
                if room != 3 {
                    return 0x0b;
                }
                0x0a // = loc_0ab12
            } else {
                // = aaf4: 8 when location < 7, else 9.
                let al = if location >= 7 { 9 } else { 8 };
                // = aafb: after game_phase 0x48 the late-game theme (0x0a) takes
                // over (appearance.lo 0x80 has bit 0 clear, so the shr path falls
                // through to loc_0ab12).
                if self.game_phase < 0x48 { al } else { 0x0a }
            }
        } else {
            // = loc_0aadf — palace/interior: pick by the active screen mode.
            match self.game_screen_mode_flags & 3 {
                0 => 5,
                1 => 6,
                _ => 7,
            }
        }
    }

    // = seg000:ad5e loc_0ad5e — game-relative background-music selector. Run
    // from the room-scene draw path (draw_room_game_screen): classify the
    // situation, look up its song, and record it as the desired song. Songs with
    // the table's 0x80 bit switch immediately when the situation's song differs
    // from the one playing; the rest just queue for when the current song ends.
    pub(crate) fn update_room_music(&mut self) {
        // = ad5e call loc_0aec6 — bail if music is disabled.
        if !self.music_service_enabled() {
            return;
        }
        // = ad63 call loc_0aa96.
        let index = self.music_situation_index();
        // = ad66 cmp music_playlist_flags,0; jz loc_0ad75 — game-relative mode.
        // CD-style mode (bit set) services its own playlist and is not ported.
        if self.music_playlist_flags != 0 {
            return;
        }
        // = loc_0ad75: bx = 375ch; xlat — the song for this situation.
        let entry = SITUATION_SONG_TABLE[index as usize];
        // = ad79 or al,al; jz — no music for this situation.
        if entry == 0 {
            return;
        }
        if entry & 0x80 == 0 {
            // = ad81: queue the song; service_midi_music starts it when the
            // driver next goes idle. (= ad84 MIDI_SetTickEnabled is implicit in
            // the port: the audio thread ticks whenever a song is playing.)
            self.music_desired_song = entry;
        } else {
            // = loc_0ad89: the situation forces a specific song.
            let song = entry & 0x3f;
            self.music_desired_song = song;
            // = ad8e cmp al,current_song_index; jnz loc_0adbe — switch now if it
            // differs from what is playing. (DOS fades the old song out over
            // 0x12c ticks; the port switches on the next service tick instead.)
            if Some(song) != self.midi.current_song() {
                self.music_force_restart = true;
            }
        }
    }

    // = seg000:ae04 service_midi_music — per-frame music scheduler. In
    // game-relative mode, start the desired song whenever the driver is idle (or
    // a forced switch is pending), so music begins at game start and loops as
    // the song ends. Called from ui_present_room_screen and the game loop.
    pub(crate) fn service_midi_music(&mut self) {
        // = ae04 call loc_0aec6 — bail if music is disabled / busy.
        if !self.music_service_enabled() {
            return;
        }
        // = ae09 test music_playlist_flags,1; jnz ret — CD-style mode services
        // its own playlist (loc_0ace6), so the game-relative path stands down.
        if self.music_playlist_flags & 1 != 0 {
            return;
        }
        // = ae10: advance only when the driver is idle (status sign clear), or
        // when a forced switch is pending (DOS: the status 0x40 "ready" bit).
        if self.midi.is_playing() && !self.music_force_restart {
            return;
        }
        self.music_force_restart = false;
        // = loc_0ad43: play the desired song (0 = nothing to play).
        if self.music_desired_song != 0 {
            let song = self.music_desired_song;
            self.midi.midi_play_song(song, &mut self.dat_file);
        }
    }
}
