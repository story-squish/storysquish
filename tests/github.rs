use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use storysquish::github::{
    BOARD_FILE_NAME, Client, CloudBoard, MissingRepo, Revision, SuggestedRepo,
};

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    authorization: Option<String>,
    accept: Option<String>,
    body: String,
}

/// Answers each request with `respond(method, path)`, closing the connection
/// after every response so no keep-alive handling is needed.
fn mock_github(
    respond: impl Fn(&str, &str) -> (u16, String) + Send + 'static,
) -> (String, Arc<Mutex<Vec<Recorded>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&recorded);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_owned();
            let path = parts.next().unwrap_or_default().to_owned();
            let mut content_length = 0;
            let mut authorization = None;
            let mut accept = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                let (name, value) = line.split_once(':').unwrap();
                match name.to_ascii_lowercase().as_str() {
                    "content-length" => content_length = value.trim().parse().unwrap(),
                    "authorization" => authorization = Some(value.trim().to_owned()),
                    "accept" => accept = Some(value.trim().to_owned()),
                    _ => {}
                }
            }
            let mut body = vec![0; content_length];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            let (status, reply) = respond(&method, &path);
            log.lock().unwrap().push(Recorded {
                method,
                path,
                authorization,
                accept,
                body,
            });
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
        }
    });
    (base, recorded)
}

const REPO: &str = "someone/storysquish-dune";

fn board_path() -> String {
    format!("/repos/{REPO}/contents/{BOARD_FILE_NAME}")
}

fn repo_json(private: bool, description: &str) -> String {
    serde_json::json!({
        "full_name": REPO,
        "private": private,
        "description": description,
    })
    .to_string()
}

fn not_found() -> (u16, String) {
    (404, r#"{"message":"Not Found"}"#.to_owned())
}

fn body_of(recorded: &Recorded) -> serde_json::Value {
    serde_json::from_str(&recorded.body).unwrap()
}

fn decoded_content(recorded: &Recorded) -> String {
    let content = body_of(recorded)["content"].as_str().unwrap().to_owned();
    String::from_utf8(BASE64.decode(content).unwrap()).unwrap()
}

#[test]
fn a_repo_is_created_private_under_the_confirmed_name() {
    let (base, recorded) = mock_github(|method, path| match (method, path) {
        ("POST", "/user/repos") => (201, repo_json(true, "StorySquish board: Dune")),
        _ => not_found(),
    });
    let client = Client::with_api_base(" tok \n", &base);

    assert_eq!(client.create_repo("my-dune", "Dune").unwrap(), REPO);

    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].authorization.as_deref(), Some("Bearer tok"));
    let created = body_of(&recorded[0]);
    assert_eq!(created["private"], true);
    assert_eq!(created["name"], "my-dune");
    assert_eq!(created["description"], "StorySquish board: Dune");
}

#[test]
fn a_taken_name_is_reported_rather_than_reused() {
    let (base, recorded) = mock_github(|_, _| {
        (
            422,
            r#"{"message":"Repository creation failed.","errors":[{"message":"name already exists on this account"}]}"#
                .to_owned(),
        )
    });
    let client = Client::with_api_base("tok", &base);

    let error = format!(
        "{:#}",
        client.create_repo("storysquish-dune", "Dune").unwrap_err()
    );
    assert!(error.contains("storysquish-dune already exists"), "{error}");
    assert_eq!(recorded.lock().unwrap().len(), 1);
}

#[test]
fn an_invalid_name_never_reaches_github() {
    let (base, recorded) = mock_github(|_, _| not_found());
    let client = Client::with_api_base("tok", &base);

    assert!(client.create_repo("my novel", "Dune").is_err());
    assert!(recorded.lock().unwrap().is_empty());
}

