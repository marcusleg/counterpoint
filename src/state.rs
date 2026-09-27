//! Small session state remembered across restarts: the most recently used folder and the
//! editor's zoom level. Kept in its own file, separate from the LLM settings in `config.rs`,
//! since it is a convenience rather than user configuration: a missing or broken state file
//! must never block the app.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::xdg;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub last_folder: Option<PathBuf>,
    pub zoom: Option<u32>,
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
            zoom: None,
        };
        assert_eq!(state.remembered_folder(), None);
    }

    #[test]
    fn remembered_folder_is_some_for_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let state = State {
            last_folder: Some(dir.path().to_path_buf()),
            zoom: None,
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
            last_folder: None,
            zoom: Some(100),
        }
        .save_to(&path)
        .unwrap();
        State {
            last_folder: None,
            zoom: Some(110),
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
