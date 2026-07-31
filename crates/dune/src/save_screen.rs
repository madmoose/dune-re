//! Port-only: the custom named save/load panel (F5).
//!
//! The DOS game has no equivalent — its save UI is the two-slot mirror-room
//! verb menu (savegame.rs). This panel is a port addition: a modal overlay over
//! the frozen game scene with a typed filename field, a scrollable list of the
//! custom saves in `./saves/`, and SAVE / LOAD / CANCEL buttons. The files are
//! byte-identical to the built-in slot saves (`save_game_to` /
//! `load_game_from`); only the location and the UI are new.
//!
//! The panel follows the house modal style (the book screen's blocking wait
//! loop): the game loop is stalled, the game clock suspended, and the panel
//! owns all input and presentation until it closes. Drawing uses only the
//! bitmap font and fill+outline chrome in the HUD color family, so no sprite
//! bank (and thus no palette shift) is involved.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::{
    FbId, GameState, Rect, cmd,
    font::{Font, TextSize},
    framebuffer::FrameBuffer,
    gfx,
    rect::rect,
};

/// Directory the custom saves live in, relative to the working directory
/// (the built-in `dune37s?.sav` slots stay in the working directory itself).
const SAVES_DIR: &str = "saves";

const PANEL_RECT: Rect = rect(60, 14, 260, 178);
const NAME_RECT: Rect = rect(68, 28, 252, 40);
const LIST_RECT: Rect = rect(68, 46, 236, 146);
const ARROW_UP_RECT: Rect = rect(238, 46, 252, 60);
const ARROW_DOWN_RECT: Rect = rect(238, 132, 252, 146);
const BTN_SAVE_RECT: Rect = rect(68, 152, 126, 164);
const BTN_LOAD_RECT: Rect = rect(131, 152, 189, 164);
const BTN_CANCEL_RECT: Rect = rect(194, 152, 252, 164);

const ROW_H: i16 = 10;
const VISIBLE_ROWS: usize = 10;
const MAX_NAME_CHARS: usize = 20;

// The HUD/verb-strip color family — present in the fixed upper palette on
// every in-game screen (room and map view), so the panel never needs a
// palette load of its own.
const COL_FILL: u8 = 0xf3;
const COL_FRAME: u8 = 0xf5;
const COL_TEXT: u8 = 0xfa;
const COL_GREY: u8 = 0xf5;
const COL_FIELD: u8 = 0xf0;

/// Caret blink half-period and status-line lifetime, in PIT ticks (~5 ms).
const CARET_BLINK_TICKS: u64 = 60;
const STATUS_TICKS: u64 = 300;

/// One `saves/*.sav` file in the panel's list.
pub(crate) struct SaveFileEntry {
    /// The file stem (no directory, no `.sav`).
    pub(crate) name: String,
    /// The file's u16 game_time header; None when the file is unreadable or
    /// too short (the entry is listed greyed and cannot be loaded sensibly,
    /// but stays visible so the player knows the file exists).
    pub(crate) game_time: Option<u16>,
}

/// The panel's transient state — a local of `custom_save_panel_run`, never
/// stored on `GameState`.
pub(crate) struct SavePanel {
    pub(crate) entries: Vec<SaveFileEntry>,
    pub(crate) selected: Option<usize>,
    pub(crate) scroll: usize,
    /// The filename edit buffer (host-typed, pre-filtered to `[A-Za-z0-9 _.-]`).
    pub(crate) name: String,
    pub(crate) caret_on: bool,
    caret_tick: u64,
    /// A pending status line: the COMMAND.BIN string id (SAVE_SUCCESSFUL /
    /// SAVE_ERROR) and the tick it expires at.
    pub(crate) status: Option<(u16, u64)>,
}

enum PanelExit {
    /// ESC or the CANCEL button: restore the grabbed backdrop and resume.
    Cancel,
    /// A save file was loaded: post_load_fixups already rebuilt and presented
    /// the full screen, so the backdrop is discarded and the suspend nesting
    /// is already zeroed.
    Loaded,
}

