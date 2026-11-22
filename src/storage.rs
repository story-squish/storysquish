use std::fs;
use std::path::{Path, PathBuf};

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
