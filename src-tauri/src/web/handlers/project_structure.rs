use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::db::service::project_structure_service;
use crate::models::ProjectStructureNodeInfo;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStructureListParams {
    pub folder_id: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStructureScanParams {
    pub folder_id: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStructureUpdateNodeParams {
    pub node_id: i32,
    pub name: Option<String>,
    pub summary: Option<String>,
    pub call_relations: Option<String>,
    pub metadata: Option<String>,
    pub kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStructureDeleteAllParams {
    pub folder_id: i32,
}

pub async fn project_structure_list(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ProjectStructureListParams>,
) -> Result<Json<Vec<ProjectStructureNodeInfo>>, AppCommandError> {
    let result = project_structure_service::list(&state.db.conn, params.folder_id)
        .await
        .map_err(AppCommandError::from)?;
    Ok(Json(result))
}

pub async fn project_structure_scan(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ProjectStructureScanParams>,
) -> Result<Json<Vec<ProjectStructureNodeInfo>>, AppCommandError> {
    // Stub: will scan filesystem later
    let _ = &state;
    let _ = params;
    Ok(Json(vec![]))
}

pub async fn project_structure_update_node(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ProjectStructureUpdateNodeParams>,
) -> Result<Json<ProjectStructureNodeInfo>, AppCommandError> {
    let result = project_structure_service::update_node(
        &state.db.conn,
        params.node_id,
        params.name,
        params.summary,
        params.call_relations,
        params.metadata,
        params.kind,
    )
    .await
    .map_err(AppCommandError::from)?;
    Ok(Json(result))
}

pub async fn project_structure_delete_all(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ProjectStructureDeleteAllParams>,
) -> Result<Json<()>, AppCommandError> {
    project_structure_service::delete_all(&state.db.conn, params.folder_id)
        .await
        .map_err(AppCommandError::from)?;
    Ok(Json(()))
}