impl SavePanel {
    fn new(entries: Vec<SaveFileEntry>, now: u64) -> Self {
        Self {
            entries,
            selected: None,
            scroll: 0,
            name: String::new(),
            caret_on: true,
            caret_tick: now,
            status: None,
        }
    }

    fn max_scroll(&self) -> usize {
        self.entries.len().saturating_sub(VISIBLE_ROWS)
    }

    fn scroll_by(&mut self, delta: i32) {
        let s = self.scroll as i32 + delta;
        self.scroll = s.clamp(0, self.max_scroll() as i32) as usize;
    }

    fn clamp_scroll_to_selection(&mut self) {
        if let Some(i) = self.selected {
            if i < self.scroll {
                self.scroll = i;
            } else if i >= self.scroll + VISIBLE_ROWS {
                self.scroll = i + 1 - VISIBLE_ROWS;
            }
        }
        self.scroll = self.scroll.min(self.max_scroll());
    }

    /// Select entry `i` and pre-fill the name field from it, so SAVE
    /// overwrites and LOAD targets it.
    fn select(&mut self, i: usize) {
        if i >= self.entries.len() {
            return;
        }
        self.selected = Some(i);
        self.name = self.entries[i].name.chars().take(MAX_NAME_CHARS).collect();
        self.clamp_scroll_to_selection();
    }

    /// Move the selection by `delta` rows (Up/Down/PgUp/PgDn), selecting the
    /// first entry when nothing is selected yet.
    fn select_step(&mut self, delta: i32) {
        if self.entries.is_empty() {
            return;
        }
        let i = match self.selected {
            Some(i) => (i as i32 + delta).clamp(0, self.entries.len() as i32 - 1) as usize,
            None => 0,
        };
        self.select(i);
    }
}

/// Scan `dir` for `*.sav` files, newest-modified first (ties broken by name,
/// case-insensitively). Any error — a missing directory included — yields an
/// empty list.
fn scan_custom_saves_in(dir: &Path) -> Vec<SaveFileEntry> {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(SystemTime, SaveFileEntry)> = Vec::new();
    for e in read_dir.flatten() {
        let path = e.path();
        if !path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("sav"))
        {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let mtime = e
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        found.push((
            mtime,
            SaveFileEntry {
                name: name.to_string(),
                game_time: GameState::save_game_timestamp_path(&path),
            },
        ));
    }
    found.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            a.1.name
                .to_ascii_lowercase()
                .cmp(&b.1.name.to_ascii_lowercase())
        })
    });
    found.into_iter().map(|(_, e)| e).collect()
}

fn scan_custom_saves() -> Vec<SaveFileEntry> {
    scan_custom_saves_in(Path::new(SAVES_DIR))
}

fn custom_save_path(name: &str) -> PathBuf {
    Path::new(SAVES_DIR).join(format!("{name}.sav"))
}

/// The final filename guard: trim, keep only `[A-Za-z0-9 _.-]`, cap the
/// length, and reject names that are empty or all dots (`.` / `..`). The
/// host input path pre-filters typed characters, but names can also arrive
/// from on-disk file stems via the pre-fill.
fn sanitize_save_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " _-.".contains(*c))
        .take(MAX_NAME_CHARS)
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        return None;
    }
    Some(cleaned)
}

/// Draw a byte string in the small font at (x, y), returning the x past the
/// last glyph. Bytes outside the font's 7-bit range render as '@', like the
/// debug overlay.
fn draw_bytes(font: &Font, fb: &mut FrameBuffer, x: i16, y: i16, color: u8, s: &[u8]) -> i16 {
    let mut x = x;
    for &b in s {
        let c = if b >= 0x80 { 0x40 } else { b };
        x += font.draw_glyph(fb, x as u16, y as u16, c, TextSize::Small, color as u16) as i16;
    }
    x
}

fn measure_bytes(font: &Font, s: &[u8]) -> i16 {
    s.iter()
        .map(|&b| font.glyph_width(if b >= 0x80 { 0x40 } else { b }, TextSize::Small) as i16)
        .sum()
}

fn draw_bytes_centered(font: &Font, fb: &mut FrameBuffer, cx: i16, y: i16, color: u8, s: &[u8]) {
    let w = measure_bytes(font, s);
    draw_bytes(font, fb, cx - w / 2, y, color, s);
}

