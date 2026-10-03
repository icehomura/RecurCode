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
import { Loader2, Expand, Shrink, RefreshCw, Trash2 } from "lucide-react"
import { FolderSelect } from "@/components/shared/folder-select"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { Button } from "@/components/ui/button"
import { useProjectViewStore } from "./project-view-store"
import { buildProjectLayout } from "./project-view-model"
import { ProjectNode } from "./nodes/project-node"
import { ProjectViewDetailPanel } from "./project-view-detail-panel"
import { projectStructureScan, projectStructureDeleteAll } from "@/lib/api"
import { toast } from "sonner"

const nodeTypes = { projectNode: ProjectNode }

export default function ProjectView() {
  const {
    folderId,
    nodes,
    loading,
    selectedNodeId,
    expandedFolderIds,
    setFolderId,
    selectNode,
    toggleFolder,
    expandAll,
    collapseAll,
    refetch,
  } = useProjectViewStore()

  // The picker needs the project roots: the same filter every other
  // `FolderSelect` call site applies.
  const folders = useAppWorkspaceStore((s) => s.folders)
  const projectFolders = useMemo(
    () => folders.filter((f) => f.parent_id == null && f.kind === "regular"),
    [folders]
  )

  const selectedNode = useMemo(
    () => nodes.find((n) => n.id === selectedNodeId) ?? null,
    [nodes, selectedNodeId]
  )

  const [scanning, setScanning] = useState(false)

  const handleScan = useCallback(async () => {
    if (!folderId) return
    setScanning(true)
    try {
      const result = await projectStructureScan(folderId)
      toast.success(`Scanned ${result.length} nodes`)
      await refetch()
    } catch (err) {
      toast.error("Scan failed", {
        description: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setScanning(false)
    }
  }, [folderId, refetch])

  const handleClear = useCallback(async () => {
    if (!folderId) return
    if (!confirm("Delete all project structure data for this folder?")) return
    try {
      await projectStructureDeleteAll(folderId)
      toast.success("Cleared project structure")
      await refetch()
    } catch (err) {
      toast.error("Clear failed", {
        description: err instanceof Error ? err.message : String(err),
      })
    }
  }, [folderId, refetch])

  const { nodes: rfNodes, edges: rfEdges } = useMemo(
    () =>
      buildProjectLayout(nodes, expandedFolderIds, {
        onToggle: toggleFolder,
        onSelect: selectNode,
      }),
    [nodes, expandedFolderIds, toggleFolder, selectNode]
  )

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-4 py-2 border-b shrink-0 flex-wrap">
        <FolderSelect
          folders={projectFolders}
          value={folderId}
          onChange={(id) => setFolderId(id)}
          className="w-64"
        />
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            handleScan().catch(console.error)
          }}
          disabled={!folderId || scanning}
        >
          {scanning ? (
            <RefreshCw className="size-3.5 mr-1 animate-spin" />
          ) : (
            <RefreshCw className="size-3.5 mr-1" />
          )}
          Scan Project
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={expandAll}
          disabled={!folderId}
        >
          <Expand className="size-3.5 mr-1" />
          Expand All
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={collapseAll}
          disabled={!folderId}
        >
          <Shrink className="size-3.5 mr-1" />
          Collapse All
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            handleClear().catch(console.error)
          }}
          disabled={!folderId}
          className="text-destructive hover:text-destructive"
        >
          <Trash2 className="size-3.5 mr-1" />
          Clear
        </Button>
        {loading && (
          <Loader2 className="size-4 animate-spin text-muted-foreground" />
        )}
        <div className="ml-auto text-xs text-muted-foreground">
          {nodes.length > 0 && `${nodes.length} nodes`}
        </div>
      </div>

      {/* ReactFlow canvas */}
      <div className="flex-1 min-h-0 relative">
        {folderId ? (
          <ReactFlow
            nodes={rfNodes}
            edges={rfEdges}
            nodeTypes={nodeTypes}
            fitView
            fitViewOptions={{ padding: 0.3 }}
            minZoom={0.05}
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
            Select a project folder to view its structure
          </div>
        )}

        {/* Detail panel */}
        <ProjectViewDetailPanel
          node={selectedNode}
          open={selectedNodeId != null}
          onClose={() => selectNode(null)}
        />
      </div>
    </div>
  )
}
