//! How the window names files: the folder under the title and the **Open Recent** entries.

use std::path::{Path, PathBuf};

/// `folder` with the home directory shortened to `~`, as file managers show it.
pub fn abbreviate_home(folder: &Path, home: &Path) -> String {
    match folder.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => folder.display().to_string(),
    }
}

/// The longest file name and folder an **Open Recent** entry shows. Menu labels do not
/// ellipsize, and every submenu is as wide as the widest one, so a long entry would widen the
/// whole main menu.
const RECENT_NAME_MAX_CHARS: usize = 30;
const RECENT_FOLDER_MAX_CHARS: usize = 20;

/// The **Open Recent** entries for `paths`: each file's name, plus its folder ("readme.md —
/// ~/blog") when another entry has the same name. Long names are shortened in the middle, long
/// folders at the start. Underscores are doubled because menu labels treat a single one as a
/// mnemonic marker.
pub fn recent_labels(paths: &[PathBuf], home: &Path) -> Vec<String> {
    let name = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    paths
        .iter()
        .map(|path| {
            let own_name = name(path);
            let mut label = shorten_middle(&own_name, RECENT_NAME_MAX_CHARS);
            if paths.iter().filter(|other| name(other) == own_name).count() > 1 {
                let folder = path
                    .parent()
                    .map(|folder| abbreviate_home(folder, home))
                    .unwrap_or_default();
                label = format!(
                    "{label} — {}",
                    shorten_folder(&folder, RECENT_FOLDER_MAX_CHARS)
                );
            }
            label.replace('_', "__")
        })
        .collect()
}

/// `folder` if it has at most `max` characters, otherwise "…" followed by its end, `max`
/// characters at most: the last folders that fit whole ("…/personal-blog"), or else the end of
/// the last one.
fn shorten_folder(folder: &str, max: usize) -> String {
    let chars: Vec<char> = folder.chars().collect();
    if chars.len() <= max {
        return folder.to_string();
    }
    let tail = &chars[chars.len() - (max - 1)..];
    let start = tail.iter().position(|&c| c == '/').unwrap_or(0);
    format!("…{}", tail[start..].iter().collect::<String>())
}

/// `text` if it has at most `max` characters, otherwise its start and end joined by "…", `max`
/// characters in all.
fn shorten_middle(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let head = (max - 1) / 2;
    let tail = max - 1 - head;
    let mut shortened: String = chars[..head].iter().collect();
    shortened.push('…');
    shortened.extend(&chars[chars.len() - tail..]);
    shortened
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(paths: &[&str]) -> Vec<String> {
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        recent_labels(&paths, Path::new("/home/u"))
    }

    #[test]
    fn recent_labels_show_only_the_file_name() {
        assert_eq!(
            labels(&["/home/u/blog/post.md", "/srv/notes/todo.md"]),
            ["post.md", "todo.md"]
        );
    }

    #[test]
    fn recent_labels_add_the_folder_when_two_files_share_a_name() {
        assert_eq!(
            labels(&[
                "/home/u/blog/readme.md",
                "/home/u/blog/post.md",
                "/srv/notes/readme.md",
            ]),
            ["readme.md — ~/blog", "post.md", "readme.md — /srv/notes"]
        );
    }

    #[test]
    fn recent_labels_shorten_a_long_name_and_folder_in_the_middle() {
        assert_eq!(
            labels(&[
                "/home/u/Documents/Writing/Blog/2026/drafts-for-review/\
                 a-rather-long-article-title-about-writing.md",
                "/home/u/a-rather-long-article-title-about-writing.md",
            ]),
            [
                "a-rather-long-…bout-writing.md — …/drafts-for-review",
                "a-rather-long-…bout-writing.md — ~",
            ]
        );
    }

    #[test]
    fn recent_labels_cut_a_long_folder_inside_its_last_part_if_that_alone_is_too_long() {
        assert_eq!(
            labels(&[
                "/home/u/an-extremely-long-folder-name/post.md",
                "/srv/post.md"
            ]),
            ["post.md — …ly-long-folder-name", "post.md — /srv"]
        );
    }

    #[test]
    fn recent_labels_double_underscores_so_they_are_not_mnemonics() {
        assert_eq!(
            labels(&["/home/u/my_blog/draft_1.md", "/srv/draft_1.md"]),
            ["draft__1.md — ~/my__blog", "draft__1.md — /srv"]
        );
    }
}
