//! = segvga:1cb6 vga_globe_init / segvga:1d07 vga_globe_setup — the globe
//! pixel renderer, together with the seg000 table builders that feed it
//! (seg000:b9f6 recalculate_globe_rotation_table, seg000:ba2d
//! build_globe_tilt_window_table).
//!
//! DOS splits the work between three buffers in the data segment: the
//! in-memory TABLAT (seg001:4948, 99 entries of {row start, row len, rotation
//! fp}), the GLOBDATA resource right after it (the globe outline stream at
//! +1 and the per-x-column latitude/cell tables at +0xcda), and the 196-word
//! tilt window table (globe_tilt_window_table, seg001:8b77) rebuilt on every
//! tilt change. The port bundles all three into one `GlobeRenderer` that owns
//! copies of GLOBDATA and MAP and rebuilds its lookup tables per draw.
//!
//! The GameState glue at the bottom ports the seg000 callers
//! (setup_globe_draw / draw_globe_with_atmosphere / map_func_gfx).

use std::io::Cursor;

use bytes_ext::ReadBytesExt;

use crate::{FrameBuffer, GameState, Rect, rect::rect, sprite_bank};

// = the 99 entries of RESOURCE_TABLAT (seg001:4948) / the 0x62 loop bound in
// recalculate_globe_rotation_table (98 rows after the equator entry).
const MAX_TILT: usize = 99;

// = one in-memory TABLAT entry {offset:u16, len:u16, fp:u32}. vga_globe_setup
// reads them as bx = [bp] (row start), cx = [bp+2] (row len), dx = [bp+4]
// (rotation fp hi word) — segvga:1db2..1db8.
#[derive(Copy, Clone, Debug, Default)]
struct RotationEntry {
    map_row_start: i16,
    map_row_len: u16,
    fp: u32,
}

// = the quadrant selector encoded in a globe_tilt_window_table word: the
// north/south and near/far sign bits vga_globe_setup dispatches on at
// segvga:1d91/1d96 (js / cbw+jns into the four branches 1d9a/1dc2/1de8/1e11).
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum GlobeSection {
    FarNorth,
    NearNorth,
    NearSouth,
    FarSouth,
}

// = one decoded globe_tilt_window_table word: which globe section the visible
// relative latitude falls in, and the 0..98 latitude row within it.
#[derive(Copy, Clone, Debug)]
struct GlobeSectionLatitude {
    pub section: GlobeSection,
    pub latitude: u8,
}

impl GlobeSectionLatitude {
    fn new(section: GlobeSection, latitude: u8) -> GlobeSectionLatitude {
        GlobeSectionLatitude { section, latitude }
    }
}

impl Default for GlobeSectionLatitude {
    fn default() -> Self {
        GlobeSectionLatitude::new(GlobeSection::FarNorth, 0)
    }
}

#[derive(Eq, PartialEq, Debug)]
enum Half {
    Upper,
    Lower,
}

pub struct GlobeRenderer {
    globdata: Vec<u8>,
    map: Vec<u8>,
    // = RESOURCE_TABLAT (seg001:4948) with the fp fields recomputed per
    // rotation (seg000:b9f6 recalculate_globe_rotation_table).
    rotation_lookup_table: [RotationEntry; MAX_TILT],
    // = globe_tilt_window_table (seg001:8b77). DOS rebuilds a 196-word window
    // centred on the current tilt (seg000:ba2d build_globe_tilt_window_table);
    // the port precomputes the full section/latitude ramp once — south at the
    // low indices, north at the high — and indexes it with
    // (outline latitude + 196 - tilt) in draw_half (the tilt carries the
    // map-row sign, negative = north).
    tilt_lookup_table: [GlobeSectionLatitude; 4 * MAX_TILT - 4],
    // = the vga_globe_setup call count of one full draw pass: the rotation
    // frame task draws one outline row per tick (each hemisphere walks the
    // stream's rows once) and one final call returns the pass-complete carry
    // — 2 * outline rows + 1 ticks.
    ticks_per_pass: u16,
    // = the segvga outline-stream cursors' progress (01ca6/01ca8), reduced to
    // a countdown: how many of the pass's ticks remain.
    pass_ticks_left: u16,
}

