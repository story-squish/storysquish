use std::collections::HashSet;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{
    self, Align, Color32, ComboBox, CornerRadius, Frame, Grid, Key, KeyboardShortcut, Label,
    Layout, Margin, Modifiers, RichText, Sense, Stroke, TextEdit, Ui, Vec2,
};

use crate::gist::{self, CloudBoard, Revision};
use crate::history::History;
use crate::model::{Act, Board, Character, Id, Location, Scene, Status};
use crate::settings::{self, Settings};
use crate::theme::{ColorScheme, Palette};
use crate::{outline, storage};

const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);
const DISK_POLL: Duration = Duration::from_secs(2);
const BACKUP_INTERVAL: Duration = Duration::from_mins(30);
const BACKUPS_KEPT: usize = 30;
const GIST_PUSH_DELAY: Duration = Duration::from_secs(5);
const GIST_POLL: Duration = Duration::from_millis(100);
const NODE_RADIUS: f32 = 26.0;
/// Short of the midpoint, so the labels of A→B and B→A land apart.
const EDGE_LABEL_POSITION: f32 = 0.4;
const CARD_WIDTH: f32 = 220.0;
const CARD_MARGIN: i8 = 12;
/// Content width plus both inner margins and the widest (selected) stroke.
const CARD_OUTER_WIDTH: f32 = CARD_WIDTH + 2.0 * CARD_MARGIN as f32 + 4.0;
const SUMMARY_PREVIEW_CHARS: usize = 180;
const UNDO_LIMIT: usize = 100;
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const NEW_SCENE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const DUPLICATE_SCENE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
const MOVE_SCENE_EARLIER: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::ArrowLeft);
const MOVE_SCENE_LATER: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::ArrowRight);
const MOVE_CHARACTER_UP: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::ArrowUp);
const MOVE_CHARACTER_DOWN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::ArrowDown);
const FIND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
const CLOSE_INSPECTOR: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);
const DELETE_SCENE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Delete);
const DELETE_SCENE_BACKSPACE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::NONE, Key::Backspace);

#[derive(Debug, Clone, PartialEq, Eq)]
enum SaveState {
    Saved,
    Pending(Instant),
    Imported,
    Reloaded,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Selection {
    Scene(Id),
    Character(Id),
    Location(Id),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    #[default]
    Board,
    Write,
    Timeline,
    Relationships,
}

impl View {
    const ALL: [Self; 4] = [
        Self::Board,
        Self::Write,
        Self::Timeline,
        Self::Relationships,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Board => "Board",
            Self::Write => "Write",
            Self::Timeline => "Timeline",
            Self::Relationships => "Relationships",
        }
    }
}

enum BoardFileAction {
    Open,
    Move,
    RecoverFromLocal,
}

#[derive(Clone, Copy)]
enum GistAction {
    Connect,
    Disconnect,
    SetGistId,
    BackUpNow,
    Restore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GistTask {
    ManualPush,
    AutoPush,
    ListRevisions,
    Fetch,
    ListCloudBoards,
    OpenCloudBoard,
}

#[derive(Default)]
struct GistState {
    token: Option<String>,
    token_draft: String,
    gist_id_draft: String,
    gist_id_invalid: bool,
    in_flight: Option<(GistTask, gist::Pending)>,
    board_changed: bool,
    /// Set by a title edit or a focus change, which push without waiting
    /// out `GIST_PUSH_DELAY`.
    push_now: bool,
    last_edit: Option<Instant>,
    last_success: Option<Instant>,
    last_pushed_json: Option<String>,
    /// The board file a push in flight belongs to, which can differ from the
    /// open one if another board was opened meanwhile.
    pushed_board: Option<PathBuf>,
    /// Pushes for boards switched away from before their edits reached
    /// the gist; they go ahead of cloud requests so a reopen fetches them.
    queued_pushes: Vec<(PathBuf, gist::Job)>,
    /// Boards edited here whose latest version is not yet in their gist,
    /// so the cloud copy must not overwrite them.
    unpushed_boards: HashSet<PathBuf>,
    api_base: Option<String>,
    error: Option<String>,
    restore: Option<GistRestore>,
    cloud: Option<CloudPicker>,
}

#[derive(Default)]
struct GistRestore {
    title: Option<String>,
    revisions: Option<Vec<Revision>>,
    error: Option<String>,
}

/// The request waits in `queued` while another one, such as an automatic
/// backup, still holds the single GitHub slot.
#[derive(Default)]
struct CloudPicker {
    boards: Option<Vec<CloudBoard>>,
    queued: Option<(GistTask, gist::Job)>,
    error: Option<String>,
}

struct LocalRecovery {
    backups: Vec<storage::LocalBackup>,
    selected: Option<usize>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct DraggedScene(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FocusSnapshot {
    widget: Option<egui::Id>,
    window: bool,
}

impl Default for FocusSnapshot {
    fn default() -> Self {
        Self {
            widget: None,
            window: true,
        }
    }
}

impl FocusSnapshot {
    fn of(ctx: &egui::Context) -> Self {
        Self {
            widget: ctx.memory(egui::Memory::focused),
            window: ctx.input(|i| i.focused),
        }
    }
}

pub struct PlannerApp {
    board: Board,
    history: History<Board>,
    path: PathBuf,
    selected: Option<Selection>,
    view: View,
    new_character: String,
    new_location: String,
    new_relationship_target: Option<Id>,
    new_relationship_label: String,
    disk_modified: Option<SystemTime>,
    last_disk_check: Instant,
    last_backup: Option<Instant>,
    save_state: SaveState,
    focus_seen: FocusSnapshot,
    scene_to_delete: Option<Id>,
    alert: Option<String>,
    focus_scene_title: bool,
    focus_search: bool,
    query: String,
    filter_status: Option<Status>,
    filter_act: Option<Act>,
    filter_pov: Option<Id>,
    show_stats: bool,
    window_title: String,
    settings: Settings,
    settings_path: PathBuf,
    settings_dialog: Option<SettingsDialog>,
    local_recovery: Option<LocalRecovery>,
    gist: GistState,
    colors: Palette,
}

#[derive(Debug, Default)]
struct SettingsDialog {
    save_error: Option<String>,
}

impl PlannerApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        board: Board,
        path: PathBuf,
        settings_path: PathBuf,
        startup_alert: Option<String>,
    ) -> Self {
        let (mut settings, settings_alert) = match settings::load(&settings_path) {
            Ok(settings) => (settings, None),
            Err(e) => (
                Settings::default(),
                Some(format!("Could not read settings, using defaults: {e:#}")),
            ),
        };
        let (token, token_alert) = match gist::load_token(&gist::token_path(&settings_path)) {
            Ok(token) => (token, None),
            Err(e) => (
                None,
                Some(format!("Could not read the GitHub token: {e:#}")),
            ),
        };
        let claimed_gist_id = settings.claim_unclaimed_gist_id(&path);
        let gist = GistState {
            token,
            gist_id_draft: settings.gist_id(&path).unwrap_or_default().to_owned(),
            ..GistState::default()
        };
        let colors = settings.color_scheme.palette();
        apply_theme(&cc.egui_ctx, colors);
        let mut app = Self {
            history: History::new(board.clone(), UNDO_LIMIT),
            board,
            disk_modified: storage::modified_time(&path),
            path,
            selected: None,
            view: View::default(),
            new_character: String::new(),
            new_location: String::new(),
            new_relationship_target: None,
            new_relationship_label: String::new(),
            last_disk_check: Instant::now(),
            last_backup: None,
            save_state: SaveState::Saved,
            focus_seen: FocusSnapshot::default(),
            scene_to_delete: None,
            alert: startup_alert.or(settings_alert).or(token_alert),
            focus_scene_title: false,
            focus_search: false,
            query: String::new(),
            filter_status: None,
            filter_act: None,
            filter_pov: None,
            show_stats: false,
            window_title: String::new(),
            settings,
            settings_path,
            settings_dialog: None,
            local_recovery: None,
            gist,
            colors,
        };
        if claimed_gist_id {
            app.save_settings_reporting_errors();
        }
        app
    }

    #[must_use]
    pub fn with_gist_api_base(mut self, api_base: &str) -> Self {
        self.gist.api_base = Some(api_base.to_owned());
        self
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn window_title(&self) -> &str {
        &self.window_title
    }

    pub fn color_scheme(&self) -> ColorScheme {
        self.settings.color_scheme
    }

    pub fn board_path(&self) -> &Path {
        &self.path
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    fn set_color_scheme(&mut self, ctx: &egui::Context, scheme: ColorScheme) {
        self.settings.color_scheme = scheme;
        self.colors = scheme.palette();
        apply_theme(ctx, self.colors);
        let save_error = settings::save(&self.settings_path, &self.settings)
            .err()
            .map(|e| format!("Could not save settings: {e:#}"));
        if let Some(dialog) = &mut self.settings_dialog {
            dialog.save_error = save_error;
        } else if let Some(error) = save_error {
            self.alert = Some(error);
        }
    }

    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let title = window_title(
            &self.board.title,
            matches!(self.save_state, SaveState::Failed(_)),
        );
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn mark_dirty(&mut self) {
        self.save_state = SaveState::Pending(Instant::now());
    }

    fn save_now(&mut self) {
        self.gist.board_changed = true;
        self.gist.last_edit = Some(match self.save_state {
            SaveState::Pending(edited_at) => edited_at,
            _ => Instant::now(),
        });
        self.history.commit(&self.board);
        if let Err(reason) = self.keep_copy_of_external_edit() {
            self.save_state = SaveState::Failed(reason);
            return;
        }
        self.back_up_if_due();
        self.save_state = match storage::save(&self.path, &self.board) {
            Ok(()) => {
                self.disk_modified = storage::modified_time(&self.path);
                SaveState::Saved
            }
            Err(e) => SaveState::Failed(format!("{e:#}")),
        };
    }

    fn changed_on_disk(&self) -> Option<SystemTime> {
        storage::modified_time(&self.path).filter(|&on_disk| Some(on_disk) != self.disk_modified)
    }

    /// Another device or program rewrote the board while there were unsaved
    /// edits here. Ours wins, but theirs is kept as a backup rather than
    /// silently overwritten; if that copy fails, the save is refused.
    fn keep_copy_of_external_edit(&mut self) -> Result<(), String> {
        if self.changed_on_disk().is_none() {
            return Ok(());
        }
        match storage::back_up(&self.path, "conflict", BACKUPS_KEPT) {
            Ok(copy) => {
                if let Some(copy) = copy {
                    self.alert = Some(format!(
                        "The board file was changed outside StorySquish, perhaps on another \
                         device, while you had unsaved edits here. Your version was kept; the \
                         other one was saved to {}.",
                        copy.display()
                    ));
                }
                Ok(())
            }
            Err(e) => Err(format!(
                "the board file was changed elsewhere and could not be backed up before \
                 overwriting it: {e:#}"
            )),
        }
    }

    /// Runs before the session's first write and then periodically, so the
    /// backups hold the board as it was before each stretch of editing.
    fn back_up_if_due(&mut self) {
        if self
            .last_backup
            .is_some_and(|at| at.elapsed() < BACKUP_INTERVAL)
        {
            return;
        }
        self.last_backup = Some(Instant::now());
        if let Err(e) = storage::back_up(&self.path, "", BACKUPS_KEPT) {
            self.alert = Some(format!("Could not back up the board: {e:#}"));
        }
    }

    /// Picks up edits synced in from another device. Skipped while edits
    /// are pending here; `save_now` handles that case as a conflict.
    fn reload_if_changed_elsewhere(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(DISK_POLL);
        if self.last_disk_check.elapsed() < DISK_POLL {
            return;
        }
        self.last_disk_check = Instant::now();
        if matches!(
            self.save_state,
            SaveState::Pending(_) | SaveState::Failed(_)
        ) {
            return;
        }
        let Some(on_disk) = self.changed_on_disk() else {
            return;
        };
        // A parse failure usually means a sync client is mid-write; the
        // next poll tries again.
        let Ok(board) = storage::load(&self.path) else {
            return;
        };
        self.disk_modified = Some(on_disk);
        if board != self.board {
            self.history.commit(&self.board);
            self.board = board;
            self.history.commit(&self.board);
            self.save_state = SaveState::Reloaded;
            self.gist.board_changed = true;
            self.gist.last_edit = Some(Instant::now());
        }
    }

    fn flush_if_due(&mut self, ctx: &egui::Context) {
        if let SaveState::Pending(since) = self.save_state {
            let elapsed = since.elapsed();
            if elapsed >= SAVE_DEBOUNCE {
                self.save_now();
            } else {
                ctx.request_repaint_after(SAVE_DEBOUNCE.saturating_sub(elapsed));
            }
        }
    }

    fn flush_on_focus_change(&mut self, ctx: &egui::Context) {
        let focus = FocusSnapshot::of(ctx);
        if focus != self.focus_seen {
            self.focus_seen = focus;
            self.flush_pending_edits();
            if self.gist.board_changed {
                self.gist.push_now = true;
                ctx.request_repaint();
            }
        }
    }

    fn undo(&mut self) {
        if let Some(board) = self.history.undo(&self.board) {
            self.board = board;
            self.save_now();
        }
    }

    fn redo(&mut self) {
        if let Some(board) = self.history.redo(&self.board) {
            self.board = board;
            self.save_now();
        }
    }

    fn dialog_open(&self) -> bool {
        self.alert.is_some()
            || self.scene_to_delete.is_some()
            || self.settings_dialog.is_some()
            || self.local_recovery.is_some()
            || self.gist.restore.is_some()
            || self.gist.cloud.is_some()
    }

    /// Undo, Escape and Delete mean something different inside a text field,
    /// so those only act on the board when no widget holds keyboard focus.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog_open() {
            return;
        }
        let pressed = |shortcut: &KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(shortcut));

