use serde_json::Value;
use std::process::Command;

#[test]
fn offline_cli_runs_tool_loop_and_resumes_session() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("demo.json");
    for prompt in ["hello", "continue"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rein"))
            .args(["demo", path.to_str().unwrap(), prompt])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.first().unwrap()["event"], "started");
        assert_eq!(events.last().unwrap()["event"], "finished");
        assert_eq!(events.last().unwrap()["model_calls"], 2);
        assert!(
            events
                .iter()
                .any(|e| e["event"] == "tool_finished" && e["is_error"] == false)
        );
    }
    let session = rein::storage::load(path).unwrap();
    assert_eq!(session.transcript.entries().len(), 8);
}

#[test]
fn bad_arguments_and_corrupt_sessions_fail_without_overwrite() {
    let output = Command::new(env!("CARGO_BIN_EXE_rein"))
        .arg("invalid")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bad.json");
    std::fs::write(&path, "corrupt").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rein"))
        .args(["demo", path.to_str().unwrap(), "hello"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "corrupt");
}

#[test]
fn help_needs_no_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_rein"))
        .arg("--help")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("rein demo"));
}

#[test]
fn missing_credentials_do_not_create_a_session() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("live.json");
    let output = Command::new(env!("CARGO_BIN_EXE_rein"))
        .args(["run", path.to_str().unwrap(), "model", "hello"])
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!path.exists());
}

#[test]
fn mismatched_model_cannot_overwrite_existing_session() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_rein"))
            .args(["demo", path.to_str().unwrap(), "hello"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let before = std::fs::read(&path).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rein"))
        .args(["run", path.to_str().unwrap(), "other-model", "hello"])
        .env("OPENAI_API_KEY", "fixture-not-used")
        .env_remove("OPENAI_BASE_URL")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(before, std::fs::read(path).unwrap());
}
