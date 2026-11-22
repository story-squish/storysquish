# StorySquish

A desktop scene board for planning and drafting novels. Lay out scenes as
cards, track their status, act, point-of-view character, location and story
day, write each scene's prose, and keep characters, relationships and
locations alongside. Built in Rust with [egui/eframe](https://github.com/emilk/egui).

A browser version lives in [`docs/index.html`](docs/index.html) and is
published at <https://story-squish.github.io/storysquish/>. It has the same
features and JSON format, so boards move between the two through Export and
Import, or by pointing both at the same file.

The web version keeps its board in the browser's `localStorage`, the last 30
versions in IndexedDB (Settings → Restore backup…), and asks the browser for
persistent storage. In Chrome and Edge on desktop, Settings → Save to file… /
Open file… links the board to a JSON file, for example in a synced folder:
every change is written to it and changes synced in are picked up, with the
same conflict handling as the desktop app. Browsers drop file permission
between visits, so a "Reconnect file" button appears until clicked. Without a
linked file, a "Back up" button appears when the board has not been exported
for a week.

## Quick start

```sh
cargo run -r                      # open the default board
cargo run -r -- path/to/board.json # open a specific board file
make install                      # release build, stripped, copied to ~/.local/bin
```

On Linux, eframe needs these system libraries:

```sh
sudo apt-get install libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
```

## Features

- Scene cards with title, summary, status (Outline, Draft, Revision, Final,
  Archived), act (I–III), POV character, location and story day
- **Write** view: a prose editor per scene with previous/next navigation and
  scene and manuscript word counts
- **Timeline** view: scenes in story-day order, so flashbacks and parallel
  threads show up next to their place in the story's chronology
- **Relationships** view: a graph of directed, labelled relationships between
  characters (added in the character inspector)
- Characters with role, goal and notes; locations with notes
- Reorder scenes by drag or keyboard, characters and locations by keyboard or
  buttons
- Search (including prose) plus filters by status, act and POV
- Undo/redo across all board edits
- Autosave after every change, rolling backups, and pick-up of changes synced
  in from other devices (see [Files](#files))
- Import/export the board as JSON; export an outline or the manuscript as
  Markdown (archived scenes are left out; outline scenes keep their board
  numbers)
- Ten color schemes, light and dark

## Keyboard shortcuts

`Cmd` is `Ctrl` on Linux and Windows.

| Shortcut                     | Action                         |
| ---------------------------- | ------------------------------ |
| `Cmd+N`                      | New scene                      |
| `Cmd+D`                      | Duplicate selected scene       |
| `Cmd+F`                      | Focus search                   |
| `Cmd+Z` / `Cmd+Shift+Z`      | Undo / redo                    |
| `Alt+←` / `Alt+→`            | Move selected scene            |
| `Alt+↑` / `Alt+↓`            | Move selected character or location |
| `Delete` / `Backspace`       | Delete selected scene          |
| `Esc`                        | Close inspector                |

Undo, redo, move, delete and `Esc` act on the board only when no text field
has focus, so they keep their usual meaning while typing.

## Files

| What     | Location                                                        |
| -------- | --------------------------------------------------------------- |
| Board    | The path given on the command line, else the one chosen in Settings, else `board.json` in the platform data dir (below) |
| Backups  | `<board>-backups/` next to the board file, e.g. `board-backups/` |
| Settings | `$XDG_CONFIG_HOME/storysquish/settings.json`, falling back to `~/.config/storysquish/settings.json` on every platform |

Platform data dir:

- macOS: `~/Library/Application Support/storysquish/`
- Linux: `~/.local/share/storysquish/`
- Windows: `%APPDATA%\storysquish\data\`

Writes go through a temp file and rename, so a crash never leaves a
half-written board. Before the first save of a session, and every 30 minutes
of editing after that, the board file is copied into the backups directory;
the newest 30 copies are kept. **Settings → Restore backup…** loads one
(undoably).

To use a board on several computers, use **Settings → Move…** to put it in a
folder synced by iCloud Drive, Dropbox, OneDrive or Syncthing, and
**Settings → Open…** to pick it on the other machines; the choice is
remembered. The app checks the file every two seconds and reloads it when it
changed elsewhere and there are no unsaved edits (the reload can be undone).
If it changed while there were unsaved edits, the local version is saved and
the other one is kept as a `-conflict` backup. If the board can't be parsed at startup, it is moved to
`board.json.corrupt` and the app starts with an empty board instead of
overwriting it on the next autosave.

Data and config left by the app's earlier names (`story-planner`,
`novel-planner`) are moved over on first launch, unless a `storysquish`
directory already exists.

## Development

```sh
make check   # cargo fmt --check, clippy -D warnings, cargo test
make format
make lint
make test
```

CI runs the same three checks on every pull request. Clippy `pedantic` is
enabled and `unsafe` code is forbidden. UI tests in [`tests/ui.rs`](tests/ui.rs)
drive the app headlessly with `egui_kittest`.

### Layout

| Module                                   | Responsibility                                   |
| ---------------------------------------- | ------------------------------------------------ |
| [`model`](src/model.rs)                  | Board, scenes, characters, locations, relationships; JSON schema |
| [`app`](src/app.rs)                      | egui UI, shortcuts, dialogs                      |
| [`history`](src/history.rs)              | Snapshot-based undo/redo                         |
| [`storage`](src/storage.rs)              | Board load/save, atomic writes, backups, export names |
| [`settings`](src/settings.rs)            | Color scheme and board location persistence      |
| [`outline`](src/outline.rs)              | Markdown outline and manuscript export           |
| [`theme`](src/theme.rs)                  | Color schemes and palettes                       |
| [`migration`](src/migration.rs)          | Move data from pre-rename directories            |
