"use client"

import { create } from "zustand"
import type { TaskTreeNode } from "@/lib/types"
import { workTaskTreeList } from "@/lib/api"

interface TaskTreeState {
  folderId: number | null
  nodes: TaskTreeNode[]
  loading: boolean
  selectedNodeId: number | null
  editingNodeId: number | null
  sessionDrawerTaskId: number | null
  documentPanelTaskId: number | null

  setFolderId: (id: number) => void
  refetch: () => Promise<void>
  selectNode: (id: number | null) => void
  openEditor: (id: number | null) => void
  openSessionDrawer: (taskId: number) => void
  closeSessionDrawer: () => void
  openDocumentPanel: (taskId: number) => void
  closeDocumentPanel: () => void
}

export const useTaskTreeStore = create<TaskTreeState>((set, get) => ({
  folderId: null,
  nodes: [],
  loading: false,
  selectedNodeId: null,
  editingNodeId: null,
  sessionDrawerTaskId: null,
  documentPanelTaskId: null,

  setFolderId: (id) => {
    set({ folderId: id, nodes: [], loading: true, selectedNodeId: null })
    void get().refetch()
  },

  refetch: async () => {
    const { folderId } = get()
    if (folderId == null) return
    set({ loading: true })
    try {
      const nodes = await workTaskTreeList(folderId)
      set({ nodes, loading: false })
    } catch {
      set({ loading: false })
    }
  },

  selectNode: (id) => set({ selectedNodeId: id }),
  openEditor: (id) => set({ editingNodeId: id }),
  openSessionDrawer: (taskId) => set({ sessionDrawerTaskId: taskId }),
  closeSessionDrawer: () => set({ sessionDrawerTaskId: null }),
  openDocumentPanel: (taskId) => set({ documentPanelTaskId: taskId }),
  closeDocumentPanel: () => set({ documentPanelTaskId: null }),
}))
