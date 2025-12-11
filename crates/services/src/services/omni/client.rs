use reqwest::Client;

use super::types::{OmniInstance, SendTextRequest, SendTextResponse};

/// HTTP client for the Omni API
#[derive(Debug, Clone)]
pub struct OmniClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl OmniClient {
    /// Create a new Omni client
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }

    /// List all available instances
    pub async fn list_instances(&self) -> Result<Vec<OmniInstance>, reqwest::Error> {
        let url = format!("{}/instance/fetchInstances", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("apikey", &self.api_key)
            .send()
            .await?
            .json()
            .await?;
        Ok(response)
    }

    /// Send a text message
    pub async fn send_text(
        &self,
        instance_name: &str,
        request: SendTextRequest,
    ) -> Result<SendTextResponse, reqwest::Error> {
        let url = format!(
            "{}/message/sendText/{}",
            self.base_url, instance_name
        );
        let response = self
            .client
            .post(&url)
            .header("apikey", &self.api_key)
            .json(&request)
            .send()
            .await?
            .json()
            .await?;
        Ok(response)
    }
}
