use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use storysquish::{APP_NAME, app::PlannerApp, migration, model::Board, settings, storage};

fn main() -> Result<()> {
    let migration_alert = migration::migrate_legacy_dirs()
        .err()
        .map(|err| format!("Could not carry over data from before the rename: {err:#}"));
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .map_or_else(storage::default_board_path, Ok)?;
    let settings_path = settings::default_settings_path()?;
    let (board, board_alert) = load_board(&path);
    let startup_alert = board_alert.or(migration_alert);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("StorySquish")
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            Ok(Box::new(PlannerApp::new(
                cc,
                board,
                path,
                settings_path,
                startup_alert,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
    .context("running the UI")
}

/// An unreadable board is moved aside rather than silently overwritten by the
/// next autosave.
fn load_board(path: &Path) -> (Board, Option<String>) {
    match storage::load_or_default(path) {
        Ok(board) => (board, None),
        Err(err) => {
            let backup = path.with_extension("json.corrupt");
            let note = match std::fs::rename(path, &backup) {
                Ok(()) => format!("It was moved to {}.", backup.display()),
                Err(e) => format!("Moving it aside also failed: {e}."),
            };
            (
                Board::default(),
                Some(format!(
                    "Could not open your board: {err:#}\n\n{note} Starting with an empty board."
                )),
            )
        }
    }
}