impl GameState {
    // Port-only: open the custom save/load panel on an F5 (scancode 0x3f)
    // key-press edge. Reads the raw kb_keys state (not the one-shot scancode
    // buffer) so it never steals a keypress from the game.
    pub(crate) fn poll_custom_save_panel(&mut self) {
        const SCANCODE_F5: usize = 0x3f;
        let down = self.input.lock().unwrap().kb_keys[SCANCODE_F5] != 0;
        if down && !self.custom_save_key_down && self.custom_save_panel_allowed() {
            self.custom_save_panel_run();
        }
        self.custom_save_key_down = down;
    }

    // Port-only: whether the panel may open right now. game_loop only runs
    // in-game (the intro, videos, and the book have their own inner loops), so
    // the guards cover the in-game states the panel cannot sit on top of: a
    // travel/flight sequence, an active dialogue line, the book screen, and a
    // pending room change. Both the room view and the map view are fine — the
    // load path rebuilds either via room_view_toggle, and Cancel restores the
    // exact pixels.
    fn custom_save_panel_allowed(&self) -> bool {
        !self.is_headless()
            && self.travel_active == 0
            && !self.is_dialogue_active
            && self.data_000c6 & 1 == 0
            && self.pending_room_screen_request == 0
    }

    // Port-only: the panel's blocking modal loop (the book screen's wait-loop
    // shape). Owns all input and presentation until it closes; game_loop is
    // stalled for the duration and the game clock suspended.
    pub(crate) fn custom_save_panel_run(&mut self) {
        self.suspend_game_clock();
        // Disable the P-key GAME PAUSED window while the panel is open (the
        // book screen's idiom): get_mouse_pos_etc runs the pause check every
        // pass, and it would otherwise eat a typed 'p' and stall the panel.
        let saved_pause_enabled = self.pause_enabled;
        self.pause_enabled = 0;
        self.kb_drain_and_clear();
        // Lift the baked cursor before grabbing the backdrop so its pixels
        // are not captured into the save-under buffer.
        self.call_restore_cursor();
        let backdrop = gfx::vga_grab_rect(&self.screen, PANEL_RECT);
        let mut panel = SavePanel::new(scan_custom_saves(), self.game_ticks());
        self.custom_save_panel_draw(&panel);
        self.draw_mouse();
        self.send_frame_to_display();

        let exit = 'modal: loop {
            let start = self.game_ticks();
            self.get_mouse_pos_etc();
            let ax = self.mouse_stuff();
            if self.redraw_mouse() {
                self.send_frame_to_display();
            }
            let mut dirty = false;

            loop {
                let sc = self.get_and_reset_key_scancode();
                if sc == 0 {
                    break;
                }
                match sc {
                    // ESC — cancel.
                    0x01 => break 'modal PanelExit::Cancel,
                    // Enter — save under the typed name.
                    0x1c => {
                        self.custom_save_panel_do_save(&mut panel);
                        dirty = true;
                    }
                    // Backspace.
                    0x0e => {
                        panel.name.pop();
                        dirty = true;
                    }
                    // Up / Down (keypad codes; the arrow keys fold onto them).
                    0x48 => {
                        panel.select_step(-1);
                        dirty = true;
                    }
                    0x50 => {
                        panel.select_step(1);
                        dirty = true;
                    }
                    // PgUp / PgDn.
                    0x49 => {
                        panel.select_step(-(VISIBLE_ROWS as i32));
                        dirty = true;
                    }
                    0x51 => {
                        panel.select_step(VISIBLE_ROWS as i32);
                        dirty = true;
                    }
                    _ => {}
                }
            }

            for c in self.take_typed_chars() {
                if panel.name.len() < MAX_NAME_CHARS {
                    panel.name.push(c);
                    dirty = true;
                }
            }

            // A fresh LMB press: live bit 0 + edge bit 2.
            if ax & 5 == 5 {
                let (mx, my) = (self.mouse_pos_x as i16, self.mouse_pos_y as i16);
                if LIST_RECT.in_rect(mx, my) {
                    let row = ((my - LIST_RECT.y0) / ROW_H) as usize;
                    let i = panel.scroll + row;
                    if i < panel.entries.len() {
                        panel.select(i);
                        dirty = true;
                    }
                } else if ARROW_UP_RECT.in_rect(mx, my) {
                    panel.scroll_by(-1);
                    dirty = true;
                } else if ARROW_DOWN_RECT.in_rect(mx, my) {
                    panel.scroll_by(1);
                    dirty = true;
                } else if BTN_SAVE_RECT.in_rect(mx, my) {
                    self.custom_save_panel_do_save(&mut panel);
                    dirty = true;
                } else if BTN_LOAD_RECT.in_rect(mx, my) {
                    if self.custom_save_panel_do_load(&mut panel) {
                        break 'modal PanelExit::Loaded;
                    }
                    dirty = true;
                } else if BTN_CANCEL_RECT.in_rect(mx, my) {
                    break 'modal PanelExit::Cancel;
                }
            }

            let now = self.game_ticks();
            if now.saturating_sub(panel.caret_tick) >= CARET_BLINK_TICKS {
                panel.caret_on = !panel.caret_on;
                panel.caret_tick = now;
                dirty = true;
            }
            if let Some((_, expiry)) = panel.status
                && now >= expiry
            {
                panel.status = None;
                dirty = true;
            }

            if dirty {
                self.call_restore_cursor();
                self.custom_save_panel_draw(&panel);
                self.draw_mouse();
                self.send_frame_to_display();
            }
            self.sleep_ticks(start, 1);
        };

