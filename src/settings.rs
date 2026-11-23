use std::collections::BTreeMap;
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
    /// The board open before the current one, offered by Open → Local.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_board_path: Option<PathBuf>,
    /// Private GitHub repository (`owner/name`) per board file, so each
    /// novel keeps its own history.
    #[serde(rename = "github-repos", skip_serializing_if = "BTreeMap::is_empty")]
    repos: BTreeMap<PathBuf, String>,
    #[serde(alias = "gist-auto-backup")]
    pub github_auto_backup: bool,
}

impl Settings {
    pub fn repo(&self, board: &Path) -> Option<&str> {
        self.repos.get(&board_key(board)).map(String::as_str)
    }

    pub fn set_repo(&mut self, board: &Path, repo: Option<String>) {
        let key = board_key(board);
        match repo {
            Some(repo) => self.repos.insert(key, repo),
            None => self.repos.remove(&key),
        };
    }

    pub fn boards_linked_to<'a>(&'a self, repo: &'a str) -> impl Iterator<Item = &'a Path> {
        self.repos
            .iter()
            .filter(move |(_, linked)| linked.as_str() == repo)
            .map(|(board, _)| board.as_path())
    }

    pub fn move_repo(&mut self, from: &Path, to: &Path) {
        let repo = self.repos.remove(&board_key(from));
        self.set_repo(to, repo);
    }
}

/// Relative command-line paths and remembered absolute ones must agree.
fn board_key(board: &Path) -> PathBuf {
    std::path::absolute(board).unwrap_or_else(|_| board.to_owned())
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
        let board = dir.path().join("Dropbox/novel.json");
        let mut settings = Settings {
            color_scheme: ColorScheme::ForestNight,
            board_path: Some(board.clone()),
            previous_board_path: Some(dir.path().join("emma.json")),
            github_auto_backup: true,
            ..Settings::default()
        };
        settings.set_repo(&board, Some("someone/storysquish-dune".to_owned()));
        save(&path, &settings).unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains(r#""color-scheme": "forest-night""#)
        );
        assert_eq!(load(&path).unwrap(), settings);
    }

    #[test]
    fn each_board_has_its_own_repo_and_moving_keeps_it() {
        let mut settings = Settings::default();
        settings.set_repo(Path::new("/novels/dune.json"), Some("aaa".to_owned()));
        settings.set_repo(Path::new("/novels/emma.json"), Some("bbb".to_owned()));
        assert_eq!(settings.repo(Path::new("/novels/dune.json")), Some("aaa"));
        assert_eq!(settings.repo(Path::new("/novels/emma.json")), Some("bbb"));

        settings.move_repo(Path::new("/novels/dune.json"), Path::new("/sync/dune.json"));
        assert_eq!(settings.repo(Path::new("/novels/dune.json")), None);
        assert_eq!(settings.repo(Path::new("/sync/dune.json")), Some("aaa"));

        assert_eq!(
            settings.boards_linked_to("aaa").collect::<Vec<_>>(),
            [Path::new("/sync/dune.json")]
        );
        assert_eq!(settings.boards_linked_to("zzz").count(), 0);

        settings.set_repo(Path::new("/sync/dune.json"), None);
        assert_eq!(settings.repo(Path::new("/sync/dune.json")), None);
    }

    #[test]
    fn relative_and_absolute_board_paths_share_a_repo() {
        let mut settings = Settings::default();
        settings.set_repo(Path::new("novel.json"), Some("aaa".to_owned()));
        let absolute = std::env::current_dir().unwrap().join("novel.json");
        assert_eq!(settings.repo(&absolute), Some("aaa"));
    }

    #[test]
    fn gist_links_are_dropped_and_auto_backup_carries_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let board = dir.path().join("dune.json");
        let old_settings = serde_json::json!({
            "gist-id": "aaa",
            "gists": { board.to_string_lossy(): "bbb" },
            "gist-auto-backup": true,
        });
        fs::write(&path, old_settings.to_string()).unwrap();
        let settings = load(&path).unwrap();

        assert_eq!(settings.repo(&board), None);
        assert!(settings.github_auto_backup);
        save(&path, &settings).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("gist"), "{saved}");
        assert!(saved.contains(r#""github-auto-backup": true"#), "{saved}");
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
