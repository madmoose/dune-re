//! Talking-head (portrait lip-sync) overlay used by the intro and in-game
//! dialogue. Mirrors the DOS routines around seg000:9123–9efd that composite
//! an animated face — eyes, brows, mouth — on top of a room or backdrop.
//!
//! A portrait sheet (LETO.HSQ, JESS.HSQ, …) ends with a lip-sync data resource
//! parsed by [`crate::Lipsync`] into three parts:
//!   - **image groups** — each a set of `(sprite, x, y)` that compose one
//!     facial element (DOS `[47cch]`).
//!   - **animations** — sequences of frames, each frame a list of image-group
//!     indices. Animations 0..3 are the ambient idle loops (DOS
//!     `_23C7A_portrait_part_2_animations`, `[47cah]`).
//!   - **lip ids** — the last animation, indexed by mouth value for speech
//!     (DOS `_23C82_portrait_part_3_lip_ids`, `[47d2h]`).
//!
//! Stage 1 (this file): faithful parse + one-frame composite over the room.
//! Stage 2 adds the random idle frame task (`loc_099be`). Stage 3 wires the
//! `.voc` lip-sync stream.

use crate::{GameState, Lipsync, Rect, SpriteSheet, gfx, sprite_blitter};

// = the resource-index → portrait list. DOS opens the head sheet with
// open_spritesheet(al + 2); the resource table at seg001:3203 starts
// [2]=RES_LETO_HSQ, [3]=RES_JESS_HSQ, … so the lip-sync resource id `al`
// indexes this list directly. Names live in DUNE.DAT as <NAME>.HSQ.
const HEAD_NAMES: [&str; 17] = [
    "LETO", // 0x00
    "JESS", // 0x01
    "HAWA", // 0x02
    "IDAH", // 0x03
    "GURN", // 0x04
    "STIL", // 0x05
    "KYNE", // 0x06
    "CHAN", // 0x07
    "HARA", // 0x08
    "BARO", // 0x09
    "FEYD", // 0x0a
    "EMPR", // 0x0b
    "HARK", // 0x0c
    "SMUG", // 0x0d
    "FRM1", // 0x0e
    "FRM2", // 0x0f
    "FRM3", // 0x10
];

const HEAD_SMUG: usize = 0x0d;
// const HEAD_FRM1: usize = 0x0e;
// const HEAD_FRM2: usize = 0x0f;
// const HEAD_FRM3: usize = 0x10;
// The player (Paul). Not an index into HEAD_NAMES — it is the DOS PERS
// sprite-pair index character_id_to_sprite returns for id >= 0x11 (seg000:9193
// `mov al, 2dh`); the portrait resolves to PAUL.HSQ.
const HEAD_PAUL: usize = 0x2d;

// = lip_sync_frame_task (seg000:a7c2) advance cadence. The per-frame time is
// `_word_21D32_audio_time_to_play_28224_samples` (the measured time to play the
// 28224-sample FREQ.HSQ calibration clip) scaled by the fixed-point math
// (×2048, then compared against the high word = ÷65536, i.e. ÷32 overall). The
// machine-dependent calibration cancels out, leaving a fixed cadence of
// 28224 / 32 = 882 source samples per mouth frame (= 22050/25, a 25 fps
// authoring rate). The mouth stream is stepped at this rate, NOT spread over the
// whole clip — so it stays locked to the audio regardless of trailing silence.
const SAMPLES_PER_LIP_FRAME: u64 = 28224 / 32; // = 882

/// In-memory state of the currently-displayed talking head. Bundles the
/// portrait sheet, its parsed lip-sync data, and the room behind it so each
/// frame can restore the backdrop before re-compositing the face. Fields named
/// after the DOS globals they stand in for.
pub struct TalkingHead {
    /// Portrait sprite sheet (e.g. LETO.HSQ).
    pub sheet: SpriteSheet,
    /// Parsed lip-sync data (last resource of `sheet`).
    pub lipsync: Lipsync,

    /// = `_word_23C74_current_lip_sync_resource_id` ([47c4h]).
    pub lip_sync_resource_id: u16,
    /// = `_word_21756_talking_head_id` ([21756h]) — the PERS sprite-pair the
    /// face maps to; used to build the `.voc` filename in Stage 3.
    pub talking_head_id: u16,

    /// = `[1bf0h]` rect (x0, y0, x1, y1). Image-group coords are relative to
    /// (x0, y0); loc_009c7 may shift x0/x1 right by `dx`.
    pub rect: (i16, i16, i16, i16),

    /// = `data_047d0` current mouth value. 0 selects the random idle path
    /// (`loc_0994f`); non-zero selects the speech animation (Stage 3).
    pub mouth: u8,

    /// = `data_047d0` facing/expression index set by character_id_to_sprite.
    /// 0 for the named-character heads (random idle); for the player (Paul) it
    /// is a game_time-derived index (`char_to_sprite_player`, loc_0917a) that pins BOTH the lively and
    /// the calm idle to animation `facing-1` (`loc_0994f` / `loc_09a7b`
    /// `(facing-1)*2` branch) instead of random. Grows with game_time, so late
    /// game selects the higher (blue-eyed) idle animations.
    pub facing: u8,

    /// Current ambient animation index and frame within it (the idle loop the
    /// `loc_099be` frame task walks). For idle these stay in animations 0..3.
    pub anim: usize,
    pub frame: usize,

    /// = `data_047d1 & 0x10` — the idle has "settled". The lively idle
    /// animations (0..3) move the mouth; once the [47ceh] idle countdown expires
    /// (`loc_09a3b`) the animator switches to the calm resting expression
    /// (`loc_09a7b` animation 4 = the last idle animation), which holds the mouth
    /// closed and only blinks the eyes. The port sets this when a voice line
    /// finishes so the mouth stops moving once the head goes quiet.
    pub settled: bool,

