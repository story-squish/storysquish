use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::APP_NAME;
use crate::model::{Board, DEFAULT_TITLE};

const BOARD_FILE_NAME: &str = "board.json";
const DOCUMENTS_FOLDER_NAME: &str = "StorySquish";

pub fn default_board_path() -> Result<PathBuf> {
    default_board_path_in(&data_dir(APP_NAME)?, documents_boards_dir().as_deref())
}

/// `StorySquish` in the user's Documents folder, where boards live unless
/// the writer picks another place.
pub fn documents_boards_dir() -> Option<PathBuf> {
    let dirs = directories::UserDirs::new()?;
    Some(dirs.document_dir()?.join(DOCUMENTS_FOLDER_NAME))
}

/// A board still in the data dir, because moving it to Documents failed,
/// stays the default so it never looks lost.
fn default_board_path_in(data_dir: &Path, boards_dir: Option<&Path>) -> Result<PathBuf> {
    let legacy = data_dir.join(BOARD_FILE_NAME);
    if legacy.try_exists()? {
        return Ok(legacy);
    }
    Ok(boards_dir.map_or(legacy, |dir| dir.join(BOARD_FILE_NAME)))
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
    let mut file = fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("writing {}", tmp.display()))?;
    file.sync_all()
        .with_context(|| format!("syncing {}", tmp.display()))?;
    drop(file);
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

/// Sibling of the board file, so a board kept in a synced folder has its
/// backups synced too.
pub fn backups_dir(board_path: &Path) -> PathBuf {
    let stem = board_stem(board_path);
    board_path.with_file_name(format!("{stem}-backups"))
}