        if pressed(&NEW_SCENE) {
            self.add_scene();
        }
        if pressed(&FIND) {
            self.focus_search = true;
        }
        if pressed(&DUPLICATE_SCENE) {
            self.duplicate_selected_scene();
        }

        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        if pressed(&REDO) {
            self.redo();
        } else if pressed(&UNDO) {
            self.undo();
        }
        if pressed(&MOVE_SCENE_EARLIER) || pressed(&MOVE_CHARACTER_UP) {
            self.shift_selection(-1);
        }
        if pressed(&MOVE_SCENE_LATER) || pressed(&MOVE_CHARACTER_DOWN) {
            self.shift_selection(1);
        }
        if pressed(&CLOSE_INSPECTOR) {
            self.selected = None;
        }
        if (pressed(&DELETE_SCENE) || pressed(&DELETE_SCENE_BACKSPACE))
            && let Some(Selection::Scene(id)) = self.selected.clone()
        {
            self.scene_to_delete = Some(id);
        }
    }

    fn add_scene(&mut self) {
        self.selected = Some(Selection::Scene(self.board.add_scene()));
        self.focus_scene_title = true;
        self.save_now();
    }

    fn duplicate_selected_scene(&mut self) {
        let Some(Selection::Scene(id)) = &self.selected else {
            return;
        };
        if let Some(copy) = self.board.duplicate_scene(id) {
            self.selected = Some(Selection::Scene(copy));
            self.save_now();
        }
    }

    fn shift_selection(&mut self, offset: isize) {
        let moved = match &self.selected {
            Some(Selection::Scene(id)) => self.board.shift_scene(id, offset),
            Some(Selection::Character(id)) => self.board.shift_character(id, offset),
            Some(Selection::Location(id)) => self.board.shift_location(id, offset),
            None => false,
        };
        if moved {
            self.save_now();
        }
    }

    fn export(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name(storage::export_file_name(&self.board, None, "json"))
            .save_file()
        else {
            return;
        };
        if let Err(e) = storage::save(&path, &self.board) {
            self.alert = Some(format!("Could not export: {e:#}"));
        }
    }