    /// = `data_047ce` — the lively-idle budget, decremented each idle frame
    /// (`loc_099f6`). It starts at `data_0478c * 4` (loc_09908); `data_0478c` is
    /// 0, so the budget is spent on the first animation and the head settles
    /// (`loc_09a1d` `cmp data_047ce,0; js loc_09a3b`) into the calm resting
    /// expression at the next animation boundary — giving the long rest-hold
    /// pauses between idle gestures.
    pub idle_countdown: i32,

    /// Speech (lip-sync) mouth stream from the `.voc` type-5 comment block —
    /// one mouth value per frame, 0xFF stripped. Empty when no voice is
    /// playing. = `_word_2D0D6_pcm_voc_lipsync_data` (the byte stream it walks).
    pub voc_lipsync: Vec<u8>,
    /// Total PCM sample count of the playing `.voc`; the mouth stream is spread
    /// across these so the lips track the audio.
    pub voc_total_samples: u64,
    /// `PcmPlayer::samples_played()` at the moment voice playback started.
    pub voc_baseline: u64,
    /// True while the voice .voc is playing — the idle frame task yields to the
    /// lip-sync task. = `_byte_2D0DB_is_voc_pcm_playing`.
    pub speaking: bool,

    /// = the `[4540h]` previous-frame image list (its leading count word is
    /// `_word_239F0_copy_of_non_pcm_lip_sync_data`). Each redraw diffs the new
    /// frame against this and restores/redraws only the changed bounding box
    /// (`redraw_head_frame_incremental`). Empty means no previous frame yet
    /// (`_239F0 == 0`), so the next draw is a full-rect composite. Holds one
    /// `(sprite id, x, y)` per composited image, flattened across image groups.
    pub prev_images: Vec<(u8, u8, u8)>,
}

impl TalkingHead {
    /// Number of ambient (idle) animations — every animation except the last,
    /// which is the speech lip-id table.
    fn idle_anim_count(&self) -> usize {
        self.lipsync.animations.len().saturating_sub(1).max(1)
    }
}

impl GameState {
    // = seg000:e3b7 rand_masked. 16-bit LCG (seed*0xe56d+1 at [0d824h]); the
    // returned value is `((product>>16)&0xff)<<8 | (seed>>8)` ANDed with the
    // mask — a *mask*, not a modulo, so rand_masked(6) yields {0,2,4,6}.
    pub fn rand_masked(&mut self, mask: u16) -> u16 {
        let product = (self.rand_seed as u32).wrapping_mul(0xe56d);
        let seed_new = ((product & 0xffff) as u16).wrapping_add(1);
        self.rand_seed = seed_new;
        let lo = seed_new >> 8;
        let hi = ((product >> 16) & 0xff) as u16;
        ((hi << 8) | lo) & mask
    }

    // = seg000:e3cc rand. 16-bit LCG with a separate seed at [0d826h] and
    // multiplier 0xcbd1; returns `((product>>16)&0xff)<<8 | (seed>>8)`. Drives
    // the rand_bits churn the game_loop performs once per pass.
    pub fn rand(&mut self) -> u16 {
        let product = (self.rand_bits_seed as u32).wrapping_mul(0xcbd1);
        let seed_new = ((product & 0xffff) as u16).wrapping_add(1);
        self.rand_bits_seed = seed_new;
        let lo = seed_new >> 8;
        let hi = ((product >> 16) & 0xff) as u16;
        (hi << 8) | lo
    }

    // = seg000:994f idle animation selector. With facing == 0 (the named heads)
    // it returns a random idle animation: rand_masked(6) ∈ {0,2,4,6} indexes the
    // 2-byte animation TOC, i.e. animation (rand&6)>>1 ∈ {0,1,2,3}. With a
    // non-zero facing (the player) it returns the fixed animation `facing-1`
    // (the `(val-1)*2` byte-offset branch). (The DOS `[0f0h]` offset is 0 for
    // the intro heads.)
    fn pick_idle_anim(&mut self) -> usize {
        let (facing, idle_count) = {
            let head = self.talking_head.as_ref().expect("talking head set");
            (head.facing, head.idle_anim_count())
        };
        let idx = if facing == 0 {
            (self.rand_masked(6) >> 1) as usize
        } else {
            (facing - 1) as usize
        };
        idx.min(idle_count - 1)
    }

    // = seg000:9a7b loc_09a7b — pick the settled idle's resting animation and its
    // pacing mask, keyed on data_047d0 (the idle `facing` expression):
    //   - facing != 0 (the player, Paul): the calm resting animation is the SAME
    //     game_time-derived animation as the lively idle, `facing-1` (9a83 `jnz`
    //     falls straight through with al = data_047d0). Only the pacing changes,
    //     not the pose. Mask bx = 0f18h.
    //   - facing == 0 (the named idle heads, e.g. Leto): al = 5 -> animation 4
    //     (9a85). For Chani (current_lip_sync_resource_id == 7) once
    //     game_phase >= 0xc8, al is bumped to 6 -> animation 5 (9a8a..9a98, the
    //     blue-eyed late-game variant). Mask bx = 0f38h.
    // Sets `anim` and returns `rand_masked(bx)`: the caller pauses while the high
    // byte is non-zero, and the low byte (& 0x38) is the window start frame.
    fn idle_select_calm_animation(&mut self) -> u16 {
        let (facing, lip_id, idle_count) = match self.talking_head.as_ref() {
            Some(h) => (h.facing, h.lip_sync_resource_id, h.idle_anim_count()),
            None => return 0,
        };
        // = 9a7b: al = data_047d0; 9a83 `jnz loc_09a9a` uses it as-is when set.
        let (al, mask): (u8, u16) = if facing != 0 {
            (facing, 0x0f18)
        } else {
            // = 9a85 al = 5, bx = 0f38h; 9a8a..9a98 the Chani late-game bump.
            let al = if lip_id == 7 && self.game_phase >= 0xc8 {
                6
            } else {
                5
            };
            (al, 0x0f38)
        };
        // = loc_09a9a `dec al` -> animation index (al-1).
        let anim = (al.wrapping_sub(1) as usize).min(idle_count - 1);
        if let Some(head) = self.talking_head.as_mut() {
            head.anim = anim;
        }
        self.rand_masked(mask)
    }

