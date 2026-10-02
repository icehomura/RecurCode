"use client"

import { useCallback, useEffect, useState } from "react"
import {
  X,
  Pencil,
  Save,
  Loader2,
  FileText,
  Code2,
  Box,
  Layers,
  Cog,
  BookOpen,
} from "lucide-react"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import type { ProjectStructureNode, ProjectCallEdge } from "@/lib/types"
import { projectStructureUpdateNode } from "@/lib/api"
import { cn } from "@/lib/utils"

const KIND_ICONS: Record<string, typeof FileText> = {
  folder: Layers,
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

interface ProjectViewDetailPanelProps {
  node: ProjectStructureNode | null
  open: boolean
  onClose: () => void
}

export function ProjectViewDetailPanel({
  node,
  open,
  onClose,
}: ProjectViewDetailPanelProps) {
  const [editing, setEditing] = useState(false)
  const [draftName, setDraftName] = useState("")
  const [draftSummary, setDraftSummary] = useState("")
  const [draftKind, setDraftKind] = useState("")
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (node) {
      setDraftName(node.name)
      setDraftSummary(node.summary ?? "")
      setDraftKind(node.kind)
      setEditing(false)
    }
  }, [node])

  const handleSave = useCallback(async () => {
    if (!node) return
    setSaving(true)
    try {
      await projectStructureUpdateNode(node.id, {
        name: draftName || null,
        summary: draftSummary || null,
        kind: draftKind || null,
      })
      toast.success("Node updated")
      setEditing(false)
    } catch (err) {
      toast.error("Update failed", {
        description: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setSaving(false)
    }
  }, [node, draftName, draftSummary, draftKind])

  if (!open || !node) return null

  const Icon = KIND_ICONS[node.kind] ?? FileText
  const colorClass = KIND_COLORS[node.kind] ?? "text-muted-foreground"

  // Parse call relations
  let callEdges: ProjectCallEdge[] = []
  if (node.call_relations) {
    try {
      callEdges = JSON.parse(node.call_relations) as ProjectCallEdge[]
    } catch {
      // ignore
    }
  }

  // Parse metadata
  let metadata: Record<string, string> = {}
  if (node.metadata) {
    try {
      metadata = JSON.parse(node.metadata) as Record<string, string>
    } catch {
      // ignore
    }
  }

  const outgoingEdges = callEdges.filter((e) => e.from_path.includes(node.path))
  const incomingEdges = callEdges.filter((e) => e.to_path.includes(node.path))

  return (
    <div className="absolute right-0 top-0 h-full w-[360px] border-l bg-card shadow-lg z-10 flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between gap-2 px-4 py-3 border-b shrink-0">
        <div className="flex items-center gap-2 min-w-0">
          <Icon className={cn("size-4 shrink-0", colorClass)} />
          <span className="text-sm font-semibold truncate">{node.name}</span>
        </div>
        <div className="flex items-center gap-1 shrink-0">
          {!editing && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2"
              onClick={() => setEditing(true)}
            >
              <Pencil className="size-3.5" />
            </Button>
          )}
          <Button
            variant="ghost"
            size="sm"
            className="h-7 px-2"
            onClick={onClose}
          >
            <X className="size-3.5" />
          </Button>
        </div>
      </div>

      {/* Content */}
      <ScrollArea className="flex-1 min-h-0">
        <div className="p-4 space-y-4">
          {/* Kind & Language */}
          <div className="flex flex-wrap gap-2">
            <Badge variant="secondary" className="text-xs">
              {node.kind}
            </Badge>
            {node.language && (
              <Badge variant="outline" className="text-xs">
                {node.language}
              </Badge>
            )}
          </div>

          {/* Path */}
          <div className="space-y-1">
            <label className="text-xs font-medium text-muted-foreground">
              Path
            </label>
            <div className="text-xs text-muted-foreground font-mono break-all">
              {node.path}
            </div>
          </div>

          {/* Editable fields */}
          {editing ? (
            <>
              <div className="space-y-1.5">
                <label className="text-xs font-medium">Name</label>
                <Input
                  value={draftName}
                  onChange={(e) => setDraftName(e.target.value)}
                  className="h-8 text-sm"
                />
              </div>
              <div className="space-y-1.5">
                <label className="text-xs font-medium">Kind</label>
                <Input
                  value={draftKind}
                  onChange={(e) => setDraftKind(e.target.value)}
                  className="h-8 text-sm"
                />
              </div>
              <div className="space-y-1.5">
                <label className="text-xs font-medium">Summary</label>
                <Textarea
                  value={draftSummary}
                  onChange={(e) => setDraftSummary(e.target.value)}
                  className="min-h-[60px] text-sm"
                />
              </div>
              <div className="flex gap-2">
                <Button
                  size="sm"
                  onClick={() => {
                    void handleSave()
                  }}
                  disabled={saving}
                >
                  {saving ? (
                    <Loader2 className="size-3.5 mr-1 animate-spin" />
                  ) : (
                    <Save className="size-3.5 mr-1" />
                  )}
                  Save
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => {
                    setEditing(false)
                    setDraftName(node.name)
                    setDraftSummary(node.summary ?? "")
                    setDraftKind(node.kind)
                  }}
                >
                  Cancel
                </Button>
              </div>
            </>
          ) : (
            <>
              {/* Summary */}
              {node.summary && (
                <div className="space-y-1">
                  <label className="text-xs font-medium text-muted-foreground">
                    Summary
                  </label>
                  <p className="text-sm">{node.summary}</p>
                </div>
              )}

              {/* Outgoing calls */}
              {outgoingEdges.length > 0 && (
                <div className="space-y-1.5">
                  <label className="text-xs font-medium text-muted-foreground">
                    Calls ({outgoingEdges.length})
                  </label>
                  <div className="space-y-1">
                    {outgoingEdges.map((edge, i) => (
                      <div
                        key={i}
                        className="flex items-center gap-2 text-xs text-muted-foreground"
                      >
                        <Badge
                          variant="outline"
                          className={cn(
                            "text-2xs px-1 py-0",
                            edge.kind === "calls" &&
                              "border-orange-400 text-orange-500",
                            edge.kind === "imports" &&
                              "border-blue-400 text-blue-500",
                            edge.kind === "extends" &&
                              "border-green-400 text-green-500",
                            edge.kind === "implements" &&
                              "border-purple-400 text-purple-500"
                          )}
                        >
                          {edge.kind}
                        </Badge>
                        <span className="truncate font-mono">
                          {edge.to_path.split("::").pop() ?? edge.to_path}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Incoming calls */}
              {incomingEdges.length > 0 && (
                <div className="space-y-1.5">
                  <label className="text-xs font-medium text-muted-foreground">
                    Called by ({incomingEdges.length})
                  </label>
                  <div className="space-y-1">
                    {incomingEdges.map((edge, i) => (
                      <div
                        key={i}
                        className="flex items-center gap-2 text-xs text-muted-foreground"
                      >
                        <Badge variant="outline" className="text-2xs px-1 py-0">
                          {edge.kind}
                        </Badge>
                        <span className="truncate font-mono">
                          {edge.from_path.split("::").pop() ?? edge.from_path}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Metadata */}
              {Object.keys(metadata).length > 0 && (
                <div className="space-y-1.5">
                  <label className="text-xs font-medium text-muted-foreground">
                    Metadata
                  </label>
                  <div className="space-y-0.5">
                    {Object.entries(metadata).map(([key, value]) => (
                      <div
                        key={key}
                        className="flex items-center gap-2 text-xs"
                      >
                        <span className="text-muted-foreground font-mono">
                          {key}:
                        </span>
                        <span>{value}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}
