//! Sound Blaster .VOC playback driver, reimplemented over CPAL.
//!
//! This is a port of DNSDB.BIN (chani project `cryo-dune-3.7-cd-dnsdb`,
//! segment `seg001`). The DOS driver feeds the Sound Blaster a Creative Voice
//! File one block at a time through DMA, the DMA-done IRQ (`playback_isr`,
//! seg001:053a) re-entering the block engine (`voc_read_loop`, seg001:0608) to
//! program the next chunk. This port keeps that same block engine and the same
//! playback-state machine, but replaces the SB hardware path with a CPAL
//! output stream: the audio callback pulls the 8-bit PCM the block engine
//! produces and resamples it to the device rate.
//!
//! State machine (mirrors `playing_flag`/`idle_flag`/`end_of_stream_flag`):
//!
//! ```text
//!            start                  stop
//!   IDLE ----------> PLAYING <--------------- (any)
//!    ^   <--------- /  | ^ pause/resume
//!    |    stop/end-at  | |
//!    |                 v |
//!    |              PAUSED
//!    |   plain end      |
//!    +-- ENDED <--------+   (queue_next auto-starts only from ENDED)
//! ```
//!
//! Public methods mirror the driver vtable entries:
//! [`PcmPlayer::start_playback`] (seg001:0106)
//! [`PcmPlayer::stop`]/[`PcmPlayer::reset`] (seg001:0109/0103)
//! [`PcmPlayer::pause`]/[`PcmPlayer::resume`] (dispatch cmd 0xA/0xB)
//! [`PcmPlayer::end_loop`]/[`PcmPlayer::break_loop`] (seg001:010c/010f)
//! [`PcmPlayer::queue_next`] (seg001:0112) and the host-readable marker (seg001:022c).

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// `+7` loop flag bit 6: replay the whole job from its first block at the
/// terminator (`voc_blk0_terminator`, seg001:0697).
pub const VOC_LOOP_WHOLE: u8 = 0x40;
/// `+7` loop flag bit 7: stop and go idle at the terminator instead of
/// chaining or looping (`voc_blk0_terminator`, seg001:0686 -> 06dc).
pub const VOC_STOP_AT_END: u8 = 0x80;

/// Bytes of the 26-byte "Creative Voice File\x1a" header; the DOS job
/// descriptor's `+0` pointer is positioned just past it, at the first block.
const VOC_HEADER_LEN: usize = 0x1a;

/// Safety cap on non-data blocks processed in one [`Engine::advance`] call, so
/// a malformed VOC that loops with no audio cannot hang the audio thread. The
/// DOS engine has no such guard but real content always reaches a data block.
const MAX_BLOCKS_PER_ADVANCE: u32 = 4096;

/// A VOC playback job. Mirrors the DOS job descriptor handed to
/// `dnsdb_start_playback`/`dnsdb_queue_next` in `ds:si`: `+0` is a far pointer
/// to the VOC data at the first block and `+7` holds the loop flags. The `+6`
/// state byte and `+4` length used by the SB-side handshake are folded into
/// the engine's `current`/`queued` slots here.
struct VocJob {
    /// VOC bytes positioned at the first block (header skipped).
    data: Arc<[u8]>,
    /// [`VOC_LOOP_WHOLE`] | [`VOC_STOP_AT_END`].
    loop_flags: u8,
}

#[derive(Debug)]
struct RepeatLevel {
    /// Remaining repeats. `0xFF` means infinite.
    count: u16,
    /// Cursor of the first block after the repeat-start block.
    loopstart: usize,
}

/// The block of PCM data currently being drained by the audio callback,
/// at its own sample rate.
struct ActiveSegment {
    /// Decoded 8-bit unsigned PCM (codec 0 = passthrough).
    pcm: Vec<u8>,
    /// Sample rate of this block, from its time constant.
    rate: u32,
    /// Fractional read position, in input samples (resampling phase).
    phase: f32,
}

impl ActiveSegment {
    fn new(pcm: Vec<u8>, rate: u32) -> Self {
        Self {
            pcm,
            rate,
            phase: 0.0,
        }
    }
}

