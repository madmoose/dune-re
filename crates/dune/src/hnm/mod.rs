#![allow(unused)]

mod frame_header;
mod hnm_decoder;

use std::io::{Cursor, Seek};

use bytes_ext::ReadBytesExt;
pub use hnm_decoder::HnmDecoder;

use crate::{GameState, blit, hnm::frame_header::FrameHeader, hsq};

pub(crate) const DFL2_HNM: usize = 1;
pub(crate) const MNT1_HNM: usize = 2;
pub(crate) const MNT2_HNM: usize = 3;
pub(crate) const MNT3_HNM: usize = 4;
pub(crate) const MNT4_HNM: usize = 5;
pub(crate) const SIET_HNM: usize = 6;
pub(crate) const PALACE_HNM: usize = 7;
pub(crate) const PALACE_HNM2: usize = 8;
pub(crate) const FORT_HNM: usize = 9;
pub(crate) const FORT_HNM2: usize = 10;
pub(crate) const DEAD3_HNM: usize = 11;
pub(crate) const DEAD_HNM: usize = 12;
pub(crate) const DEAD2_HNM: usize = 13;
pub(crate) const VER_HNM: usize = 14;
pub(crate) const TITLE_HNM: usize = 15;
pub(crate) const MTG1_HNM: usize = 16;
pub(crate) const MTG2_HNM: usize = 17;
pub(crate) const MTG3_HNM: usize = 18;
pub(crate) const PLANT_HNM: usize = 19;
pub(crate) const CREDITS_HNM: usize = 20;
pub(crate) const VIRGIN_HNM: usize = 21;
pub(crate) const CRYO_HNM: usize = 22;
pub(crate) const CRYO2_HNM: usize = 23;
pub(crate) const PRESENT_HNM: usize = 24;
pub(crate) const IRULAN_HNM: usize = 25;
pub(crate) const SEQA_HNM: usize = 26;
pub(crate) const SEQL_HNM: usize = 27;
pub(crate) const SEQM_HNM: usize = 28;
pub(crate) const SEQP_HNM: usize = 29;
pub(crate) const SEQQ_HNM: usize = 30;
pub(crate) const SEQJ_HNM: usize = 31;
pub(crate) const SEQK_HNM: usize = 32;
pub(crate) const SEQI_HNM: usize = 33;
pub(crate) const SEQD_HNM: usize = 34;
pub(crate) const SEQN_HNM: usize = 35;
pub(crate) const SEQR_HNM: usize = 36;

#[derive(Clone)]
struct HNMResource {
    data: u16,
    name: &'static str,
}

const fn res(data: u16, name: &'static str) -> HNMResource {
    HNMResource { data, name }
}

// = seg001:33a3 RESOURCE_LIST_HNM — indexed by HNM video id. Index 0 is the
// RES_BLANK_ENTRY placeholder, so the table is 1-based and the DFL2_HNM..SEQR_HNM
// ids above index straight into it.
const HNM_RESOURCES: [HNMResource; 37] = [
    res(0x0000, ""),
    res(0x1221, "DFL2.HNM"),
    res(0x109d, "MNT1.HNM"),
    res(0x109d, "MNT2.HNM"),
    res(0x109d, "MNT3.HNM"),
    res(0x109d, "MNT4.HNM"),
    res(0x109c, "SIET.HNM"),
    res(0x109c, "PALACE.HNM"),
    res(0x109c, "PALACE.HNM"),
    res(0x1090, "FORT.HNM"),
    res(0x1090, "FORT.HNM"),
    res(0x0a00, "DEAD3.HNM"),
    res(0x4200, "DEAD.HNM"),
    res(0x0a00, "DEAD2.HNM"),
    res(0x1900, "VER.HNM"),
    res(0x0a00, "TITLE.HNM"),
    res(0x1090, "MTG1.HNM"),
    res(0x1090, "MTG2.HNM"),
    res(0x1090, "MTG3.HNM"),
    res(0x1910, "PLANT.HNM"),
    res(0x0740, "CREDITS.HNM"),
    res(0x1000, "VIRGIN.HNM"),
    res(0x1100, "CRYO.HNM"),
    res(0x0400, "CRYO2.HNM"),
    res(0x1600, "PRESENT.HNM"),
    res(0x1080, "IRULAN.HNM"),
    res(0x1080, "SEQA.HNM"),
    res(0x1080, "SEQL.HNM"),
    res(0x1080, "SEQM.HNM"),
    res(0x1080, "SEQP.HNM"),
    res(0x1080, "SEQQ.HNM"),
    res(0x1080, "SEQJ.HNM"),
    res(0x1080, "SEQK.HNM"),
    res(0x1080, "SEQI.HNM"),
    res(0x1080, "SEQD.HNM"),
    res(0x1080, "SEQN.HNM"),
    res(0x1080, "SEQR.HNM"),
];

