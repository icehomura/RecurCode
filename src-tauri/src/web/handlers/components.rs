use std::collections::HashMap;
use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::components as component_commands;
use crate::commands::components::ComponentInfo;

pub async fn components_list(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<ComponentInfo>>, AppCommandError> {
    let result = component_commands::components_list_core(&state.db).await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentsSetEnabledParams {
    pub id: String,
    pub enabled: bool,
}

pub async fn components_set_enabled(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ComponentsSetEnabledParams>,
) -> Result<Json<()>, AppCommandError> {
    let emitter = state.emitter.clone();
    component_commands::components_set_enabled_core(
        &state.db,
        &emitter,
        &params.id,
        params.enabled,
    )
    .await?;
    Ok(Json(()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentsSetEnvParams {
    pub id: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

pub async fn components_set_env(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ComponentsSetEnvParams>,
) -> Result<Json<()>, AppCommandError> {
    component_commands::components_set_env_core(&state.db, &params.id, params.env).await?;
    Ok(Json(()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentsUpdateParams {
    pub id: String,
    pub task_id: String,
}

pub async fn components_update(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ComponentsUpdateParams>,
) -> Result<Json<()>, AppCommandError> {
    let emitter = state.emitter.clone();
    component_commands::components_update_core(&state.db, &emitter, &params.id, &params.task_id)
        .await?;
    Ok(Json(()))
}
