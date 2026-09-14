//! Port-only autosaves (no DOS counterpart). Every pass of game_loop compares
//! game_phase with the phase last autosaved; when it differs, the current
//! state is written to `saves/autosave-<start>-phase-<hex>.sav`. `<start>` is
//! the UTC wall-clock stamp (`YYYYMMDD-HHMMSS`) of the moment the playthrough
//! began (start() or RESTART GAME) and `<hex>` the new game_phase, so one
//! playthrough leaves one file per story step.
//!
//! The check runs once per game_loop pass, not at every write to game_phase:
//! the DOS code sets the byte from some thirty places, many of them inside a
//! scripted scene where the state is mid-transition. Saving after the scene
//! has returned to the loop captures a state the load path can rebuild.

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{GameState, save_screen};

/// `saves/autosave-<series>-phase-<hex>.sav`.
fn autosave_path(series: &str, phase: u8) -> PathBuf {
    save_screen::custom_save_path(&format!("autosave-{series}-phase-{phase:02x}"))
}

/// `YYYYMMDD-HHMMSS` in UTC for a wall-clock instant.
fn utc_stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl GameState {
    /// Start a new autosave series: stamp the playthrough's start time and
    /// treat the current phase as already saved. Called where a fresh game
    /// begins (start() and RESTART GAME).
    pub(crate) fn autosave_begin_series(&mut self) {
        self.autosave_series = utc_stamp(SystemTime::now());
        self.autosave_last_phase = self.game_phase;
    }

    /// Re-sync after a load so the loaded phase is not written straight back
    /// out as a new autosave. The series (start stamp) is kept: a load inside
    /// a playthrough continues that playthrough's files.
    pub(crate) fn autosave_sync_phase(&mut self) {
        self.autosave_last_phase = self.game_phase;
    }

    /// The per-pass check: write an autosave when game_phase has moved since
    /// the last one. Failures are reported on stdout and never stop the game.
    pub(crate) fn autosave_on_phase_change(&mut self) {
        if self.game_phase == self.autosave_last_phase {
            return;
        }
        self.autosave_last_phase = self.game_phase;
        let path = autosave_path(&self.autosave_series, self.game_phase);
        let result =
            fs::create_dir_all(save_screen::SAVES_DIR).and_then(|()| self.save_game_to(&path));
        match result {
            Ok(()) => println!("autosave: {}", path.display()),
            Err(e) => println!("autosave {}: {e}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn utc_stamp_known_instants() {
        assert_eq!(utc_stamp(UNIX_EPOCH), "19700101-000000");
        // 2001-09-09 01:46:40 UTC.
        assert_eq!(
            utc_stamp(UNIX_EPOCH + Duration::from_secs(1_000_000_000)),
            "20010909-014640"
        );
        // 2000-02-29 12:00:00 UTC (a leap day in a 400-year leap year).
        assert_eq!(
            utc_stamp(UNIX_EPOCH + Duration::from_secs(951_825_600)),
            "20000229-120000"
        );
    }

    #[test]
    fn autosave_path_shape() {
        assert_eq!(
            autosave_path("20260912-143012", 0x28),
            PathBuf::from("saves/autosave-20260912-143012-phase-28.sav")
        );
    }
}
