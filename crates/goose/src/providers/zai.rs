use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::TryStreamExt;
use serde_json::Value;
use std::io;
use tokio::pin;
use tokio_util::io::StreamReader;

use super::api_client::{ApiClient, AuthMethod};
use super::base::{ConfigKey, MessageStream, ModelInfo, Provider, ProviderMetadata, ProviderUsage};
use super::errors::ProviderError;
use super::formats::anthropic::{
    create_request, get_usage, response_to_message, response_to_streaming_message,
};
use super::retry::ProviderRetry;
use super::utils::handle_status_openai_compat;
use crate::conversation::message::Message;
use crate::model::ModelConfig;
use crate::providers::utils::RequestLog;
use rmcp::model::Tool;

pub const ZAI_DEFAULT_MODEL: &str = "glm-4.5";
pub const ZAI_DEFAULT_FAST_MODEL: &str = "glm-4.5-air";
pub const ZAI_KNOWN_MODELS: &[(&str, usize)] = &[
    ("glm-4.6", 200_000),
    ("glm-4.5", 128_000),
    ("glm-4.5-air", 128_000),
];

pub const ZAI_DOC_URL: &str = "https://z.ai/docs";

#[derive(Debug, serde::Serialize)]
pub struct ZaiProvider {
    #[serde(skip)]
    api_client: ApiClient,
    model: ModelConfig,
    name: String,
}

impl ZaiProvider {
    pub async fn from_env(model: ModelConfig) -> Result<Self> {
        let model = model.with_fast(ZAI_DEFAULT_FAST_MODEL.to_string());

        let config = crate::config::Config::global();
        let api_key: String = config.get_secret("ZAI_API_KEY")?;
        let host: String = config
            .get_param("ZAI_HOST")
            .unwrap_or_else(|_| "https://api.z.ai".to_string());
        let timeout_secs: u64 = config.get_param("ZAI_TIMEOUT").unwrap_or(600);

        // Use x-api-key header for Anthropic-compatible API
        let auth = AuthMethod::ApiKey {
            header_name: "x-api-key".to_string(),
            key: api_key,
        };
        
        let mut api_client =
            ApiClient::with_timeout(host, auth, std::time::Duration::from_secs(timeout_secs))?;
        
        api_client = api_client.with_header("anthropic-version", "2023-06-01")?;

        Ok(Self {
            api_client,
            model,
            name: "zai".to_string(),
        })
    }

    async fn post(&self, payload: &Value) -> Result<Value, ProviderError> {
        let response = self
            .api_client
            .response_post("api/anthropic/v1/messages", payload)
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        if !status.is_success() {
            return Err(ProviderError::RequestFailed(format!(
                "API request failed with status {}: {}",
                status, body
            )));
        }

        serde_json::from_str(&body)
            .map_err(|e| ProviderError::RequestFailed(format!("Failed to parse response: {}", e)))
    }
}

#[async_trait]
impl Provider for ZaiProvider {
    fn metadata() -> ProviderMetadata {
        let models: Vec<ModelInfo> = ZAI_KNOWN_MODELS
            .iter()
            .map(|(name, limit)| ModelInfo::new(*name, *limit))
            .collect();
        ProviderMetadata::with_models(
            "zai",
            "Z.ai",
            "Z.ai GLM models for coding assistance",
            ZAI_DEFAULT_MODEL,
            models,
            ZAI_DOC_URL,
            vec![
                ConfigKey::new("ZAI_API_KEY", true, true, None),
                ConfigKey::new("ZAI_HOST", false, false, Some("https://api.z.ai")),
                ConfigKey::new("ZAI_TIMEOUT", false, false, Some("600")),
            ],
        )
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn get_model_config(&self) -> ModelConfig {
        self.model.clone()
    }

    #[tracing::instrument(
        skip(self, model_config, system, messages, tools),
        fields(model_config, input, output, input_tokens, output_tokens, total_tokens)
    )]
    async fn complete_with_model(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        let payload = create_request(model_config, system, messages, tools)?;

        let mut log = RequestLog::start(&self.model, &payload)?;
        let json_response = self
            .with_retry(|| async {
                let payload_clone = payload.clone();
                self.post(&payload_clone).await
            })
            .await
            .inspect_err(|e| {
                let _ = log.error(e);
            })?;

        let message = response_to_message(&json_response)
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;
        let usage = get_usage(&json_response)
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        log.write(&json_response, Some(&usage))?;
        Ok((message, ProviderUsage::new(model_config.model_name.clone(), usage)))
    }

    async fn stream(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let mut payload = create_request(&self.model, system, messages, tools)
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;
        
        // Enable streaming
        payload
            .as_object_mut()
            .unwrap()
            .insert("stream".to_string(), serde_json::Value::Bool(true));

        let mut log = RequestLog::start(&self.model, &payload)?;

        let resp = self
            .api_client
            .response_post("api/anthropic/v1/messages", &payload)
            .await
            .inspect_err(|e| {
                let _ = log.error(e);
            })?;

        let response = handle_status_openai_compat(resp).await.inspect_err(|e| {
            let _ = log.error(e);
        })?;

        let stream = response.bytes_stream().map_err(io::Error::other);

        Ok(Box::pin(try_stream! {
            let stream_reader = StreamReader::new(stream);
            let framed = tokio_util::codec::FramedRead::new(
                stream_reader, 
                tokio_util::codec::LinesCodec::new()
            ).map_err(anyhow::Error::from);

            let message_stream = response_to_streaming_message(framed);
            pin!(message_stream);
            while let Some(message) = futures::StreamExt::next(&mut message_stream).await {
                let (message, usage) = message.map_err(|e| ProviderError::RequestFailed(format!("Stream decode error: {}", e)))?;
                log.write(&message, usage.as_ref().map(|f| f.usage).as_ref())?;
                yield (message, usage);
            }
        }))
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}
