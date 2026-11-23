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
use ureq::config::RedirectAuthHeaders;

pub const API_BASE: &str = "https://api.github.com";
pub const BOARD_FILE_NAME: &str = "storysquish-board.json";
pub const TOKEN_FILE_NAME: &str = "github-token";
const REPO_NAME_PREFIX: &str = "storysquish-";
const UNTITLED_REPO_NAME: &str = "storysquish-board";
const MAX_SUGGESTED_REPO_NAME_LEN: usize = 80;
const MAX_REPO_NAME_LEN: usize = 100;
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

/// The linked repository is gone, or the token can no longer see it, so the
/// board needs a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingRepo(pub String);

impl std::fmt::Display for MissingRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} does not exist or the token cannot see it", self.0)
    }
}

impl std::error::Error for MissingRepo {}

#[derive(Deserialize)]
struct UserResponse {
    login: String,
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
            // GitHub answers a renamed repository's old name with a redirect
            // that only resolves for private repositories when authorized.
            .redirect_auth_headers(RedirectAuthHeaders::SameHost)
            .build()
            .into();
        Self {
            agent,
            api_base: api_base.trim_end_matches('/').to_owned(),
            token: token.trim().to_owned(),
        }
    }

    /// Commits a new version to `repo`, which must be private, renaming it
    /// to follow the title when its name came from the previous title.
    pub fn push(&self, repo: &str, title: &str, board_json: &str) -> Result<Pushed> {
        let existing = self
            .repo_info_if_visible(repo)?
            .ok_or_else(|| MissingRepo(repo.to_owned()))?;
        if !existing.private {
            bail!("{repo} is public; StorySquish backs up only to private repositories");
        }
        let repo = match renamed_repo_base(&existing, title) {
            Some(base_name) => self.rename_repo(&existing.full_name, &base_name)?,
            None => existing.full_name,
        };
        let description = repo_description(title);
        if existing.description.as_deref() != Some(description.as_str()) {
            self.set_description(&repo, &description)?;
        }
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

    /// Creates the private repository `name` on the token's account and
    /// returns its `owner/name`; a taken name is an error, never a reuse.
    pub fn create_repo(&self, name: &str, title: &str) -> Result<String> {
        if !is_valid_repo_name(name) {
            bail!("“{name}” is not a valid repository name");
        }
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
            bail!("{name} already exists on your account; choose another name");
        }
        if !(200..300).contains(&status) {
            bail!("{}", describe_failure(status, &text));
        }
        let created: RepoResponse =
            serde_json::from_str(&text).context("GitHub sent an unexpected response")?;
        Ok(created.full_name)
    }

    /// The name `create_repo` should offer for a board titled `title`: the
    /// title's slug, numbered when the account already has a repo by that
    /// name.
    pub fn suggest_repo_name(&self, title: &str) -> Result<String> {
        let login = self.login()?;
        let base_name = suggested_repo_name(title);
        for attempt in 1..=MAX_REPO_NAME_ATTEMPTS {
            let name = numbered_repo_name(&base_name, attempt);
            if self
                .repo_info_if_visible(&format!("{login}/{name}"))?
                .is_none()
            {
                return Ok(name);
            }
        }
        bail!("every repository name tried for this board, from {base_name}, is taken")
    }

    fn login(&self) -> Result<String> {
        let response = self
            .authorized(self.agent.get(format!("{}/user", self.api_base)))
            .call()
            .context("contacting GitHub")?;
        read_json::<UserResponse>(response).map(|user| user.login)
    }

    fn repo_info_if_visible(&self, repo: &str) -> Result<Option<RepoResponse>> {
        let response = self
            .authorized(self.agent.get(format!("{}/repos/{repo}", self.api_base)))
            .call()
            .context("contacting GitHub")?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        read_json(response).map(Some)
    }

    fn repo_info(&self, repo: &str) -> Result<RepoResponse> {
        let response = self
            .authorized(self.agent.get(format!("{}/repos/{repo}", self.api_base)))
            .call()
            .context("contacting GitHub")?;
        read_json(response)
    }

    /// Tries `base_name` and its numbered variants, keeping the current name
    /// when every one is taken, and returns the resulting `owner/name`.
    fn rename_repo(&self, repo: &str, base_name: &str) -> Result<String> {
        for attempt in 1..=MAX_REPO_NAME_ATTEMPTS {
            let name = numbered_repo_name(base_name, attempt);
            let response = self
                .authorized(self.agent.patch(format!("{}/repos/{repo}", self.api_base)))
                .send_json(json!({ "name": name }))
                .context("contacting GitHub")?;
            let status = response.status().as_u16();
            let text = read_body(response)?;
            if status == 422 && text.contains("already exists") {
                continue;
            }
            if !(200..300).contains(&status) {
                bail!("{}", describe_failure(status, &text));
            }
            let renamed: RepoResponse =
                serde_json::from_str(&text).context("GitHub sent an unexpected response")?;
            return Ok(renamed.full_name);
        }
        Ok(repo.to_owned())
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
pub fn suggested_repo_name(title: &str) -> String {
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
    name.truncate(MAX_SUGGESTED_REPO_NAME_LEN);
    name.trim_end_matches('-').to_owned()
}

/// The base name to rename `existing` to for `title`, or `None` when its name
/// does not follow the title it was backed up under, e.g. one the writer
/// chose, or when the new title gives the same name.
fn renamed_repo_base(existing: &RepoResponse, title: &str) -> Option<String> {
    let description = existing.description.as_deref().unwrap_or_default();
    if !is_board_description(description) {
        return None;
    }
    let previous_base = suggested_repo_name(&title_from_description(description));
    let base_name = suggested_repo_name(title);
    (base_name != previous_base
        && is_numbered_repo_name(repo_name(&existing.full_name), &previous_base))
    .then_some(base_name)
}

fn is_numbered_repo_name(name: &str, base_name: &str) -> bool {
    (1..=MAX_REPO_NAME_ATTEMPTS).any(|attempt| numbered_repo_name(base_name, attempt) == name)
}

fn numbered_repo_name(base_name: &str, attempt: usize) -> String {
    if attempt == 1 {
        base_name.to_owned()
    } else {
        format!("{base_name}-{attempt}")
    }
}

pub fn is_valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_REPO_NAME_LEN
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The `name` part of `owner/name`.
pub fn repo_name(repo: &str) -> &str {
    repo.rsplit_once('/').map_or(repo, |(_, name)| name)
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
    (owner_is_valid && is_valid_repo_name(name)).then(|| format!("{owner}/{name}"))
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
        repo: String,
        title: String,
        board_json: String,
    },
    SuggestRepoName {
        title: String,
    },
    CreateRepo {
        name: String,
        title: String,
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
    RepoMissing {
        repo: String,
    },
    SuggestedRepoName(String),
    CreatedRepo(String),
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
        } => match client.push(&repo, &title, &board_json) {
            Err(e) if e.is::<MissingRepo>() => Ok(Reply::RepoMissing { repo }),
            pushed => pushed.map(|pushed| Reply::Pushed { pushed, board_json }),
        },
        Job::SuggestRepoName { title } => client
            .suggest_repo_name(&title)
            .map(Reply::SuggestedRepoName),
        Job::CreateRepo { name, title } => {
            client.create_repo(&name, &title).map(Reply::CreatedRepo)
        }
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
        assert_eq!(suggested_repo_name("Dune"), "storysquish-dune");
        assert_eq!(
            suggested_repo_name("  Dune: Messiah!  "),
            "storysquish-dune-messiah"
        );
        assert_eq!(suggested_repo_name("Émile & co"), "storysquish-mile-co");
        assert_eq!(suggested_repo_name("  "), "storysquish-board");
        assert_eq!(suggested_repo_name("……"), "storysquish-board");
        let long = suggested_repo_name(&"word ".repeat(40));
        assert!(long.len() <= MAX_SUGGESTED_REPO_NAME_LEN, "{long}");
        assert!(!long.ends_with('-'), "{long}");
        assert!(is_valid_repo_name(&numbered_repo_name(&long, 20)));
    }

    fn repo(full_name: &str, description: &str) -> RepoResponse {
        RepoResponse {
            full_name: full_name.to_owned(),
            private: true,
            description: Some(description.to_owned()),
            pushed_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn only_repos_named_after_the_previous_title_follow_a_new_one() {
        let dune = repo("someone/storysquish-dune", "StorySquish board: Dune");
        assert_eq!(
            renamed_repo_base(&dune, "Dune Messiah").as_deref(),
            Some("storysquish-dune-messiah")
        );
        assert_eq!(renamed_repo_base(&dune, "Dune"), None);
        assert_eq!(renamed_repo_base(&dune, "  DUNE!"), None);

        let numbered = repo("someone/storysquish-dune-3", "StorySquish board: Dune");
        assert_eq!(
            renamed_repo_base(&numbered, "Arrakis").as_deref(),
            Some("storysquish-arrakis")
        );

        let untitled = repo("someone/storysquish-board", "StorySquish board");
        assert_eq!(
            renamed_repo_base(&untitled, "Dune").as_deref(),
            Some("storysquish-dune")
        );

        let chosen = repo("someone/my-novel", "StorySquish board: Dune");
        assert_eq!(renamed_repo_base(&chosen, "Dune Messiah"), None);

        let foreign = repo("someone/storysquish-dune", "my notes");
        assert_eq!(renamed_repo_base(&foreign, "Dune Messiah"), None);
    }

    #[test]
    fn repo_names_follow_githubs_rules() {
        for name in ["storysquish-dune", "a", "my_novel.v2"] {
            assert!(is_valid_repo_name(name), "{name}");
        }
        for name in ["", ".", "..", "my novel", "dune/2", &"a".repeat(101)] {
            assert!(!is_valid_repo_name(name), "{name}");
        }
        assert_eq!(repo_name("someone/storysquish-dune"), "storysquish-dune");
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