    fn export_markdown(
        &mut self,
        what: &str,
        file_suffix: Option<&str>,
        render: fn(&Board) -> String,
    ) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name(storage::export_file_name(&self.board, file_suffix, "md"))
            .save_file()
        else {
            return;
        };
        if let Err(e) = std::fs::write(&path, render(&self.board)) {
            self.alert = Some(format!("Could not export {what}: {e}"));
        }
    }

    fn import(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        {
            self.replace_board_from(&path);
        }
    }

    fn open_local_recovery(&mut self) {
        match storage::list_backups(&self.path) {
            Ok(backups) => {
                let selected = backups.iter().position(|b| b.contents.is_some());
                self.local_recovery = Some(LocalRecovery {
                    backups,
                    selected,
                    error: None,
                });
            }
            Err(e) => self.alert = Some(format!("Could not list backups: {e:#}")),
        }
    }

    fn recover_from_local(&mut self, path: &Path) {
        match storage::load(path) {
            Ok(board) => {
                self.replace_board(board);
                self.local_recovery = None;
            }
            Err(e) => {
                if let Some(recovery) = &mut self.local_recovery {
                    recovery.error = Some(format!("Could not recover: {e:#}"));
                }
            }
        }
    }

    /// Starts in the backups directory, or beside the board before its first
    /// backup, since a missing start directory sends the picker elsewhere.
    fn browse_for_local_backup(&mut self) {
        let backups = storage::backups_dir(&self.path);
        let start = if backups.is_dir() {
            backups
        } else {
            self.path.parent().unwrap_or(Path::new(".")).to_path_buf()
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_directory(start)
            .pick_file()
        {
            self.recover_from_local(&path);
        }
    }

    fn local_recovery_dialog(&mut self, ctx: &egui::Context) {
        let Some(recovery) = &mut self.local_recovery else {
            return;
        };
        let colors = self.colors;
        let backups_dir = storage::backups_dir(&self.path);
        let now = SystemTime::now();
        let mut recover = false;
        let mut browse = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("local_recovery")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Recover from local");
            ui.label(
                RichText::new(backups_dir.display().to_string())
                    .small()
                    .color(colors.muted),
            );
            if let Some(error) = &recovery.error {
                ui.label(RichText::new(error).small().color(colors.danger));
            }
            if recovery.backups.is_empty() {
                ui.label("This board has no backups yet.");
            } else {
                ui.label(
                    RichText::new("Newest first. Recovering can be undone.")
                        .small()
                        .color(colors.muted),
                );
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (index, backup) in recovery.backups.iter().enumerate() {
                            let row = ui
                                .add_enabled(
                                    backup.contents.is_some(),
                                    egui::Button::selectable(
                                        recovery.selected == Some(index),
                                        local_backup_label(backup, now),
                                    ),
                                )
                                .on_hover_text(backup.path.display().to_string());
                            if row.clicked() {
                                recovery.selected = Some(index);
                            }
                            if row.double_clicked() {
                                recover = true;
                            }
                        }
                    });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                recover |= ui
                    .add_enabled(recovery.selected.is_some(), egui::Button::new("Recover"))
                    .clicked();
                browse = ui
                    .button("Browse…")
                    .on_hover_text("Recover from any board file")
                    .clicked();
                close = ui.button("Cancel").clicked();
            });
        });
        let chosen = recovery
            .selected
            .filter(|_| recover)
            .map(|index| recovery.backups[index].path.clone());
        if let Some(path) = chosen {
            self.recover_from_local(&path);
        } else if browse {
            self.browse_for_local_backup();
        } else if close || modal.should_close() {
            self.local_recovery = None;
        }
    }

    fn replace_board_from(&mut self, path: &Path) {
        match storage::load(path) {
            Ok(board) => self.replace_board(board),
            Err(e) => self.alert = Some(format!("Could not import: {e:#}")),
        }
    }

    /// Undoable: the current board stays in history.
    fn replace_board(&mut self, board: Board) {
        self.board = board;
        self.selected = None;
        self.save_now();
        if self.save_state == SaveState::Saved {
            self.save_state = SaveState::Imported;
        }
    }

    fn open_board_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_directory(self.path.parent().unwrap_or(Path::new(".")))
            .pick_file()
        {
            self.switch_to_board_file(path);
        }
    }

    /// Returns whether the board at `path` is now the open one.
    fn switch_to_board_file(&mut self, path: PathBuf) -> bool {
        match storage::load(&path) {
            Ok(board) => {
                self.flush_pending_edits();
                self.queue_gist_push_of_open_board();
                self.history = History::new(board.clone(), UNDO_LIMIT);
                self.board = board;
                self.selected = None;
                if path != self.path {
                    self.settings.previous_board_path = Some(self.path.clone());
                }
                self.use_board_path(path);
                true
            }
            Err(e) => {
                self.alert = Some(format!("Could not open: {e:#}"));
                false
            }
        }
    }

    /// The current board stays in its own file, with its gist link, and
    /// becomes the previous board.
    fn new_board_file(&mut self) {
        let board = Board::default();
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let path = storage::unused_board_path(dir, &board);
        if let Err(e) = storage::save(&path, &board) {
            self.alert = Some(format!("Could not create a new board: {e:#}"));
            return;
        }
        self.switch_to_board_file(path);
    }

    fn open_cloud_picker(&mut self) {
        self.gist.cloud = Some(CloudPicker {
            queued: Some((GistTask::ListCloudBoards, gist::Job::ListBoards)),
            ..CloudPicker::default()
        });
    }

    fn start_queued_cloud_job(&mut self) {
        if self.gist.in_flight.is_some() || !self.gist.queued_pushes.is_empty() {
            return;
        }
        let Some((task, job)) = self.gist.cloud.as_mut().and_then(|c| c.queued.take()) else {
            return;
        };
        self.start_gist_job(task, job);
    }

    /// A gist already linked to a local file reopens that file, with the
    /// cloud version applied as an undoable change; otherwise the board
    /// gets a new file beside the current one.
    fn open_cloud_board(&mut self, gist_id: String, board: Board) {
        let linked = if self.gist_id().as_deref() == Some(gist_id.as_str()) {
            Some(self.path.clone())
        } else {
            self.settings
                .boards_linked_to(&gist_id)
                .find(|path| path.exists())
                .map(Path::to_owned)
        };
        if let Some(path) = linked {
            let local_is_newer = self.gist.unpushed_boards.contains(&path);
            if path != self.path && !self.switch_to_board_file(path) {
                return;
            }
            if local_is_newer {
                self.gist.board_changed = true;
                self.gist.push_now = true;
                return;
            }
            self.replace_board(board);
        } else {
            let dir = self.path.parent().unwrap_or(Path::new("."));
            let path = storage::unused_board_path(dir, &board);
            if let Err(e) = storage::save(&path, &board) {
                self.alert = Some(format!("Could not save the cloud board: {e:#}"));
                return;
            }
            self.settings.set_gist_id(&path, Some(gist_id));
            if !self.switch_to_board_file(path) {
                return;
            }
        }
        if let gist::Job::Push { board_json, .. } = self.push_job() {
            self.gist.last_pushed_json = Some(board_json);
        }
        self.gist.board_changed = false;
    }

    fn move_board_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name(
                self.path
                    .file_name()
                    .map_or_else(|| "board.json".into(), |n| n.to_string_lossy()),
            )
            .save_file()
        else {
            return;
        };
        if path == self.path {
            return;
        }
        if let Err(e) = storage::save(&path, &self.board) {
            self.alert = Some(format!("Could not move the board: {e:#}"));
            return;
        }
        self.history.commit(&self.board);
        self.settings.move_gist_id(&self.path, &path);
        self.use_board_path(path);
    }

    fn flush_pending_edits(&mut self) {
        if matches!(self.save_state, SaveState::Pending(_)) {
            self.save_now();
        }
    }

    /// Makes `path` the board file for this and later launches.
    fn use_board_path(&mut self, path: PathBuf) {
        self.path = path;
        self.disk_modified = storage::modified_time(&self.path);
        self.save_state = SaveState::Saved;
        self.last_backup = None;
        self.settings.board_path = Some(self.path.clone());
        self.save_settings_reporting_errors();
        self.gist.gist_id_draft = self.gist_id().unwrap_or_default();
        self.gist.gist_id_invalid = false;
        self.gist.last_pushed_json = None;
        self.gist.last_success = None;
        self.gist.board_changed = false;
        self.gist.push_now = false;
    }

    fn gist_id(&self) -> Option<String> {
        self.settings.gist_id(&self.path).map(str::to_owned)
    }

    fn token_path(&self) -> PathBuf {
        gist::token_path(&self.settings_path)
    }

    fn save_settings_reporting_errors(&mut self) {
        let save_error = settings::save(&self.settings_path, &self.settings)
            .err()
            .map(|e| format!("Could not save settings: {e:#}"));
        if let Some(dialog) = &mut self.settings_dialog {
            dialog.save_error = save_error;
        } else if let Some(error) = save_error {
            self.alert = Some(error);
        }
    }

    fn connect_github(&mut self) {
        let token = self.gist.token_draft.trim().to_owned();
        if token.is_empty() {
            return;
        }
        match gist::save_token(&self.token_path(), &token) {
            Ok(()) => {
                self.gist.token = Some(token);
                self.gist.token_draft.clear();
                self.gist.error = None;
            }
            Err(e) => self.gist.error = Some(format!("Could not save the token: {e:#}")),
        }
    }

    /// Forgets the token but keeps the gist id, so reconnecting resumes
    /// backing up to the same gist.
    fn disconnect_github(&mut self) {
        match gist::delete_token(&self.token_path()) {
            Ok(()) => {
                self.gist.token = None;
                self.gist.in_flight = None;
                self.gist.pushed_board = None;
                self.gist.error = None;
                self.gist.last_success = None;
            }
            Err(e) => self.gist.error = Some(format!("Could not remove the token: {e:#}")),
        }
    }

    fn apply_gist_id_draft(&mut self) {
        let draft = self.gist.gist_id_draft.trim();
        let gist_id = if draft.is_empty() {
            None
        } else if let Some(id) = gist::parse_gist_id(draft) {
            Some(id)
        } else {
            self.gist.gist_id_invalid = true;
            return;
        };
        self.gist.gist_id_invalid = false;
        if gist_id != self.gist_id() {
            self.settings.set_gist_id(&self.path, gist_id);
            self.gist.last_pushed_json = None;
            self.save_settings_reporting_errors();
        }
    }

    fn set_gist_auto_backup(&mut self, enabled: bool) {
        self.settings.gist_auto_backup = enabled;
        self.save_settings_reporting_errors();
    }

    fn gist_client(&self) -> Option<gist::Client> {
        let token = self.gist.token.as_deref()?;
        Some(match &self.gist.api_base {
            Some(api_base) => gist::Client::with_api_base(token, api_base),
            None => gist::Client::new(token),
        })
    }

    fn start_gist_job(&mut self, task: GistTask, job: gist::Job) {
        if self.gist.in_flight.is_some() {
            return;
        }
        let Some(client) = self.gist_client() else {
            return;
        };
        let pending = gist::Pending::start(client, job);
        self.gist.in_flight = Some((task, pending));
    }

    /// `None` when the gist already holds the board as it is now.
    fn unpushed_job(&self) -> Option<gist::Job> {
        let job = self.push_job();
        match &job {
            gist::Job::Push { board_json, .. }
                if self.gist.last_pushed_json.as_ref() == Some(board_json) =>
            {
                None
            }
            _ => Some(job),
        }
    }

    fn queue_gist_push_of_open_board(&mut self) {
        if !self.settings.gist_auto_backup
            || self.gist.token.is_none()
            || !self.gist.board_changed
            || self.board.is_empty()
        {
            return;
        }
        let Some(job) = self.unpushed_job() else {
            return;
        };
        self.gist
            .queued_pushes
            .retain(|(board, _)| *board != self.path);
        self.gist.queued_pushes.push((self.path.clone(), job));
        self.gist.unpushed_boards.insert(self.path.clone());
    }

    fn start_queued_push(&mut self) {
        if self.gist.in_flight.is_some() || self.gist.queued_pushes.is_empty() {
            return;
        }
        let (board, job) = self.gist.queued_pushes.remove(0);
        self.start_gist_job(GistTask::AutoPush, job);
        if self.gist.in_flight.is_some() {
            self.gist.pushed_board = Some(board);
        }
    }

    fn push_job(&self) -> gist::Job {
        gist::Job::Push {
            gist_id: self.gist_id(),
            title: self.board.title.clone(),
            board_json: serde_json::to_string_pretty(&self.board).unwrap_or_default(),
        }
    }

    /// An empty board is almost always a fresh start, and pushing
    /// it would make the gist's latest revision a blank board.
    fn back_up_to_gist(&mut self, task: GistTask) {
        if self.board.is_empty() {
            return;
        }
        self.gist.board_changed = false;
        self.gist.push_now = false;
        let job = self.push_job();
        self.start_gist_job(task, job);
        if self.gist.in_flight.is_some() {
            self.gist.pushed_board = Some(self.path.clone());
        }
    }

    /// Pushes once the board has gone unedited for `GIST_PUSH_DELAY`, so a
    /// burst of typing leaves one revision rather than one per keystroke.
    fn back_up_to_gist_if_due(&mut self, ctx: &egui::Context) {
        if !self.settings.gist_auto_backup
            || self.gist.token.is_none()
            || self.gist.in_flight.is_some()
            || !self.gist.board_changed
            || self.board.is_empty()
            || matches!(self.save_state, SaveState::Pending(_))
        {
            return;
        }
        if let Some(wait) = gist_push_wait(
            self.gist.last_edit.map(|at| at.elapsed()),
            self.gist.push_now,
        ) {
            ctx.request_repaint_after(wait);
            return;
        }
        if self.unpushed_job().is_none() {
            self.gist.board_changed = false;
            return;
        }
        self.back_up_to_gist(GistTask::AutoPush);
    }

    fn open_gist_restore(&mut self) {
        let Some(gist_id) = self.gist_id() else {
            return;
        };
        self.gist.restore = Some(GistRestore::default());
        self.start_gist_job(
            GistTask::ListRevisions,
            gist::Job::ListRevisions { gist_id },
        );
    }

    fn restore_gist_revision(&mut self, version: String) {
        let Some(gist_id) = self.gist_id() else {
            return;
        };
        self.start_gist_job(GistTask::Fetch, gist::Job::Fetch { gist_id, version });
    }

    fn receive_gist_reply(&mut self, ctx: &egui::Context) {
        let Some((task, pending)) = &self.gist.in_flight else {
            return;
        };
        let task = *task;
        let Some(reply) = pending.poll() else {
            ctx.request_repaint_after(GIST_POLL);
            return;
        };
        self.gist.in_flight = None;
        self.handle_gist_reply(task, reply);
    }

    fn handle_gist_reply(&mut self, task: GistTask, reply: gist::Reply) {
        match reply {
            gist::Reply::Pushed { pushed, board_json } => {
                let board = self
                    .gist
                    .pushed_board
                    .take()
                    .unwrap_or_else(|| self.path.clone());
                self.gist.error = None;
                if !self
                    .gist
                    .queued_pushes
                    .iter()
                    .any(|(queued, _)| *queued == board)
                {
                    self.gist.unpushed_boards.remove(&board);
                }
                if board == self.path {
                    self.gist.last_pushed_json = Some(board_json);
                    self.gist.last_success = Some(Instant::now());
                    self.gist.gist_id_draft.clone_from(&pushed.gist_id);
                }
                if self.settings.gist_id(&board) != Some(pushed.gist_id.as_str()) {
                    self.settings.set_gist_id(&board, Some(pushed.gist_id));
                    self.save_settings_reporting_errors();
                }
            }
            gist::Reply::Revisions { title, revisions } => {
                if let Some(restore) = &mut self.gist.restore {
                    restore.title = Some(title);
                    restore.revisions = Some(revisions);
                }
            }
            gist::Reply::Fetched(json) => match serde_json::from_str::<Board>(&json) {
                Ok(board) => {
                    self.gist.restore = None;
                    self.settings_dialog = None;
                    self.replace_board(board);
                }
                Err(e) => {
                    if let Some(restore) = &mut self.gist.restore {
                        restore.error = Some(format!("That version is not a valid board: {e}"));
                    }
                }
            },
            gist::Reply::Boards(boards) => {
                if let Some(cloud) = &mut self.gist.cloud {
                    cloud.boards = Some(boards);
                }
            }
            gist::Reply::FetchedLatest {
                gist_id,
                board_json,
            } => {
                if self.gist.cloud.is_none() {
                    return;
                }
                match serde_json::from_str::<Board>(&board_json) {
                    Ok(board) => {
                        self.gist.cloud = None;
                        self.open_cloud_board(gist_id, board);
                    }
                    Err(e) => {
                        if let Some(cloud) = &mut self.gist.cloud {
                            cloud.error = Some(format!("That gist is not a valid board: {e}"));
                        }
                    }
                }
            }
            gist::Reply::Failed(reason) if is_cloud_task(task) => {
                if let Some(cloud) = &mut self.gist.cloud {
                    cloud.error = Some(format!("Could not reach GitHub: {reason}"));
                }
            }
            gist::Reply::Failed(reason) => {
                let message = format!("GitHub backup failed: {reason}");
                match task {
                    GistTask::ListRevisions | GistTask::Fetch => {
                        if let Some(restore) = &mut self.gist.restore {
                            restore.error = Some(message);
                        }
                    }
                    GistTask::AutoPush if self.gist.error.is_none() => {
                        self.alert = Some(format!(
                            "{message}\n\nEdits are still saved locally. Check Settings → \
                             GitHub Gist."
                        ));
                        self.gist.error = Some(message);
                    }
                    GistTask::AutoPush | GistTask::ManualPush => self.gist.error = Some(message),
                    GistTask::ListCloudBoards | GistTask::OpenCloudBoard => {}
                }
                if matches!(task, GistTask::AutoPush | GistTask::ManualPush) {
                    self.gist.pushed_board = None;
                }
            }
        }
    }

    /// Blocks shutdown on one last push so the gist ends the session with
    /// the final state rather than one from before the last few edits.
    fn back_up_to_gist_before_exit(&mut self) {
        if let Some((task, pending)) = self.gist.in_flight.take() {
            let reply = pending.wait();
            self.handle_gist_reply(task, reply);
        }
        self.queue_gist_push_of_open_board();
        let Some(client) = self.gist_client() else {
            return;
        };
        for (board, job) in std::mem::take(&mut self.gist.queued_pushes) {
            let reply = gist::run(&client, job);
            self.gist.pushed_board = Some(board);
            self.handle_gist_reply(GistTask::ManualPush, reply);
        }
    }

    fn gist_section(&mut self, ui: &mut Ui) -> Option<GistAction> {
        let section_label = field_label(ui, self.colors, "GitHub Gist");
        let action = if self.gist.token.is_some() {
            self.gist_controls(ui, section_label)
        } else {
            self.gist_connect_form(ui, section_label)
        };
        let (status, color) = self.gist_status();
        if !status.is_empty() {
            ui.label(RichText::new(status).small().color(color));
        }
        action
    }

    fn gist_connect_form(&mut self, ui: &mut Ui, label: egui::Id) -> Option<GistAction> {
        ui.label(
            RichText::new(
                "Optionally back up the board to a secret gist on your GitHub account, with \
                 every version kept in its history. Create a fine-grained token with the \
                 account permission Gists: read and write (or a classic token with the gist \
                 scope) and paste it here.",
            )
            .small()
            .color(self.colors.muted),
        );
        let mut action = None;
        ui.horizontal(|ui| {
            let field = ui
                .add(
                    TextEdit::singleline(&mut self.gist.token_draft)
                        .password(true)
                        .hint_text("GitHub token")
                        .desired_width(240.0),
                )
                .labelled_by(label);
            let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui.button("Connect").clicked() || submitted {
                action = Some(GistAction::Connect);
            }
        });
        action
    }

    fn gist_controls(&mut self, ui: &mut Ui, label: egui::Id) -> Option<GistAction> {
        let mut action = None;
        let busy = self.gist.in_flight.is_some();
        ui.horizontal(|ui| {
            let id_field = ui
                .add(
                    TextEdit::singleline(&mut self.gist.gist_id_draft)
                        .hint_text("Gist id or URL; empty creates one")
                        .desired_width(240.0),
                )
                .labelled_by(label);
            if id_field.changed() {
                action = Some(GistAction::SetGistId);
            }
            if let Some(id) = self.gist_id() {
                ui.hyperlink_to("Open", format!("https://gist.github.com/{id}"));
            }
        });
        if self.gist.gist_id_invalid {
            ui.label(
                RichText::new("Not a gist id or URL")
                    .small()
                    .color(self.colors.danger),
            );
        }
        let mut auto = self.settings.gist_auto_backup;
        if ui
            .checkbox(&mut auto, "Back up automatically")
            .on_hover_text(
                "5 seconds after you stop editing, right away on a title edit or \
                     focus change, and when you quit",
            )
            .changed()
        {
            self.set_gist_auto_backup(auto);
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy && !self.board.is_empty(),
                    egui::Button::new("Back up now"),
                )
                .clicked()
            {
                action = Some(GistAction::BackUpNow);
            }
            if ui
                .add_enabled(
                    !busy && self.gist_id().is_some(),
                    egui::Button::new("Restore from gist…"),
                )
                .clicked()
            {
                action = Some(GistAction::Restore);
            }
            if ui
                .button("Disconnect")
                .on_hover_text("Forget the token; the gist itself stays on GitHub")
                .clicked()
            {
                action = Some(GistAction::Disconnect);
            }
        });
        action
    }

    fn gist_status(&self) -> (String, Color32) {
        let muted = self.colors.muted;
        if let Some(error) = &self.gist.error {
            return (error.clone(), self.colors.danger);
        }
        if self.gist.token.is_none() {
            return (String::new(), muted);
        }
        let status = if self.gist.in_flight.is_some() {
            "Talking to GitHub…".to_owned()
        } else if self.board.is_empty() {
            "Empty boards are not backed up.".to_owned()
        } else if let Some(at) = self.gist.last_success {
            format!("Backed up {}", elapsed_description(at.elapsed()))
        } else if self.gist_id().is_none() {
            "The first backup creates a secret gist.".to_owned()
        } else {
            "Not backed up in this session yet.".to_owned()
        };
        (status, muted)
    }

    fn handle_gist_action(&mut self, action: GistAction) {
        match action {
            GistAction::Connect => self.connect_github(),
            GistAction::Disconnect => self.disconnect_github(),
            GistAction::SetGistId => self.apply_gist_id_draft(),
            GistAction::BackUpNow => self.back_up_to_gist(GistTask::ManualPush),
            GistAction::Restore => self.open_gist_restore(),
        }
    }

    fn gist_restore_dialog(&mut self, ctx: &egui::Context) {
        let Some(restore) = &self.gist.restore else {
            return;
        };
        let colors = self.colors;
        let busy = self.gist.in_flight.is_some();
        let mut chosen = None;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("gist_restore")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Restore from gist");
            if let Some(title) = &restore.title {
                let title = if title.is_empty() { "Untitled" } else { title };
                ui.label(RichText::new(title).strong());
            }
            if let Some(error) = &restore.error {
                ui.label(RichText::new(error).small().color(colors.danger));
            }
            match &restore.revisions {
                None if busy => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading versions…");
                    });
                }
                None => {}
                Some(revisions) if revisions.is_empty() => {
                    ui.label("This gist has no versions yet.");
                }
                Some(revisions) => {
                    ui.label(
                        RichText::new("Newest first. Restoring can be undone.")
                            .small()
                            .color(colors.muted),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for revision in revisions {
                                let label = format!(
                                    "{}  ·  {}",
                                    display_timestamp(&revision.committed_at),
                                    short_version(&revision.version)
                                );
                                if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                                    chosen = Some(revision.version.clone());
                                }
                            }
                        });
                    if busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Downloading…");
                        });
                    }
                }
            }
            ui.add_space(8.0);
            close = ui.button("Cancel").clicked();
        });
        if let Some(version) = chosen {
            if let Some(restore) = &mut self.gist.restore {
                restore.error = None;
            }
            self.restore_gist_revision(version);
        }
        if close || modal.should_close() {
            self.gist.restore = None;
        }
    }

    fn cloud_open_dialog(&mut self, ctx: &egui::Context) {
        let Some(cloud) = &self.gist.cloud else {
            return;
        };
        let colors = self.colors;
        let busy = cloud.queued.is_some()
            || matches!(&self.gist.in_flight, Some((task, _)) if is_cloud_task(*task));
        let open_gist_id = self.gist_id();
        let mut chosen = None;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("cloud_open")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Open from cloud");
            if let Some(error) = &cloud.error {
                ui.label(RichText::new(error).small().color(colors.danger));
            }
            let other_boards = cloud
                .boards
                .as_deref()
                .map(|boards| boards_other_than(boards, open_gist_id.as_deref()));
            match other_boards {
                None if busy => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading boards…");
                    });
                }
                None => {}
                Some(boards) if boards.is_empty() => {
                    let message = if open_gist_id.is_some() {
                        "No other boards backed up to your gists."
                    } else {
                        "No boards backed up to your gists yet."
                    };
                    ui.label(message);
                }
                Some(boards) => {
                    ui.label(
                        RichText::new("Each opens at its latest version.")
                            .small()
                            .color(colors.muted),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for board in boards {
                                let title = display_cloud_title(&board.title);
                                if ui
                                    .add_enabled(!busy, egui::Button::new(title))
                                    .on_hover_text(format!(
                                        "Last saved {}",
                                        display_timestamp(&board.updated_at)
                                    ))
                                    .clicked()
                                {
                                    chosen = Some(board.gist_id.clone());
                                }
                            }
                        });
                    if busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Downloading…");
                        });
                    }
                }
            }
            ui.add_space(8.0);
            close = ui.button("Cancel").clicked();
        });
        if let Some(gist_id) = chosen
            && let Some(cloud) = &mut self.gist.cloud
        {
            cloud.error = None;
            cloud.queued = Some((GistTask::OpenCloudBoard, gist::Job::FetchLatest { gist_id }));
            self.start_queued_cloud_job();
        }
        if close || modal.should_close() {
            self.gist.cloud = None;
        }
    }

    fn local_open_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Local", |ui| {
            match self.settings.previous_board_path.clone() {
                Some(path) => {
                    let name = path
                        .file_name()
                        .map_or_else(|| path.display().to_string(), |n| n.display().to_string());
                    if ui
                        .button(name)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        self.switch_to_board_file(path);
                    }
                }
                None => {
                    ui.label(RichText::new("No other board opened yet").color(self.colors.muted));
                }
            }
        });
        ui.add_enabled(false, egui::Button::new("Cloud…"))
            .on_disabled_hover_text("Connect GitHub in Settings first");
    }

    fn export_menu(&mut self, ui: &mut Ui) {
        if ui.button("Board (JSON)").clicked() {
            self.export();
        }
        if ui.button("Outline (Markdown)").clicked() {
            self.export_markdown("outline", Some("outline"), outline::to_markdown);
        }
        if ui
            .button("Manuscript (Markdown)")
            .on_hover_text("The prose of every scene, for a word processor")
            .clicked()
        {
            self.export_markdown(
                "manuscript",
                Some("manuscript"),
                outline::manuscript_markdown,
            );
        }
        if ui
            .button("Outline + Manuscript (Markdown)")
            .on_hover_text("The outline followed by the manuscript, in one file")
            .clicked()
        {
            self.export_markdown("outline and manuscript", None, outline::combined_markdown);
        }
    }

    fn open_button(&mut self, ui: &mut Ui) {
        if self.gist.token.is_none() {
            ui.menu_button("Open", |ui| self.local_open_menu(ui));
        } else if ui
            .button("Open")
            .on_hover_text("Boards backed up to your GitHub gists")
            .clicked()
        {
            self.open_cloud_picker();
        }
    }

    fn header(&mut self, ui: &mut Ui) {
        let colors = self.colors;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Manuscript Board")
                    .size(17.0)
                    .strong()
                    .color(colors.header_text),
            );
            let title = ui.add(
                TextEdit::singleline(&mut self.board.title)
                    .hint_text("Novel title")
                    .desired_width(360.0),
            );
            if title.changed() {
                self.gist.push_now = true;
                self.save_now();
            }

            ui.checkbox(
                &mut self.show_stats,
                RichText::new("Stats").color(colors.header_text),
            );
            if ui.button("Settings").clicked() {
                self.settings_dialog = Some(SettingsDialog::default());
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("+ Scene").color(colors.on_accent))
                            .fill(colors.accent),
                    )
                    .on_hover_text(ui.ctx().format_shortcut(&NEW_SCENE))
                    .clicked()
                {
                    self.add_scene();
                }
                if ui
                    .add_enabled(
                        self.history.can_redo(&self.board),
                        egui::Button::new("Redo"),
                    )
                    .on_hover_text(ui.ctx().format_shortcut(&REDO))
                    .clicked()
                {
                    self.redo();
                }
                if ui
                    .add_enabled(
                        self.history.can_undo(&self.board),
                        egui::Button::new("Undo"),
                    )
                    .on_hover_text(ui.ctx().format_shortcut(&UNDO))
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .button("New")
                    .on_hover_text("Start an empty board in a new file")
                    .clicked()
                {
                    self.new_board_file();
                }
                if ui.button("Import").clicked() {
                    self.import();
                }
                self.open_button(ui);
                ui.menu_button("Export", |ui| self.export_menu(ui));
                let (text, color) = match &self.save_state {
                    SaveState::Saved => ("saved", colors.header_text),
                    SaveState::Pending(_) => ("saving…", colors.header_text),
                    SaveState::Imported => ("imported", colors.header_text),
                    SaveState::Reloaded => ("reloaded", colors.header_text),
                    SaveState::Failed(_) => ("save failed", Color32::from_rgb(0xff, 0x9a, 0x8a)),
                };
                let label = ui.label(RichText::new(text).small().color(color));
                match &self.save_state {
                    SaveState::Failed(reason) => {
                        label.on_hover_text(reason);
                    }
                    SaveState::Reloaded => {
                        label.on_hover_text("Picked up changes made to the board file elsewhere");
                    }
                    _ => {}
                }
            });
        });
    }

    fn filter_panel(&mut self, ui: &mut Ui) {
        let colors = self.colors;
        ui.heading("Filter");
        ui.add_space(2.0);

        ui.horizontal(|ui| {
            let search = ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text("Search scenes, prose, POV, location")
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut self.focus_search) {
                search.request_focus();
            }
        });
        ui.add_space(6.0);

        let filter_status_label = ui.label(filter_label(colors, "Status")).id;
        ComboBox::from_id_salt("filter_status")
            .selected_text(match self.filter_status {
                Some(status) => status.to_string(),
                None => "All".to_owned(),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.filter_status, None, "All");
                for status in Status::ALL {
                    ui.selectable_value(&mut self.filter_status, Some(status), status.to_string());
                }
            })
            .response
            .labelled_by(filter_status_label);
        ui.add_space(4.0);

        let filter_act_label = ui.label(filter_label(colors, "Act")).id;
        ComboBox::from_id_salt("filter_act")
            .selected_text(match self.filter_act {
                Some(act) => act.to_string(),
                None => "All".to_owned(),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.filter_act, None, "All");
                for act in Act::ALL {
                    ui.selectable_value(&mut self.filter_act, Some(act), act.to_string());
                }
            })
            .response
            .labelled_by(filter_act_label);
        ui.add_space(4.0);

        let filter_pov_label = ui.label(filter_label(colors, "POV")).id;
        let pov_options: Vec<(Id, String)> = self
            .board
            .characters
            .iter()
            .map(|c| (c.id.clone(), display_name(&c.name).to_owned()))
            .collect();
        let pov_selected = self
            .filter_pov
            .as_ref()
            .and_then(|id| self.board.character(id))
            .map_or_else(|| "All".to_owned(), |c| display_name(&c.name).to_owned());
        ComboBox::from_id_salt("filter_pov")
            .selected_text(pov_selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.filter_pov, None, "All");
                for (id, name) in &pov_options {
                    ui.selectable_value(&mut self.filter_pov, Some(id.clone()), name);
                }
            })
            .response
            .labelled_by(filter_pov_label);

        ui.add_space(6.0);
        let count = self
            .board
            .matching_indices(
                &self.query,
                self.filter_status,
                self.filter_act,
                self.filter_pov.as_ref(),
            )
            .len();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} scene{}",
                    count,
                    if count == 1 { "" } else { "s" }
                ))
                .small()
                .color(colors.muted),
            );
            if self.filters_active() && ui.small_button("Clear filters").clicked() {
                self.clear_filters();
            }
        });
    }

    fn filters_active(&self) -> bool {
        !self.query.trim().is_empty()
            || self.filter_status.is_some()
            || self.filter_act.is_some()
            || self.filter_pov.is_some()
    }

    fn clear_filters(&mut self) {
        self.query.clear();
        self.filter_status = None;
        self.filter_act = None;
        self.filter_pov = None;
    }

    fn characters_panel(&mut self, ui: &mut Ui) {
        let colors = self.colors;
        ui.heading("Characters");
        ui.add_space(4.0);

        if self.board.characters.is_empty() {
            ui.label(
                RichText::new("No characters yet.")
                    .small()
                    .color(colors.muted),
            );
        }

        let mut to_delete = None;
        let mut clicked = None;
        for character in &self.board.characters {
            let pov_count = self.board.pov_scene_count(&character.id);
            let is_selected = self.selected == Some(Selection::Character(character.id.clone()));
            let card = ui.scope_builder(egui::UiBuilder::new().sense(Sense::click()), |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                card_frame(colors, colors.card)
                    .stroke(selection_stroke(colors, is_selected))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(display_name(&character.name)).strong());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let delete = ui.add(
                                    egui::Label::new(
                                        RichText::new("×").strong().color(colors.danger),
                                    )
                                    .sense(Sense::click()),
                                );
                                if delete.on_hover_text("Remove character").clicked() {
                                    to_delete = Some(character.id.clone());
                                }
                            });
                        });
                        let plural = if pov_count == 1 { "" } else { "s" };
                        ui.label(
                            RichText::new(format!("{pov_count} POV scene{plural}"))
                                .small()
                                .color(colors.muted),
                        );
                        if !character.notes.is_empty() {
                            ui.label(RichText::new(preview(&character.notes)).small());
                        }
                    });
            });
            if card.response.clicked() {
                clicked = Some(character.id.clone());
            }
            if card.response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.add_space(6.0);
        }
        if let Some(id) = clicked {
            self.selected = Some(Selection::Character(id));
        }
        if let Some(id) = to_delete {
            self.delete_character(&id);
        }

        let input = ui.add(
            TextEdit::singleline(&mut self.new_character)
                .hint_text("New character name")
                .desired_width(f32::INFINITY),
        );
        if input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            if self.board.add_character(&self.new_character) {
                self.new_character.clear();
                self.save_now();
            }
            input.request_focus();
        }
        ui.label(
            RichText::new("Type a name + Enter to add.")
                .small()
                .color(colors.muted),
        );
    }

    fn locations_panel(&mut self, ui: &mut Ui) {
        let colors = self.colors;
        ui.heading("Locations");
        ui.add_space(4.0);
        if self.board.locations.is_empty() {
            ui.label(
                RichText::new("No locations yet.")
                    .small()
                    .color(colors.muted),
            );
        }
        let mut clicked = None;
        for location in &self.board.locations {
            let is_selected = self.selected == Some(Selection::Location(location.id.clone()));
            let count = self.board.location_scene_count(&location.id);
            let response = ui
                .selectable_label(is_selected, display_name(&location.name))
                .on_hover_text(format!("{count} scene{}", plural(count)));
            if response.clicked() {
                clicked = Some(location.id.clone());
            }
        }
        if let Some(id) = clicked {
            self.selected = Some(Selection::Location(id));
        }
        ui.add_space(4.0);
        let input = ui.add(
            TextEdit::singleline(&mut self.new_location)
                .hint_text("New location name")
                .desired_width(f32::INFINITY),
        );
        if input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            if self.board.add_location(&self.new_location) {
                self.new_location.clear();
                self.save_now();
            }
            input.request_focus();
        }
    }

    fn stats_panel(&self, ui: &mut Ui) {
        ui.heading("Statistics");
        ui.add_space(4.0);
        Grid::new("stats")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for (status, count) in self.board.status_counts() {
                    ui.label(status.to_string());
                    ui.label(count.to_string());
                    ui.end_row();
                }
                for (act, count) in self.board.act_counts() {
                    ui.label(act.to_string());
                    ui.label(count.to_string());
                    ui.end_row();
                }
                ui.label(RichText::new("Total").strong());
                ui.label(RichText::new(self.board.scenes.len().to_string()).strong());
                ui.end_row();
                ui.label("Words");
                ui.label(format_count(self.board.manuscript_word_count()));
                ui.end_row();
            });
    }

    fn delete_character(&mut self, id: &Id) {
        self.board.delete_character(id);
        if self.selected == Some(Selection::Character(id.clone())) {
            self.selected = None;
        }
        self.save_now();
    }

    /// Deleting, switching boards or undoing can remove the character a filter
    /// points at, which would otherwise hide every card.
    fn drop_stale_pov_filter(&mut self) {
        if let Some(id) = &self.filter_pov
            && self.board.character(id).is_none()
        {
            self.filter_pov = None;
        }
    }

    fn inspector_panel(&mut self, ui: &mut Ui) {
        match self.selected.clone() {
            Some(Selection::Scene(id)) => self.scene_inspector(ui, id),
            Some(Selection::Character(id)) => self.character_inspector(ui, &id),
            Some(Selection::Location(id)) => self.location_inspector(ui, &id),
            None => {}
        }
    }

    fn character_inspector(&mut self, ui: &mut Ui, id: &Id) {
        let colors = self.colors;
        let pov_count = self.board.pov_scene_count(id);
        let character_count = self.board.characters.len();
        let Some(index) = self.board.character_index(id) else {
            self.selected = None;
            return;
        };
        let Some(character) = self.board.character_mut(id) else {
            self.selected = None;
            return;
        };
        let mut changed = false;
        let mut close = false;

        close |= inspector_header(ui, colors, "CHARACTER", display_name(&character.name));

        changed |= character_profile_fields(ui, colors, character);

        let plural = if pov_count == 1 { "" } else { "s" };
        ui.label(
            RichText::new(format!("POV in {pov_count} scene{plural}"))
                .small()
                .color(colors.muted),
        );

        let (relationships_changed, relationship_added) = self.relationship_editor(ui, id);
        changed |= relationships_changed;

        ui.add_space(10.0);
        let mut shift = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(index > 0, egui::Button::new("Move up"))
                .on_hover_text(ui.ctx().format_shortcut(&MOVE_CHARACTER_UP))
                .clicked()
            {
                shift = Some(-1);
            }
            if ui
                .add_enabled(index + 1 < character_count, egui::Button::new("Move down"))
                .on_hover_text(ui.ctx().format_shortcut(&MOVE_CHARACTER_DOWN))
                .clicked()
            {
                shift = Some(1);
            }
        });

        ui.add_space(6.0);
        let delete = ui
            .add(
                egui::Button::new(RichText::new("Delete character").color(Color32::WHITE))
                    .fill(colors.danger),
            )
            .clicked();

        if changed {
            self.mark_dirty();
        }
        if relationship_added {
            self.save_now();
        }
        if let Some(offset) = shift {
            self.shift_selection(offset);
        }
        if delete {
            self.delete_character(id);
        } else if close {
            self.selected = None;
        }
    }

    /// Returns (labels edited, relationship added or removed).
    fn relationship_editor(&mut self, ui: &mut Ui, id: &Id) -> (bool, bool) {
        let colors = self.colors;
        let mut edited = false;
        let mut to_remove = None;
        let names: Vec<(Id, String)> = self
            .board
            .characters
            .iter()
            .map(|c| (c.id.clone(), display_name(&c.name).to_owned()))
            .collect();
        let name_of = |other: &Id| {
            names
                .iter()
                .find(|(cid, _)| cid == other)
                .map_or("Unnamed", |(_, name)| name.as_str())
        };

        let linked_pairs: HashSet<(Id, Id)> = self
            .board
            .relationships
            .iter()
            .map(|r| (r.from.clone(), r.to.clone()))
            .collect();
        let mut to_reverse = None;

        let relationships_label = field_label(ui, colors, "Relationships");
        let mut any = false;
        for relationship in self
            .board
            .relationships
            .iter_mut()
            .filter(|r| &r.from == id || &r.to == id)
        {
            any = true;
            let (arrow, other) = if &relationship.from == id {
                ("to", &relationship.to)
            } else {
                ("from", &relationship.from)
            };
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .small_button("×")
                        .on_hover_text("Remove relationship")
                        .clicked()
                    {
                        to_remove = Some(relationship.id.clone());
                    }
                    let reverse_pair = (relationship.to.clone(), relationship.from.clone());
                    if !linked_pairs.contains(&reverse_pair)
                        && ui
                            .small_button("Add reverse")
                            .on_hover_text("Add a relationship pointing the other way")
                            .clicked()
                    {
                        to_reverse = Some(relationship.id.clone());
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(Label::new(format!("{arrow} {}", name_of(other))).truncate());
                    });
                });
            });
            edited |= ui
                .add(
                    TextEdit::singleline(&mut relationship.label)
                        .hint_text("how they relate")
                        .desired_width(f32::INFINITY),
                )
                .changed();
        }
        if !any {
            ui.label(RichText::new("None yet.").small().color(colors.muted));
        }

        let mut added = self.add_relationship_row(ui, id, &names, relationships_label);
        if let Some(rel) = to_remove {
            self.board.delete_relationship(&rel);
            added = true;
        }
        if let Some(rel) = to_reverse {
            added |= self.board.add_reverse_relationship(&rel);
        }
        (edited, added)
    }

    /// Returns `true` when a relationship was added.
    fn add_relationship_row(
        &mut self,
        ui: &mut Ui,
        id: &Id,
        names: &[(Id, String)],
        label: egui::Id,
    ) -> bool {
        let others: Vec<&(Id, String)> = names.iter().filter(|(cid, _)| cid != id).collect();
        if self
            .new_relationship_target
            .as_ref()
            .is_some_and(|t| t == id || !others.iter().any(|(cid, _)| cid == t))
        {
            self.new_relationship_target = None;
        }
        if others.is_empty() {
            return false;
        }
        let mut added = false;
        ui.horizontal(|ui| {
            let selected = self
                .new_relationship_target
                .as_ref()
                .and_then(|t| others.iter().find(|(cid, _)| cid == t))
                .map_or("Character…", |(_, name)| name.as_str());
            ComboBox::from_id_salt("relationship_target")
                .selected_text(selected)
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (cid, name) in &others {
                        ui.selectable_value(
                            &mut self.new_relationship_target,
                            Some(cid.clone()),
                            name.as_str(),
                        );
                    }
                })
                .response
                .labelled_by(label);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let add_clicked = ui
                    .add_enabled(
                        self.new_relationship_target.is_some(),
                        egui::Button::new("Add"),
                    )
                    .clicked();
                ui.add(
                    TextEdit::singleline(&mut self.new_relationship_label)
                        .hint_text("mentor, sister…")
                        .desired_width(f32::INFINITY),
                );
                if add_clicked && let Some(target) = self.new_relationship_target.take() {
                    added = self
                        .board
                        .add_relationship(id, &target, &self.new_relationship_label);
                    self.new_relationship_label.clear();
                }
            });
        });
        added
    }

    fn location_inspector(&mut self, ui: &mut Ui, id: &Id) {
        let colors = self.colors;
        let scene_count = self.board.location_scene_count(id);
        let location_count = self.board.locations.len();
        let Some(index) = self.board.location_index(id) else {
            self.selected = None;
            return;
        };
        let Some(location) = self.board.location_mut(id) else {
            self.selected = None;
            return;
        };
        let mut changed = false;
        let mut close = false;

        close |= inspector_header(ui, colors, "LOCATION", display_name(&location.name));

        let name_label = field_label(ui, colors, "Name");
        changed |= ui
            .add(TextEdit::singleline(&mut location.name).desired_width(f32::INFINITY))
            .labelled_by(name_label)
            .changed();

        let notes_label = field_label(ui, colors, "Notes");
        changed |= ui
            .add(
                TextEdit::multiline(&mut location.notes)
                    .hint_text("Look, sound, smell, history, who goes there.")
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(notes_label)
            .changed();

        ui.label(
            RichText::new(format!(
                "Setting of {scene_count} scene{}",
                plural(scene_count)
            ))
            .small()
            .color(colors.muted),
        );

        ui.add_space(10.0);
        let mut shift = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(index > 0, egui::Button::new("Move up"))
                .on_hover_text(ui.ctx().format_shortcut(&MOVE_CHARACTER_UP))
                .clicked()
            {
                shift = Some(-1);
            }
            if ui
                .add_enabled(index + 1 < location_count, egui::Button::new("Move down"))
                .on_hover_text(ui.ctx().format_shortcut(&MOVE_CHARACTER_DOWN))
                .clicked()
            {
                shift = Some(1);
            }
        });

        ui.add_space(6.0);
        let delete = ui
            .add(
                egui::Button::new(RichText::new("Delete location").color(Color32::WHITE))
                    .fill(colors.danger),
            )
            .clicked();

        if changed {
            self.mark_dirty();
        }
        if let Some(offset) = shift {
            self.shift_selection(offset);
        }
        if delete {
            self.board.delete_location(id);
            self.selected = None;
            self.save_now();
        } else if close {
            self.selected = None;
        }
    }

    fn scene_inspector(&mut self, ui: &mut Ui, id: Id) {
        let colors = self.colors;
        let Some(index) = self.board.scene_index(&id) else {
            self.selected = None;
            return;
        };
        let characters = self.board.characters.clone();
        let locations = self.board.locations.clone();
        let in_write_view = self.view == View::Write;
        let is_last = index + 1 == self.board.scenes.len();
        let scene = &mut self.board.scenes[index];
        let mut changed = false;
        let mut close = false;

        close |= inspector_header(ui, colors, &scene_number(index), scene_title(scene));

        let title_label = field_label(ui, colors, "Title");
        let title = ui
            .add(
                TextEdit::singleline(&mut scene.title)
                    .hint_text("e.g. The confrontation in the rain")
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(title_label);
        if std::mem::take(&mut self.focus_scene_title) {
            title.request_focus();
        }
        changed |= title.changed();

        let summary_label = field_label(ui, colors, "Summary / beat");
        changed |= ui
            .add(
                TextEdit::multiline(&mut scene.summary)
                    .hint_text("What happens in this scene. What changes.")
                    .desired_rows(5)
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(summary_label)
            .changed();

        changed |= classification_fields(ui, colors, scene, &characters);
        changed |= setting_fields(ui, colors, scene, &locations);

        ui.add_space(8.0);
        let write = word_count_row(ui, colors, scene, !in_write_view);

        ui.add_space(10.0);
        let mut shift = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(index > 0, egui::Button::new("Move earlier"))
                .on_hover_text(ui.ctx().format_shortcut(&MOVE_SCENE_EARLIER))
                .clicked()
            {
                shift = Some(-1);
            }
            if ui
                .add_enabled(!is_last, egui::Button::new("Move later"))
                .on_hover_text(ui.ctx().format_shortcut(&MOVE_SCENE_LATER))
                .clicked()
            {
                shift = Some(1);
            }
        });

        ui.add_space(6.0);
        let mut duplicate = false;
        ui.horizontal(|ui| {
            duplicate = ui
                .button("Duplicate")
                .on_hover_text(ui.ctx().format_shortcut(&DUPLICATE_SCENE))
                .clicked();
            if ui
                .add(
                    egui::Button::new(RichText::new("Delete scene").color(Color32::WHITE))
                        .fill(colors.danger),
                )
                .clicked()
            {
                self.scene_to_delete = Some(id);
            }
        });

        if changed {
            self.mark_dirty();
        }
        if let Some(offset) = shift {
            self.shift_selection(offset);
        }
        if write {
            self.view = View::Write;
        }
        if duplicate {
            self.duplicate_selected_scene();
        } else if close {
            self.selected = None;
        }
    }

    fn view_tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for view in View::ALL {
                ui.selectable_value(&mut self.view, view, view.name());
            }
        });
        ui.add_space(10.0);
    }

    fn write_view(&mut self, ui: &mut Ui) {
        let colors = self.colors;
        if self.board.scenes.is_empty() {
            empty_state(
                ui,
                "No scenes yet",
                "Add a scene, then write its prose here.",
            );
            return;
        }
        let selected_index = match &self.selected {
            Some(Selection::Scene(id)) => self.board.scene_index(id),
            _ => None,
        };
        let Some(index) = selected_index else {
            self.scene_picker(ui);
            return;
        };

        let scene_count = self.board.scenes.len();
        let total_words = self.board.manuscript_word_count();
        let mut go_to = None;
        let mut prose_label = None;
        let scene = &mut self.board.scenes[index];
        ui.horizontal(|ui| {
            if ui
                .add_enabled(index > 0, egui::Button::new("Previous"))
                .clicked()
            {
                go_to = Some(index - 1);
            }
            prose_label = Some(
                ui.label(
                    RichText::new(format!("{}  {}", scene_number(index), scene_title(scene)))
                        .strong(),
                )
                .id,
            );
            if ui
                .add_enabled(index + 1 < scene_count, egui::Button::new("Next"))
                .clicked()
            {
                go_to = Some(index + 1);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let words = scene.word_count();
                ui.label(
                    RichText::new(format!(
                        "{} word{} · {} in manuscript",
                        format_count(words),
                        plural(words),
                        format_count(total_words)
                    ))
                    .small()
                    .color(colors.muted),
                );
            });
        });
        if !scene.summary.trim().is_empty() {
            ui.label(
                RichText::new(scene.summary.trim())
                    .small()
                    .italics()
                    .color(colors.muted),
            );
        }
        ui.add_space(6.0);

        let mut changed = false;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let prose = ui.add_sized(
                    ui.available_size(),
                    TextEdit::multiline(&mut scene.text)
                        .id_salt(("prose", &scene.id))
                        .hint_text("Start writing…")
                        .font(egui::FontId::proportional(16.0))
                        .margin(Margin::same(14))
                        .frame(
                            Frame::new()
                                .fill(colors.card)
                                .stroke(Stroke::new(1.0, colors.line)),
                        ),
                );
                changed = prose.changed();
                if let Some(label) = prose_label {
                    prose.labelled_by(label);
                }
            });
        if changed {
            self.mark_dirty();
        }
        if let Some(target) = go_to {
            self.selected = Some(Selection::Scene(self.board.scenes[target].id.clone()));
        }
    }

    fn scene_picker(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Pick a scene to write").size(16.0).strong());
        ui.add_space(6.0);
        let mut picked = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, scene) in self.board.scenes.iter().enumerate() {
                if ui
                    .link(format!("{}  {}", scene_number(i), scene_title(scene)))
                    .clicked()
                {
                    picked = Some(scene.id.clone());
                }
            }
        });
        if let Some(id) = picked {
            self.selected = Some(Selection::Scene(id));
        }
    }

    fn timeline_view(&mut self, ui: &mut Ui) {
        let colors = self.colors;
        let dated = self.board.chronological_indices();
        let undated: Vec<usize> = self
            .board
            .scenes
            .iter()
            .enumerate()
            .filter(|(_, s)| s.day.is_none() && s.status != Status::Archived)
            .map(|(i, _)| i)
            .collect();
        if dated.is_empty() && undated.is_empty() {
            empty_state(
                ui,
                "No scenes yet",
                "Scenes with a story day appear here in story order.",
            );
            return;
        }

        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if dated.is_empty() {
                    ui.label(
                        RichText::new(
                            "Set a story day on scenes in the inspector to see them in \
                             story order, flashbacks included.",
                        )
                        .color(colors.muted),
                    );
                }
                let mut current_day = None;
                for &index in &dated {
                    let day = self.board.scenes[index].day;
                    if day != current_day {
                        current_day = day;
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("DAY {}", day.unwrap_or_default()))
                                .size(12.0)
                                .strong()
                                .color(colors.muted),
                        );
                    }
                    if self.timeline_row(ui, index) {
                        clicked = Some(index);
                    }
                }
                if !undated.is_empty() {
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new("UNDATED")
                            .size(12.0)
                            .strong()
                            .color(colors.muted),
                    );
                    for &index in &undated {
                        if self.timeline_row(ui, index) {
                            clicked = Some(index);
                        }
                    }
                }
            });
        if let Some(index) = clicked {
            self.selected = Some(Selection::Scene(self.board.scenes[index].id.clone()));
        }
    }

    /// Returns `true` when the row was clicked.
    fn timeline_row(&self, ui: &mut Ui, index: usize) -> bool {
        let colors = self.colors;
        let scene = &self.board.scenes[index];
        let is_selected = self.selected == Some(Selection::Scene(scene.id.clone()));
        let row = ui.scope_builder(egui::UiBuilder::new().sense(Sense::click()), |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            Frame::new()
                .fill(colors.card)
                .stroke(selection_stroke(colors, is_selected))
                .corner_radius(4)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width().min(720.0));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(scene_number(index))
                                .small()
                                .strong()
                                .color(colors.muted),
                        );
                        ui.label(RichText::new(scene_title(scene)).strong());
                        let details = scene_details(&self.board, scene);
                        if !details.is_empty() {
                            ui.label(RichText::new(details).small().color(colors.muted));
                        }
                    });
                });
        });
        ui.add_space(4.0);
        if row.response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        row.response.clicked()
    }

    fn relationships_view(&mut self, ui: &mut Ui) {
        let colors = self.colors;
        let characters = &self.board.characters;
        if characters.len() < 2 {
            empty_state(
                ui,
                "Not enough characters",
                "Add at least two characters to map how they relate.",
            );
            return;
        }
        if self.board.relationships.is_empty() {
            ui.label(
                RichText::new("Select a character and add relationships in the inspector.")
                    .color(colors.muted),
            );
        }

        let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let center = rect.center();
        let ring = (rect.width().min(rect.height()) / 2.0 - NODE_RADIUS - 24.0).max(NODE_RADIUS);
        #[expect(clippy::cast_precision_loss, reason = "character counts are small")]
        let step = TAU / characters.len() as f32;
        let positions: Vec<egui::Pos2> = (0..characters.len())
            .map(|i| {
                #[expect(clippy::cast_precision_loss, reason = "character counts are small")]
                let angle = step * i as f32 - FRAC_PI_2;
                center + ring * Vec2::angled(angle)
            })
            .collect();
        let position_of = |id: &Id| {
            characters
                .iter()
                .position(|c| &c.id == id)
                .map(|i| positions[i])
        };

        let painter = ui.painter_at(rect);
        let font = egui::FontId::proportional(12.0);
        for relationship in &self.board.relationships {
            if let (Some(from), Some(to)) = (
                position_of(&relationship.from),
                position_of(&relationship.to),
            ) {
                draw_relationship_edge(&painter, colors, from, to, relationship.label.trim());
            }
        }

        let mut clicked = None;
        for (character, &position) in characters.iter().zip(&positions) {
            let is_selected = self.selected == Some(Selection::Character(character.id.clone()));
            let node = egui::Rect::from_center_size(position, Vec2::splat(NODE_RADIUS * 2.0));
            let response = ui.interact(
                node,
                egui::Id::unique(("relationship-node", &character.id)),
                Sense::click(),
            );
            let fill = if is_selected || response.hovered() {
                colors.accent
            } else {
                colors.card
            };
            let text = if is_selected || response.hovered() {
                colors.on_accent
            } else {
                colors.ink
            };
            painter.circle(position, NODE_RADIUS, fill, Stroke::new(1.0, colors.line));
            let name = display_name(&character.name);
            let initials: String = name
                .split_whitespace()
                .filter_map(|word| word.chars().next())
                .take(2)
                .collect();
            painter.text(
                position,
                egui::Align2::CENTER_CENTER,
                initials,
                egui::FontId::proportional(15.0),
                text,
            );
            painter.text(
                position + Vec2::new(0.0, NODE_RADIUS + 4.0),
                egui::Align2::CENTER_TOP,
                name,
                font.clone(),
                colors.ink,
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                clicked = Some(character.id.clone());
            }
        }
        if let Some(id) = clicked {
            self.selected = Some(Selection::Character(id));
        }
    }

    fn board_view(&mut self, ui: &mut Ui) {
        let colors = self.colors;
        if self.board.scenes.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.label(RichText::new("No scenes yet").size(20.0).strong());
                ui.label("Click “+ Scene” to pin your first index card.");
            });
            return;
        }

        let indices = self.board.matching_indices(
            &self.query,
            self.filter_status,
            self.filter_act,
            self.filter_pov.as_ref(),
        );

        let dragging = egui::DragAndDrop::payload::<DraggedScene>(ui.ctx()).map(|p| p.0);
        let mut clicked = None;
        let mut moved = None;

        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(16.0);
                if indices.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(RichText::new("No matching scenes").size(16.0).strong());
                        ui.label("Clear the search or filters to show cards.");
                    });
                    return;
                }
                let columns = card_columns(ui.available_width(), ui.spacing().item_spacing.x);
                for row in indices.chunks(columns) {
                    ui.horizontal_top(|ui| {
                        for &index in row {
                            let scene = &self.board.scenes[index];
                            let is_selected =
                                self.selected == Some(Selection::Scene(scene.id.clone()));
                            let is_dragged = dragging == Some(index);
                            let pov = scene.pov.as_ref().and_then(|id| self.board.character(id));
                            let footnote = card_footnote(&self.board, scene);

                            let card = card_frame(colors, colors.card)
                                .stroke(selection_stroke(colors, is_selected))
                                .multiply_with_opacity(if is_dragged { 0.4 } else { 1.0 })
                                .show(ui, |ui| {
                                    scene_card_contents(ui, colors, index, scene, pov, &footnote);
                                });

                            let response = ui.interact(
                                card.response.rect,
                                egui::Id::unique(("scene-card", &scene.id)),
                                Sense::click_and_drag(),
                            );
                            if response.drag_started() {
                                egui::DragAndDrop::set_payload(ui.ctx(), DraggedScene(index));
                            }
                            if response.clicked() {
                                clicked = Some(scene.id.clone());
                            }
                            if let Some(from) = response.dnd_hover_payload::<DraggedScene>()
                                && from.0 != index
                            {
                                ui.painter().rect_stroke(
                                    card.response.rect.expand(4.0),
                                    CornerRadius::same(6),
                                    Stroke::new(2.0, colors.accent),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            if let Some(from) = response.dnd_release_payload::<DraggedScene>() {
                                moved = Some((from.0, index));
                            }
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(if dragging.is_some() {
                                    egui::CursorIcon::Grabbing
                                } else {
                                    egui::CursorIcon::PointingHand
                                });
                            }
                        }
                    });
                }
            });

        if let Some(id) = clicked {
            self.selected = Some(Selection::Scene(id));
        }
        if let Some((from, to)) = moved {
            self.board.move_scene(from, to);
            self.save_now();
        }
    }

    /// True when "Recover from local…" was clicked.
    fn backups_section(&self, ui: &mut Ui) -> bool {
        let backups_label = field_label(ui, self.colors, "Backups");
        ui.label(
            RichText::new(format!(
                "The last {BACKUPS_KEPT} copies are kept in {}",
                storage::backups_dir(&self.path).display()
            ))
            .small(),
        )
        .labelled_by(backups_label);
        ui.button("Recover from local…").clicked()
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &self.settings_dialog else {
            return;
        };
        let save_error = dialog.save_error.clone();
        let colors = self.colors;
        let mut chosen = self.settings.color_scheme;
        let mut close = false;
        let mut board_file_action = None;
        let mut gist_action = None;
        let modal = egui::Modal::new(egui::Id::unique("settings")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("Settings");

            let scheme_label = field_label(ui, colors, "Color scheme");
            ui.horizontal(|ui| {
                scheme_swatch(ui, chosen.palette());
                ComboBox::from_id_salt("color_scheme")
                    .selected_text(chosen.name())
                    .width(ui.available_width())
                    .height(f32::INFINITY)
                    .show_ui(ui, |ui| {
                        for scheme in ColorScheme::ALL {
                            ui.horizontal(|ui| {
                                scheme_swatch(ui, scheme.palette());
                                ui.selectable_value(&mut chosen, scheme, scheme.name());
                            });
                        }
                    })
                    .response
                    .labelled_by(scheme_label);
            });

            let board_file_label = field_label(ui, colors, "Board file");
            ui.label(RichText::new(self.path.display().to_string()).small())
                .labelled_by(board_file_label);
            ui.horizontal(|ui| {
                if ui
                    .button("Open…")
                    .on_hover_text("Switch to another board file")
                    .clicked()
                {
                    board_file_action = Some(BoardFileAction::Open);
                }
                if ui
                    .button("Move…")
                    .on_hover_text("Save this board somewhere else and keep using it there")
                    .clicked()
                {
                    board_file_action = Some(BoardFileAction::Move);
                }
            });
            ui.label(
                RichText::new(
                    "To use the board on several computers, move it into a synced folder \
                     (iCloud Drive, Dropbox, OneDrive, Syncthing) and open it from there on \
                     each one. Changes synced in are picked up automatically.",
                )
                .small()
                .color(colors.muted),
            );

            if self.backups_section(ui) {
                board_file_action = Some(BoardFileAction::RecoverFromLocal);
            }

            gist_action = self.gist_section(ui);

            ui.add_space(6.0);
            match &save_error {
                Some(error) => {
                    ui.label(RichText::new(error).small().color(colors.danger));
                }
                None => {
                    ui.label(
                        RichText::new(format!("Saved to {}", self.settings_path.display()))
                            .small()
                            .color(colors.muted),
                    );
                }
            }

            ui.add_space(8.0);
            close = ui.button("Close").clicked();
        });

        if chosen != self.settings.color_scheme {
            self.set_color_scheme(ctx, chosen);
        }
        match board_file_action {
            Some(BoardFileAction::Open) => self.open_board_file(),
            Some(BoardFileAction::Move) => self.move_board_file(),
            Some(BoardFileAction::RecoverFromLocal) => self.open_local_recovery(),
            None => {}
        }
        if let Some(action) = gist_action {
            self.handle_gist_action(action);
        }
        if (close || modal.should_close())
            && self.gist.restore.is_none()
            && self.local_recovery.is_none()
        {
            self.settings_dialog = None;
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let colors = self.colors;
        if let Some(message) = &self.alert {
            let mut dismissed = false;
            let modal = egui::Modal::new(egui::Id::unique("alert")).show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.label(message);
                ui.add_space(8.0);
                dismissed = ui.button("OK").clicked();
            });
            if dismissed || modal.should_close() {
                self.alert = None;
            }
        }

        let Some(id) = self.scene_to_delete.clone() else {
            return;
        };
        let mut decision = None;
        let modal = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.label("Delete this scene?");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Delete").color(Color32::WHITE))
                            .fill(colors.danger),
                    )
                    .clicked()
                {
                    decision = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    decision = Some(false);
                }
            });
        });
        if modal.should_close() && decision.is_none() {
            decision = Some(false);
        }
        let Some(accepted) = decision else {
            return;
        };
        self.scene_to_delete = None;
        if !accepted {
            return;
        }
        self.board.delete_scene(&id);
        if self.selected == Some(Selection::Scene(id)) {
            self.selected = None;
        }
        self.save_now();
    }
}