/// Outcome of the terminator handler (`voc_blk0_terminator`, seg001:0676).
enum TermAction {
    /// Cursor was repointed (chain to queued / loop whole job); keep reading.
    Continue,
    /// Stream finished; stop the engine.
    Stop,
}

struct Engine {
    /// = `cur_job_ptr` (seg001:0118): the playing job, walked by `cursor`.
    current: Option<VocJob>,
    /// = `queued_job_ptr` (seg001:011c): next job for gapless chaining.
    queued: Option<VocJob>,
    /// = `voc_cursor` (seg001:023c): byte offset of the next block to read.
    cursor: usize,
    /// = repeat stack (seg001:0254/025c).
    repeat_stack: Vec<RepeatLevel>,
    active: Option<ActiveSegment>,
    /// = `last_time_constant` (seg001:02a5): reused by continuation blocks.
    last_tc: u8,
    /// = `last_codec` (seg001:02a6).
    last_codec: u8,
    /// = `cur_marker` (seg001:022c): host-readable playback position.
    marker: u16,
    /// Monotonic count of input (VOC-rate) samples drained across the engine's
    /// lifetime: the sum of every fully-consumed segment's length. The live
    /// total adds the active segment's integer phase (see [`Engine::samples_played`]).
    /// Slaves talking-head lip-sync to the sample clock, exactly as the former
    /// `PcmStream::samples_consumed` did.
    samples_played: u64,
    volume: u8,
    /// Mixer balance/pan byte (0..0xf0, center 0x78). The DOS dnsdb driver took
    /// this as `ah` of set_volume but discarded it (the entry body is a `retf`);
    /// the port applies it as a per-channel output gain. See [`balance_to_gains`].
    balance: u8,
    /// = `playing_flag` (seg001:023b).
    playing: bool,
    /// = `idle_flag` (seg001:02a7); defaults set (idle) after init.
    idle: bool,
    /// = `end_of_stream_flag` (seg001:0250).
    end_of_stream: bool,
    /// Soft pause (DOS pause halts the DMA but leaves `playing_flag` set, so a
    /// paused stream is flag-identical to a playing one).
    paused: bool,
}

impl Engine {
    fn new() -> Self {
        Self {
            current: None,
            queued: None,
            cursor: 0,
            repeat_stack: Vec::new(),
            active: None,
            last_tc: 0,
            last_codec: 0,
            marker: 0,
            samples_played: 0,
            volume: 255,
            balance: 120,
            playing: false,
            idle: true,
            end_of_stream: false,
            paused: false,
        }
    }

    // = dnsdb_start_playback_impl (seg001:0187) + cmd_start_playback (seg001:0891):
    // prime a fresh job. Shared by start_playback and the queue immediate-start
    // path. Resets the cursor, repeat stack and end-of-stream flag, marks
    // PLAYING (seg001:0898) / not-IDLE (seg001:08c6), initializes the marker to
    // 0xFFFF (seg001:08bd) and runs the read loop to fetch the first block.
    fn begin(&mut self, job: VocJob) {
        self.current = Some(job);
        self.cursor = 0;
        self.repeat_stack.clear();
        self.end_of_stream = false;
        self.marker = 0xffff;
        self.paused = false;
        self.playing = true;
        self.idle = false;
        self.active = self.advance(); // = voc_read_loop (seg001:08c3)
    }

    // = dnsdb_start_playback (seg001:0106).
    fn start_playback(&mut self, voc: &[u8], loop_flags: u8) -> bool {
        if self.playing {
            return false;
        }
        self.begin(make_job(voc, loop_flags));
        true
    }

    // = dnsdb_stop / dnsdb_reset (seg001:0109/0103) -> cmd_stop_playback
    // (seg001:08ce). If PLAYING, tear down the transfer and clear playing/marker
    // state (playback_stop_dma, seg001:05e9); always enter IDLE (seg001:08e0).
    fn stop(&mut self) {
        if self.playing {
            self.playing = false; // = seg001:05f6
            self.repeat_stack.clear(); // = seg001:05f9
            self.marker = 0; // = seg001:05fc
            self.active = None;
        }
        self.idle = true;
        self.paused = false;
    }

