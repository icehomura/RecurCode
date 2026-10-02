import type { Node, Edge } from "@xyflow/react"
import type { TaskTreeNode } from "@/lib/types"

export interface TaskTreeCardData {
  node: TaskTreeNode
  onSelect: (taskId: number) => void
  onAddChild: (parentId: number) => void
  onEdit: (taskId: number) => void
  onDelete: (taskId: number) => void
  onOpenSession: (taskId: number) => void
  onOpenDocuments: (taskId: number) => void
}

const NODE_WIDTH = 280
const NODE_HEIGHT_BASE = 100
const NODE_HEIGHT_PER_GOAL = 24
const HORIZONTAL_GAP = 60
const VERTICAL_GAP = 40

/**
 * Convert a flat list of TaskTreeNodes into ReactFlow Node[] and Edge[].
 * Uses a simple top-down BFS layout: root nodes at the top, children below.
 */
export function buildTreeLayout(
  nodes: TaskTreeNode[],
  callbacks: {
    onSelect: (id: number) => void
    onAddChild: (id: number) => void
    onEdit: (id: number) => void
    onDelete: (id: number) => void
    onOpenSession: (id: number) => void
    onOpenDocuments: (id: number) => void
  }
): { nodes: Node<TaskTreeCardData>[]; edges: Edge[] } {
  if (nodes.length === 0) return { nodes: [], edges: [] }

  const byId = new Map<number, TaskTreeNode>()
  const childrenOf = new Map<number | null, TaskTreeNode[]>()

  for (const n of nodes) {
    byId.set(n.id, n)
    const key = n.parent_id
    if (!childrenOf.has(key)) childrenOf.set(key, [])
    childrenOf.get(key)!.push(n)
  }

  // Sort children by sort_order within each group.
  for (const children of childrenOf.values()) {
    children.sort((a, b) => a.sort_order - b.sort_order)
  }

  const rfNodes: Node<TaskTreeCardData>[] = []
  const rfEdges: Edge[] = []

  // BFS to assign positions level by level.
  const roots = childrenOf.get(null) ?? []
  // Sort roots by sort_order.
  roots.sort((a, b) => a.sort_order - b.sort_order)

  type QueueEntry = { node: TaskTreeNode; x: number; y: number }
  const queue: QueueEntry[] = []

  // Position roots horizontally centered.
  const totalRootWidth =
    roots.length * NODE_WIDTH + (roots.length - 1) * HORIZONTAL_GAP
  let startX = -totalRootWidth / 2

  for (const root of roots) {
    queue.push({ node: root, x: startX, y: 0 })
    startX += NODE_WIDTH + HORIZONTAL_GAP
  }

  while (queue.length > 0) {
    const { node, x, y } = queue.shift()!
    const goalCount = node.goals?.length ?? 0
    const height = NODE_HEIGHT_BASE + goalCount * NODE_HEIGHT_PER_GOAL

    rfNodes.push({
      id: String(node.id),
      type: "taskTreeCard",
      position: { x, y },
      data: {
        node,
        onSelect: callbacks.onSelect,
        onAddChild: callbacks.onAddChild,
        onEdit: callbacks.onEdit,
        onDelete: callbacks.onDelete,
        onOpenSession: callbacks.onOpenSession,
        onOpenDocuments: callbacks.onOpenDocuments,
      },
    })

    const children = childrenOf.get(node.id) ?? []
    if (children.length === 0) continue

    const totalChildWidth =
      children.length * NODE_WIDTH + (children.length - 1) * HORIZONTAL_GAP
    let childX = x + NODE_WIDTH / 2 - totalChildWidth / 2
    const childY = y + height + VERTICAL_GAP

    for (const child of children) {
      rfEdges.push({
        id: `e-${node.id}-${child.id}`,
        source: String(node.id),
        target: String(child.id),
        type: "smoothstep",
      })
      queue.push({ node: child, x: childX, y: childY })
      childX += NODE_WIDTH + HORIZONTAL_GAP
    }
  }

  return { nodes: rfNodes, edges: rfEdges }
}
