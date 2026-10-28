use rein::AgentSession;
use rein::runtime::{Event, RunOutcome};
use rig_core::message::{AssistantContent, Message};
use rig_core::streaming::{Item, StreamEvent};
use std::io::{self, Write};

/// Model and workspace text must never be interpreted as terminal escape sequences.
pub(super) fn safe(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

pub(super) struct Display<W> {
    output: W,
    answer_started: bool,
    line_open: bool,
    streamed_turn: bool,
    unstreamed: Vec<u64>,
}

impl<W: Write> Display<W> {
    pub fn new(output: W) -> Self {
        Self {
            output,
            answer_started: false,
            line_open: false,
            streamed_turn: false,
            unstreamed: Vec::new(),
        }
    }

    pub fn event(&mut self, event: Event) -> io::Result<()> {
        match event {
            Event::ModelStarted { call } => {
                self.end_answer()?;
                writeln!(self.output, "[thinking · model call {call}]")?;
            }
            Event::Stream {
                item: Item::Event(StreamEvent::Text { text, .. }),
            } if !text.is_empty() => {
                self.text(&text)?;
                self.streamed_turn = true;
            }
            Event::TurnAccepted { entry_id, .. } => {
                if !self.streamed_turn {
                    self.unstreamed.push(entry_id);
                }
                self.streamed_turn = false;
                self.end_answer()?;
            }
            Event::ToolStarted { name, .. } => {
                self.end_answer()?;
                writeln!(self.output, "[tool] {}", safe(&name))?;
            }
            Event::ToolFinished { is_error, .. } => {
                writeln!(
                    self.output,
                    "[tool {}]",
                    if is_error { "error" } else { "ok" }
                )?;
            }
            _ => {}
        }
        self.output.flush()
    }

    pub fn finish(
        &mut self,
        session: &AgentSession,
        outcome: Option<&RunOutcome>,
    ) -> io::Result<()> {
        self.end_answer()?;
        // Non-streaming backends still expose their accepted answer, without duplicating deltas.
        let ids = std::mem::take(&mut self.unstreamed);
        for entry in session
            .transcript
            .entries()
            .iter()
            .filter(|entry| ids.contains(&entry.id.value()))
        {
            if let Message::Assistant(message) = &entry.message {
                for part in &message.content {
                    if let AssistantContent::Text(text) = part {
                        self.text(&text.text)?;
                    }
                }
                self.end_answer()?;
            }
        }
        if let Some(outcome) = outcome {
            let tokens = outcome
                .completion_tokens
                .map_or("unknown".into(), |count| count.to_string());
            writeln!(
                self.output,
                "[done · {} model calls · {tokens} output tokens · session saved]\n",
                outcome.model_calls
            )?;
        }
        self.output.flush()
    }

    fn text(&mut self, text: &str) -> io::Result<()> {
        if !self.answer_started {
            write!(self.output, "rein > ")?;
            self.answer_started = true;
        }
        let text = safe(text);
        write!(self.output, "{text}")?;
        self.line_open = !text.ends_with('\n');
        Ok(())
    }

    fn end_answer(&mut self) -> io::Result<()> {
        if self.line_open {
            writeln!(self.output)?;
        }
        self.answer_started = false;
        self.line_open = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_controls_are_removed_but_unicode_and_layout_survive() {
        assert_eq!(
            safe("hi\x1b[2J\r\x07\x08\u{009b}世界\n\t!"),
            "hi[2J世界\n\t!"
        );
    }

    #[test]
    fn text_streams_without_reasoning_unknown_payloads_or_duplicate_end_text() {
        let mut display = Display::new(Vec::new());
        for event in [
            serde_json::json!({"event":"text","part":0,"text":"Hello "}),
            serde_json::json!({"event":"reasoning","part":1,"text":"private reasoning"}),
            serde_json::json!({"event":"text","part":0,"text":"世界"}),
            serde_json::json!({"event":"end","part":0,"content":{"type":"text","text":"Hello 世界"}}),
        ] {
            display
                .event(Event::Stream {
                    item: Item::Event(serde_json::from_value(event).unwrap()),
                })
                .unwrap();
        }
        display
            .event(Event::TurnAccepted {
                entry_id: 2,
                output_tokens: Some(2),
            })
            .unwrap();
        assert_eq!(
            String::from_utf8(display.output).unwrap(),
            "rein > Hello 世界\n"
        );
    }
}