impl GameState {
    // = seg000:c92b
    fn hnm_open(&mut self, id: u16) {
        self.hnm_video_id = id;

        self.hnm_close();
        self.hnm_reset_buffers();
        self.hnm_finished = false;
        self.hnm_reset_frame_counters();
        self.hnm_read_header();
    }

    // = seg000:c93c hnm_read_header — open the resource for the active video id
    // and parse its header. The header is laid out as {size:u16, palette, 0xff
    // padding, frame-offset table}. DOS streams it through the scratch buffer;
    // the single-buffer port reads the whole resource into hnm_bytes and parses
    // it in place.
    pub(crate) fn hnm_read_header(&mut self) {
        // = c93c
        self.hnm_active_video_id = self.hnm_video_id;

        // = c942 hnm_get_res_entry_by_index / c945: entry->unk0 is the resource
        // flag word (current_hnm_resource_flag, seg001:dbfe). The HIGH byte is the
        // playback rate for clips without SD audio (see hnm_load_first_frame). The
        // LOW byte is a decode/streaming bitfield:
        //   bit 0 (0x01)  loop: at the 'mm' end marker rewind to the body instead
        //                 of finishing (cb61; hnm_step_frame).
        //   bit 1 (0x02)  unused.
        //   bit 2 (0x04)  alternate frame-offset slot (+0x10, c9b6, below) and, on
        //                 loop, a companion resource video_id+0x61 (cbb8).
        //   bit 3 (0x08)  loop clip-chain/redirect with a stored rect (cb88).
        //   bits 4-5 (0x30) full-screen copy modes that reroute the frame blit
        //                 (ccae): bit 4 -> loc_04afd (copy whole fb1 to screen),
        //                 bit 5 -> loc_04aeb (VER text handler).
        //   bit 6 (0x40)  stage to the back buffer instead of framebuffer_1 (cac3)
        //                 and skip the prefetch warm-up (ca48).
        //   bit 7 (0x80)  streaming/prefetch-ahead mode: extra prefetch + skip the
        //                 0x800-byte read alignment (ca67/cb20).
        // The resident single-buffer port only acts on bit 0 (loop) and bit 2's
        // offset slot; the rest drive the streaming reader / full-screen-copy
        // paths it does not model. The only scaled clips (IRULAN/SEQ*) set just
        // bit 7, which the port does not need.
        let res = &HNM_RESOURCES[self.hnm_video_id as usize];
        self.hnm_resource_data = res.data;

        // = c94d open_res_or_file_or_die. Single-buffer port: read it all now.
        let bytes = self
            .dat_file
            .read_raw(res.name)
            .unwrap_or_else(|e| panic!("Failed to open HNM resource {}: {e}", res.name));

        // = c96b hnm_read_header_size: first word = total header size. The frame
        // offsets in the table are relative to the end of the header.
        let header_size = read_le_u16(&bytes, 0);
        self.hnm_header_size = header_size;

        // = c9a9 apply_palette + c9ad: the palette follows the size word.
        // Palette::apply_palette_update applies the entries AND skips the trailing
        // 0xff padding, returning the bytes consumed, so the frame-offset table
        // begins right after.
        let pal_size = self.apply_palette_update(&bytes[2..]) as usize;
        let table = 2 + pal_size;

        // = c9b4..c9bd: the first frame is table slot 0, or the slot 0x10 bytes
        // in (entry index 4) when resource flag bit 2 is set.
        let slot = if self.hnm_resource_data & 4 != 0 {
            0x10
        } else {
            0
        };

        // = c9bf: rel = first-frame offset, relative to the header end.
        let rel = read_le_u32(&bytes, table + slot) as usize;

        // = c9c6: cache the absolute first-frame position for hnm_prefetch. DOS
        // adds the file offset (already advanced past the header by header_size);
        // the resource buffer starts at 0 here, so the body sits at
        // header_size + rel.
        self.hnm_body_offset = header_size as usize + rel;

        self.hnm_bytes = Some(bytes);
    }

