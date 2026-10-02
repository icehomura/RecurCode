use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Tree structure columns for the task tree board view.
        // `parent_id`: adjacency-list pointer; NULL = root node (project-level).
        // `tree_description`: introduction/description text shown on tree cards.
        manager
            .alter_table(
                Table::alter()
                    .table(WorkTask::Table)
                    .add_column(ColumnDef::new(WorkTask::ParentId).integer().null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(WorkTask::Table)
                    .add_column(ColumnDef::new(WorkTask::TreeDescription).text().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_work_task_parent")
                    .table(WorkTask::Table)
                    .col(WorkTask::ParentId)
                    .to_owned(),
            )
            .await?;

        // task_tree_goal: one-to-many goals per tree node, displayed on cards.
        manager
            .create_table(
                Table::create()
                    .table(TaskTreeGoal::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TaskTreeGoal::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TaskTreeGoal::TaskId).integer().not_null())
                    .col(ColumnDef::new(TaskTreeGoal::Title).text().not_null())
                    .col(
                        ColumnDef::new(TaskTreeGoal::SortOrder)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(TaskTreeGoal::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TaskTreeGoal::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TaskTreeGoal::Table, TaskTreeGoal::TaskId)
                            .to(WorkTask::Table, WorkTask::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_task_tree_goal_task")
                    .table(TaskTreeGoal::Table)
                    .col(TaskTreeGoal::TaskId)
                    .to_owned(),
            )
            .await?;

        // task_tree_document: AI-generated or user-created documents (PRD, plan, notes)
        // attached to a tree node.
        manager
            .create_table(
                Table::create()
                    .table(TaskTreeDocument::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TaskTreeDocument::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TaskTreeDocument::TaskId).integer().not_null())
                    .col(ColumnDef::new(TaskTreeDocument::Title).text().not_null())
                    .col(ColumnDef::new(TaskTreeDocument::Content).text().not_null())
                    // 'plan' | 'prd' | 'note' | other
                    .col(ColumnDef::new(TaskTreeDocument::DocumentType).text().not_null())
                    .col(
                        ColumnDef::new(TaskTreeDocument::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TaskTreeDocument::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TaskTreeDocument::Table, TaskTreeDocument::TaskId)
                            .to(WorkTask::Table, WorkTask::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_task_tree_document_task")
                    .table(TaskTreeDocument::Table)
                    .col(TaskTreeDocument::TaskId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(TaskTreeDocument::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(TaskTreeGoal::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_work_task_parent")
                    .table(WorkTask::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(WorkTask::Table)
                    .drop_column(WorkTask::TreeDescription)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(WorkTask::Table)
                    .drop_column(WorkTask::ParentId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum WorkTask {
    Table,
    Id,
    ParentId,
    TreeDescription,
}

#[derive(DeriveIden)]
enum TaskTreeGoal {
    Table,
    Id,
    TaskId,
    Title,
    SortOrder,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TaskTreeDocument {
    Table,
    Id,
    TaskId,
    Title,
    Content,
    DocumentType,
    CreatedAt,
    UpdatedAt,
}
