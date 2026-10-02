use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "task_tree_document")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub task_id: i32,
    pub title: String,
    /// Document content in Markdown format.
    #[sea_orm(column_type = "Text")]
    pub content: String,
    /// 'plan' | 'prd' | 'note' | other freeform type.
    #[sea_orm(column_type = "Text")]
    pub document_type: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::work_task::Entity",
        from = "Column::TaskId",
        to = "super::work_task::Column::Id"
    )]
    Task,
}

impl Related<super::work_task::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Task.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
