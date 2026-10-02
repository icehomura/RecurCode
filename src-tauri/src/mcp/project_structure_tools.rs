use serde::{Deserialize, Serialize};

/// MCP tool definition for project structure operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStructureTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Returns the list of MCP tools for project structure management.
pub fn project_structure_mcp_tools() -> Vec<ProjectStructureTool> {
    vec![
        ProjectStructureTool {
            name: "project_structure_list".into(),
            description: "List the project structure tree for a folder. Returns all nodes with their hierarchy.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "folder_id": { "type": "integer", "description": "The folder ID to list structure for" }
                },
                "required": ["folder_id"]
            }),
        },
        ProjectStructureTool {
            name: "project_structure_update_node".into(),
            description: "Update a node in the project structure mind map. Can update name, summary, kind, call_relations (JSON), and metadata (JSON).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "integer", "description": "The node ID to update" },
                    "name": { "type": "string", "description": "New name for the node" },
                    "summary": { "type": "string", "description": "New summary/description" },
                    "kind": { "type": "string", "description": "Node kind: folder|file|function|class|method|struct|enum|trait|interface|module|config|doc" },
                    "call_relations": { "type": "string", "description": "JSON array of call edges: [{\"from_path\":\"...\", \"to_path\":\"...\", \"kind\":\"calls|imports|extends|implements\"}]" },
                    "metadata": { "type": "string", "description": "JSON metadata object" }
                },
                "required": ["node_id"]
            }),
        },
        ProjectStructureTool {
            name: "project_structure_delete_all".into(),
            description: "Delete all project structure data for a folder. Use before re-scanning.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "folder_id": { "type": "integer", "description": "The folder ID to clear" }
                },
                "required": ["folder_id"]
            }),
        },
    ]
}
