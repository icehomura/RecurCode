#[cfg(feature = "tauri-runtime")]
use crate::db::error::DbError;
#[cfg(feature = "tauri-runtime")]
use crate::db::service::project_structure_service;
#[cfg(feature = "tauri-runtime")]
use crate::db::AppDatabase;
#[cfg(feature = "tauri-runtime")]
use crate::models::ProjectStructureNodeInfo;

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn project_structure_list(
    db: tauri::State<'_, AppDatabase>,
    folder_id: i32,
) -> Result<Vec<ProjectStructureNodeInfo>, DbError> {
    project_structure_service::list(&db.conn, folder_id).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn project_structure_scan(
    db: tauri::State<'_, AppDatabase>,
    folder_id: i32,
) -> Result<Vec<ProjectStructureNodeInfo>, DbError> {
    // Look up the folder path from the database
    use crate::db::entities::folder;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let folder_row = folder::Entity::find()
        .filter(folder::Column::Id.eq(folder_id))
        .one(&db.conn)
        .await
        .map_err(|e| DbError::Database(e))?;

    let folder_path = match folder_row {
        Some(f) => std::path::PathBuf::from(&f.path),
        None => return Err(DbError::NotFound(format!("Folder {folder_id} not found"))),
    };

    if !folder_path.exists() {
        return Err(DbError::Validation(format!(
            "Folder path does not exist: {}",
            folder_path.display()
        )));
    }

    // Run the scanner
    crate::parsers::project_structure::scan_project(&folder_path, folder_id, &db)
        .await
        .map_err(|e| DbError::Validation(e))
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn project_structure_update_node(
    db: tauri::State<'_, AppDatabase>,
    node_id: i32,
    name: Option<String>,
    summary: Option<String>,
    call_relations: Option<String>,
    metadata: Option<String>,
    kind: Option<String>,
) -> Result<ProjectStructureNodeInfo, DbError> {
    project_structure_service::update_node(
        &db.conn,
        node_id,
        name,
        summary,
        call_relations,
        metadata,
        kind,
    )
    .await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn project_structure_delete_all(
    db: tauri::State<'_, AppDatabase>,
    folder_id: i32,
) -> Result<(), DbError> {
    project_structure_service::delete_all(&db.conn, folder_id).await
}
