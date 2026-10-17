use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    IntoActiveModel, QueryFilter, Set,
};

use crate::db::entities::component_setting;
use crate::db::error::DbError;

/// Insert a row for every built-in component that does not already have one.
///
/// Default `enabled` is `true` for every built-in: the registry is what
/// RecurCode ships and updates for its agents, so a fresh install is "on"
/// until the user opts out. The hardcoded registry is the source of truth for
/// which ids exist, so this is idempotent and never resurrects a row the user
/// turned off (an existing row is left untouched).
pub async fn ensure_defaults(conn: &DatabaseConnection, ids: &[&str]) -> Result<(), DbError> {
    let now = Utc::now();
    for id in ids {
        let existing = component_setting::Entity::find()
            .filter(component_setting::Column::ComponentId.eq(*id))
            .one(conn)
            .await?;
        if existing.is_some() {
            continue;
        }
        let active = component_setting::ActiveModel {
            id: NotSet,
            component_id: Set((*id).to_string()),
            enabled: Set(true),
            env_json: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        match active.insert(conn).await {
            Ok(_) => {}
            // A concurrent ensure hit the UNIQUE constraint first — fine.
            Err(e) if e.to_string().contains("UNIQUE constraint failed") => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub async fn list(conn: &DatabaseConnection) -> Result<Vec<component_setting::Model>, DbError> {
    let rows = component_setting::Entity::find().all(conn).await?;
    Ok(rows)
}

pub async fn get(
    conn: &DatabaseConnection,
    component_id: &str,
) -> Result<Option<component_setting::Model>, DbError> {
    let model = component_setting::Entity::find()
        .filter(component_setting::Column::ComponentId.eq(component_id))
        .one(conn)
        .await?;
    Ok(model)
}

/// Persist the enabled toggle for a component, creating the row if the
/// built-in has not been listed yet.
pub async fn set_enabled(
    conn: &DatabaseConnection,
    component_id: &str,
    enabled: bool,
) -> Result<(), DbError> {
    ensure_defaults(conn, &[component_id]).await?;
    let model = component_setting::Entity::find()
        .filter(component_setting::Column::ComponentId.eq(component_id))
        .one(conn)
        .await?
        .ok_or_else(|| {
            DbError::Migration(format!("component setting not found: {component_id}"))
        })?;
    let mut active = model.into_active_model();
    active.enabled = Set(enabled);
    active.updated_at = Set(Utc::now());
    active.update(conn).await?;
    Ok(())
}

/// Persist the component's env overrides as a JSON object (`None` clears them).
pub async fn set_env(
    conn: &DatabaseConnection,
    component_id: &str,
    env_json: Option<String>,
) -> Result<(), DbError> {
    ensure_defaults(conn, &[component_id]).await?;
    let model = component_setting::Entity::find()
        .filter(component_setting::Column::ComponentId.eq(component_id))
        .one(conn)
        .await?
        .ok_or_else(|| {
            DbError::Migration(format!("component setting not found: {component_id}"))
        })?;
    let mut active = model.into_active_model();
    active.env_json = Set(env_json);
    active.updated_at = Set(Utc::now());
    active.update(conn).await?;
    Ok(())
}
