"use client"

import { useCallback, useMemo, useState } from "react"
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  BackgroundVariant,
} from "@xyflow/react"
import "@xyflow/react/dist/style.css"
import { Plus, Loader2 } from "lucide-react"
import { FolderSelect } from "@/components/shared/folder-select"
import { Button } from "@/components/ui/button"
import { useTaskTreeStore } from "./task-tree-store"
import { useTaskTreeData } from "./task-tree-data"
import { buildTreeLayout } from "./task-tree-model"
import { TaskTreeCardNode } from "./nodes/task-tree-card-node"
import { TaskTreeEditDialog } from "./task-tree-edit-dialog"
import { TaskTreeSessionDrawer } from "./task-tree-session-drawer"
import { TaskTreeDocumentPanel } from "./task-tree-document-panel"
import { workTaskTreeCreate, workTaskTreeDelete } from "@/lib/api"

const nodeTypes = { taskTreeCard: TaskTreeCardNode }

export default function TaskTreeView() {
  const {
    folderId,
    nodes,
    loading,
    editingNodeId,
    sessionDrawerTaskId,
    documentPanelTaskId,
    setFolderId,
    selectNode,
    openEditor,
    openSessionDrawer,
    closeSessionDrawer,
    openDocumentPanel,
    closeDocumentPanel,
    refetch,
  } = useTaskTreeStore()

  useTaskTreeData()

  const [newNodeParentId, setNewNodeParentId] = useState<number | null>(null)
  const [showNewNodeInput, setShowNewNodeInput] = useState(false)
  const [newNodeTitle, setNewNodeTitle] = useState("")

  const editingNode = useMemo(
    () => nodes.find((n) => n.id === editingNodeId) ?? null,
    [nodes, editingNodeId]
  )
  const sessionDrawerNode = useMemo(
    () => nodes.find((n) => n.id === sessionDrawerTaskId) ?? null,
    [nodes, sessionDrawerTaskId]
  )
  const documentPanelNode = useMemo(
    () => nodes.find((n) => n.id === documentPanelTaskId) ?? null,
    [nodes, documentPanelTaskId]
  )

  const handleSelect = useCallback(
    (id: number) => {
      selectNode(id)
      openSessionDrawer(id)
    },
    [selectNode, openSessionDrawer]
  )

  const handleAddChild = useCallback((parentId: number) => {
    setNewNodeParentId(parentId)
    setNewNodeTitle("")
    setShowNewNodeInput(true)
  }, [])

  const handleAddRoot = useCallback(() => {
    setNewNodeParentId(null)
    setNewNodeTitle("")
    setShowNewNodeInput(true)
  }, [])

  const handleConfirmAdd = useCallback(async () => {
    if (!folderId || !newNodeTitle.trim()) return
    await workTaskTreeCreate(folderId, newNodeParentId, newNodeTitle.trim())
    setNewNodeTitle("")
    setShowNewNodeInput(false)
    setNewNodeParentId(null)
    await refetch()
  }, [folderId, newNodeParentId, newNodeTitle, refetch])

  const handleDelete = useCallback(
    async (id: number) => {
      if (!confirm("Delete this node and all its children?")) return
      await workTaskTreeDelete(id)
      await refetch()
    },
    [refetch]
  )

  const { nodes: rfNodes, edges: rfEdges } = useMemo(
    () =>
      buildTreeLayout(nodes, {
        onSelect: handleSelect,
        onAddChild: handleAddChild,
        onEdit: (id) => openEditor(id),
        onDelete: handleDelete,
        onOpenSession: (id) => openSessionDrawer(id),
        onOpenDocuments: (id) => openDocumentPanel(id),
      }),
    [
      nodes,
      handleSelect,
      handleAddChild,
      handleDelete,
      openEditor,
      openSessionDrawer,
      openDocumentPanel,
    ]
  )

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center gap-3 px-4 py-2 border-b shrink-0">
        <FolderSelect
          value={folderId}
          onChange={(id) => setFolderId(id)}
          className="w-64"
        />
        <Button
          variant="outline"
          size="sm"
          onClick={handleAddRoot}
          disabled={!folderId}
        >
          <Plus className="size-3.5 mr-1" />
          Add root node
        </Button>

        {/* Inline new node input */}
        {showNewNodeInput && (
          <div className="flex items-center gap-2">
            <input
              autoFocus
              className="h-8 px-2 text-sm border rounded-md w-48"
              placeholder="Node title..."
              value={newNodeTitle}
              onChange={(e) => setNewNodeTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleConfirmAdd()
                if (e.key === "Escape") setShowNewNodeInput(false)
              }}
            />
            <Button
              size="sm"
              onClick={handleConfirmAdd}
              disabled={!newNodeTitle.trim()}
            >
              Add
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setShowNewNodeInput(false)}
            >
              Cancel
            </Button>
          </div>
        )}

        {loading && (
          <Loader2 className="size-4 animate-spin text-muted-foreground" />
        )}
      </div>

      {/* ReactFlow canvas */}
      <div className="flex-1 min-h-0">
        {folderId ? (
          <ReactFlow
            nodes={rfNodes}
            edges={rfEdges}
            nodeTypes={nodeTypes}
            fitView
            fitViewOptions={{ padding: 0.2 }}
            minZoom={0.1}
            maxZoom={2}
            proOptions={{ hideAttribution: true }}
            onPaneClick={() => selectNode(null)}
          >
            <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
            <Controls />
            <MiniMap zoomable pannable />
          </ReactFlow>
        ) : (
          <div className="h-full flex items-center justify-center text-muted-foreground text-sm">
            Select a project to view its task tree
          </div>
        )}
      </div>

      {/* Dialogs & Drawers */}
      <TaskTreeEditDialog
        node={editingNode}
        open={editingNodeId != null}
        onClose={() => openEditor(null)}
      />
      <TaskTreeSessionDrawer
        node={sessionDrawerNode}
        open={sessionDrawerTaskId != null}
        onClose={closeSessionDrawer}
      />
      <TaskTreeDocumentPanel
        node={documentPanelNode}
        open={documentPanelTaskId != null}
        onClose={closeDocumentPanel}
      />
    </div>
  )
}
