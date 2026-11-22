# StorySquish

A desktop scene board for planning novels. Lay out scenes as cards, track
their status, act and point-of-view character, and keep character notes
alongside. Built in Rust with [egui/eframe](https://github.com/emilk/egui).

A browser version lives in [`docs/index.html`](docs/index.html) and is
published at <https://story-squish.github.io/storysquish/>. It stores its board
in the browser's `localStorage` and uses the same JSON format, so boards move
between the two through Export and Import.

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
  Archived), act (I–III) and POV character
- Character list with notes
- Reorder scenes by drag or keyboard, characters by keyboard or ↑/↓ buttons
- Search plus filters by status, act and POV
- Undo/redo across all board edits
- Autosave after every change
- Import/export the board as JSON, export an outline as Markdown
  (archived scenes are left out; the rest keep their board numbers)
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
| `Alt+↑` / `Alt+↓`            | Move selected character        |
| `Delete` / `Backspace`       | Delete selected scene          |
| `Esc`                        | Close inspector                |

Undo, redo, move, delete and `Esc` act on the board only when no text field
has focus, so they keep their usual meaning while typing.

## Files

| What     | Location                                                        |
| -------- | --------------------------------------------------------------- |
| Board    | `board.json` in the platform data dir (below), or the path given on the command line |
| Settings | `$XDG_CONFIG_HOME/storysquish/settings.json`, falling back to `~/.config/storysquish/settings.json` on every platform |

Platform data dir:

- macOS: `~/Library/Application Support/storysquish/`
- Linux: `~/.local/share/storysquish/`
- Windows: `%APPDATA%\storysquish\data\`

Writes go through a temp file and rename, so a crash never leaves a
half-written board. If the board can't be parsed at startup, it is moved to
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
| [`model`](src/model.rs)                  | Board, scenes, characters; JSON schema           |
| [`app`](src/app.rs)                      | egui UI, shortcuts, dialogs                      |
| [`history`](src/history.rs)              | Snapshot-based undo/redo                         |
| [`storage`](src/storage.rs)              | Board load/save, atomic writes, export names     |
| [`settings`](src/settings.rs)            | Color scheme persistence                         |
| [`outline`](src/outline.rs)              | Markdown outline export                          |
| [`theme`](src/theme.rs)                  | Color schemes and palettes                       |
| [`migration`](src/migration.rs)          | Move data from pre-rename directories            |
