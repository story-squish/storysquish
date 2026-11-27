use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::theme::ColorScheme;
use crate::{APP_NAME, storage};

const RECENT_BOARDS_LIMIT: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    pub color_scheme: ColorScheme,
    /// Where the board lives when no path is given on the command line,
    /// e.g. a folder synced by iCloud Drive, Dropbox or Syncthing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_path: Option<PathBuf>,
    /// Boards switched away from, most recent first, offered by Open → Local.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    recent_boards: Vec<PathBuf>,
    /// Read from settings written before `recent_boards`, then folded into it.
    #[serde(rename = "previous-board-path", skip_serializing)]
    legacy_previous_board: Option<PathBuf>,
    /// Private GitHub repository (`owner/name`) per board file, so each
    /// novel keeps its own history.
    #[serde(rename = "github-repos", skip_serializing_if = "BTreeMap::is_empty")]
    repos: BTreeMap<PathBuf, String>,
    /// Boards with edits that have not reached their repo yet, e.g. made
    /// offline, so the next launch backs them up and does not let the older
    /// cloud copy replace them.
    #[serde(rename = "github-unpushed", skip_serializing_if = "BTreeSet::is_empty")]
    unpushed: BTreeSet<PathBuf>,
    #[serde(alias = "gist-auto-backup")]
    pub github_auto_backup: bool,
    /// Read from settings written before the goal moved into the board,
    /// then handed to the first board opened.
    #[serde(rename = "word-goal", skip_serializing)]
    legacy_word_goal: Option<usize>,
    pub spellcheck_dictionary: SpellcheckDictionary,
    /// Added through "Add to dictionary", checked with every dictionary.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub spelling_words: BTreeSet<String>,
    /// Offers synonyms on right-click, in the spellcheck dictionary's language.
    pub thesaurus: bool,
}

/// Saved as `null` (off), `"auto"` (follow the keyboard layout) or a
/// dictionary id.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "Option<String>", into = "Option<String>")]
pub enum SpellcheckDictionary {
    Off,
    #[default]
    Auto,
    Fixed(String),
}

const AUTO_DICTIONARY: &str = "auto";

impl From<Option<String>> for SpellcheckDictionary {
    fn from(saved: Option<String>) -> Self {
        match saved {
            None => Self::Off,
            Some(id) if id == AUTO_DICTIONARY => Self::Auto,
            Some(id) => Self::Fixed(id),
        }
    }
}

impl From<SpellcheckDictionary> for Option<String> {
    fn from(dictionary: SpellcheckDictionary) -> Self {
        match dictionary {
            SpellcheckDictionary::Off => None,
            SpellcheckDictionary::Auto => Some(AUTO_DICTIONARY.to_owned()),
            SpellcheckDictionary::Fixed(id) => Some(id),
        }
    }
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

    pub fn linked_boards(&self) -> impl Iterator<Item = &Path> {
        self.repos.keys().map(PathBuf::as_path)
    }

    pub fn recent_boards(&self) -> impl Iterator<Item = &Path> {
        self.recent_boards.iter().map(PathBuf::as_path)
    }

    pub fn remember_recent_board(&mut self, board: &Path) {
        let key = board_key(board);
        self.recent_boards.retain(|recent| *recent != key);
        self.recent_boards.insert(0, key);
        self.recent_boards.truncate(RECENT_BOARDS_LIMIT);
    }

    pub fn take_legacy_word_goal(&mut self) -> Option<usize> {
        self.legacy_word_goal.take()
    }

    fn adopt_legacy_previous_board(&mut self) {
        if let Some(previous) = self.legacy_previous_board.take()
            && self.recent_boards.is_empty()
        {
            self.recent_boards.push(board_key(&previous));
        }
    }

    /// Carries a board's repo link, unpushed mark and recent entry over to
    /// its new path after a move or rename.
    pub fn move_board(&mut self, from: &Path, to: &Path) {
        let (from, to) = (board_key(from), board_key(to));
        let repo = self.repos.remove(&from);
        self.set_repo(&to, repo);
        if self.unpushed.remove(&from) {
            self.unpushed.insert(to.clone());
        }
        for recent in &mut self.recent_boards {
            if *recent == from {
                recent.clone_from(&to);
            }
        }
    }

    /// Points every remembered board inside `from` at the same place inside
    /// `to`. Returns whether this changed anything, so callers save only then.
    pub fn relocate_boards(&mut self, from: &Path, to: &Path) -> bool {
        let (from, to) = (board_key(from), board_key(to));
        let relocated = |board: &Path| Some(to.join(board.strip_prefix(&from).ok()?));
        let mut changed = false;
        let mut relocate = |board: &mut PathBuf| {
            if let Some(moved) = relocated(board) {
                *board = moved;
                changed = true;
            }
        };
        if let Some(board) = &mut self.board_path {
            relocate(board);
        }
        self.recent_boards.iter_mut().for_each(&mut relocate);
        self.repos = std::mem::take(&mut self.repos)
            .into_iter()
            .map(|(mut board, repo)| {
                relocate(&mut board);
                (board, repo)
            })
            .collect();
        self.unpushed = std::mem::take(&mut self.unpushed)
            .into_iter()
            .map(|mut board| {
                relocate(&mut board);
                board
            })
            .collect();
        changed
    }

    pub fn is_unpushed(&self, board: &Path) -> bool {
        self.unpushed.contains(&board_key(board))
    }

    /// Returns whether this changed anything, so callers save only then.
    pub fn set_unpushed(&mut self, board: &Path, unpushed: bool) -> bool {
        let key = board_key(board);
        if unpushed {
            self.unpushed.insert(key)
        } else {
            self.unpushed.remove(&key)
        }
    }