impl GlobeRenderer {
    pub fn new(globdata: &[u8], map: &[u8], tablat: &[u8]) -> GlobeRenderer {
        // Row count of the outline stream (same walk as draw_half's row
        // loop), for the rotation task's pass length.
        let mut rows: u16 = 0;
        let mut i = 0;
        loop {
            let n = globdata[i] as i8;
            assert!(n < 0);
            let line_len = (!n) as usize;
            if line_len == 0 {
                break;
            }
            i += 1 + line_len;
            rows += 1;
        }
        let ticks_per_pass = 2 * rows + 1;

        let mut r = GlobeRenderer {
            globdata: globdata.to_vec(),
            map: map.to_vec(),
            rotation_lookup_table: [RotationEntry::default(); MAX_TILT],
            tilt_lookup_table: [GlobeSectionLatitude::default(); 4 * MAX_TILT - 4],
            ticks_per_pass,
            pass_ticks_left: ticks_per_pass,
        };

        // = seg000:ba2d build_globe_tilt_window_table: the far-south ..
        // far-north latitude ramp its stosw loops write, laid out here for
        // every tilt at once.
        let mut tilt_lookup_table = Vec::with_capacity(r.tilt_lookup_table.len());
        for i in 1..=98 {
            tilt_lookup_table.push(GlobeSectionLatitude::new(GlobeSection::FarSouth, i));
        }
        for i in (0..=98).rev() {
            tilt_lookup_table.push(GlobeSectionLatitude::new(GlobeSection::NearSouth, i));
        }
        for i in 1..=98 {
            tilt_lookup_table.push(GlobeSectionLatitude::new(GlobeSection::NearNorth, i));
        }
        for i in (2..=98).rev() {
            tilt_lookup_table.push(GlobeSectionLatitude::new(GlobeSection::FarNorth, i));
        }
        r.tilt_lookup_table = tilt_lookup_table.try_into().unwrap();

        // = the TABLAT.BIN entries as loaded into RESOURCE_TABLAT: 8 bytes per
        // row, big-endian {offset, len}, the trailing 4 bytes the fp scratch.
        for (i, e) in r.rotation_lookup_table.iter_mut().enumerate() {
            let offset = 8 * i;
            e.map_row_start = i16::from_be_bytes(tablat[offset..offset + 2].try_into().unwrap());
            e.map_row_len = u16::from_be_bytes(tablat[offset + 2..offset + 4].try_into().unwrap());
        }

        r.precalculate_globe_rotation_lookup_table(0);
        r
    }

    // = the GLOBDATA per-x-column tables at +0xcda (segvga:1d52 data_01caa;
    // 200 bytes per x column): [x*200 + latitude] = the rotation-table row
    // selector (segvga:1d9e mov bl,[bp+si]).
    fn globdata_table_2(&self, x: usize, latitude: usize) -> u8 {
        assert!(x < 64);
        assert!(latitude < 100);
        self.globdata[3290 + x * 200 + latitude]
    }

    // = the paired cell-offset table 100 bytes further in (segvga:1da0
    // mov al,[bp+si+64h]): the signed cell offset within the map row.
    fn globdata_table_3(&self, x: usize, latitude: usize) -> i16 {
        assert!(x < 64);
        assert!(latitude < 100);
        self.globdata[3290 + x * 200 + latitude + 100] as i8 as i16
    }

    // = seg000:b9f6 recalculate_globe_rotation_table. `phase` is the rotation
    // in 1/398ths of a revolution (the _dword_23DFC seed's integer word);
    // entry 0's fp is the seed itself and every other row's fp is
    // 2 * bx * row len, with bx = (phase<<16 + 0x8000) / 398 (the seg000:b9f9
    // div with the seed still in dx:ax).
    fn precalculate_globe_rotation_lookup_table(&mut self, phase: u16) {
        // = seg000:ba7d/ba7f the seed dword: hi word = phase, lo word 0.
        let fp0 = (phase as u32) << 16;
        self.rotation_lookup_table[0].fp = fp0;

        let bx = (fp0 + 0x8000) / 398;
        for i in 1..self.rotation_lookup_table.len() {
            let dxax = 2 * bx * self.rotation_lookup_table[i].map_row_len as u32;

            self.rotation_lookup_table[i].fp = dxax;
        }
    }

