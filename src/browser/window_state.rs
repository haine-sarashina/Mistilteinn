//! Where the window was left.
//!
//! A browser that opens in the corner it was closed in is one less thing to
//! arrange every morning, so the window's position, size and maximized state
//! outlive the process in a file next to the bookmarks. The format is one
//! `key=value` line per field: unknown keys are skipped and missing ones fall
//! back to the default, so an older file still opens and a hand edit cannot
//! do worse than lose a placement.

use std::path::{Path, PathBuf};

/// The smallest window worth restoring to. A saved size below this came from
/// something going wrong rather than from a choice, and a window too small to
/// hold the chrome is one the user cannot recover from without a mouse drag.
const MIN_SIZE: (u32, u32) = (400, 300);

/// The window a first run gets.
const DEFAULT_SIZE: (u32, u32) = (1280, 800);

/// How much of the window has to land on a monitor for the placement to be
/// worth reusing: enough of the title bar to see it and grab it.
const MIN_VISIBLE: (i32, i32) = (120, 40);

/// A monitor's desktop rectangle: `(x, y, width, height)`.
pub type MonitorRect = (i32, i32, u32, u32);

/// Where the window sat and how big it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowGeometry {
    /// The top-left corner of the window frame, in desktop coordinates.
    ///
    /// `None` on a first run, and whenever the saved corner no longer lands on
    /// a monitor — the window manager places the window better than a stale
    /// guess would.
    pub position: Option<(i32, i32)>,
    /// The size of the drawable area, not counting the frame.
    pub size: (u32, u32),
    pub maximized: bool,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            position: None,
            size: DEFAULT_SIZE,
            maximized: false,
        }
    }
}

/// Where the geometry lives: alongside the rest of this app's data.
fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(base).join("Mistilteinn").join("window.conf"))
}

impl WindowGeometry {
    /// Read the saved geometry, or the default window if there is none yet.
    pub fn load() -> Self {
        match default_path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        }
    }

    /// Read the geometry from a specific file.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .map(|text| parse(&text))
            .unwrap_or_default()
    }

    /// Write the geometry out, creating the directory if it is not there yet.
    ///
    /// A failure is logged and otherwise ignored: opening where you were is a
    /// convenience, and refusing to close because a file could not be written
    /// would be a far worse trade.
    pub fn save(&self) {
        let Some(path) = default_path() else { return };
        self.save_to(&path);
    }

    /// Write the geometry to a specific file.
    pub fn save_to(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::warn!("could not create the window state directory {dir:?}: {e}");
                return;
            }
        }
        if let Err(e) = std::fs::write(path, serialize(self)) {
            log::warn!("could not save the window geometry to {path:?}: {e}");
        }
    }

    /// The geometry as it should actually be applied, given the monitors that
    /// are attached right now.
    ///
    /// A window saved on a monitor that has since been unplugged, or on a
    /// desktop that has since been rearranged, would come back somewhere the
    /// user cannot reach it. Rather than restore that, the corner is dropped
    /// and the window manager gets to place the window; the size is kept,
    /// since a size is useful wherever the window lands.
    pub fn sanitized(&self, monitors: &[MonitorRect]) -> Self {
        Self {
            position: self.position.filter(|_| self.is_on_screen(monitors)),
            size: (self.size.0.max(MIN_SIZE.0), self.size.1.max(MIN_SIZE.1)),
            maximized: self.maximized,
        }
    }

    /// Whether enough of the window would land on some monitor to be usable.
    ///
    /// Usable means two things. Enough of the window overlaps a screen to see
    /// it, and its top edge is not above that screen's top edge — the title
    /// bar is the only handle the window has, and one pushed up past the top
    /// of the desktop cannot be dragged back down.
    ///
    /// With no monitors reported at all — which some platforms do — the
    /// placement is taken on trust rather than thrown away.
    fn is_on_screen(&self, monitors: &[MonitorRect]) -> bool {
        if monitors.is_empty() {
            return true;
        }
        let Some((x, y)) = self.position else {
            return false;
        };
        let (w, h) = (self.size.0 as i32, self.size.1 as i32);
        monitors.iter().any(|&(mx, my, mw, mh)| {
            let overlap_w = (x + w).min(mx + mw as i32) - x.max(mx);
            let overlap_h = (y + h).min(my + mh as i32) - y.max(my);
            overlap_w >= MIN_VISIBLE.0 && overlap_h >= MIN_VISIBLE.1 && y >= my
        })
    }
}

/// Read the fields back out of a saved file.
///
/// A line that is blank, has no `=`, or carries a value that is not a number
/// is skipped: one unreadable field should cost that field, not the placement.
fn parse(text: &str) -> WindowGeometry {
    let mut geometry = WindowGeometry::default();
    let (mut x, mut y) = (None, None);

    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "x" => x = value.parse::<i32>().ok(),
            "y" => y = value.parse::<i32>().ok(),
            "width" => {
                if let Ok(w) = value.parse::<u32>() {
                    geometry.size.0 = w;
                }
            }
            "height" => {
                if let Ok(h) = value.parse::<u32>() {
                    geometry.size.1 = h;
                }
            }
            "maximized" => geometry.maximized = value == "true",
            _ => {}
        }
    }

    // A corner is only half a corner if one coordinate is missing, so both
    // have to have been read for the position to mean anything.
    geometry.position = x.zip(y);
    geometry
}