    // = seg000:cda0 hnm_open_at_body_start — position the reader at the first
    // frame to play. DOS reads the frame's size word into the top of the scratch
    // buffer and seeks the file there; in the single-buffer port the body is
    // already resident, so seat the read cursor at the cached body offset.
    fn hnm_open_at_body_start(&mut self) {
        self.hnm_reset_buffers();
        self.hnm_read_offset = self.hnm_body_offset;
    }

    // = seg000:ca01
    pub(crate) fn hnm_close(&mut self) {
        if self.hnm_bytes.is_none() {
            return;
        }

        // = the seg000 close path zeroing video_decode_buf_seg — drop any
        // prefetched pipeline frame with the stream.
        self.hnm_video_frame_ready = false;
        self.hnm_reset_frame_counters();
        self.hnm_bytes = None;
    }

    pub fn hnm_restart(&mut self) {
        self.hnm_reset_buffers();
    }

    /// True while a clip is open (single-buffer port).
    pub fn hnm_is_open(&self) -> bool {
        self.hnm_bytes.is_some()
    }

    /// True once a non-looping clip has played its last frame (= hnm_finished_flag).
    pub fn hnm_finished(&self) -> bool {
        self.hnm_finished
    }

    // = seg000:ce01 hnm_reset_frame_counter (falls through into ce07
    // hnm_reset_counters). Zeroes the frame counters and disarms the loop-point
    // frame count; hnm_counter_3 (the saved loop length, seg000:cb76) has no
    // ported reader.
    fn hnm_reset_frame_counters(&mut self) {
        self.hnm_frame_counter = 0;
        // = ce07 hnm_counter_2 = 0; ce13 hnm_counter_4 = -1.
        self.hnm_counter_2 = 0;
        self.hnm_counter_4 = 0xffff;
    }

    // = seg000:ce1a hnm_reset_buffers. DOS repoints its scratch read/decode
    // buffers and clears the cursors; the port only needs the read cursor reset
    // and the decode target reset to the saved framebuffer. Any spliced .LOP
    // bridge records live in the DOS stream scratch this repoints, so pending
    // bridge chunks are dropped with it.
    fn hnm_reset_buffers(&mut self) {
        self.hnm_read_offset = 0;
        self.hnm_framebuffer = crate::FbId::Saved;
        self.hnm_lop_remaining = 0;
    }

    // = seg000:ca1b hnm_load_first_frame — open the resource, seek to the first
    // frame and decode it. The trailing DOS prefetch warm-up loop (ca48..ca57)
    // belongs to the streaming reader the single-buffer port omits. (Named to
    // avoid clashing with the existing HnmDecoder-based `hnm_load_first_frame`.)
    pub fn hnm_open_and_decode_first_frame(&mut self, id: u16) {
        // = ca1b call hnm_open_and_load_palette — store id, reset state, read
        // the header and apply the header palette.
        self.hnm_open(id);
        // = ca20 call hnm_open_at_body_start — seat the cursor at the body.
        self.hnm_open_at_body_start();
        // = ca2a..ca3a decode the first frame. (ca37 decode_sd_block — HNM audio
        // is not ported yet.)
        let _ = self.hnm_decode_frame();
        // = ca40 advance to the next frame. DOS calls hnm_reset_buffers here
        // (ca3d) to clear the streaming scratch and bumps hnm_frame_counter; in
        // the single-buffer port the only state to carry forward is the body
        // cursor, which hnm_advance_to_next_frame steps past frame 0.
        self.hnm_advance_to_next_frame();
    }