        match exit {
            PanelExit::Cancel => {
                self.call_restore_cursor();
                gfx::vga_put_rect(&mut self.screen, &backdrop, PANEL_RECT);
                self.draw_mouse();
                self.send_frame_to_display();
                self.resume_game_clock();
            }
            PanelExit::Loaded => {
                // post_load_fixups already zeroed the suspend nesting
                // (reset_game_suspend) and rebuilt + presented the full
                // screen; the grabbed backdrop is deliberately discarded.
            }
        }
        self.kb_drain_and_clear();
        // pause_enabled (seg001:ce80) lies outside the save image's state
        // block, so restoring the saved value is correct on the Loaded path
        // too.
        self.pause_enabled = saved_pause_enabled;
        // Re-anchor the clock and frame-task deltas: game_loop recomputes the
        // elapsed ticks from these on every pass, and without this the pass
        // after the modal loop would integrate the whole panel duration.
        self.last_task_tick = self.game_ticks();
        self.game_clock_last_tick = self.last_task_tick;
    }

    // Port-only: the SAVE action — sanitize the typed name, write
    // saves/<name>.sav, and refresh the list so the new entry (with its fresh
    // header) appears selected. The panel stays open; the status line is the
    // feedback.
    fn custom_save_panel_do_save(&mut self, panel: &mut SavePanel) {
        let expiry = self.game_ticks() + STATUS_TICKS;
        let Some(name) = sanitize_save_name(&panel.name) else {
            panel.status = Some((cmd::SAVE_ERROR, expiry));
            return;
        };
        let result = fs::create_dir_all(SAVES_DIR)
            .and_then(|()| self.save_game_to(&custom_save_path(&name)));
        match result {
            Ok(()) => {
                panel.name = name.clone();
                panel.entries = scan_custom_saves();
                panel.selected = panel.entries.iter().position(|e| e.name == name);
                panel.clamp_scroll_to_selection();
                panel.status = Some((cmd::SAVE_SUCCESSFUL, expiry));
            }
            Err(e) => {
                println!("custom save {name:?}: {e}");
                panel.status = Some((cmd::SAVE_ERROR, expiry));
            }
        }
    }

    // Port-only: the LOAD action — restore the selected entry's file through
    // the shared slot-load fixups. Returns true when a save was loaded (the
    // screen is already rebuilt and presented); false leaves the panel open.
    fn custom_save_panel_do_load(&mut self, panel: &mut SavePanel) -> bool {
        let Some(i) = panel.selected else {
            return false;
        };
        let path = custom_save_path(&panel.entries[i].name);
        let toggle = self.pre_load_fixups();
        match self.load_game_from(&path) {
            Ok(()) => {
                self.post_load_fixups(toggle);
                true
            }
            Err(e) => {
                println!("custom load {}: {e}", path.display());
                panel.status = Some((cmd::SAVE_ERROR, self.game_ticks() + STATUS_TICKS));
                false
            }
        }
    }

    // Port-only: a small solid triangle centered in a scroll-arrow box (the
    // font has no usable arrow glyphs), one fill per row.
    fn draw_arrow_triangle(&mut self, r: Rect, up: bool, color: u8) {
        let cx = (r.x0 + r.x1) / 2;
        let y0 = r.y0 + (r.y1 - r.y0) / 2 - 2;
        for k in 0..5i16 {
            let half = if up { k } else { 4 - k };
            gfx::vga_fill_rect(
                self,
                FbId::Screen,
                (cx - half) as u16,
                (y0 + k) as u16,
                (cx + half + 1) as u16,
                (y0 + k + 1) as u16,
                color,
            );
        }
    }

    // Port-only: paint the whole panel into the visible screen buffer. The
    // segvga primitives apply y_offset and target the active framebuffer, so
    // both are pinned (0 / Screen) for the duration to keep every coordinate
    // an absolute screen coordinate — matching the mouse position and the
    // grab/put backdrop rect. A full-panel repaint per change is trivially
    // cheap at 200x164.
    pub(crate) fn custom_save_panel_draw(&mut self, panel: &SavePanel) {
        let saved_fb = self.active_fb();
        let saved_yoff = self.y_offset;
        let saved_pattern = self.line_pattern;
        self.active_fb = FbId::Screen;
        self.y_offset = 0;
        self.line_pattern = 0xffff;

        // The panel body: fill + inset outline, plus a second inner outline
        // for depth.
        self.map_draw_panel_record(PANEL_RECT, COL_FILL, COL_FRAME);
        self.draw_rect_outline(
            PANEL_RECT.x0 + 3,
            PANEL_RECT.y0 + 3,
            PANEL_RECT.x1 - 4,
            PANEL_RECT.y1 - 4,
            COL_FIELD,
        );
        draw_bytes_centered(
            &self.font,
            &mut self.screen,
            160,
            18,
            COL_TEXT,
            b"SAVE / LOAD GAME",
        );

        // The filename field.
        gfx::vga_fill_rect(
            self,
            FbId::Screen,
            NAME_RECT.x0 as u16,
            NAME_RECT.y0 as u16,
            NAME_RECT.x1 as u16,
            NAME_RECT.y1 as u16,
            COL_FIELD,
        );
        self.draw_rect_outline(
            NAME_RECT.x0,
            NAME_RECT.y0,
            NAME_RECT.x1 - 1,
            NAME_RECT.y1 - 1,
            COL_FRAME,
        );
        let name_bytes: Vec<u8> = panel.name.bytes().collect();
        let end_x = draw_bytes(
            &self.font,
            &mut self.screen,
            NAME_RECT.x0 + 3,
            NAME_RECT.y0 + 3,
            COL_TEXT,
            &name_bytes,
        );
        if panel.caret_on {
            gfx::vga_fill_rect(
                self,
                FbId::Screen,
                (end_x + 1) as u16,
                (NAME_RECT.y0 + 3) as u16,
                (end_x + 2) as u16,
                (NAME_RECT.y0 + 10) as u16,
                COL_TEXT,
            );
        }

        // The save list.
        if panel.entries.is_empty() {
            draw_bytes_centered(
                &self.font,
                &mut self.screen,
                (LIST_RECT.x0 + LIST_RECT.x1) / 2,
                (LIST_RECT.y0 + LIST_RECT.y1) / 2 - 4,
                COL_GREY,
                b"NO SAVED GAMES",
            );
        }
        for row in 0..VISIBLE_ROWS {
            let i = panel.scroll + row;
            let Some(entry) = panel.entries.get(i) else {
                break;
            };
            let y0 = LIST_RECT.y0 + row as i16 * ROW_H;
            let selected = panel.selected == Some(i);
            if selected {
                gfx::vga_fill_rect(
                    self,
                    FbId::Screen,
                    LIST_RECT.x0 as u16,
                    y0 as u16,
                    LIST_RECT.x1 as u16,
                    (y0 + ROW_H) as u16,
                    COL_TEXT,
                );
            }
            let (name_col, time_col) = match (selected, entry.game_time.is_some()) {
                (true, _) => (COL_FILL, COL_FILL),
                (false, true) => (COL_TEXT, COL_TEXT),
                // An unreadable file: listed greyed, name only.
                (false, false) => (COL_GREY, COL_GREY),
            };
            let time_bytes = entry.game_time.map(|t| self.format_save_day_time(t));
            let time_w = time_bytes
                .as_ref()
                .map_or(0, |s| measure_bytes(&self.font, s));
            if let Some(s) = &time_bytes {
                draw_bytes(
                    &self.font,
                    &mut self.screen,
                    LIST_RECT.x1 - 3 - time_w,
                    y0 + 2,
                    time_col,
                    s,
                );
            }
            // The name, truncated glyph-by-glyph to the space left of the
            // day/time column.
            let max_w = LIST_RECT.x1 - 3 - time_w - 4 - (LIST_RECT.x0 + 3);
            let mut x = LIST_RECT.x0 + 3;
            for b in entry.name.bytes() {
                let c = if b >= 0x80 { 0x40 } else { b };
                let w = self.font.glyph_width(c, TextSize::Small) as i16;
                if x + w > LIST_RECT.x0 + 3 + max_w {
                    break;
                }
                x += self.font.draw_glyph(
                    &mut self.screen,
                    x as u16,
                    (y0 + 2) as u16,
                    c,
                    TextSize::Small,
                    name_col as u16,
                ) as i16;
            }
        }

        // The scroll arrows, greyed when the list cannot move that way.
        let up_col = if panel.scroll > 0 { COL_TEXT } else { COL_GREY };
        let down_col = if panel.scroll < panel.max_scroll() {
            COL_TEXT
        } else {
            COL_GREY
        };
        self.map_draw_panel_record(ARROW_UP_RECT, COL_FILL, COL_FRAME);
        self.draw_arrow_triangle(ARROW_UP_RECT, true, up_col);
        self.map_draw_panel_record(ARROW_DOWN_RECT, COL_FILL, COL_FRAME);
        self.draw_arrow_triangle(ARROW_DOWN_RECT, false, down_col);

        // The buttons.
        let save_col = if sanitize_save_name(&panel.name).is_some() {
            COL_TEXT
        } else {
            COL_GREY
        };
        let load_col = if panel.selected.is_some() {
            COL_TEXT
        } else {
            COL_GREY
        };
        for (r, label, col) in [
            (BTN_SAVE_RECT, &b"SAVE"[..], save_col),
            (BTN_LOAD_RECT, &b"LOAD"[..], load_col),
            (BTN_CANCEL_RECT, &b"CANCEL"[..], COL_TEXT),
        ] {
            self.map_draw_panel_record(r, COL_FILL, COL_FRAME);
            draw_bytes_centered(
                &self.font,
                &mut self.screen,
                (r.x0 + r.x1) / 2,
                r.y0 + 3,
                col,
                label,
            );
        }

        // The status line (SAVE SUCCESSFUL / SAVE ERROR), space-trimmed for
        // centering.
        if let Some((id, _)) = panel.status {
            let s: Vec<u8> = self
                .get_phrase_or_command_string(id)
                .iter()
                .copied()
                .filter(|&b| b != 0xff)
                .collect();
            let trimmed: &[u8] = {
                let start = s.iter().position(|&b| b != b' ').unwrap_or(0);
                let end = s.iter().rposition(|&b| b != b' ').map_or(0, |e| e + 1);
                &s[start..end]
            };
            draw_bytes_centered(&self.font, &mut self.screen, 160, 167, COL_TEXT, trimmed);
        }

        self.line_pattern = saved_pattern;
        self.y_offset = saved_yoff;
        self.active_fb = saved_fb;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::dat_file::DatFile;

    #[test]
    fn sanitize_save_name_cases() {
        assert_eq!(sanitize_save_name("Arrakeen 3"), Some("Arrakeen 3".into()));
        assert_eq!(sanitize_save_name("  padded  "), Some("padded".into()));
        assert_eq!(
            sanitize_save_name("we/ird:ch?ars"),
            Some("weirdchars".into())
        );
        assert_eq!(sanitize_save_name(""), None);
        assert_eq!(sanitize_save_name("   "), None);
        assert_eq!(sanitize_save_name("."), None);
        assert_eq!(sanitize_save_name(".."), None);
        assert_eq!(sanitize_save_name("///"), None);
        let long = "x".repeat(40);
        assert_eq!(sanitize_save_name(&long).unwrap().len(), MAX_NAME_CHARS);
    }

    #[test]
    fn scan_custom_saves_orders_and_filters() {
        let dir = std::env::temp_dir().join("dune_scan_custom_saves_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // A valid header (game_time 0x0123), an older valid file, a truncated
        // one-byte file, and a non-.sav file that must be ignored.
        fs::write(dir.join("older.sav"), [0x23, 0x01, 0, 0, 0, 0]).unwrap();
        fs::write(dir.join("broken.sav"), [0x42]).unwrap();
        fs::write(dir.join("ignored.txt"), [0, 0]).unwrap();
        // Ensure a distinct, newer mtime for newest.sav.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(dir.join("newest.sav"), [0x77, 0x02, 0, 0, 0, 0]).unwrap();

        let entries = scan_custom_saves_in(&dir);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "newest");
        assert_eq!(entries[0].game_time, Some(0x0277));
        let older = entries.iter().find(|e| e.name == "older").unwrap();
        assert_eq!(older.game_time, Some(0x0123));
        let broken = entries.iter().find(|e| e.name == "broken").unwrap();
        assert_eq!(broken.game_time, None);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_custom_saves_missing_dir_is_empty() {
        let dir = std::env::temp_dir().join("dune_scan_no_such_dir");
        let _ = fs::remove_dir_all(&dir);
        assert!(scan_custom_saves_in(&dir).is_empty());
    }

    // Render the panel headless over the starting throne room with a fake
    // entry list and write save_panel.png for inspection. Asset-gated. Run:
    //   cargo test -p dune --bin dune -- --ignored save_panel
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn save_panel_renders() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);

        let mut panel = SavePanel::new(
            vec![
                SaveFileEntry {
                    name: "before the harvester".into(),
                    game_time: Some(0x0123),
                },
                SaveFileEntry {
                    name: "sietch tabr".into(),
                    game_time: Some(0x0790),
                },
                SaveFileEntry {
                    name: "broken file".into(),
                    game_time: None,
                },
            ],
            0,
        );
        panel.select(1);
        panel.name = "sietch tabr".into();
        panel.caret_on = true;
        panel.status = Some((cmd::SAVE_SUCCESSFUL, u64::MAX));

        game.custom_save_panel_draw(&panel);
        game.screen
            .write_png_scaled(&game.palette, "save_panel.png")
            .expect("write save_panel.png");
    }

    // Round trip through a custom save path: save into a temp saves dir, find
    // it via the scanner with the right header word, and load it back into a
    // fresh GameState. Asset-gated. Run:
    //   cargo test -p dune --bin dune -- --ignored custom_save_round_trip
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn custom_save_round_trip() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let dir = std::env::temp_dir().join("dune_custom_save_round_trip");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let (tx, _rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(DatFile::open(dat_path).unwrap(), tx);
        game.set_headless();
        game.start(true);
        game.game_time = 0x0154;
        game.charisma = 77;

        let path = dir.join("round trip.sav");
        game.save_game_to(&path).unwrap();

        let entries = scan_custom_saves_in(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "round trip");
        assert_eq!(entries[0].game_time, Some(0x0154));

        let (tx2, _rx2) = mpsc::sync_channel(64);
        let mut fresh = GameState::new(DatFile::open(dat_path).unwrap(), tx2);
        fresh.set_headless();
        fresh.start(true);
        let toggle = fresh.pre_load_fixups();
        fresh.load_game_from(&path).unwrap();
        assert_eq!(fresh.game_time, 0x0154);
        assert_eq!(fresh.charisma, 77);
        fresh.post_load_fixups(toggle);

        let _ = dat_file;
        fs::remove_dir_all(&dir).unwrap();
    }
}
