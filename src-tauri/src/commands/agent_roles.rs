#[cfg(feature = "tauri-runtime")]
use crate::db::error::DbError;
#[cfg(feature = "tauri-runtime")]
use crate::db::service::agent_role_service;
#[cfg(feature = "tauri-runtime")]
use crate::db::AppDatabase;
#[cfg(feature = "tauri-runtime")]
use crate::models::AgentRoleInfo;

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn agent_role_list(
    db: tauri::State<'_, AppDatabase>,
) -> Result<Vec<AgentRoleInfo>, DbError> {
    agent_role_service::list(&db.conn).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn agent_role_get(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
) -> Result<Option<AgentRoleInfo>, DbError> {
    agent_role_service::get(&db.conn, id).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn agent_role_create(
    db: tauri::State<'_, AppDatabase>,
    name: String,
    description: String,
    system_prompt: String,
    agent_type: Option<String>,
    model_id: Option<String>,
    tools_json: Option<String>,
    temperature: Option<f64>,
) -> Result<AgentRoleInfo, DbError> {
    agent_role_service::create(
        &db.conn,
        &name,
        &description,
        &system_prompt,
        agent_type.as_deref(),
        model_id.as_deref(),
        tools_json.as_deref(),
        temperature,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn agent_role_update(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
    name: Option<String>,
    description: Option<String>,
    system_prompt: Option<String>,
    agent_type: Option<String>,
    model_id: Option<String>,
    tools_json: Option<String>,
    temperature: Option<f64>,
) -> Result<AgentRoleInfo, DbError> {
    agent_role_service::update(
        &db.conn,
        id,
        name,
        description,
        system_prompt,
        agent_type,
        model_id,
        tools_json,
        temperature,
        false, // clear_agent_type
        false, // clear_model_id
        false, // clear_tools_json
        false, // clear_temperature
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn agent_role_delete(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
) -> Result<(), DbError> {
    agent_role_service::delete(&db.conn, id).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn agent_role_reorder(
    db: tauri::State<'_, AppDatabase>,
    ids: Vec<i32>,
) -> Result<(), DbError> {
    agent_role_service::reorder(&db.conn, ids).await
}
