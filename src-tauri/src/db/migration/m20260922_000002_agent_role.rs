use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AgentRole::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AgentRole::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AgentRole::Name).string().not_null())
                    .col(
                        ColumnDef::new(AgentRole::Description)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(AgentRole::SystemPrompt).text().not_null())
                    .col(ColumnDef::new(AgentRole::AgentType).string())
                    .col(ColumnDef::new(AgentRole::ModelId).string())
                    .col(ColumnDef::new(AgentRole::ToolsJson).text())
                    .col(ColumnDef::new(AgentRole::Temperature).double())
                    .col(
                        ColumnDef::new(AgentRole::IsBuiltin)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(AgentRole::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(AgentRole::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AgentRole::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_agent_role_sort_order")
                    .table(AgentRole::Table)
                    .col(AgentRole::SortOrder)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AgentRole::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum AgentRole {
    Table,
    Id,
    Name,
    Description,
    SystemPrompt,
    AgentType,
    ModelId,
    ToolsJson,
    Temperature,
    IsBuiltin,
    SortOrder,
    CreatedAt,
    UpdatedAt,
}
