use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // RecurCode rebrands and pins Recur Agent (`ra`) as the first, default
        // agent. `agent_setting.sort_order` is seeded only at first insert
        // (`ensure_defaults`), so a database created before the rebrand still
        // has the OLD order (Claude Code = 0, `ra` last). The settings drag-
        // sort writes these same integers, so re-pinning them here is safe: it
        // is a one-time rebrand of the built-in order, and any later drag
        // simply overwrites the value again. Custom agents (sort_order
        // `i32::MAX/2`) are left untouched by the ELSE branch.
        //
        // `agent_type` stores the serde JSON of `AgentType` (a quoted wire
        // name, e.g. `"ra"`), so the CASE values carry the quotes.
        let sql = "UPDATE agent_setting SET sort_order = CASE agent_type \
            WHEN '\"ra\"' THEN 0 \
            WHEN '\"claude_code\"' THEN 1 \
            WHEN '\"codex\"' THEN 2 \
            WHEN '\"open_code\"' THEN 3 \
            WHEN '\"gemini\"' THEN 4 \
            WHEN '\"open_claw\"' THEN 5 \
            WHEN '\"cline\"' THEN 6 \
            WHEN '\"hermes\"' THEN 7 \
            WHEN '\"code_buddy\"' THEN 8 \
            WHEN '\"kimi_code\"' THEN 9 \
            WHEN '\"pi\"' THEN 10 \
            WHEN '\"grok\"' THEN 11 \
            WHEN '\"cursor\"' THEN 12 \
            WHEN '\"deepseek\"' THEN 13 \
            WHEN '\"qoder\"' THEN 14 \
            WHEN '\"antigravity\"' THEN 15 \
            ELSE sort_order END";
        manager
            .get_connection()
            .execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Order is user-editable state; there is no safe inverse beyond the
        // old hardcoded list. Leave a no-op: the re-pin is harmless.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::Database;

    #[tokio::test]
    async fn up_pins_recur_agent_first_and_leaves_customs_alone() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE agent_setting (id INTEGER PRIMARY KEY, agent_type TEXT, sort_order INTEGER)",
        )
        .await
        .expect("create stub table");
        for (id, ty, so) in [
            (1, r#""claude_code""#, 0),
            (2, r#""ra""#, 15),
            (3, r#""antigravity""#, 14),
            (4, r#""my-custom""#, 1000),
        ] {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO agent_setting (id, agent_type, sort_order) VALUES (?, ?, ?)",
                [id.into(), ty.into(), so.into()],
            ))
            .await
            .expect("insert row");
        }

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        let mut got = std::collections::HashMap::new();
        let rows = conn
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT agent_type, sort_order FROM agent_setting".to_string(),
            ))
            .await
            .expect("select rows");
        for row in rows {
            let ty: String = row.try_get("", "agent_type").expect("agent_type");
            let so: i32 = row.try_get("", "sort_order").expect("sort_order");
            got.insert(ty, so);
        }
        assert_eq!(got.get(r#""ra""#), Some(&0), "ra first");
        assert_eq!(got.get(r#""claude_code""#), Some(&1));
        assert_eq!(got.get(r#""antigravity""#), Some(&15));
        assert_eq!(got.get(r#""my-custom""#), Some(&1000), "custom untouched");
    }
}
