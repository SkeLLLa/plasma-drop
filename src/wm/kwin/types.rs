use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::wm::{ManagedWindow, Point};

#[derive(Debug, Clone, Serialize)]
pub struct CommandEnvelope {
    #[serde(rename = "type")]
    pub command_type: String,
    #[serde(rename = "responderId")]
    pub responder_id: Uuid,
    pub params: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseEnvelope {
    #[serde(rename = "cmdType")]
    pub cmd_type: String,
    #[serde(rename = "responderId")]
    pub responder_id: Uuid,
    pub params: Value,
    pub exception_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WindowListResponse {
    pub windows: Vec<ManagedWindow>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WindowResponse {
    pub window: Option<ManagedWindow>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CursorPositionResponse {
    pub position: Option<Point>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SupportInformationResponse {
    pub text: String,
}
