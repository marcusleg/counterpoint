//! LLM endpoint settings, stored in the user's settings file.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: None,
            model: None,
        }
    }
}

impl Config {
    /// Builds a configuration from user input; blank values count as unset.
    pub fn from_fields(base_url: &str, api_key: &str, model: &str) -> Self {
        let non_empty = |value: &str| Some(value.trim().to_string()).filter(|v| !v.is_empty());
        Self {
            base_url: non_empty(base_url).unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            api_key: non_empty(api_key),
            model: non_empty(model),
        }
    }

    /// Loads the user's settings file; a missing file yields the defaults.
    pub fn load() -> Result<Self, String> {
        Self::load_from(&settings_path()?)
    }

    pub fn load_from(path: &Path) -> Result<Self, String> {
        let error = |e: &dyn fmt::Display| {
            format!("Could not read the settings in {}: {e}", path.display())
        };
        let json = match fs::read_to_string(path) {
            Ok(json) => json,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(error(&e)),
        };
        let stored: Self = serde_json::from_str(&json).map_err(|e| error(&e))?;
        Ok(Self::from_fields(
            &stored.base_url,
            stored.api_key.as_deref().unwrap_or(""),
            stored.model.as_deref().unwrap_or(""),
        ))
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&settings_path()?)
    }

    /// Writes the settings atomically with owner-only permissions, since they include the API key.
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let error =
            |e: std::io::Error| format!("Could not save the settings to {}: {e}", path.display());
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(error)?;
        }
        let mut json = serde_json::to_string_pretty(self).expect("settings always serialize");
        json.push('\n');

        let temp_path = path.with_extension("json.tmp");
        // A leftover temp file could carry wider permissions; start from a fresh one.
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
                file.sync_all()
            })
            .and_then(|()| fs::rename(&temp_path, path));
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result.map_err(error)
    }

    pub fn require_model(&self) -> Result<&str, String> {
        self.model
            .as_deref()
            .ok_or_else(|| "No model configured. Choose one in Tools > Options….".to_string())
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .finish()
    }
}

/// The user's settings file: `$XDG_CONFIG_HOME/counterpoint/settings.json`, or
/// `~/.config/counterpoint/settings.json`.
pub fn settings_path() -> Result<PathBuf, String> {
    settings_path_from(|name| std::env::var(name).ok())
}

pub fn settings_path_from(lookup: impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    let config_home = lookup("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            lookup("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .ok_or_else(|| {
            "Cannot locate the settings file: neither XDG_CONFIG_HOME nor HOME is set.".to_string()
        })?;
    Ok(config_home.join("counterpoint").join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn defaults_use_the_local_ollama_endpoint() {
        let config = Config::default();
        assert_eq!(config.base_url, DEFAULT_BASE_URL);
        assert_eq!(config.api_key, None);
        assert_eq!(config.model, None);
    }

    #[test]
    fn from_fields_trims_and_treats_blank_as_unset() {
        let config = Config::from_fields("  https://api.example.com/v1 ", " secret ", " m ");
        assert_eq!(config.base_url, "https://api.example.com/v1");
        assert_eq!(config.api_key.as_deref(), Some("secret"));
        assert_eq!(config.model.as_deref(), Some("m"));
        assert_eq!(Config::from_fields(" ", "", "  "), Config::default());
    }

    #[test]
    fn require_model_returns_the_model() {
        assert_eq!(Config::from_fields("", "", "m").require_model(), Ok("m"));
    }

    #[test]
    fn require_model_points_to_the_options_dialog() {
        let error = Config::default().require_model().unwrap_err();
        assert!(error.contains("Tools > Options"), "{error}");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("counterpoint").join("settings.json");
        let config = Config::from_fields("https://api.example.com/v1", "secret", "m");
        config.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), Ok(config));
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Config::load_from(&path), Ok(Config::default()));
    }

    #[test]
    fn invalid_file_is_an_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "not json").unwrap();
        let error = Config::load_from(&path).unwrap_err();
        assert!(error.contains("settings.json"), "{error}");
    }

    #[test]
    fn blank_values_in_the_file_count_as_unset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"base_url": " ", "api_key": "", "model": ""}"#).unwrap();
        assert_eq!(Config::load_from(&path), Ok(Config::default()));
    }

    #[cfg(unix)]
    #[test]
    fn settings_file_is_readable_only_by_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        Config::from_fields("", "secret", "m")
            .save_to(&path)
            .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn saving_twice_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        Config::from_fields("", "", "a").save_to(&path).unwrap();
        Config::from_fields("", "", "b").save_to(&path).unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("settings.json")]);
        assert_eq!(
            Config::load_from(&path).unwrap().model.as_deref(),
            Some("b")
        );
    }

    #[test]
    fn settings_path_prefers_xdg_config_home() {
        let path = settings_path_from(lookup(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/u")]));
        assert_eq!(path, Ok(PathBuf::from("/xdg/counterpoint/settings.json")));
    }

    #[test]
    fn settings_path_falls_back_to_home_config() {
        let path = settings_path_from(lookup(&[
            ("XDG_CONFIG_HOME", "relative"),
            ("HOME", "/home/u"),
        ]));
        assert_eq!(
            path,
            Ok(PathBuf::from("/home/u/.config/counterpoint/settings.json"))
        );
    }

    #[test]
    fn settings_path_needs_home_or_xdg_config_home() {
        assert!(settings_path_from(lookup(&[])).is_err());
    }

    #[test]
    fn debug_output_hides_the_api_key() {
        let config = Config::from_fields("", "secret", "m");
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
    }
}