/// Copies the board file into its backups directory under a timestamped
/// name and prunes all but the newest `keep` copies with each tag, so routine
/// backups never push out a conflict copy. Returns `None` when there is no
/// board file to copy yet.
pub fn back_up(board_path: &Path, tag: &str, keep: usize) -> Result<Option<PathBuf>> {
    if !board_path.try_exists()? {
        return Ok(None);
    }
    let dir = backups_dir(board_path);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let backup = dir.join(backup_file_name(board_path, SystemTime::now(), tag));
    fs::copy(board_path, &backup)
        .with_context(|| format!("copying {} to {}", board_path.display(), backup.display()))?;
    prune_backups(&dir, &board_stem(board_path), keep)?;
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

pub struct LocalBackup {
    pub path: PathBuf,
    pub saved_at: Option<SystemTime>,
    /// Empty for a routine backup; `conflict` for a copy kept because the
    /// file was changed elsewhere while there were unsaved edits.
    pub tag: String,
    /// `None` when the file is not a readable board.
    pub contents: Option<BackupContents>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BackupContents {
    pub title: String,
    pub scene_count: usize,
}

/// Newest first. A board that has never been backed up has no backups
/// directory, which is not an error.
pub fn list_backups(board_path: &Path) -> Result<Vec<LocalBackup>> {
    let dir = backups_dir(board_path);
    if !dir.try_exists()? {
        return Ok(Vec::new());
    }
    let stem = board_stem(board_path);
    let mut backups: Vec<LocalBackup> = fs::read_dir(&dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| describe_backup(&stem, path))
        .collect();
    backups.sort_by(|a, b| {
        b.saved_at
            .cmp(&a.saved_at)
            .then_with(|| b.path.cmp(&a.path))
    });
    Ok(backups)
}

fn describe_backup(board_stem: &str, path: PathBuf) -> LocalBackup {
    let (saved_at, tag) = match parse_backup_name(board_stem, &path) {
        Some((at, tag)) => (Some(at), tag),
        None => (modified_time(&path), String::new()),
    };
    let contents = load(&path).ok().map(|board| BackupContents {
        scene_count: board.scenes.len(),
        title: board.title,
    });
    LocalBackup {
        path,
        saved_at,
        tag,
        contents,
    }
}

/// Inverse of `backup_file_name`.
fn parse_backup_name(board_stem: &str, path: &Path) -> Option<(SystemTime, String)> {
    let name = path.file_stem()?.to_str()?;
    let rest = name.strip_prefix(board_stem)?.strip_prefix('-')?;
    let (digits, suffix) = rest.split_at_checked(15)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let tag = match suffix {
        "" => "",
        suffix => suffix.strip_prefix('-')?,
    };
    let at = UNIX_EPOCH + Duration::from_millis(digits.parse().ok()?);
    Some((at, tag.to_owned()))
}

/// Files whose names `parse_backup_name` rejects were not written by
/// `back_up` and are left alone.
fn prune_backups(dir: &Path, board_stem: &str, keep: usize) -> Result<()> {
    let mut backups: Vec<(String, SystemTime, PathBuf)> = fs::read_dir(dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|path| {
            let (at, tag) = parse_backup_name(board_stem, &path)?;
            Some((tag, at, path))
        })
        .collect();
    backups.sort();
    for same_tag in backups.chunk_by(|a, b| a.0 == b.0) {
        let excess = same_tag.len().saturating_sub(keep);
        for (_, _, old) in &same_tag[..excess] {
            fs::remove_file(old).with_context(|| format!("removing {}", old.display()))?;
        }
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

pub fn export_file_name(board: &Board, suffix: Option<&str>, extension: &str) -> String {
    let title = board.title.trim();
    let title = if title.is_empty() { "novel" } else { title };
    let stem = title.split_whitespace().collect::<Vec<_>>().join("_");
    match suffix {
        Some(suffix) => format!("{stem}_{suffix}.{extension}"),
        None => format!("{stem}.{extension}"),
    }
}

/// Beside `board_path`, unless it is a backup opened from a backups
/// directory: new boards there would be mistaken for its backups.
pub fn new_board_dir(board_path: &Path) -> PathBuf {
    let mut dir = board_path.parent().unwrap_or(Path::new("."));
    while dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("-backups"))
        && let Some(parent) = dir.parent()
    {
        dir = parent;
    }
    dir.to_path_buf()
}

/// A file in `dir` named after the board's title that neither exists nor
/// has a backups directory left behind yet.
pub fn unused_board_path(dir: &Path, board: &Board) -> PathBuf {
    let stem = board_file_stem(&board.title);
    let mut number = 1;
    loop {
        let path = dir.join(format!("{}.json", numbered_stem(&stem, number)));
        if !path.exists() && !backups_dir(&path).exists() {
            return path;
        }
        number += 1;
    }
}

fn board_file_stem(title: &str) -> String {
    let board = Board {
        title: title.to_owned(),
        ..Board::default()
    };
    let name = export_file_name(&board, None, "json");
    let stem: String = name
        .trim_end_matches(".json")
        .trim_start_matches('.')
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    if stem.is_empty() {
        "novel".to_owned()
    } else {
        stem
    }
}

fn numbered_stem(stem: &str, number: usize) -> String {
    if number == 1 {
        stem.to_owned()
    } else {
        format!("{stem}_{number}")
    }
}

/// Compared without case, as the default macOS and Windows file systems
/// compare names.
fn is_numbered_stem(stem: &str, base: &str) -> bool {
    let (stem, base) = (stem.to_lowercase(), base.to_lowercase());
    match stem.strip_prefix(&base) {
        Some("") => true,
        Some(rest) => rest
            .strip_prefix('_')
            .and_then(|digits| digits.parse::<usize>().ok())
            .is_some_and(|number| number >= 2 && rest == format!("_{number}")),
        None => false,
    }
}

/// Where `board_path` moves so its name follows the board's new title, or
/// `None` when the name stays: the file is not saved yet, it already follows
/// the title, the title is a placeholder, or the name was chosen rather than derived from
/// `previous_title` or a placeholder, e.g. with Move.
pub fn title_following_path(
    board_path: &Path,
    previous_title: &str,
    board: &Board,
) -> Option<PathBuf> {
    if board.has_placeholder_title() || !board_path.is_file() {
        return None;
    }
    let stem = board_stem(board_path);
    if is_numbered_stem(&stem, &board_file_stem(&board.title)) {
        return None;
    }
    let derived = [previous_title, DEFAULT_TITLE, "", "board"]
        .iter()
        .any(|title| is_numbered_stem(&stem, &board_file_stem(title)));
    let dir = board_path.parent()?;
    derived.then(|| unused_board_path(dir, board))
}

/// Moves the board file and its backups, renaming each backup to the new
/// stem so it is still listed and pruned with the board.
pub fn rename_board(from: &Path, to: &Path) -> Result<()> {
    if to.try_exists()? {
        bail!("{} already exists", to.display());
    }
    fs::rename(from, to)
        .with_context(|| format!("renaming {} to {}", from.display(), to.display()))?;
    let (old_dir, new_dir) = (backups_dir(from), backups_dir(to));
    if !old_dir.try_exists()? {
        return Ok(());
    }
    fs::rename(&old_dir, &new_dir)
        .with_context(|| format!("renaming {} to {}", old_dir.display(), new_dir.display()))?;
    let (old_stem, new_stem) = (board_stem(from), board_stem(to));
    let entries =
        fs::read_dir(&new_dir).with_context(|| format!("listing {}", new_dir.display()))?;
    for backup in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if parse_backup_name(&old_stem, &backup).is_none() {
            continue;
        }
        let Some(rest) = backup
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(&old_stem))
        else {
            continue;
        };
        let renamed = new_dir.join(format!("{new_stem}{rest}"));
        fs::rename(&backup, &renamed).with_context(|| format!("renaming {}", backup.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_board_lives_in_documents() {
        let data = tempfile::tempdir().unwrap();
        let documents = tempfile::tempdir().unwrap();
        let path = default_board_path_in(data.path(), Some(documents.path())).unwrap();
        assert_eq!(path, documents.path().join("board.json"));
    }

    #[test]
    fn existing_board_in_data_dir_stays_the_default() {
        let data = tempfile::tempdir().unwrap();
        let documents = tempfile::tempdir().unwrap();
        let legacy = data.path().join("board.json");
        fs::write(&legacy, "{}").unwrap();
        let path = default_board_path_in(data.path(), Some(documents.path())).unwrap();
        assert_eq!(path, legacy);
    }

    #[test]
    fn default_board_falls_back_to_data_dir_without_documents() {
        let data = tempfile::tempdir().unwrap();
        let path = default_board_path_in(data.path(), None).unwrap();
        assert_eq!(path, data.path().join("board.json"));
    }

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

    fn backup_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn backups_keep_only_the_newest_copies_with_each_tag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("novel.json");
        let backups = backups_dir(&path);
        assert_eq!(backups, dir.path().join("novel-backups"));
        fs::create_dir_all(&backups).unwrap();
        let at = |millis| UNIX_EPOCH + Duration::from_millis(millis);
        for (millis, tag) in [(1, "conflict"), (2, ""), (3, "conflict"), (4, "")] {
            fs::write(
                backups.join(backup_file_name(&path, at(millis), tag)),
                "old",
            )
            .unwrap();
        }
        save(&path, &Board::default()).unwrap();

        let latest = back_up(&path, "", 2).unwrap().unwrap();

        assert_eq!(load(&latest).unwrap(), Board::default());
        let latest_name = latest.file_name().unwrap().to_str().unwrap().to_owned();
        assert_eq!(
            backup_names(&backups),
            [
                "novel-000000000000001-conflict.json".to_owned(),
                "novel-000000000000003-conflict.json".to_owned(),
                "novel-000000000000004.json".to_owned(),
                latest_name,
            ]
        );
    }

    #[test]
    fn pruning_leaves_files_it_did_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("novel.json");
        let backups = backups_dir(&path);
        fs::create_dir_all(&backups).unwrap();
        let foreign = [
            "a-notes.json",
            "novel-final.json",
            "other-000000000000001.json",
        ];
        for name in foreign {
            fs::write(backups.join(name), "{}").unwrap();
        }
        save(&path, &Board::default()).unwrap();

        back_up(&path, "", 1).unwrap();
        back_up(&path, "", 1).unwrap();

        let names = backup_names(&backups);
        assert_eq!(names.len(), foreign.len() + 1);
        assert!(foreign.iter().all(|f| names.iter().any(|n| n == f)));
    }

    #[test]
    fn board_without_backups_lists_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            list_backups(&dir.path().join("novel.json"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn backups_list_newest_first_with_tags_and_unreadable_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my-novel.json");
        let backups = backups_dir(&path);
        let at = |millis| UNIX_EPOCH + Duration::from_millis(millis);
        let mut board = Board {
            title: "Draft".into(),
            ..Board::default()
        };
        board.add_scene();
        save(&backups.join(backup_file_name(&path, at(1), "")), &board).unwrap();
        board.title = "Revised".into();
        board.add_scene();
        save(
            &backups.join(backup_file_name(&path, at(2), "conflict")),
            &board,
        )
        .unwrap();
        fs::write(
            backups.join(backup_file_name(&path, at(3), "")),
            "{not json",
        )
        .unwrap();
        fs::write(backups.join("notes.txt"), "ignored").unwrap();

        let listed = list_backups(&path).unwrap();

        let summary: Vec<_> = listed
            .iter()
            .map(|b| {
                let contents = b.contents.as_ref();
                (
                    b.saved_at,
                    b.tag.as_str(),
                    contents.map(|c| (c.title.as_str(), c.scene_count)),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (Some(at(3)), "", None),
                (Some(at(2)), "conflict", Some(("Revised", 2))),
                (Some(at(1)), "", Some(("Draft", 1))),
            ]
        );
    }

    #[test]
    fn backup_names_parse_back_to_time_and_tag() {
        let path = Path::new("novel.json");
        let at = UNIX_EPOCH + Duration::from_millis(42);
        assert_eq!(
            parse_backup_name(
                "novel",
                &path.with_file_name(backup_file_name(path, at, "x"))
            ),
            Some((at, "x".to_owned()))
        );
        assert_eq!(
            parse_backup_name("novel", Path::new("other-000000000000042.json")),
            None
        );
        assert_eq!(
            parse_backup_name("novel", Path::new("novel-00000000000004x.json")),
            None
        );
        assert_eq!(
            parse_backup_name("novel", Path::new("novel-000000000000042x.json")),
            None
        );
    }

    #[test]
    fn export_name_collapses_whitespace() {
        let mut board = Board {
            title: "  The   Long Night ".into(),
            ..Board::default()
        };
        assert_eq!(
            export_file_name(&board, None, "json"),
            "The_Long_Night.json"
        );
        assert_eq!(
            export_file_name(&board, Some("outline"), "md"),
            "The_Long_Night_outline.md"
        );
        board.title.clear();
        assert_eq!(export_file_name(&board, None, "md"), "novel.md");
        assert_eq!(
            export_file_name(&board, Some("manuscript"), "md"),
            "novel_manuscript.md"
        );
    }

    #[test]
    fn numbered_stems_match_their_base_without_case() {
        assert!(is_numbered_stem("Dune", "Dune"));
        assert!(is_numbered_stem("dune_3", "Dune"));
        assert!(!is_numbered_stem("Dune_1", "Dune"));
        assert!(!is_numbered_stem("Dune_03", "Dune"));
        assert!(!is_numbered_stem("Dune_Messiah", "Dune"));
        assert!(!is_numbered_stem("Dun", "Dune"));
    }

    #[test]
    fn only_names_derived_from_a_title_follow_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let dune = Board {
            title: "Dune".into(),
            ..Board::default()
        };
        let path_for = |name: &str| {
            let path = dir.path().join(name);
            save(&path, &dune).unwrap();
            path
        };
        let renamed = Some(dir.path().join("Dune.json"));
        assert_eq!(
            title_following_path(&path_for("Untitled_Novel_4.json"), "Dune", &dune),
            renamed
        );
        assert_eq!(
            title_following_path(&path_for("board.json"), "Dune", &dune),
            renamed
        );
        assert_eq!(
            title_following_path(&path_for("Arrakis_2.json"), "Arrakis", &dune),
            renamed
        );
        assert_eq!(
            title_following_path(&path_for("draft.json"), "Arrakis", &dune),
            None
        );
        assert_eq!(
            title_following_path(&path_for("dune_2.json"), "Arrakis", &dune),
            None
        );
        assert_eq!(
            title_following_path(&dir.path().join("Untitled_Novel_9.json"), "", &dune),
            None
        );
        let untitled = Board::default();
        assert_eq!(
            title_following_path(&path_for("board.json"), "Dune", &untitled),
            None
        );
    }

    #[test]
    fn renaming_a_board_moves_its_backups_under_the_new_name() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("Untitled_Novel.json");
        save(&from, &Board::default()).unwrap();
        back_up(&from, "", 5).unwrap();
        back_up(&from, "conflict", 5).unwrap();
        fs::write(backups_dir(&from).join("notes.txt"), "mine").unwrap();
        let to = dir.path().join("Dune.json");

        rename_board(&from, &to).unwrap();

        assert!(!from.exists() && !backups_dir(&from).exists());
        assert!(to.is_file());
        let mut tags: Vec<String> = list_backups(&to)
            .unwrap()
            .into_iter()
            .filter(|backup| backup.saved_at.is_some())
            .map(|backup| backup.tag)
            .collect();
        tags.sort();
        assert_eq!(tags, ["", "conflict"]);
        assert!(backups_dir(&to).join("notes.txt").is_file());
        assert!(rename_board(&to, &to).is_err());
    }

    #[test]
    fn new_boards_go_beside_the_board_outside_backups_directories() {
        let root = Path::new("/novels");
        assert_eq!(new_board_dir(&root.join("Dune.json")), root);
        assert_eq!(
            new_board_dir(&root.join("board-backups/board-001.json")),
            root
        );
        assert_eq!(
            new_board_dir(&root.join("board-backups/x-backups/x-002.json")),
            root
        );
        assert_eq!(new_board_dir(Path::new("Dune.json")), Path::new(""));
    }

    #[test]
    fn unused_board_path_skips_names_with_leftover_backups() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("Untitled_Novel-backups")).unwrap();
        assert_eq!(
            unused_board_path(dir.path(), &Board::default()),
            dir.path().join("Untitled_Novel_2.json")
        );
    }

    #[test]
    fn unused_board_path_avoids_existing_files_and_separators() {
        let dir = tempfile::tempdir().unwrap();
        let board = Board {
            title: "Dune / Messiah".into(),
            ..Board::default()
        };
        let first = unused_board_path(dir.path(), &board);
        assert_eq!(first, dir.path().join("Dune___Messiah.json"));
        fs::write(&first, "{}").unwrap();
        assert_eq!(
            unused_board_path(dir.path(), &board),
            dir.path().join("Dune___Messiah_2.json")
        );
        let hidden = Board {
            title: "..".into(),
            ..Board::default()
        };
        assert_eq!(
            unused_board_path(dir.path(), &hidden),
            dir.path().join("novel.json")
        );
    }
}