impl eframe::App for PlannerApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.flush_if_due(ctx);
        self.reload_if_changed_elsewhere(ctx);
        self.receive_gist_reply(ctx);
        self.start_queued_push();
        self.start_queued_cloud_job();
        self.back_up_to_gist_if_due(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let colors = self.colors;
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        self.drop_stale_pov_filter();

        egui::Panel::top("header")
            .frame(
                Frame::new()
                    .fill(colors.header)
                    .inner_margin(Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| self.header(ui));

        egui::Panel::left("characters")
            .default_size(250.0)
            .frame(Frame::new().fill(colors.sidebar).inner_margin(16))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.filter_panel(ui);
                    ui.add_space(10.0);
                    self.characters_panel(ui);
                    ui.add_space(10.0);
                    self.locations_panel(ui);
                });
            });

        if self.selected.is_some() {
            egui::Panel::right("inspector")
                .default_size(340.0)
                .frame(Frame::new().fill(colors.inspector).inner_margin(16))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.inspector_panel(ui));
                });
        }

        if self.show_stats {
            egui::Panel::bottom("statistics")
                .frame(Frame::new().fill(colors.inspector).inner_margin(12))
                .show(ui, |ui| self.stats_panel(ui));
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(colors.board).inner_margin(22))
            .show(ui, |ui| {
                self.view_tabs(ui);
                match self.view {
                    View::Board => self.board_view(ui),
                    View::Write => self.write_view(ui),
                    View::Timeline => self.timeline_view(ui),
                    View::Relationships => self.relationships_view(ui),
                }
            });

        self.settings_dialog(&ctx);
        self.local_recovery_dialog(&ctx);
        self.gist_restore_dialog(&ctx);
        self.cloud_open_dialog(&ctx);
        self.dialogs(&ctx);
        self.flush_on_focus_change(&ctx);
        self.sync_window_title(&ctx);
    }

    fn on_exit(&mut self) {
        if matches!(self.save_state, SaveState::Pending(_)) {
            self.save_now();
        }
        self.back_up_to_gist_before_exit();
    }
}

