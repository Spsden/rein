use super::{
    display::{Display, safe},
    session::{SessionHost, valid_prompt},
};
use rein::runtime::{ModelBackend, RunError};
use rig_core::message::{Message, UserContent};
use rustyline::{Config, DefaultEditor, error::ReadlineError};
use std::error::Error;
use std::io::{self, Write};

const HELP: &str = "/help     Show commands
/status   Show session, model, workspace and history size
/paste    Enter multiple lines; /end sends, Ctrl-C discards
/exit     Save and exit (also /quit or Ctrl-D)
Use // to send a prompt beginning with /, e.g. //help.
Up/Down browse previous prompts; normal cursor editing is available.
Ctrl-C during a run cancels it and returns here. Edits already made remain.
Tools can read and edit workspace files; shell/test execution is NOT available.";

pub(super) async fn run(
    mut host: SessionHost,
    backend: impl ModelBackend,
) -> Result<(), Box<dyn Error>> {
    let config = Config::builder().max_history_size(100)?.build();
    let mut editor = DefaultEditor::with_config(config)?;
    for entry in host.session.transcript.entries() {
        if entry.source == rein::session::EntrySource::User
            && let Message::User { content } = &entry.message
        {
            let text: String = content
                .iter()
                .filter_map(|part| match part {
                    UserContent::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect();
            editor.add_history_entry(history_entry(&text))?;
        }
    }
    println!(
        "Rein · interactive chat\nType /help for commands. Streamed answers are provisional until the turn succeeds."
    );
    status(&host);
    if host.workspace.is_none() {
        println!("Offline deterministic echo demo — not an LLM.");
    }
    loop {
        let (next_editor, line) = read_line(editor, "you > ").await?;
        editor = next_editor;
        let line = match line {
            Ok(line) => line,
            Err(ReadlineError::Eof) => break,
            Err(ReadlineError::Interrupted) => {
                println!("Input cleared. /exit to quit.");
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let mut prompt = line;
        match prompt.trim() {
            "" => continue,
            "/exit" | "/quit" => break,
            "/help" => {
                println!("{HELP}");
                continue;
            }
            "/status" => {
                status(&host);
                continue;
            }
            "/paste" => {
                println!("Paste text; /end on its own line sends. Ctrl-C discards.");
                prompt.clear();
                let mut oversized = false;
                loop {
                    let (next_editor, line) = read_line(editor, "... > ").await?;
                    editor = next_editor;
                    match line {
                        Ok(line) if line == "/end" => break,
                        Ok(line) if !oversized => {
                            prompt.push_str(&line);
                            prompt.push('\n');
                        }
                        Ok(_) => {}
                        Err(ReadlineError::Interrupted) => {
                            prompt.clear();
                            println!("Paste discarded.");
                            break;
                        }
                        Err(ReadlineError::Eof) => {
                            println!("Paste discarded.");
                            return Ok(());
                        }
                        Err(error) => return Err(error.into()),
                    }
                    if prompt.len() > 1_000_000 {
                        prompt.clear();
                        oversized = true;
                        println!("Paste exceeds 1 MB; discarding through /end.");
                    }
                }
                if prompt.trim().is_empty() {
                    continue;
                }
            }
            _ if prompt.starts_with("//") => {
                prompt.remove(0);
            }
            _ if prompt.starts_with('/') => {
                println!("Unknown command. Type /help, or use // for a literal /.");
                continue;
            }
            _ => {}
        }
        if !valid_prompt(&prompt) {
            println!("Prompt must be nonempty and at most 1 MB.");
            continue;
        }
        editor.add_history_entry(history_entry(&prompt))?;
        let mut stdout = io::stdout().lock();
        let mut display = Display::new(&mut stdout);
        let result = host
            .turn(&backend, &prompt, &mut |event| display.event(event))
            .await;
        display.finish(&host.session, result.as_ref().ok())?;
        match result {
            Ok(_) => {}
            Err(error) if error.is::<RunError>() => {
                writeln!(stdout, "[turn stopped: {error} · session saved]")?;
                writeln!(
                    stdout,
                    "Any unfinished streamed answer was not accepted. Completed file edits remain.\n"
                )?;
            }
            Err(error) => return Err(error),
        }
    }
    println!("Session saved. Goodbye.");
    Ok(())
}

// Move the editor to a blocking worker only while reading, never during a model run.
async fn read_line(
    mut editor: DefaultEditor,
    prompt: &'static str,
) -> Result<(DefaultEditor, Result<String, ReadlineError>), Box<dyn Error>> {
    Ok(tokio::task::spawn_blocking(move || {
        let line = editor.readline(prompt);
        (editor, line)
    })
    .await?)
}

fn status(host: &SessionHost) {
    println!(
        "Model: {}\nSession: {}\nHistory: {} accepted entries",
        safe(&host.session.model.model_id),
        safe(&host.path.display().to_string()),
        host.session.transcript.entries().len()
    );
    if let Some(root) = &host.workspace {
        println!(
            "Workspace: {} (reads + edits; no shell)",
            safe(&root.display().to_string())
        );
    }
}

fn history_entry(prompt: &str) -> String {
    if prompt.starts_with('/') {
        format!("/{prompt}")
    } else {
        prompt.into()
    }
}
