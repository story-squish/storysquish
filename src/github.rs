use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde_json::json;
use ureq::Agent;

pub const API_BASE: &str = "https://api.github.com";
pub const BOARD_FILE_NAME: &str = "storysquish-board.json";
pub const TOKEN_FILE_NAME: &str = "github-token";
const REPO_NAME_PREFIX: &str = "storysquish-";
const UNTITLED_REPO_NAME: &str = "storysquish-board";
const MAX_REPO_NAME_LEN: usize = 80;
const MAX_REPO_NAME_ATTEMPTS: usize = 20;
const UNTITLED_DESCRIPTION: &str = "StorySquish board";
const TITLED_DESCRIPTION_PREFIX: &str = "StorySquish board: ";
const REVISIONS_LISTED: usize = 30;
const REPOS_PER_PAGE: usize = 100;
const MAX_REPO_PAGES: usize = 10;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: u64 = 100 * 1024 * 1024;
const RAW_MEDIA_TYPE: &str = "application/vnd.github.raw+json";
const JSON_MEDIA_TYPE: &str = "application/vnd.github+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub version: String,
    pub committed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudBoard {
    pub repo: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    pub repo: String,
}

#[derive(Deserialize)]
struct RepoResponse {
    full_name: String,
    private: bool,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    pushed_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

#[derive(Deserialize)]
struct ContentResponse {
    sha: String,
}

#[derive(Deserialize)]
struct CommitResponse {
    sha: String,
    commit: CommitDetail,
}

#[derive(Deserialize)]
struct CommitDetail {
    committer: CommitPerson,
}

#[derive(Deserialize)]
struct CommitPerson {
    date: String,
}

#[derive(Clone)]
pub struct Client {
    agent: Agent,
    api_base: String,
    token: String,
}

impl Client {
    pub fn new(token: &str) -> Self {
        Self::with_api_base(token, API_BASE)
    }

    pub fn with_api_base(token: &str, api_base: &str) -> Self {
        let agent = Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            agent,
            api_base: api_base.trim_end_matches('/').to_owned(),
            token: token.trim().to_owned(),
        }
    }

    /// Creates a private repository when `repo` is `None`, otherwise commits
    /// a new version to the existing one, which must be private.
    pub fn push(&self, repo: Option<&str>, title: &str, board_json: &str) -> Result<Pushed> {
        let repo = match repo {
            None => self.create_repo(title)?,
            Some(repo) => {
                let existing = self.repo_info(repo)?;
                if !existing.private {
                    bail!("{repo} is public; StorySquish backs up only to private repositories");
                }
                let description = repo_description(title);
                if existing.description.as_deref() != Some(description.as_str()) {
                    self.set_description(repo, &description)?;
                }
                existing.full_name
            }
        };
        let mut body = json!({
            "message": commit_message(title),
            "content": BASE64.encode(board_json),
        });
        if let Some(sha) = self.board_file_sha(&repo)? {
            body["sha"] = json!(sha);
        }
        let response = self
            .authorized(self.agent.put(self.board_file_url(&repo)))
            .send_json(&body)
            .context("contacting GitHub")?;
        read_text(response)?;
        Ok(Pushed { repo })
    }

    fn create_repo(&self, title: &str) -> Result<String> {
        let base_name = repo_name_for(title);
        for attempt in 1..=MAX_REPO_NAME_ATTEMPTS {
            let name = if attempt == 1 {
                base_name.clone()
            } else {
                format!("{base_name}-{attempt}")
            };
            let response = self
                .authorized(self.agent.post(format!("{}/user/repos", self.api_base)))
                .send_json(json!({
                    "name": name,
                    "description": repo_description(title),
                    "private": true,
                    "has_issues": false,
                    "has_projects": false,
                    "has_wiki": false,
                }))
                .context("contacting GitHub")?;
            let status = response.status().as_u16();
            let text = read_body(response)?;
            if status == 422 && text.contains("name already exists") {
                continue;
            }
            if !(200..300).contains(&status) {
                bail!("{}", describe_failure(status, &text));
            }
            let created: RepoResponse =
                serde_json::from_str(&text).context("GitHub sent an unexpected response")?;
            return Ok(created.full_name);
        }
        bail!("every repository name tried for this board, from {base_name}, is taken")
    }

    fn repo_info(&self, repo: &str) -> Result<RepoResponse> {
        let response = self
            .authorized(self.agent.get(format!("{}/repos/{repo}", self.api_base)))
            .call()
            .context("contacting GitHub")?;
        read_json(response)
    }

