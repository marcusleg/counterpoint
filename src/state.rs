//! Small session state remembered across restarts: the most recently used folder, the recently
//! opened files and the editor's zoom level. Kept in its own file, separate from the LLM
//! settings in `config.rs`, since it is a convenience rather than user configuration: a missing
//! or broken state file must never block the app.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::xdg;

/// How many files **Open Recent** lists.
pub const MAX_RECENT_FILES: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub last_folder: Option<PathBuf>,
    pub zoom: Option<u32>,
    /// Recently opened or saved files, newest first, at most `MAX_RECENT_FILES`.
    pub recent_files: Vec<PathBuf>,
}

impl State {
    /// Loads the state file; a missing or unreadable/invalid file yields the defaults.
    pub fn load() -> Self {
        match state_path() {
            Ok(path) => Self::load_from(&path),
            Err(_) => Self::default(),
        }
    }

    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&state_path()?)
    }

    /// Writes the state atomically, readable only by the owner since it names the user's
    /// folders; a leftover temp file from a previous crash is replaced.
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let error =
            |e: &dyn fmt::Display| format!("Could not save the state to {}: {e}", path.display());
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| error(&e))?;
        }
        let mut json = serde_json::to_string_pretty(self).map_err(|e| error(&e))?;
        json.push('\n');

        let temp_path = path.with_extension("json.tmp");
        let _ = fs::remove_file(&temp_path);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // No `sync_all` before the rename: this is a convenience file rewritten on every zoom
        // step on the main thread, and the temp-file-then-rename already prevents a truncated
        // file from ever replacing it.
        let result = options
            .open(&temp_path)
            .and_then(|mut file| file.write_all(json.as_bytes()))
            .and_then(|()| fs::rename(&temp_path, path));
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result.map_err(|e| error(&e))
    }

    /// Records `path` as just opened or saved: its folder becomes `last_folder` and the file
    /// moves to the front of the recent files. As in `add_recent`, a folder that is not valid
    /// UTF-8 is not recorded.
    pub fn remember_file(&mut self, path: &Path) {
        if let Some(folder) = path.parent().filter(|folder| folder.to_str().is_some()) {
            self.last_folder = Some(folder.to_path_buf());
        }
        self.add_recent(path);
    }

    /// Moves `path` to the front of the recent files, dropping the oldest beyond
    /// `MAX_RECENT_FILES`. A path that is not valid UTF-8 is skipped: JSON cannot hold it, and
    /// it would make every later save of the state fail.
    pub fn add_recent(&mut self, path: &Path) {
        if path.to_str().is_none() {
            return;
        }
        self.forget_recent(path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(MAX_RECENT_FILES);
    }

    pub fn forget_recent(&mut self, path: &Path) {
        self.recent_files.retain(|known| known != path);
    }

    /// `last_folder`, but only if it still exists as a directory.
    pub fn remembered_folder(&self) -> Option<&Path> {
        self.last_folder.as_deref().filter(|folder| folder.is_dir())
    }
}

/// The state file: `$XDG_STATE_HOME/counterpoint/state.json`, or
/// `~/.local/state/counterpoint/state.json`.
pub fn state_path() -> Result<PathBuf, String> {
    xdg::user_file_from_env("XDG_STATE_HOME", &[".local", "state"], "state.json")
}

