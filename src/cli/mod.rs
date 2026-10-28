mod chat;
mod demo;
mod display;
mod host;
mod session;

use rein::provider::OpenAiBackend;
use rein::runtime::ModelBackend;
use rig_core::providers::openai::OpenAIConfig;
use std::error::Error;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

const HELP: &str = "Rein — a small Rust agent harness

Interactive (readable output, editing/history, saved conversation):
  rein chat SESSION.json MODEL WORKSPACE         OpenCode Go
  rein chat-openai SESSION.json MODEL WORKSPACE  OpenAI-compatible
  rein chat-demo SESSION.json                   Offline echo demo, not an LLM

One task (JSON Lines output):
  rein demo SESSION.json PROMPT
  rein run SESSION.json MODEL PROMPT
  rein go SESSION.json MODEL PROMPT
  rein code SESSION.json MODEL WORKSPACE PROMPT
  rein go-code SESSION.json MODEL WORKSPACE PROMPT

Resume by reusing the same session path and settings. Quote PROMPT as one argument.
run/code/chat-openai use OPENAI_API_KEY and optional OPENAI_BASE_URL (Chat Completions).
go/go-code/chat use OPENCODE_GO_API_KEY. Credentials must be in the environment.
Workspace modes authorize reads and edits, but NO command execution.
Other modes grant only echo. Chat commands: /help, /status, /paste, /exit.
Ctrl-C cancels a running turn; chat stays open. Ctrl-D exits at the prompt.
Session files contain conversation data. Use one process per session and workspace.";

pub async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("{HELP}");
        return Ok(());
    }
    match args.as_slice() {
        [command, path] if command == "chat-demo" => {
            execute(path, "demo", "demo", None, None, |_, _| Ok(demo::Demo)).await
        }
        [command, path, model, root] if command == "chat" || command == "chat-openai" => {
            let (connection, config) = provider_config(command == "chat")?;
            execute(
                path,
                connection,
                model,
                None,
                Some(root),
                |connection, id| Ok(OpenAiBackend::for_session(connection, config, id)?),
            )
            .await
        }
        [command, path, prompt] if command == "demo" => {
            execute(path, "demo", "demo", Some(prompt), None, |_, _| {
                Ok(demo::Demo)
            })
            .await
        }
        [command, path, model, prompt] if command == "run" || command == "go" => {
            let (connection, config) = provider_config(command == "go")?;
            execute(
                path,
                connection,
                model,
                Some(prompt),
                None,
                |connection, id| Ok(OpenAiBackend::for_session(connection, config, id)?),
            )
            .await
        }
        [command, path, model, root, prompt] if command == "code" || command == "go-code" => {
            let (connection, config) = provider_config(command == "go-code")?;
            execute(
                path,
                connection,
                model,
                Some(prompt),
                Some(root),
                |connection, id| Ok(OpenAiBackend::for_session(connection, config, id)?),
            )
            .await
        }
        _ => Err(format!("invalid arguments\n{HELP}").into()),
    }
}

fn provider_config(go: bool) -> Result<(&'static str, OpenAIConfig), Box<dyn Error>> {
    if go {
        let key =
            std::env::var("OPENCODE_GO_API_KEY").map_err(|_| "configure OPENCODE_GO_API_KEY")?;
        if key.trim().is_empty() {
            return Err("OPENCODE_GO_API_KEY is empty".into());
        }
        let mut config = OpenAIConfig::new(key);
        config.base_url = "https://opencode.ai/inference/go/openai/v1".into();
        Ok(("opencode-go", config))
    } else {
        Ok((
            "openai",
            OpenAIConfig::from_env()
                .map_err(|_| "configure OPENAI_API_KEY and a valid OPENAI_BASE_URL")?,
        ))
    }
}

async fn execute<B: ModelBackend>(
    path: &str,
    connection: &str,
    model: &str,
    prompt: Option<&str>,
    workspace: Option<&str>,
    make_backend: impl FnOnce(&str, &str) -> Result<B, Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    if model.trim().is_empty() || prompt.is_some_and(|p| !session::valid_prompt(p)) {
        return Err("prompt/model is empty or prompt exceeds 1 MB".into());
    }
    let mut host = session::SessionHost::open(path, connection, model, workspace)?;
    let backend = make_backend(&host.session.model.connection_id, host.session.id())?;
    if let Some(prompt) = prompt {
        let mut stdout = std::io::stdout().lock();
        host.turn(&backend, prompt, &mut |event| {
            serde_json::to_writer(&mut stdout, &event).map_err(std::io::Error::other)?;
            stdout.write_all(b"\n")?;
            stdout.flush()
        })
        .await?;
    } else {
        chat::run(host, backend).await?;
    }
    Ok(())
}

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
