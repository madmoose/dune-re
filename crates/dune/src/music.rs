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
//! Both jukebox modes are ported: the game-relative selector and the CD-style
//! playlist (`music_cd_playlist_service` = `loc_0ace6`, stepped from
//! `process_frame_tasks`), with the order tables at seg001:37fa/3804 and the
//! in-place shuffle (`music_cd_playlist_shuffle` = `loc_0acbf`).

use crate::{GameState, game_phase::PHASE_48_CHANI_MET};

/// = seg001:375c — the game-relative song table, indexed by the situation index
/// from [`GameState::music_situation_index`]. The low 6 bits are the song number
/// (1-based, see [`crate::midi`] `song_name`); bit 0x80 means "switch to this
/// song immediately even if another is playing" (the situation changed), while
/// an entry without it simply queues the song to start when the current one ends.
/// An entry of 0 means "no music for this situation".
const SITUATION_SONG_TABLE: [u8; 14] = [
    0x82, 0x82, 0x01, 0x82, 0x84, 0x04, 0x85, 0x85, 0x87, 0x88, 0x86, 0x89, 0x83, 0x83,
];

/// = seg001:3804 music_cd_standard_order — the pristine standard CD order
/// (9 song numbers + the 0xff terminator) STANDARD ORDER copies into the
/// working playlist. The working copy (seg001:37fa music_cd_playlist) is
/// initialised to the same bytes.
pub(crate) const MUSIC_CD_STANDARD_ORDER: [u8; 10] = [9, 6, 8, 1, 4, 3, 7, 5, 2, 0xff];

impl GameState {
    // = seg000:adbe midi_begin_song_fade_out — begin the switch to the queued
    // song: unless music is disabled, the CD playlist owns the driver, or a
    // dynamics ramp is already running (status bit 0x40), MIDI_SetDynamics(ax
    // = 0x12c ticks, bx = volume 0) fades the current song out.
    pub(crate) fn midi_begin_song_fade_out(&mut self) {
        // = seg000:adbe call check_music_enabled; jb ret.
        if !self.settings_music_enabled() {
            return;
        }
        // = seg000:adc3 test [music_playlist_flags],1; jnz ret.
        if self.music_playlist_flags & 1 != 0 {
            return;
        }
        // = seg000:adca test [midi_status],40h; jnz ret.
        if self.midi.is_fading() {
            return;
        }
        // = seg000:add2..addb MIDI_SetDynamics(12ch, 0).
        self.midi.set_ducking(0x12c, 0, 0);
    }

    // = seg000:aec6 check_music_enabled — gate the music service: disabled when
    // cmd_args_memory bit 4 (the MUSIC OFF menu toggle) is set or the MIDI
    // settings flag (loc_0ae28 = settings_flags bit 0x100) is clear. DOS
    // returns CF set when disabled.
    fn music_service_enabled(&self) -> bool {
        self.cmd_args_memory & 0x10 == 0 && self.settings_flags & 0x100 != 0
    }

