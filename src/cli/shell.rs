use rein::runtime::ToolError;
use rig_core::completion::ToolDefinition;
use rig_core::message::ToolCall;
use serde_json::json;
use std::path::Path;

pub(super) const NAME: &str = "run_command";

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition::new(
        NAME.try_into().expect("constant name"),
        "Request approval to run a POSIX shell command from the workspace root. Returns exit status and bounded stdout/stderr. Keep commands focused; each needs fresh approval. Do not retry a denied command.",
        json!({
            "type": "object",
            "properties": {"command": {"type": "string", "maxLength": 4096}},
            "required": ["command"],
            "additionalProperties": false
        }),
    )
}

pub(super) struct Shell {
    #[cfg(unix)]
    root: std::path::PathBuf,
}

impl Shell {
    pub(super) fn new(root: &Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        return unix::open(root);
        #[cfg(not(unix))]
        {
            let _ = root;
            Err(std::io::Error::other(
                "--allow-shell requires a Unix terminal",
            ))
        }
    }

    pub(super) async fn execute(&self, call: &ToolCall) -> Result<String, ToolError> {
        #[cfg(unix)]
        return unix::execute(&self.root, call).await;
        #[cfg(not(unix))]
        {
            let _ = call;
            Err(ToolError::Conflict("Shell requires a Unix terminal"))
        }
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use serde::Deserialize;
    use std::fs::{File, OpenOptions};
    use std::io::{self, IsTerminal, Read, Write};
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::io::{AsyncRead, AsyncReadExt};
    use tokio::process::{Child, Command};

    // Even six-byte JSON escapes for every byte fit the runtime's 64 KB result limit.
    const OUTPUT_BYTES: usize = 4_096;
    const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
    const APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Arguments {
        command: String,
    }

    pub(super) fn open(root: &Path) -> io::Result<Shell> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal()
        {
            return Err(io::Error::other(
                "--allow-shell requires an interactive terminal",
            ));
        }
        terminal()?;
        Ok(Shell {
            root: root.to_owned(),
        })
    }

    pub(super) async fn execute(root: &Path, call: &ToolCall) -> Result<String, ToolError> {
        let invalid = || {
            ToolError::InvalidArguments(
                "run_command requires one nonempty command string of at most 4096 bytes without NUL",
            )
        };
        if call.function.name.as_str() != NAME || call.function.invalid_arguments.is_some() {
            return Err(invalid());
        }
        let Arguments { command } =
            serde_json::from_value(serde_json::Value::Object(call.function.arguments.clone()))
                .map_err(|_| invalid())?;
        if command.trim().is_empty() || command.len() > 4096 || command.contains('\0') {
            return Err(invalid());
        }

        match tokio::time::timeout(APPROVAL_TIMEOUT, approve(root, &command)).await {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => return Err(ToolError::Conflict("Command not run: approval denied")),
            Ok(Err(_)) => {
                return Err(ToolError::Conflict(
                    "Command not run: approval terminal unavailable",
                ));
            }
            Err(_) => return Err(ToolError::Conflict("Command not run: approval timed out")),
        }

        let mut process = Command::new("/bin/sh");
        process
            .args(["-c", &command])
            .current_dir(root)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true);
        // Keep toolchain discovery working without automatically passing provider keys.
        for name in [
            "PATH",
            "HOME",
            "TMPDIR",
            "LANG",
            "CARGO_HOME",
            "RUSTUP_HOME",
        ] {
            if let Some(value) = std::env::var_os(name) {
                process.env(name, value);
            }
        }
        let mut child = ProcessGroup(
            process
                .spawn()
                .map_err(|e| ToolError::Host(e.to_string()))?,
        );
        let stdout = child.0.stdout.take().expect("piped stdout");
        let stderr = child.0.stderr.take().expect("piped stderr");
        let result = tokio::time::timeout(COMMAND_TIMEOUT, async {
            // Drain both pipes before reaping; while pending the child still anchors its PGID.
            let (stdout, stderr) = tokio::try_join!(capture(stdout), capture(stderr))?;
            let status = child.0.wait().await?;
            Ok::<_, io::Error>((status, stdout, stderr))
        })
        .await;
        let (status, (stdout, stdout_truncated), (stderr, stderr_truncated)) = result
            .map_err(|_| ToolError::Conflict("Command timed out; effects may remain"))?
            .map_err(|e| ToolError::Host(e.to_string()))?;
        // Display local status even when the model misreports success. Never print raw command output.
        let _ = writeln!(io::stderr().lock(), "[command] {status}");
        Ok(json!({
            "success": status.success(),
            "exit_code": status.code(),
            "status": status.to_string(),
            "stdout": String::from_utf8_lossy(&stdout),
            "stderr": String::from_utf8_lossy(&stderr),
            "stdout_truncated": stdout_truncated,
            "stderr_truncated": stderr_truncated
        })
        .to_string())
    }

    fn terminal() -> io::Result<File> {
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open("/dev/tty")?;
        // SAFETY: the file owns a live tty descriptor; neither call borrows user memory.
        if unsafe { libc::tcgetpgrp(tty.as_raw_fd()) != libc::getpgrp() } {
            return Err(io::Error::other("Rein needs the foreground terminal"));
        }
        Ok(tty)
    }

    fn flush_input(tty: &File) -> io::Result<()> {
        // SAFETY: the descriptor remains open for the duration of the call.
        if unsafe { libc::tcflush(tty.as_raw_fd(), libc::TCIFLUSH) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    async fn approve(root: &Path, command: &str) -> io::Result<bool> {
        let mut tty = terminal()?;
        let mut random = [0; 4];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let code = format!("{:08x}", u32::from_ne_bytes(random));
        let expected = format!("run {code}\n");
        flush_input(&tty)?;
        writeln!(
            tty,
            "\n[approval required · not sandboxed]\nDirectory: \"{}\"\n/bin/sh -c \"{}\"\nRuns as your user; output is sent to the model. Only approve commands you trust.\nType run {code} to approve, or Enter to deny: ",
            root.to_string_lossy().escape_default(),
            command.escape_default()
        )?;
        let mut buffer = [0; 128];
        loop {
            match tty.read(&mut buffer) {
                Ok(n) => {
                    flush_input(&tty)?;
                    return Ok(&buffer[..n] == expected.as_bytes());
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No blocking worker owns the terminal after cancellation.
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn capture(mut stream: impl AsyncRead + Unpin) -> io::Result<(Vec<u8>, bool)> {
        let mut output = Vec::new();
        let mut truncated = false;
        let mut buffer = [0; 4096];
        loop {
            let n = stream.read(&mut buffer).await?;
            if n == 0 {
                return Ok((output, truncated));
            }
            let keep = n.min(OUTPUT_BYTES - output.len());
            output.extend_from_slice(&buffer[..keep]);
            truncated |= keep < n;
        }
    }

    struct ProcessGroup(Child);

    impl Drop for ProcessGroup {
        fn drop(&mut self) {
            if let Some(pid) = self.0.id() {
                // SAFETY: the unreaped child owns this process group; a negative PID addresses the group.
                unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
            }
            // shortcut: detached children can outlive the group; use a real sandbox for untrusted code.
        }
    }
}