    // = seg000:ca60 hnm_do_frame — display the current frame and step to the
    // next one. The DOS prefetch/buffering (ca71 loc_0caa0, ca76 hnm_prefetch)
    // and PIT/PCM pacing (ca7b hnm_wait_for_frame) belong to the streaming reader
    // the single-buffer port omits; pacing is the caller's frame-task job, as in
    // the HnmDecoder path. (Named to avoid clashing with the existing
    // HnmDecoder-based `hnm_do_frame`.)
    pub fn hnm_step_frame(&mut self) -> bool {
        // = ca60: nothing to do once the clip is closed/finished.
        if self.hnm_bytes.is_none() {
            return false;
        }

        // = loc_0cabf: later frames stage to framebuffer_1, or the back buffer
        // when resource flag bit 6 is set. The port has no back buffer, so it
        // records Fb1 either way (the composite still lands in framebuffer_active;
        // see hnm_decode_frame).
        self.hnm_framebuffer = crate::FbId::Fb1;

        // = loc_0caa0/cab4: the body ends with an 'mm' (0x6d6d) record. Reaching
        // it on a looping clip (resource flag bit 0) rewinds to the body start;
        // a non-looping clip that somehow steps onto it is finished. Normal play
        // sets hnm_finished after the last real frame below, so this is a guard.
        // = seg000:cb00 reaching hnm_counter_4 frames since the last loop point
        // counts as the loop point too — hnm_switch_active_video arms it so the
        // orni arrival redirects into the approach clip at an exact mid-loop
        // frame (travel_arrival_landing_sequence, seg000:488a).
        if self.hnm_next_record_is_loop_marker() || self.hnm_counter_2 == self.hnm_counter_4 {
            if self.hnm_resource_data & 1 != 0 {
                // = seg000:cb70..cb76 counter_3 = counter_2, then
                // hnm_reset_counters — restart the per-loop record count and
                // disarm the loop-point frame count. counter_3 (the latched
                // pass length) drives the display-side advance at
                // seg000:cc6e..cc84, which wraps hnm_frame_counter back to 1
                // once it exceeds counter_3 — so the frame counter counts
                // 1..N within each loop pass. The single-buffer port reads at
                // display time, making the wrap point the loop point itself:
                // reset the frame counter here (the bridge/body decodes below
                // re-count from 1).
                self.hnm_frame_counter = 0;
                self.hnm_counter_2 = 0;
                self.hnm_counter_4 = 0xffff;
                // = seg000:cb7c..cb9f at the loop point a differing
                // hnm_active_video_id (travel_select_flight_video re-picks it
                // per terrain) redirects the stream to that clip when its
                // resource carries flag bit 3 and a cached body offset. DOS
                // jumps straight into the cached body (the cache pass at
                // seg000:ce9f computes it with the same header-slot logic);
                // the single-buffer port re-opens the resource, re-applying
                // its header palette — the flight clips carry empty header
                // palettes, so the switch stays seamless.
                if self.hnm_active_video_id != self.hnm_video_id
                    && HNM_RESOURCES[self.hnm_active_video_id as usize].data & 8 != 0
                {
                    println!(
                        "hnm_step_frame: switching from video id {} to video id {}",
                        self.hnm_video_id, self.hnm_active_video_id
                    );
                    let id = self.hnm_active_video_id;
                    self.hnm_open(id);
                    self.hnm_open_at_body_start();
                } else {
                    self.hnm_read_offset = self.hnm_body_offset;
                }
                // = seg000:cbb8..cc04 the companion splice: when the (possibly
                // just redirected-to) clip carries resource flag bit 2, DOS
                // builds four stream records pointing into its companion
                // resource (video_id + 0x61 = MNT1.LOP .. PALACE.LOP) — the
                // bridge frames played across the loop seam before the body
                // resumes. The port queues them; the decode below plays one
                // per step.
                //
                // TODO: the remaining loop-point jitter suspect is the PIT
                // pacing carry across the rewind, which the port's
                // hnm_last_frame_tick tick pacing does not reproduce.
                if self.hnm_resource_data & 4 != 0 {
                    self.hnm_lop_queue_bridge();
                }
            } else {
                // = loc_0cb4c: mark finished and release the resource.
                self.hnm_finished = true;
                self.hnm_close();
                return false;
            }
        }

        // = seg000:cd37..cd4b the spliced records are consumed before the body
        // resumes: each makes the decoder follow the record's far pointer into
        // the resident .LOP chunk and fall into the video-chunk decode
        // (loc_0cd4e), counted like a stream frame (loc_0cc0c/loc_0cc4e). The
        // port decodes one queued bridge chunk per step.
        if self.hnm_lop_remaining > 0 {
            let bytes = self
                .hnm_lop_bytes
                .take()
                .expect("bridge chunks queued without a .LOP resource");
            let size = read_le_u16(&bytes, self.hnm_lop_cursor) as usize;
            let _ = self.hnm_decode_record(&bytes, self.hnm_lop_cursor);
            self.hnm_lop_bytes = Some(bytes);
            // = seg000:cbfa add bp, ax — the chunks lie head-to-tail, each
            // starting with its self-inclusive size word.
            self.hnm_lop_cursor += size;
            self.hnm_lop_remaining -= 1;
            self.hnm_frame_counter = self.hnm_frame_counter.wrapping_add(1);
            self.hnm_counter_2 = self.hnm_counter_2.wrapping_add(1);
            return true;
        }

        // = ca80..ca89: apply any pending palette chunk and blit the frame (both
        // folded into hnm_decode_frame).
        let _ = self.hnm_decode_frame();
        // = ca8c loc_0cc4e: step the cursor to the next frame.
        self.hnm_advance_to_next_frame();

        // If the next record is the end marker (or the buffer is exhausted) and
        // the clip does not loop, this was the final frame: mark it finished now
        // so hnm_finished()/hnm_is_complete() flip right after the last frame is
        // decoded, matching the streaming decoder's frame-count completion.
        if self.hnm_resource_data & 1 == 0 && self.hnm_next_record_is_loop_marker() {
            self.hnm_finished = true;
        }

        true
    }

