//! Reading and writing Markdown files.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The file behind the editor's text, with what it takes to save that text back the way the
/// file was written and to notice when another program changed the file in the meantime. The
/// default is a new document, which has no file yet and is saved with LF line endings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenFile {
    path: Option<PathBuf>,
    format: DiskFormat,
    /// The file's modification time when it was last read or written here.
    modified: Option<SystemTime>,
}

impl OpenFile {
    /// Reads the file at `path`, returning its text as the editor holds it and the open file.
    pub fn read(path: &Path) -> Result<(String, Self), String> {
        let contents = read_file(path)?;
        let (text, format) = from_disk(&contents);
        let file = Self {
            path: Some(path.to_path_buf()),
            format,
            modified: modification_time(path),
        };
        Ok((text, file))
    }

    /// Writes the editor's `text` to `path` in this file's line endings and byte order mark, so
    /// that Save As keeps them too, and returns the file as saved there.
    pub fn write(&self, path: &Path, text: &str) -> Result<Self, String> {
        write_file(path, &to_disk(text, self.format))?;
        Ok(Self {
            path: Some(path.to_path_buf()),
            format: self.format,
            modified: modification_time(path),
        })
    }

    /// The file, or `None` for a new document.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The file's name, or "Untitled" for a new document.
    pub fn name(&self) -> String {
        self.path()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string())
    }

    /// True if the file's modification time is no longer the one it had when it was last read
    /// or written here. A file whose time is unknown, or that is gone, does not count as
    /// changed: there is nothing on disk that saving would lose.
    pub fn changed_on_disk(&self) -> bool {
        let current = self.path().and_then(modification_time);
        matches!((self.modified, current), (Some(known), Some(current)) if known != current)
    }
}

fn modification_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Reads a UTF-8 text file. Files that are not UTF-8, or that contain NUL bytes (which a GTK
/// text buffer cannot hold), are rejected with a message naming the file.
pub fn read_file(path: &Path) -> Result<String, String> {
    let contents = fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::InvalidData {
            format!("Could not open {}: it is not UTF-8 text.", path.display())
        } else {
            format!("Could not open {}: {e}", path.display())
        }
    })?;
    if contents.contains('\0') {
        return Err(format!(
            "Could not open {}: it is not a text file (it contains NUL bytes).",
            path.display()
        ));
    }
    Ok(contents)
}

/// What a file looked like on disk beyond its text, so a save can reproduce it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiskFormat {
    /// The file had at least one CRLF line ending; it is saved entirely with CRLF.
    pub crlf: bool,
    /// The file started with a UTF-8 byte order mark.
    pub bom: bool,
}

const BOM: char = '\u{FEFF}';

/// Prepares file contents for the editor: strips a leading byte order mark (which would sit
/// invisibly before the cursor otherwise), turns every CRLF line ending into LF, and reports
/// both so a save can restore them.
pub fn from_disk(text: &str) -> (String, DiskFormat) {
    let (text, bom) = match text.strip_prefix(BOM) {
        Some(rest) => (rest, true),
        None => (text, false),
    };
    let crlf = text.contains("\r\n");
    let text = if crlf {
        text.replace("\r\n", "\n")
    } else {
        text.to_string()
    };
    (text, DiskFormat { crlf, bom })
}

/// Prepares editor text for saving: CRLF (e.g. pasted) becomes LF, then, if `format.crlf`,
/// every LF becomes CRLF, and the byte order mark is put back if there was one. A lone `\r` is
/// kept.
pub fn to_disk(text: &str, format: DiskFormat) -> String {
    let text = text.replace("\r\n", "\n");
    let text = if format.crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    };
    if format.bom {
        format!("{BOM}{text}")
    } else {
        text
    }
}

