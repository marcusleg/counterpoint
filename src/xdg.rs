//! Resolves per-user file locations following the XDG Base Directory specification.

use std::path::PathBuf;

/// `$<var>/counterpoint/<file>`, or `$HOME/<fallback...>/counterpoint/<file>` when `var` is
/// unset or not an absolute path. `lookup` reads environment variables, so tests can supply
/// their own.
pub fn user_file(
    lookup: impl Fn(&str) -> Option<String>,
    var: &str,
    fallback: &[&str],
    file: &str,
) -> Result<PathBuf, String> {
    let base = lookup(var)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            lookup("HOME").filter(|home| !home.is_empty()).map(|home| {
                fallback
                    .iter()
                    .fold(PathBuf::from(home), |path, part| path.join(part))
            })
        })
        .ok_or_else(|| format!("Cannot locate the {file} file: neither {var} nor HOME is set."))?;
    Ok(base.join("counterpoint").join(file))
}

/// `user_file` reading the real environment.
pub fn user_file_from_env(var: &str, fallback: &[&str], file: &str) -> Result<PathBuf, String> {
    user_file(|name| std::env::var(name).ok(), var, fallback, file)
}

#[cfg(test)]
pub(crate) fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: std::collections::HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name| map.get(name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_xdg_variable() {
        let path = user_file(
            lookup(&[("XDG_DATA_HOME", "/xdg"), ("HOME", "/home/u")]),
            "XDG_DATA_HOME",
            &[".local", "share"],
            "history.json",
        );
        assert_eq!(path, Ok(PathBuf::from("/xdg/counterpoint/history.json")));
    }

    #[test]
    fn falls_back_to_home_for_a_relative_or_missing_variable() {
        for vars in [
            vec![("XDG_DATA_HOME", "relative"), ("HOME", "/home/u")],
            vec![("HOME", "/home/u")],
        ] {
            let path = user_file(
                lookup(&vars),
                "XDG_DATA_HOME",
                &[".local", "share"],
                "history.json",
            );
            assert_eq!(
                path,
                Ok(PathBuf::from(
                    "/home/u/.local/share/counterpoint/history.json"
                ))
            );
        }
    }

    #[test]
    fn needs_home_or_the_variable() {
        let error = user_file(lookup(&[]), "XDG_DATA_HOME", &[".local"], "x.json").unwrap_err();
        assert!(error.contains("XDG_DATA_HOME"), "{error}");
    }
}