pub fn state_path_from(lookup: impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    xdg::user_file(lookup, "XDG_STATE_HOME", &[".local", "state"], "state.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xdg::lookup;

    #[test]
    fn state_path_prefers_xdg_state_home() {
        let path = state_path_from(lookup(&[("XDG_STATE_HOME", "/xdg"), ("HOME", "/home/u")]));
        assert_eq!(path, Ok(PathBuf::from("/xdg/counterpoint/state.json")));
    }

    #[test]
    fn state_path_falls_back_to_home_local_state() {
        let path = state_path_from(lookup(&[
            ("XDG_STATE_HOME", "relative"),
            ("HOME", "/home/u"),
        ]));
        assert_eq!(
            path,
            Ok(PathBuf::from(
                "/home/u/.local/state/counterpoint/state.json"
            ))
        );
    }

    #[test]
    fn state_path_needs_home_or_xdg_state_home() {
        assert!(state_path_from(lookup(&[])).is_err());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("counterpoint").join("state.json");
        let state = State {
            last_folder: Some(PathBuf::from("/tmp/example")),
            zoom: Some(150),
            recent_files: vec![PathBuf::from("/tmp/example/post.md")],
        };
        state.save_to(&path).unwrap();
        assert_eq!(State::load_from(&path), state);
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        assert_eq!(State::load_from(&path), State::default());
    }

    #[test]
    fn invalid_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(State::load_from(&path), State::default());
    }

    #[test]
    fn remembered_folder_is_none_for_a_missing_directory() {
        let state = State {
            last_folder: Some(PathBuf::from("/no/such/directory/at/all")),
            ..Default::default()
        };
        assert_eq!(state.remembered_folder(), None);
    }

    #[test]
    fn remembered_folder_is_some_for_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let state = State {
            last_folder: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        assert_eq!(state.remembered_folder(), Some(dir.path()));
    }

    #[test]
    fn save_to_reports_an_error_instead_of_panicking_for_a_non_utf8_last_folder() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let state = State {
            last_folder: Some(PathBuf::from(OsStr::from_bytes(b"/tmp/caf\xe9"))),
            ..Default::default()
        };
        assert!(state.save_to(&path).is_err());
        assert!(!path.exists(), "a failed save must write nothing");
    }

    #[test]
    fn add_recent_puts_the_newest_file_first() {
        let mut state = State::default();
        state.add_recent(Path::new("/a.md"));
        state.add_recent(Path::new("/b.md"));
        assert_eq!(
            state.recent_files,
            vec![PathBuf::from("/b.md"), PathBuf::from("/a.md")]
        );
    }

    #[test]
    fn add_recent_moves_a_known_file_to_the_front_without_duplicating_it() {
        let mut state = State::default();
        state.add_recent(Path::new("/a.md"));
        state.add_recent(Path::new("/b.md"));
        state.add_recent(Path::new("/a.md"));
        assert_eq!(
            state.recent_files,
            vec![PathBuf::from("/a.md"), PathBuf::from("/b.md")]
        );
    }

    #[test]
    fn add_recent_keeps_only_the_ten_newest_files() {
        let mut state = State::default();
        for i in 0..12 {
            state.add_recent(&PathBuf::from(format!("/{i}.md")));
        }
        let expected: Vec<_> = (2..12)
            .rev()
            .map(|i| PathBuf::from(format!("/{i}.md")))
            .collect();
        assert_eq!(state.recent_files, expected);
    }

    #[test]
    fn add_recent_skips_a_non_utf8_path_so_the_state_can_still_be_saved() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let mut state = State::default();
        state.add_recent(Path::new(OsStr::from_bytes(b"/tmp/caf\xe9.md")));
        assert!(state.recent_files.is_empty());
    }

    #[test]
    fn remember_file_records_the_folder_and_the_recent_file() {
        let mut state = State::default();
        state.remember_file(Path::new("/blog/post.md"));
        assert_eq!(state.last_folder, Some(PathBuf::from("/blog")));
        assert_eq!(state.recent_files, vec![PathBuf::from("/blog/post.md")]);
    }

    #[test]
    fn remember_file_keeps_a_non_utf8_folder_out_so_the_state_can_still_be_saved() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut state = State::default();
        state.remember_file(Path::new("/blog/post.md"));
        state.remember_file(Path::new(OsStr::from_bytes(b"/caf\xe9/post.md")));
        assert_eq!(state.last_folder, Some(PathBuf::from("/blog")));
        assert!(state.save_to(&path).is_ok());
    }

    #[test]
    fn forget_recent_removes_only_that_file() {
        let mut state = State::default();
        state.add_recent(Path::new("/a.md"));
        state.add_recent(Path::new("/b.md"));
        state.forget_recent(Path::new("/a.md"));
        assert_eq!(state.recent_files, vec![PathBuf::from("/b.md")]);
    }

    #[test]
    fn a_state_file_without_recent_files_loads_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, r#"{"last_folder": "/tmp", "zoom": 120}"#).unwrap();
        let state = State::load_from(&path);
        assert_eq!(state.zoom, Some(120));
        assert!(state.recent_files.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn state_file_is_readable_only_by_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        State::default().save_to(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn saving_twice_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        State {
            zoom: Some(100),
            ..Default::default()
        }
        .save_to(&path)
        .unwrap();
        State {
            zoom: Some(110),
            ..Default::default()
        }
        .save_to(&path)
        .unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("state.json")]);
        assert_eq!(State::load_from(&path).zoom, Some(110));
    }
}
