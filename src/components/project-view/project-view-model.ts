"use client"

import type { Node, Edge } from "@xyflow/react"
import type { ProjectStructureNode, ProjectCallEdge } from "@/lib/types"
import {
  Folder,
  FileText,
  Code2,
  Box,
  Cog,
  BookOpen,
  Layers,
} from "lucide-react"

export interface ProjectNodeData {
  node: ProjectStructureNode
  hasChildren: boolean
  isExpanded: boolean
  onToggle: (id: number) => void
  onSelect: (id: number) => void
}

const NODE_WIDTH = 240
const NODE_HEIGHT = 64
const H_GAP = 48
const V_GAP = 32

const CALL_EDGE_STYLES: Record<
  ProjectCallEdge["kind"],
  { color: string; strokeDasharray: string }
> = {
  calls: { color: "#f97316", strokeDasharray: "8 4" },
  imports: { color: "#3b82f6", strokeDasharray: "4 4" },
  extends: { color: "#22c55e", strokeDasharray: "none" },
  implements: { color: "#a855f7", strokeDasharray: "none" },
}

const KIND_ICONS: Record<string, typeof Folder> = {
  folder: Folder,
  file: FileText,
  module: Layers,
  class: Box,
  function: Code2,
  method: Code2,
  interface: Layers,
  trait: Layers,
  struct: Box,
  enum: Box,
  config: Cog,
  doc: BookOpen,
}

export function getIconForKind(kind: string) {
  return KIND_ICONS[kind] ?? FileText
}

const KIND_COLORS: Record<string, string> = {
  folder: "text-amber-500",
  file: "text-muted-foreground",
  module: "text-blue-500",
  class: "text-purple-500",
  function: "text-green-500",
  method: "text-green-400",
  interface: "text-cyan-500",
  trait: "text-cyan-400",
  struct: "text-orange-500",
  enum: "text-orange-400",
  config: "text-slate-400",
  doc: "text-pink-500",
}

export function getColorForKind(kind: string) {
  return KIND_COLORS[kind] ?? "text-muted-foreground"
}

/**
 * Build visible ReactFlow nodes/edges from the flat list,
 * respecting expandedFolderIds to show/hide children.
 */
export function buildProjectLayout(
  allNodes: ProjectStructureNode[],
  expandedFolderIds: Set<number>,
  callbacks: {
    onToggle: (id: number) => void
    onSelect: (id: number) => void
  },
): { nodes: Node<ProjectNodeData>[]; edges: Edge[] } {
  if (allNodes.length === 0) return { nodes: [], edges: [] }

  const childrenOf = new Map<number | null, ProjectStructureNode[]>()
  for (const n of allNodes) {
    const key = n.parent_id
    if (!childrenOf.has(key)) childrenOf.set(key, [])
    childrenOf.get(key)!.push(n)
  }
  for (const children of childrenOf.values()) {
    children.sort((a, b) => a.sort_order - b.sort_order)
  }

  const rfNodes: Node<ProjectNodeData>[] = []
  const rfEdges: Edge[] = []

  type QueueEntry = { node: ProjectStructureNode; x: number; y: number }
  const queue: QueueEntry[] = []

  const roots = childrenOf.get(null) ?? []
  const totalRootWidth =
    roots.length * NODE_WIDTH + (roots.length - 1) * H_GAP
  let startX = -totalRootWidth / 2
  for (const root of roots) {
    queue.push({ node: root, x: startX, y: 0 })
    startX += NODE_WIDTH + H_GAP
  }

  while (queue.length > 0) {
    const { node, x, y } = queue.shift()!
    const children = childrenOf.get(node.id) ?? []
    const hasChildren = children.length > 0
    const isFolder = node.kind === "folder"
    const isExpanded = expandedFolderIds.has(node.id)

    rfNodes.push({
      id: String(node.id),
      type: "projectNode",
      position: { x, y },
      data: {
        node,
        hasChildren,
        isExpanded,
        onToggle: callbacks.onToggle,
        onSelect: callbacks.onSelect,
      },
    })

    // Only show children of expanded folders
    if (!isFolder || !isExpanded || children.length === 0) continue

    const totalChildWidth =
      children.length * NODE_WIDTH + (children.length - 1) * H_GAP
    let childX = x + NODE_WIDTH / 2 - totalChildWidth / 2
    const childY = y + NODE_HEIGHT + V_GAP

    for (const child of children) {
      rfEdges.push({
        id: `e-${node.id}-${child.id}`,
        source: String(node.id),
        target: String(child.id),
        type: "smoothstep",
      })
      queue.push({ node: child, x: childX, y: childY })
      childX += NODE_WIDTH + H_GAP
    }
  }

  // Build a path-to-nodeId lookup for call edge resolution
  const pathToNodeId = new Map<string, string>()
  for (const rfNode of rfNodes) {
    pathToNodeId.set(rfNode.data.node.path, rfNode.id)
  }

  // Parse call_relations from every node and create styled call edges
  const callEdgeIds = new Set<string>()
  for (const rfNode of rfNodes) {
    const raw = rfNode.data.node.call_relations
    if (!raw) continue

    let relations: ProjectCallEdge[]
    try {
      relations = JSON.parse(raw) as ProjectCallEdge[]
    } catch {
      continue
    }

    for (const rel of relations) {
      const sourceId = pathToNodeId.get(rel.from_path)
      const targetId = pathToNodeId.get(rel.to_path)
      if (!sourceId || !targetId) continue
      if (sourceId === targetId) continue

      const edgeId = `ec-${sourceId}-${targetId}-${rel.kind}`
      if (callEdgeIds.has(edgeId)) continue
      callEdgeIds.add(edgeId)

      const style = CALL_EDGE_STYLES[rel.kind]

      rfEdges.push({
        id: edgeId,
        source: sourceId,
        target: targetId,
        type: "default",
        animated: rel.kind === "calls",
        label: rel.kind,
        labelStyle: {
          fill: style.color,
          fontWeight: 600,
          fontSize: 11,
        },
        labelBgStyle: {
          fill: "hsl(var(--background))",
          fillOpacity: 0.9,
        },
        style: {
          stroke: style.color,
          strokeWidth: 1.5,
          strokeDasharray: style.strokeDasharray,
        },
      })
    }
  }

  return { nodes: rfNodes, edges: rfEdges }
}