    pub fn unpushed_boards(&self) -> impl Iterator<Item = &Path> {
        self.unpushed.iter().map(PathBuf::as_path)
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
    let mut settings: Settings = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a valid settings file", path.display()))?;
    settings.adopt_legacy_previous_board();
    Ok(settings)
}

pub fn save(path: &Path, settings: &Settings) -> Result<()> {
    storage::write_atomically(path, &serde_json::to_string_pretty(settings)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocate_boards_moves_every_path_inside_the_old_dir() {
        let old = Path::new("/data/storysquish");
        let new = Path::new("/docs/StorySquish");
        let outside = Path::new("/dropbox/novel.json");
        let mut settings = Settings {
            board_path: Some(old.join("board.json")),
            ..Settings::default()
        };
        settings.remember_recent_board(&old.join("sequel.json"));
        settings.remember_recent_board(outside);
        settings.set_repo(&old.join("board.json"), Some("me/novel".into()));
        settings.set_repo(outside, Some("me/other".into()));
        settings.set_unpushed(&old.join("sequel.json"), true);

        assert!(settings.relocate_boards(old, new));

        assert_eq!(settings.board_path, Some(new.join("board.json")));
        assert_eq!(
            settings.recent_boards().collect::<Vec<_>>(),
            [outside, &new.join("sequel.json")]
        );
        assert_eq!(settings.repo(&new.join("board.json")), Some("me/novel"));
        assert_eq!(settings.repo(outside), Some("me/other"));
        assert!(settings.is_unpushed(&new.join("sequel.json")));
        assert!(!settings.relocate_boards(old, new));
    }

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
            github_auto_backup: true,
            ..Settings::default()
        };
        settings.remember_recent_board(&dir.path().join("emma.json"));
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

        settings.move_board(Path::new("/novels/dune.json"), Path::new("/sync/dune.json"));
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
    fn unpushed_boards_persist_and_follow_a_move() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let board = dir.path().join("dune.json");
        let moved = dir.path().join("sync/dune.json");
        let mut settings = Settings::default();
        assert!(settings.set_unpushed(&board, true));
        assert!(!settings.set_unpushed(&board, true));
        save(&path, &settings).unwrap();
        let mut settings = load(&path).unwrap();
        assert!(settings.is_unpushed(&board));

        settings.move_board(&board, &moved);
        assert!(!settings.is_unpushed(&board));
        assert_eq!(settings.unpushed_boards().collect::<Vec<_>>(), [&moved]);
        assert!(settings.set_unpushed(&moved, false));
        save(&path, &settings).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("unpushed"));
    }

    #[test]
    fn recent_boards_are_newest_first_unique_and_capped() {
        let mut settings = Settings::default();
        let board = |n: usize| PathBuf::from(format!("/novels/{n}.json"));
        for n in 0..=RECENT_BOARDS_LIMIT {
            settings.remember_recent_board(&board(n));
        }
        settings.remember_recent_board(&board(3));

        let recent: Vec<_> = settings.recent_boards().map(Path::to_owned).collect();
        assert_eq!(recent.len(), RECENT_BOARDS_LIMIT);
        assert_eq!(recent[0], board(3));
        assert_eq!(recent[1], board(RECENT_BOARDS_LIMIT));
        assert!(!recent.contains(&board(0)));
        assert_eq!(recent.iter().filter(|path| **path == board(3)).count(), 1);
    }

    #[test]
    fn recent_boards_follow_a_move() {
        let mut settings = Settings::default();
        settings.remember_recent_board(Path::new("/novels/dune.json"));
        settings.move_board(Path::new("/novels/dune.json"), Path::new("/sync/dune.json"));
        assert_eq!(
            settings.recent_boards().collect::<Vec<_>>(),
            [Path::new("/sync/dune.json")]
        );
    }

    #[test]
    fn previous_board_path_becomes_the_first_recent_board() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let previous = dir.path().join("emma.json");
        let old_settings = serde_json::json!({ "previous-board-path": previous });
        fs::write(&path, old_settings.to_string()).unwrap();

        let settings = load(&path).unwrap();
        assert_eq!(settings.recent_boards().collect::<Vec<_>>(), [&previous]);
        save(&path, &settings).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("previous-board-path"), "{saved}");
        assert!(saved.contains("recent-boards"), "{saved}");
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
    fn spellcheck_defaults_to_auto_and_every_choice_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(
            Settings::default().spellcheck_dictionary,
            SpellcheckDictionary::Auto
        );
        let cases = [
            (
                SpellcheckDictionary::Off,
                r#""spellcheck-dictionary": null"#,
            ),
            (
                SpellcheckDictionary::Auto,
                r#""spellcheck-dictionary": "auto""#,
            ),
            (
                SpellcheckDictionary::Fixed("en_GB".to_owned()),
                r#""spellcheck-dictionary": "en_GB""#,
            ),
        ];
        for (dictionary, json) in cases {
            let settings = Settings {
                spellcheck_dictionary: dictionary,
                spelling_words: BTreeSet::from(["grimdark".to_owned()]),
                ..Settings::default()
            };
            save(&path, &settings).unwrap();
            let saved = fs::read_to_string(&path).unwrap();
            assert!(saved.contains(json), "{saved}");
            assert_eq!(load(&path).unwrap(), settings);
        }
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

    #[test]
    fn legacy_word_goal_is_read_once_and_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"word-goal": 80000}"#).unwrap();

        let mut settings = load(&path).unwrap();
        assert_eq!(settings.take_legacy_word_goal(), Some(80_000));
        assert_eq!(settings.take_legacy_word_goal(), None);

        let mut unclaimed = load(&path).unwrap();
        save(&path, &unclaimed).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("word-goal"));
        unclaimed = load(&path).unwrap();
        assert_eq!(unclaimed.take_legacy_word_goal(), None);
    }
}
