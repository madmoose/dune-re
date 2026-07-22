//! Built-in game-clip recorder (video + audio).
//!
//! Port-only dev tooling — not a port of any DOS routine, so it carries no
//! `= seg:ofs` link. It captures the game's own output directly instead of a
//! screen grab, so clips are always pixel-exact and identically sized with no
//! window chrome, letterbox, or cursor to crop out afterwards.
//!
//! Two backends, chosen by [`RecordFormat`]:
//!
//! * **MP4** (default) — pipes raw RGBA frames to an external `ffmpeg` (which
//!   must be on `PATH`), which upscales 5×/6× to a full-range BT.709-tagged
//!   1600×1200 H.264. Audio is buffered and, on stop, written to a temp WAV that
//!   a *detached* background `ffmpeg` muxes in (and cleans up), so closing the
//!   game is instant rather than blocking on the mux.
//! * **AVI** — a self-contained, in-process alternative (no ffmpeg): the capture
//!   thread expands frames to 24-bit BGR and streams them, interleaved with the
//!   mixed audio, straight into an uncompressed native-320×200 `.avi` via the
//!   [`crate::avi`] muxer. Finalised synchronously on stop; larger files, capped
//!   at ~2 GB by the AVI format.
//!
//! Shared by both: a dedicated capture thread samples the latest frame from the
//! recorder's own tee'd cell at a fixed rate; and the two independent CPAL
//! streams (PCM voice/SFX and OPL3 music) each call [`Recorder::mix_in`] from
//! their output callback, appending contiguously into one buffer (summed where
//! they overlap). Everything is anchored to a single `Instant` captured when
//! recording starts, which keeps audio and video in sync despite their separate
//! clocks.

use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    avi::SegmentedAviWriter,
    frame_slot::{Frame, FrameSink, FrameSlot},
    framebuffer::FrameBuffer,
    palette::Palette,
};

/// Native framebuffer size captured before ffmpeg's upscale.
const SRC_W: usize = 320;
const SRC_H: usize = 200;
/// Aspect-corrected output size (5× horizontal, 6× vertical → 4:3).
const OUT_W: u32 = 1600;
const OUT_H: u32 = 1200;
/// Fixed capture frame rate written to the video file (ffmpeg/MP4 backend).
const FPS: f64 = 60.0;
/// Capture frame rate for the AVI backend. Lower than the MP4 path because AVI
/// stores raw video — 30 fps roughly halves file size and doubles the ~2 GB
/// duration cap, with negligible visual loss for this game.
const AVI_FPS: f64 = 30.0;
/// Roll the AVI over to a new file before this size, staying safely under
/// classic AVI's ~2 GB (signed 32-bit) limit.
const AVI_MAX_BYTES: u64 = 1_900_000_000;
/// Timeline rate for the mixed audio buffer / WAV. Fixed (not the device rate)
/// so the recorder is decoupled from whatever rate each CPAL stream opened at;
/// [`Recorder::mix_in`] resamples every block into this rate.
const REC_AUDIO_RATE: u32 = 48000;

/// Output container/codec for a recording.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordFormat {
    /// H.264 in MP4 via ffmpeg (external process), aspect-corrected to 1600×1200.
    Mp4,
    /// Uncompressed 24-bit AVI written in-process (no ffmpeg), native 320×200.
    Avi,
}

impl RecordFormat {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => RecordFormat::Avi,
            _ => RecordFormat::Mp4,
        }
    }

    /// The output-file extension for this format.
    pub fn ext(self) -> &'static str {
        match self {
            RecordFormat::Mp4 => "mp4",
            RecordFormat::Avi => "avi",
        }
    }

    /// Deduce the format from a path's extension (case-insensitive), if it names
    /// a supported container.
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("mp4") => Some(RecordFormat::Mp4),
            Some(e) if e.eq_ignore_ascii_case("avi") => Some(RecordFormat::Avi),
            _ => None,
        }
    }
}