    // = cmd_pause (seg001:08e6): no-op unless PLAYING; halts output without
    // touching the state flags.
    fn pause(&mut self) {
        if self.playing {
            self.paused = true;
        }
    }

    // = cmd_resume (seg001:08f6): no-op unless PLAYING.
    fn resume(&mut self) {
        if self.playing {
            self.paused = false;
        }
    }

    // = dnsdb_queue_next (seg001:0112) -> dnsdb_queue_next_impl (seg001:01c0).
    // Record the next job (seg001:01c1, marked ready). It is started immediately
    // only when neither PLAYING nor IDLE — i.e. from the ENDED state where a
    // stream just finished naturally (gate seg001:01f2/01fa). Otherwise it is
    // left for voc_blk0_terminator to chain at the current stream's end.
    fn queue_next(&mut self, voc: &[u8], loop_flags: u8) {
        self.queued = Some(make_job(voc, loop_flags));
        if !self.playing && !self.idle {
            let job = self.queued.take().unwrap();
            self.begin(job);
        }
    }

    // = dnsdb_end_loop (seg001:010c) -> cmd_break_loop arg 0 (seg001:0908).
    // Zero the innermost loop count so it exits at the next repeat-end (after
    // finishing the current iteration). Returns true if no loop was active.
    fn end_loop(&mut self) -> bool {
        match self.repeat_stack.last_mut() {
            Some(top) => {
                top.count = 0;
                false
            }
            None => true,
        }
    }

    // = dnsdb_break_loop (seg001:010f) -> cmd_break_loop arg 1 (seg001:0908).
    // Pop the innermost loop now (seg001:0917/091f), cut the current block
    // (~dsp_wait_block_done) and resume reading after it (voc_advance_block +
    // voc_read_loop, seg001:0927/092a). Returns true if no loop was active.
    fn break_loop(&mut self) -> bool {
        if self.repeat_stack.is_empty() {
            return true;
        }
        self.repeat_stack.pop();
        self.active = None;
        self.active = self.advance();
        if self.end_of_stream {
            self.playing = false;
        }
        false
    }

    const VOC_BLK0_TERMINATOR: u8 = 0;
    const VOC_BLK1_SOUND_DATA: u8 = 1;
    const VOC_BLK2_CONTINUATION: u8 = 2;
    const VOC_BLK3_SILENCE: u8 = 3;
    const VOC_BLK4_MARKER: u8 = 4;
    const VOC_BLK5_TEXT: u8 = 5;
    const VOC_BLK6_REPEAT_START: u8 = 6;
    const VOC_BLK7_REPEAT_END: u8 = 7;