/// Writes to a temporary file next to `path` (or, if `path` is a symlink, next to the file it
/// resolves to, so the link itself stays a link) and renames it into place, so a failed write
/// never leaves a truncated file behind. The temporary file is created exclusively, so a stale
/// one, or a symlink planted under its name in a shared folder, is never written through. The
/// original file's permissions are kept, but ownership, hard links and extended attributes are
/// not; a read-only file is replaced rather than left unwritten.
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

    let permissions = fs::metadata(&real_path).ok().map(|m| m.permissions());
    let write_temp = || -> std::io::Result<()> {
        let _ = fs::remove_file(&temp_path);
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        (&file).write_all(contents.as_bytes())?;
        if let Some(permissions) = permissions {
            // Best effort, on the open handle rather than the path: a failure here only means
            // the saved file gets default permissions.
            let _ = file.set_permissions(permissions);
        }
        file.sync_all()
    };
    if let Err(e) = write_temp() {
        let _ = fs::remove_file(&temp_path);
        return Err(error(e));
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

    const LF: DiskFormat = DiskFormat {
        crlf: false,
        bom: false,
    };
    const CRLF: DiskFormat = DiskFormat {
        crlf: true,
        bom: false,
    };

    #[test]
    fn lf_text_passes_through_unchanged() {
        assert_eq!(from_disk("a\nb\n"), ("a\nb\n".to_string(), LF));
        assert_eq!(to_disk("a\nb\n", LF), "a\nb\n");
    }

    #[test]
    fn crlf_text_is_edited_as_lf_and_saved_as_crlf() {
        assert_eq!(from_disk("a\r\nb\r\n"), ("a\nb\n".to_string(), CRLF));
        assert_eq!(to_disk("a\nb\n", CRLF), "a\r\nb\r\n");
    }

    #[test]
    fn any_crlf_marks_the_file_as_crlf() {
        assert_eq!(from_disk("a\r\nb\n"), ("a\nb\n".to_string(), CRLF));
    }

    #[test]
    fn pasted_crlf_is_normalised_on_save() {
        assert_eq!(to_disk("a\r\nb\n", CRLF), "a\r\nb\r\n");
        assert_eq!(to_disk("a\r\nb\n", LF), "a\nb\n");
    }

    #[test]
    fn lone_carriage_return_is_kept() {
        assert_eq!(from_disk("a\rb\r\n"), ("a\rb\n".to_string(), CRLF));
        assert_eq!(to_disk("a\rb\n", CRLF), "a\rb\r\n");
        assert_eq!(to_disk("a\rb\n", LF), "a\rb\n");
    }

    #[test]
    fn byte_order_mark_is_stripped_and_restored() {
        let with_bom = DiskFormat {
            crlf: false,
            bom: true,
        };
        assert_eq!(from_disk("\u{FEFF}# T\n"), ("# T\n".to_string(), with_bom));
        assert_eq!(to_disk("# T\n", with_bom), "\u{FEFF}# T\n");
        // Only a leading mark is a byte order mark.
        assert_eq!(from_disk("a\u{FEFF}b"), ("a\u{FEFF}b".to_string(), LF));
        assert_eq!(
            from_disk("\u{FEFF}a\r\n"),
            (
                "a\n".to_string(),
                DiskFormat {
                    crlf: true,
                    bom: true
                }
            )
        );
    }

    /// Gives the file at `path` a modification time far from now, as another program saving it
    /// would.
    fn touch_long_ago(path: &Path) {
        let long_ago = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
    }

    #[test]
    fn a_new_document_is_untitled_without_a_file_and_never_changed_on_disk() {
        let file = OpenFile::default();
        assert_eq!(file.path(), None);
        assert_eq!(file.name(), "Untitled");
        assert!(!file.changed_on_disk());
    }

    #[test]
    fn an_open_file_is_named_after_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        fs::write(&path, "# Post\n").unwrap();
        let (text, file) = OpenFile::read(&path).unwrap();
        assert_eq!(text, "# Post\n");
        assert_eq!(file.path(), Some(path.as_path()));
        assert_eq!(file.name(), "post.md");
    }

    #[test]
    fn an_open_file_notices_when_another_program_changed_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        fs::write(&path, "old\n").unwrap();
        let (_, file) = OpenFile::read(&path).unwrap();
        assert!(!file.changed_on_disk());

        fs::write(&path, "changed elsewhere\n").unwrap();
        touch_long_ago(&path);
        assert!(file.changed_on_disk());
    }

    #[test]
    fn saving_counts_as_the_latest_change_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        fs::write(&path, "old\n").unwrap();
        let (_, file) = OpenFile::read(&path).unwrap();
        touch_long_ago(&path);
        assert!(file.changed_on_disk());

        let saved = file.write(&path, "new\n").unwrap();
        assert!(!saved.changed_on_disk());
    }

    #[test]
    fn a_deleted_file_does_not_count_as_changed_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        fs::write(&path, "old\n").unwrap();
        let (_, file) = OpenFile::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(!file.changed_on_disk());
    }

    #[test]
    fn saving_keeps_the_line_endings_and_byte_order_mark_of_the_file_that_was_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        fs::write(&path, "\u{FEFF}a\r\nb\r\n").unwrap();
        let (text, file) = OpenFile::read(&path).unwrap();
        assert_eq!(text, "a\nb\n");

        file.write(&path, "a\nb\nc\n").unwrap();
        assert_eq!(read_file(&path), Ok("\u{FEFF}a\r\nb\r\nc\r\n".to_string()));
    }

    #[test]
    fn save_as_keeps_the_format_and_moves_to_the_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        let copy = dir.path().join("copy.md");
        fs::write(&path, "a\r\n").unwrap();
        let (_, file) = OpenFile::read(&path).unwrap();

        let saved = file.write(&copy, "a\nb\n").unwrap();
        assert_eq!(read_file(&copy), Ok("a\r\nb\r\n".to_string()));
        assert_eq!(read_file(&path), Ok("a\r\n".to_string()));
        assert_eq!(saved.path(), Some(copy.as_path()));
        assert_eq!(saved.name(), "copy.md");
    }

    #[test]
    fn a_new_document_is_saved_with_lf_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        OpenFile::default().write(&path, "a\r\nb\n").unwrap();
        assert_eq!(read_file(&path), Ok("a\nb\n".to_string()));
    }

    #[test]
    fn non_utf8_file_is_rejected_with_a_clear_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin1.md");
        fs::write(&path, b"caf\xe9\n").unwrap();
        let error = read_file(&path).unwrap_err();
        assert!(error.contains("not UTF-8 text"), "{error}");
        assert!(error.contains("latin1.md"), "{error}");
    }

    #[test]
    fn file_with_nul_bytes_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary.md");
        fs::write(&path, b"text\0more\n").unwrap();
        let error = read_file(&path).unwrap_err();
        assert!(error.contains("NUL"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_planted_as_the_temp_file_is_not_written_through() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("post.md");
        let victim = dir.path().join("victim");
        fs::write(&victim, "untouched\n").unwrap();
        symlink(&victim, dir.path().join(".post.md.tmp")).unwrap();

        write_file(&path, "new\n").unwrap();

        assert_eq!(read_file(&victim), Ok("untouched\n".to_string()));
        assert_eq!(read_file(&path), Ok("new\n".to_string()));
        assert!(!dir.path().join(".post.md.tmp").exists());
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
            eprintln!("skipped: running as root, who can write into read-only directories");
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
