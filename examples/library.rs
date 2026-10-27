//! OPENAI_API_KEY=... cargo run --example library -- MODEL PROMPT
use rein::AgentSession;
use rein::provider::OpenAiBackend;
use rein::runtime::{self, RunOptions};
use rein::session::{ModelSelection, ToolCatalog, UserInput};
use rein::tools::echo::{self, Echo};
use rig_core::providers::openai::OpenAIConfig;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [model, prompt] = args.as_slice() else {
        return Err("usage: library MODEL PROMPT".into());
    };
    let backend = OpenAiBackend::new("openai", OpenAIConfig::from_env()?);
    let mut session = AgentSession::new(
        "example",
        ModelSelection {
            connection_id: "openai".into(),
            model_id: model.clone(),
        },
        ToolCatalog::new(vec![echo::definition()])?,
    )?;
    let options = RunOptions {
        allowed_tools: vec!["echo".into()],
        ..RunOptions::default()
    };
    let (_sender, mut controls) = mpsc::channel(16);
    runtime::run(
        &mut session,
        "example-run",
        UserInput {
            id: "input-1".into(),
            text: prompt.clone(),
        },
        &backend,
        &Echo,
        &options,
        &mut controls,
        &mut |event| {
            println!(
                "{}",
                serde_json::to_string(&event).expect("event serialization")
            );
        },
    )
    .await?;
    Ok(())
}
