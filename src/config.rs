//! LLM endpoint settings, stored in the user's settings file.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::json_file::{self, Durability};
use crate::xdg;

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
        let stored: Option<Self> = json_file::read(path)
            .map_err(|e| format!("Could not read the settings in {}: {e}", path.display()))?;
        Ok(stored.map_or_else(Self::default, |stored| {
            Self::from_fields(
                &stored.base_url,
                stored.api_key.as_deref().unwrap_or(""),
                stored.model.as_deref().unwrap_or(""),
            )
        }))
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&settings_path()?)
    }

    /// Writes the settings atomically and synced to disk, with owner-only permissions since
    /// they include the API key.
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        json_file::write(path, self, Durability::Synced)
            .map_err(|e| format!("Could not save the settings to {}: {e}", path.display()))
    }

    pub fn require_model(&self) -> Result<&str, String> {
        self.model.as_deref().ok_or_else(|| {
            "No model configured. Choose one under Preferences in the main menu.".to_string()
        })
    }

    /// Checks that `base_url` is an `http` or `https` URL without query, fragment or
    /// credentials, and returns it parsed. Credentials in the URL would be sent as a Basic
    /// Authorization header and printed in connection errors; the API key field exists for
    /// secrets.
    pub fn parse_base_url(base_url: &str) -> Result<reqwest::Url, String> {
        let url = reqwest::Url::parse(base_url.trim())
            .map_err(|e| format!("The base URL is not a valid URL: {e}."))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("The base URL must start with http:// or https://.".to_string());
        }
        if url.host_str().is_none() {
            return Err("The base URL has no host.".to_string());
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err("The base URL must not contain a query (?) or fragment (#).".to_string());
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(
                "The base URL must not contain a user name or password; use the API key field."
                    .to_string(),
            );
        }
        Ok(url)
    }

    /// True if requests to this endpoint would travel unencrypted beyond this machine, so an
    /// API key and the document would be readable on the network.
    pub fn sends_in_the_clear(&self) -> bool {
        match Self::parse_base_url(&self.base_url) {
            Ok(url) => {
                url.scheme() == "http"
                    && !matches!(
                        url.host_str(),
                        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
                    )
            }
            Err(_) => false,
        }
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
    xdg::user_file_from_env("XDG_CONFIG_HOME", &[".config"], "settings.json")
}

pub fn settings_path_from(lookup: impl Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    xdg::user_file(lookup, "XDG_CONFIG_HOME", &[".config"], "settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xdg::lookup;

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
    fn require_model_points_to_the_preferences_dialog() {
        let error = Config::default().require_model().unwrap_err();
        assert!(error.contains("Preferences in the main menu"), "{error}");
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

    #[test]
    fn parse_base_url_accepts_http_and_https() {
        assert!(Config::parse_base_url("http://localhost:11434/v1").is_ok());
        assert!(Config::parse_base_url(" https://api.example.com/v1/ ").is_ok());
    }

    #[test]
    fn parse_base_url_rejects_other_schemes_queries_and_credentials() {
        for url in [
            "ftp://example.com/v1",
            "localhost:11434/v1",
            "https://example.com/v1?x=1",
            "https://example.com/v1#frag",
            "https://user:secret@example.com/v1",
            "https://user@example.com/v1",
            "not a url",
        ] {
            let error = Config::parse_base_url(url).unwrap_err();
            assert!(!error.contains("secret"), "{url}: {error}");
        }
    }

    #[test]
    fn plain_http_beyond_loopback_sends_in_the_clear() {
        assert!(!Config::from_fields("http://localhost:11434/v1", "", "").sends_in_the_clear());
        assert!(!Config::from_fields("http://127.0.0.1:8765/v1", "", "").sends_in_the_clear());
        assert!(!Config::from_fields("http://[::1]:8765/v1", "", "").sends_in_the_clear());
        assert!(!Config::from_fields("https://api.example.com/v1", "", "").sends_in_the_clear());
        assert!(Config::from_fields("http://api.example.com/v1", "", "").sends_in_the_clear());
        assert!(Config::from_fields("http://192.168.1.5:11434/v1", "", "").sends_in_the_clear());
    }
}
