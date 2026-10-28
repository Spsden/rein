use super::*;
use rig_core::message::{ToolCall, ToolFunction};
use serde_json::{Value, json};

async fn invoke(tools: &WorkspaceTools, name: &str, args: Value) -> Result<Value, ToolError> {
    let call = ToolCall::from_wire("call", ToolFunction::new(name.try_into().unwrap(), args));
    tools
        .execute(&call)
        .await
        .map(|text| serde_json::from_str(&text).unwrap())
}

#[tokio::test]
async fn reads_searches_edits_and_creates_without_overwriting() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("main.rs"),
        "fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n",
    )
    .unwrap();
    let tools = WorkspaceTools::read_only(directory.path())
        .unwrap()
        .with_edits();
    assert_eq!(
        invoke(&tools, "list_files", json!({})).await.unwrap()["files"],
        json!(["main.rs"])
    );
    let found = invoke(&tools, "search", json!({"query":"a - b"}))
        .await
        .unwrap();
    assert_eq!(found["matches"][0]["line"], 2);
    let read = invoke(&tools, "read_file", json!({"path":"main.rs","max_lines":1}))
        .await
        .unwrap();
    assert_eq!(read["next_line"], 2);
    let read = invoke(
        &tools,
        "read_file",
        json!({"path":"main.rs","start_line":2}),
    )
    .await
    .unwrap();
    assert!(read["content"].as_str().unwrap().contains("2:     a - b"));
    invoke(
        &tools,
        "edit_file",
        json!({"path":"main.rs","old_text":"a - b","new_text":"a + b"}),
    )
    .await
    .unwrap();
    assert!(
        std::fs::read_to_string(directory.path().join("main.rs"))
            .unwrap()
            .contains("a + b")
    );
    invoke(
        &tools,
        "write_file",
        json!({"path":"test.txt","content":"नमस्ते"}),
    )
    .await
    .unwrap();
    assert!(matches!(
        invoke(
            &tools,
            "write_file",
            json!({"path":"test.txt","content":"overwrite"})
        )
        .await,
        Err(ToolError::Conflict(_))
    ));
    assert_eq!(
        std::fs::read_to_string(directory.path().join("test.txt")).unwrap(),
        "नमस्ते"
    );
    assert!(std::fs::read_dir(directory.path()).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".rein-edit")
    }));
}

#[tokio::test]
async fn read_only_policy_rejects_direct_write_calls_even_without_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let tools = WorkspaceTools::read_only(directory.path()).unwrap();
    assert_eq!(tools.definitions().len(), 3);
    assert!(
        invoke(
            &tools,
            "write_file",
            json!({"path":"new.txt","content":"x"})
        )
        .await
        .is_err()
    );
    assert!(!directory.path().join("new.txt").exists());
}

#[tokio::test]
async fn stale_ambiguous_and_overlapping_edits_preserve_original_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("file.txt");
    let tools = WorkspaceTools::read_only(directory.path())
        .unwrap()
        .with_edits();
    for (content, old) in [
        ("hello", "stale"),
        ("repeat repeat", "repeat"),
        ("aaaa", "aaa"),
        ("hello", ""),
    ] {
        std::fs::write(&path, content).unwrap();
        assert!(
            invoke(
                &tools,
                "edit_file",
                json!({"path":"file.txt","old_text":old,"new_text":"changed"})
            )
            .await
            .is_err(),
            "accepted ambiguous/stale match: {content} / {old}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }
}

#[tokio::test]
async fn rejects_traversal_hidden_files_schema_errors_and_binary_content() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join(".env"), "API_KEY=not-real").unwrap();
    std::fs::write(directory.path().join("binary"), [0, 1, 2]).unwrap();
    std::fs::create_dir(directory.path().join("target")).unwrap();
    std::fs::write(directory.path().join("target/generated"), "ignore").unwrap();
    let tools = WorkspaceTools::read_only(directory.path())
        .unwrap()
        .with_edits();
    for name in [
        "../outside",
        "/etc/passwd",
        ".env",
        "target/generated",
        "binary",
    ] {
        assert!(
            invoke(&tools, "read_file", json!({"path":name}))
                .await
                .is_err(),
            "read {name}"
        );
    }
    for args in [
        json!({}),
        json!({"path":3}),
        json!({"path":"binary","extra":"field"}),
    ] {
        assert!(invoke(&tools, "read_file", args).await.is_err());
    }
    let listed = invoke(&tools, "list_files", json!({}))
        .await
        .unwrap()
        .to_string();
    assert!(!listed.contains(".env") && !listed.contains("target"));
    assert!(invoke(&tools, "search", json!({"query":""})).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escapes_cannot_read_create_or_modify_outside_workspace() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "outside data").unwrap();
    symlink(outside.path(), directory.path().join("escape")).unwrap();
    symlink(
        outside.path().join("secret"),
        directory.path().join("alias"),
    )
    .unwrap();
    let tools = WorkspaceTools::read_only(directory.path())
        .unwrap()
        .with_edits();
    assert!(
        invoke(&tools, "read_file", json!({"path":"escape/secret"}))
            .await
            .is_err()
    );
    assert!(
        invoke(&tools, "read_file", json!({"path":"alias"}))
            .await
            .is_err()
    );
    assert!(
        invoke(
            &tools,
            "write_file",
            json!({"path":"escape/new","content":"bad"})
        )
        .await
        .is_err()
    );
    assert!(
        invoke(
            &tools,
            "edit_file",
            json!({"path":"alias","old_text":"outside data","new_text":"bad"})
        )
        .await
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("secret")).unwrap(),
        "outside data"
    );
    assert!(!outside.path().join("new").exists());
    assert_eq!(
        invoke(&tools, "list_files", json!({})).await.unwrap()["files"],
        json!([])
    );
}

#[tokio::test]
async fn oversized_files_and_output_are_bounded_and_report_truncation() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("large"),
        "x".repeat(MAX_FILE_BYTES + 1),
    )
    .unwrap();
    std::fs::write(directory.path().join("many.txt"), "match\n".repeat(150)).unwrap();
    let tools = WorkspaceTools::read_only(directory.path())
        .unwrap()
        .with_edits();
    assert!(
        invoke(&tools, "read_file", json!({"path":"large"}))
            .await
            .is_err()
    );
    assert!(
        invoke(
            &tools,
            "write_file",
            json!({"path":"new","content":"x".repeat(MAX_FILE_BYTES+1)})
        )
        .await
        .is_err()
    );
    assert!(!directory.path().join("new").exists());
    let result = invoke(&tools, "search", json!({"query":"match"}))
        .await
        .unwrap();
    assert_eq!(result["matches"].as_array().unwrap().len(), 100);
    assert_eq!(result["truncated"], true);
    assert!(result.to_string().len() < MAX_OUTPUT_BYTES);
}

#[cfg(unix)]
#[tokio::test]
async fn edits_preserve_executable_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("script.sh");
    std::fs::write(&path, "echo old\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let tools = WorkspaceTools::read_only(directory.path())
        .unwrap()
        .with_edits();
    invoke(
        &tools,
        "edit_file",
        json!({"path":"script.sh","old_text":"old","new_text":"new"}),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o755
    );
}
