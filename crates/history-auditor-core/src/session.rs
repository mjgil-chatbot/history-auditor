use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMessage {
    pub content: String,
    pub is_user: bool,
    pub timestamp: DateTime<Utc>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launched_from: Option<String>,
    #[serde(default)]
    pub messages: Vec<SessionMessage>,
}