#[test]
fn the_suggested_name_skips_names_the_account_already_uses() {
    let (base, _) = mock_github(|_, path| match path {
        "/user" => (200, r#"{"login":"someone"}"#.to_owned()),
        "/repos/someone/storysquish-dune" => (200, repo_json(true, "my notes")),
        _ => not_found(),
    });
    let client = Client::with_api_base("tok", &base);

    assert_eq!(
        client.suggest_repo_name("Dune").unwrap(),
        SuggestedRepo {
            owner: "someone".to_owned(),
            name: "storysquish-dune-2".to_owned(),
        }
    );
}

#[test]
fn the_first_push_to_a_new_repo_commits_without_a_previous_version() {
    let (base, recorded) = mock_github(|method, path| match (method, path) {
        ("GET", path) if path == format!("/repos/{REPO}") => {
            (200, repo_json(true, "StorySquish board: Dune"))
        }
        ("PUT", _) => (201, "{}".to_owned()),
        _ => not_found(),
    });
    let client = Client::with_api_base("tok", &base);

    let pushed = client.push(REPO, "Dune", r#"{"title":"Dune"}"#).unwrap();
    assert_eq!(pushed.repo, REPO);

    let recorded = recorded.lock().unwrap();
    let put = recorded.last().unwrap();
    assert_eq!(put.method, "PUT");
    assert_eq!(body_of(put)["message"], "Back up Dune");
    assert_eq!(body_of(put).get("sha"), None);
    assert_eq!(decoded_content(put), r#"{"title":"Dune"}"#);
}

#[test]
fn a_missing_repo_is_reported_and_never_recreated() {
    let (base, recorded) = mock_github(|_, _| not_found());
    let client = Client::with_api_base("tok", &base);

    let error = client.push(REPO, "Dune", "{}").unwrap_err();
    assert_eq!(
        error.downcast_ref::<MissingRepo>(),
        Some(&MissingRepo(REPO.to_owned()))
    );
    let recorded = recorded.lock().unwrap();
    assert!(recorded.iter().all(|r| r.method == "GET"), "{recorded:?}");
}

#[test]
fn later_pushes_replace_the_file_and_keep_the_description_fresh() {
    let (base, recorded) = mock_github(|method, path| match (method, path) {
        ("GET", path) if path == format!("/repos/{REPO}") => {
            (200, repo_json(true, "StorySquish board: Dune"))
        }
        ("GET", _) => (200, r#"{"sha":"f00d"}"#.to_owned()),
        _ => (200, "{}".to_owned()),
    });
    let client = Client::with_api_base("tok", &base);

    client.push(REPO, "Dune", r#"{"title":"Dune 2"}"#).unwrap();
    client.push(REPO, "dune!", "{}").unwrap();

    let recorded = recorded.lock().unwrap();
    let calls: Vec<_> = recorded
        .iter()
        .map(|r| (r.method.as_str(), r.path.as_str()))
        .collect();
    let repo_path = format!("/repos/{REPO}");
    assert_eq!(
        calls,
        [
            ("GET", repo_path.as_str()),
            ("GET", board_path().as_str()),
            ("PUT", board_path().as_str()),
            ("GET", repo_path.as_str()),
            ("PATCH", repo_path.as_str()),
            ("GET", board_path().as_str()),
            ("PUT", board_path().as_str()),
        ]
    );
    assert_eq!(body_of(&recorded[2])["sha"], "f00d");
    assert_eq!(
        body_of(&recorded[4]),
        serde_json::json!({ "description": "StorySquish board: dune!" })
    );
}

const RENAMED: &str = "someone/storysquish-dune-messiah";

fn renamed_json() -> String {
    serde_json::json!({ "full_name": RENAMED, "private": true }).to_string()
}

#[test]
fn a_new_title_renames_a_repo_named_after_the_old_one() {
    let (base, recorded) = mock_github(|method, path| match (method, path) {
        ("GET", path) if path == format!("/repos/{REPO}") => {
            (200, repo_json(true, "StorySquish board: Dune"))
        }
        ("PATCH", path) if path == format!("/repos/{REPO}") => (200, renamed_json()),
        ("GET", _) => not_found(),
        _ => (200, "{}".to_owned()),
    });
    let client = Client::with_api_base("tok", &base);

    let pushed = client.push(REPO, "Dune Messiah", "{}").unwrap();

    assert_eq!(pushed.repo, RENAMED);
    let recorded = recorded.lock().unwrap();
    let calls: Vec<_> = recorded
        .iter()
        .map(|r| (r.method.as_str(), r.path.as_str()))
        .collect();
    let old_path = format!("/repos/{REPO}");
    let new_path = format!("/repos/{RENAMED}");
    let new_board_path = format!("{new_path}/contents/{BOARD_FILE_NAME}");
    assert_eq!(
        calls,
        [
            ("GET", old_path.as_str()),
            ("PATCH", old_path.as_str()),
            ("PATCH", new_path.as_str()),
            ("GET", new_board_path.as_str()),
            ("PUT", new_board_path.as_str()),
        ]
    );
    assert_eq!(
        body_of(&recorded[1]),
        serde_json::json!({ "name": "storysquish-dune-messiah" })
    );
    assert_eq!(
        body_of(&recorded[2]),
        serde_json::json!({ "description": "StorySquish board: Dune Messiah" })
    );
}

const NAME_TAKEN: &str = r#"{"message":"Repository creation failed.","errors":[{"message":"name already exists on this account"}]}"#;

/// Answers the first `taken` rename requests as a taken name, then renames to
/// `RENAMED` numbered 2.
fn mock_with_taken_names(taken: usize) -> (String, Arc<Mutex<Vec<Recorded>>>) {
    let patches = AtomicUsize::new(0);
    mock_github(move |method, path| match (method, path) {
        ("GET", path) if path == format!("/repos/{REPO}") => {
            (200, repo_json(true, "StorySquish board: Dune"))
        }
        ("PATCH", _) if patches.fetch_add(1, Ordering::SeqCst) < taken => {
            (422, NAME_TAKEN.to_owned())
        }
        ("PATCH", path) if path == format!("/repos/{REPO}") => (
            200,
            serde_json::json!({ "full_name": format!("{RENAMED}-2"), "private": true }).to_string(),
        ),
        ("GET", _) => not_found(),
        _ => (200, "{}".to_owned()),
    })
}

fn requested_names(recorded: &[Recorded]) -> Vec<String> {
    recorded
        .iter()
        .filter(|r| r.body.contains("\"name\""))
        .map(|r| body_of(r)["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_rename_skips_names_the_account_already_uses() {
    let (base, recorded) = mock_with_taken_names(1);
    let client = Client::with_api_base("tok", &base);

    let pushed = client.push(REPO, "Dune Messiah", "{}").unwrap();

    assert_eq!(pushed.repo, format!("{RENAMED}-2"));
    assert_eq!(
        requested_names(&recorded.lock().unwrap()),
        ["storysquish-dune-messiah", "storysquish-dune-messiah-2"]
    );
}

#[test]
fn a_repo_keeps_its_name_when_every_new_one_is_taken() {
    let (base, recorded) = mock_with_taken_names(20);
    let client = Client::with_api_base("tok", &base);

    let pushed = client.push(REPO, "Dune Messiah", "{}").unwrap();

    assert_eq!(pushed.repo, REPO);
    let recorded = recorded.lock().unwrap();
    assert_eq!(requested_names(&recorded).len(), 20);
    assert!(
        recorded
            .iter()
            .any(|r| r.method == "PUT" && r.path == board_path()),
        "{recorded:?}"
    );
}

#[test]
fn a_repo_named_by_the_writer_keeps_its_name() {
    let (base, recorded) = mock_github(|method, path| match (method, path) {
        ("GET", "/repos/someone/my-novel") => (
            200,
            serde_json::json!({
                "full_name": "someone/my-novel",
                "private": true,
                "description": "StorySquish board: Dune",
            })
            .to_string(),
        ),
        ("GET", _) => not_found(),
        _ => (200, "{}".to_owned()),
    });
    let client = Client::with_api_base("tok", &base);

    let pushed = client
        .push("someone/my-novel", "Dune Messiah", "{}")
        .unwrap();

    assert_eq!(pushed.repo, "someone/my-novel");
    let recorded = recorded.lock().unwrap();
    assert!(
        recorded.iter().all(|r| !r.body.contains("\"name\"")),
        "{recorded:?}"
    );
}

#[test]
fn a_public_repo_is_never_written_to() {
    let (base, recorded) = mock_github(|_, _| (200, repo_json(false, "StorySquish board")));
    let client = Client::with_api_base("tok", &base);

    let error = format!("{:#}", client.push(REPO, "", "{}").unwrap_err());
    assert!(error.contains("public"), "{error}");
    let recorded = recorded.lock().unwrap();
    assert!(recorded.iter().all(|r| r.method == "GET"), "{recorded:?}");
}

#[test]
fn revisions_are_listed_and_fetched_raw() {
    let (base, recorded) = mock_github(|_, path| {
        if path == format!("/repos/{REPO}/commits?path={BOARD_FILE_NAME}&per_page=30") {
            (
                200,
                r#"[{"sha":"v2","commit":{"committer":{"date":"2026-11-22T10:00:00Z"}}},
                    {"sha":"v1","commit":{"committer":{"date":"2026-11-21T10:00:00Z"}}}]"#
                    .to_owned(),
            )
        } else if path == format!("{}?ref=v1", board_path()) {
            (200, r#"{"title":"Old"}"#.to_owned())
        } else {
            not_found()
        }
    });
    let client = Client::with_api_base("tok", &base);

    assert_eq!(
        client.revisions(REPO).unwrap(),
        vec![
            Revision {
                version: "v2".into(),
                committed_at: "2026-11-22T10:00:00Z".into()
            },
            Revision {
                version: "v1".into(),
                committed_at: "2026-11-21T10:00:00Z".into()
            },
        ]
    );
    assert_eq!(client.board_at(REPO, "v1").unwrap(), r#"{"title":"Old"}"#);
    let recorded = recorded.lock().unwrap();
    assert_eq!(
        recorded[1].accept.as_deref(),
        Some("application/vnd.github.raw+json")
    );
}

#[test]
fn an_empty_repo_has_no_revisions() {
    let (base, _) =
        mock_github(|_, _| (409, r#"{"message":"Git Repository is empty."}"#.to_owned()));
    let client = Client::with_api_base("tok", &base);

    assert_eq!(client.revisions(REPO).unwrap(), []);
}

#[test]
fn failures_name_the_likely_cause() {
    let (base, _) = mock_github(|_, _| (401, r#"{"message":"Bad credentials"}"#.to_owned()));
    let client = Client::with_api_base("expired", &base);

    let error = format!("{:#}", client.push(REPO, "", "{}").unwrap_err());
    assert_eq!(
        error,
        "the token is invalid or expired (HTTP 401: Bad credentials)"
    );
}

#[test]
fn a_version_without_the_board_file_is_an_error() {
    let (base, _) = mock_github(|_, _| not_found());
    let client = Client::with_api_base("tok", &base);

    let error = format!("{:#}", client.board_at(REPO, "v1").unwrap_err());
    assert!(error.contains(BOARD_FILE_NAME), "{error}");
}

#[test]
fn title_comes_from_the_repo_description() {
    let (base, _) = mock_github(|_, path| match path {
        "/repos/someone/titled" => (200, repo_json(true, "StorySquish board: Dune")),
        _ => (200, repo_json(true, "StorySquish board")),
    });
    let client = Client::with_api_base("tok", &base);

    assert_eq!(client.title("someone/titled").unwrap(), "Dune");
    assert_eq!(client.title("someone/untitled").unwrap(), "");
}

#[test]
fn cloud_boards_are_private_repos_holding_a_board_newest_first() {
    let (base, recorded) = mock_github(|_, path| match path {
        "/user/repos?visibility=private&affiliation=owner&per_page=100&page=1" => (
            200,
            serde_json::json!([
                {"full_name": "someone/storysquish-emma", "private": true,
                 "description": "StorySquish board: Emma",
                 "pushed_at": "2026-01-01T00:00:00Z", "updated_at": "2026-12-05T00:00:00Z"},
                {"full_name": "someone/dotfiles", "private": true,
                 "description": "my config", "pushed_at": "2026-12-01T00:00:00Z"},
                {"full_name": "someone/storysquish-board", "private": true,
                 "description": "StorySquish board", "pushed_at": "2026-11-22T00:00:00Z"},
                {"full_name": "someone/empty", "private": true, "description": null,
                 "pushed_at": null},
            ])
            .to_string(),
        ),
        _ => not_found(),
    });
    let client = Client::with_api_base("tok", &base);

    assert_eq!(
        client.boards().unwrap(),
        vec![
            CloudBoard {
                repo: "someone/storysquish-board".into(),
                title: String::new(),
                updated_at: "2026-11-22T00:00:00Z".into()
            },
            CloudBoard {
                repo: "someone/storysquish-emma".into(),
                title: "Emma".into(),
                updated_at: "2026-01-01T00:00:00Z".into()
            },
        ]
    );
    assert_eq!(recorded.lock().unwrap().len(), 1);
}

#[test]
fn latest_board_is_the_file_on_the_default_branch() {
    let (base, _) = mock_github(|_, path| {
        if path == board_path() {
            (200, r#"{"title":"New"}"#.to_owned())
        } else {
            not_found()
        }
    });
    let client = Client::with_api_base("tok", &base);

    assert_eq!(client.latest_board(REPO).unwrap(), r#"{"title":"New"}"#);
}
