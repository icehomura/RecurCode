use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStructureNodeInfo {
    pub id: i32,
    pub folder_id: i32,
    pub path: String,
    pub kind: String,
    pub name: String,
    pub parent_id: Option<i32>,
    pub language: Option<String>,
    pub summary: Option<String>,
    pub call_relations: Option<String>,
    pub metadata: Option<String>,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectStructureNodeDraft {
    pub path: String,
    pub kind: String,
    pub name: String,
    pub parent_id: Option<i32>,
    pub language: Option<String>,
    pub summary: Option<String>,
    pub call_relations: Option<String>,
    pub metadata: Option<String>,
    pub sort_order: Option<i32>,
}