/// A recording target: streams video (via ffmpeg or the in-process AVI muxer)
/// and accumulates a mixed stereo audio buffer from the two output streams.
pub struct Recorder {
    // Lock-free fast path for the audio callbacks: skip all work when idle.
    active: AtomicBool,
    // Output format, selectable before recording starts (0 = Mp4, 1 = Avi).
    format: AtomicU8,
    // Shared with the capture thread so the AVI backend can drain finished audio.
    audio: Arc<Mutex<Option<AudioSink>>>,
    video: Mutex<Option<VideoWorker>>,
    // The most recently published frame, tee'd in on every publish while
    // recording (see [`RecorderTee`]). This is the recorder's *own* copy, never
    // drained — unlike peeking the display's `FrameSlot`, which the present
    // thread consumes with `take_latest`, so the capture thread always samples
    // the freshest frame instead of racing the display for it.
    frame: Arc<Mutex<Option<Frame>>>,
}

/// Which independently-clocked stream a block of samples came from. Each has its
/// own contiguous write cursor into the mix buffer.
#[derive(Clone, Copy)]
pub enum AudioTrack {
    /// PCM voice/SFX (`pcm_player`).
    Pcm = 0,
    /// OPL3 music (`midi`).
    Midi = 1,
}

/// Per-stream write position. The first block is anchored to the recording
/// timeline by wall-clock; every later block is appended contiguously from
/// `base`, so a stream's samples never overlap or gap themselves.
#[derive(Clone, Copy, Default)]
struct TrackCursor {
    base: Option<usize>,
    written: usize,
}

/// Audio timeline built while recording. Each stream appends its samples
/// contiguously; the two streams are summed where they overlap, so PCM and OPL3
/// output mixes together.
struct AudioSink {
    start: Instant,
    buf: Vec<[f32; 2]>,
    cursors: [TrackCursor; 2],
    // How many samples the AVI backend has already drained to disk.
    emitted: usize,
}

impl AudioSink {
    fn new(start: Instant) -> Self {
        Self {
            start,
            buf: Vec::new(),
            cursors: [TrackCursor::default(); 2],
            emitted: 0,
        }
    }

    /// Sample index below which every *started* stream has contributed, so the
    /// samples are final and safe to write out. A stream that hasn't produced its
    /// first block yet is ignored (both device streams start within a few ms of
    /// recording, and each track's wall-clock base lands at ~the current emit
    /// position, so a late-starting track doesn't lose samples below the mark).
    fn watermark(&self) -> usize {
        self.cursors
            .iter()
            .filter_map(|c| c.base.map(|b| b + c.written))
            .min()
            .unwrap_or(0)
    }

