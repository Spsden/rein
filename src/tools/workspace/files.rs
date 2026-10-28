use super::{MAX_FILE_BYTES, MAX_OUTPUT_BYTES};
use crate::runtime::ToolError;
use cap_std::fs::{Dir, OpenOptions};
use serde::Deserialize;
use serde_json::json;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct List {
    #[serde(default = "dot")]
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadFile {
    path: String,
    #[serde(default = "one")]
    start_line: usize,
    #[serde(default = "lines")]
    max_lines: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Search {
    query: String,
    #[serde(default = "dot")]
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Edit {
    path: String,
    old_text: String,
    new_text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WriteFile {
    path: String,
    content: String,
}
fn dot() -> String {
    ".".into()
}
fn one() -> usize {
    1
}
fn lines() -> usize {
    200
}

fn host(error: std::io::Error) -> ToolError {
    ToolError::Host(error.to_string())
}
fn invalid(message: &'static str) -> ToolError {
    ToolError::InvalidArguments(message)
}

fn excluded(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules" | "vendor" | "venv")
}

fn path(directory: &Dir, name: &str, allow_root: bool) -> Result<PathBuf, ToolError> {
    if allow_root && name == "." {
        return Ok(PathBuf::from("."));
    }
    if name.is_empty() || name.len() > 4096 {
        return Err(invalid("provide a nonempty workspace-relative path"));
    }
    let mut checked = PathBuf::new();
    for component in Path::new(name).components() {
        let Component::Normal(part) = component else {
            return Err(invalid("absolute paths and parent traversal are forbidden"));
        };
        let text = part.to_str().ok_or_else(|| invalid("path must be UTF-8"))?;
        if excluded(text) {
            return Err(invalid(
                "hidden files and dependency/build directories are excluded",
            ));
        }
        checked.push(part);
        match directory.symlink_metadata(&checked) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(invalid("symlinks are not supported"));
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(host(error)),
        }
    }
    Ok(checked)
}

fn text(directory: &Dir, path: &Path) -> Result<String, ToolError> {
    if !directory.symlink_metadata(path).map_err(host)?.is_file() {
        return Err(invalid("path must name a regular text file"));
    }
    let file = directory.open(path).map_err(host)?;
    if !file.metadata().map_err(host)?.is_file() {
        return Err(invalid("path must name a regular text file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(host)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(invalid("file exceeds 256000 bytes; use a smaller file"));
    }
    if bytes.contains(&0) {
        return Err(invalid("binary files are not supported"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("file must contain UTF-8 text"))
}

fn walk(
    directory: &Dir,
    at: &Path,
    found: &mut Vec<PathBuf>,
    visited: &mut usize,
    depth: usize,
) -> Result<(), ToolError> {
    if depth > 32 {
        return Err(invalid("directory nesting exceeds 32; narrow the path"));
    }
    for entry in directory.read_dir(at).map_err(host)? {
        *visited += 1;
        if *visited > 2000 {
            return Err(invalid("directory exceeds 2000 entries; narrow the path"));
        }
        let entry = entry.map_err(host)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if excluded(name) {
            continue;
        }
        let kind = entry.file_type().map_err(host)?;
        if kind.is_symlink() {
            continue;
        }
        let child = at.join(name);
        if kind.is_dir() {
            walk(directory, &child, found, visited, depth + 1)?;
        } else if kind.is_file() {
            found.push(child);
        }
    }
    Ok(())
}

fn files(directory: &Dir, at: &str) -> Result<Vec<PathBuf>, ToolError> {
    let at = path(directory, at, true)?;
    let mut found = Vec::new();
    walk(directory, &at, &mut found, &mut 0, 0)?;
    found.sort();
    Ok(found)
}

pub(super) fn list(directory: &Dir, args: List) -> Result<String, ToolError> {
    let found = files(directory, &args.path)?;
    let mut output = Vec::new();
    let mut bytes = 0;
    for path in &found {
        let name = path.to_string_lossy().trim_start_matches("./").to_owned();
        bytes += name.len() * 6 + 4;
        if bytes > MAX_OUTPUT_BYTES || output.len() == 200 {
            break;
        }
        output.push(name);
    }
    Ok(json!({"files":output,"truncated":output.len() < found.len()}).to_string())
}

pub(super) fn read(directory: &Dir, args: ReadFile) -> Result<String, ToolError> {
    if args.start_line == 0 || args.max_lines == 0 || args.max_lines > 500 {
        return Err(invalid(
            "start_line >= 1 and max_lines between 1 and 500 are required",
        ));
    }
    let contents = text(directory, &path(directory, &args.path, false)?)?;
    let lines: Vec<_> = contents.lines().collect();
    if args.start_line > lines.len() + 1 {
        return Err(invalid("start_line is past the end of the file"));
    }
    let mut output = String::new();
    let mut next = args.start_line - 1;
    for (index, line) in lines.iter().enumerate().skip(next).take(args.max_lines) {
        let numbered = format!("{}: {line}\n", index + 1);
        if (output.len() + numbered.len()) * 6 > MAX_OUTPUT_BYTES {
            break;
        }
        output.push_str(&numbered);
        next = index + 1;
    }
    if next < lines.len() && output.is_empty() {
        return Err(invalid(
            "line exceeds output limit; search for a short matching fragment instead",
        ));
    }
    Ok(json!({"path":args.path,"content":output,"next_line":(next < lines.len()).then_some(next+1)}).to_string())
}

pub(super) fn search(directory: &Dir, args: Search) -> Result<String, ToolError> {
    if args.query.is_empty() || args.query.len() > 1000 {
        return Err(invalid(
            "query must contain 1 to 1000 bytes of literal text",
        ));
    }
    let mut matches = Vec::new();
    let mut bytes = 0;
    let mut skipped = 0;
    for path in files(directory, &args.path)? {
        let Ok(contents) = text(directory, &path) else {
            skipped += 1;
            continue;
        };
        for (index, line) in contents.lines().enumerate() {
            if !line.contains(&args.query) {
                continue;
            }
            let found = json!({"path":path.to_string_lossy().trim_start_matches("./"),"line":index+1,"text":line.chars().take(240).collect::<String>()});
            bytes += found.to_string().len();
            if matches.len() == 100 || bytes > MAX_OUTPUT_BYTES - 256 {
                return Ok(
                    json!({"matches":matches,"truncated":true,"skipped_files":skipped}).to_string(),
                );
            }
            matches.push(found);
        }
    }
    Ok(json!({"matches":matches,"truncated":false,"skipped_files":skipped}).to_string())
}

pub(super) fn write(directory: &Dir, args: WriteFile) -> Result<String, ToolError> {
    let path = path(directory, &args.path, false)?;
    if args.content.len() > MAX_FILE_BYTES || args.content.contains('\0') {
        return Err(invalid("content must be text and at most 256000 bytes"));
    }
    let mut file = directory
        .open_with(path, OpenOptions::new().write(true).create_new(true))
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ToolError::Conflict("file exists; read it and use edit_file")
            } else {
                host(error)
            }
        })?;
    file.write_all(args.content.as_bytes()).map_err(host)?;
    file.sync_all().map_err(host)?;
    Ok(json!({"created":args.path}).to_string())
}

pub(super) fn edit(directory: &Dir, args: Edit) -> Result<String, ToolError> {
    let path = path(directory, &args.path, false)?;
    let current = text(directory, &path)?;
    if args.old_text.is_empty() {
        return Err(invalid(
            "old_text cannot be empty; use write_file to create files",
        ));
    }
    if current.find(&args.old_text).is_none()
        || current.find(&args.old_text) != current.rfind(&args.old_text)
    {
        return Err(ToolError::Conflict(
            "old_text must match exactly once; re-read the file and provide a unique exact fragment",
        ));
    }
    let updated = current.replacen(&args.old_text, &args.new_text, 1);
    if updated.len() > MAX_FILE_BYTES || updated.contains('\0') {
        return Err(invalid(
            "updated content must be text and at most 256000 bytes",
        ));
    }
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let temporary = path.parent().unwrap_or(Path::new("")).join(format!(
        ".rein-edit-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = directory
        .open_with(&temporary, OpenOptions::new().write(true).create_new(true))
        .map_err(host)?;
    let result = (|| {
        file.set_permissions(directory.metadata(&path)?.permissions())?;
        file.write_all(updated.as_bytes())?;
        file.sync_all()?;
        directory.rename(&temporary, directory, &path)
    })();
    if result.is_err() {
        let _ = directory.remove_file(&temporary);
    }
    result.map_err(host)?;
    Ok(json!({"edited":args.path,"replacements":1}).to_string())
}
