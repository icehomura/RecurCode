use chrono::Utc;
use sea_orm::DatabaseConnection;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DbBackend, EntityTrait,
    IntoActiveModel, QueryFilter, QueryOrder, Set,
};

use crate::db::entities::agent_role;
use crate::db::error::DbError;
use crate::models::AgentRoleInfo;

fn to_info(m: agent_role::Model) -> AgentRoleInfo {
    AgentRoleInfo {
        id: m.id,
        name: m.name,
        description: m.description,
        system_prompt: m.system_prompt,
        agent_type: m.agent_type,
        model_id: m.model_id,
        tools_json: m.tools_json,
        temperature: m.temperature,
        is_builtin: m.is_builtin,
        sort_order: m.sort_order,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn list(conn: &DatabaseConnection) -> Result<Vec<AgentRoleInfo>, DbError> {
    let rows = agent_role::Entity::find()
        .order_by_asc(agent_role::Column::SortOrder)
        .all(conn)
        .await?;

    Ok(rows.into_iter().map(to_info).collect())
}

pub async fn get(
    conn: &DatabaseConnection,
    id: i32,
) -> Result<Option<AgentRoleInfo>, DbError> {
    let row = agent_role::Entity::find_by_id(id).one(conn).await?;
    Ok(row.map(to_info))
}

pub async fn create(
    conn: &DatabaseConnection,
    name: &str,
    description: &str,
    system_prompt: &str,
    agent_type: Option<&str>,
    model_id: Option<&str>,
    tools_json: Option<&str>,
    temperature: Option<f64>,
) -> Result<AgentRoleInfo, DbError> {
    let now = Utc::now();

    let max_order = agent_role::Entity::find()
        .order_by_desc(agent_role::Column::SortOrder)
        .one(conn)
        .await?
        .map(|m| m.sort_order)
        .unwrap_or(-1);

    let active = agent_role::ActiveModel {
        id: NotSet,
        name: Set(name.to_string()),
        description: Set(description.to_string()),
        system_prompt: Set(system_prompt.to_string()),
        agent_type: Set(agent_type.map(|s| s.to_string())),
        model_id: Set(model_id.map(|s| s.to_string())),
        tools_json: Set(tools_json.map(|s| s.to_string())),
        temperature: Set(temperature),
        is_builtin: Set(false),
        sort_order: Set(max_order + 1),
        created_at: Set(now),
        updated_at: Set(now),
    };

    let model = active.insert(conn).await?;
    Ok(to_info(model))
}

pub async fn update(
    conn: &DatabaseConnection,
    id: i32,
    name: Option<String>,
    description: Option<String>,
    system_prompt: Option<String>,
    agent_type: Option<String>,
    model_id: Option<String>,
    tools_json: Option<String>,
    temperature: Option<f64>,
    clear_agent_type: bool,
    clear_model_id: bool,
    clear_tools_json: bool,
    clear_temperature: bool,
) -> Result<AgentRoleInfo, DbError> {
    let row = agent_role::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::Migration(format!("AgentRole {} not found", id)))?;

    let mut active = row.into_active_model();
    if let Some(v) = name {
        active.name = Set(v);
    }
    if let Some(v) = description {
        active.description = Set(v);
    }
    if let Some(v) = system_prompt {
        active.system_prompt = Set(v);
    }
    if clear_agent_type {
        active.agent_type = Set(None);
    } else if let Some(v) = agent_type {
        active.agent_type = Set(Some(v));
    }
    if clear_model_id {
        active.model_id = Set(None);
    } else if let Some(v) = model_id {
        active.model_id = Set(Some(v));
    }
    if clear_tools_json {
        active.tools_json = Set(None);
    } else if let Some(v) = tools_json {
        active.tools_json = Set(Some(v));
    }
    if clear_temperature {
        active.temperature = Set(None);
    } else if let Some(v) = temperature {
        active.temperature = Set(Some(v));
    }
    active.updated_at = Set(Utc::now());

    let model = active.update(conn).await?;
    Ok(to_info(model))
}

pub async fn delete(conn: &DatabaseConnection, id: i32) -> Result<(), DbError> {
    agent_role::Entity::delete_by_id(id).exec(conn).await?;
    Ok(())
}

pub async fn reorder(conn: &DatabaseConnection, ids: Vec<i32>) -> Result<(), DbError> {
    if ids.is_empty() {
        return Ok(());
    }

    let now = Utc::now();

    // Update each row individually using SeaORM query builder to avoid raw SQL.
    for (idx, id) in ids.iter().enumerate() {
        agent_role::Entity::update_many()
            .col_expr(
                agent_role::Column::SortOrder,
                sea_orm::sea_query::Expr::val(idx as i32).into(),
            )
            .col_expr(
                agent_role::Column::UpdatedAt,
                sea_orm::sea_query::Expr::val(now).into(),
            )
            .filter(agent_role::Column::Id.eq(*id))
            .exec(conn)
            .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::fresh_in_memory_db;

    async fn seed_role(
        conn: &DatabaseConnection,
        name: &str,
    ) -> AgentRoleInfo {
        create(conn, name, "desc", "sys prompt", None, None, None, None)
            .await
            .expect("seed role")
    }

    #[tokio::test]
    async fn create_and_list_roles() {
        let db = fresh_in_memory_db().await;
        let r = seed_role(&db.conn, "Coder").await;
        assert_eq!(r.name, "Coder");
        assert!(!r.is_builtin);

        let all = list(&db.conn).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, r.id);
    }

    #[tokio::test]
    async fn get_role_by_id() {
        let db = fresh_in_memory_db().await;
        let created = create(
            &db.conn,
            "Researcher",
            "does research",
            "be thorough",
            Some("claude_code"),
            Some("claude-sonnet-4-20250514"),
            Some("[\"web_search\"]"),
            Some(0.7),
        )
        .await
        .unwrap();

        let fetched = get(&db.conn, created.id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Researcher");
        assert_eq!(fetched.description, "does research");
        assert_eq!(fetched.system_prompt, "be thorough");
        assert_eq!(fetched.agent_type.as_deref(), Some("claude_code"));
        assert_eq!(fetched.model_id.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(fetched.tools_json.as_deref(), Some("[\"web_search\"]"));
        assert!((fetched.temperature.unwrap() - 0.7).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn update_role() {
        let db = fresh_in_memory_db().await;
        let r = seed_role(&db.conn, "old name").await;

        let updated = update(
            &db.conn,
            r.id,
            Some("new name".into()),
            None,
            Some("updated prompt".into()),
            None,
            None,
            None,
            Some(0.5),
            false,
            false,
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(updated.name, "new name");
        assert_eq!(updated.system_prompt, "updated prompt");
        assert!((updated.temperature.unwrap() - 0.5).abs() < f64::EPSILON);
        // Unchanged fields preserved.
        assert_eq!(updated.description, "desc");
        assert!(updated.updated_at >= r.created_at);
    }

    #[tokio::test]
    async fn delete_role() {
        let db = fresh_in_memory_db().await;
        let r = seed_role(&db.conn, "doomed").await;

        delete(&db.conn, r.id).await.unwrap();

        let gone = get(&db.conn, r.id).await.unwrap();
        assert!(gone.is_none());
        assert!(list(&db.conn).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn reorder_roles() {
        let db = fresh_in_memory_db().await;
        let a = seed_role(&db.conn, "A").await;
        let b = seed_role(&db.conn, "B").await;
        let c = seed_role(&db.conn, "C").await;

        // Reverse: C, B, A
        reorder(&db.conn, vec![c.id, b.id, a.id]).await.unwrap();

        let ordered = list(&db.conn).await.unwrap();
        assert_eq!(
            ordered.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["C", "B", "A"],
        );
        assert_eq!(ordered[0].sort_order, 0);
        assert_eq!(ordered[1].sort_order, 1);
        assert_eq!(ordered[2].sort_order, 2);
    }

    #[tokio::test]
    async fn sort_order_auto_increments() {
        let db = fresh_in_memory_db().await;

        let r1 = seed_role(&db.conn, "first").await;
        let r2 = seed_role(&db.conn, "second").await;
        let r3 = seed_role(&db.conn, "third").await;

        assert_eq!(r1.sort_order, 0);
        assert_eq!(r2.sort_order, 1);
        assert_eq!(r3.sort_order, 2);

        // After deleting the middle one, a new role should get sort_order = 3,
        // not reuse the gap.
        delete(&db.conn, r2.id).await.unwrap();
        let r4 = seed_role(&db.conn, "fourth").await;
        assert_eq!(r4.sort_order, 3);
    }
}