    // = voc_read_loop (seg001:0608)
    fn advance(&mut self) -> Option<ActiveSegment> {
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > MAX_BLOCKS_PER_ADVANCE {
                self.end_of_stream = true;
                self.playing = false;
                return None;
            }

            let data = Arc::clone(&self.current.as_ref()?.data);
            let cursor = self.cursor;
            let block_type = data.get(cursor).copied().unwrap_or(0);

            // = voc_skip_unknown_block (seg001:061b): types >= 8 are skipped.
            if block_type >= 8 {
                self.cursor = next_cursor(&data, cursor);
                continue;
            }

            match block_type {
                // = voc_blk0_terminator (seg001:0676)
                Self::VOC_BLK0_TERMINATOR => match self.terminate() {
                    TermAction::Continue => continue,
                    TermAction::Stop => return None,
                },
                // = voc_blk1_sound_data (seg001:06e9)
                Self::VOC_BLK1_SOUND_DATA => {
                    let body = block_body(&data, cursor);
                    let tc = body.first().copied().unwrap_or(0);
                    let codec = body.get(1).copied().unwrap_or(0);
                    self.last_tc = tc;
                    self.last_codec = codec;
                    let pcm = decode_pcm(codec, body.get(2..).unwrap_or(&[]));
                    self.cursor = next_cursor(&data, cursor);
                    if pcm.is_empty() {
                        continue;
                    }
                    return Some(ActiveSegment::new(pcm, rate_from_tc(tc)));
                }
                // = voc_blk2_continuation (seg001:0731)
                // Reuses the previous block's time constant and codec.
                Self::VOC_BLK2_CONTINUATION => {
                    let body = block_body(&data, cursor);
                    let pcm = decode_pcm(self.last_codec, body);
                    self.cursor = next_cursor(&data, cursor);
                    if pcm.is_empty() {
                        continue;
                    }
                    return Some(ActiveSegment::new(pcm, rate_from_tc(self.last_tc)));
                }
                // = voc_blk3_silence (seg001:073f)
                Self::VOC_BLK3_SILENCE => {
                    let body = block_body(&data, cursor);
                    let samples = read_u16(body, 0) as usize + 1;
                    let tc = body.get(2).copied().unwrap_or(0);
                    self.cursor = next_cursor(&data, cursor);
                    return Some(ActiveSegment::new(vec![0x80u8; samples], rate_from_tc(tc)));
                }
                // = voc_blk4_marker (seg001:0768)
                Self::VOC_BLK4_MARKER => {
                    let body = block_body(&data, cursor);
                    self.marker = read_u16(body, 0);
                    self.cursor = next_cursor(&data, cursor);
                    continue;
                }
                // = voc_blk5_text (seg001:0774): ASCII comment
                Self::VOC_BLK5_TEXT => {
                    self.cursor = next_cursor(&data, cursor);
                    continue;
                }
                // = voc_blk6_repeat_start (seg001:0779)
                Self::VOC_BLK6_REPEAT_START => {
                    let body = block_body(&data, cursor);
                    let count = read_u16(body, 0);
                    self.cursor = next_cursor(&data, cursor);
                    self.repeat_stack.push(RepeatLevel {
                        count,
                        loopstart: self.cursor,
                    });
                    continue;
                }
                // = voc_blk7_repeat_end (seg001:07a2): loop back or pop.
                Self::VOC_BLK7_REPEAT_END => {
                    match self.repeat_stack.last_mut() {
                        None => self.cursor = next_cursor(&data, cursor),
                        Some(top) if top.count == 0 => {
                            self.repeat_stack.pop();
                            self.cursor = next_cursor(&data, cursor);
                        }
                        Some(top) => {
                            self.cursor = top.loopstart;
                            if top.count != 0xff {
                                top.count -= 1;
                            }
                        }
                    }
                    continue;
                }
                _ => unreachable!(),
            }
        }
    }

    // = voc_blk0_terminator (seg001:0676). Priority: stop-at-end (+7 bit7) >
    // chain to queued > loop whole job (+7 bit6) > plain end.
    fn terminate(&mut self) -> TermAction {
        let flags = self.current.as_ref().map(|j| j.loop_flags).unwrap_or(0);

        // bit7: stop and go idle (seg001:0686 -> 06dc/06e1).
        if flags & VOC_STOP_AT_END != 0 {
            self.idle = true;
            self.end_of_stream = true;
            self.playing = false;
            return TermAction::Stop;
        }
        // queued ready: promote it to current (seg001:068c == 2 -> 06b8).
        if self.queued.is_some() {
            self.current = self.queued.take();
            self.cursor = 0;
            return TermAction::Continue;
        }
        // bit6: replay the whole job (seg001:0697).
        if flags & VOC_LOOP_WHOLE != 0 {
            self.cursor = 0;
            return TermAction::Continue;
        }
        // plain end: end-of-stream, leave idle clear -> ENDED state (seg001:06e1).
        self.end_of_stream = true;
        self.playing = false;
        TermAction::Stop
    }

    // = playback_isr drain (seg001:053a) + resampling. Produce one output-rate
    // sample (mono, -1.0..1.0 scaled by volume). Advances the block engine when
    // a block is exhausted; returns silence (0.0) when paused/stopped.
    fn next_sample(&mut self, output_rate: u32) -> f32 {
        if !self.playing || self.paused {
            return 0.0;
        }
        loop {
            if self.active.is_none() {
                match self.advance() {
                    Some(seg) => self.active = Some(seg),
                    None => return 0.0, // ended; advance() set the flags
                }
            }
            let seg = self.active.as_mut().unwrap();
            let idx = seg.phase as usize;
            if idx >= seg.pcm.len() {
                // Block consumed; fold its full length into the lifetime input
                // sample count, then fetch the next one.
                self.samples_played += seg.pcm.len() as u64;
                self.active = None;
                continue;
            }
            let s1 = seg.pcm[idx] as f32;
            let s2 = if idx + 1 < seg.pcm.len() {
                seg.pcm[idx + 1] as f32
            } else {
                s1
            };
            let frac = seg.phase - idx as f32;
            let sample = s1 + (s2 - s1) * frac;
            seg.phase += seg.rate as f32 / output_rate as f32;
            let volume = self.volume as f32 / 255.0;
            return ((sample - 128.0) / 128.0) * volume;
        }
    }

    // Lifetime count of input samples drained: fully-consumed segments plus the
    // active segment's integer phase (clamped to its length).
    fn samples_played(&self) -> u64 {
        let active = self
            .active
            .as_ref()
            .map(|s| (s.phase as u64).min(s.pcm.len() as u64))
            .unwrap_or(0);
        self.samples_played + active
    }
}

