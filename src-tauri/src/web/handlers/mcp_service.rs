//! HTTP handlers for the rc-mcp service status indicator — the web-mode
//! mirror of the Tauri commands in `commands::mcp_service`.
//!
//! Both transports call the same `_core` helpers, so the probe, the state
//! ladder and the rebind behave identically whether the workspace is running
//! in the desktop shell or against a remote `rc-server`.

use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::mcp_service::{
    recur_code_mcp_service_status_core, set_recur_code_mcp_tool_group_core, start_recur_code_mcp_service_core,
    RecurCodeMcpServiceStatus, RecurCodeMcpStatusSources, RecurCodeMcpToolGroupTargets,
};

pub async fn get_recur_code_mcp_service_status(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<RecurCodeMcpServiceStatus>, AppCommandError> {
    Ok(Json(
        recur_code_mcp_service_status_core(RecurCodeMcpStatusSources {
            broker: &state.delegation_broker,
            tokens: &state.delegation_tokens,
            feedback: &state.feedback_config,
            question: &state.question_config,
            session_info: &state.session_info_config,
            browser: &state.browser_tools_config,
            authoring: &state.chat_authoring_config,
            computer: &state.computer_tools_config,
        })
        .await,
    ))
}

pub async fn start_recur_code_mcp_service() -> Result<Json<()>, AppCommandError> {
    start_recur_code_mcp_service_core().await?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct SetRecurCodeMcpToolGroupParams {
    pub key: String,
    pub enabled: bool,
}

pub async fn set_recur_code_mcp_tool_group(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<SetRecurCodeMcpToolGroupParams>,
) -> Result<Json<()>, AppCommandError> {
    set_recur_code_mcp_tool_group_core(
        &state.db.conn,
        RecurCodeMcpToolGroupTargets {
            broker: &state.delegation_broker,
            feedback: &state.feedback_config,
            question: &state.question_config,
            session_info: &state.session_info_config,
            browser: &state.browser_tools_config,
            authoring: &state.chat_authoring_config,
            computer: &state.computer_tools_config,
        },
        &state.emitter,
        &params.key,
        params.enabled,
    )
    .await?;
    Ok(Json(()))
}
