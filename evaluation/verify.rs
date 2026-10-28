use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[derive(Serialize)]
pub(super) struct Verification {
    pub passed: bool,
    pub stage: &'static str,
    pub diagnostics: String,
}

pub(super) async fn verify(
    directory: &Path,
    sandboxed: bool,
) -> Result<Verification, Box<dyn std::error::Error>> {
    let mut compile = Command::new("rustc");
    compile
        .arg(directory.join("oracle.rs"))
        .args(["--edition=2024", "--test", "-o"])
        .arg(directory.join("test-bin"));
    let compile = super::sandbox::wrap(compile, directory, true, sandboxed)?;
    let (passed, diagnostics) = command(compile, Duration::from_secs(30), directory).await?;
    if !passed {
        return Ok(Verification {
            passed,
            stage: "compile",
            diagnostics,
        });
    }
    let mut tests = Command::new(directory.join("test-bin"));
    tests.arg("--test-threads=1");
    let tests = super::sandbox::wrap(tests, directory, false, sandboxed)?;
    let (passed, diagnostics) = command(tests, Duration::from_secs(5), directory).await?;
    Ok(Verification {
        passed,
        stage: "tests",
        diagnostics,
    })
}

async fn command(
    mut command: Command,
    deadline: Duration,
    directory: &Path,
) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let sdk_root = command
        .as_std()
        .get_envs()
        .find(|(name, _)| *name == "SDKROOT")
        .and_then(|(_, value)| value.map(std::ffi::OsStr::to_owned));
    command
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Toolchain discovery/linking needs these; provider keys and other credentials do not.
    for name in [
        "PATH",
        "HOME",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "SystemRoot",
        "TMPDIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let temporary = directory.join("tmp");
    if let Some(sdk) = sdk_root {
        command.env("SDKROOT", sdk);
    }
    std::fs::create_dir_all(&temporary)?;
    command
        .env("TMPDIR", &temporary)
        .env("TMP", &temporary)
        .env("TEMP", &temporary);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let execution = async move {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut stdout = stdout.take(8_000);
        let mut stderr = stderr.take(8_000);
        let (status, out_result, err_result) = tokio::join!(
            child.wait(),
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err)
        );
        out_result?;
        err_result?;
        let status = status?;
        Ok::<_, std::io::Error>((
            status.success(),
            format!(
                "{status}\n{}{}",
                String::from_utf8_lossy(&out),
                String::from_utf8_lossy(&err)
            ),
        ))
    };
    match tokio::time::timeout(deadline, execution).await {
        Ok(result) => Ok(result?),
        Err(_) => Ok((
            false,
            "verification timed out; direct child terminated".into(),
        )),
    }
}
