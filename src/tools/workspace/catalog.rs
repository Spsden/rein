use rig_core::completion::ToolDefinition;
use serde_json::{Value, json};

pub(super) fn definitions(writable: bool) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        tool(
            "list_files",
            "List workspace text/code files recursively; narrow path if the directory is too large. Skips hidden files, dependencies and symlinks.",
            json!({"path":{"type":"string", "description":"Relative directory, default dot"}}),
            &[],
        ),
        tool(
            "read_file",
            "Read UTF-8 text with line numbers. Follow next_line for pagination. File limit 256000 bytes.",
            json!({"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"max_lines":{"type":"integer","minimum":1,"maximum":500}}),
            &["path"],
        ),
        tool(
            "search",
            "Search literal case-sensitive text in workspace UTF-8 files. Results have file paths and line numbers. Limit 100 matches.",
            json!({"query":{"type":"string"},"path":{"type":"string", "description":"Relative directory, default dot"}}),
            &["query"],
        ),
    ];
    if writable {
        definitions.push(tool("edit_file", "Replace exactly one occurrence of old_text with new_text in an existing file. Read first. Stale or ambiguous matches fail without modification; use a larger unique old_text and re-read on conflicts.", json!({"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}}), &["path","old_text","new_text"]));
        definitions.push(tool("write_file", "Create a NEW UTF-8 file. Never overwrites an existing file; use edit_file for changes. Parent directory must already exist. Maximum 256000 bytes.", json!({"path":{"type":"string"},"content":{"type":"string"}}), &["path","content"]));
    }
    definitions
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDefinition {
    ToolDefinition::new(
        name.try_into().expect("constant tool name"),
        description,
        json!({"type":"object", "properties":properties,"required":required,"additionalProperties":false}),
    )
}