    // = seg000:ce4b hnm_switch_active_video — set hnm_active_video_id (DOS bx;
    // the loop point then redirects into that clip, seg000:cb7c) and arm
    // hnm_counter_4 (DOS ax) so the loop point fires at exactly that frame
    // count (seg000:cb00).
    pub(crate) fn hnm_switch_active_video(&mut self, video_id: u16, at_frame: u16) {
        self.hnm_active_video_id = video_id;
        self.hnm_counter_4 = at_frame;
    }

    // = seg000:cbb8..cc04 — queue the four companion bridge chunks for the
    // current clip. The .LOP resource is a small header (a self-inclusive size
    // word + an empty palette block) followed by four size-prefixed video
    // chunks laid head-to-tail; DOS seats the resource pointer past the header
    // (open_resource_by_index reads the first word as the data offset,
    // seg000:c18a) and builds one stream record per chunk (cx = 4, cbcc).
    fn hnm_lop_queue_bridge(&mut self) {
        // = seg000:cbbf..cbc5 companion resource id = video_id + 0x61 — the
        // resource list covers only the flight ids 2..7 (0x63..0x68,
        // MNT1.LOP..PALACE.LOP); no other bit-2 clip reaches a loop point.
        if !(2..=7).contains(&self.hnm_video_id) {
            return;
        }
        // The resident cache: reload only when the clip changed (DOS keeps all
        // six companion resources open across the flight).
        if self.hnm_lop_video_id != self.hnm_video_id || self.hnm_lop_bytes.is_none() {
            let name = HNM_RESOURCES[self.hnm_video_id as usize]
                .name
                .replace(".HNM", ".LOP");
            let bytes = self
                .dat_file
                .read_raw(&name)
                .unwrap_or_else(|e| panic!("Failed to open LOP resource {name}: {e}"));
            self.hnm_lop_bytes = Some(bytes);
            self.hnm_lop_video_id = self.hnm_video_id;
        }
        // The first chunk sits right past the header (its size word).
        self.hnm_lop_cursor = read_le_u16(self.hnm_lop_bytes.as_deref().unwrap(), 0) as usize;
        // = seg000:cbcc mov cx, 4.
        self.hnm_lop_remaining = 4;
    }

