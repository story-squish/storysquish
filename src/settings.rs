use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::theme::ColorScheme;
use crate::{APP_NAME, storage};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    pub color_scheme: ColorScheme,
    /// Where the board lives when no path is given on the command line,
    /// e.g. a folder synced by iCloud Drive, Dropbox or Syncthing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_path: Option<PathBuf>,
}

/// `~/.config/storysquish/settings.json` on every platform, honouring
/// `XDG_CONFIG_HOME` when it is set to an absolute path.
pub fn default_settings_path() -> Result<PathBuf> {
    Ok(config_dir(APP_NAME)?.join("settings.json"))
}

pub fn config_dir(app_name: &str) -> Result<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute());
    let config_home = match config_home {
        Some(dir) => dir,
        None => directories::BaseDirs::new()
            .context("could not determine the home directory")?
            .home_dir()
            .join(".config"),
    };
    Ok(config_home.join(app_name))
}

/// A missing file means defaults, not an error.
pub fn load(path: &Path) -> Result<Settings> {
    if !path.try_exists()? {
        return Ok(Settings::default());
    }
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("{} is not a valid settings file", path.display()))
}

pub fn save(path: &Path, settings: &Settings) -> Result<()> {
    storage::write_atomically(path, &serde_json::to_string_pretty(settings)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let settings = load(&dir.path().join("settings.json")).unwrap();
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn save_then_load_round_trips_into_new_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("storysquish/settings.json");
        let settings = Settings {
            color_scheme: ColorScheme::ForestNight,
            board_path: Some(dir.path().join("Dropbox/novel.json")),
        };
        save(&path, &settings).unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains(r#""color-scheme": "forest-night""#)
        );
        assert_eq!(load(&path).unwrap(), settings);
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{}").unwrap();
        assert_eq!(load(&path).unwrap(), Settings::default());
    }

    #[test]
    fn unknown_scheme_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"color-scheme": "neon"}"#).unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn default_path_lives_under_dot_config() {
        let path = default_settings_path().unwrap();
        assert!(path.ends_with("storysquish/settings.json"));
    }
}
