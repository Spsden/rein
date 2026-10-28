//! Optional macOS subprocess restrictions for the synthetic task evaluator.
use std::io;
use std::path::Path;
use tokio::process::Command;

pub(super) fn wrap(
    command: Command,
    directory: &Path,
    compile: bool,
    enabled: bool,
) -> io::Result<Command> {
    if !enabled {
        return Ok(command);
    }
    restrict(command, directory, compile)
}

#[cfg(not(target_os = "macos"))]
fn restrict(_: Command, _: &Path, _: bool) -> io::Result<Command> {
    Err(io::Error::other(
        "--macos-sandbox requires macOS; use an external sandbox on this platform",
    ))
}

#[cfg(target_os = "macos")]
fn restrict(command: Command, directory: &Path, compile: bool) -> io::Result<Command> {
    use std::sync::OnceLock;
    struct Toolchain {
        compiler: std::path::PathBuf,
        sdk: std::path::PathBuf,
        linker: std::path::PathBuf,
    }
    static TOOLS: OnceLock<Result<Toolchain, String>> = OnceLock::new();
    let tools = TOOLS
        .get_or_init(|| {
            Ok(Toolchain {
                compiler: resolve("rustup", &["which", "rustc"])?,
                sdk: resolve("xcrun", &["--sdk", "macosx", "--show-sdk-path"])?,
                linker: resolve("xcrun", &["--find", "clang"])?,
            })
        })
        .as_ref()
        .map_err(|message| io::Error::other(message.clone()))?;
    let compiler = &tools.compiler;
    let directory = std::fs::canonicalize(directory)?;
    let temporary = directory.join("tmp");
    std::fs::create_dir_all(&temporary)?;
    let program = if compile {
        compiler.as_os_str()
    } else {
        command.as_std().get_program()
    };
    let toolchain = compiler
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("invalid compiler installation"))?;
    let mut restricted = Command::new("/usr/bin/sandbox-exec");
    restricted.arg("-p").arg(PROFILE);
    for (key, path) in [
        ("PROGRAM", Path::new(program)),
        ("TOOLCHAIN", toolchain),
        (
            "READ_ROOT",
            if compile {
                directory.as_path()
            } else {
                temporary.as_path()
            },
        ),
        (
            "WRITE_ROOT",
            if compile {
                directory.as_path()
            } else {
                temporary.as_path()
            },
        ),
    ] {
        restricted.arg("-D").arg(format!(
            "{key}={}",
            path.to_str()
                .ok_or_else(|| io::Error::other("sandbox paths must be UTF-8"))?
        ));
    }
    restricted
        .arg(program)
        .args(command.as_std().get_args())
        .current_dir(&directory);
    if compile {
        restricted
            .arg("-C")
            .arg(format!("linker={}", tools.linker.display()))
            .env("SDKROOT", &tools.sdk);
    }
    Ok(restricted)
}

#[cfg(target_os = "macos")]
fn resolve(program: &str, arguments: &[&str]) -> Result<std::path::PathBuf, String> {
    let mut resolver = std::process::Command::new(program);
    resolver.args(arguments).env_clear();
    for name in ["PATH", "HOME", "RUSTUP_HOME", "CARGO_HOME", "DEVELOPER_DIR"] {
        if let Some(value) = std::env::var_os(name) {
            resolver.env(name, value);
        }
    }
    let result = resolver
        .output()
        .map_err(|_| "cannot resolve compiler/SDK/linker".to_owned())?;
    if !result.status.success() {
        return Err("cannot resolve compiler/SDK/linker".into());
    }
    let path = std::path::PathBuf::from(
        String::from_utf8(result.stdout)
            .map_err(|_| "non-UTF8 toolchain path".to_owned())?
            .trim(),
    );
    if !path.is_absolute() || !path.exists() {
        return Err("invalid toolchain path".into());
    }
    std::fs::canonicalize(path).map_err(|_| "cannot resolve toolchain path".into())
}

#[cfg(target_os = "macos")]
const PROFILE: &str = r#"
(version 1)
(deny default)
(allow process-exec process-fork signal sysctl-read mach-lookup)
(allow file-read-metadata)
(allow file-read* file-map-executable
    (literal "/")
    (subpath "/System/Library") (subpath "/System/DriverKit") (subpath "/System/Cryptexes")
    (subpath "/private/preboot/Cryptexes/OS") (subpath "/private/var/db/dyld")
    (subpath "/usr/lib") (subpath "/usr/bin") (subpath "/usr/libexec") (subpath "/usr/share")
    (subpath "/Library/Developer") (subpath "/Applications/Xcode.app")
    (literal "/dev/null") (literal "/dev/random") (literal "/dev/urandom")
    (subpath (param "TOOLCHAIN"))
    (literal (param "PROGRAM"))
    (subpath (param "READ_ROOT")))
(allow file-write* (subpath (param "WRITE_ROOT")) (literal "/dev/null"))
"#;

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::super::verify::verify;

    #[tokio::test]
    async fn native_profile_blocks_external_data_writes_and_network() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("private-canary.txt");
        std::fs::write(&outside, "synthetic private canary").unwrap();
        let case = root.path().join("case");
        std::fs::create_dir(&case).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let alias =
            std::path::Path::new("/System/Volumes/Data").join(outside.strip_prefix("/").unwrap());
        let source = format!(
            r#"
            #[test] fn restricted() {{
                assert!(std::fs::read_to_string({outside:?}).is_err());
                assert!(std::fs::read_to_string({alias:?}).is_err());
                assert!(std::fs::write({outside:?}, "changed").is_err());
                assert!(std::fs::write("oracle.rs", "changed").is_err());
                assert!(std::net::TcpStream::connect({address:?}).is_err());
                assert!(std::env::var("OPENCODE_GO_API_KEY").is_err());
                std::fs::write("tmp/allowed.txt", "allowed").unwrap();
            }}
        "#
        );
        std::fs::write(case.join("oracle.rs"), &source).unwrap();
        let result = verify(&case, true).await.unwrap();
        assert!(result.passed, "{}", result.diagnostics);
        assert_eq!(
            std::fs::read_to_string(outside).unwrap(),
            "synthetic private canary"
        );
        assert_eq!(
            std::fs::read_to_string(case.join("oracle.rs")).unwrap(),
            source
        );
        assert_eq!(
            std::fs::read_to_string(case.join("tmp/allowed.txt")).unwrap(),
            "allowed"
        );
    }

    #[tokio::test]
    async fn compiler_cannot_embed_files_outside_task_directory() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("private-canary.txt");
        std::fs::write(&outside, "synthetic private canary").unwrap();
        let case = root.path().join("case");
        std::fs::create_dir(&case).unwrap();
        std::fs::write(
            case.join("oracle.rs"),
            format!("#[test] fn probe() {{ let _ = include_str!({outside:?}); }}"),
        )
        .unwrap();
        let result = verify(&case, true).await.unwrap();
        assert!(!result.passed);
        assert_eq!(result.stage, "compile");
    }
}