    // = seg000:aa96 loc_0aa96 — classify the current game state into a music
    // situation index 0..0x0d for the SITUATION_SONG_TABLE lookup. Earlier
    // checks win: special states and UI modes select fixed indices; otherwise
    // the index is derived from the location/room and the palace screen mode.
    fn music_situation_index(&self) -> u8 {
        // = seg000:aa98 cmp [data_04774],0; jnz — a special state overrides everything,
        // yielding index 0x0a only during game_phase 0x48.
        if self.is_dialogue_active {
            return if self.game_phase == PHASE_48_CHANI_MET {
                0x0a
            } else {
                0
            };
        }
        // = seg000:aaa7 loc_0aaa7 — the normal cascade; each test that fires returns its index.
        if self.pending_room_screen_request != 0 {
            return 0x0d; // = aaa7
        }
        if self.globe_screen_active != 0 {
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
        // = seg000:aacd — index 5 base, refined by the scene below.
        let location_and_room = self.location_and_room;
        let room = (location_and_room & 0xff) as u8; // dl
        let location = (location_and_room >> 8) as u8; // dh
        let appearance = (self.location_appearance & 0xff) as u8; // bl

        // = seg000:aad5: appearance.lo == 0x80 && room != 1 takes the location-based
        // branch (loc_0aaef); everything else takes the palace/room branch.
        if appearance == 0x80 && room != 1 {
            // = seg000:aaef loc_0aaef — desert/location music keyed on the location byte.
            if location >= 0x20 {
                // = seg000:ab08 loc_0ab08.
                if location != 0x20 {
                    return 0x0c;
                }
                // location == 0x20.
                if room != 3 {
                    return 0x0b;
                }
                0x0a // = loc_0ab12
            } else {
                // = seg000:aaf4: 8 when location < 7, else 9.
                let al = if location >= 7 { 9 } else { 8 };
                // = seg000:aafb: after game_phase 0x48 the late-game theme (0x0a) takes
                // over (appearance.lo 0x80 has bit 0 clear, so the shr path falls
                // through to loc_0ab12).
                if self.game_phase < PHASE_48_CHANI_MET {
                    al
                } else {
                    0x0a
                }
            }
        } else {
            // = seg000:aadf loc_0aadf — palace/interior: pick by the active screen mode.
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
        // = seg000:ad5e call loc_0aec6 — bail if music is disabled.
        if !self.music_service_enabled() {
            return;
        }
        // = seg000:ad63 call loc_0aa96.
        let index = self.music_situation_index();
        // = seg000:ad66 cmp music_playlist_flags,0; jz loc_0ad75 — game-relative mode.
        // = seg000:ad6d..ad72 CD-style mode: when the driver is idle, advance the
        //   playlist right away (no end-of-song debounce on this path).
        if self.music_playlist_flags != 0 {
            if !self.midi.is_playing() {
                self.music_cd_playlist_advance();
            }
            return;
        }
        // = seg000:ad75 loc_0ad75: bx = 375ch; xlat — the song for this situation.
        let entry = SITUATION_SONG_TABLE[index as usize];
        // = seg000:ad79 or al,al; jz — no music for this situation.
        if entry == 0 {
            return;
        }
        if entry & 0x80 == 0 {
            // = seg000:ad81: queue the song; service_midi_music starts it when the
            // driver next goes idle. (= ad84 MIDI_SetTickEnabled is implicit in
            // the port: the audio thread ticks whenever a song is playing.)
            self.music_desired_song = entry;
        } else {
            // = seg000:ad89 loc_0ad89: the situation forces a specific song.
            let song = entry & 0x3f;
            self.music_desired_song = song;
            // = seg000:ad8e cmp al,current_song_index; jnz loc_0adbe — a song other
            // than the one playing begins the switch.
            if Some(song) != self.midi.current_song() {
                // = seg000:ad92 jmp midi_begin_song_fade_out — fade the
                // current song out rather than cutting over. The driver
                // raises status bit 0x40 for the ramp, so the next
                // service_midi_music call switches into the desired song
                // (seg000:ae17) — in DOS that is the first takeoff frame after
                // the travel confirm's disk-bound departure setup, giving the
                // audible music stop before the flight theme starts.
                self.midi_begin_song_fade_out();
            }
        }
    }

    // = seg000:ae04 service_midi_music — per-frame music scheduler. In
    // game-relative mode, start the desired song whenever the driver is idle (or
    // a forced switch is pending), so music begins at game start and loops as
    // the song ends. Called from ui_present_room_screen and the game loop.
    pub(crate) fn service_midi_music(&mut self) {
        // = seg000:ae04 call loc_0aec6 — bail if music is disabled / busy.
        if !self.music_service_enabled() {
            return;
        }
        // = seg000:ae09 test music_playlist_flags,1; jnz ret — CD-style mode services
        // its own playlist (loc_0ace6), so the game-relative path stands down.
        if self.music_playlist_flags & 1 != 0 {
            return;
        }
        // = seg000:ae10 cmp midi_status,0; jns loc_0ae1e — the driver is idle (the
        // song ended, or a forced-switch fade-out completed and silenced it);
        // = seg000:ae17 test midi_status,40h; jz ret — or a dynamics ramp is still
        // running, which DOS also takes as the go-ahead to switch. Every
        // dynamics ramp raises 0x40 (ADLSetDynamicsCurve, dnadl seg001:035e)
        // — narration ducks and their end-of-line restores included — which
        // is why this function runs only from its few DOS call sites and NOT
        // from the game loop: the per-frame pump is music_cd_playlist_
        // service's idle-only branch (loc_0ad37).
        if self.midi.is_playing() && !self.midi.is_fading() {
            return;
        }
        self.music_start_desired_song();
    }

    // = seg000:ad43 loc_0ad43 — start the desired song (0 = nothing to play),
    // shared by service_midi_music and the frame-task pump. DOS clears
    // current_song_index first so midi_play_song's same-song skip never blocks
    // a restart; the port calls the driver directly.
    fn music_start_desired_song(&mut self) {
        if self.music_desired_song != 0 {
            let song = self.music_desired_song;
            self.midi.midi_play_song(song, &mut self.dat_file);
            // = seg000:adb8/adba xor ax,ax; mov [music_song_end_tick_stamp],ax.
            self.music_song_end_tick_stamp = 0;
        }
    }

    // = seg000:ace6 music_cd_playlist_service — the per-frame music pump,
    // stepped from process_frame_tasks (seg000:d9d2) and the in-game HNM play
    // loop (seg000:c913): the CD-playlist streamer, or in game-relative mode
    // the idle-only desired-song start (loc_0ad37).
    pub(crate) fn music_cd_playlist_service(&mut self) {
        // = seg000:ace6 call is_voc_pcm_playing; jnz ret — stand down under a voice.
        if self.pcm_player.is_playing() {
            return;
        }
        // = seg000:aceb test music_playlist_flags,1; jz loc_0ad37 — with the CD mode
        //   off this is the game-relative pump: = ad37 check_music_enabled;
        //   = seg000:ad3c cmp midi_status,0; js ret — advance into the desired song
        //   only when the driver is FULLY idle. Deliberately no 0x40 test
        //   here: a dynamics ramp (a narration duck or its end-of-line
        //   restore) must not restart the playing song from this per-frame
        //   path; only service_midi_music's few call sites switch mid-ramp.
        if self.music_playlist_flags & 1 == 0 {
            if self.music_service_enabled() && !self.midi.is_playing() {
                self.music_start_desired_song();
            }
            return;
        }
        // = seg000:acf2 cmp [suppress_sky_240_255],0; jnz ret.
        if self.data_0227d != 0 {
            return;
        }
        // = seg000:acf9 cmp midi_status,0; js ret — a song is still playing.
        if self.midi.is_playing() {
            return;
        }
        // = seg000:ad00..ad0a stamp the first idle sighting (0 = unset).
        let now = self.game_ticks() as u16;
        if self.music_song_end_tick_stamp == 0 {
            self.music_song_end_tick_stamp = now;
        }
        // = seg000:ad0d..ad16 advance only 0xc8 ticks after the song ended.
        if now.wrapping_sub(self.music_song_end_tick_stamp) < 0xc8 {
            return;
        }
        // = seg000:ad18 falls into music_cd_playlist_advance.
        self.music_cd_playlist_advance();
    }

    // = seg000:ad18 music_cd_playlist_advance — play the next CD-playlist
    // entry: a high-bit terminator restarts the playlist, otherwise bump the
    // cursor and play the song.
    fn music_cd_playlist_advance(&mut self) {
        // = seg000:ad18 si = [music_cd_playlist_cursor]; lodsb; or al,al; js — the
        //   0xff terminator wraps to the top.
        let entry = self.music_cd_playlist[self.music_cd_playlist_cursor];
        if entry & 0x80 != 0 {
            self.music_cd_playlist_restart();
            return;
        }
        // = seg000:ad30 loc_0ad30 store the advanced cursor; jmp midi_play_song.
        self.music_cd_playlist_cursor += 1;
        self.midi_play_song_gated(entry);
    }

    // = seg000:ad21 music_cd_playlist_restart — restart the CD playlist from
    // the top: in shuffle mode first permute the entries, then play the first.
    pub(crate) fn music_cd_playlist_restart(&mut self) {
        // = seg000:ad21 si = music_cd_playlist; lodsb.
        // = seg000:ad25 test music_playlist_flags,2; jz loc_0ad30 — shuffle mode
        //   permutes in place and re-reads the first entry.
        if self.music_playlist_flags & 2 != 0 {
            self.music_cd_playlist_shuffle();
        }
        let entry = self.music_cd_playlist[0];
        // = seg000:ad30 loc_0ad30 store the cursor past entry 0; jmp midi_play_song.
        self.music_cd_playlist_cursor = 1;
        self.midi_play_song_gated(entry);
    }

    // = seg000:acbf music_cd_playlist_shuffle — shuffle the 9-entry playlist in
    // place: 0x12 rounds of swapping two rand_iterated(8) slots, perturbing the
    // rand seed with the PIT counter xor the loop counter between the draws.
    // The terminator at index 9 is out of rand_iterated(8)'s range.
    fn music_cd_playlist_shuffle(&mut self) {
        // = seg000:acc2 cx = 12h (DOS counts 0x12 down to 1).
        for cx in (1..=0x12u16).rev() {
            // = seg000:acc8 call rand_iterated (bx = 8); si = the first slot.
            let si = self.rand_iterated(8) as usize;
            // = seg000:accd..acd2 seed += pit_counter ^ cx — timer entropy between
            //   the two draws.
            let perturb = (self.game_ticks() as u16) ^ cx;
            self.rand_iterated_seed = self.rand_iterated_seed.wrapping_add(perturb);
            // = seg000:acd6 call rand_iterated; di = the second slot.
            let di = self.rand_iterated(8) as usize;
            // = seg000:acdb..acdf swap playlist[si], playlist[di].
            self.music_cd_playlist.swap(si, di);
        }
    }

    // = seg000:ad95 midi_play_song — the gated play wrapper the CD paths jump
    // to: bail when music is disabled (= ad97 check_music_enabled) or the song
    // is already the current one (= ad9c), else load + start it (the port's
    // driver-side midi_play_song) and clear the end-of-song stamp (= adba).
    // DOS also passes music_playlist_flags & 1 to MIDI_Open as its al flag
    // (seg000:adac); the port's driver always plays once and both modes
    // reschedule externally (service_midi_music / the CD service).
    pub(crate) fn midi_play_song_gated(&mut self, song: u8) {
        if !self.music_service_enabled() {
            return;
        }
        if self.midi.current_song() == Some(song) {
            return;
        }
        self.midi.midi_play_song(song, &mut self.dat_file);
        self.music_song_end_tick_stamp = 0;
    }

    // = seg000:ad50 play_music_WORMSUIT_HSQ — reset the driver (ad50) and start
    // song 3 through the gated midi_play_song (ad55 jmp), so the MUSIC OFF verb
    // and a missing MIDI card silence the score exactly like every other song
    // start. The in-game callers — the book's credits page (loc_00a09) and the
    // desert collapse (loc_00e77) — come through here.
    //
    // The intro's two calls (seg000:0228 / 0321) drive `Midi` directly instead:
    // the port models "no MIDI card" by muting the output while the song keeps
    // running, so the intro's music-synced waits still pace, and the MUSIC OFF
    // verb they would otherwise read cannot be set before the game starts.
    pub(crate) fn play_music_wormsuit_hsq(&mut self) {
        // = seg000:ad50 call midi_reset — ungated in DOS.
        self.midi.midi_reset();
        // = seg000:ad53/ad55 al = 3; jmp midi_play_song.
        self.midi_play_song_gated(3);
    }
}
