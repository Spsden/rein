//! One host-configured OpenAI Chat Completions connection, through Rig.
use crate::runtime::{Event, ModelBackend, RunError};
use futures::StreamExt;
use rig_core::completion::{CompletionRequest, CompletionResponse};
use rig_core::providers::openai::{OpenAI, OpenAIConfig};

pub struct OpenAiBackend {
    connection_id: String,
    client: OpenAI,
}
impl OpenAiBackend {
    /// Saved sessions cannot change endpoints or credentials.
    pub fn new(connection_id: impl Into<String>, config: OpenAIConfig) -> Self {
        Self {
            connection_id: connection_id.into(),
            client: config.client(),
        }
    }
}
impl ModelBackend for OpenAiBackend {
    async fn complete(
        &self,
        connection: &str,
        request: CompletionRequest,
        max_response_bytes: usize,
        emit: &mut dyn FnMut(Event),
    ) -> Result<CompletionResponse, RunError> {
        if connection != self.connection_id {
            return Err(RunError::Model);
        }
        let model = self
            .client
            .chat(request.model.clone().ok_or(RunError::Model)?);
        let mut stream = model.stream(request).map_err(|_| RunError::Model)?;
        let mut bytes = 0_usize;
        while let Some(item) = stream.next().await {
            let item = item.map_err(|_| RunError::Model)?;
            bytes = bytes.saturating_add(
                serde_json::to_vec(&item)
                    .map_err(|_| RunError::Model)?
                    .len(),
            );
            if bytes > max_response_bytes {
                return Err(RunError::ResponseTooLarge);
            }
            emit(Event::Stream { item });
        }
        stream.finish().await.map_err(|_| RunError::Model)
    }
}
