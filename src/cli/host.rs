use rein::runtime::{ToolError, ToolExecutor};
use rein::tools::{echo, workspace::WorkspaceTools};
use rig_core::completion::ToolDefinition;
use rig_core::message::ToolCall;

pub(super) enum HostTools {
    Echo,
    Workspace {
        files: WorkspaceTools,
        shell: Option<Shell>,
    },
}
impl HostTools {
    pub(super) fn definitions(&self) -> Vec<ToolDefinition> {
        match self {
            Self::Echo => vec![echo::definition()],
            Self::Workspace { files, shell } => {
                let mut definitions = files.definitions();
                if shell.is_some() {
                    definitions.push(shell::definition());
                }
                definitions
            }
        }
    }
}
impl ToolExecutor for HostTools {
    async fn execute(&self, call: &ToolCall) -> Result<String, ToolError> {
        match self {
            Self::Echo => echo::Echo.execute(call).await,
            Self::Workspace { files, shell } => {
                if call.function.name.as_str() == shell::NAME {
                    return match shell {
                        Some(shell) => shell.execute(call).await,
                        None => Err(ToolError::Conflict("Shell is disabled by the host")),
                    };
                }
                files.execute(call).await
            }
        }
    }
}
use super::shell::{self, Shell};