    /// Drain `[emitted, end)` as interleaved 16-bit PCM and advance `emitted`.
    fn drain_pcm(&mut self, end: usize) -> Vec<u8> {
        let end = end.min(self.buf.len());
        if end <= self.emitted {
            return Vec::new();
        }
        let mut out = Vec::with_capacity((end - self.emitted) * 4);
        for frame in &self.buf[self.emitted..end] {
            for &s in frame {
                let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        self.emitted = end;
        out
    }
}

/// How a running recording is finalised on stop.
enum Backend {
    /// ffmpeg path: temp video + WAV muxed by a detached background process.
    Mp4 {
        tmp_video: PathBuf,
        tmp_wav: PathBuf,
        out_path: PathBuf,
    },
    /// In-process AVI: the capture thread finalises the file itself on stop.
    Avi { out_path: PathBuf },
}

/// The video-capture thread plus what stop() needs to finalise it.
struct VideoWorker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    backend: Backend,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            format: AtomicU8::new(RecordFormat::Mp4 as u8),
            audio: Arc::new(Mutex::new(None)),
            video: Mutex::new(None),
            frame: Arc::new(Mutex::new(None)),
        }
    }

    /// Select the output format for subsequent recordings.
    pub fn set_format(&self, format: RecordFormat) {
        self.format.store(format as u8, Ordering::Relaxed);
    }

    fn format(&self) -> RecordFormat {
        RecordFormat::from_u8(self.format.load(Ordering::Relaxed))
    }

    /// True while a recording is in progress.
    pub fn is_recording(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Store the latest published frame for the capture thread. Called from the
    /// [`RecorderTee`] on every publish while recording.
    pub fn store_frame(&self, framebuffer: FrameBuffer, palette: Palette) {
        *self.frame.lock().unwrap() = Some((framebuffer, palette));
    }

    /// Start or stop recording. `out_path`, when `None`, auto-numbers
    /// `dune-rec-NNN.mp4` in the working directory.
    pub fn toggle(&self, out_path: Option<PathBuf>) {
        if self.is_recording() {
            self.stop();
        } else {
            self.start(out_path);
        }
    }

    /// Begin recording. No-op (with a message) if already recording or if the
    /// backend cannot be started. An explicit `out_path` whose extension names a
    /// known container (`.mp4`/`.avi`) selects that format, overriding the
    /// configured one and sticking for later F9 recordings; otherwise the
    /// configured format is used and the file is auto-numbered.
    pub fn start(&self, out_path: Option<PathBuf>) {
        if self.is_recording() {
            return;
        }
        if let Some(fmt) = out_path.as_deref().and_then(RecordFormat::from_path) {
            self.set_format(fmt);
        }
        let format = self.format();
        // Clear any stale frame from a previous session so the capture thread
        // starts from black until the first live frame is tee'd in.
        *self.frame.lock().unwrap() = None;
        let out_path = out_path.unwrap_or_else(|| next_rec_path(format.ext()));

        let start = Instant::now();
        let stop = Arc::new(AtomicBool::new(false));

        let (handle, backend) = match format {
            RecordFormat::Mp4 => self.start_mp4(&out_path, start, &stop),
            RecordFormat::Avi => self.start_avi(&out_path, start, &stop),
        }
        .unwrap_or_else(|| (None, None));
        let (Some(handle), Some(backend)) = (handle, backend) else {
            return;
        };

        // Publish the audio sink before flipping `active`: mix_in only records
        // once the sink is in place.
        *self.audio.lock().unwrap() = Some(AudioSink::new(start));
        *self.video.lock().unwrap() = Some(VideoWorker {
            stop,
            handle: Some(handle),
            backend,
        });
        self.active.store(true, Ordering::Release);
        eprintln!("recording: started → {}", out_path.display());
    }

    /// Spawn the ffmpeg/MP4 capture thread: raw RGBA frames to ffmpeg's stdin at
    /// a fixed rate. The thread waits for ffmpeg #1 on stop so the temp video is
    /// complete before `stop()` muxes it.
    #[allow(clippy::type_complexity)]
    fn start_mp4(
        &self,
        out_path: &Path,
        start: Instant,
        stop: &Arc<AtomicBool>,
    ) -> Option<(Option<JoinHandle<()>>, Option<Backend>)> {
        let tmp_video = out_path.with_extension("tmp.mp4");
        let tmp_wav = out_path.with_extension("tmp.wav");
        let mut child = match spawn_video_ffmpeg(&tmp_video) {
            Ok(child) => child,
            Err(e) => {
                eprintln!("recording: could not start ffmpeg ({e}); is it installed and on PATH?");
                return None;
            }
        };
        let mut stdin = child.stdin.take().expect("ffmpeg stdin");

        let frame_cell = Arc::clone(&self.frame);
        let thread_stop = Arc::clone(stop);
        let handle = thread::spawn(move || {
            let mut rgba = vec![0u8; SRC_W * SRC_H * 4];
            let mut frames_written: u64 = 0;
            loop {
                if thread_stop.load(Ordering::Acquire) {
                    break;
                }
                let target = (start.elapsed().as_secs_f64() * FPS).floor() as u64;
                if frames_written < target {
                    if let Some((fb, pal)) = frame_cell.lock().unwrap().clone() {
                        expand_frame(&fb, &pal, &mut rgba);
                    }
                    while frames_written < target {
                        if stdin.write_all(&rgba).is_err() {
                            thread_stop.store(true, Ordering::Release);
                            break;
                        }
                        frames_written += 1;
                    }
                }
                thread::sleep(Duration::from_millis(2));
            }
            drop(stdin); // EOF to ffmpeg
            let _ = child.wait(); // finalise the temp video before stop() muxes it
        });

        Some((
            Some(handle),
            Some(Backend::Mp4 {
                tmp_video,
                tmp_wav,
                out_path: out_path.to_path_buf(),
            }),
        ))
    }

    /// Spawn the in-process AVI capture thread: expand frames to 24-bit BGR and
    /// write them to the AVI at a fixed rate, interleaving mixed audio drained up
    /// to the watermark. Rolls over to a new file before the ~2 GB AVI limit and
    /// finalises the last one when stopped.
    #[allow(clippy::type_complexity)]
    fn start_avi(
        &self,
        out_path: &Path,
        start: Instant,
        stop: &Arc<AtomicBool>,
    ) -> Option<(Option<JoinHandle<()>>, Option<Backend>)> {
        let writer = match SegmentedAviWriter::new(
            out_path,
            SRC_W as u16,
            SRC_H as u16,
            AVI_FPS as u32,
            REC_AUDIO_RATE,
            AVI_MAX_BYTES,
        ) {
            Ok(writer) => writer,
            Err(e) => {
                eprintln!("recording: could not create AVI file ({e})");
                return None;
            }
        };

        let frame_cell = Arc::clone(&self.frame);
        let audio = Arc::clone(&self.audio);
        let thread_stop = Arc::clone(stop);
        let handle = thread::spawn(move || {
            let mut writer = writer;
            // Current frame (black until the first live frame is tee'd in).
            let mut cur = (FrameBuffer::new(SRC_W as u16, SRC_H as u16), Palette::new());
            let mut frames_written: u64 = 0;
            loop {
                if thread_stop.load(Ordering::Acquire) {
                    break;
                }
                let target = (start.elapsed().as_secs_f64() * AVI_FPS).floor() as u64;
                if frames_written < target {
                    if let Some(frame) = frame_cell.lock().unwrap().clone() {
                        cur = frame;
                    }
                    while frames_written < target {
                        if writer.write_video(&cur.0, &cur.1).is_err() {
                            thread_stop.store(true, Ordering::Release);
                            break;
                        }
                        frames_written += 1;
                    }
                }
                // Interleave audio: drain everything finalised so far.
                let pcm = drain_audio(&audio, false);
                let _ = writer.write_audio(&pcm);
                thread::sleep(Duration::from_millis(2));
            }
            // Flush the tail and finalise the last segment.
            let pcm = drain_audio(&audio, true);
            let _ = writer.write_audio(&pcm);
            let segments = writer.segment_count();
            let _ = writer.finalize();
            if segments > 1 {
                eprintln!("recording: split into {segments} AVI files (size limit)");
            }
        });

        Some((
            Some(handle),
            Some(Backend::Avi {
                out_path: out_path.to_path_buf(),
            }),
        ))
    }

    /// Stop recording and finalise the clip.
    pub fn stop(&self) {
        if !self.is_recording() {
            return;
        }
        // Clear first so no audio callback appends after the capture thread's
        // final drain / the WAV write.
        self.active.store(false, Ordering::Release);

        let worker = self.video.lock().unwrap().take();
        let Some(worker) = worker else {
            return;
        };

        // Stop and join the capture thread. AVI flushes remaining audio and
        // finalises the file here; MP4 waits for ffmpeg #1 to finish the video.
        worker.stop.store(true, Ordering::Release);
        if let Some(handle) = worker.handle {
            let _ = handle.join();
        }

        // The capture thread is done with the audio sink now; clear it.
        let sink = self.audio.lock().unwrap().take();

        match worker.backend {
            Backend::Avi { out_path } => {
                eprintln!("recording: wrote {}", out_path.display());
            }
            Backend::Mp4 {
                tmp_video,
                tmp_wav,
                out_path,
            } => self.finalise_mp4(sink, &tmp_video, &tmp_wav, &out_path),
        }
    }

    /// Write the mixed audio to a temp WAV and hand the mux to a detached
    /// background process, so closing the game is instant instead of blocking on
    /// ffmpeg's AAC re-encode. The WAV write is synchronous because the process
    /// may `std::process::exit` right after (the in-game EXIT GAME path).
    fn finalise_mp4(
        &self,
        sink: Option<AudioSink>,
        tmp_video: &Path,
        tmp_wav: &Path,
        out_path: &Path,
    ) {
        let audio_ok = match sink {
            Some(sink) => match write_wav(tmp_wav, &sink.buf, REC_AUDIO_RATE) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("recording: failed to write audio ({e}); saving video only");
                    false
                }
            },
            None => false,
        };

        let spawned = if audio_ok {
            spawn_detached_mux(tmp_video, tmp_wav, out_path)
        } else {
            spawn_detached_copy(tmp_video, out_path)
        };

        match spawned {
            Ok(child) => {
                // Reap in the background so no zombie lingers when recording is
                // stopped mid-session (F9); on process exit the child is orphaned
                // and finishes under init instead.
                thread::spawn(move || {
                    let mut child = child;
                    let _ = child.wait();
                });
                eprintln!("recording: finalising {} in background", out_path.display());
            }
            Err(e) => eprintln!("recording: could not start ffmpeg mux ({e})"),
        }
    }

    /// Mix a block of interleaved stereo samples from `track` into the recording
    /// timeline. Called from the CPAL output callbacks; a cheap atomic check
    /// keeps the idle path lock-free. `src_rate` is the callback's device rate;
    /// the block is linearly resampled to [`REC_AUDIO_RATE`].
    ///
    /// The block is appended **contiguously** after this track's previous block.
    /// Only the first block per track is anchored by wall-clock (to line the two
    /// independently-started streams up). Positioning every block by wall-clock
    /// instead would leave gaps/overlaps at each boundary — the device delivers
    /// callbacks in irregular bursts, so `Instant::now()` at callback entry does
    /// not track the block's true playback position — which is audible as static.
    pub fn mix_in(&self, track: AudioTrack, interleaved: &[f32], src_rate: u32) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }
        let mut guard = self.audio.lock().unwrap();
        if let Some(sink) = guard.as_mut() {
            sink.mix(track, interleaved, src_rate);
        }
    }
}