    // = seg000:9a60 loc_09a60 (+ loc_09a74) — start a settled-idle window: skip to
    // frame `al & 0x38` of the calm animation and reload the 8-frame budget
    // (data_047ce = 8).
    fn idle_start_window(&mut self, al: u16) {
        if let Some(head) = self.talking_head.as_mut() {
            let calm_len = head
                .lipsync
                .animations
                .get(head.anim)
                .map(|a| a.frames.len().max(1))
                .unwrap_or(1);
            head.frame = ((al & 0x38) as usize).min(calm_len - 1);
            head.idle_countdown = 8; // = loc_09a74
        }
    }

    // = seg000:9f1c loc_09f1c — when a voice line starts (loc_09efd calls this for
    // heads with current_lip_sync_resource_id < 0x10), settle the head into the
    // calm resting idle right away. So by the time the line finishes the head is
    // already in the paused calm idle — no lively "talk" frames play afterward.
    fn idle_settle_for_voice(&mut self) {
        // = 9f1c setup_lip_sync_data_from_current (the port composites on demand).
        // = 9f1f or data_047d1, 10h.
        if let Some(head) = self.talking_head.as_mut() {
            head.settled = true;
        }
        // = 9f24 call loc_09a7b; 9f27 xor ah,ah (force the no-pause path);
        //   9f29 call loc_09a60; 9f2c [data_047c6] = si.
        let r = self.idle_select_calm_animation();
        self.idle_start_window(r);
    }

    // = seg000:9123 character_id_to_sprite. Map a person id to its talking-head
    // index (a HEAD_* constant — an index into HEAD_NAMES, or HEAD_PAUL for the
    // player) and the idle expression `facing` (data_047d0). Four cases, matching
    // the DOS branch structure:
    //   - id < 0x0d (named characters): head index = id, facing 0 (random idle).
    //   - id == 0x0d (SMUG): facing = (command_menu_x >> 1) + 1 — NOT PORTED
    //     (todo!() below).
    //   - id 0x0e..0x10 (char_to_sprite_walk_facing): folds in walk/facing
    //     animation state — NOT PORTED (todo!() below).
    //   - id >= 0x11, incl. the player 0x2d (char_to_sprite_player): HEAD_PAUL,
    //     facing derived from game_time.
    fn character_id_to_sprite(&self, id: u8) -> (usize, u8) {
        if id >= 0x11 {
            // = char_to_sprite_player (loc_0917a): the player's idle expression
            // (talking_head_idle_expr / data_047d0) tracks the in-game clock so
            // Paul visibly ages across the game. ah = min((game_time*4)>>8, 8) ==
            // min(game_time>>6, 8), doubled, + (desert_walk_counter >= 0x10), + 1.
            // Higher game_time selects the later (blue-eyed) idle animations; at
            // game start game_time is small so facing == 1 -> the youngest idle
            // animation 0.
            let mut anim = (self.game_time >> 6).min(8) as u8;
            anim <<= 1;
            if self.desert_walk_counter >= 0x10 {
                anim += 1;
            }
            anim += 1;
            (HEAD_PAUL, anim)
        } else if id as usize == HEAD_SMUG {
            // = char_to_sprite_smug (seg000:912f): for the SMUG smuggler head the
            // idle expression is data_047d0 = (command_menu_x >> 1) + 1, read from
            // the active verb-list header byte [command_list_ptr][0] (the menu's x
            // pixel origin; see set_command_menu_origin seg000:2e98).
            //
            // INCOMPLETE: not ported. The command-list data model doesn't exist in
            // the port yet, so command_menu_x is unavailable (see the
            // set_command_menu_origin TODO in room_game_screen.rs), and the reason
            // the smuggler's idle facing keys off the menu x-origin is not yet
            // understood. Marked todo!() so this reverse-engineering gap surfaces
            // loudly instead of silently defaulting to facing 0.
            todo!(
                "character_id_to_sprite id 0x0d (SMUG): data_047d0 = (command_menu_x >> 1) + 1 \
                 — needs the command-list data model (command_menu_x); intent not understood"
            )
        } else if id >= 0x0e {
            // = char_to_sprite_walk_facing (seg000:913b): id 0x0e..0x10 (the
            // FRM1..FRM3 generic Fremen heads). DOS derives BOTH the returned
            // sprite index and data_047d0 from the person's walk/facing state
            // (data_04756 / data_04758 / data_0476c, gated on game_phase 0xc8) —
            // loc_09155 divides the facing byte by 3 and folds it into the index.
            //
            // INCOMPLETE: not ported. That walk-state data model doesn't exist in
            // the port, so neither the sprite index nor the facing can be computed
            // faithfully. Marked todo!() so this reverse-engineering gap surfaces
            // loudly instead of silently returning (id, facing 0).
            todo!(
                "character_id_to_sprite id 0x0e..0x10 (FRM1..FRM3): sprite index + \
                 data_047d0 from walk/facing state — needs the walk-state data model"
            )
        } else {
            // = id < 0x0d named characters: head index = id, facing 0 (random idle).
            (id as usize, 0)
        }
    }

