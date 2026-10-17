use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // `default_enabled` omitted `AgentType::Ra`, so every database that
        // seeded its `agent_setting` row before the rebrand created RecurAgent
        // DISABLED. A disabled agent is filtered out of the picker and can
        // never be the default, which is the opposite of the rebrand's intent
        // (RecurAgent is now the pinned-first default). Turn it on; the two
        // other `Ra`-first migrations pin its sort order. Custom agents and
        // every other row keep whatever the user chose.
        let sql = "UPDATE agent_setting SET enabled = 1 WHERE agent_type = '\"ra\"'";
        manager
            .get_connection()
            .execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Re-disabling would hide the default agent again; leave it enabled.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::Database;

    #[tokio::test]
    async fn up_enables_only_recur_agent() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE agent_setting (id INTEGER PRIMARY KEY, agent_type TEXT, enabled INTEGER)",
        )
        .await
        .expect("create stub table");
        for (id, ty, en) in [
            (1, r#""ra""#, 0),
            (2, r#""claude_code""#, 1),
            (3, r#""my-custom""#, 0),
        ] {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO agent_setting (id, agent_type, enabled) VALUES (?, ?, ?)",
                [id.into(), ty.into(), en.into()],
            ))
            .await
            .expect("insert row");
        }

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        let ra: i32 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT enabled FROM agent_setting WHERE agent_type = '\"ra\"'".to_string(),
            ))
            .await
            .expect("query ra")
            .expect("ra row")
            .try_get("", "enabled")
            .expect("enabled");
        let custom: i32 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT enabled FROM agent_setting WHERE agent_type = '\"my-custom\"'".to_string(),
            ))
            .await
            .expect("query custom")
            .expect("custom row")
            .try_get("", "enabled")
            .expect("enabled");
        assert_eq!(ra, 1, "recur agent enabled");
        assert_eq!(custom, 0, "custom untouched");
    }
}
