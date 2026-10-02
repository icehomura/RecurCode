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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Trash2, Plus } from "lucide-react"
import type { TaskTreeNode, TaskTreeDocument } from "@/lib/types"
import { workTaskTreeAddDocument, workTaskTreeRemoveDocument } from "@/lib/api"
import { useTaskTreeStore } from "./task-tree-store"

interface Props {
  node: TaskTreeNode | null
  open: boolean
  onClose: () => void
}

export function TaskTreeDocumentPanel({ node, open, onClose }: Props) {
  const refetch = useTaskTreeStore((s) => s.refetch)
  const [adding, setAdding] = useState(false)
  const [newTitle, setNewTitle] = useState("")
  const [newContent, setNewContent] = useState("")
  const [newType, setNewType] = useState("plan")
  const [viewingDoc, setViewingDoc] = useState<TaskTreeDocument | null>(null)

  const handleAdd = async () => {
    if (!node || !newTitle.trim()) return
    await workTaskTreeAddDocument(node.id, newTitle.trim(), newContent, newType)
    setNewTitle("")
    setNewContent("")
    setAdding(false)
    await refetch()
  }

  const handleRemove = async (docId: number) => {
    await workTaskTreeRemoveDocument(docId)
    setViewingDoc(null)
    await refetch()
  }

  if (!node) return null

  return (
    <>
      <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
        <DialogContent className="max-w-lg max-h-[80vh] flex flex-col">
          <DialogHeader>
            <DialogTitle className="truncate">
              Documents — {node.title}
            </DialogTitle>
          </DialogHeader>

          <div className="flex-1 overflow-y-auto space-y-2 py-2">
            {node.documents.length === 0 && !adding && (
              <p className="text-sm text-muted-foreground text-center py-8">
                No documents yet. Add a PRD, plan, or note.
              </p>
            )}

            {node.documents.map((doc) => (
              <div
                key={doc.id}
                className="flex items-center gap-2 p-2 rounded-md border hover:bg-muted/50 cursor-pointer"
                onClick={() => setViewingDoc(doc)}
              >
                <span className="text-xs font-mono text-muted-foreground uppercase shrink-0">
                  {doc.document_type}
                </span>
                <span className="text-sm truncate flex-1">{doc.title}</span>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-6 shrink-0"
                  onClick={(e) => {
                    e.stopPropagation()
                    handleRemove(doc.id)
                  }}
                >
                  <Trash2 className="size-3" />
                </Button>
              </div>
            ))}

            {adding && (
              <div className="space-y-2 p-2 rounded-md border">
                <Input
                  value={newTitle}
                  onChange={(e) => setNewTitle(e.target.value)}
                  placeholder="Document title"
                />
                <Select value={newType} onValueChange={setNewType}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="plan">Plan</SelectItem>
                    <SelectItem value="prd">PRD</SelectItem>
                    <SelectItem value="note">Note</SelectItem>
                  </SelectContent>
                </Select>
                <Textarea
                  value={newContent}
                  onChange={(e) => setNewContent(e.target.value)}
                  placeholder="Document content (Markdown)..."
                  rows={5}
                />
                <div className="flex gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setAdding(false)}
                  >
                    Cancel
                  </Button>
                  <Button
                    size="sm"
                    onClick={handleAdd}
                    disabled={!newTitle.trim()}
                  >
                    Add
                  </Button>
                </div>
              </div>
            )}
          </div>

          <DialogFooter>
            {!adding && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => setAdding(true)}
              >
                <Plus className="size-3.5 mr-1" />
                Add document
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Document viewer */}
      {viewingDoc && (
        <Dialog
          open={!!viewingDoc}
          onOpenChange={(v) => !v && setViewingDoc(null)}
        >
          <DialogContent className="max-w-2xl max-h-[80vh] flex flex-col">
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <span className="text-xs font-mono text-muted-foreground uppercase">
                  {viewingDoc.document_type}
                </span>
                <span className="truncate">{viewingDoc.title}</span>
              </DialogTitle>
            </DialogHeader>
            <div className="flex-1 overflow-y-auto prose prose-sm dark:prose-invert max-w-none whitespace-pre-wrap text-sm">
              {viewingDoc.content || "(empty)"}
            </div>
          </DialogContent>
        </Dialog>
      )}
    </>
  )
}
