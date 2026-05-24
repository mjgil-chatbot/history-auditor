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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InspectedSession {
    pub provider: String,
    pub provider_display_name: String,
    pub session_id: String,
    pub session_path: String,
    pub session: SessionSnapshot,
}

pub trait SessionSource: Send + Sync {
    fn inspect_session(&self, session_id: &str) -> Result<InspectedSession>;
}

impl<T> SessionSource for Arc<T>
where
    T: SessionSource + ?Sized,
{
    fn inspect_session(&self, session_id: &str) -> Result<InspectedSession> {
        (**self).inspect_session(session_id)
    }
}
