use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use super::api_client::{ApiClient, AuthMethod};
use super::base::{ConfigKey, ModelInfo, Provider, ProviderMetadata, ProviderUsage, Usage};
use super::errors::ProviderError;
use super::retry::ProviderRetry;
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

    fn create_request(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
    ) -> Result<Value, ProviderError> {
        let mut anthropic_messages: Vec<Value> = Vec::new();

        for msg in messages {
            let role = match msg.role {
                rmcp::model::Role::User => "user",
                rmcp::model::Role::Assistant => "assistant",
            };

            let content = msg.as_concat_text();
            anthropic_messages.push(json!({
                "role": role,
                "content": content
            }));
        }

        let mut request = json!({
            "model": model_config.model_name,
            "max_tokens": 8192,
            "messages": anthropic_messages
        });

        if !system.is_empty() {
            request["system"] = json!(system);
        }

        Ok(request)
    }

    fn parse_response(&self, response: &Value) -> Result<(Message, Usage), ProviderError> {
        let content = response
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("");

        let message = Message::assistant().with_text(content);

        let usage = response
            .get("usage")
            .map(|u| Usage::new(
                u.get("input_tokens").and_then(|v| v.as_i64()).map(|v| v as i32),
                u.get("output_tokens").and_then(|v| v.as_i64()).map(|v| v as i32),
                None,
            ))
            .unwrap_or_default();

        Ok((message, usage))
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
        skip(self, model_config, system, messages, _tools),
        fields(model_config, input, output, input_tokens, output_tokens, total_tokens)
    )]
    async fn complete_with_model(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        _tools: &[Tool],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        let payload = self.create_request(model_config, system, messages)?;

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

        let (message, usage) = self.parse_response(&json_response)?;

        log.write(&json_response, Some(&usage))?;
        Ok((message, ProviderUsage::new(model_config.model_name.clone(), usage)))
    }

    fn supports_streaming(&self) -> bool {
        false // For now, disable streaming until we implement it properly
    }
}
