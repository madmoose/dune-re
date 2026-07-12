use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicU16, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{
    MIDI_SAMPLE_RATE,
    dat_file::DatFile,
    herad::HeradADL,
    pcm_player::{balance_to_gains, supported_output_rate},
};

const QUEUE_MAX: usize = 8192;

pub struct Midi {
    cmd_tx: mpsc::Sender<MidiCommand>,
    shared: Arc<MidiShared>,
    audio_thread: Option<thread::JoinHandle<()>>,
    initialized: bool,
    // Mirrors [0dbcbh]: the index of the currently loaded song, or None.
    current_song: Option<u8>,
}

enum MidiCommand {
    PlaySong {
        data: Box<[u8]>,
        play_count: u8,
    },
    Reset,
    Stop,
    SetVolume(u8),
    SetBalance(u8),
    SetDucking {
        ramp_ticks: u16,
        volume: u8,
        _balance: u8,
    },
}

struct MidiShared {
    status: AtomicU8,
    measure: AtomicU16,
    tick: AtomicU16,
}

impl Midi {
    pub fn new() -> Self {
        let shared = Arc::new(MidiShared {
            status: AtomicU8::new(0),
            measure: AtomicU16::new(0),
            tick: AtomicU16::new(0x60),
        });
        let (cmd_tx, cmd_rx) = mpsc::channel::<MidiCommand>();
        let shared_audio = Arc::clone(&shared);
        let audio_thread = thread::spawn(move || {
            audio_thread_main(cmd_rx, shared_audio);
        });

        Self {
            cmd_tx,
            shared,
            audio_thread: Some(audio_thread),
            initialized: false,
            current_song: None,
        }
    }

    // = seg000:aeb7 midi_reset — stop the current song.
    // Clears the current-song index, silences all OPL voices in the driver, and
    // sets [_byte_2D07D_midi_status] to 0 (which disables subsequent midi_wait_until
    // calls until a new song is started).
    pub fn midi_reset(&mut self) {
        self.current_song = None;
        self.initialized = false;
        let _ = self.cmd_tx.send(MidiCommand::Reset);
    }

    // = the MIDI_SetVolume vtable entry (seg001:3985) — set the master music
    // volume on the driver. The mixer panel's music-slider apply hook
    // (loc_0a650) drives this with the slider value (clamped >= 4).
    pub fn set_music_volume(&self, volume: u8) {
        let _ = self.cmd_tx.send(MidiCommand::SetVolume(volume));
    }

    // = the `ah` (balance/pan) half of the MIDI_SetVolume call (loc_0a660) — the
    // byte the mixer panel's MUSIC balance knob (seg001:28ae) supplies. The DOS
    // AdLib driver discarded it; the port applies it as a per-channel gain on the
    // OPL3 stereo mix.
    pub fn set_balance(&self, balance: u8) {
        let _ = self.cmd_tx.send(MidiCommand::SetBalance(balance));
    }

    // = the MIDI_SetDynamics vtable entry — set the music-ducking ramp: glide
    // the music to `volume` over `ramp_ticks`. Drives the ducking pair
    // midi_duck_music_volume (seg000:ade0) / midi_restore_music_volume
    // (seg000:aded): the score dips under voice lines and swells back after.
    // `volume` packs the level in the low byte and its paired mixer record in
    // the high byte (the volume_to_attenuation `(ah, al)` split).
    pub fn set_ducking(&self, ramp_ticks: u16, volume: u8, _balance: u8) {
        let _ = self.cmd_tx.send(MidiCommand::SetDucking {
            ramp_ticks,
            volume,
            _balance,
        });
    }