fn classification_fields(
    ui: &mut Ui,
    colors: Palette,
    scene: &mut Scene,
    characters: &[Character],
) -> bool {
    let mut changed = false;
    ui.columns(2, |cols| {
        let status_label = field_label(&mut cols[0], colors, "Status");
        ComboBox::from_id_salt("status")
            .selected_text(scene.status.to_string())
            .width(cols[0].available_width())
            .show_ui(&mut cols[0], |ui| {
                for status in Status::ALL {
                    changed |= ui
                        .selectable_value(&mut scene.status, status, status.to_string())
                        .changed();
                }
            })
            .response
            .labelled_by(status_label);

        let pov_label = field_label(&mut cols[1], colors, "POV character");
        let pov_name = scene
            .pov
            .as_ref()
            .and_then(|pov| characters.iter().find(|c| &c.id == pov))
            .map_or("— none —", |c| display_name(&c.name));
        ComboBox::from_id_salt("pov")
            .selected_text(pov_name)
            .width(cols[1].available_width())
            .show_ui(&mut cols[1], |ui| {
                changed |= ui
                    .selectable_value(&mut scene.pov, None, "— none —")
                    .changed();
                for c in characters {
                    changed |= ui
                        .selectable_value(&mut scene.pov, Some(c.id.clone()), display_name(&c.name))
                        .changed();
                }
            })
            .response
            .labelled_by(pov_label);
    });

    let act_label = field_label(ui, colors, "Act");
    let act_text = scene
        .act
        .map_or_else(|| "— none —".to_owned(), |a| a.to_string());
    ComboBox::from_id_salt("act")
        .selected_text(act_text)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(&mut scene.act, None, "— none —")
                .changed();
            for act in Act::ALL {
                changed |= ui
                    .selectable_value(&mut scene.act, Some(act), act.to_string())
                    .changed();
            }
        })
        .response
        .labelled_by(act_label);
    changed
}

