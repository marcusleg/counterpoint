//! Reading and writing Markdown files.

use std::fs;
use std::io::Write;
use std::path::Path;

pub fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Could not open {}: {e}", path.display()))
}

/// True if `text` contains at least one CRLF line ending.
pub fn uses_crlf(text: &str) -> bool {
    text.contains("\r\n")
}

/// Converts every `\n` to `\r\n`. Only meaningful for text that contains no `\r\n` already
/// (the editor never produces `\r`), such as the plain-text source coming out of the editor.
pub fn to_crlf(text: &str) -> String {
    text.replace('\n', "\r\n")
}

/// Writes to a temporary file next to `path` (or, if `path` is a symlink, next to the file it
/// resolves to, so the link itself stays a link) and renames it into place, so a failed write
/// never leaves a truncated file behind. The original file's permissions are kept, but ownership,
/// hard links and extended attributes are not; a read-only file is replaced rather than left
/// unwritten.
pub fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    let error = |e: std::io::Error| format!("Could not save {}: {e}", path.display());
    let real_path = if path.exists() {
        fs::canonicalize(path).map_err(error)?
    } else {
        path.to_path_buf()
    };
    let file_name = real_path
        .file_name()
        .ok_or_else(|| format!("Could not save {}: not a file path", path.display()))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(".tmp");
    let temp_path = real_path.with_file_name(temp_name);

    let write_temp = || -> std::io::Result<()> {
        let file = fs::File::create(&temp_path)?;
        (&file).write_all(contents.as_bytes())?;
        file.sync_all()
    };
    if let Err(e) = write_temp() {
        let _ = fs::remove_file(&temp_path);
        return Err(error(e));
    }
    if let Ok(metadata) = fs::metadata(&real_path) {
        // Best effort: a failure here only means the saved file gets default permissions.
        let _ = fs::set_permissions(&temp_path, metadata.permissions());
    }
    fs::rename(&temp_path, &real_path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        error(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_utf8_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        write_file(&path, "# Grüße\n\nText.\n").unwrap();
        assert_eq!(read_file(&path), Ok("# Grüße\n\nText.\n".to_string()));
    }

    #[test]
    fn read_error_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.md");
        let error = read_file(&path).unwrap_err();
        assert!(error.starts_with("Could not open"), "{error}");
        assert!(error.contains("missing.md"), "{error}");
    }

    #[test]
    fn write_error_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-such-dir").join("post.md");
        let error = write_file(&path, "x").unwrap_err();
        assert!(error.starts_with("Could not save"), "{error}");
        assert!(error.contains("post.md"), "{error}");
    }

    #[test]
    fn overwrites_existing_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        write_file(&path, "old\n").unwrap();
        write_file(&path, "new\n").unwrap();
        assert_eq!(read_file(&path), Ok("new\n".to_string()));
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("post.md")]);
    }

    #[cfg(unix)]
    #[test]
    fn keeps_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        write_file(&path, "old\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_file(&path, "new\n").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn detects_crlf() {
        assert!(uses_crlf("a\r\nb\n"));
        assert!(!uses_crlf("a\nb\n"));
    }

    #[test]
    fn converts_lf_to_crlf() {
        assert_eq!(to_crlf("a\nb\n"), "a\r\nb\r\n");
    }

    #[cfg(unix)]
    #[test]
    fn writing_through_a_symlink_updates_the_target_and_keeps_the_link() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.md");
        let link = dir.path().join("link.md");
        write_file(&target, "old\n").unwrap();
        symlink(&target, &link).unwrap();

        write_file(&link, "new\n").unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(read_file(&target), Ok("new\n".to_string()));
        assert_eq!(read_file(&link), Ok("new\n".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn failed_temp_write_leaves_no_leftover_temp_file() {
        use std::os::unix::fs::PermissionsExt;

        if std::env::var("USER") == Ok("root".to_string()) {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        write_file(&path, "old\n").unwrap();

        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();

        // Root (and some filesystems/mounts) can still write into a read-only directory;
        // skip the assertion in that case rather than fail for the wrong reason.
        let probe = dir.path().join(".probe");
        let directory_is_actually_read_only = fs::write(&probe, "x").is_err();
        let _ = fs::remove_file(&probe);

        if directory_is_actually_read_only {
            let result = write_file(&path, "new\n");
            assert!(result.is_err());
        }

        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();

        if directory_is_actually_read_only {
            let names: Vec<_> = fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(names, vec![std::ffi::OsString::from("post.md")]);
        }
    }
}
