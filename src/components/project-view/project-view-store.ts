"use client"

import { create } from "zustand"
import type { ProjectStructureNode } from "@/lib/types"
import { projectStructureList } from "@/lib/api"

interface ProjectViewState {
  folderId: number | null
  nodes: ProjectStructureNode[]
  loading: boolean
  selectedNodeId: number | null
  expandedFolderIds: Set<number>

  setFolderId: (id: number) => void
  refetch: () => Promise<void>
  selectNode: (id: number | null) => void
  toggleFolder: (id: number) => void
  expandAll: () => void
  collapseAll: () => void
}

export const useProjectViewStore = create<ProjectViewState>((set, get) => ({
  folderId: null,
  nodes: [],
  loading: false,
  selectedNodeId: null,
  expandedFolderIds: new Set(),

  setFolderId: (id) => {
    set({ folderId: id, nodes: [], loading: true, selectedNodeId: null })
    void get().refetch()
  },

  refetch: async () => {
    const { folderId } = get()
    if (folderId == null) return
    set({ loading: true })
    try {
      const nodes = await projectStructureList(folderId)
      // Auto-expand first 2 levels
      const expanded = new Set<number>()
      for (const n of nodes) {
        if (n.kind === "folder" && n.parent_id != null) {
          const depth = calcDepth(nodes, n)
          if (depth <= 2) expanded.add(n.id)
        }
      }
      // Always expand root folders
      for (const n of nodes) {
        if (n.kind === "folder" && n.parent_id == null) {
          expanded.add(n.id)
        }
      }
      set({ nodes, loading: false, expandedFolderIds: expanded })
    } catch {
      set({ loading: false })
    }
  },

  selectNode: (id) => set({ selectedNodeId: id }),

  toggleFolder: (id) =>
    set((s) => {
      const next = new Set(s.expandedFolderIds)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return { expandedFolderIds: next }
    }),

  expandAll: () =>
    set((s) => {
      const next = new Set<number>()
      for (const n of s.nodes) {
        if (n.kind === "folder") next.add(n.id)
      }
      return { expandedFolderIds: next }
    }),

  collapseAll: () => set({ expandedFolderIds: new Set() }),
}))

function calcDepth(
  nodes: ProjectStructureNode[],
  target: ProjectStructureNode,
): number {
  let depth = 0
  let current = target
  const byId = new Map(nodes.map((n) => [n.id, n]))
  while (current.parent_id != null) {
    depth++
    const parent = byId.get(current.parent_id)
    if (!parent) break
    current = parent
  }
  return depth
}