fn character_profile_fields(ui: &mut Ui, colors: Palette, character: &mut Character) -> bool {
    let mut changed = false;
    let name_label = field_label(ui, colors, "Name");
    changed |= ui
        .add(TextEdit::singleline(&mut character.name).desired_width(f32::INFINITY))
        .labelled_by(name_label)
        .changed();

    let role_label = field_label(ui, colors, "Role");
    changed |= ui
        .add(
            TextEdit::singleline(&mut character.role)
                .hint_text("Protagonist, mentor, antagonist…")
                .desired_width(f32::INFINITY),
        )
        .labelled_by(role_label)
        .changed();

    let gender_label = field_label(ui, colors, "Gender");
    changed |= ui
        .add(
            TextEdit::singleline(&mut character.gender)
                .hint_text("Woman, man, nonbinary…")
                .desired_width(f32::INFINITY),
        )
        .labelled_by(gender_label)
        .changed();

    let goal_label = field_label(ui, colors, "Goal");
    changed |= ui
        .add(
            TextEdit::multiline(&mut character.goal)
                .hint_text("What they want, and what stands in the way.")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        )
        .labelled_by(goal_label)
        .changed();

    let notes_label = field_label(ui, colors, "Notes");
    changed |= ui
        .add(
            TextEdit::multiline(&mut character.notes)
                .hint_text("Appearance, voice, backstory, arc.")
                .desired_rows(6)
                .desired_width(f32::INFINITY),
        )
        .labelled_by(notes_label)
        .changed();
    changed
}

