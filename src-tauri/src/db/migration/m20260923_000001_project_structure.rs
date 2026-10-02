use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProjectStructure::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ProjectStructure::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ProjectStructure::FolderId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ProjectStructure::Path).text().not_null())
                    .col(
                        ColumnDef::new(ProjectStructure::Kind)
                            .text()
                            .not_null()
                            .default("file"),
                    )
                    .col(ColumnDef::new(ProjectStructure::Name).text().not_null())
                    .col(ColumnDef::new(ProjectStructure::ParentId).integer())
                    .col(ColumnDef::new(ProjectStructure::Language).text())
                    .col(ColumnDef::new(ProjectStructure::Summary).text())
                    .col(ColumnDef::new(ProjectStructure::CallRelations).text())
                    .col(ColumnDef::new(ProjectStructure::Metadata).text())
                    .col(
                        ColumnDef::new(ProjectStructure::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(ProjectStructure::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ProjectStructure::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ps_folder")
                    .table(ProjectStructure::Table)
                    .col(ProjectStructure::FolderId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ps_path")
                    .table(ProjectStructure::Table)
                    .col(ProjectStructure::FolderId)
                    .col(ProjectStructure::Path)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ps_parent")
                    .table(ProjectStructure::Table)
                    .col(ProjectStructure::ParentId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ProjectStructure::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ProjectStructure {
    Table,
    Id,
    FolderId,
    Path,
    Kind,
    Name,
    ParentId,
    Language,
    Summary,
    CallRelations,
    Metadata,
    SortOrder,
    CreatedAt,
    UpdatedAt,
}
