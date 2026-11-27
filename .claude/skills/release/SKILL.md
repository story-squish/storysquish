---
name: release
description: Cut a StorySquish release end to end — bump the version in Cargo.toml through a PR, merge it, publish the GitHub release that builds the macOS .pkg and Windows .zip, then update the storysquish cask in meslab/homebrew-tap with the new version and sha256. Use whenever the user asks to release, ship, tag, publish, "bump and release", do a patch/minor/major bump, or update meslab/tap / the Homebrew cask for StorySquish, even if they only name one of those steps.
---

# Release StorySquish and update meslab/tap

The release is a chain of PRs and GitHub actions that other people see, so
each outward step (push, PR, merge, release, tap PR) happens only because the
user asked for a release. If they asked for only part of it (say, "bump the
version"), stop at that part.

Use the scratchpad directory for anything temporary (downloaded `.pkg`, tap
clone), and delete the tap clone at the end.

## 1. Pre-flight

```sh
git checkout main && git pull
git status --short          # must be clean
gh release list -L 3        # latest released version
gh pr list --state open     # unmerged work the user may expect in the release
```

Pending feature PRs go first: wait for CI, request review, squash-merge. A
release only ships what is on `main`.

## 2. Pick the version

Semver from what landed since the last tag (`git log v<last>..main --oneline`):

- patch — fixes only (`fix:`), e.g. 0.3.1 → 0.3.2
- minor — new features or behavior a user notices on upgrade (`feat:`, moving
  user files, new settings), e.g. 0.3.2 → 0.4.0
- major — only when the user says so

If the user named the bump, use it. If the commits make it ambiguous, say which
you picked and why.

## 3. Bump PR

```sh
V=0.4.0
git checkout -b chore/bump-$V
sed -i '' "s/^version = \".*\"/version = \"$V\"/" Cargo.toml   # only [package] has a line-leading version
cargo check -q                                                # refreshes Cargo.lock
git diff --stat                                               # Cargo.toml + Cargo.lock only
make check
git commit -am "chore: bump version to $V"
git push -u origin chore/bump-$V
gh pr create --title "chore: bump version to $V" --body "Minor release for #68."
```

The body is one line naming the PRs being released, e.g. `Bugfix release for
#66.` Don't change dependencies in this PR; a bump of a dependency belongs in
its own commit.

## 4. CI and review

```sh
gh pr checks <pr> --watch --interval 20
```

`main` requires an approving review, and auto-merge is disabled on the repo, so:

- Request the reviewer who approved recent PRs
  (`gh pr view <recent-pr> --json reviews`); it has been `dezerdigah-duo`.
- Wait for approval in the background rather than polling in the foreground:

  ```sh
  until [ "$(gh pr view <pr> --json reviewDecision -q .reviewDecision)" = APPROVED ]; do sleep 30; done
  ```

- Never merge with `--admin` to skip review unless the user explicitly says so.

If CI fails on a version-only change, read the log before re-running
(`gh run view <run> --log-failed`). A version bump can't break tests, so a
failure there is a flake or a problem already on `main`; re-run once with
`gh run rerun <run> --failed`, and if it recurs, report it rather than looping.
egui_kittest's `Harness::run exceeded max_steps` used to be caused by
background spellcheck loading; that is fixed by `finish_startup` in
`tests/ui.rs`, so a new occurrence is a real regression worth reporting.

## 5. Merge and release

```sh
gh pr merge <pr> --squash --delete-branch
git checkout main && git pull
grep '^version' Cargo.toml            # must show $V
gh release create v$V --target main --title v$V --generate-notes
```

Publishing the release triggers `.github/workflows/release.yml`, which first
checks that `Cargo.toml` matches the tag, then builds and attaches
`StorySquish-v$V-macos-universal.pkg` and `StorySquish-v$V-windows-x86_64.zip`.
Wait for it, then confirm both assets exist:

```sh
gh run list -w release.yml -L 1 --json databaseId -q '.[0].databaseId'
gh run watch <run-id> --exit-status --interval 30
gh release view v$V --json assets -q '.assets[].name'
```

The tap update needs the `.pkg`, so don't start it until it is attached.

## 6. Update meslab/homebrew-tap

The cask is `Casks/storysquish.rb` in `meslab/homebrew-tap`. Its `url` is
built from `version`, so only `version` and `sha256` change. Compute the hash
from the released asset itself, never from a local build:

```sh
cd <scratchpad>
gh release download v$V -R story-squish/storysquish -p '*.pkg' -D pkg --clobber
SHA=$(shasum -a 256 pkg/*.pkg | cut -d' ' -f1)
gh repo clone meslab/homebrew-tap tap -- -q && cd tap
git checkout -b storysquish-$V
sed -i '' -e "s/^  version \".*\"/  version \"$V\"/" \
          -e "s/^  sha256 \".*\"/  sha256 \"$SHA\"/" Casks/storysquish.rb
git diff                               # exactly two lines changed
brew style Casks/storysquish.rb        # use --fix for autocorrectable offenses
git commit -am "chore: bump storysquish to $V"
git push -u origin storysquish-$V
gh pr create -R meslab/homebrew-tap --title "chore: bump storysquish to $V" \
  --body "Update storysquish cask to $V."
gh pr checks <tap-pr> -R meslab/homebrew-tap --watch --interval 20   # test-bot on macOS + Linux
gh pr merge <tap-pr> -R meslab/homebrew-tap --squash --delete-branch
```

Verify the merged cask, then remove the clone and the downloaded package:

```sh
gh api repos/meslab/homebrew-tap/contents/Casks/storysquish.rb --jq .content | base64 -d | head -3
rm -rf <scratchpad>/tap <scratchpad>/pkg
```

### Cask rules

- `zap` lists only the app's own files: `~/.config/storysquish`,
  `~/Library/Application Support/storysquish`,
  `~/Library/Preferences/io.github.storysquish.plist`. Boards
  (`~/Documents/StorySquish`, wherever else the user keeps them, and their
  `*-backups`) are the writer's manuscripts; `brew uninstall --zap` must never
  touch them, even when a release moves where boards live.
- `brew style` requires `zap` arrays in alphabetical order.
- If a release changes where the app stores its own data, update `zap` in the
  same tap PR and say so in the PR body.

## 7. Report

Finish with a short status list: each PR (number, merged or waiting on what),
the release URL and its assets, the tap PR, and anything left undone or flaky.
If you stopped waiting on a review, say so plainly — the release is not done
until the tap PR is merged.
