use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::wm::{ManagedWindow, Point};

#[derive(Debug, Clone, Serialize)]
pub struct CommandEnvelope<'a> {
    #[serde(rename = "type")]
    pub command_type: &'a str,
    #[serde(rename = "responderId")]
    pub responder_id: u64,
    pub params: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseEnvelope {
    #[serde(rename = "cmdType")]
    pub cmd_type: String,
    #[serde(rename = "responderId")]
    pub responder_id: u64,
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
pub struct ScreensResponse {
    /// Empty when the script finds no `workspace.screens`.
    pub screens: Vec<ScreenEntry>,
}

/// One enabled output from `workspace.screens`, in logical coordinates.
#[derive(Debug, Clone, Deserialize)]
pub struct ScreenEntry {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
