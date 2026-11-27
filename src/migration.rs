use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{APP_NAME, settings, storage};

/// Newest first, so a user who ran every release keeps their most recent data.
const LEGACY_APP_NAMES: [&str; 2] = ["story-planner", "novel-planner"];

/// Carries config and data saved before the app was renamed over to the
/// current directories, leaving any directory that already exists untouched.
pub fn migrate_legacy_dirs() -> Result<()> {
    migrate_dirs_named_by(settings::config_dir)?;
    migrate_dirs_named_by(storage::data_dir)
}

/// Moves boards out of the data dir into Documents and repoints settings
/// at them. The settings step runs on every
/// launch until it finds nothing to change, so a crash between the move and
/// the settings save heals itself.
pub fn migrate_boards_to_documents(settings_path: &Path) -> Result<()> {
    let Some(boards_dir) = storage::documents_boards_dir() else {
        return Ok(());
    };
    move_boards_and_repoint_settings(&storage::data_dir(APP_NAME)?, &boards_dir, settings_path)
}

fn move_boards_and_repoint_settings(
    data_dir: &Path,
    boards_dir: &Path,
    settings_path: &Path,
) -> Result<()> {
    move_dir_unless_target_exists(data_dir, boards_dir)?;
    if data_dir.try_exists()? {
        return Ok(());
    }
    // Unreadable settings are reported by the app itself; rewriting them
    // here would replace the writer's file with defaults.
    let Ok(mut settings) = settings::load(settings_path) else {
        return Ok(());
    };
    if settings.relocate_boards(data_dir, boards_dir) {
        settings::save(settings_path, &settings)?;
    }
    Ok(())
}

fn migrate_dirs_named_by(dir_for_app: impl Fn(&str) -> Result<PathBuf>) -> Result<()> {
    let current = dir_for_app(APP_NAME)?;
    for legacy_name in LEGACY_APP_NAMES {
        move_dir_unless_target_exists(&dir_for_app(legacy_name)?, &current)?;
    }
    Ok(())
}

fn move_dir_unless_target_exists(legacy: &Path, current: &Path) -> Result<()> {
    if current.try_exists()? || !legacy.try_exists()? {
        return Ok(());
    }
    if let Some(parent) = current.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::rename(legacy, current)
        .with_context(|| format!("moving {} to {}", legacy.display(), current.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_marker(dir: &Path, contents: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("board.json"), contents).unwrap();
    }

    #[test]
    fn moves_boards_to_documents_and_repoints_settings() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data/storysquish");
        let documents = root.path().join("Documents/StorySquish");
        let settings_path = root.path().join("config/settings.json");
        write_marker(&data, "mine");
        fs::create_dir_all(data.join("board-backups")).unwrap();
        let mut saved = settings::Settings::default();
        saved.set_repo(&data.join("board.json"), Some("me/novel".into()));
        settings::save(&settings_path, &saved).unwrap();

        move_boards_and_repoint_settings(&data, &documents, &settings_path).unwrap();

        assert!(!data.exists());
        assert_eq!(
            fs::read_to_string(documents.join("board.json")).unwrap(),
            "mine"
        );
        assert!(documents.join("board-backups").is_dir());
        let settings = settings::load(&settings_path).unwrap();
        assert_eq!(
            settings.repo(&documents.join("board.json")),
            Some("me/novel")
        );
        assert_eq!(settings.repo(&data.join("board.json")), None);
    }

    #[test]
    fn repoints_settings_left_behind_by_an_interrupted_move() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data/storysquish");
        let documents = root.path().join("Documents/StorySquish");
        let settings_path = root.path().join("settings.json");
        write_marker(&documents, "mine");
        let mut saved = settings::Settings::default();
        saved.board_path = Some(data.join("board.json"));
        settings::save(&settings_path, &saved).unwrap();

        move_boards_and_repoint_settings(&data, &documents, &settings_path).unwrap();

        let settings = settings::load(&settings_path).unwrap();
        assert_eq!(settings.board_path, Some(documents.join("board.json")));
    }

    #[test]
    fn keeps_boards_in_data_dir_when_documents_folder_exists() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data/storysquish");
        let documents = root.path().join("Documents/StorySquish");
        let settings_path = root.path().join("settings.json");
        write_marker(&data, "old");
        write_marker(&documents, "other");
        let mut saved = settings::Settings::default();
        saved.board_path = Some(data.join("board.json"));
        settings::save(&settings_path, &saved).unwrap();

        move_boards_and_repoint_settings(&data, &documents, &settings_path).unwrap();

        assert_eq!(fs::read_to_string(data.join("board.json")).unwrap(), "old");
        let settings = settings::load(&settings_path).unwrap();
        assert_eq!(settings.board_path, Some(data.join("board.json")));
    }

    #[test]
    fn moves_legacy_dir_when_current_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("novel-planner");
        let current = root.path().join("nested/storysquish");
        write_marker(&legacy, "old");

        move_dir_unless_target_exists(&legacy, &current).unwrap();

        assert!(!legacy.exists());
        assert_eq!(
            fs::read_to_string(current.join("board.json")).unwrap(),
            "old"
        );
    }

    #[test]
    fn leaves_both_dirs_alone_when_current_exists() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("novel-planner");
        let current = root.path().join("storysquish");
        write_marker(&legacy, "old");
        write_marker(&current, "new");

        move_dir_unless_target_exists(&legacy, &current).unwrap();

        assert_eq!(
            fs::read_to_string(legacy.join("board.json")).unwrap(),
            "old"
        );
        assert_eq!(
            fs::read_to_string(current.join("board.json")).unwrap(),
            "new"
        );
    }

    #[test]
    fn does_nothing_without_legacy_dir() {
        let root = tempfile::tempdir().unwrap();
        let current = root.path().join("storysquish");

        move_dir_unless_target_exists(&root.path().join("novel-planner"), &current).unwrap();

        assert!(!current.exists());
    }

    #[test]
    fn prefers_newest_legacy_dir_and_keeps_older_ones() {
        let root = tempfile::tempdir().unwrap();
        let dir_for_app = |name: &str| Ok(root.path().join(name));
        write_marker(&root.path().join("novel-planner"), "oldest");
        write_marker(&root.path().join("story-planner"), "newer");

        migrate_dirs_named_by(dir_for_app).unwrap();

        assert_eq!(
            fs::read_to_string(root.path().join(APP_NAME).join("board.json")).unwrap(),
            "newer"
        );
        assert!(!root.path().join("story-planner").exists());
        assert!(root.path().join("novel-planner").exists());
    }

    #[test]
    fn migrates_oldest_legacy_dir_when_it_is_the_only_one() {
        let root = tempfile::tempdir().unwrap();
        let dir_for_app = |name: &str| Ok(root.path().join(name));
        write_marker(&root.path().join("novel-planner"), "oldest");

        migrate_dirs_named_by(dir_for_app).unwrap();

        assert_eq!(
            fs::read_to_string(root.path().join(APP_NAME).join("board.json")).unwrap(),
            "oldest"
        );
    }
}
