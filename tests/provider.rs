//! Exercise the real Rig HTTP/SSE boundary without credentials or paid calls.
use rein::AgentSession;
use rein::provider::OpenAiBackend;
use rein::runtime::{self, Event, ModelBackend, RunError, RunOptions};
use rein::session::{ModelSelection, ToolCatalog, UserInput};
use rein::tools::echo::{self, Echo};
use rig_core::completion::CompletionRequest;
use rig_core::providers::openai::OpenAIConfig;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

fn fixture(responses: Vec<(u16, String)>) -> (OpenAiBackend, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut requests = vec![];
        for (status, body) in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "fixture timed out waiting for request"
                        );
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            requests.push(read_request(&mut stream));
            let kind = if status == 200 {
                "text/event-stream"
            } else {
                "application/json"
            };
            write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    let mut config = OpenAIConfig::new("fixture-key-not-a-secret");
    config.base_url = endpoint;
    (OpenAiBackend::new("fixture", config), server)
}

fn read_request(stream: &mut TcpStream) -> Value {
    let mut bytes = vec![];
    let end = loop {
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "unexpected request EOF");
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 1_000_000);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..end]);
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() - end < length {
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
    }
    serde_json::from_slice(&bytes[end..end + length]).unwrap()
}

fn sse(delta: Value, reason: &str) -> String {
    let chunk = json!({"id":"fixture-response", "object":"chat.completion.chunk", "created":1,
        "model":"fixture-model", "choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let end = json!({"id":"fixture-response", "object":"chat.completion.chunk", "created":1,
        "model":"fixture-model", "choices":[{"index":0,"delta":{},"finish_reason":reason}]});
    let usage = json!({"id":"fixture-response", "object":"chat.completion.chunk", "created":1,
        "model":"fixture-model", "choices":[], "usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}});
    format!("data: {chunk}\n\ndata: {end}\n\ndata: {usage}\n\ndata: [DONE]\n\n")
}

#[tokio::test]
async fn real_rig_streams_tools_and_replays_correlated_results() {
    let tool = sse(
        json!({"role":"assistant", "tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"echo","arguments":"{\"text\":\"hello\"}"}}]}),
        "tool_calls",
    );
    let text = sse(json!({"role":"assistant","content":"Hello back"}), "stop");
    let (backend, server) = fixture(vec![(200, tool), (200, text)]);
    let mut session = AgentSession::new(
        "fixture",
        ModelSelection {
            connection_id: "fixture".into(),
            model_id: "fixture-model".into(),
        },
        ToolCatalog::new(vec![echo::definition()]).unwrap(),
    )
    .unwrap();
    let options = RunOptions {
        allowed_tools: vec!["echo".into()],
        ..RunOptions::default()
    };
    let (_tx, mut rx) = mpsc::channel(8);
    let mut events = vec![];
    let result = runtime::run(
        &mut session,
        "fixture-run",
        UserInput {
            id: "input".into(),
            text: "hello".into(),
        },
        &backend,
        &Echo,
        &options,
        &mut rx,
        &mut |event| events.push(event),
    )
    .await;
    let requests = server.join().unwrap();
    let result = result.unwrap();
    assert_eq!(result.model_calls, 2);
    assert_eq!(result.completion_tokens, Some(6));
    assert!(events.iter().any(|e| matches!(e, Event::Stream { item: rig_core::streaming::Item::Event(rig_core::streaming::StreamEvent::Text { text, .. }) } if text == "Hello back")));
    assert_eq!(requests[0]["model"], "fixture-model");
    assert_eq!(requests[0]["stream"], true);
    let messages = requests[1]["messages"].as_array().unwrap();
    assert!(
        messages.iter().any(|m| m["role"] == "tool"
            && m["tool_call_id"] == "call-1"
            && m["content"] == "hello")
    );
    assert!(rein::prepare_context(&session, "", 100_000).is_ok());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("provider.json");
    rein::storage::save(&session, &path).unwrap();
    let loaded = rein::storage::load(path).unwrap();
    for (before, after) in session
        .transcript
        .entries()
        .iter()
        .zip(loaded.transcript.entries())
    {
        assert_eq!(before.id, after.id);
        assert_eq!(before.message, after.message);
    }
}

#[tokio::test]
async fn provider_errors_truncated_streams_and_stream_limits_fail_closed() {
    let partial = "data: {\"id\":\"f\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"partial\"},\"finish_reason\":null}]}\n\n";
    for (status, body, limit, expected) in [
        (
            401,
            json!({"error":{"message":"sensitive-provider-message","type":"invalid_api_key"}})
                .to_string(),
            100_000,
            RunError::Model,
        ),
        (200, partial.into(), 100_000, RunError::Model),
        (
            200,
            sse(json!({"role":"assistant","content":"hello"}), "stop"),
            1,
            RunError::ResponseTooLarge,
        ),
    ] {
        let (backend, server) = fixture(vec![(status, body)]);
        let mut request = CompletionRequest::new("hello");
        request.model = Some("fixture-model".into());
        let mut events = vec![];
        let result = backend
            .complete("fixture", request, limit, &mut |event| events.push(event))
            .await;
        server.join().unwrap();
        assert_eq!(result.unwrap_err(), expected);
        assert!(
            !serde_json::to_string(&events)
                .unwrap()
                .contains("sensitive-provider-message")
        );
    }
}

#[tokio::test]
async fn connection_mismatch_is_rejected_before_network_io() {
    let backend = OpenAiBackend::new("one", OpenAIConfig::new("unused"));
    assert_eq!(
        backend
            .complete("other", CompletionRequest::new("hello"), 1000, &mut |_| {})
            .await
            .unwrap_err(),
        RunError::Model
    );
}