    // = seg000:loc_0cc4e — step past the current frame. DOS advances its consume
    // cursor by the frame's size word and decrements the buffered-byte count; the
    // single-buffer port only advances the body cursor and bumps the frame
    // counters.
    fn hnm_advance_to_next_frame(&mut self) {
        let frame_size = read_le_u16(self.hnm_bytes.as_deref().unwrap(), self.hnm_read_offset);
        self.hnm_read_offset += frame_size as usize;
        self.hnm_frame_counter = self.hnm_frame_counter.wrapping_add(1);
        // = seg000:cc26 (and ca44 for frame 0) — one more frame record
        // consumed since the last loop point.
        self.hnm_counter_2 = self.hnm_counter_2.wrapping_add(1);
    }

    // True when the record at the read cursor is the 'mm' end-of-stream marker:
    // a size word followed by the tag 0x6d6d (seg000:cab4 `cmp word es:[si], 6d6dh`).
    // A cursor past the end of the resident resource counts as the end too.
    fn hnm_next_record_is_loop_marker(&self) -> bool {
        let bytes = self.hnm_bytes.as_deref().unwrap();
        let tag = self.hnm_read_offset + 2;
        match bytes.get(tag..tag + 2) {
            Some(b) => b == [0x6d, 0x6d],
            None => true,
        }
    }

    // = seg000:ccf4 hnm_decode_typed_chunk_video_to_bp — the deferred two-stage
    // DOS decode collapsed into one pass:
    // hnm_decode_typed_chunk_video_to_bp (seg000:ccf4) scans the frame's blocks
    // and decompresses the video chunk into the staging buffer `bp`, then
    // hnm_decode_video_frame (seg000:cc96) blits that chunk onto framebuffer_active.
    //
    // Output framebuffer: the chunk lands in framebuffer_active (= self.active_fb;
    // seg000:ccbb `mov es, framebuffer_active_seg`). The DOS staging segment `bp`
    // is self.hnm_framebuffer (framebuffer_saved for the first frame, framebuffer_1
    // after; the back buffer for resource flag bit 6 is not modelled — the scaled
    // clips never set it, seg000:loc_0cabf).
    //
    // The 1:1 blit (video id < 0x19) decompresses straight onto framebuffer_active,
    // skipping the staging copy. The checkerboard 2x blit (id >= 0x19, seg000:ccd7)
    // needs raw pixels, so it decompresses into the staging buffer first and then
    // spreads it. The full-screen-copy resource modes (flag bits 0x10/0x20,
    // seg000:ccae) are not modelled yet.
    fn hnm_decode_frame(&mut self) -> std::io::Result<(u16, u16)> {
        let bytes = self
            .hnm_bytes
            .take()
            .expect("hnm_decode_frame without an open resource");
        let result = self.hnm_decode_record(&bytes, self.hnm_read_offset);
        self.hnm_bytes = Some(bytes);
        result
    }

