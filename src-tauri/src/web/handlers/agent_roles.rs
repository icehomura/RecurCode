use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::db::service::agent_role_service;
use crate::models::AgentRoleInfo;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentRoleParams {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub agent_type: Option<String>,
    pub model_id: Option<String>,
    pub tools_json: Option<String>,
    pub temperature: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgentRoleParams {
    pub id: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentRoleParams {
    pub id: i32,
    pub name: Option<String>,
    pub description: Option<String>,
    pub system_prompt: Option<String>,
    pub agent_type: Option<String>,
    pub model_id: Option<String>,
    pub tools_json: Option<String>,
    pub temperature: Option<f64>,
    #[serde(default)]
    pub clear_agent_type: bool,
    #[serde(default)]
    pub clear_model_id: bool,
    #[serde(default)]
    pub clear_tools_json: bool,
    #[serde(default)]
    pub clear_temperature: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAgentRoleParams {
    pub id: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderAgentRolesParams {
    pub ids: Vec<i32>,
}

pub async fn agent_role_list(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<AgentRoleInfo>>, AppCommandError> {
    let result = agent_role_service::list(&state.db.conn)
        .await
        .map_err(AppCommandError::from)?;
    Ok(Json(result))
}

pub async fn agent_role_get(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<GetAgentRoleParams>,
) -> Result<Json<Option<AgentRoleInfo>>, AppCommandError> {
    let result = agent_role_service::get(&state.db.conn, params.id)
        .await
        .map_err(AppCommandError::from)?;
    Ok(Json(result))
}

pub async fn agent_role_create(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<CreateAgentRoleParams>,
) -> Result<Json<AgentRoleInfo>, AppCommandError> {
    let result = agent_role_service::create(
        &state.db.conn,
        &params.name,
        &params.description,
        &params.system_prompt,
        params.agent_type.as_deref(),
        params.model_id.as_deref(),
        params.tools_json.as_deref(),
        params.temperature,
    )
    .await
    .map_err(AppCommandError::from)?;
    Ok(Json(result))
}

pub async fn agent_role_update(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<UpdateAgentRoleParams>,
) -> Result<Json<AgentRoleInfo>, AppCommandError> {
    let result = agent_role_service::update(
        &state.db.conn,
        params.id,
        params.name,
        params.description,
        params.system_prompt,
        params.agent_type,
        params.model_id,
        params.tools_json,
        params.temperature,
        params.clear_agent_type,
        params.clear_model_id,
        params.clear_tools_json,
        params.clear_temperature,
    )
    .await
    .map_err(AppCommandError::from)?;
    Ok(Json(result))
}

pub async fn agent_role_delete(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DeleteAgentRoleParams>,
) -> Result<Json<()>, AppCommandError> {
    agent_role_service::delete(&state.db.conn, params.id)
        .await
        .map_err(AppCommandError::from)?;
    Ok(Json(()))
}

pub async fn agent_role_reorder(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ReorderAgentRolesParams>,
) -> Result<Json<()>, AppCommandError> {
    agent_role_service::reorder(&state.db.conn, params.ids)
        .await
        .map_err(AppCommandError::from)?;
    Ok(Json(()))
}