    // = segvga:1e47..1e5c: the map byte at [MAP centre 0x62fc + offset] maps
    // to a colour — al = (v & 0x0f), +12 when the flag nibble (v & 0x30) is
    // exactly 0x10 and the colour is < 8, then +0x10 into the globe palette
    // block.
    fn map_color(&self, offset: i16) -> u8 {
        let map_value = self.map[(0x62fc_i32 + offset as i32) as usize];
        let flags = map_value & 0x30;
        let mut color = map_value & 0x0f;

        if flags == 0x10 && color < 8 {
            color += 12;
        }

        color + 0x10
    }

    // = the segvga:1d07 vga_globe_setup row loop, run for one hemisphere. DOS
    // walks the GLOBDATA outline stream with two cursors (one per hemisphere,
    // segvga 01ca6/01ca8) and stores mirrored west/east pixels through the
    // 01cae/01cb0 cursors around the centre column; the port re-reads the
    // stream per half and mirrors around (center_x, center_y). The cursors
    // are seeded with absolute framebuffer offsets (segvga:1d35 ax = 0x64a0 =
    // row 80, column 160) — the globe does not add fb_base_ofs, so no
    // y_offset applies here (intro2 runs with row base 0, = seg000:0332).
    fn draw_half(&self, fb: &mut FrameBuffer, half: Half, tilt: i16) {
        let center_x = 160 - 1;
        let center_y = 80 - 1;

        let mut y = 0;

        let mut globdata_reader = Cursor::new(&self.globdata);

        loop {
            // = the negative row-length marker byte in the outline stream
            // (segvga:1ea1 or al,al; js — a positive byte is a pixel entry).
            let n = globdata_reader.read_i8().unwrap();
            assert!(n < 0);

            let line_len = !n as i16;
            if line_len == 0 {
                break;
            }

            for x in 0..line_len {
                let n = match half {
                    Half::Upper => globdata_reader.read_i8().unwrap(),
                    Half::Lower => -globdata_reader.read_i8().unwrap(),
                };

                // = segvga:1d85 the globe_tilt_window_table lookup (indexed
                // from the table middle by the outline latitude byte). The
                // DOS window builder (seg000:ba2d) starts its south run at
                // tilt + 98, so the latitude visible at the window centre is
                // n - tilt: the tilt carries the map-row sign (positive =
                // south, negative = north) and enters this south-low /
                // north-high table negated.
                let section_latitude = self.tilt_lookup_table[(n as i16 + 196 - tilt) as usize];

                // = segvga:1d9e/1da0 the per-x-column GLOBDATA tables.
                let bx_ = self.globdata_table_2(x as usize, section_latitude.latitude as usize);
                let mut ax = self.globdata_table_3(x as usize, section_latitude.latitude as usize);

                // = segvga:1da7..1db8 rotation-table entry: selector/2 * 8
                // bytes → {row start bx, row len cx, rotation fp hi word dx}.
                let bp = (bx_ / 2) as usize;
                let mut bx = self.rotation_lookup_table[bp].map_row_start;
                let mut cx = self.rotation_lookup_table[bp].map_row_len as i16;
                let mut dx = (self.rotation_lookup_table[bp].fp >> 16) as i16;

                // = the four quadrant branches (segvga:1d9a/1dc2/1de8/1e11):
                // the far sections mirror the cell offset (ax = len - ax) and
                // the north sections negate the row start.
                match section_latitude.section {
                    GlobeSection::FarNorth => {
                        ax = cx - ax;
                        bx = -bx;
                    }
                    GlobeSection::NearNorth => {
                        bx = -bx;
                    }
                    GlobeSection::NearSouth => {}
                    GlobeSection::FarSouth => {
                        ax = cx - ax;
                    }
                };

                // = segvga:1e32..1e3e: wrap (fp hi − cell offset) into the
                // doubled row length, then add the row start.
                cx *= 2;
                let mut bp = dx - ax;
                if bp < 0 {
                    bp += cx;
                }
                bp += bx;
                dx += ax;

                // = segvga:1e47..1e66 the west pixel (std stosb through the
                // leftward cursor).
                let color = self.map_color(bp);
                let py = match half {
                    Half::Upper => center_y - y,
                    Half::Lower => center_y + y,
                };

                {
                    let x = (center_x - x) as u16;
                    fb.set(x, py as u16, color);
                }

                // = segvga:1e6b..1e73 the mirrored east cell offset.
                let mut bp = dx - cx;
                if bp < 0 {
                    bp += cx;
                }
                bp += bx;

                // = segvga:1e75..1e90 the east pixel (cld stosb through the
                // rightward cursor).
                let color = self.map_color(bp);
                {
                    let x = (center_x + x + 1) as u16;
                    fb.set(x, py as u16, color);
                }
            }

            y += 1;
        }
    }

