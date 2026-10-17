use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Per-component user state for the hardcoded built-in component
        // registry. No row is seeded here: `ensure_defaults` writes one row
        // (enabled = true) for every built-in the first time the registry is
        // listed or toggled, so the table can stay empty until then.
        manager
            .create_table(
                Table::create()
                    .table(ComponentSetting::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ComponentSetting::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ComponentSetting::ComponentId)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(ComponentSetting::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(ColumnDef::new(ComponentSetting::EnvJson).text().null())
                    .col(
                        ColumnDef::new(ComponentSetting::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ComponentSetting::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ComponentSetting::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ComponentSetting {
    Table,
    Id,
    ComponentId,
    Enabled,
    EnvJson,
    CreatedAt,
    UpdatedAt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    #[tokio::test]
    async fn up_creates_component_setting_table() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        // `enabled` defaults to true so a row inserted by a direct API call
        // (bypassing `ensure_defaults`) is still "on".
        conn.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO component_setting (component_id, created_at, updated_at) \
             VALUES ('@rg', datetime('now'), datetime('now'))"
                .to_string(),
        ))
        .await
        .expect("insert defaults row");

        let row = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT enabled FROM component_setting WHERE component_id = '@rg'".to_string(),
            ))
            .await
            .expect("select row")
            .expect("row exists");
        let enabled: bool = row.try_get("", "enabled").expect("enabled");
        assert!(enabled, "enabled defaults to true");

        // `component_id` is UNIQUE: a second row with the same id must fail.
        let duplicate = conn
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT INTO component_setting (component_id, created_at, updated_at) \
                 VALUES ('@rg', datetime('now'), datetime('now'))"
                    .to_string(),
            ))
            .await;
        assert!(duplicate.is_err(), "component_id is unique");
    }
}
