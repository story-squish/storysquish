use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use storysquish::gist::{BOARD_FILE_NAME, Client, Revision};

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    authorization: Option<String>,
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

fn gist_json(id: &str, files: &serde_json::Value) -> String {
    serde_json::json!({
        "id": id,
        "html_url": format!("https://gist.github.com/{id}"),
        "files": files,
    })
    .to_string()
}

#[test]
fn first_push_creates_a_secret_gist_and_later_pushes_update_it() {
    let (base, recorded) = mock_github(|method, _| match method {
        "POST" => (201, gist_json("abc123", &serde_json::json!({}))),
        _ => (200, gist_json("abc123", &serde_json::json!({}))),
    });
    let client = Client::with_api_base(" tok \n", &base);

    let created = client.push(None, "Dune", r#"{"title":"Dune"}"#).unwrap();
    assert_eq!(created.gist_id, "abc123");
    assert_eq!(created.html_url, "https://gist.github.com/abc123");
    client
        .push(Some("abc123"), "Dune", r#"{"title":"Dune 2"}"#)
        .unwrap();

    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded[0].method, "POST");
    assert_eq!(recorded[0].path, "/gists");
    assert_eq!(recorded[0].authorization.as_deref(), Some("Bearer tok"));
    let body: serde_json::Value = serde_json::from_str(&recorded[0].body).unwrap();
    assert_eq!(body["public"], false);
    assert_eq!(body["description"], "StorySquish board: Dune");
    assert_eq!(
        body["files"][BOARD_FILE_NAME]["content"],
        r#"{"title":"Dune"}"#
    );
    assert_eq!(recorded[1].method, "PATCH");
    assert_eq!(recorded[1].path, "/gists/abc123");
}

#[test]
fn revisions_are_listed_and_fetched() {
    let (base, _) = mock_github(|_, path| match path {
        "/gists/abc123/commits?per_page=30" => (
            200,
            r#"[{"version":"v2","committed_at":"2026-11-22T10:00:00Z","url":"x"},
                {"version":"v1","committed_at":"2026-11-21T10:00:00Z","url":"x"}]"#
                .to_owned(),
        ),
        "/gists/abc123/v1" => (
            200,
            gist_json(
                "abc123",
                &serde_json::json!({ BOARD_FILE_NAME: {
                    "content": "{\"title\":\"Old\"}", "truncated": false, "raw_url": "unused"
                }}),
            ),
        ),
        _ => (404, r#"{"message":"Not Found"}"#.to_owned()),
    });
    let client = Client::with_api_base("tok", &base);

    let revisions = client.revisions("abc123").unwrap();
    assert_eq!(
        revisions,
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
    assert_eq!(
        client.board_at("abc123", "v1").unwrap(),
        r#"{"title":"Old"}"#
    );
}

#[test]
fn truncated_boards_are_downloaded_from_the_raw_url() {
    let listener_base = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&listener_base);
    let (base, recorded) = mock_github(move |_, path| match path {
        "/gists/abc123/v1" => (
            200,
            gist_json(
                "abc123",
                &serde_json::json!({ BOARD_FILE_NAME: {
                    "content": "{\"tit", "truncated": true,
                    "raw_url": format!("{}/raw/board.json", shared.lock().unwrap())
                }}),
            ),
        ),
        "/raw/board.json" => (200, r#"{"title":"Long"}"#.to_owned()),
        _ => (404, String::new()),
    });
    base.clone_into(&mut listener_base.lock().unwrap());
    let client = Client::with_api_base("tok", &base);

    assert_eq!(
        client.board_at("abc123", "v1").unwrap(),
        r#"{"title":"Long"}"#
    );
    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded[1].path, "/raw/board.json");
    assert_eq!(recorded[1].authorization, None);
}

#[test]
fn failures_name_the_likely_cause() {
    let (base, _) = mock_github(|_, _| (401, r#"{"message":"Bad credentials"}"#.to_owned()));
    let client = Client::with_api_base("expired", &base);

    let error = format!("{:#}", client.push(None, "", "{}").unwrap_err());
    assert_eq!(
        error,
        "the token is invalid or expired (HTTP 401: Bad credentials)"
    );
}

#[test]
fn a_revision_without_the_board_file_is_an_error() {
    let (base, _) = mock_github(|_, _| (200, gist_json("abc123", &serde_json::json!({}))));
    let client = Client::with_api_base("tok", &base);

    let error = format!("{:#}", client.board_at("abc123", "v1").unwrap_err());
    assert!(error.contains(BOARD_FILE_NAME), "{error}");
}

#[test]
fn title_comes_from_the_gist_description() {
    let (base, _) = mock_github(|_, path| match path {
        "/gists/titled" => (
            200,
            r#"{"id":"titled","html_url":"x","description":"StorySquish board: Dune"}"#.to_owned(),
        ),
        _ => (
            200,
            r#"{"id":"untitled","html_url":"x","description":"StorySquish board"}"#.to_owned(),
        ),
    });
    let client = Client::with_api_base("tok", &base);

    assert_eq!(client.title("titled").unwrap(), "Dune");
    assert_eq!(client.title("untitled").unwrap(), "");
}