    // = segvga:1cb6 vga_globe_init in its full-redraw mode (the
    // globe_draw_skips_pixel_stores = 0 patch): rebuild the rotation fp table
    // for `phase` (1/398ths of a revolution), then draw both hemispheres.
    pub fn draw(&mut self, fb: &mut FrameBuffer, phase: u16, tilt: i16) {
        let tilt = tilt.clamp(-96, 96);
        self.precalculate_globe_rotation_lookup_table(phase);
        self.draw_half(fb, Half::Upper, tilt);
        self.draw_half(fb, Half::Lower, tilt);
    }

    // = one vga_globe_setup call from the rotation frame task (seg000:b9b2):
    // DOS draws one outline row into fb1 per tick and returns carry on the
    // call that completes the pass. Partial passes are never presented, so
    // the port only counts the ticks here and reports the completing one —
    // tick_globe_rotation then draws the whole finished pass at once.
    pub fn tick_outline_row(&mut self) -> bool {
        self.pass_ticks_left -= 1;
        if self.pass_ticks_left > 0 {
            return false;
        }
        self.pass_ticks_left = self.ticks_per_pass;
        true
    }
}

// = seg001:2448 _word_218F8_rect — the globe-view navigation mouse hot-zone.
const GLOBE_NAV_RECT: Rect = rect(96, 25, 224, 134);

// = seg001:2440 _word_218F0_rect — the globe-view sprite clip rect
// (draw_globe_with_atmosphere copies it to _unk_2CCE4_sprite_clip_rect); the
// rotation frame task presents this rect from fb1 on each finished pass.
const GLOBE_CLIP_RECT: Rect = rect(96, 15, 234, 134);

impl GameState {
    // = seg000:b8a7 setup_globe_draw — load GLOBDATA and seed the globe
    // orientation from the zoomed-globe centre, then open FRESK (whose
    // palette the globe colours live in) and flush the palette.
    pub(crate) fn setup_globe_draw(&mut self) {
        // = seg000:b8a9..b8af si = 0x92 (GLOBDATA.HSQ) into RESOURCE_GLOBDATA
        // (seg001:4c60). The renderer also snapshots MAP.HSQ (= the res_map_ofs
        // buffer) and the TABLAT rows (= RESOURCE_TABLAT).
        let globdata = self
            .dat_file
            .read("GLOBDATA.HSQ")
            .expect("load GLOBDATA.HSQ");
        let tablat = self.dat_file.read("TABLAT.BIN").expect("load TABLAT.BIN");
        self.globe_renderer = Some(GlobeRenderer::new(&globdata, &self.map, &tablat));

        // = seg000:b8b2..b8ba dx = zoomed_globe_longitude,
        // bx = zoomed_globe_latitude; call set_globe_tilt_and_rotation.
        self.set_globe_tilt_and_rotation(self.zoomed_globe_longitude, self.zoomed_globe_latitude);

        // = seg000:b8bd..b8c0 ax = 1; open_resource_by_index — FRESK.HSQ
        // (also applies its embedded palette).
        self.open_sprite_bank(sprite_bank::FRESK);
        // = seg000:b8c3 jmp update_screen_palette.
        self.update_screen_palette();
    }