    // = seg000:de0c midi_wait_until — block until current song position >= target.
    // Returns false on user-input interrupt (currently always true; ESC plumbing TODO).
    // Position formula matches midi_wait_until at seg000:de20:
    //   (_word_2D07E_midi_measure << 4) | (((0x60 - _word_2D080_midi_ticks_remaining) / 6) & 0xf)
    pub fn midi_wait_until(&self, target: u16) -> bool {
        if !self.initialized || target == 0 {
            return true;
        }

        loop {
            let status = self.shared.status.load(Ordering::Relaxed);
            let measure = self.shared.measure.load(Ordering::Relaxed);
            let tick = self.shared.tick.load(Ordering::Relaxed);

            let delta = 0x60u16.wrapping_sub(tick) / 6;
            let pos = (measure << 4) | (delta & 0xf);

            if target <= pos {
                return true;
            }
            if (status & 0x80) == 0 {
                // Playback has stopped — don't block forever.
                return true;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    // = seg000:ad57 play_music_MORNING_HSQ.
    pub fn play_music_morning_hsq(&mut self, dat: &mut DatFile) {
        self.midi_reset();
        self.midi_play_song(6, dat);
    }

    // = seg000:ad50 play_music_WORMSUIT_HSQ.
    pub fn play_music_wormsuit_hsq(&mut self, dat: &mut DatFile) {
        self.midi_reset();
        self.midi_play_song(3, dat);
    }

    // = seg000:ad95 play_music — load song and start it.
    pub fn midi_play_song(&mut self, song_index: u8, dat: &mut DatFile) {
        let data = self.midi_load_song(song_index, dat);
        let _ = self.cmd_tx.send(MidiCommand::PlaySong {
            data,
            play_count: 1,
        });
        // Mirrors `mov [_byte_2D07B_current_song_index], al` at seg000:ada5.
        self.current_song = Some(song_index);
        // The driver's ADLOpen returns al = 0x80 (sign bit set) on success
        // (`mov [_byte_2D07D_midi_status], al`, seg000:adb5). Publish it now,
        // synchronously, so is_playing() reads true the instant after a play is
        // issued — the audio thread also sets it once it picks up the command,
        // but the synchronous store keeps service_midi_music from re-triggering
        // the same song in the window before the thread runs.
        self.shared.status.store(0x80, Ordering::Relaxed);
        // The driver's ADLOpen return is also what gates midi_wait_until at
        // seg000:de0c.
        self.initialized = true;
    }

    // Mirrors the `cmp byte ptr [_byte_2D07B_current_song_index], 0` check used by play loops
    // (e.g. play_VIRGIN_HNM at seg000:063c) to decide whether to drive the midi driver.
    pub fn is_song_loaded(&self) -> bool {
        self.current_song.is_some()
    }

    // = the `_byte_2D07B_current_song_index` value (seg001:dbcb): the song the
    // driver currently has loaded, or None. Read by the game-relative selector
    // (update_room_music) to decide whether a situation change needs a switch.
    pub fn current_song(&self) -> Option<u8> {
        self.current_song
    }

    // True while a song is playing — the sign bit of the driver status byte
    // (`_byte_2D07D_midi_status`, seg001:dbcd). service_midi_music starts the
    // next game-relative song once this clears (the song ended or was reset).
    pub fn is_playing(&self) -> bool {
        (self.shared.status.load(Ordering::Relaxed) & 0x80) != 0
    }

    // Mirrors [_word_2D07E_midi_measure]: the current measure of the playing song.
    pub fn current_measure(&self) -> u16 {
        self.shared.measure.load(Ordering::Relaxed)
    }

    // = seg000:ae62 load_music — return song bytes for index N.
    pub fn midi_load_song(&mut self, song_index: u8, dat: &mut DatFile) -> Box<[u8]> {
        let name = song_name(song_index);
        dat.read(name).expect("song load")
    }
}

impl Default for Midi {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Midi {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(MidiCommand::Stop);
        if let Some(handle) = self.audio_thread.take() {
            let _ = handle.join();
        }
    }
}

fn song_name(idx: u8) -> &'static str {
    // Maps song index → resource name. Original game looks these up at table offset 0xA4 + idx.
    let song_names = [
        /*  1 */ "SEKENCE.HSQ",
        /*  2 */ "WATER.HSQ",
        /*  3 */ "WORMSUIT.HSQ",
        /*  4 */ "WORMINTR.HSQ",
        /*  5 */ "WARSONG.HSQ",
        /*  6 */ "MORNING.HSQ",
        /*  7 */ "SIETCHM.HSQ",
        /*  8 */ "BAGDAD.HSQ",
        /*  9 */ "ARRAKIS.HSQ",
        /* 10 */ "CRYOMUS.HSQ",
    ];

    song_names
        .get(idx as usize - 1)
        .unwrap_or_else(|| panic!("unknown song index {idx}"))
}

fn audio_thread_main(cmd_rx: mpsc::Receiver<MidiCommand>, shared: Arc<MidiShared>) {
    let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    let (_stream, sample_rate) = start_audio_stream(Arc::clone(&queue), MIDI_SAMPLE_RATE, 2);

    // Nuked-OPL3 synthesizes at the chip-native 49716 Hz and resamples to the
    // rate it is created with, so the stream's actual device rate keeps the
    // pitch correct even when 49716 Hz itself is unsupported.
    let opl3 = Rc::new(RefCell::new(opl3_rs::Opl3Device::new(sample_rate)));
    let device = Opl3DeviceWrapper {
        inner: Rc::clone(&opl3),
    };
    let mut player = HeradADL::new(Box::new(device));
    player.init();

    let mut song_data: Box<[u8]> = Box::new([]);
    let mut status: u8 = 0;
    // Mixer balance/pan byte; center (no pan) until the panel sets it.
    let mut balance: u8 = 120;
    let mut sample_buf: Vec<i16> = vec![0; 32768];
    let mut out_buf: Vec<f32> = vec![0.0; 32768];

    loop {
        loop {
            match cmd_rx.try_recv() {
                Ok(MidiCommand::PlaySong { data, play_count }) => {
                    song_data = data;
                    player.reset();
                    player.open(&song_data, play_count);
                    status = 0x80;
                    shared.status.store(status, Ordering::Relaxed);
                    shared.measure.store(1, Ordering::Relaxed);
                    shared.tick.store(0x60, Ordering::Relaxed);
                }
                Ok(MidiCommand::Reset) => {
                    // = DNADL's ADLReset (seg001:02f3): silence all OPL voices and clear
                    // the playing-status byte. The song bytes can stay around — nothing
                    // ticks them until a new PlaySong arrives.
                    player.reset();
                    status = 0;
                    shared.status.store(0, Ordering::Relaxed);
                }
                Ok(MidiCommand::SetVolume(vol)) => {
                    // = the MIDI_SetVolume vtable entry (seg001:3985) the mixer
                    // panel's music-slider apply hook (loc_0a650) drives: set the
                    // driver's master music volume.
                    player.set_volume(vol);
                }
                Ok(MidiCommand::SetBalance(b)) => {
                    // The mixer panel's MUSIC balance knob (loc_0a660 `ah`): pan
                    // the OPL3 mix. Applied below when the samples are mixed out.
                    balance = b;
                }
                Ok(MidiCommand::SetDucking {
                    ramp_ticks,
                    volume,
                    _balance, // TODO: Implement MIDI balance
                }) => {
                    // = the MIDI_SetDynamics vtable entry the music-ducking pair
                    // (midi_duck_music_volume / midi_restore_music_volume) drives:
                    // ramp the music to `volume` over `ramp_ticks`.
                    player.set_music_during_voices(ramp_ticks, volume);
                }
                Ok(MidiCommand::Stop) => return,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }

        // Backpressure: don't outrun the audio device.
        if queue.lock().unwrap().len() >= QUEUE_MAX {
            thread::sleep(Duration::from_millis(1));
            continue;
        }

        let n_samples = opl3.borrow_mut().run(5000.0);
        let n_total_samples = 2 * n_samples;
        opl3.borrow_mut()
            .generate_samples(&mut sample_buf[0..n_total_samples])
            .unwrap();

        // OPL3 emits interleaved stereo (even = left, odd = right); scale each
        // channel by the balance gain as it is converted to f32.
        let (left_gain, right_gain) = balance_to_gains(balance);
        for i in 0..n_total_samples {
            let gain = if i % 2 == 0 { left_gain } else { right_gain };
            out_buf[i] = (sample_buf[i] as f32 / i16::MAX as f32) * gain;
        }

        {
            let mut q = queue.lock().unwrap();
            q.extend(&out_buf[0..n_total_samples]);
        }

        if (status & 0x80) != 0 {
            let (new_status, measure, tick) = player.tick_handler(&song_data);
            status = new_status;
            shared.status.store(new_status, Ordering::Relaxed);
            shared.measure.store(measure, Ordering::Relaxed);
            shared.tick.store(tick, Ordering::Relaxed);
        }
    }
}

struct Opl3DeviceWrapper {
    inner: Rc<RefCell<opl3_rs::Opl3Device>>,
}

impl crate::herad::dnadl::Device for Opl3DeviceWrapper {
    fn write_opl(&mut self, reg: u16, val: u8) {
        let bank = || {
            if reg >= 0x100 {
                opl3_rs::OplRegisterFile::Secondary
            } else {
                opl3_rs::OplRegisterFile::Primary
            }
        };
        let reg_byte = (reg & 0xff) as u8;
        let mut inner = self.inner.borrow_mut();
        inner.write_address(reg_byte, bank()).unwrap();
        inner.write_data(val, bank(), true).unwrap();
    }
}

// Returns the stream and the sample rate it actually opened at (the preferred
// rate when supported, the device default otherwise).
fn start_audio_stream(
    queue: Arc<Mutex<VecDeque<f32>>>,
    sample_rate: u32,
    channels: u16,
) -> (cpal::Stream, u32) {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No audio output device found");
    let sample_rate = supported_output_rate(&device, sample_rate);
    let config = cpal::StreamConfig {
        channels,
        sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _| {
                let mut q = queue.lock().unwrap();
                for out in data.iter_mut() {
                    *out = q.pop_front().unwrap_or(0.0);
                }
            },
            |err| eprintln!("Audio stream error: {err}"),
            None,
        )
        .expect("Failed to build audio stream");
    stream.play().unwrap();
    (stream, sample_rate)
}