/// Returns `true` when "Write" was clicked.
fn word_count_row(ui: &mut Ui, colors: Palette, scene: &Scene, offer_write: bool) -> bool {
    let mut write = false;
    ui.horizontal(|ui| {
        let words = scene.word_count();
        ui.label(
            RichText::new(format!("{} word{}", format_count(words), plural(words)))
                .small()
                .color(colors.muted),
        );
        if offer_write {
            write = ui
                .button("Write prose")
                .on_hover_text("Open this scene's prose in the editor")
                .clicked();
        }
    });
    write
}

fn draw_relationship_edge(
    painter: &egui::Painter,
    colors: Palette,
    from: egui::Pos2,
    to: egui::Pos2,
    label: &str,
) {
    let stroke = Stroke::new(1.5, colors.muted);
    let direction = (to - from).normalized();
    // Offsetting sideways keeps A→B and B→A from drawing on top of each other.
    let side = direction.rot90() * 6.0;
    let start = from + direction * NODE_RADIUS + side;
    let tip = to - direction * (NODE_RADIUS + 2.0) + side;
    painter.line_segment([start, tip], stroke);
    for wing in [direction.rot90(), -direction.rot90()] {
        painter.line_segment([tip, tip - direction * 9.0 + wing * 5.0], stroke);
    }
    if label.is_empty() {
        return;
    }
    let galley = painter.layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.0),
        colors.ink,
    );
    let label_rect = egui::Rect::from_center_size(
        start.lerp(tip, EDGE_LABEL_POSITION) + side * 1.5,
        galley.size() + Vec2::new(8.0, 4.0),
    );
    painter.rect_filled(label_rect, 3.0, colors.board);
    painter.galley(label_rect.min + Vec2::new(4.0, 2.0), galley, colors.ink);
}

fn inspector_header(ui: &mut Ui, colors: Palette, kind: &str, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(kind).small().strong().color(colors.muted));
        ui.label(RichText::new(title).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = ui.button("×").on_hover_text("Close").clicked();
        });
    });
    close
}

fn setting_fields(ui: &mut Ui, colors: Palette, scene: &mut Scene, locations: &[Location]) -> bool {
    let mut changed = false;
    ui.columns(2, |cols| {
        let location_label = field_label(&mut cols[0], colors, "Location");
        let location_name = scene
            .location
            .as_ref()
            .and_then(|id| locations.iter().find(|l| &l.id == id))
            .map_or("— none —", |l| display_name(&l.name));
        ComboBox::from_id_salt("location")
            .selected_text(location_name)
            .width(cols[0].available_width())
            .show_ui(&mut cols[0], |ui| {
                changed |= ui
                    .selectable_value(&mut scene.location, None, "— none —")
                    .changed();
                for l in locations {
                    changed |= ui
                        .selectable_value(
                            &mut scene.location,
                            Some(l.id.clone()),
                            display_name(&l.name),
                        )
                        .changed();
                }
            })
            .response
            .labelled_by(location_label);

        let day_label = field_label(&mut cols[1], colors, "Story day");
        cols[1].horizontal(|ui| {
            let mut dated = scene.day.is_some();
            if ui
                .checkbox(&mut dated, "")
                .on_hover_text("Place this scene on the timeline")
                .labelled_by(day_label)
                .changed()
            {
                scene.day = dated.then_some(1);
                changed = true;
            }
            if let Some(day) = &mut scene.day {
                changed |= ui
                    .add(egui::DragValue::new(day).prefix("Day "))
                    .labelled_by(day_label)
                    .changed();
            }
        });
    });
    changed
}

fn scene_card_contents(
    ui: &mut Ui,
    colors: Palette,
    index: usize,
    scene: &Scene,
    pov: Option<&Character>,
    footnote: &str,
) {
    // The card sits in a horizontal row and would otherwise inherit its
    // layout, putting the title, summary and badges side by side.
    ui.vertical(|ui| {
        ui.set_width(CARD_WIDTH);
        ui.set_min_height(90.0);
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
        ui.label(
            RichText::new(scene_number(index))
                .small()
                .strong()
                .color(colors.muted),
        );
        if scene.title.is_empty() {
            ui.label(RichText::new("Untitled scene").italics());
        } else {
            ui.label(RichText::new(&scene.title).strong());
        }
        let summary = if scene.summary.is_empty() {
            "No summary yet."
        } else {
            &scene.summary
        };
        ui.label(RichText::new(preview(summary)).small().color(colors.muted));
        ui.horizontal_wrapped(|ui| {
            badge(
                ui,
                &scene.status.to_string(),
                colors.status(scene.status),
                colors.ink,
            );
            if let Some(act) = scene.act {
                badge(ui, &act.to_string(), colors.act, colors.ink);
            }
            if let Some(c) = pov {
                badge(ui, display_name(&c.name), colors.header, colors.header_text);
            }
        });
        if !footnote.is_empty() {
            ui.label(RichText::new(footnote).small().color(colors.muted));
        }
    });
}

