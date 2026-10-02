"use client"

import { useEffect } from "react"
import { useTaskTreeStore } from "./task-tree-store"

/**
 * Data wiring hook: fetches tree data on mount and subscribes to task://changed
 * events for real-time updates. Call this once in the main view component.
 */
export function useTaskTreeData() {
  const folderId = useTaskTreeStore((s) => s.folderId)
  const refetch = useTaskTreeStore((s) => s.refetch)

  useEffect(() => {
    if (folderId == null) return
    void refetch()
  }, [folderId, refetch])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    void (async () => {
      try {
        const { subscribe } = await import("@/lib/platform")
        const dispose = await subscribe<{ id?: number }>(
          "task://changed",
          () => {
            if (!disposed) void refetch()
          }
        )
        if (disposed) {
          dispose()
          return
        }
        unlisten = dispose
      } catch {
        // Non-Tauri environment — skip subscription.
      }
    })()

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [refetch])
}
