mod demo;
use rein::provider::OpenAiBackend;
use rein::runtime::{self, Control, ModelBackend, RunOptions};
use rein::session::{ModelSelection, ToolCatalog, UserInput};
use rein::tools::echo;
use rein::{AgentSession, storage};
use rig_core::providers::openai::OpenAIConfig;
use std::error::Error;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

const HELP: &str = "Rein — a small Rust agent harness\n\n  rein demo SESSION.json PROMPT\n  rein run SESSION.json MODEL PROMPT\n\nResume by reusing the same session path. Quote PROMPT as one argument.\nrun uses OPENAI_API_KEY and optional OPENAI_BASE_URL (Chat Completions).\nOutput is JSON Lines; Ctrl-C cancels the run. Only echo is authorized.\nSession files contain conversation data. Use one process per session path.";

pub async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("{HELP}");
        return Ok(());
    }
    match args.as_slice() {
        [command, path, prompt] if command == "demo" => {
            execute(path, "demo", "demo", prompt, &demo::Demo).await
        }
        [command, path, model, prompt] if command == "run" => {
            let config = OpenAIConfig::from_env()
                .map_err(|_| "configure OPENAI_API_KEY and a valid OPENAI_BASE_URL")?;
            execute(
                path,
                "openai",
                model,
                prompt,
                &OpenAiBackend::new("openai", config),
            )
            .await
        }
        _ => Err(format!("invalid arguments\n{HELP}").into()),
    }
}

async fn execute(
    path: &str,
    connection: &str,
    model: &str,
    prompt: &str,
    backend: &impl ModelBackend,
) -> Result<(), Box<dyn Error>> {
    if prompt.trim().is_empty() || prompt.len() > 1_000_000 || model.trim().is_empty() {
        return Err("prompt/model is empty or prompt exceeds 1 MB".into());
    }
    let path = Path::new(path);
    let mut session = match storage::load(path) {
        Ok(session) => session,
        Err(storage::StorageError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            AgentSession::new(
                format!("session-{}", nonce()),
                ModelSelection {
                    connection_id: connection.into(),
                    model_id: model.into(),
                },
                ToolCatalog::default(),
            )?
        }
        Err(error) => return Err(error.into()),
    };
    if session.model.connection_id != connection || session.model.model_id != model {
        return Err(
            "session model/connection differs; use its original model or a new session path".into(),
        );
    }
    // Rebuild capabilities from the host, not a potentially edited snapshot.
    session.tools = ToolCatalog::new(vec![echo::definition()])?;
    storage::save(&session, path)?;
    let options = RunOptions {
        allowed_tools: vec!["echo".into()],
        ..RunOptions::default()
    };
    let (sender, mut controls) = mpsc::channel(16);
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = sender.send(Control::Cancel).await;
        }
    });
    let mut output_error = None;
    let mut stdout = std::io::stdout().lock();
    let mut emit = |event| {
        if output_error.is_none() {
            let result = serde_json::to_writer(&mut stdout, &event)
                .map_err(std::io::Error::other)
                .and_then(|_| stdout.write_all(b"\n"))
                .and_then(|_| stdout.flush());
            if let Err(error) = result {
                output_error = Some(error);
            }
        }
    };
    let id = nonce();
    let result = runtime::run(
        &mut session,
        &format!("run-{id}"),
        UserInput {
            id: format!("input-{id}"),
            text: prompt.into(),
        },
        backend,
        &echo::Echo,
        &options,
        &mut controls,
        &mut emit,
    )
    .await;
    signal.abort();
    // Save even on cancellation/failure; a storage error must not be reported as success.
    storage::save(&session, path)?;
    if let Some(error) = output_error {
        return Err(error.into());
    }
    result?;
    Ok(())
}

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