/// Write the fields out, one per line.
fn serialize(geometry: &WindowGeometry) -> String {
    let mut text = String::new();
    if let Some((x, y)) = geometry.position {
        text.push_str(&format!("x={x}\ny={y}\n"));
    }
    text.push_str(&format!(
        "width={}\nheight={}\nmaximized={}\n",
        geometry.size.0, geometry.size.1, geometry.maximized
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One monitor at the origin, the ordinary single-screen desktop.
    const PRIMARY: MonitorRect = (0, 0, 1920, 1080);

    fn geometry(x: i32, y: i32, w: u32, h: u32) -> WindowGeometry {
        WindowGeometry {
            position: Some((x, y)),
            size: (w, h),
            maximized: false,
        }
    }

    #[test]
    fn a_placement_survives_a_round_trip() {
        let saved = WindowGeometry {
            position: Some((-1720, 240)),
            size: (1024, 768),
            maximized: true,
        };
        assert_eq!(parse(&serialize(&saved)), saved);
    }

    #[test]
    fn a_first_run_has_no_corner_and_the_default_size() {
        let fresh = WindowGeometry::default();
        assert_eq!(fresh.position, None);
        assert_eq!(fresh.size, DEFAULT_SIZE);
        assert_eq!(parse(&serialize(&fresh)), fresh);
    }

    #[test]
    fn a_half_written_corner_is_not_used() {
        // A file cut off mid-write leaves an x with no y. Placing the window
        // at a guessed y is worse than letting the window manager place it.
        let parsed = parse("x=100\nwidth=800\nheight=600\n");
        assert_eq!(parsed.position, None);
        assert_eq!(parsed.size, (800, 600), "the size is still worth keeping");
    }

    #[test]
    fn unreadable_lines_cost_their_own_field_and_nothing_else() {
        let parsed = parse("width=oops\nheight=600\ngarbage\n\nmaximized=true\n");
        assert_eq!(parsed.size, (DEFAULT_SIZE.0, 600));
        assert!(parsed.maximized);
    }

    #[test]
    fn a_window_on_the_desktop_keeps_its_corner() {
        let saved = geometry(100, 100, 1280, 800);
        assert_eq!(saved.sanitized(&[PRIMARY]), saved);
    }

    #[test]
    fn a_window_from_an_unplugged_monitor_loses_its_corner_but_keeps_its_size() {
        // The second screen was to the left and is gone; -1700 is nowhere now.
        let saved = geometry(-1700, 200, 1024, 768);
        let restored = saved.sanitized(&[PRIMARY]);
        assert_eq!(restored.position, None);
        assert_eq!(restored.size, (1024, 768));
    }

    #[test]
    fn a_window_on_a_second_monitor_still_opens_there() {
        let saved = geometry(-1700, 200, 1024, 768);
        assert_eq!(
            saved.sanitized(&[PRIMARY, (-1920, 0, 1920, 1080)]),
            saved,
            "the screen it was on is still attached"
        );
    }

    #[test]
    fn a_window_hanging_off_an_edge_is_kept_while_the_title_bar_is_reachable() {
        // Windows are routinely left a little past the right edge; that is a
        // placement the user chose, not one to correct.
        let saved = geometry(1800, 300, 1280, 800);
        assert_eq!(saved.sanitized(&[PRIMARY]).position, Some((1800, 300)));

        let barely_there = geometry(1900, 300, 1280, 800);
        assert_eq!(
            barely_there.sanitized(&[PRIMARY]).position,
            None,
            "20 pixels of window is not something you can grab"
        );
    }

    #[test]
    fn a_window_above_the_desktop_is_not_restored() {
        // A negative y hides the title bar behind the top edge, leaving the
        // window there with no way to drag it back down.
        let saved = geometry(200, -60, 1280, 800);
        assert_eq!(saved.sanitized(&[PRIMARY]).position, None);
    }

    #[test]
    fn a_collapsed_size_is_grown_back_to_something_usable() {
        let saved = WindowGeometry {
            position: Some((0, 0)),
            size: (1, 1),
            maximized: false,
        };
        assert_eq!(saved.sanitized(&[PRIMARY]).size, MIN_SIZE);
    }

    #[test]
    fn with_no_monitors_reported_the_placement_is_taken_on_trust() {
        let saved = geometry(100, 100, 1280, 800);
        assert_eq!(saved.sanitized(&[]), saved);
    }

    #[test]
    fn a_maximized_window_comes_back_maximized() {
        let saved = WindowGeometry {
            position: Some((80, 40)),
            size: (1000, 700),
            maximized: true,
        };
        let restored = saved.sanitized(&[PRIMARY]);
        assert!(restored.maximized);
        assert_eq!(
            restored.size,
            (1000, 700),
            "un-maximizing has to land back on the size the user chose"
        );
    }

    #[test]
    fn geometry_is_written_and_read_back_from_disk() {
        let path =
            std::env::temp_dir().join(format!("mistilteinn-window-{}.conf", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let saved = geometry(120, 90, 1440, 900);
        saved.save_to(&path);
        assert_eq!(WindowGeometry::load_from(&path), saved);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_reads_as_a_first_run() {
        let path = std::env::temp_dir().join("mistilteinn-window-does-not-exist.conf");
        let _ = std::fs::remove_file(&path);
        assert_eq!(WindowGeometry::load_from(&path), WindowGeometry::default());
    }
}