    fn set_description(&self, repo: &str, description: &str) -> Result<()> {
        let response = self
            .authorized(self.agent.patch(format!("{}/repos/{repo}", self.api_base)))
            .send_json(json!({ "description": description }))
            .context("contacting GitHub")?;
        read_text(response).map(drop)
    }

    /// `None` while the repository does not hold the board yet, including
    /// when it has no commits at all.
    fn board_file_sha(&self, repo: &str) -> Result<Option<String>> {
        let response = self
            .authorized(self.agent.get(self.board_file_url(repo)))
            .call()
            .context("contacting GitHub")?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        let content: ContentResponse = read_json(response)?;
        Ok(Some(content.sha))
    }

    /// Newest first.
    pub fn revisions(&self, repo: &str) -> Result<Vec<Revision>> {
        let response = self
            .authorized(self.agent.get(format!(
                "{}/repos/{repo}/commits?path={BOARD_FILE_NAME}&per_page={REVISIONS_LISTED}",
                self.api_base
            )))
            .call()
            .context("contacting GitHub")?;
        // GitHub answers 409 for a repository without any commits.
        if response.status().as_u16() == 409 {
            return Ok(Vec::new());
        }
        let commits: Vec<CommitResponse> = read_json(response)?;
        Ok(commits
            .into_iter()
            .map(|commit| Revision {
                version: commit.sha,
                committed_at: commit.commit.committer.date,
            })
            .collect())
    }

    /// The board title from the repository description, which every push
    /// refreshes; empty for an untitled board.
    pub fn title(&self, repo: &str) -> Result<String> {
        let repo = self.repo_info(repo)?;
        Ok(title_from_description(
            repo.description.as_deref().unwrap_or_default(),
        ))
    }

    /// The account's private repositories that hold a board, most recently
    /// pushed first, each named by its latest title.
    pub fn boards(&self) -> Result<Vec<CloudBoard>> {
        let mut boards = Vec::new();
        for page in 1..=MAX_REPO_PAGES {
            let response = self
                .authorized(self.agent.get(format!(
                    "{}/user/repos?visibility=private&affiliation=owner\
                     &per_page={REPOS_PER_PAGE}&page={page}",
                    self.api_base
                )))
                .call()
                .context("contacting GitHub")?;
            let repos: Vec<RepoResponse> = read_json(response)?;
            let last_page = repos.len() < REPOS_PER_PAGE;
            boards.extend(repos.into_iter().filter_map(|repo| {
                let description = repo.description.as_deref().unwrap_or_default();
                is_board_description(description).then(|| CloudBoard {
                    title: title_from_description(description),
                    updated_at: repo.pushed_at.or(repo.updated_at).unwrap_or_default(),
                    repo: repo.full_name,
                })
            }));
            if last_page {
                break;
            }
        }
        boards.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(boards)
    }

    pub fn latest_board(&self, repo: &str) -> Result<String> {
        self.raw_board(&self.board_file_url(repo))
    }

    pub fn board_at(&self, repo: &str, version: &str) -> Result<String> {
        self.raw_board(&format!("{}?ref={version}", self.board_file_url(repo)))
    }

    fn raw_board(&self, url: &str) -> Result<String> {
        let response = self
            .authorized(self.agent.get(url))
            .header("Accept", RAW_MEDIA_TYPE)
            .call()
            .context("contacting GitHub")?;
        if response.status().as_u16() == 404 {
            bail!("this version of the repository has no {BOARD_FILE_NAME}");
        }
        read_text(response)
    }

    fn board_file_url(&self, repo: &str) -> String {
        format!("{}/repos/{repo}/contents/{BOARD_FILE_NAME}", self.api_base)
    }

    fn authorized<B>(&self, request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        request
            .header("Accept", JSON_MEDIA_TYPE)
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", crate::APP_NAME)
            .header("Authorization", format!("Bearer {}", self.token))
    }
}

fn read_body(mut response: ureq::http::Response<ureq::Body>) -> Result<String> {
    response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .context("reading GitHub's response")
}

fn read_text(response: ureq::http::Response<ureq::Body>) -> Result<String> {
    let status = response.status();
    let text = read_body(response)?;
    if status.is_success() {
        return Ok(text);
    }
    bail!("{}", describe_failure(status.as_u16(), &text))
}

