use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::APP_NAME;
use crate::model::Board;

pub fn default_board_path() -> Result<PathBuf> {
    Ok(data_dir(APP_NAME)?.join("board.json"))
}

pub fn data_dir(app_name: &str) -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", app_name)
        .context("could not determine a data directory for this user")?;
    Ok(dirs.data_dir().to_path_buf())
}

/// A missing file is a fresh board, not an error.
pub fn load_or_default(path: &Path) -> Result<Board> {
    if !path.try_exists()? {
        return Ok(Board::default());
    }
    load(path)
}

pub fn load(path: &Path) -> Result<Board> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("{} is not a valid board file", path.display()))
}

pub fn save(path: &Path, board: &Board) -> Result<()> {
    write_atomically(path, &serde_json::to_string_pretty(board)?)
}

/// Writes via a sibling temp file and rename so a crash never leaves a
/// half-written file behind.
pub fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

/// Sibling of the board file, so a board kept in a synced folder has its
/// backups synced too.
pub fn backups_dir(board_path: &Path) -> PathBuf {
    let stem = board_stem(board_path);
    board_path.with_file_name(format!("{stem}-backups"))
}

/// Copies the board file into its backups directory under a timestamped
/// name and prunes all but the newest `keep` copies. Returns `None` when
/// there is no board file to copy yet.
pub fn back_up(board_path: &Path, tag: &str, keep: usize) -> Result<Option<PathBuf>> {
    if !board_path.try_exists()? {
        return Ok(None);
    }
    let dir = backups_dir(board_path);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let backup = dir.join(backup_file_name(board_path, SystemTime::now(), tag));
    fs::copy(board_path, &backup)
        .with_context(|| format!("copying {} to {}", board_path.display(), backup.display()))?;
    prune_backups(&dir, keep)?;
    Ok(Some(backup))
}

fn backup_file_name(board_path: &Path, at: SystemTime, tag: &str) -> String {
    let millis = at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let stem = board_stem(board_path);
    let suffix = if tag.is_empty() {
        String::new()
    } else {
        format!("-{tag}")
    };
    // Zero-padded so lexical order is chronological order when pruning.
    format!("{stem}-{millis:015}{suffix}.json")
}

fn prune_backups(dir: &Path, keep: usize) -> Result<()> {
    let mut backups: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    backups.sort();
    let excess = backups.len().saturating_sub(keep);
    for old in &backups[..excess] {
        fs::remove_file(old).with_context(|| format!("removing {}", old.display()))?;
    }
    Ok(())
}

fn board_stem(board_path: &Path) -> String {
    board_path
        .file_stem()
        .map_or_else(|| "board".to_owned(), |s| s.to_string_lossy().into_owned())
}

/// `None` when the file is missing or the platform has no mtime.
pub fn modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

pub fn export_file_name(board: &Board, extension: &str) -> String {
    let title = board.title.trim();
    let stem = if title.is_empty() { "novel" } else { title };
    format!(
        "{}.{extension}",
        stem.split_whitespace().collect::<Vec<_>>().join("_")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_default_board() {
        let dir = tempfile::tempdir().unwrap();
        let board = load_or_default(&dir.path().join("nope.json")).unwrap();
        assert_eq!(board, Board::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/board.json");
        let mut board = Board::default();
        board.add_scene();
        save(&path, &board).unwrap();
        assert_eq!(load(&path).unwrap(), board);
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.json");
        fs::write(&path, "{not json").unwrap();
        assert!(load_or_default(&path).is_err());
    }

    #[test]
    fn importing_missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("nope.json")).is_err());
    }

    #[test]
    fn backup_of_missing_board_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            back_up(&dir.path().join("board.json"), "", 3).unwrap(),
            None
        );
        assert!(!dir.path().join("board-backups").exists());
    }

    #[test]
    fn backups_keep_only_the_newest_copies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("novel.json");
        let backups = backups_dir(&path);
        assert_eq!(backups, dir.path().join("novel-backups"));
        fs::create_dir_all(&backups).unwrap();
        for millis in [1_u64, 2, 3] {
            let at = UNIX_EPOCH + std::time::Duration::from_millis(millis);
            fs::write(backups.join(backup_file_name(&path, at, "")), "old").unwrap();
        }
        save(&path, &Board::default()).unwrap();

        let latest = back_up(&path, "conflict", 2).unwrap().unwrap();

        assert!(latest.to_string_lossy().ends_with("-conflict.json"));
        assert_eq!(load(&latest).unwrap(), Board::default());
        let mut left: Vec<_> = fs::read_dir(&backups)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left.len(), 2);
        assert_eq!(left[0], "novel-000000000000003.json");
    }

    #[test]
    fn export_name_collapses_whitespace() {
        let mut board = Board {
            title: "  The   Long Night ".into(),
            ..Board::default()
        };
        assert_eq!(export_file_name(&board, "json"), "The_Long_Night.json");
        board.title.clear();
        assert_eq!(export_file_name(&board, "md"), "novel.md");
    }
}
