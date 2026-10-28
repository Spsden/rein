use rein::runtime::{ToolError, ToolExecutor};
use rein::tools::{echo, workspace::WorkspaceTools};
use rig_core::completion::ToolDefinition;
use rig_core::message::ToolCall;

pub(super) enum HostTools {
    Echo,
    Workspace(WorkspaceTools),
}
impl HostTools {
    pub(super) fn definitions(&self) -> Vec<ToolDefinition> {
        match self {
            Self::Echo => vec![echo::definition()],
            Self::Workspace(tools) => tools.definitions(),
        }
    }
}
impl ToolExecutor for HostTools {
    async fn execute(&self, call: &ToolCall) -> Result<String, ToolError> {
        match self {
            Self::Echo => echo::Echo.execute(call).await,
            Self::Workspace(tools) => tools.execute(call).await,
        }
    }
}