    // The body of hnm_decode_frame, parameterised over the source buffer so the
    // spliced .LOP bridge chunks (hnm_step_frame's loop point) decode through
    // the same path: a record is a size word followed by sd/pl/video blocks,
    // and a .LOP chunk is exactly a size word followed by one video block.
    fn hnm_decode_record(&mut self, bytes: &[u8], frame_pos: usize) -> std::io::Result<(u16, u16)> {
        // Tags are stored as the ASCII byte pairs 's''d' / 'p''l'; DOS reads them
        // with a little-endian lodsw (0x6473 / 0x6c70), which is the same bytes
        // read big-endian here.
        const BLOCK_TYPE_SD: u16 = 0x7364;
        const BLOCK_TYPE_PL: u16 = 0x706c;
        const BLOCK_TYPE_MM: u16 = 0x6d6d;

        let target = self.active_fb;
        // The per-clip blit offset (hnm_load_first_frame's `y_offset`), mirroring
        // how DOS shifts the HNM blit destination by fb_base_ofs: full-screen
        // intro logos use 0, the game-area clips (CREDITS/MTG/PLANT/VER) use 24.
        let y_offset = self.hnm_y_offset;

        // = ca2e es:lodsw — the frame opens with its total size word.
        let frame_size = read_le_u16(bytes, frame_pos) as usize;
        let frame_end = frame_pos + frame_size;
        let mut r = Cursor::new(&bytes[frame_pos + 2..frame_end]);

        let mut w = 0u16;
        let mut h = 0u16;
        let mut scratch: Vec<u8> = Vec::new();

        loop {
            let block_type = r.read_be_u16()?;
            match block_type {
                // = ccf4 loc_0cd0c 'sd': digital-audio chunk. Capture its payload
                // (the size word counts the 4-byte block header) for the audio
                // orchestration to wrap as a VOC; hnm_take_sd_block consumes it.
                BLOCK_TYPE_SD => {
                    let block_size = r.read_le_u16()? as usize;
                    let mut sd = vec![0u8; block_size - 4];
                    std::io::Read::read_exact(&mut r, &mut sd)?;
                    self.hnm_sd_block = Some(sd);
                }
                // = ccf4 loc_0cd25 'pl': palette chunk. DOS records its offset and
                // applies it from hnm_handle_pal_chunk (seg000:ce3b); applying it
                // inline here is equivalent.
                BLOCK_TYPE_PL => {
                    let block_size = r.read_le_u16()?;
                    self.palette
                        .apply_palette_update(&r.get_ref()[r.position() as usize..])?;
                    r.seek_relative(block_size as i64 - 4)?;
                }
                // = ccf4 loc_0cd37 'mm': the streaming loop/redirect marker. The
                // whole resource is resident here, so treat it as end-of-frame.
                BLOCK_TYPE_MM => break,
                // = ccf4 loc_0cd4e: the video chunk carrying the frame pixels.
                _ => {
                    r.seek_relative(-2)?;
                    let frame_header = FrameHeader::new(&mut r)?;
                    if frame_header.is_empty() {
                        break;
                    }
                    w = frame_header.width();
                    h = frame_header.height();

                    // = cd6d..cd7c: a compressed chunk is HSQ-packed; unpack it
                    // into scratch before blitting.
                    if frame_header.is_compressed() {
                        r.seek_relative(6)?;
                        {
                            let mut out = Cursor::new(&mut scratch);
                            hsq::unhsq(r, &mut out)?;
                        }
                        r = Cursor::new(&scratch);
                    }

                    // Full-frame chunks land at the origin; partial chunks carry
                    // an (x, y) blit offset.
                    let (x, y) = if frame_header.is_full_frame() {
                        (0, 0)
                    } else {
                        (r.read_le_i16()?, r.read_le_i16()?)
                    };

                    let data = &r.get_ref()[r.position() as usize..];

                    // = seg000:ccd7 cmp ax, 19h / jnb: clips with video id >=
                    // 0x19 (IRULAN.HNM and the SEQ* talking heads) blit through
                    // the checkerboard 2x path (loc_0cce3 ->
                    // gfx_vtable_vga_blit_checkerboard -> vga_blit_checkerboard,
                    // segvga:0133) instead of the 1:1 blit at ss:[38c9h].
                    if self.hnm_video_id >= 0x19 {
                        // vga_blit_checkerboard spreads the WxH frame across a
                        // 2Wx2H region: each source pixel is written at the
                        // even-column/even-row destination (x + 2*sx, y + 2*sy)
                        // and the odd positions are left untouched, so the
                        // cleared background shows through the gaps.
                        //
                        // DOS stage 1 decompresses the chunk 1:1 into the
                        // staging buffer `bp` = self.hnm_framebuffer
                        // (framebuffer_saved on the first frame, framebuffer_1
                        // after; loc_0cabf) before stage 2 spreads it. Reuse
                        // that buffer rather than allocating per frame. Clear it
                        // first so a source 0 stays 0 (the Blitter skips 0,
                        // whereas vga_blit_checkerboard's movsb writes every
                        // pixel), then spread it into framebuffer_active.
                        let staging_id = self.hnm_framebuffer;
                        // The staging buffer must differ from framebuffer_active
                        // so the spread reads and writes distinct buffers. The
                        // real IRULAN play renders to the screen (bp != active),
                        // but headless captures render straight into fb1, where
                        // bp would collide — fall back to another scratch buffer.
                        let staging_id = if staging_id == target {
                            if target == crate::FbId::Saved {
                                crate::FbId::Fb1
                            } else {
                                crate::FbId::Saved
                            }
                        } else {
                            staging_id
                        };
                        {
                            let staging = self.fb_mut(staging_id);
                            staging.clear();
                            blit::Blitter::new(data, staging)
                                .size(w, h)
                                .rle(frame_header.is_rle())
                                .pal_offset(frame_header.mode())
                                .draw()?;
                        }

                        let ox = x;
                        let oy = y + y_offset;
                        let (staging, fb) = self.fb_pair_mut(staging_id, target);
                        for sy in 0..h {
                            for sx in 0..w {
                                let dx = ox + 2 * sx as i16;
                                let dy = oy + 2 * sy as i16;
                                if dx >= 0
                                    && dy >= 0
                                    && (dx as u16) < fb.w()
                                    && (dy as u16) < fb.h()
                                {
                                    fb.set(dx as u16, dy as u16, staging.get(sx, sy));
                                }
                            }
                        }
                    } else {
                        // = seg000:ccae the full-screen-copy clips (resource
                        // flag bits 0x30: the MNT* flights and VER) skip the
                        // chunk blit and raw-copy the whole decoded frame
                        // (loc_04afd/loc_04aeb), so their index-0 pixels land
                        // in the buffer instead of reading as transparent.
                        let opaque = self.hnm_resource_data & 0x30 != 0;
                        let fb = self.fb_mut(target);
                        blit::Blitter::new(data, fb)
                            .at(x, y + y_offset)
                            .size(w, h)
                            .rle(frame_header.is_rle())
                            .pal_offset(frame_header.mode())
                            .opaque(opaque)
                            .draw()?;
                    }
                    break;
                }
            }
        }

        Ok((w, h))
    }

