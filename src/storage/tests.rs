use super::*;
use crate::memory::WorkingNote;
use crate::session::{RunBudget, RunPhase, RunState};

#[test]
fn roundtrip_preserves_memory_evidence_queues_and_entry_ids() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let mut session = crate::test_support::session("saved");
    session.tools = ToolCatalog::new(vec![crate::tools::echo::definition()]).unwrap();
    let id = session
        .transcript
        .append(Message::user("नमस्ते"), EntrySource::User);
    session.working_memory.progress.push(WorkingNote {
        text: "started".into(),
        evidence: vec![id],
    });
    session.followups.push_back(UserInput {
        id: "next".into(),
        text: "continue".into(),
    });
    save(&session, &path).unwrap();
    let mut restored = load(&path).unwrap();
    assert_eq!(restored.id(), "saved");
    assert_eq!(restored.transcript.entries()[0].id, id);
    assert_eq!(restored.working_memory.progress[0].evidence, vec![id]);
    assert_eq!(restored.followups.pop_front().unwrap().id, "next");
    assert!(restored.tools.find("echo").is_some());
    assert!(restored.active_run.is_none());
    assert_eq!(
        restored
            .transcript
            .append(Message::user("next"), EntrySource::User)
            .value(),
        2
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn invalid_new_state_never_overwrites_good_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let mut session = crate::test_support::session("saved");
    save(&session, &path).unwrap();
    let original = std::fs::read(&path).unwrap();
    session.model.model_id.clear();
    assert!(save(&session, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(load(&path).is_ok());
}

#[test]
fn rejects_corruption_versions_fields_model_and_queue_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    save(&crate::test_support::session("saved"), &path).unwrap();
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for mode in 0..6 {
        let mut value = original.clone();
        match mode {
            0 => value["version"] = 2.into(),
            1 => value["rig_version"] = "old".into(),
            2 => value["unexpected"] = true.into(),
            3 => value["model"]["model_id"] = "".into(),
            4 => value["followups"] = serde_json::json!([{"id":"", "text":"x"}]),
            _ => value["id"] = " ".into(),
        }
        assert!(matches!(
            decode(&serde_json::to_vec(&value).unwrap()),
            Err(StorageError::InvalidSnapshot)
        ));
    }
    assert!(decode(b"{broken").is_err());
    assert!(decode(b"").is_err());
}

#[test]
fn active_run_and_dangling_evidence_cannot_be_saved() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let mut session = crate::test_support::session("saved");
    session.active_run = Some(RunState {
        id: "r".into(),
        phase: RunPhase::CallingModel,
        budget: RunBudget::default(),
        model_calls: 0,
        completion_tokens: None,
        pending_tools: vec![],
        draft_response: String::new(),
    });
    assert!(matches!(
        save(&session, &path),
        Err(StorageError::ActiveRun)
    ));
    session.active_run = None;
    let mut other = crate::test_support::session("other");
    let id = other
        .transcript
        .append(Message::user("x"), EntrySource::User);
    session.working_memory.progress.push(WorkingNote {
        text: "bad".into(),
        evidence: vec![id],
    });
    assert!(matches!(
        save(&session, &path),
        Err(StorageError::InvalidSnapshot)
    ));
    assert!(!path.exists());
}

#[test]
fn oversized_file_and_missing_parent_are_explicit_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.json");
    File::create(&path)
        .unwrap()
        .set_len(MAX_SNAPSHOT_BYTES as u64 + 1)
        .unwrap();
    assert!(matches!(load(&path), Err(StorageError::TooLarge)));
    assert!(matches!(
        save(
            &crate::test_support::session("saved"),
            directory.path().join("missing/session.json")
        ),
        Err(StorageError::Io(_))
    ));
}
