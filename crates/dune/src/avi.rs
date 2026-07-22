//! Minimal streaming AVI muxer — a pure-Rust alternative to the ffmpeg backend.
//!
//! Port-only dev tooling (no DOS equivalent). Writes a single `.avi` with one
//! uncompressed 24-bit video stream (`00db`, BI_RGB, bottom-up rows) and one
//! 16-bit PCM stereo audio stream (`01wb`), interleaved as they arrive. AVI is
//! written streaming: the `hdrl` header goes out first with placeholder sizes,
//! chunks stream into the `movi` list while an index is accumulated, and
//! [`AviWriter::finalize`] appends the `idx1` index and seeks back to patch the
//! handful of size/count fields. No external process, no temp files, no remux.
//!
//! Note: classic AVI uses 32-bit offsets, so a single file is capped at ~2 GB —
//! a few minutes of uncompressed video. [`SegmentedAviWriter`] transparently
//! rolls over to a fresh file before that limit, so a long recording becomes a
//! numbered sequence of standard AVIs instead. Frames are stored at the native
//! 320×200 with square pixels (no aspect correction).

use std::{
    fs::File,
    io::{self, BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use crate::{framebuffer::FrameBuffer, palette::Palette};

// Byte offsets of the fixed-size header fields patched by `finalize`, and the
// position of the `movi` FOURCC (the base for index offsets). These are fixed
// because everything up to the first `movi` chunk has a constant layout; the
// `debug_assert`s in `new` guard the arithmetic.
const P_RIFF_SIZE: u64 = 4;
const P_TOTAL_FRAMES: u64 = 48;
const P_VID_LENGTH: u64 = 140;
const P_AUD_LENGTH: u64 = 264;
const P_MOVI_SIZE: u64 = 318;
const MOVI_FOURCC_POS: u64 = 322;
const HEADER_LEN: u64 = 326;

const AVIF_HASINDEX: u32 = 0x10;
const AVIIF_KEYFRAME: u32 = 0x10;

const AUDIO_CHANNELS: u16 = 2;
const AUDIO_BITS: u16 = 16;
const AUDIO_BLOCK_ALIGN: u16 = AUDIO_CHANNELS * AUDIO_BITS / 8; // 4

/// One `idx1` index entry (built up as chunks are written).
struct IdxEntry {
    ckid: [u8; 4],
    flags: u32,
    offset: u32, // relative to the `movi` FOURCC
    size: u32,
}

/// A streaming AVI file being written.
pub struct AviWriter {
    file: BufWriter<File>,
    width: u16,
    height: u16,
    audio_rate: u32,
    pos: u64,
    frame_count: u32,
    audio_frames: u32, // audio sample-frames (per channel) written
    index: Vec<IdxEntry>,
    bgr: Vec<u8>, // scratch for the indexed→BGR expansion
}

impl AviWriter {
    /// Create `path` and write the header. `width`/`height` are the native frame
    /// size; `fps` the video rate; `audio_rate` the PCM sample rate.
    pub fn new(
        path: &Path,
        width: u16,
        height: u16,
        fps: u32,
        audio_rate: u32,
    ) -> io::Result<Self> {
        let file = BufWriter::new(File::create(path)?);
        let frame_bytes = width as u32 * height as u32 * 3;
        let max_bytes_per_sec = frame_bytes * fps + audio_rate * AUDIO_BLOCK_ALIGN as u32;

        let mut w = AviWriter {
            file,
            width,
            height,
            audio_rate,
            pos: 0,
            frame_count: 0,
            audio_frames: 0,
            index: Vec::new(),
            bgr: vec![0; frame_bytes as usize],
        };
        w.write_header(fps, frame_bytes, max_bytes_per_sec)?;
        Ok(w)
    }

    fn write_header(
        &mut self,
        fps: u32,
        frame_bytes: u32,
        max_bytes_per_sec: u32,
    ) -> io::Result<()> {
        let (w, h) = (self.width as u32, self.height as u32);
        let mut b: Vec<u8> = Vec::with_capacity(HEADER_LEN as usize);

        // ---- RIFF / AVI ----
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&0u32.to_le_bytes()); // riff size (patched)
        b.extend_from_slice(b"AVI ");

        // ---- LIST hdrl ----
        b.extend_from_slice(b"LIST");
        b.extend_from_slice(&294u32.to_le_bytes()); // fixed hdrl size
        b.extend_from_slice(b"hdrl");

        // avih (MainAVIHeader, 56 bytes)
        b.extend_from_slice(b"avih");
        b.extend_from_slice(&56u32.to_le_bytes());
        b.extend_from_slice(&(1_000_000u32 / fps).to_le_bytes()); // dwMicroSecPerFrame
        b.extend_from_slice(&max_bytes_per_sec.to_le_bytes()); // dwMaxBytesPerSec
        b.extend_from_slice(&0u32.to_le_bytes()); // dwPaddingGranularity
        b.extend_from_slice(&AVIF_HASINDEX.to_le_bytes()); // dwFlags
        b.extend_from_slice(&0u32.to_le_bytes()); // dwTotalFrames (patched)
        b.extend_from_slice(&0u32.to_le_bytes()); // dwInitialFrames
        b.extend_from_slice(&2u32.to_le_bytes()); // dwStreams
        b.extend_from_slice(&frame_bytes.to_le_bytes()); // dwSuggestedBufferSize
        b.extend_from_slice(&w.to_le_bytes()); // dwWidth
        b.extend_from_slice(&h.to_le_bytes()); // dwHeight
        b.extend_from_slice(&[0u8; 16]); // dwReserved[4]

        // ---- LIST strl (video) ----
        b.extend_from_slice(b"LIST");
        b.extend_from_slice(&116u32.to_le_bytes()); // fixed
        b.extend_from_slice(b"strl");
        // strh (AVIStreamHeader, 56 bytes)
        b.extend_from_slice(b"strh");
        b.extend_from_slice(&56u32.to_le_bytes());
        b.extend_from_slice(b"vids");
        b.extend_from_slice(b"DIB "); // fccHandler
        b.extend_from_slice(&0u32.to_le_bytes()); // dwFlags
        b.extend_from_slice(&0u16.to_le_bytes()); // wPriority
        b.extend_from_slice(&0u16.to_le_bytes()); // wLanguage
        b.extend_from_slice(&0u32.to_le_bytes()); // dwInitialFrames
        b.extend_from_slice(&1u32.to_le_bytes()); // dwScale
        b.extend_from_slice(&fps.to_le_bytes()); // dwRate
        b.extend_from_slice(&0u32.to_le_bytes()); // dwStart
        b.extend_from_slice(&0u32.to_le_bytes()); // dwLength (patched)
        b.extend_from_slice(&frame_bytes.to_le_bytes()); // dwSuggestedBufferSize
        b.extend_from_slice(&0u32.to_le_bytes()); // dwQuality
        b.extend_from_slice(&0u32.to_le_bytes()); // dwSampleSize
        b.extend_from_slice(&0i16.to_le_bytes()); // rcFrame.left
        b.extend_from_slice(&0i16.to_le_bytes()); // rcFrame.top
        b.extend_from_slice(&(self.width as i16).to_le_bytes()); // right
        b.extend_from_slice(&(self.height as i16).to_le_bytes()); // bottom
        // strf (BITMAPINFOHEADER, 40 bytes)
        b.extend_from_slice(b"strf");
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes()); // biSize
        b.extend_from_slice(&(w as i32).to_le_bytes()); // biWidth
        b.extend_from_slice(&(h as i32).to_le_bytes()); // biHeight (>0 = bottom-up)
        b.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
        b.extend_from_slice(&24u16.to_le_bytes()); // biBitCount
        b.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
        b.extend_from_slice(&frame_bytes.to_le_bytes()); // biSizeImage
        b.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
        b.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
        b.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
        b.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

        // ---- LIST strl (audio) ----
        b.extend_from_slice(b"LIST");
        b.extend_from_slice(&94u32.to_le_bytes()); // fixed
        b.extend_from_slice(b"strl");
        // strh (56 bytes)
        b.extend_from_slice(b"strh");
        b.extend_from_slice(&56u32.to_le_bytes());
        b.extend_from_slice(b"auds");
        b.extend_from_slice(&1u32.to_le_bytes()); // fccHandler = PCM
        b.extend_from_slice(&0u32.to_le_bytes()); // dwFlags
        b.extend_from_slice(&0u16.to_le_bytes()); // wPriority
        b.extend_from_slice(&0u16.to_le_bytes()); // wLanguage
        b.extend_from_slice(&0u32.to_le_bytes()); // dwInitialFrames
        b.extend_from_slice(&1u32.to_le_bytes()); // dwScale
        b.extend_from_slice(&self.audio_rate.to_le_bytes()); // dwRate
        b.extend_from_slice(&0u32.to_le_bytes()); // dwStart
        b.extend_from_slice(&0u32.to_le_bytes()); // dwLength (patched, in samples)
        b.extend_from_slice(&0u32.to_le_bytes()); // dwSuggestedBufferSize
        b.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // dwQuality
        b.extend_from_slice(&(AUDIO_BLOCK_ALIGN as u32).to_le_bytes()); // dwSampleSize
        b.extend_from_slice(&[0u8; 8]); // rcFrame (unused)
        // strf (WAVEFORMATEX, 18 bytes)
        b.extend_from_slice(b"strf");
        b.extend_from_slice(&18u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // wFormatTag = PCM
        b.extend_from_slice(&AUDIO_CHANNELS.to_le_bytes());
        b.extend_from_slice(&self.audio_rate.to_le_bytes()); // nSamplesPerSec
        b.extend_from_slice(&(self.audio_rate * AUDIO_BLOCK_ALIGN as u32).to_le_bytes()); // nAvgBytesPerSec
        b.extend_from_slice(&AUDIO_BLOCK_ALIGN.to_le_bytes());
        b.extend_from_slice(&AUDIO_BITS.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes()); // cbSize

        // ---- LIST movi (open) ----
        b.extend_from_slice(b"LIST");
        b.extend_from_slice(&0u32.to_le_bytes()); // movi size (patched)
        b.extend_from_slice(b"movi");

        debug_assert_eq!(b.len() as u64, HEADER_LEN, "AVI header layout drifted");
        self.file.write_all(&b)?;
        self.pos = b.len() as u64;
        Ok(())
    }

    /// Bytes written to the file so far (the `idx1` appended at finalize adds a
    /// further `8 + 16 * chunk_count`, which is tiny next to the raw frames).
    pub fn bytes_written(&self) -> u64 {
        self.pos
    }

    /// Number of video frames written so far.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Expand and append one indexed frame as an uncompressed 24-bit BGR chunk.
    pub fn write_video(&mut self, fb: &FrameBuffer, pal: &Palette) -> io::Result<()> {
        let w = self.width as usize;
        let h = self.height as usize;
        let pixels = fb.pixels();
        // DIB rows run bottom-to-top; each pixel is BGR.
        let mut o = 0;
        for y in (0..h).rev() {
            let row = &pixels[y * w..y * w + w];
            for &idx in row {
                let c = pal.get_rgb888(idx as usize);
                self.bgr[o] = c.2;
                self.bgr[o + 1] = c.1;
                self.bgr[o + 2] = c.0;
                o += 3;
            }
        }
        let size = self.bgr.len() as u32;
        let offset = (self.pos - MOVI_FOURCC_POS) as u32;
        self.file.write_all(b"00db")?;
        self.file.write_all(&size.to_le_bytes())?;
        // `bgr` is borrowed from self, so take it out for the write.
        let bgr = std::mem::take(&mut self.bgr);
        self.file.write_all(&bgr)?;
        self.bgr = bgr;
        self.pos += 8 + size as u64;
        // Frame bytes (w*h*3) are always even, so no padding is needed.
        self.index.push(IdxEntry {
            ckid: *b"00db",
            flags: AVIIF_KEYFRAME,
            offset,
            size,
        });
        self.frame_count += 1;
        Ok(())
    }

    /// Append a block of interleaved 16-bit PCM stereo audio (`01wb`).
    pub fn write_audio(&mut self, pcm_s16le: &[u8]) -> io::Result<()> {
        if pcm_s16le.is_empty() {
            return Ok(());
        }
        let size = pcm_s16le.len() as u32;
        let offset = (self.pos - MOVI_FOURCC_POS) as u32;
        self.file.write_all(b"01wb")?;
        self.file.write_all(&size.to_le_bytes())?;
        self.file.write_all(pcm_s16le)?;
        self.pos += 8 + size as u64;
        // Pad to an even boundary if the block size is odd.
        if size % 2 == 1 {
            self.file.write_all(&[0u8])?;
            self.pos += 1;
        }
        self.index.push(IdxEntry {
            ckid: *b"01wb",
            flags: 0,
            offset,
            size,
        });
        self.audio_frames += size / AUDIO_BLOCK_ALIGN as u32;
        Ok(())
    }

    /// Write the `idx1` index and patch the header size/count fields.
    pub fn finalize(mut self) -> io::Result<()> {
        // idx1 chunk: one 16-byte entry per chunk.
        let idx_bytes = (self.index.len() * 16) as u32;
        let movi_end = self.pos; // movi data ends here
        self.file.write_all(b"idx1")?;
        self.file.write_all(&idx_bytes.to_le_bytes())?;
        for e in &self.index {
            self.file.write_all(&e.ckid)?;
            self.file.write_all(&e.flags.to_le_bytes())?;
            self.file.write_all(&e.offset.to_le_bytes())?;
            self.file.write_all(&e.size.to_le_bytes())?;
        }
        let total_len = movi_end + 8 + idx_bytes as u64;

        let mut file = self.file.into_inner()?;
        let patch = |file: &mut File, at: u64, v: u32| -> io::Result<()> {
            file.seek(SeekFrom::Start(at))?;
            file.write_all(&v.to_le_bytes())
        };
        patch(&mut file, P_RIFF_SIZE, (total_len - 8) as u32)?;
        patch(&mut file, P_TOTAL_FRAMES, self.frame_count)?;
        patch(&mut file, P_VID_LENGTH, self.frame_count)?;
        patch(&mut file, P_AUD_LENGTH, self.audio_frames)?;
        patch(&mut file, P_MOVI_SIZE, (movi_end - MOVI_FOURCC_POS) as u32)?;
        file.flush()?;
        Ok(())
    }
}

/// The file name for segment `seg` of a recording. Segment 0 keeps `base`
/// verbatim; later segments insert a `-NN` suffix before the extension
/// (`clip.avi` → `clip-02.avi`, `clip-03.avi`, …).
fn segment_path(base: &Path, seg: u32) -> PathBuf {
    if seg == 0 {
        return base.to_path_buf();
    }
    let stem = base
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dune-rec");
    let ext = base.extension().and_then(|s| s.to_str()).unwrap_or("avi");
    base.with_file_name(format!("{stem}-{:02}.{ext}", seg + 1))
}

/// An [`AviWriter`] that rolls over to a fresh file before crossing `max_bytes`,
/// so a long recording never exceeds classic AVI's ~2 GB limit. Each segment is
/// an independent, standard AVI; concatenate them (e.g. `ffmpeg -f concat`) for
/// one clip. Rollover is transparent to the caller.
pub struct SegmentedAviWriter {
    base: PathBuf,
    width: u16,
    height: u16,
    fps: u32,
    audio_rate: u32,
    max_bytes: u64,
    frame_bytes: u64,
    seg: u32,
    writer: Option<AviWriter>,
}

impl SegmentedAviWriter {
    pub fn new(
        base: &Path,
        width: u16,
        height: u16,
        fps: u32,
        audio_rate: u32,
        max_bytes: u64,
    ) -> io::Result<Self> {
        let writer = AviWriter::new(&segment_path(base, 0), width, height, fps, audio_rate)?;
        Ok(Self {
            base: base.to_path_buf(),
            width,
            height,
            fps,
            audio_rate,
            max_bytes,
            frame_bytes: width as u64 * height as u64 * 3,
            seg: 0,
            writer: Some(writer),
        })
    }

    /// Number of segment files this recording has produced.
    pub fn segment_count(&self) -> u32 {
        self.seg + 1
    }

    /// Append a video frame, rolling to a new segment first if this frame would
    /// push the current file past `max_bytes`. A segment always holds at least
    /// one frame, so a pathologically small `max_bytes` can't spin.
    pub fn write_video(&mut self, fb: &FrameBuffer, pal: &Palette) -> io::Result<()> {
        let roll = self
            .writer
            .as_ref()
            .map(|w| {
                w.frame_count() > 0 && w.bytes_written() + self.frame_bytes + 8 >= self.max_bytes
            })
            .unwrap_or(false);
        if roll {
            self.roll()?;
        }
        if let Some(w) = self.writer.as_mut() {
            w.write_video(fb, pal)?;
        }
        Ok(())
    }

    /// Append audio into the current segment.
    pub fn write_audio(&mut self, pcm_s16le: &[u8]) -> io::Result<()> {
        if let Some(w) = self.writer.as_mut() {
            w.write_audio(pcm_s16le)?;
        }
        Ok(())
    }

    /// Finalise the last segment.
    pub fn finalize(mut self) -> io::Result<()> {
        if let Some(w) = self.writer.take() {
            w.finalize()?;
        }
        Ok(())
    }

    fn roll(&mut self) -> io::Result<()> {
        // Open the next segment before finalising the current one, so a failure
        // to create it leaves the current writer intact.
        let next = AviWriter::new(
            &segment_path(&self.base, self.seg + 1),
            self.width,
            self.height,
            self.fps,
            self.audio_rate,
        )?;
        self.seg += 1;
        if let Some(old) = self.writer.replace(next) {
            old.finalize()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;
    use crate::color::Color;

    fn have(tool: &str) -> bool {
        Command::new(tool)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Write a synthetic AVI with the muxer and confirm ffprobe/ffmpeg accept it:
    /// correct dimensions and stream count, and a clean full decode (which
    /// exercises the header layout and the idx1 offsets). Ignored by default
    /// because it needs ffmpeg/ffprobe on PATH.
    #[test]
    #[ignore = "needs ffmpeg + ffprobe on PATH; writes a temp file"]
    fn writes_a_valid_avi() {
        if !have("ffmpeg") || !have("ffprobe") {
            eprintln!("skipping: ffmpeg/ffprobe not found");
            return;
        }
        let out = std::env::temp_dir().join("dune-avi-selftest.avi");
        let _ = std::fs::remove_file(&out);

        let rate = 48000u32;
        let fps = 30u32;
        let mut w = AviWriter::new(&out, 320, 200, fps, rate).expect("create avi");

        // A red frame and ~1s of a 440 Hz tone, interleaved per frame.
        let mut fb = FrameBuffer::new(320, 200);
        for p in fb.pixels_mut() {
            *p = 1;
        }
        let mut pal = Palette::new();
        pal.set(1, Color(63, 0, 0));

        let samples_per_frame = rate / fps;
        let mut phase = 0u32;
        for _ in 0..fps {
            w.write_video(&fb, &pal).expect("video");
            let mut pcm = Vec::with_capacity(samples_per_frame as usize * 4);
            for _ in 0..samples_per_frame {
                let t = phase as f32 / rate as f32;
                let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 0.3 * i16::MAX as f32) as i16;
                pcm.extend_from_slice(&s.to_le_bytes()); // L
                pcm.extend_from_slice(&s.to_le_bytes()); // R
                phase += 1;
            }
            w.write_audio(&pcm).expect("audio");
        }
        w.finalize().expect("finalize");

        // Dimensions + stream count.
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,width,height",
                "-of",
                "csv=p=0",
            ])
            .arg(&out)
            .output()
            .expect("ffprobe");
        let info = String::from_utf8_lossy(&probe.stdout);
        assert!(info.contains("video"), "no video stream: {info:?}");
        assert!(info.contains("audio"), "no audio stream: {info:?}");
        assert!(
            info.contains("320") && info.contains("200"),
            "wrong size: {info:?}"
        );

        // A full decode must succeed without errors (validates idx1 + layout).
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

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn segment_paths_number_after_the_first() {
        let base = Path::new("/tmp/clip.avi");
        assert_eq!(segment_path(base, 0), PathBuf::from("/tmp/clip.avi"));
        assert_eq!(segment_path(base, 1), PathBuf::from("/tmp/clip-02.avi"));
        assert_eq!(segment_path(base, 2), PathBuf::from("/tmp/clip-03.avi"));
    }

    /// With a small `max_bytes`, a SegmentedAviWriter must roll to fresh files,
    /// each an independently decodable AVI holding at least one frame.
    #[test]
    #[ignore = "needs ffmpeg on PATH; writes temp files"]
    fn segments_roll_over_and_each_decodes() {
        if !have("ffmpeg") {
            eprintln!("skipping: ffmpeg not found");
            return;
        }
        let base = std::env::temp_dir().join("dune-avi-segtest.avi");
        for seg in 0..8 {
            let _ = std::fs::remove_file(segment_path(&base, seg));
        }

        // One 320x200 24-bit frame is 192000 bytes; a 500 KB cap → ~2 frames each.
        let mut seg = SegmentedAviWriter::new(&base, 320, 200, 30, 48000, 500_000).unwrap();
        let mut fb = FrameBuffer::new(320, 200);
        for p in fb.pixels_mut() {
            *p = 1;
        }
        let mut pal = Palette::new();
        pal.set(1, Color(0, 0, 63));
        for _ in 0..6 {
            seg.write_video(&fb, &pal).unwrap();
            seg.write_audio(&[0u8; 3200]).unwrap(); // a little silence
        }
        let n = seg.segment_count();
        seg.finalize().unwrap();
        assert!(n >= 2, "expected multiple segments, got {n}");

        for i in 0..n {
            let path = segment_path(&base, i);
            let decode = Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&path)
                .args(["-f", "null", "-"])
                .output()
                .expect("ffmpeg decode");
            assert!(
                decode.status.success() && decode.stderr.is_empty(),
                "segment {i} decode errors: {}",
                String::from_utf8_lossy(&decode.stderr)
            );
            let _ = std::fs::remove_file(&path);
        }
    }
}
