use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;
use ureq::Agent;

pub const API_BASE: &str = "https://api.github.com";
pub const BOARD_FILE_NAME: &str = "storysquish-board.json";
pub const TOKEN_FILE_NAME: &str = "github-token";
const REVISIONS_LISTED: usize = 30;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Revision {
    pub version: String,
    pub committed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    pub gist_id: String,
    pub html_url: String,
}

#[derive(Deserialize)]
struct GistResponse {
    id: String,
    html_url: String,
    #[serde(default)]
    files: std::collections::HashMap<String, Option<GistFile>>,
}

#[derive(Deserialize)]
struct GistFile {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    truncated: bool,
    raw_url: String,
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

    /// Creates a secret gist when `gist_id` is `None`, otherwise adds a
    /// revision to the existing one.
    pub fn push(&self, gist_id: Option<&str>, title: &str, board_json: &str) -> Result<Pushed> {
        let body = json!({
            "description": gist_description(title),
            "public": false,
            "files": { BOARD_FILE_NAME: { "content": board_json } },
        });
        let response = match gist_id {
            None => self
                .authorized(self.agent.post(format!("{}/gists", self.api_base)))
                .send_json(&body),
            Some(id) => self
                .authorized(self.agent.patch(format!("{}/gists/{id}", self.api_base)))
                .send_json(&body),
        }
        .context("contacting GitHub")?;
        let gist: GistResponse = read_json(response)?;
        Ok(Pushed {
            gist_id: gist.id,
            html_url: gist.html_url,
        })
    }

    /// Newest first.
    pub fn revisions(&self, gist_id: &str) -> Result<Vec<Revision>> {
        let response = self
            .authorized(self.agent.get(format!(
                "{}/gists/{gist_id}/commits?per_page={REVISIONS_LISTED}",
                self.api_base
            )))
            .call()
            .context("contacting GitHub")?;
        read_json(response)
    }

    pub fn board_at(&self, gist_id: &str, version: &str) -> Result<String> {
        let response = self
            .authorized(
                self.agent
                    .get(format!("{}/gists/{gist_id}/{version}", self.api_base)),
            )
            .call()
            .context("contacting GitHub")?;
        let mut gist: GistResponse = read_json(response)?;
        let Some(Some(file)) = gist.files.remove(BOARD_FILE_NAME) else {
            bail!("this gist revision has no {BOARD_FILE_NAME}");
        };
        match file.content {
            Some(content) if !file.truncated => Ok(content),
            // The API inlines at most 1 MB; larger boards come from the raw
            // URL, which embeds the secret gist's id and needs no token.
            _ => {
                let response = self
                    .agent
                    .get(&file.raw_url)
                    .call()
                    .context("downloading the board from GitHub")?;
                read_text(response)
            }
        }
    }

    fn authorized<B>(&self, request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        request
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", crate::APP_NAME)
            .header("Authorization", format!("Bearer {}", self.token))
    }
}

fn read_text(mut response: ureq::http::Response<ureq::Body>) -> Result<String> {
    let status = response.status();
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .context("reading GitHub's response")?;
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
        403 => "the token lacks the Gist permission, or the rate limit was hit",
        404 => "the gist does not exist or belongs to another account",
        _ => "GitHub refused the request",
    };
    if detail.is_empty() {
        format!("{hint} (HTTP {status})")
    } else {
        format!("{hint} (HTTP {status}: {detail})")
    }
}

fn gist_description(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "StorySquish board".to_owned()
    } else {
        format!("StorySquish board: {title}")
    }
}

/// Accepts a bare id or any gist URL, e.g. one copied from the address bar.
pub fn parse_gist_id(input: &str) -> Option<String> {
    let last_segment = input
        .trim()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let id = last_segment.split(['#', '?']).next().unwrap_or_default();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit())).then(|| id.to_owned())
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
        gist_id: Option<String>,
        title: String,
        board_json: String,
    },
    ListRevisions {
        gist_id: String,
    },
    Fetch {
        gist_id: String,
        version: String,
    },
}

#[derive(Debug)]
pub enum Reply {
    Pushed { pushed: Pushed, board_json: String },
    Revisions(Vec<Revision>),
    Fetched(String),
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
            gist_id,
            title,
            board_json,
        } => client
            .push(gist_id.as_deref(), &title, &board_json)
            .map(|pushed| Reply::Pushed { pushed, board_json }),
        Job::ListRevisions { gist_id } => client.revisions(&gist_id).map(Reply::Revisions),
        Job::Fetch { gist_id, version } => client.board_at(&gist_id, &version).map(Reply::Fetched),
    };
    result.unwrap_or_else(|e| Reply::Failed(format!("{e:#}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gist_ids_come_from_ids_and_urls() {
        let id = "aa5a315d61ae9438b18d";
        for input in [
            id,
            "https://gist.github.com/someone/aa5a315d61ae9438b18d",
            "https://gist.github.com/aa5a315d61ae9438b18d/",
            "https://gist.github.com/someone/aa5a315d61ae9438b18d#file-x",
            "  aa5a315d61ae9438b18d \n",
        ] {
            assert_eq!(parse_gist_id(input).as_deref(), Some(id), "{input}");
        }
        assert_eq!(parse_gist_id(""), None);
        assert_eq!(parse_gist_id("https://gist.github.com/"), None);
        assert_eq!(parse_gist_id("not a gist"), None);
    }

    #[test]
    fn description_names_the_novel() {
        assert_eq!(gist_description("  "), "StorySquish board");
        assert_eq!(gist_description("Dune"), "StorySquish board: Dune");
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