fn read_json<T: for<'de> Deserialize<'de>>(
    response: ureq::http::Response<ureq::Body>,
) -> Result<T> {
    let text = read_text(response)?;
    serde_json::from_str(&text).context("GitHub sent an unexpected response")
}

fn describe_failure(status: u16, body: &str) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        message: String,
    }
    let detail =
        serde_json::from_str::<ErrorBody>(body).map_or_else(|_| String::new(), |e| e.message);
    let hint = match status {
        401 => "the token is invalid or expired",
        403 => {
            "the token lacks the Contents or Administration permission, or the rate limit was hit"
        }
        404 => "the repository does not exist or the token cannot see it",
        409 | 422 => {
            "GitHub rejected the change; another device may have backed up at the same time"
        }
        _ => "GitHub refused the request",
    };
    if detail.is_empty() {
        format!("{hint} (HTTP {status})")
    } else {
        format!("{hint} (HTTP {status}: {detail})")
    }
}

fn repo_description(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        UNTITLED_DESCRIPTION.to_owned()
    } else {
        format!("{TITLED_DESCRIPTION_PREFIX}{title}")
    }
}

fn is_board_description(description: &str) -> bool {
    let description = description.trim();
    description == UNTITLED_DESCRIPTION || description.starts_with(TITLED_DESCRIPTION_PREFIX)
}

fn title_from_description(description: &str) -> String {
    let description = description.trim();
    if description == UNTITLED_DESCRIPTION {
        return String::new();
    }
    description
        .strip_prefix(TITLED_DESCRIPTION_PREFIX)
        .unwrap_or(description)
        .trim()
        .to_owned()
}

fn commit_message(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "Back up untitled board".to_owned()
    } else {
        format!("Back up {title}")
    }
}

/// `storysquish-` followed by the title in lowercase ASCII with every other
/// run of characters turned into one hyphen.
fn repo_name_for(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        return UNTITLED_REPO_NAME.to_owned();
    }
    let mut name = format!("{REPO_NAME_PREFIX}{slug}");
    name.truncate(MAX_REPO_NAME_LEN);
    name.trim_end_matches('-').to_owned()
}

/// Accepts `owner/name` or a GitHub URL for the repository, e.g. one copied
/// from the address bar or a clone URL.
pub fn parse_repo(input: &str) -> Option<String> {
    let input = input.trim();
    let path = ["https://", "http://"]
        .iter()
        .find_map(|scheme| input.strip_prefix(scheme))
        .map_or(input, |rest| rest.strip_prefix("www.").unwrap_or(rest));
    let path = path
        .strip_prefix("github.com/")
        .or_else(|| path.strip_prefix("git@github.com:"))
        .unwrap_or(path);
    let mut segments = path.split(['/', '#', '?']);
    let owner = segments.next()?;
    let name = segments.next()?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    let owner_is_valid =
        !owner.is_empty() && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let name_is_valid = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (owner_is_valid && name_is_valid).then(|| format!("{owner}/{name}"))
}

pub fn repo_url(repo: &str) -> String {
    format!("https://github.com/{repo}")
}

pub fn token_path(settings_path: &Path) -> PathBuf {
    settings_path.with_file_name(TOKEN_FILE_NAME)
}

/// A missing file means no token, not an error.
pub fn load_token(path: &Path) -> Result<Option<String>> {
    if !path.try_exists()? {
        return Ok(None);
    }
    let token = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let token = token.trim();
    Ok((!token.is_empty()).then(|| token.to_owned()))
}

/// Kept apart from settings.json, readable only by the owner, so the
/// settings file can be shared or synced without leaking the token.
pub fn save_token(path: &Path, token: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restricting {}", path.display()))?;
    }
    file.write_all(token.trim().as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}

pub fn delete_token(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", path.display()))
        }
        _ => Ok(()),
    }
}

#[derive(Debug)]
pub enum Job {
    Push {
        repo: Option<String>,
        title: String,
        board_json: String,
    },
    ListRevisions {
        repo: String,
    },
    Fetch {
        repo: String,
        version: String,
    },
    ListBoards,
    FetchLatest {
        repo: String,
    },
}

#[derive(Debug)]
pub enum Reply {
    Pushed {
        pushed: Pushed,
        board_json: String,
    },
    Revisions {
        title: String,
        revisions: Vec<Revision>,
    },
    Fetched(String),
    Boards(Vec<CloudBoard>),
    FetchedLatest {
        repo: String,
        board_json: String,
    },
    Failed(String),
}

/// Runs one request on a background thread so the UI never waits on the
/// network.
pub struct Pending(Receiver<Reply>);

