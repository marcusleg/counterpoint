//! The small JSON files Counterpoint keeps in the user's directories: the settings, the session
//! state and the chat history. Each of them holds something private (an API key, the user's
//! folders, their conversations), so they are written readable only by the owner, and
//! atomically, so a crash mid-write never leaves a truncated file in place of a good one.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Whether `write` flushes the new file to disk before renaming it into place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// `sync_all` before the rename, so the write survives a power loss once `write` returns.
    Synced,
    /// No `sync_all`: cheaper, for files rewritten often whose latest change may be lost.
    Unsynced,
}

/// Reads and parses the JSON file at `path`; a missing file is `Ok(None)`.
pub fn read<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read_to_string(path) {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Writes `value` as pretty-printed JSON to `path`, creating its folder. The JSON goes to
/// `<name>.json.tmp` next to it first, created with owner-only permissions, and is then renamed
/// into place; on failure the temp file is removed. A value that cannot be serialized writes
/// nothing.
pub fn write<T: Serialize>(path: &Path, value: &T, durability: Durability) -> io::Result<()> {
    let mut json = serde_json::to_string_pretty(value)?;
    json.push('\n');
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }

    let temp_path = path.with_extension("json.tmp");
    // A leftover temp file from a crash could carry wider permissions; start from a fresh one.
    let _ = fs::remove_file(&temp_path);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = options
        .open(&temp_path)
        .and_then(|mut file| {
            file.write_all(json.as_bytes())?;
            match durability {
                Durability::Synced => file.sync_all(),
                Durability::Unsynced => Ok(()),
            }
        })
        .and_then(|()| fs::rename(&temp_path, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::ffi::OsString;

    fn names_in(dir: &Path) -> Vec<OsString> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn write_then_read_round_trips_and_creates_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("counterpoint").join("file.json");
        let value = BTreeMap::from([("zoom".to_string(), 150)]);
        write(&path, &value, Durability::Synced).unwrap();
        assert_eq!(read(&path).unwrap(), Some(value));
        assert!(fs::read_to_string(&path).unwrap().ends_with("}\n"));
    }

    #[test]
    fn reading_a_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.json");
        assert_eq!(read::<BTreeMap<String, u32>>(&path).unwrap(), None);
    }

    #[test]
    fn reading_an_invalid_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.json");
        fs::write(&path, "not json").unwrap();
        assert!(read::<BTreeMap<String, u32>>(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_only_by_the_owner_even_after_a_leftover_temp_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.json");
        let temp_path = dir.path().join("file.json.tmp");
        fs::write(&temp_path, "stale").unwrap();
        fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o644)).unwrap();
        for durability in [Durability::Synced, Durability::Unsynced] {
            write(&path, &1, durability).unwrap();
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn writing_twice_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.json");
        write(&path, &1, Durability::Synced).unwrap();
        write(&path, &2, Durability::Unsynced).unwrap();
        assert_eq!(names_in(dir.path()), vec![OsString::from("file.json")]);
        assert_eq!(read(&path).unwrap(), Some(2));
    }

    #[test]
    fn a_failed_write_removes_the_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        // A non-empty folder in the file's place makes the rename fail after the temp file
        // has been written.
        let path = dir.path().join("file.json");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("inside"), "").unwrap();
        assert!(write(&path, &1, Durability::Synced).is_err());
        assert_eq!(names_in(dir.path()), vec![OsString::from("file.json")]);
    }
}
