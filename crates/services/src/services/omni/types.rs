use serde::{Deserialize, Serialize};

/// Configuration for the Omni service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniConfig {
    /// The base URL of the Omni API
    pub api_url: String,
    /// The API key for authentication
    pub api_key: String,
    /// The instance name to use
    pub instance_name: String,
    /// Default recipient for notifications
    pub default_recipient: Option<String>,
    /// Type of recipient (phone number format)
    pub recipient_type: RecipientType,
}

impl Default for OmniConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:8082".to_string(),
            api_key: String::new(),
            instance_name: "default".to_string(),
            default_recipient: None,
            recipient_type: RecipientType::Phone,
        }
    }
}

/// Type of recipient identifier
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RecipientType {
    #[default]
    Phone,
    WhatsAppId,
}

/// Response from listing Omni instances
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniInstance {
    pub instance_name: String,
    pub status: String,
}

/// Request to send a text message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextRequest {
    pub number: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<u64>,
}

/// Response from sending a text message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextResponse {
    pub key: MessageKey,
    pub message: MessageDetails,
}

/// Message key containing identifiers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageKey {
    #[serde(rename = "remoteJid")]
    pub remote_jid: String,
    #[serde(rename = "fromMe")]
    pub from_me: bool,
    pub id: String,
}

/// Message details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDetails {
    #[serde(rename = "extendedTextMessage")]
    pub extended_text_message: Option<ExtendedTextMessage>,
}

/// Extended text message content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedTextMessage {
    pub text: String,
}
