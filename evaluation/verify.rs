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

pub(super) async fn verify(directory: &Path) -> Result<Verification, Box<dyn std::error::Error>> {
    let mut compile = Command::new("rustc");
    compile
        .arg(directory.join("oracle.rs"))
        .args(["--edition=2024", "--test", "-o"])
        .arg(directory.join("test-bin"));
    let (passed, diagnostics) = command(compile, Duration::from_secs(30)).await?;
    if !passed {
        return Ok(Verification {
            passed,
            stage: "compile",
            diagnostics,
        });
    }
    let mut tests = Command::new(directory.join("test-bin"));
    tests.arg("--test-threads=1");
    let (passed, diagnostics) = command(tests, Duration::from_secs(5)).await?;
    Ok(Verification {
        passed,
        stage: "tests",
        diagnostics,
    })
}

async fn command(
    mut command: Command,
    deadline: Duration,
) -> Result<(bool, String), Box<dyn std::error::Error>> {
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
        Ok::<_, std::io::Error>((
            status?.success(),
            format!(
                "{}{}",
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
