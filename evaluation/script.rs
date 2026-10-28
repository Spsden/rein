use super::tasks::Task;
use rein::runtime::{Event, ModelBackend, RunError};
use rig_core::completion::{CompletionRequest, CompletionResponse, FinishReason, Usage};
use rig_core::message::{AssistantContent, Origin, ToolCall, ToolFunction};
use serde_json::{Value, json};
use std::cell::Cell;

/// A known solution script tests orchestration, NOT LLM reasoning capability.
pub(super) struct Script {
    pub task: &'static Task,
    pub step: Cell<usize>,
}

impl ModelBackend for Script {
    async fn complete(
        &self,
        _: &str,
        _: CompletionRequest,
        _: usize,
        _: &mut dyn FnMut(Event),
    ) -> Result<CompletionResponse, RunError> {
        let step = self.step.replace(self.step.get() + 1);
        let call = |index: usize, name: &str, args: Value| {
            AssistantContent::ToolCall(ToolCall::from_wire(
                format!("{step}-{index}"),
                ToolFunction::new(name.try_into().unwrap(), args),
            ))
        };
        let correct_at = if self.task.wrong.is_some() {
            6
        } else if self.task.conflict {
            5
        } else {
            3
        };
        let content = match step {
            0 => vec![call(0, "list_files", json!({}))],
            1 => vec![call(0, "search", json!({"query":self.task.edits[0].old}))],
            2 => self
                .task
                .edits
                .iter()
                .enumerate()
                .map(|(i, e)| call(i, "read_file", json!({"path":e.path})))
                .collect(),
            3 if self.task.wrong.is_some() => vec![call(0, "edit_file", json!({"path":self.task.edits[0].path,"old_text":self.task.edits[0].old,"new_text":self.task.wrong.unwrap()}))],
            4 if self.task.wrong.is_some() => vec![call(0, "run_tests", json!({}))],
            5 if self.task.wrong.is_some() => vec![call(0, "read_file", json!({"path":self.task.edits[0].path}))],
            3 if self.task.conflict => vec![call(
                0,
                "edit_file",
                json!({"path":self.task.edits[0].path,"old_text":"stale text that is not in the file","new_text":"wrong"}),
            )],
            4 if self.task.conflict => vec![call(
                0,
                "read_file",
                json!({"path":self.task.edits[0].path}),
            )],
            n if n == correct_at => self
                .task
                .edits
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    call(
                        i,
                        "edit_file",
                        json!({"path":e.path,"old_text":self.task.wrong.unwrap_or(e.old),"new_text":e.new}),
                    )
                })
                .collect(),
            n if n == correct_at + 1 => vec![call(0, "run_tests", json!({}))],
            n if n == correct_at + 2 => vec![AssistantContent::text(
                "Changed the relevant functions and ran the task tests.",
            )],
            _ => return Err(RunError::InvalidResponse),
        };
        Ok(CompletionResponse::new(
            content,
            Usage::new().output_tokens(3),
            Origin::new("script", "offline", "script"),
            json!({}),
        )
        .with_finish_reason(FinishReason::Stop))
    }
}
