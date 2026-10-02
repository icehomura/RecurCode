"use client"

import { useState } from "react"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Trash2, Plus } from "lucide-react"
import type { TaskTreeNode } from "@/lib/types"
import {
  workTaskTreeUpdate,
  workTaskTreeAddGoal,
  workTaskTreeRemoveGoal,
} from "@/lib/api"
import { useTaskTreeStore } from "./task-tree-store"

interface Props {
  node: TaskTreeNode | null
  open: boolean
  onClose: () => void
}

export function TaskTreeEditDialog({ node, open, onClose }: Props) {
  const refetch = useTaskTreeStore((s) => s.refetch)
  const [title, setTitle] = useState(node?.title ?? "")
  const [description, setDescription] = useState(node?.tree_description ?? "")
  const [newGoal, setNewGoal] = useState("")
  const [saving, setSaving] = useState(false)

  // Sync state when node changes.
  if (node && open) {
    if (title !== node.title) setTitle(node.title)
    if (description !== (node.tree_description ?? ""))
      setDescription(node.tree_description ?? "")
  }

  const handleSave = async () => {
    if (!node) return
    setSaving(true)
    try {
      await workTaskTreeUpdate(node.id, title || null, description || null)
      await refetch()
      onClose()
    } finally {
      setSaving(false)
    }
  }

  const handleAddGoal = async () => {
    if (!node || !newGoal.trim()) return
    await workTaskTreeAddGoal(node.id, newGoal.trim())
    setNewGoal("")
    await refetch()
  }

  const handleRemoveGoal = async (goalId: number) => {
    await workTaskTreeRemoveGoal(goalId)
    await refetch()
  }

  if (!node) return null

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Edit task node</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <label className="text-sm font-medium">Title</label>
            <Input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Task title (optional)"
            />
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium">Description</label>
            <Textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Task description (optional — leave blank for AI to generate)"
              rows={3}
            />
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium">Goals</label>
            <div className="space-y-1">
              {node.goals.map((goal) => (
                <div key={goal.id} className="flex items-center gap-2">
                  <span className="text-sm flex-1 truncate">{goal.title}</span>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-6 shrink-0"
                    onClick={() => handleRemoveGoal(goal.id)}
                  >
                    <Trash2 className="size-3" />
                  </Button>
                </div>
              ))}
            </div>
            <div className="flex gap-2">
              <Input
                value={newGoal}
                onChange={(e) => setNewGoal(e.target.value)}
                placeholder="New goal..."
                onKeyDown={(e) => e.key === "Enter" && handleAddGoal()}
              />
              <Button
                variant="outline"
                size="icon"
                className="shrink-0"
                onClick={handleAddGoal}
                disabled={!newGoal.trim()}
              >
                <Plus className="size-4" />
              </Button>
            </div>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={saving}>
            {saving ? "Saving..." : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
