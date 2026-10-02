use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRoleInfo {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub agent_type: Option<String>,
    pub model_id: Option<String>,
    pub tools_json: Option<String>,
    pub temperature: Option<f64>,
    pub is_builtin: bool,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
