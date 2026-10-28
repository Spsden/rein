#[path = "../evaluation/mod.rs"]
mod evaluation;

#[tokio::test]
async fn offline_agent_tasks_change_code_pass_independent_tests_and_recover_from_conflicts() {
    let directory = tempfile::tempdir().unwrap();
    let report_path = directory.path().join("report");
    let report = evaluation::run(&["offline".into(), report_path.to_string_lossy().into_owned()])
        .await
        .unwrap();
    assert_eq!(report.mode, "offline-script");
    assert_eq!((report.passed, report.total), (4, 4));
    assert!(
        report
            .cases
            .iter()
            .all(|case| case.baseline_failed && case.test_calls > 0)
    );
    let recovery = report
        .cases
        .iter()
        .find(|case| case.id == "boundary-recovery")
        .unwrap();
    assert_eq!(recovery.tool_errors, 1);
    assert_eq!(
        report
            .cases
            .iter()
            .find(|c| c.id == "arithmetic")
            .unwrap()
            .test_calls,
        2
    );
    let trace = std::fs::read_to_string(report_path.join("arithmetic/events.jsonl")).unwrap();
    let session = rein::storage::load(report_path.join("arithmetic/session.json")).unwrap();
    let checks: Vec<bool> = session
        .transcript
        .entries()
        .iter()
        .flat_map(|entry| match &entry.message {
            rig_core::completion::Message::User { content } => content
                .iter()
                .filter_map(|part| {
                    let rig_core::message::UserContent::ToolResult(result) = part else {
                        return None;
                    };
                    if result.name.as_str() != "run_tests" {
                        return None;
                    }
                    result.content.iter().find_map(|part| {
                        let rig_core::message::ToolResultContent::Text(text) = part else {
                            return None;
                        };
                        serde_json::from_str::<serde_json::Value>(&text.text)
                            .ok()?
                            .get("passed")?
                            .as_bool()
                    })
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(
        checks,
        [false, true],
        "the agent must receive a failing test result before its repair passes"
    );
    assert!(trace.contains("run_tests"));
    assert!(report_path.join("report.json").is_file());
    for case in &report.cases {
        let session = rein::storage::load(report_path.join(&case.id).join("session.json")).unwrap();
        assert!(session.active_run.is_none());
        assert!(rein::prepare_context(&session, "", 1_000_000).is_ok());
    }
}

#[tokio::test]
async fn evaluation_never_overwrites_existing_reports_or_implicitly_executes_live_code() {
    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().to_string_lossy().into_owned();
    assert!(
        evaluation::run(&["offline".into(), existing.clone()])
            .await
            .is_err()
    );
    assert!(
        evaluation::run(&["live".into(), "model".into(), existing])
            .await
            .is_err()
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn claiming_success_without_fixing_code_cannot_pass_the_task_oracle() {
    let directory = tempfile::tempdir().unwrap();
    let report_path = directory.path().join("negative");
    let report = evaluation::run(&[
        "negative-control".into(),
        report_path.to_string_lossy().into_owned(),
    ])
    .await
    .unwrap();
    assert_eq!(report.passed, 0);
    assert_eq!(report.total, 4);
    assert!(
        report
            .cases
            .iter()
            .all(|case| !case.passed && case.baseline_failed && case.test_calls == 0)
    );
}