    /// Consume the SD (digital-audio) chunk captured by the last decoded frame,
    /// if any. = HnmDecoder::take_sd_block in the streaming decoder.
    pub(crate) fn hnm_take_sd_block(&mut self) -> Option<Vec<u8>> {
        self.hnm_sd_block.take()
    }
}

// = seg001:33a3 RESOURCE_LIST_HNM lookup by file name — resolve a clip name to
// its HNM video id (the 1-based index into HNM_RESOURCES).
pub(crate) fn hnm_id_by_name(name: &str) -> u16 {
    HNM_RESOURCES
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("unknown HNM resource {name}")) as u16
}

// Little-endian reads from the resident resource buffer; the DOS reader pulls
// these out of its streaming scratch buffer instead.
fn read_le_u16(bytes: &[u8], pos: usize) -> u16 {
    u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap())
}

fn read_le_u32(bytes: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{GameState, dat_file::DatFile};

    // The flight-clip loop seam: at the 'mm' loop point the four companion
    // .LOP bridge chunks (seg000:cbb8..cc04) play before the body resumes,
    // counted like stream records (loc_0cc0c/loc_0cc4e). Asset-gated; run
    // with: cargo test -p dune --bin dune -- --ignored hnm_loop_bridge
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn hnm_loop_bridge_frames_play_at_the_loop_point() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        game.hnm_load_first_frame_by_id(2, 0);

        // Step to the loop point (MNT1's body is ~180 frames).
        let mut steps = 1;
        while !game.hnm_next_record_is_loop_marker() && steps < 1000 {
            game.hnm_step_frame();
            steps += 1;
        }
        assert!(steps < 1000, "no loop point found");

        // The loop-point step rewinds, queues the .LOP bridge and decodes its
        // first chunk; three more bridge chunks follow before the body.
        game.hnm_step_frame();
        assert_eq!(
            game.hnm_lop_remaining, 3,
            "bridge not queued at the loop point"
        );
        assert_eq!(game.hnm_counter_2, 1);
        for expected in [2, 1, 0] {
            game.hnm_step_frame();
            assert_eq!(game.hnm_lop_remaining, expected);
        }
        assert_eq!(
            game.hnm_counter_2, 4,
            "bridge frames not counted as stream records"
        );
        // The body cursor sits untouched at its start for the next step.
        assert_eq!(game.hnm_read_offset, game.hnm_body_offset);
    }
}
