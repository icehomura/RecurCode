use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    IntoActiveModel, QueryFilter, QueryOrder, Set,
};

use crate::db::entities::project_structure;
use crate::db::error::DbError;
use crate::models::project_structure::{ProjectStructureNodeDraft, ProjectStructureNodeInfo};

fn to_info(m: project_structure::Model) -> ProjectStructureNodeInfo {
    ProjectStructureNodeInfo {
        id: m.id,
        folder_id: m.folder_id,
        path: m.path,
        kind: m.kind,
        name: m.name,
        parent_id: m.parent_id,
        language: m.language,
        summary: m.summary,
        call_relations: m.call_relations,
        metadata: m.metadata,
        sort_order: m.sort_order,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn list(
    conn: &DatabaseConnection,
    folder_id: i32,
) -> Result<Vec<ProjectStructureNodeInfo>, DbError> {
    let rows = project_structure::Entity::find()
        .filter(project_structure::Column::FolderId.eq(folder_id))
        .order_by_asc(project_structure::Column::SortOrder)
        .all(conn)
        .await?;

    Ok(rows.into_iter().map(to_info).collect())
}

pub async fn upsert_bulk(
    conn: &DatabaseConnection,
    folder_id: i32,
    nodes: Vec<ProjectStructureNodeDraft>,
) -> Result<Vec<ProjectStructureNodeInfo>, DbError> {
    // Delete all existing nodes for this folder first.
    delete_all(conn, folder_id).await?;

    let now = Utc::now();
    let mut results = Vec::with_capacity(nodes.len());

    for (idx, draft) in nodes.into_iter().enumerate() {
        let sort_order = draft.sort_order.unwrap_or(idx as i32);

        let active = project_structure::ActiveModel {
            id: NotSet,
            folder_id: Set(folder_id),
            path: Set(draft.path),
            kind: Set(draft.kind),
            name: Set(draft.name),
            parent_id: Set(draft.parent_id),
            language: Set(draft.language),
            summary: Set(draft.summary),
            call_relations: Set(draft.call_relations),
            metadata: Set(draft.metadata),
            sort_order: Set(sort_order),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let model = active.insert(conn).await?;
        results.push(to_info(model));
    }

    Ok(results)
}

pub async fn update_node(
    conn: &DatabaseConnection,
    id: i32,
    name: Option<String>,
    summary: Option<String>,
    call_relations: Option<String>,
    metadata: Option<String>,
    kind: Option<String>,
) -> Result<ProjectStructureNodeInfo, DbError> {
    let row = project_structure::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("ProjectStructure node {} not found", id)))?;

    let mut active = row.into_active_model();
    if let Some(v) = name {
        active.name = Set(v);
    }
    if let Some(v) = summary {
        active.summary = Set(Some(v));
    }
    if let Some(v) = call_relations {
        active.call_relations = Set(Some(v));
    }
    if let Some(v) = metadata {
        active.metadata = Set(Some(v));
    }
    if let Some(v) = kind {
        active.kind = Set(v);
    }
    active.updated_at = Set(Utc::now());

    let model = active.update(conn).await?;
    Ok(to_info(model))
}

pub async fn delete_all(conn: &DatabaseConnection, folder_id: i32) -> Result<(), DbError> {
    project_structure::Entity::delete_many()
        .filter(project_structure::Column::FolderId.eq(folder_id))
        .exec(conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::fresh_in_memory_db;

    fn make_draft(path: &str, kind: &str, name: &str) -> ProjectStructureNodeDraft {
        ProjectStructureNodeDraft {
            path: path.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            parent_id: None,
            language: None,
            summary: None,
            call_relations: None,
            metadata: None,
            sort_order: None,
        }
    }

    #[tokio::test]
    async fn create_and_list_nodes() {
        let db = fresh_in_memory_db().await;
        let folder_id = 1;

        let drafts = vec![
            make_draft("src/main.rs", "file", "main.rs"),
            make_draft("src/lib.rs", "file", "lib.rs"),
        ];
        let created = upsert_bulk(&db.conn, folder_id, drafts).await.unwrap();
        assert_eq!(created.len(), 2);
        assert_eq!(created[0].name, "main.rs");
        assert_eq!(created[1].name, "lib.rs");
        assert_eq!(created[0].sort_order, 0);
        assert_eq!(created[1].sort_order, 1);

        let listed = list(&db.conn, folder_id).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, created[0].id);
        assert_eq!(listed[1].id, created[1].id);
    }

    #[tokio::test]
    async fn upsert_replaces_existing() {
        let db = fresh_in_memory_db().await;
        let folder_id = 1;

        let first = vec![make_draft("old.rs", "file", "old.rs")];
        upsert_bulk(&db.conn, folder_id, first).await.unwrap();

        let second = vec![
            make_draft("new_a.rs", "file", "new_a.rs"),
            make_draft("new_b.rs", "file", "new_b.rs"),
        ];
        let replaced = upsert_bulk(&db.conn, folder_id, second).await.unwrap();

        let listed = list(&db.conn, folder_id).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "new_a.rs");
        assert_eq!(listed[1].name, "new_b.rs");
        assert_eq!(replaced.len(), 2);
    }

    #[tokio::test]
    async fn delete_all_clears_folder() {
        let db = fresh_in_memory_db().await;
        let folder_id = 1;

        let drafts = vec![
            make_draft("a.rs", "file", "a.rs"),
            make_draft("b.rs", "file", "b.rs"),
        ];
        upsert_bulk(&db.conn, folder_id, drafts).await.unwrap();

        delete_all(&db.conn, folder_id).await.unwrap();

        let listed = list(&db.conn, folder_id).await.unwrap();
        assert!(listed.is_empty());
    }
}