/// Build a job from raw VOC bytes, skipping the 26-byte header so `data` is
/// positioned at the first block (= descriptor `+0`, seg001:0106).
fn make_job(voc: &[u8], loop_flags: u8) -> VocJob {
    let start = if voc.len() >= VOC_HEADER_LEN && voc.starts_with(b"Creative Voice File") {
        VOC_HEADER_LEN
    } else {
        0
    };
    VocJob {
        data: Arc::from(&voc[start..]),
        loop_flags,
    }
}

/// Decode a VOC data block's payload to 8-bit unsigned PCM. Codec 0 (8-bit PCM,
/// `codec_to_dsp_cmd[0]` = DSP 0x14, seg001:026e) is a passthrough. The ADPCM
/// codecs (1..3) were decoded by the SB DSP in hardware; Dune's assets are all
/// codec 0, so they are passed through unchanged here.
fn decode_pcm(codec: u8, bytes: &[u8]) -> Vec<u8> {
    let _ = codec;
    bytes.to_vec()
}

/// Sample rate from a VOC time constant: `1e6 / (256 - tc)` (the SB DSP 0x40
/// semantics, `voc_blk1_sound_data`, seg001:06e9).
fn rate_from_tc(tc: u8) -> u32 {
    1_000_000 / (256 - tc as u32)
}

/// Block body length: the 24-bit field at `cursor+1..+4` (`voc_advance_block`,
/// seg001:0620).
fn block_size(data: &[u8], cursor: usize) -> usize {
    let b0 = data.get(cursor + 1).copied().unwrap_or(0) as usize;
    let b1 = data.get(cursor + 2).copied().unwrap_or(0) as usize;
    let b2 = data.get(cursor + 3).copied().unwrap_or(0) as usize;
    b0 | (b1 << 8) | (b2 << 16)
}

/// Cursor of the next block: 4 header bytes + the 24-bit body length
/// (`voc_advance_block`, seg001:0620).
fn next_cursor(data: &[u8], cursor: usize) -> usize {
    cursor + 4 + block_size(data, cursor)
}

/// The current block's body slice (`[type u8][size u24][body…]`), clamped.
fn block_body(data: &[u8], cursor: usize) -> &[u8] {
    let start = (cursor + 4).min(data.len());
    let end = (start + block_size(data, cursor)).min(data.len());
    &data[start..end]
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    let lo = bytes.get(at).copied().unwrap_or(0) as u16;
    let hi = bytes.get(at + 1).copied().unwrap_or(0) as u16;
    lo | (hi << 8)
}