    // = seg000:ba75 set_globe_tilt_and_rotation — seed the rotation phase
    // (= the _dword_23DFC fixed point: hi word of 398 * longitude, i.e. the
    // longitude in 1/398ths of a revolution) and the clamped tilt
    // (_word_21910_globe_tilt). The DOS table rebuilds
    // (recalculate_globe_rotation_table, build_globe_tilt_window_table) are
    // folded into GlobeRenderer::draw.
    pub(crate) fn set_globe_tilt_and_rotation(&mut self, rotation: u16, tilt: i16) {
        // = seg000:ba78..ba7f mov ax,18eh; mul dx; mov [si],dx.
        self.globe_rotation = ((398 * rotation as u32) >> 16) as u16;
        // = seg000:ba86..ba96 clamp the tilt magnitude up to at least 0x20
        // (unsigned compares: 0..0x1f → 0x20, -0x1f..-1 → -0x20), so the view
        // is never exactly equator-centred.
        let tilt = if (tilt as u16) < 0x20 { 0x20 } else { tilt };
        let tilt = if (tilt as u16) >= 0xffe0 { -0x20 } else { tilt };
        self.globe_tilt = tilt;
    }

    // = seg000:b85a draw_globe_with_atmosphere — the globe main view: the
    // FRESK atmosphere ring with the globe pixels rendered inside it.
    pub(crate) fn draw_globe_with_atmosphere(&mut self) {
        // = seg000:b85a..b85d ax = 1; open_resource_by_index — FRESK.HSQ.
        self.open_sprite_bank(sprite_bank::FRESK);
        // = seg000:b860..b869 draw_sprite(ax=2, dx=0x5b, bx=0x14) — the
        // atmosphere ring at (91, 20). DOS register convention: dx=X, bx=Y.
        self.draw_active_bank_sprite(2, 0x5b, 0x14);
        // = seg000:b86c..b86f set_mouse_nav_rect(_word_218F8_rect).
        self.set_mouse_nav_rect(GLOBE_NAV_RECT);
        // = seg000:b872..b878 sprite clip rect = _word_218F0_rect
        // {96,15,234,134}. The port passes clip rects per draw call (see
        // map_screen_open) and the globe disc stays inside it, so nothing is
        // stored here.
        // = seg000:b87b jmp map_func_gfx.
        self.map_func_gfx();
    }

    // = seg000:b977 map_func_gfx — render the globe pixels into fb1
    // (es = _word_2D086_framebuffer_1_seg) from the MAP centre
    // (ds:si = res_map_ofs) and the rotation table (bp = RESOURCE_TABLAT),
    // via segvga vga_globe_init (the gfx vtable slot at seg001:3911).
    // al = globe_draw_skips_pixel_stores selects the full redraw (0, the only
    // mode the port implements) or the pixel-store-skipping patch.
    pub(crate) fn map_func_gfx(&mut self) {
        let phase = self.globe_rotation;
        let tilt = self.globe_tilt;
        if let Some(globe) = self.globe_renderer.as_mut() {
            globe.draw(&mut self.framebuffer, phase, tilt);
        }
    }

    // = seg000:b8ea add_globe_rotation_frame_task — install
    // frame_task_callback_0b9ae with interval 1 (bp=1). Tail-called by
    // intro2_scene_globe (seg000:02f5) and the map screen globe path
    // (seg000:b415).
    pub(crate) fn add_globe_rotation_frame_task(&mut self) {
        self.add_frame_task(1, crate::TaskId::GlobeRotation);
    }

