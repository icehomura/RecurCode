use sea_orm::entity::prelude::*;

/// Persisted per-component user state for the hardcoded built-in component
/// registry (see `commands::components::COMPONENTS`).
///
/// The registry itself is compiled in and has no create/delete API; this table
/// only stores the toggles and env overrides a user can change. `component_id`
/// is the registry's `ComponentDef::id` (e.g. `@rg`), so a renamed/removed
/// built-in simply leaves an orphan row that `ensure_defaults` never reads.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "component_setting")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub component_id: String,
    pub enabled: bool,
    pub env_json: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