impl Pending {
    pub fn start(client: Client, job: Job) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(run(&client, job));
        });
        Self(rx)
    }

    pub fn poll(&self) -> Option<Reply> {
        match self.0.try_recv() {
            Ok(reply) => Some(reply),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Reply::Failed(
                "the GitHub request stopped unexpectedly".to_owned(),
            )),
        }
    }

    pub fn wait(self) -> Reply {
        self.0
            .recv()
            .unwrap_or_else(|_| Reply::Failed("the GitHub request stopped unexpectedly".to_owned()))
    }
}

pub fn run(client: &Client, job: Job) -> Reply {
    let result = match job {
        Job::Push {
            repo,
            title,
            board_json,
        } => client
            .push(repo.as_deref(), &title, &board_json)
            .map(|pushed| Reply::Pushed { pushed, board_json }),
        Job::ListRevisions { repo } => client.revisions(&repo).and_then(|revisions| {
            Ok(Reply::Revisions {
                title: client.title(&repo)?,
                revisions,
            })
        }),
        Job::Fetch { repo, version } => client.board_at(&repo, &version).map(Reply::Fetched),
        Job::ListBoards => client.boards().map(Reply::Boards),
        Job::FetchLatest { repo } => client
            .latest_board(&repo)
            .map(|board_json| Reply::FetchedLatest { repo, board_json }),
    };
    result.unwrap_or_else(|e| Reply::Failed(format!("{e:#}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repos_come_from_names_and_urls() {
        let repo = "someone/storysquish-dune";
        for input in [
            repo,
            "https://github.com/someone/storysquish-dune",
            "https://www.github.com/someone/storysquish-dune/",
            "https://github.com/someone/storysquish-dune/commits/main",
            "https://github.com/someone/storysquish-dune.git",
            "git@github.com:someone/storysquish-dune.git",
            "github.com/someone/storysquish-dune#readme",
            "  someone/storysquish-dune \n",
        ] {
            assert_eq!(parse_repo(input).as_deref(), Some(repo), "{input}");
        }
        for input in [
            "",
            "storysquish-dune",
            "https://github.com/someone",
            "someone/..",
            "some one/novel",
            "someone/my novel",
            "aa5a315d61ae9438b18d",
        ] {
            assert_eq!(parse_repo(input), None, "{input}");
        }
    }

    #[test]
    fn repo_names_come_from_the_title() {
        assert_eq!(repo_name_for("Dune"), "storysquish-dune");
        assert_eq!(
            repo_name_for("  Dune: Messiah!  "),
            "storysquish-dune-messiah"
        );
        assert_eq!(repo_name_for("Émile & co"), "storysquish-mile-co");
        assert_eq!(repo_name_for("  "), "storysquish-board");
        assert_eq!(repo_name_for("……"), "storysquish-board");
        let long = repo_name_for(&"word ".repeat(40));
        assert!(long.len() <= MAX_REPO_NAME_LEN, "{long}");
        assert!(!long.ends_with('-'), "{long}");
    }

    #[test]
    fn description_names_the_novel() {
        assert_eq!(repo_description("  "), "StorySquish board");
        assert_eq!(repo_description("Dune"), "StorySquish board: Dune");
    }

    #[test]
    fn title_round_trips_through_the_description() {
        for title in ["", "Dune", "Dune: Messiah"] {
            let description = repo_description(title);
            assert!(is_board_description(&description));
            assert_eq!(title_from_description(&description), title);
        }
        assert!(!is_board_description("my notes"));
        assert!(!is_board_description("StorySquish boards are great"));
    }

    #[test]
    fn failures_explain_the_status_and_keep_githubs_message() {
        assert_eq!(
            describe_failure(401, r#"{"message":"Bad credentials"}"#),
            "the token is invalid or expired (HTTP 401: Bad credentials)"
        );
        assert_eq!(
            describe_failure(502, "<html>"),
            "GitHub refused the request (HTTP 502)"
        );
    }

    #[test]
    fn token_round_trips_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = token_path(&dir.path().join("storysquish/settings.json"));
        assert_eq!(load_token(&path).unwrap(), None);
        save_token(&path, " ghp_secret\n").unwrap();
        assert_eq!(load_token(&path).unwrap().as_deref(), Some("ghp_secret"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        delete_token(&path).unwrap();
        delete_token(&path).unwrap();
        assert_eq!(load_token(&path).unwrap(), None);
    }
}