impl AudioSink {
    /// Append one resampled, contiguous block from `track` into the mix buffer.
    /// See [`Recorder::mix_in`] for why blocks are appended rather than
    /// wall-clock-positioned.
    fn mix(&mut self, track: AudioTrack, interleaved: &[f32], src_rate: u32) {
        let src_frames = interleaved.len() / 2;
        if src_frames == 0 || src_rate == 0 {
            return;
        }

        let idx = track as usize;
        if self.cursors[idx].base.is_none() {
            let anchor = (self.start.elapsed().as_secs_f64() * REC_AUDIO_RATE as f64) as usize;
            self.cursors[idx].base = Some(anchor);
        }
        let write_start = self.cursors[idx].base.unwrap() + self.cursors[idx].written;

        let out_len =
            ((src_frames as f64) * (REC_AUDIO_RATE as f64) / (src_rate as f64)).round() as usize;

        let end = write_start + out_len;
        if self.buf.len() < end {
            self.buf.resize(end, [0.0, 0.0]);
        }

        let ratio = src_rate as f64 / REC_AUDIO_RATE as f64;
        for k in 0..out_len {
            // Position within the source block, in source frames.
            let t = k as f64 * ratio;
            let i0 = t.floor() as usize;
            let i1 = (i0 + 1).min(src_frames - 1);
            let frac = (t - i0 as f64) as f32;
            let l = interleaved[i0 * 2] + (interleaved[i1 * 2] - interleaved[i0 * 2]) * frac;
            let r = interleaved[i0 * 2 + 1]
                + (interleaved[i1 * 2 + 1] - interleaved[i0 * 2 + 1]) * frac;
            let dst = &mut self.buf[write_start + k];
            dst[0] += l;
            dst[1] += r;
        }
        self.cursors[idx].written += out_len;
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

/// A [`FrameSink`] that forwards every published frame to the display's
/// `FrameSlot` and, while recording, tees a copy into the [`Recorder`]. The tee
/// gives the recorder its own never-drained latest-frame cell so the capture
/// thread never loses frames to the display's `take_latest`. The frame is
/// cloned into the recorder only while recording, so the idle cost is one
/// atomic load.
pub struct RecorderTee {
    display: FrameSlot,
    recorder: Arc<Recorder>,
}

impl RecorderTee {
    pub fn new(display: FrameSlot, recorder: Arc<Recorder>) -> Self {
        Self { display, recorder }
    }
}

impl FrameSink for RecorderTee {
    fn publish(&self, framebuffer: FrameBuffer, palette: Palette) {
        if self.recorder.is_recording() {
            self.recorder
                .store_frame(framebuffer.clone(), palette.clone());
        }
        self.display.publish(framebuffer, palette);
    }
}

/// Expand a 320×200 indexed frame through its palette into RGBA8 (mirrors the
/// present path's `Gpu::upload_frame`).
fn expand_frame(fb: &FrameBuffer, pal: &Palette, rgba: &mut [u8]) {
    for (i, &idx) in fb.pixels().iter().enumerate() {
        let c = pal.get_rgb888(idx as usize);
        let o = i * 4;
        rgba[o] = c.0;
        rgba[o + 1] = c.1;
        rgba[o + 2] = c.2;
        rgba[o + 3] = 255;
    }
}

/// ffmpeg #1: raw RGBA frames on stdin → H.264 in `tmp_video`, upscaled
/// nearest-neighbour to the aspect-corrected output size.
///
/// Colour handling: the game's palette produces full-range (0–255) RGB. Left to
/// its defaults, swscale compresses that into limited-range (16–235) YUV and
/// leaves the stream untagged, so each player guesses the range — QuickTime and
/// VLC guess differently, and the full-range guess shows lifted blacks / dimmed
/// whites (the "faded" look). We instead convert with the `colorspace` filter at
/// full range and a BT.709 matrix (`iall=bt709` == `all=bt709`, so it applies
/// only the RGB→YUV matrix — no gamma/primary remap), then tag the stream
/// full-range BT.709 and write the `nclx colr` atom QuickTime reads. Every
/// player then expands the levels identically; a black→white ramp round-trips
/// 0x00→0xff.
fn spawn_video_ffmpeg(tmp_video: &Path) -> std::io::Result<Child> {
    Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-video_size",
            &format!("{SRC_W}x{SRC_H}"),
            "-framerate",
            &format!("{FPS}"),
            "-i",
            "pipe:0",
            // Nearest-neighbour upscale, then an explicit full-range BT.709
            // RGB→YUV conversion so the pixel levels match the colour tags below.
            "-vf",
            &format!(
                "scale={OUT_W}:{OUT_H}:flags=neighbor,format=gbrp,\
                 colorspace=all=bt709:iall=bt709:range=pc:format=yuv420p"
            ),
            "-c:v",
            "libx264",
            "-crf",
            "18",
            "-color_range",
            "pc",
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-movflags",
            "+write_colr",
        ])
        .arg(tmp_video)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// ffmpeg #2, detached: mux the encoded video with the WAV into the final file,
/// then delete the temps. Runs as a background `sh` so the game can close
/// immediately; the paths are passed as positional arguments (`$1`/`$2`/`$3`) so
/// they need no shell escaping. Video is stream-copied (its colour tags ride
/// along, `+write_colr` keeps the colr atom); only the audio is (re)encoded.
fn spawn_detached_mux(video: &Path, wav: &Path, out: &Path) -> std::io::Result<Child> {
    Command::new("sh")
        .arg("-c")
        .arg(
            "if ffmpeg -y -i \"$1\" -i \"$2\" -c:v copy -c:a aac -shortest \
             -movflags +write_colr \"$3\"; then rm -f \"$1\" \"$2\"; fi",
        )
        .arg("dune-mux") // $0
        .arg(video) // $1
        .arg(wav) // $2
        .arg(out) // $3
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// No-audio fallback for [`spawn_detached_mux`]: stream-copy the video into place
/// (re-writing the colr atom) and delete the temp, in a detached `sh`.
fn spawn_detached_copy(video: &Path, out: &Path) -> std::io::Result<Child> {
    Command::new("sh")
        .arg("-c")
        .arg("if ffmpeg -y -i \"$1\" -c copy -movflags +write_colr \"$2\"; then rm -f \"$1\"; fi")
        .arg("dune-mux") // $0
        .arg(video) // $1
        .arg(out) // $2
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Lock the shared audio sink and drain finished samples as 16-bit PCM. `all`
/// drains everything (final flush at stop); otherwise only up to the watermark.
fn drain_audio(audio: &Arc<Mutex<Option<AudioSink>>>, all: bool) -> Vec<u8> {
    let mut guard = audio.lock().unwrap();
    match guard.as_mut() {
        Some(sink) => {
            let end = if all {
                sink.buf.len()
            } else {
                sink.watermark()
            };
            sink.drain_pcm(end)
        }
        None => Vec::new(),
    }
}

/// Auto-number `dune-rec-NNN.<ext>` in the working directory, skipping existing.
fn next_rec_path(ext: &str) -> PathBuf {
    for seq in 0..1000 {
        let path = PathBuf::from(format!("dune-rec-{seq:03}.{ext}"));
        if !path.exists() {
            return path;
        }
    }
    PathBuf::from(format!("dune-rec-999.{ext}"))
}

/// Write a 16-bit PCM stereo WAV. Samples are clamped from the summed f32 mix.
fn write_wav(path: &Path, samples: &[[f32; 2]], rate: u32) -> std::io::Result<()> {
    let n_frames = samples.len() as u32;
    let channels: u16 = 2;
    let bits: u16 = 16;
    let block_align = channels * bits / 8;
    let byte_rate = rate * block_align as u32;
    let data_bytes = n_frames * block_align as u32;

    let mut f = File::create(path)?;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_bytes).to_le_bytes())?;
    f.write_all(b"WAVE")?;
    f.write_all(b"fmt ")?;
    f.write_all(&16u32.to_le_bytes())?; // PCM fmt chunk size
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&channels.to_le_bytes())?;
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&byte_rate.to_le_bytes())?;
    f.write_all(&block_align.to_le_bytes())?;
    f.write_all(&bits.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_bytes.to_le_bytes())?;

    let mut bytes = Vec::with_capacity(data_bytes as usize);
    for frame in samples {
        for &s in frame {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    f.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::color::Color;

    /// Wait up to `secs` for the detached background mux to produce `path`.
    fn wait_for_file(path: &Path, secs: u64) -> bool {
        for _ in 0..(secs * 20) {
            if fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false) {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn format_deduced_from_extension() {
        assert_eq!(
            RecordFormat::from_path(Path::new("clip.avi")),
            Some(RecordFormat::Avi)
        );
        assert_eq!(
            RecordFormat::from_path(Path::new("/tmp/Clip.MP4")),
            Some(RecordFormat::Mp4)
        );
        assert_eq!(RecordFormat::from_path(Path::new("clip.mkv")), None);
        assert_eq!(RecordFormat::from_path(Path::new("clip")), None);
    }

    /// A single stream's blocks must reconstruct exactly and contiguously —
    /// no gaps or overlaps at block boundaries (the bug that caused static).
    #[test]
    fn contiguous_append_reconstructs_the_stream() {
        let mut sink = AudioSink::new(Instant::now());

        // A ramp so any duplicated/skipped sample is easy to spot. Fed in
        // irregular block sizes at the timeline rate (ratio 1.0, exact copy).
        let total = 5000usize;
        let ramp: Vec<f32> = (0..total).map(|i| i as f32).collect();
        let mut interleaved = Vec::with_capacity(total * 2);
        for &s in &ramp {
            interleaved.push(s); // L
            interleaved.push(s); // R
        }

        let block_sizes = [100usize, 250, 37, 512, 1, 900, 400];
        let mut off = 0;
        let mut i = 0;
        while off < total {
            let n = block_sizes[i % block_sizes.len()].min(total - off);
            sink.mix(
                AudioTrack::Pcm,
                &interleaved[off * 2..(off + n) * 2],
                REC_AUDIO_RATE,
            );
            off += n;
            i += 1;
        }

        let base = sink.cursors[AudioTrack::Pcm as usize].base.unwrap();
        assert_eq!(sink.cursors[AudioTrack::Pcm as usize].written, total);
        // Everything before base is untouched silence.
        for s in &sink.buf[..base] {
            assert_eq!(*s, [0.0, 0.0]);
        }
        // From base on, the ramp comes back sample-for-sample.
        for (k, &expected) in ramp.iter().enumerate() {
            assert_eq!(sink.buf[base + k], [expected, expected], "mismatch at {k}");
        }
    }

    fn have(tool: &str) -> bool {
        Command::new(tool)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// End-to-end: publish a frame, feed synthetic audio, and confirm ffmpeg
    /// produces a playable mp4 with both a video and an audio stream. Ignored by
    /// default because it needs ffmpeg/ffprobe on PATH and writes a file.
    #[test]
    #[ignore = "needs ffmpeg + ffprobe on PATH; writes a temp file"]
    fn records_a_playable_clip() {
        if !have("ffmpeg") || !have("ffprobe") {
            eprintln!("skipping: ffmpeg/ffprobe not found");
            return;
        }
        let out = std::env::temp_dir().join("dune-rec-selftest.mp4");
        let _ = fs::remove_file(&out);

        // A solid red 320x200 frame.
        let mut fb = FrameBuffer::new(320, 200);
        for p in fb.pixels.iter_mut() {
            *p = 1;
        }
        let mut pal = Palette::new();
        pal.set(1, Color(63, 0, 0));

        let rec = Recorder::new();
        rec.start(Some(out.clone()));
        assert!(rec.is_recording());
        rec.store_frame(fb, pal);

        // ~0.3 s of a 440 Hz stereo tone at 48 kHz.
        let rate = 48000u32;
        let n = (rate as f32 * 0.3) as usize;
        let mut block = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.3;
            block.push(s);
            block.push(s);
        }
        thread::sleep(Duration::from_millis(150));
        rec.mix_in(AudioTrack::Pcm, &block, rate);
        thread::sleep(Duration::from_millis(250));

        rec.stop();
        assert!(!rec.is_recording());

        // The mux runs detached in the background now, so wait for the file.
        assert!(
            wait_for_file(&out, 20),
            "background mux did not produce {}",
            out.display()
        );

        // Confirm both streams are present.
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "csv=p=0",
            ])
            .arg(&out)
            .output()
            .expect("run ffprobe");
        let streams = String::from_utf8_lossy(&probe.stdout);
        assert!(streams.contains("video"), "no video stream: {streams:?}");
        assert!(streams.contains("audio"), "no audio stream: {streams:?}");

        // The colour tags must be present and full-range: an untagged / limited-
        // range stream is what makes players (QuickTime vs VLC) disagree and look
        // "faded". Confirm the video stream is tagged pc range + BT.709.
        let color = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=color_range,color_space,color_primaries,color_transfer",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(&out)
            .output()
            .expect("run ffprobe");
        let color = String::from_utf8_lossy(&color.stdout);
        assert!(color.contains("pc"), "video not full-range (pc): {color:?}");
        assert!(
            color.contains("bt709"),
            "video not tagged BT.709: {color:?}"
        );

        let _ = fs::remove_file(&out);
    }

    /// End-to-end for the AVI backend: drive the Recorder with `RecordFormat::Avi`
    /// and confirm the in-process muxer produces a file ffmpeg decodes cleanly,
    /// with both streams. Unlike MP4, AVI is finalised synchronously in `stop()`,
    /// so the file is ready as soon as it returns (no ffmpeg needed to record —
    /// only to verify).
    #[test]
    #[ignore = "needs ffmpeg + ffprobe on PATH to verify; writes a temp file"]
    fn records_an_avi_clip() {
        if !have("ffmpeg") || !have("ffprobe") {
            eprintln!("skipping: ffmpeg/ffprobe not found");
            return;
        }
        let out = std::env::temp_dir().join("dune-rec-selftest.avi");
        let _ = fs::remove_file(&out);

        let mut fb = FrameBuffer::new(320, 200);
        for p in fb.pixels.iter_mut() {
            *p = 1;
        }
        let mut pal = Palette::new();
        pal.set(1, Color(0, 63, 0));

        let rec = Recorder::new();
        rec.set_format(RecordFormat::Avi);
        rec.start(Some(out.clone()));
        assert!(rec.is_recording());
        rec.store_frame(fb, pal);

        // Feed ~0.4 s of a tone across a few blocks while the capture thread runs.
        let rate = 48000u32;
        for _ in 0..4 {
            thread::sleep(Duration::from_millis(80));
            let n = (rate as f32 * 0.08) as usize;
            let mut block = Vec::with_capacity(n * 2);
            for i in 0..n {
                let t = i as f32 / rate as f32;
                let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.3;
                block.push(s);
                block.push(s);
            }
            rec.mix_in(AudioTrack::Pcm, &block, rate);
        }

        rec.stop();
        assert!(!rec.is_recording());
        // AVI finalises synchronously, so the file is complete now.
        let meta = fs::metadata(&out).expect("avi exists");
        assert!(meta.len() > 0, "avi is empty");

        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "csv=p=0",
            ])
            .arg(&out)
            .output()
            .expect("ffprobe");
        let streams = String::from_utf8_lossy(&probe.stdout);
        assert!(streams.contains("video"), "no video stream: {streams:?}");
        assert!(streams.contains("audio"), "no audio stream: {streams:?}");

        // Clean full decode validates the header + idx1 offsets end to end.
        let decode = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&out)
            .args(["-f", "null", "-"])
            .output()
            .expect("ffmpeg decode");
        assert!(
            decode.status.success() && decode.stderr.is_empty(),
            "decode errors: {}",
            String::from_utf8_lossy(&decode.stderr)
        );

        let _ = fs::remove_file(&out);
    }
}