/// Map a mixer balance/pan byte to per-channel (left, right) linear gains:
/// `0` = hard left, `0x78` (120) = center (both unity), `>= 0xf0` (240) = hard
/// right. A plain balance law (attenuate the opposite channel, unity at center).
///
/// This is a port enhancement with no DOS reference curve: the dnsdb / DNADL
/// drivers accepted the balance byte (`ah` of set_volume) but the SB / AdLib
/// output paths discarded it. Shared by the PCM callback and the MIDI mixer so
/// both channels pan identically.
pub(crate) fn balance_to_gains(balance: u8) -> (f32, f32) {
    let pan = ((balance as f32 - 120.0) / 120.0).clamp(-1.0, 1.0);
    let left = if pan > 0.0 { 1.0 - pan } else { 1.0 };
    let right = if pan < 0.0 { 1.0 + pan } else { 1.0 };
    (left, right)
}

/// The .VOC playback driver. Holds the shared engine state and owns the CPAL
/// output stream (dropping it stops audio). Construct with [`PcmPlayer::new`]
/// (= dnsdb_init, seg001:0120) and keep it alive for the lifetime of playback.
pub struct PcmPlayer {
    shared: Arc<Mutex<Engine>>,
    _stream: Option<cpal::Stream>,
}

impl PcmPlayer {
    // = dnsdb_init (seg001:0120): bring up the backend and leave the driver
    // idle. If no output device is available the engine still tracks state,
    // only the audio output is absent.
    pub fn new(output_rate: u32) -> Self {
        let shared = Arc::new(Mutex::new(Engine::new()));
        let stream = build_stream(Arc::clone(&shared), output_rate);
        Self {
            shared,
            _stream: stream,
        }
    }

    /// = dnsdb_start_playback (seg001:0106). Returns false if refused because a
    /// voice is already playing (no preemption).
    pub fn start_playback(&self, voc: &[u8], loop_flags: u8) -> bool {
        self.shared.lock().unwrap().start_playback(voc, loop_flags)
    }

    /// = dnsdb_stop (seg001:0109).
    pub fn stop(&self) {
        self.shared.lock().unwrap().stop();
    }

    /// = dnsdb_reset (seg001:0103); identical to [`stop`](Self::stop).
    pub fn reset(&self) {
        self.shared.lock().unwrap().stop();
    }

    /// = dispatch cmd 0xA (cmd_pause, seg001:08e6).
    pub fn pause(&self) {
        self.shared.lock().unwrap().pause();
    }

    /// = dispatch cmd 0xB (cmd_resume, seg001:08f6).
    pub fn resume(&self) {
        self.shared.lock().unwrap().resume();
    }

    /// = dnsdb_queue_next (seg001:0112).
    pub fn queue_next(&self, voc: &[u8], loop_flags: u8) {
        self.shared.lock().unwrap().queue_next(voc, loop_flags);
    }

    /// = dnsdb_end_loop (seg001:010c). Returns true if no loop was active.
    pub fn end_loop(&self) -> bool {
        self.shared.lock().unwrap().end_loop()
    }

    /// = dnsdb_break_loop (seg001:010f). Returns true if no loop was active.
    pub fn break_loop(&self) -> bool {
        self.shared.lock().unwrap().break_loop()
    }

    /// Host-readable playback marker (= cur_marker, seg001:022c). 0xFFFF at the
    /// start of a job; set by type-4 marker blocks as they are reached.
    pub fn marker(&self) -> u16 {
        self.shared.lock().unwrap().marker
    }

    /// True while a voice is playing (= playing_flag, seg001:023b). Stays true
    /// while paused.
    pub fn is_playing(&self) -> bool {
        self.shared.lock().unwrap().playing
    }

    /// True when stopped/idle (= idle_flag, seg001:02a7).
    pub fn is_idle(&self) -> bool {
        self.shared.lock().unwrap().idle
    }

    /// = dnsdb_set_volume (seg001:0115), the `al` (level) half. The DOS entry
    /// body is a `retf` — the SB hardware path carried no software volume — so
    /// this is a port enhancement layered over the CPAL mixer.
    pub fn set_volume(&self, volume: u8) {
        self.shared.lock().unwrap().volume = volume;
    }

    /// = dnsdb_set_volume (seg001:0115), the `ah` (balance/pan) half — the byte
    /// the mixer panel's VOICES balance knob (seg001:28a6) supplies. Discarded by
    /// the DOS driver; the port applies it as a per-channel output gain.
    pub fn set_balance(&self, balance: u8) {
        self.shared.lock().unwrap().balance = balance;
    }

