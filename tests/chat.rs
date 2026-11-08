//! The interactive host also accepts piped input, so conversation tests need no TTY or key.
use rein::session::EntrySource;
use rig_core::message::{Message, UserContent};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn chat(path: &Path, input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rein"))
        .args(["chat-demo", path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("interactive CLI hung");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}

fn prompts(path: &Path) -> Vec<String> {
    rein::storage::load(path)
        .unwrap()
        .transcript
        .entries()
        .iter()
        .filter(|entry| entry.source == EntrySource::User)
        .map(|entry| match &entry.message {
            Message::User { content } => content
                .iter()
                .filter_map(|part| match part {
                    UserContent::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect(),
            _ => panic!("user entry must contain a user message"),
        })
        .collect()
}

#[test]
fn interactive_conversation_commands_paste_and_resume() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("chat.json");
    let output = chat(
        &path,
        "\n/help\n/status\n/unknown\nhello\n/paste\nline one\nline two\n/end\n//help\n/exit\nnever submitted\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Unknown command"));
    assert!(text.contains("[tool] echo"));
    assert_eq!(text.matches("rein > Demo complete:").count(), 3);
    assert_eq!(text.matches("session saved]").count(), 3);
    assert!(!text.contains("\"event\":"));
    assert_eq!(prompts(&path), ["hello", "line one\nline two\n", "/help"]);

    let output = chat(&path, "continue\n"); // EOF saves and exits without /exit.
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("History: 12 accepted entries"));
    assert_eq!(prompts(&path).last().unwrap(), "continue");
    assert!(rein::storage::load(&path).unwrap().active_run.is_none());
}

#[test]
fn eof_during_paste_discards_incomplete_input() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("chat.json");
    let output = chat(&path, "/paste\nnot submitted\n");
    assert!(output.status.success());
    assert!(prompts(&path).is_empty());
}

#[test]
fn oversized_paste_discards_remaining_lines_instead_of_executing_them() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("chat.json");
    let input = format!(
        "/paste\n{}\naccidental task\n/end\nhello\n/exit\n",
        "x".repeat(1_000_001)
    );
    let output = chat(&path, &input);
    assert!(output.status.success());
    assert_eq!(prompts(&path), ["hello"]);
}

#[test]
fn chat_rejects_corrupt_sessions_without_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("chat.json");
    std::fs::write(&path, "corrupt").unwrap();
    let output = chat(&path, "");
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "corrupt");
}

#[test]
fn chat_requires_its_explicit_provider_key_before_creating_session() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("chat.json");
    let output = Command::new(env!("CARGO_BIN_EXE_rein"))
        .args([
            "chat",
            path.to_str().unwrap(),
            "model",
            directory.path().to_str().unwrap(),
        ])
        .env_remove("OPENCODE_GO_API_KEY")
        .env("OPENAI_API_KEY", "not-for-go")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("OPENCODE_GO_API_KEY"));
    assert!(!path.exists());
}

#[test]
fn shell_opt_in_cannot_be_used_with_piped_input() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("chat.json");
    let output = Command::new(env!("CARGO_BIN_EXE_rein"))
        .args([
            "chat-openai",
            path.to_str().unwrap(),
            "model",
            directory.path().to_str().unwrap(),
            "--allow-shell",
        ])
        .env("OPENAI_API_KEY", "fixture")
        .env("OPENAI_BASE_URL", "http://127.0.0.1:9/v1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires"));
    assert!(!path.exists());
}

#[test]
fn failed_turn_recovers_and_real_streaming_tools_preserve_conversation() {
    use serde_json::{Value, json};
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("note.txt"), "before\n").unwrap();
    let path = directory.path().join("chat.json");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let sse = |delta: Value, reason: &str| {
        let chunk = json!({"id":"fixture", "model":"fixture", "choices":[{"index":0,"delta":delta,"finish_reason":null}]});
        let end = json!({"id":"fixture", "model":"fixture", "choices":[{"index":0,"delta":{},"finish_reason":reason}]});
        let usage = json!({"id":"fixture", "model":"fixture", "choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}});
        format!("data: {chunk}\n\ndata: {end}\n\ndata: {usage}\n\ndata: [DONE]\n\n")
    };
    let tool = |id: &str, name: &str, args: Value| json!({"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]});
    let responses = vec![
        (
            401,
            json!({"error":{"message":"sensitive-fixture-detail","type":"invalid_api_key"}})
                .to_string(),
        ),
        (
            200,
            sse(
                tool("read", "read_file", json!({"path":"note.txt"})),
                "tool_calls",
            ),
        ),
        (
            200,
            sse(
                tool(
                    "edit",
                    "edit_file",
                    json!({"path":"note.txt","old_text":"before","new_text":"after"}),
                ),
                "tool_calls",
            ),
        ),
        (
            200,
            sse(
                json!({"role":"assistant","content":"Fixed\u{001b}[2J the file."}),
                "stop",
            ),
        ),
        (
            200,
            sse(
                json!({"role":"assistant","content":"The file now says after."}),
                "stop",
            ),
        ),
    ];
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        let mut identities = Vec::new();
        for (status, body) in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing fixture request");
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                    if name.eq_ignore_ascii_case("x-opencode-session") {
                        identities.push(value.trim().to_owned());
                    }
                }
            }
            assert!(length > 0 && length < 1_000_000);
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            requests.push(serde_json::from_slice::<Value>(&bytes).unwrap());
            let kind = if status == 200 {
                "text/event-stream"
            } else {
                "application/json"
            };
            write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        (requests, identities)
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_rein"))
        .args([
            "chat-openai",
            path.to_str().unwrap(),
            "fixture",
            workspace.to_str().unwrap(),
        ])
        .env("OPENAI_API_KEY", "fixture-not-a-secret")
        .env("OPENAI_BASE_URL", endpoint)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"first fails\nfix the file\nwhat changed\n/exit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let (requests, identities) = server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("turn stopped: Model"));
    assert!(text.contains("[tool] read_file"));
    assert!(text.contains("[tool] edit_file"));
    assert_eq!(text.matches("rein > Fixed[2J the file.").count(), 1);
    assert!(text.contains("rein > The file now says after."));
    assert!(!text.contains('\x1b'));
    assert!(!text.contains("sensitive-fixture-detail"));
    assert!(!text.contains("fixture-not-a-secret"));
    assert_eq!(
        std::fs::read_to_string(workspace.join("note.txt")).unwrap(),
        "after\n"
    );
    assert_eq!(
        prompts(&path),
        ["first fails", "fix the file", "what changed"]
    );
    assert_eq!(identities.len(), 5);
    assert!(identities.iter().all(|id| id == &identities[0]));
    let messages = requests.last().unwrap()["messages"].as_array().unwrap();
    assert!(
        messages
            .iter()
            .any(|m| m["role"] == "assistant" && m["content"] == "Fixed\u{001b}[2J the file.")
    );
    assert!(
        messages
            .iter()
            .any(|m| m["role"] == "tool" && m["tool_call_id"] == "edit")
    );
    assert!(rein::prepare_context(&rein::storage::load(&path).unwrap(), "", 100_000).is_ok());
}