    // = seg000:b9ae frame_task_callback_0b9ae — the globe rotation task. DOS
    // draws one outline row into fb1 per tick (es = fb1; vga_globe_setup) and
    // on the pass-complete carry return presents the sprite clip rect from
    // fb1 to the screen, then advances the rotation phase by 1 for the next
    // pass — the slow globe spin. The port draws nothing on the row ticks
    // (partial passes are never presented) and renders the whole pass on the
    // completing tick instead.
    pub(crate) fn tick_globe_rotation(&mut self) {
        let Some(globe) = self.globe_renderer.as_mut() else {
            return;
        };
        // = seg000:b9b2 call vga_globe_setup; jb — nothing to do until the
        // pass completes.
        if !globe.tick_outline_row() {
            return;
        }
        // The pass the task's row ticks drew, rendered in one go.
        self.map_func_gfx();
        // = seg000:b98e call loc_0baf2 — the player globe-cursor position.
        // It returns dx = 0 while _byte_227D_suppress_sky_240_255 is set (the
        // intro), so the cursor only shows on the in-game map screen; the
        // seg000:b9a2 draw_globe_cursor_at_dx_bx draw is not ported yet.
        // = seg000:b993..b999 restore_mouse_if_rect_intersects(sprite_clip_
        // rect) + update_screen_at_sprite_rect_updating_head; the seg000:b9a5
        // draw_mouse_cursor_if_needed rearm closes the bracket. The port's
        // present handles the cursor through its own protocol (cf.
        // map_view_redraw).
        self.present_screen_rect(GLOBE_CLIP_RECT);
        // = seg000:b9a8 mov ax,1; jmp globe_rotation_increment_ax.
        self.globe_rotation_increment(1);
    }

    // = seg000:b9e0 globe_rotation_increment_ax — advance the rotation phase
    // (the _dword_23DFC seed word) by ax, wrapping into 0..398 (one
    // revolution). The fall-through rebuild of the per-row fp table
    // (seg000:b9f4 → recalculate_globe_rotation_table) happens in the next
    // GlobeRenderer::draw.
    pub(crate) fn globe_rotation_increment(&mut self, ax: i16) {
        let mut phase = self.globe_rotation as i16 + ax;
        // = seg000:b9ea jns / add dx,cx — wrap a negative phase up.
        if phase < 0 {
            phase += 398;
        }
        // = seg000:b9ee cmp dx,cx; js / sub dx,cx — wrap an overflow down.
        if phase >= 398 {
            phase -= 398;
        }
        self.globe_rotation = phase as u16;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{DatFile, GameState};

    // Renders intro2 scene 2 (the parallax starfield with the globe and its
    // FRESK atmosphere ring) into fb1 and writes globe_intro2_scene2.png for
    // visual inspection. Asset-gated; run with:
    //   cargo test -p dune --bin dune -- --ignored intro2_globe
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn intro2_globe_scene_renders() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.initialize_resources();
        // = seg000:0332 (play_credits' tail): intro2 always enters with the
        // blit row base cleared.
        game.clear_global_y_offset();

        game.set_fb1_as_active_framebuffer();
        game.intro2_render_scene_for_test(2);

        // The globe disc renders in the 0x10..0x1f palette block around the
        // scene centre (159/160, 79).
        let cy = 79;
        let globe_pixels = (cy - 40..cy + 40)
            .map(|y| {
                (120..200)
                    .filter(|&x| (0x10..0x20).contains(&game.framebuffer.pixels()[y * 320 + x]))
                    .count()
            })
            .sum::<usize>();
        assert!(
            globe_pixels > 4000,
            "globe disc missing from the scene centre ({globe_pixels} map-colour pixels)"
        );

        game.framebuffer
            .write_png(&game.palette, "globe_intro2_scene2.png")
            .unwrap();

        // The scene also installed the rotation frame task (= seg000:02f5).
        // Its first finished pass repaints the same phase; the next passes
        // advance it (1/398 revolution each) and shift map cells, so the
        // framebuffer must diverge within a few passes (a pass is ~2 rows +
        // 1 ticks ≈ 121 ticks).
        let phase0 = game.globe_rotation;
        assert_eq!(phase0, ((398 * 0x1964u32) >> 16) as u16);
        let before = game.framebuffer.pixels().to_vec();
        let mut ticks = 0;
        while game.framebuffer.pixels() == &before[..] {
            game.tick_globe_rotation();
            ticks += 1;
            assert!(ticks < 2000, "globe rotation task never repainted");
        }
        assert_ne!(
            game.globe_rotation, phase0,
            "rotation phase did not advance"
        );

        // A quarter revolution ahead, for eyeballing the spin direction.
        game.globe_rotation_increment(100);
        game.map_func_gfx();
        game.framebuffer
            .write_png(&game.palette, "globe_intro2_scene2_quarter_turn.png")
            .unwrap();
    }
}