    // = the head index → portrait file base name. HEAD_PAUL is the player's
    // PERS sprite-pair index (not a HEAD_NAMES entry), so it resolves to PAUL;
    // every other head indexes HEAD_NAMES directly.
    fn head_name(head: usize) -> &'static str {
        if head == HEAD_PAUL {
            "PAUL"
        } else {
            HEAD_NAMES.get(head).copied().unwrap_or(HEAD_NAMES[0])
        }
    }

    // = seg000:91a0 setup_lip_sync_data_from_sprite_sheet (+ loc_009c7 rect
    // setup). Open the portrait sheet for character `lip_sync_resource_id`,
    // parse its lip-sync resource, save the backdrop into fb2, and render the
    // first (idle) frame. `dx` shifts the head right (loc_009c7; intro heads
    // pass 0).
    pub fn setup_talking_head(&mut self, lip_sync_resource_id: u8, dx: i16) {
        // = character_id_to_sprite (seg000:9123) + open_talking_head_resource.
        let (head, facing) = self.character_id_to_sprite(lip_sync_resource_id);
        let name = Self::head_name(head);
        let file = format!("{name}.HSQ");

        let data = self
            .dat_file
            .read(&file)
            .unwrap_or_else(|_| panic!("failed to read {file}"));
        let sheet =
            SpriteSheet::from_slice(&data).unwrap_or_else(|_| panic!("failed to parse {file}"));
        // = open_spritesheet -> apply_sprite_sheet_palette.
        sheet
            .apply_palette_update(&mut self.palette)
            .expect("failed to apply portrait palette");

        let last = sheet.resource_count() - 1;
        let lipsync_data = sheet
            .get_resource(last)
            .expect("portrait sheet has no lip-sync resource");
        let lipsync = Lipsync::from_bytes(lipsync_data);

        // = loc_009c7: shift the rect right by `dx`, clamping x1 ≤ 0x140.
        let (mut x0, y0, mut x1, y1) = lipsync.rect;
        if x0 < dx {
            x0 += dx;
            x1 = (x1 + dx).min(0x140);
        }

        // = copy_active_framebuffer_to_framebuffer_2: save the freshly-drawn
        // room (active = fb1 during init) into fb2, the clean backdrop the head
        // is composited over and restored from each frame.
        self.copy_active_framebuffer_to_framebuffer_2();

        self.talking_head = Some(TalkingHead {
            sheet,
            lipsync,
            lip_sync_resource_id: lip_sync_resource_id as u16,
            // = character_id_to_sprite(al) = the returned head/sprite-pair index
            // (id itself for ids < 0x0d, HEAD_PAUL for the player).
            talking_head_id: head as u16,
            rect: (x0, y0, x1, y1),
            mouth: 0,
            facing,
            anim: 0,
            frame: 0,
            settled: false,
            // = loc_09908: data_047ce = data_0478c * 4, and data_0478c is 0.
            idle_countdown: 0,
            voc_lipsync: Vec::new(),
            voc_total_samples: 0,
            voc_baseline: 0,
            speaking: false,
            prev_images: Vec::new(),
        });

        // = loc_0978e first render: pick a random idle animation and draw its
        // first frame onto the backdrop. This composites into the offscreen
        // framebuffer ONLY — like every other intro init, the screen is left
        // untouched so play_intro's stage transition reveals the room+head at
        // the right moment (and under the new palette). Copying to the screen
        // here would flash the new pixels under the old palette during the
        // dissolve (a black silhouette of the head).
        let anim = self.pick_idle_anim();
        if let Some(head) = self.talking_head.as_mut() {
            head.anim = anim;
            head.frame = 0;
        }
        self.composite_head_frame(anim, 0);

        // = copy_non_pcm_lip_sync_data_and_draw_talking_head's [460a]->[4540]
        // copy (seg000:9d18): record this first pose as the previous frame so
        // every later tick diffs against it and redraws incrementally
        // (redraw_head_frame_incremental) rather than wiping the whole head
        // rect. This is what preserves a sprite drawn over the backdrop after
        // setup — the LOOK AT MIRROR frame (MIRROR.HSQ sprite 2).
        let first_images = self
            .talking_head
            .as_ref()
            .map(|h| flatten_frame(&h.lipsync, anim, 0))
            .unwrap_or_default();
        if let Some(head) = self.talking_head.as_mut() {
            head.prev_images = first_images;
        }

        // = seg000:9945 loc_09908 installs the idle animator (loc_099be) as a
        // frame task at interval bp=0x10 (16 ticks). It walks the current
        // animation's frames and re-randomises the animation at each end, until
        // the idle countdown ([47ceh]) runs out. DOS installs it here, during
        // the init render; because transitions don't run frame tasks, it first
        // fires in the post-transition wait loop (so the head is revealed by the
        // transition, then animates).
        self.add_frame_task(0x10, crate::TaskId::TalkingHeadIdle);
    }

    // = seg000:99be loc_099be (via loc_099da/loc_099f6) — one tick of the idle
    // animator, in two phases gated by the [47ceh] budget (data_047ce):
    //
    //   - LIVELY (loc_0994f path, data_047d1 sign clear): play the lively idle
    //     animation (data_047d0 == 0 -> random {0..3}; the player -> the fixed
    //     game_time-derived animation `facing-1`), which moves the mouth,
    //     spending one budget unit per frame. The budget starts at [478ch]*4 = 0,
    //     so it runs out during the first animation; at that boundary
    //     (loc_09a1d `js loc_09a3b`) the head settles (data_047d1 |= 0x10).
    //   - SETTLED (loc_09a40 path): the calm resting idle. Play the resting
    //     animation chosen by loc_09a7b (data_047d0 == 0 -> animation 4, or 5 for
    //     Chani late game; the player -> the SAME animation `facing-1` as the
    //     lively idle) in 8-frame windows (data_047ce reloaded to 8 at loc_09a74)
    //     starting at a random frame offset; BETWEEN windows the `rand_masked`
    //     high byte holds the current frame (`or ah,ah; jnz loc_09a1c`) for a
    //     random number of ticks — the pauses between idle gestures.
    //
    // The task runs continuously until the next stage calls remove_all_frame_tasks.
    pub(crate) fn tick_talking_head_idle(&mut self) {
        // Yield while speaking; read the current animation's length (for the
        // in-window frame advance below). The settled resting animation was
        // chosen by idle_select_calm_animation (loc_09a7b) — animation `facing-1`
        // for the player, animation 4/5 for the named idle heads — so its length
        // is read from head.anim, not a fixed index.
        let (settled, calm_len) = {
            let Some(head) = self.talking_head.as_ref() else {
                self.remove_frame_task(crate::TaskId::TalkingHeadIdle);
                return;
            };
            if head.speaking {
                // Speech in progress — the voc task owns the frame; idle yields.
                return;
            }
            let calm_len = head
                .lipsync
                .animations
                .get(head.anim)
                .map(|a| a.frames.len().max(1))
                .unwrap_or(1);
            (head.settled, calm_len)
        };

        // = loc_09a40 — the settled (calm) idle.
        if settled {
            if self.talking_head.as_ref().unwrap().idle_countdown <= 0 {
                // = loc_09a48 budget spent -> loc_09a7b. `or ah,ah; jnz loc_09a1c`
                // holds the current frame (a pause) while the high byte is set.
                let r = self.idle_select_calm_animation();
                if (r >> 8) != 0 {
                    return;
                }
                // = loc_09a60 + loc_09a74: start a window at a random frame offset.
                self.idle_start_window(r);
            } else {
                // = loc_09a4d `jg loc_099f6`: still inside the window — advance.
                let head = self.talking_head.as_mut().unwrap();
                head.frame = (head.frame + 1) % calm_len;
            }
            let (anim, frame) = {
                let head = self.talking_head.as_mut().unwrap();
                head.idle_countdown -= 1; // = loc_099f6 `dec data_047ce`
                (head.anim, head.frame)
            };
            if let Some(rect) = self.redraw_head_frame_incremental(anim, frame) {
                self.present_head_dirty_rect(rect);
            }
            return;
        }

        // = loc_0994f path — the lively idle.
        let advance = {
            let head = self.talking_head.as_mut().unwrap();
            head.idle_countdown -= 1; // = loc_099f6 `dec data_047ce`
            let animation_len = head
                .lipsync
                .animations
                .get(head.anim)
                .map(|a| a.frames.len())
                .unwrap_or(0);
            let advance = head.frame + 1 >= animation_len;
            // = loc_09a1d `cmp data_047ce,0; js loc_09a3b` — settle at the boundary
            // once the budget is spent; the settled branch takes over next tick.
            if advance && head.idle_countdown < 0 {
                head.settled = true;
            }
            advance
        };

        // Pick the next lively animation at a boundary — unless we just settled,
        // in which case this tick finishes the current animation's last frame and
        // the settled branch runs from the next tick.
        let next_anim = if advance && !self.talking_head.as_ref().unwrap().settled {
            Some(self.pick_idle_anim())
        } else {
            None
        };

        let (anim, frame) = {
            let head = self.talking_head.as_mut().unwrap();
            if let Some(anim) = next_anim {
                head.anim = anim;
                head.frame = 0;
            } else if !advance {
                head.frame += 1;
            }
            (head.anim, head.frame)
        };

        // = seg000:9a05 jz loc_09a1c — when nothing changed, skip the whole
        // present tail; else push the dirty rect through it.
        if let Some(rect) = self.redraw_head_frame_incremental(anim, frame) {
            self.present_head_dirty_rect(rect);
        }
    }

    // = seg000:9a0d..9a19 present_head_dirty_rect — the shared draw tail of the
    // idle animator (9a0d..9a19) and the lip-sync task (seg000:9e48..9e54):
    // after the incremental head redraw left a dirty rect in fb1, push it to
    // the visible screen through the shared present chain.
    fn present_head_dirty_rect(&mut self, rect: Rect) {
        // = seg000:9a0d call loc_0908c — redraw the speech-bubble border sprite
        //   when the dirty rect reaches into the bubble area
        //   (current_bubble_layout_ptr == 0x223c); the bubble system is unported.
        // = seg000:9a10 si = 0d834h; 9a13 call restore_mouse_if_rect_intersects —
        //   lift the software cursor when the rect overlaps it. The port hides it
        //   unconditionally (call_restore_cursor, the same routine minus the
        //   intersect early-out; a no-op for the overlay cursor).
        self.call_restore_cursor();
        // = seg000:9a16 call present_screen_rect
        //   (seg000:c4f0) — redraw the HUD head into fb1 when the rect overlaps
        //   its box (suppressed while data_0227d != 0, i.e. through the whole
        //   intro), then push the rect fb1 -> screen via copy_rect_fb1_to_screen
        //   (skipped while the front buffer is redirected to fb1 or the mixer
        //   panel owns the mouse handlers).
        self.present_screen_rect(rect);
        // = seg000:9a19 jmp draw_mouse_cursor_if_needed — re-show the cursor.
        self.draw_mouse();
    }

    // = seg000:9efd loc_09efd + load_voc_and_lipsync_data (a6e6) + loc_0a75c —
    // load the current head's voice .voc for `voc_index` and, on success, start
    // PCM playback and install the lip-sync frame task. On failure (file absent /
    // no lip-sync stream) the head just keeps idling (the carry-clear ret path).
    //
    // `voc_index` is the post-transform index (the caller applies the a6ee
    // `ah &= 0xf3` strip + the data_0d7f4 per-person base subtraction). `suffix`
    // is the create_voc_file_name_from_bx 'I'/'O' name letter (a8e1..a8fa).
    pub(crate) fn play_talking_head_voc(&mut self, voc_index: u16, suffix: char) {
        let Some(head) = self.talking_head.as_ref() else {
            return;
        };
        // = load_voc_and_lipsync_data (seg000:a6e6): the voc directory id is the
        // lip-sync id clamped to 0x0e (`cmp bl,0eh; jb +; mov bl,0eh`). The
        // player (0x2d) maps to 'A'+0x0e = 'O' → the "PO" dir; named heads pass
        // through unchanged (Leto id 0 → 'A' → the "PA" dir).
        let dir_id = head.lip_sync_resource_id.min(0x0e) as u8;

        // = create_voc_file_name_from_bx (seg000:a8bc): "P<L>\P<L><idx><X>.VOC",
        // where the directory/name letter L = 'A' + dir_id, <idx> is the 3-hex-
        // digit voc index, and <X> is the 'I'/'O' suffix.
        let letter = (b'A' + dir_id) as char;
        let name = format!("P{letter}\\P{letter}{voc_index:03X}{suffix}.VOC");

        // = load_voc_and_lipsync_data -> voc_get_lipsync_data: read the .voc,
        // pull the type-5 comment-block mouth stream and the type-1 PCM block.
        let Ok(data) = self.dat_file.read(&name) else {
            return; // no voice file in this DAT — keep idling.
        };
        // A talking head needs the lip-sync mouth stream; without it keep idling
        // rather than play a mute voice.
        let Some(voc) = crate::voc::parse(&data).filter(|v| !v.lipsync.is_empty()) else {
            return;
        };

        // = loc_09efd 9f0f: cmp current_lip_sync_resource_id, 10h; jnb loc_09f19;
        //   call loc_09f1c — for an in-range head, settle into the calm idle as
        //   the line starts so it is already in the paused calm idle when the line
        //   ends (no lively "talk" frames afterward).
        if self.current_lip_sync_resource_id < 0x10 {
            self.idle_settle_for_voice();
        }

        // = seg000:a754 — the voice is about to start: duck the score under it
        self.midi_duck_music_volume();

        // = seg000:a757- flip the TALK TO ME verb to its talking variant
        // (mark_talk_to_me_verb_talking, 0x90 '>>>> TALK TO ME <<<<').
        self.set_talk_to_me_verb_text(0x90);

        // = loc_0a75c: start the Sound Blaster voice and seed the lip-sync
        // timing. pcm_stop_voc first, then start this clip on the dnsdb driver.
        self.pcm_player.stop();
        let baseline = self.pcm_player.samples_played();
        let total = voc.pcm.len() as u64;
        self.pcm_player.start_playback(&data, 0);

        if let Some(head) = self.talking_head.as_mut() {
            head.voc_lipsync = voc.lipsync;
            head.voc_total_samples = total;
            head.voc_baseline = baseline;
            head.mouth = 0;
            head.speaking = true;
        }

        // = loc_0a75c add_frame_task(bp=0, lip_sync_frame_task). Polls the PCM
        // sample clock every tick and advances the mouth.
        self.add_frame_task(0, crate::TaskId::TalkingHeadVoc);
    }

    // = seg000:ab15 audio_start_voc — play a .voc sound effect by name (e.g.
    // SN3.VOC, the night-attack sound). Loads the resource, parses its type-1
    // PCM block, and queues it on the Sound Blaster voice. Unlike a talking
    // head this needs no lip-sync stream. Silently does nothing if the file is
    // absent or has no audio.
    //
    // The DOS `is_voc_pcm_playing` non-interrupt guard (don't restart while a
    // voice is mid-playback) is not modelled; callers in the intro only fire a
    // single effect when nothing else is playing.
    pub fn audio_start_voc(&mut self, name: &str) {
        let Ok(data) = self.dat_file.read(name) else {
            return;
        };
        // = pcm_stop_voc then start the clip on the dnsdb driver. The driver
        // parses the VOC blocks itself, so the raw bytes (past the 0x1a header)
        // are handed straight to it.
        self.pcm_player.stop();
        self.pcm_player.start_playback(&data[26..], 0);
    }

    // = seg000:a7c2 lip_sync_frame_task (+ advance_lipsync / set_lipsync_data_to_al).
    // Step the mouth value from the .voc stream in lock-step with PCM playback:
    // mouth index = samples_played / SAMPLES_PER_LIP_FRAME (a fixed cadence),
    // holding the last value once the stream ends. When a new value arrives it
    // draws the matching speech frame (last animation, frame = mouth value).
    // When the clip's audio finishes the head reverts to idle.
    pub(crate) fn tick_talking_head_voc(&mut self) {
        let played = self.pcm_player.samples_played();

        let (lip_anim, frame, mouth, done) = {
            let Some(head) = self.talking_head.as_ref() else {
                self.remove_frame_task(crate::TaskId::TalkingHeadVoc);
                return;
            };
            let played = played.saturating_sub(head.voc_baseline);
            // = is_voc_pcm_playing / pcm_test_audio_done: no lip-sync stream, no
            // audio, or the clip has drained → over.
            if head.voc_lipsync.is_empty()
                || head.voc_total_samples == 0
                || played >= head.voc_total_samples
            {
                (0usize, 0usize, 0u8, true)
            } else {
                // = lip_sync_frame_task timing: the mouth advances one stream
                // value per fixed SAMPLES_PER_LIP_FRAME of audio, slaved to the
                // SB sample clock. NOT spread over the whole clip — when the
                // stream ends the last value holds while any trailing silence
                // plays out (the stage waits on the audio, not the stream).
                let len = head.voc_lipsync.len() as u64;
                let idx = (played / SAMPLES_PER_LIP_FRAME).min(len - 1) as usize;
                let mouth = head.voc_lipsync[idx];
                // = the last animation is the lip-id table; mouth value v
                // selects frame v ([047d0]==0 path → si = lip_ids + mouth*2).
                let lip_anim = head.lipsync.animations.len().saturating_sub(1);
                let last_frame = head
                    .lipsync
                    .animations
                    .get(lip_anim)
                    .map(|a| a.frames.len().saturating_sub(1))
                    .unwrap_or(0);
                (lip_anim, (mouth as usize).min(last_frame), mouth, false)
            }
        };

        if done {
            // = the frame task's drained path: seg000:a7ce pcm_test_audio_done
            // -> loc_0a789 -> lip_sync_stop (the data_0dc30 chained-voc branch
            // at a793 is not ported). Revert to idle (mouth=0); the idle task
            // then resumes. DOS does NOT settle here — the idle finishes its
            // current lively animation and settles to the calm expression only
            // when the [47ceh] countdown runs out (loc_09a1d -> loc_09a3b),
            // which tick_talking_head_idle models.
            if let Some(head) = self.talking_head.as_mut() {
                head.mouth = 0;
                // Port mechanism (not a DOS step): speech draws each mouth via a
                // full head-rect composite (composite_head_layers) without keeping
                // prev_images, so drop it to force one clean full redraw when the
                // idle resumes; otherwise its incremental diff vs the stale
                // pre-speech frame can leave the last speech mouth on screen.
                head.prev_images.clear();
            }
            self.lip_sync_stop();
            return;
        }

        // = set_lipsync_data_to_al `cmp al,[_byte_2D0DA_last_lipsync_data]; jz`
        // — only redraw when the mouth value changes.
        let changed = {
            let head = self.talking_head.as_mut().unwrap();
            let c = head.mouth != mouth;
            head.mouth = mouth;
            c
        };

        if changed {
            // Speech lip-id frames carry only the mouth; draw a neutral base
            // face (idle animation 0, frame 0) underneath so the head stays
            // whole, then the speech mouth on top.
            let rect = self.composite_head_layers(&[(0, 0), (lip_anim, frame)]);
            // = the seg000:9e48..9e54 draw tail (loc_0908c -> restore_mouse_if_
            // rect_intersects -> draw_hud_head_if_needed_and_update_screen_rect_
            // at_si -> draw_mouse_cursor_if_needed) — the same shared present
            // chain as the idle animator's.
            self.present_head_dirty_rect(rect);
        }
    }

    // = seg000:a7a5 lip_sync_stop — stop any active voice lip-sync: remove the
    // voc frame task, drop the mouth stream and flip the TALK TO ME verb to its
    // idle text. While a voice is marked playing (is_voc_pcm_playing),
    // additionally silence the PCM voice and swell the score back to its normal
    // level.
    pub(crate) fn lip_sync_stop(&mut self) {
        // = seg000:a7a5 mov si, lip_sync_frame_task; a7a8 call remove_frame_task.
        self.remove_frame_task(crate::TaskId::TalkingHeadVoc);
        // = seg000:a7ab mov [_word_2D0D6_pcm_voc_lipsync_data], 0 — drop the
        // mouth stream. The playing flag (head.speaking = _byte_2D0DB) is read
        // here for the a7b4 gate and cleared in the same step (= seg000:a7b9
        // call set_voc_pcm_is_not_playing).
        let was_speaking = self.talking_head.as_mut().is_some_and(|head| {
            head.voc_lipsync.clear();
            std::mem::take(&mut head.speaking)
        });
        // = seg000:a7b1 call mark_talk_to_me_verb_idle — flip the verb to its
        // quoted idle variant (0x9f '" TALK TO ME "') and redraw it in place.
        self.set_talk_to_me_verb_text(0x9f);
        // = seg000:a7b4 call is_voc_pcm_playing; a7b7 jz ret.
        if !was_speaking {
            return;
        }
        // = seg000:a7bc call close_pcm_voice_file_handle — silence the voice.
        self.pcm_player.stop();
        // = seg000:a7bf jmp midi_restore_music_volume — swell the score back to
        // its normal level now the voice line is over.
        self.midi_restore_music_volume();
    }

    // Composite a single (anim, frame) over the backdrop. Returns the head rect.
    fn composite_head_frame(&mut self, anim: usize, frame_idx: usize) -> Rect {
        self.composite_head_layers(&[(anim, frame_idx)])
    }

    // = seg000:9d2d draw_talking_head_at_si.
    // Composite one talking-head pose over the room backdrop, drawing the given
    // (anim, frame) layers in order. Restores the room from fb2 (the saved clean
    // scene, = copy_rect_fb2_to_fb1), then draws every image group of each layer
    // at (image.x + rect.x0, image.y + rect.y0) with the gfx y-offset applied
    // (mirroring the DOS blit's fb_base_ofs). = the seg000:9d2d
    // draw_talking_head_at_si image-group blit loop.
    //
    // Each image group is clipped to the head rect (x0,y0,x1,y1): DOS draws
    // every group through j_vga_blit_clipped with the clip rect at [0d834h],
    // which is seeded from the head rect [1bf0h] (seg000:9bbc). Without the clip
    // a sprite that overruns its box — e.g. Kynes' hair or Stilgar's hood —
    // bleeds across the rest of the scene.
    //
    // Idle frames bundle the whole face (base + eyes + mouth), so a single
    // layer suffices. The speech lip-id frames carry *only* the mouth — DOS
    // keeps the rest of the face on screen via incremental redraw — so speech
    // passes a neutral base layer first, then the mouth layer on top.
    // Returns the head clip rect it composited into (in absolute framebuffer
    // coordinates), so callers can push just that rect to the visible screen.
    fn composite_head_layers(&mut self, layers: &[(usize, usize)]) -> Rect {
        if self.talking_head.is_none() {
            return Rect::default();
        }
        let head = self.talking_head.as_ref().unwrap();

        let yoff = self.y_offset as i16;
        let (x0, y0, x1, y1) = head.rect;
        // = the [0d834h] / [1bf0h] head clip rect, in absolute framebuffer
        // coordinates (y shifted by fb_base_ofs to match the draw positions).
        let clip = Rect {
            x0,
            y0: y0 + yoff,
            x1,
            y1: y1 + yoff,
        };
        // = seg000:c446 copy_rect_fb2_to_fb1 — restore the clean backdrop
        // ONLY inside the head rect, not the whole framebuffer. Pixels outside
        // (e.g. intro2's narration subtitle strip at y >= 153) are owned by
        // other draws and must be left intact.
        gfx::vga_copy_rect(&mut self.framebuffer, &self.framebuffer_saved, clip);
        let lipsync = &head.lipsync;
        let sheet = &head.sheet;

        for &(anim, frame_idx) in layers {
            let Some(animation) = lipsync.animations.get(anim) else {
                continue;
            };
            let Some(frame) = animation.frames.get(frame_idx) else {
                continue;
            };
            for &group_idx in &frame.image_groups {
                let Some(group) = lipsync.image_groups.get(group_idx as usize) else {
                    continue;
                };
                for image in group {
                    // = seg000:9d2d: sprite index is id-1; position is image
                    // (x,y) + rect origin, + the framebuffer base row; clipped
                    // to the head rect via j_vga_blit_clipped.
                    if let Some(sprite) = sheet.get_sprite(image.id as u16 - 1) {
                        let _ = sprite_blitter(sprite, &mut self.framebuffer)
                            .at(image.x as i16 + x0, image.y as i16 + y0 + yoff)
                            .clip_rect(clip)
                            .draw();
                    }
                }
            }
        }

        clip
    }

    // = seg000:9bb1 loc_09bb1 with `_word_239F0 != 0` — the incremental redraw
    // the idle / lip-sync frame tasks run on every tick after the first full
    // composite. It diffs the new frame's flattened image list against the
    // previous one (loc_09c2d / loc_09cc6): it unions the bounding boxes of
    // only the images that differ (present in one frame but not the other,
    // matched on exact id+x+y), restores the backdrop from fb2 over JUST that
    // box, then redraws the whole new frame clipped to it. The static pixels at
    // the edges of the head rect are never touched, so a sprite drawn over the
    // backdrop after setup — the LOOK AT MIRROR frame, MIRROR.HSQ sprite 2 —
    // survives every tick instead of being wiped by a full-head-rect restore.
    //
    // With no previous frame yet (`_239F0 == 0`, i.e. prev_images empty — only
    // straight after setup_talking_head) it falls back to the full composite,
    // matching loc_09bb1's first-draw branch (seg000:9bbc).
    //
    // Returns the rect that was redrawn (= the [0d834h] clip rect DOS pushes to
    // screen at seg000:9a16), or `None` when nothing changed and the screen
    // already shows this pose.
    fn redraw_head_frame_incremental(&mut self, anim: usize, frame_idx: usize) -> Option<Rect> {
        let head = self.talking_head.as_ref()?;

        // = the `_239F0 == 0` first-draw branch (seg000:9bbc): no previous frame
        // to diff against, so restore + redraw the whole head rect.
        if head.prev_images.is_empty() {
            let rect = self.composite_head_frame(anim, frame_idx);
            let first_images = self
                .talking_head
                .as_ref()
                .map(|h| flatten_frame(&h.lipsync, anim, frame_idx))
                .unwrap_or_default();
            if let Some(head) = self.talking_head.as_mut() {
                head.prev_images = first_images;
            }
            return Some(rect);
        }

        let yoff = self.y_offset as i16;
        let (rx0, ry0, _, _) = head.rect;
        let cur = flatten_frame(&head.lipsync, anim, frame_idx);

        // = loc_09c2d: union the bounding boxes of the images in the symmetric
        // difference of the two frames (loc_09c54 walks each list looking for an
        // exact id+x+y match in the other; an unmatched image is "changed" and
        // expands the box via loc_09cc6). Seeded inverted — x0,y0 at the max
        // corner, x1,y1 at the min — so an x0 still at 0x13f flags "no change".
        let mut x0 = 0x13fi16;
        let mut y0 = 0xc7i16;
        let mut x1 = 0i16;
        let mut y1 = 0i16;
        for &(id, x, y) in cur
            .iter()
            .filter(|i| !head.prev_images.contains(i))
            .chain(head.prev_images.iter().filter(|i| !cur.contains(i)))
        {
            // = loc_09cc6: the image spans [left, left+w) × [top, top+h), with
            // the sprite header's width (&1ffh) and height (low byte).
            let Some(sprite) = head.sheet.get_sprite(id as u16 - 1) else {
                continue;
            };
            let left = x as i16 + rx0;
            let top = y as i16 + ry0 + yoff;
            x0 = x0.min(left);
            y0 = y0.min(top);
            x1 = x1.max(left + sprite.width() as i16);
            y1 = y1.max(top + sprite.height() as i16);
        }

        // = seg000:9bcf/9a05 `cmp [2CCE4],13fh; jz` — x0 never moved, so no image
        // changed and the screen already shows this pose; skip the redraw.
        let mut dirty = None;
        if x0 != 0x13f {
            // = seg000:9bda clamp the dirty box's bottom to the game-area floor
            // (0x98 = 152) so the head redraw never reaches the subtitle strip.
            let clip = Rect {
                x0,
                y0,
                x1,
                y1: y1.min(152 + yoff),
            };
            // = seg000:9be6 copy_rect_fb2_to_fb1 — restore the backdrop over only
            // the changed box.
            gfx::vga_copy_rect(&mut self.framebuffer, &self.framebuffer_saved, clip);
            // = seg000:9be9 draw_talking_head_at_si — redraw every image of the
            // new frame, clipped to the changed box.
            let head = self.talking_head.as_ref().unwrap();
            for &(id, x, y) in &cur {
                if let Some(sprite) = head.sheet.get_sprite(id as u16 - 1) {
                    let _ = sprite_blitter(sprite, &mut self.framebuffer)
                        .at(x as i16 + rx0, y as i16 + ry0 + yoff)
                        .clip_rect(clip)
                        .draw();
                }
            }
            dirty = Some(clip);
        }

        // = the [460a]->[4540] copy (seg000:9d18): the new frame becomes the
        // previous one for the next diff.
        if let Some(head) = self.talking_head.as_mut() {
            head.prev_images = cur;
        }

        dirty
    }
}

// = setup_non_lip_sync_data_structure (seg000:9bee): flatten one (anim, frame)
// pose into the [460ah] image list the incremental redraw diffs — every image
// of every image group the frame references, as (sprite id, x, y).
fn flatten_frame(lipsync: &Lipsync, anim: usize, frame_idx: usize) -> Vec<(u8, u8, u8)> {
    let mut images = Vec::new();
    let Some(animation) = lipsync.animations.get(anim) else {
        return images;
    };
    let Some(frame) = animation.frames.get(frame_idx) else {
        return images;
    };
    for &group_idx in &frame.image_groups {
        if let Some(group) = lipsync.image_groups.get(group_idx as usize) {
            for image in group {
                images.push((image.id, image.x, image.y));
            }
        }
    }
    images
}