/// Location, story day and word count, the parts that are set.
fn card_footnote(board: &Board, scene: &Scene) -> String {
    let mut parts = Vec::new();
    if let Some(location) = scene.location.as_ref().and_then(|id| board.location(id)) {
        parts.push(display_name(&location.name).to_owned());
    }
    if let Some(day) = scene.day {
        parts.push(format!("Day {day}"));
    }
    let words = scene.word_count();
    if words > 0 {
        parts.push(format!("{} word{}", format_count(words), plural(words)));
    }
    parts.join(" · ")
}

/// Status, POV and location for one-line scene listings.
fn scene_details(board: &Board, scene: &Scene) -> String {
    let mut parts = vec![scene.status.to_string()];
    if let Some(pov) = scene.pov.as_ref().and_then(|id| board.character(id)) {
        parts.push(format!("POV {}", display_name(&pov.name)));
    }
    if let Some(location) = scene.location.as_ref().and_then(|id| board.location(id)) {
        parts.push(display_name(&location.name).to_owned());
    }
    parts.join(" · ")
}

fn scene_title(scene: &Scene) -> &str {
    if scene.title.trim().is_empty() {
        "Untitled"
    } else {
        &scene.title
    }
}

fn empty_state(ui: &mut Ui, heading: &str, hint: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        ui.label(RichText::new(heading).size(20.0).strong());
        ui.label(hint);
    });
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn format_count(count: usize) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn selection_stroke(colors: Palette, selected: bool) -> Stroke {
    if selected {
        Stroke::new(2.0, colors.accent)
    } else {
        Stroke::new(1.0, colors.line)
    }
}

fn display_name(name: &str) -> &str {
    if name.trim().is_empty() {
        "Unnamed"
    } else {
        name
    }
}

fn apply_theme(ctx: &egui::Context, colors: Palette) {
    let (theme, base) = if colors.dark {
        (egui::Theme::Dark, egui::Visuals::dark())
    } else {
        (egui::Theme::Light, egui::Visuals::light())
    };
    ctx.set_theme(theme);
    // eframe may switch to the OS light/dark theme after startup, so both
    // styles carry the palette rather than just the one requested here.
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |style| apply_palette(style, base.clone(), colors));
    }
}

fn apply_palette(style: &mut egui::Style, base: egui::Visuals, colors: Palette) {
    style.visuals = base;
    style.visuals.override_text_color = Some(colors.ink);
    style.visuals.extreme_bg_color = colors.field_background();
    style.visuals.window_fill = colors.inspector;
    style.visuals.window_stroke = Stroke::new(1.0, colors.line);
    style.visuals.panel_fill = colors.sidebar;
    style.visuals.selection.bg_fill = colors.act;
    style.visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
}

fn scheme_swatch(ui: &mut Ui, colors: Palette) {
    const SIZE: Vec2 = Vec2::new(10.0, 14.0);
    let fills = [colors.header, colors.board, colors.card, colors.accent];
    #[expect(clippy::cast_precision_loss, reason = "four swatches")]
    let width = SIZE.x * fills.len() as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SIZE.y), Sense::hover());
    for (i, fill) in fills.into_iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "four swatches")]
        let min = rect.min + Vec2::new(SIZE.x * i as f32, 0.0);
        ui.painter()
            .rect_filled(egui::Rect::from_min_size(min, SIZE), 0.0, fill);
    }
    ui.painter().rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, colors.line),
        egui::StrokeKind::Inside,
    );
}

fn card_frame(colors: Palette, fill: Color32) -> Frame {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, colors.line))
        .corner_radius(4)
        .inner_margin(Margin::same(CARD_MARGIN))
        .shadow(egui::Shadow {
            offset: [0, 3],
            blur: 6,
            spread: 0,
            color: Color32::from_black_alpha(16),
        })
}

fn badge(ui: &mut Ui, text: &str, fill: Color32, color: Color32) {
    Frame::new()
        .fill(fill)
        .corner_radius(10)
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text.to_uppercase())
                    .size(10.0)
                    .strong()
                    .color(color),
            );
        });
}

fn field_label(ui: &mut Ui, colors: Palette, text: &str) -> egui::Id {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(colors.muted),
    )
    .id
}

fn filter_label(colors: Palette, text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .size(11.0)
        .strong()
        .color(colors.muted)
}

/// egui's wrapping layouts cannot wrap whole frames, so rows are built by hand.
fn card_columns(available_width: f32, spacing: f32) -> usize {
    let fitting = ((available_width + spacing) / (CARD_OUTER_WIDTH + spacing)).floor();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "floored, clamped to at least one, and bounded by screen width"
    )]
    let columns = fitting.max(1.0) as usize;
    columns
}

fn window_title(novel_title: &str, save_failed: bool) -> String {
    let novel_title = novel_title.trim();
    let novel_title = if novel_title.is_empty() {
        "Untitled Novel"
    } else {
        novel_title
    };
    let suffix = if save_failed { " (save failed)" } else { "" };
    format!("{novel_title} — StorySquish{suffix}")
}

fn elapsed_description(elapsed: Duration) -> String {
    const HOUR: u64 = 60;
    const TWO_HOURS: u64 = 2 * HOUR;
    const DAY: u64 = 24 * HOUR;
    const TWO_DAYS: u64 = 2 * DAY;
    match elapsed.as_secs() / 60 {
        0 => "just now".to_owned(),
        1 => "a minute ago".to_owned(),
        minutes @ 2..HOUR => format!("{minutes} minutes ago"),
        HOUR..TWO_HOURS => "an hour ago".to_owned(),
        minutes @ TWO_HOURS..DAY => format!("{} hours ago", minutes / HOUR),
        DAY..TWO_DAYS => "a day ago".to_owned(),
        minutes => format!("{} days ago", minutes / DAY),
    }
}

fn local_backup_label(backup: &storage::LocalBackup, now: SystemTime) -> String {
    let age = backup.saved_at.map_or_else(
        || "unknown time".to_owned(),
        |at| elapsed_description(now.duration_since(at).unwrap_or_default()),
    );
    let contents = match &backup.contents {
        None => "unreadable".to_owned(),
        Some(contents) => {
            let title = display_cloud_title(contents.title.trim());
            match contents.scene_count {
                1 => format!("{title}  ·  1 scene"),
                count => format!("{title}  ·  {count} scenes"),
            }
        }
    };
    let origin = match backup.tag.as_str() {
        "" => String::new(),
        "conflict" => "  ·  changed elsewhere".to_owned(),
        tag => format!("  ·  {tag}"),
    };
    format!("{age}  ·  {contents}{origin}")
}

/// GitHub's `2026-11-22T09:41:07Z` as `2026-11-22 09:41 UTC`.
fn display_timestamp(iso: &str) -> String {
    match (iso.get(..10), iso.get(11..16)) {
        (Some(date), Some(time)) if iso.ends_with('Z') => format!("{date} {time} UTC"),
        _ => iso.to_owned(),
    }
}

fn is_cloud_task(task: GistTask) -> bool {
    matches!(task, GistTask::ListCloudBoards | GistTask::OpenCloudBoard)
}

fn display_cloud_title(title: &str) -> &str {
    if title.is_empty() { "Untitled" } else { title }
}

fn short_version(version: &str) -> &str {
    version.get(..7).unwrap_or(version)
}

fn scene_number(index: usize) -> String {
    format!("SCENE {:02}", index + 1)
}

fn preview(summary: &str) -> String {
    let mut chars = summary.chars();
    let head: String = chars.by_ref().take(SUMMARY_PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        format!("{}…", head.trim_end())
    } else {
        head
    }
}

fn gist_push_wait(since_last_edit: Option<Duration>, push_now: bool) -> Option<Duration> {
    if push_now {
        return None;
    }
    since_last_edit
        .filter(|&elapsed| elapsed < GIST_PUSH_DELAY)
        .map(|elapsed| GIST_PUSH_DELAY.saturating_sub(elapsed))
}

fn boards_other_than<'a>(
    boards: &'a [CloudBoard],
    open_gist_id: Option<&str>,
) -> Vec<&'a CloudBoard> {
    boards
        .iter()
        .filter(|board| open_gist_id != Some(board.gist_id.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_picker_leaves_out_the_open_board() {
        let board = |gist_id: &str| CloudBoard {
            gist_id: gist_id.to_owned(),
            title: gist_id.to_owned(),
            updated_at: String::new(),
        };
        let boards = [board("dune"), board("emma")];
        let ids = |open| -> Vec<&str> {
            boards_other_than(&boards, open)
                .into_iter()
                .map(|b| b.gist_id.as_str())
                .collect()
        };
        assert_eq!(ids(Some("dune")), ["emma"]);
        assert_eq!(ids(None), ["dune", "emma"]);
    }

    #[test]
    fn gist_push_waits_for_a_pause_unless_pushed_now() {
        let recent = Some(Duration::from_secs(1));
        assert_eq!(gist_push_wait(recent, false), Some(Duration::from_secs(4)));
        assert_eq!(gist_push_wait(recent, true), None);
        assert_eq!(gist_push_wait(None, false), None);
        assert_eq!(gist_push_wait(Some(GIST_PUSH_DELAY), false), None);
    }

    #[test]
    fn preview_truncates_on_char_boundary() {
        let long = "é".repeat(SUMMARY_PREVIEW_CHARS + 5);
        let p = preview(&long);
        assert_eq!(p.chars().count(), SUMMARY_PREVIEW_CHARS + 1);
        assert!(p.ends_with('…'));
        assert_eq!(preview("short"), "short");
    }

    #[test]
    fn card_columns_fill_width_and_never_drop_to_zero() {
        let step = CARD_OUTER_WIDTH + 16.0;
        assert_eq!(card_columns(0.0, 16.0), 1);
        assert_eq!(card_columns(CARD_OUTER_WIDTH, 16.0), 1);
        assert_eq!(card_columns(3.0 * step - 16.0, 16.0), 3);
        assert_eq!(card_columns(3.0 * step - 17.0, 16.0), 2);
    }

    #[test]
    fn window_title_names_novel_and_flags_failed_save() {
        assert_eq!(window_title("  Dune ", false), "Dune — StorySquish");
        assert_eq!(window_title("", false), "Untitled Novel — StorySquish");
        assert_eq!(
            window_title("Dune", true),
            "Dune — StorySquish (save failed)"
        );
    }

    #[test]
    fn counts_get_thousands_separators() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1,000");
        assert_eq!(format_count(1_234_567), "1,234,567");
    }

    #[test]
    fn gist_timestamps_and_versions_are_shortened() {
        assert_eq!(
            display_timestamp("2026-11-22T09:41:07Z"),
            "2026-11-22 09:41 UTC"
        );
        assert_eq!(display_timestamp("yesterday"), "yesterday");
        assert_eq!(
            short_version("57a7f021a713b1c5a6a199b54cc514735d2d462f"),
            "57a7f02"
        );
        assert_eq!(short_version("abc"), "abc");
    }

    #[test]
    fn backup_age_reads_naturally() {
        assert_eq!(elapsed_description(Duration::from_secs(20)), "just now");
        assert_eq!(elapsed_description(Duration::from_secs(90)), "a minute ago");
        assert_eq!(
            elapsed_description(Duration::from_mins(12)),
            "12 minutes ago"
        );
        assert_eq!(elapsed_description(Duration::from_mins(75)), "an hour ago");
        assert_eq!(elapsed_description(Duration::from_hours(5)), "5 hours ago");
        assert_eq!(elapsed_description(Duration::from_hours(30)), "a day ago");
        assert_eq!(elapsed_description(Duration::from_hours(80)), "3 days ago");
    }

    #[test]
    fn local_backup_label_names_age_contents_and_origin() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_hours(100);
        let contents = |title: &str, scene_count| {
            Some(storage::BackupContents {
                title: title.to_owned(),
                scene_count,
            })
        };
        let backup = |hours_ago, tag: &str, contents| storage::LocalBackup {
            path: PathBuf::from("board-backups/x.json"),
            saved_at: Some(now - Duration::from_hours(hours_ago)),
            tag: tag.to_owned(),
            contents,
        };
        assert_eq!(
            local_backup_label(&backup(2, "", contents("Dune", 1)), now),
            "2 hours ago  ·  Dune  ·  1 scene"
        );
        assert_eq!(
            local_backup_label(&backup(5, "conflict", contents("  ", 3)), now),
            "5 hours ago  ·  Untitled  ·  3 scenes  ·  changed elsewhere"
        );
        assert_eq!(
            local_backup_label(&backup(0, "", None), now),
            "just now  ·  unreadable"
        );
    }

    #[test]
    fn scene_numbers_are_one_based_and_padded() {
        assert_eq!(scene_number(0), "SCENE 01");
        assert_eq!(scene_number(11), "SCENE 12");
    }
}
