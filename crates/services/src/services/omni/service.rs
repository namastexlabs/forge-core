use super::client::OmniClient;
use super::types::{OmniConfig, OmniInstance, SendTextRequest, SendTextResponse};

/// Service for sending notifications via Omni (WhatsApp/SMS)
#[derive(Debug, Clone)]
pub struct OmniService {
    client: OmniClient,
    config: OmniConfig,
}

impl OmniService {
    /// Create a new Omni service from configuration
    pub fn new(config: OmniConfig) -> Self {
        let client = OmniClient::new(&config.api_url, &config.api_key);
        Self { client, config }
    }

    /// Get the current configuration
    pub fn config(&self) -> &OmniConfig {
        &self.config
    }

    /// List all available Omni instances
    pub async fn list_instances(&self) -> Result<Vec<OmniInstance>, reqwest::Error> {
        self.client.list_instances().await
    }

    /// Send a text message to the default recipient
    pub async fn send_notification(&self, message: &str) -> Result<Option<SendTextResponse>, reqwest::Error> {
        if let Some(recipient) = &self.config.default_recipient {
            self.send_text(recipient, message).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Send a text message to a specific recipient
    pub async fn send_text(
        &self,
        recipient: &str,
        message: &str,
    ) -> Result<SendTextResponse, reqwest::Error> {
        let request = SendTextRequest {
            number: recipient.to_string(),
            text: message.to_string(),
            delay: None,
        };
        self.client
            .send_text(&self.config.instance_name, request)
            .await
    }
}
