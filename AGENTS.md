# AGENTS

Guidelines for agents working in this repo. The global rules at
`~/.local/dotfiles/agents/AGENTS.md` still apply and take precedence where the
two disagree.

## What this is

StorySquish, a Rust desktop scene board for planning and drafting novels
(`egui`/`eframe`). The browser version in [`docs/index.html`](docs/index.html)
shares the board's JSON format, so boards move between them through Export and
Import rather than by code sharing.

## Language and tooling

- Rust, `edition = 2024` (Rust 1.85+). Do not assume a lower edition's
  conventions; check the pinned version with `make rust-version` if unsure.
- Do not bump dependencies or edition without saying why. The repo pins every
  version in `Cargo.toml`; a bump belongs in its own commit.
- Formatting and linting:

  ```sh
  make format        # cargo fmt --all
  make lint          # cargo clippy --all-targets -D warnings
  make test          # cargo test --all-features
  make check         # fmt + clippy + test, run before opening a PR
  ```

- The crate forbids `unsafe` and runs Clippy with both `all` and
  `pedantic` at warn level. Keep new code warning-free under both; the same set
  runs in CI and blocks PRs. Three lints are allowed crate-wide —
  `module_name_repetitions`, `must_use_candidate`, `missing_errors_doc` — so
  don't add doc comments or `#[must_use]` just to silence the rest of them.

## Safety and conventions

- Conventional Commits (`feat:`, `fix:`, `docs:`, ...), imperative mood, single
  line preferred (max ~72 chars). Never commit unless asked.
- Prefer renaming or extracting a function over adding comments; comment only a
  non-obvious *why*.

## Layout

- [`src/model.rs`](src/model.rs) — board, scenes, characters, locations,
  relationships; the JSON schema shared with the web version. The source of
  truth for board-file compatibility, so check here before changing storage.
- [`src/app.rs`](src/app.rs) — egui UI, shortcuts, dialogs. The app is driven
  here; most behavior changes touch this and `model`.
- [`src/history.rs`](src/history.rs) — snapshot-based undo/redo.
- [`src/storage.rs`](src/storage.rs) — load/save, atomic writes (temp file +
  rename), backups, export/outline names. Writes must stay crash-safe; never
  write the board in place.
- [`src/settings.rs`](src/settings.rs), [`src/gist.rs`](src/gist.rs) — settings
  and the optional GitHub Gist backup (token in a separate `0600` file).
- [`src/outline.rs`](src/outline.rs) — Markdown outline and manuscript export.
- [`src/theme.rs`](src/theme.rs), [`src/migration.rs`](src/migration.rs) — color
  schemes and pre-rename data migration.

## Tests

- [`tests/ui.rs`](tests/ui.rs) drives the app headlessly with `egui_kittest`.
  Add UI tests for interaction changes; they run in CI. [`tests/gist.rs`](tests/gist.rs)
  covers the backup client. Prefer a `check` run (which includes these) over a
  manual build before proposing changes.