    /// Lifetime count of input (VOC-rate) samples drained. Slaves talking-head
    /// lip-sync to the sample clock (the former `PcmStream::samples_consumed`).
    pub fn samples_played(&self) -> u64 {
        self.shared.lock().unwrap().samples_played()
    }

    /// True while the gapless-chain slot holds a queued-but-not-yet-promoted
    /// job. The faithful analog of the DOS HNM job-state byte `[si+6]==1`
    /// (`hnm_wait_for_frame`, seg000:cad4): a frame carrying an SD chunk waits
    /// while this is true and advances once the driver has picked the buffer up.
    pub fn queue_slot_filled(&self) -> bool {
        self.shared.lock().unwrap().queued.is_some()
    }
}

/// Pick the output rate for `device`: `preferred` when the device supports it,
/// otherwise the device's default rate. The DOS drivers run at the SB/OPL
/// native 49716 Hz; cpal 0.18 validates the requested rate against the device
/// (0.16 let the backend resample silently), and modern hardware rarely
/// accepts 49716. Both synthesis paths resample to whatever rate this returns
/// ([`Engine::next_sample`] here, Nuked-OPL3's resampler in midi.rs).
pub(crate) fn supported_output_rate(device: &cpal::Device, preferred: u32) -> u32 {
    if let Ok(configs) = device.supported_output_configs() {
        for config in configs {
            if config.channels() >= 2
                && config.sample_format() == cpal::SampleFormat::F32
                && (config.min_sample_rate()..=config.max_sample_rate()).contains(&preferred)
            {
                return preferred;
            }
        }
    }
    device
        .default_output_config()
        .map(|config| config.sample_rate())
        .unwrap_or(48000)
}

/// Open a stereo CPAL output stream whose callback drains the engine.
fn build_stream(shared: Arc<Mutex<Engine>>, output_rate: u32) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(device) => device,
        None => {
            eprintln!("PcmPlayer: no audio output device; running silent");
            return None;
        }
    };
    let output_rate = supported_output_rate(&device, output_rate);
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: output_rate,
        buffer_size: cpal::BufferSize::Default,
    };
    let cb_shared = Arc::clone(&shared);
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let mut engine = cb_shared.lock().unwrap();
            // Balance is constant across the callback; split the mono sample into
            // the stereo frame with the per-channel gains.
            let (left_gain, right_gain) = balance_to_gains(engine.balance);
            for frame in data.chunks_exact_mut(2) {
                let sample = engine.next_sample(output_rate);
                frame[0] = sample * left_gain;
                frame[1] = sample * right_gain;
            }
        },
        |err| eprintln!("PcmPlayer stream error: {err}"),
        None,
    );
    match stream {
        Ok(stream) => {
            stream.play().ok();
            Some(stream)
        }
        Err(err) => {
            eprintln!("PcmPlayer: failed to build output stream: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::balance_to_gains;

    #[test]
    fn balance_center_is_unity_both_channels() {
        // 0x78 (120) — the knob's center position — leaves both channels at unity.
        assert_eq!(balance_to_gains(120), (1.0, 1.0));
    }

    #[test]
    fn balance_extremes_mute_the_opposite_channel() {
        // 0 = hard left (right muted); 0xf0 (240) = hard right (left muted).
        assert_eq!(balance_to_gains(0), (1.0, 0.0));
        assert_eq!(balance_to_gains(0xf0), (0.0, 1.0));
    }

    #[test]
    fn balance_attenuates_only_the_opposite_channel() {
        // Halfway left of center keeps the left channel at unity and pulls the
        // right channel down; the matching value right of center mirrors it.
        let (l, r) = balance_to_gains(60); // 60 = (120 - 60)/120 = -0.5 pan
        assert_eq!(l, 1.0);
        assert!((r - 0.5).abs() < 1e-6);
        let (l, r) = balance_to_gains(180); // 180 = +0.5 pan
        assert!((l - 0.5).abs() < 1e-6);
        assert_eq!(r, 1.0);
    }
}
