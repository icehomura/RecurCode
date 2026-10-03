"use client"

import { memo } from "react"
import { type Node, type NodeProps } from "@xyflow/react"
import {
  FileText,
  Pencil,
  Play,
  CircleDot,
  CheckCircle2,
  XCircle,
  Clock,
} from "lucide-react"
import type { TaskTreeCardData } from "../task-tree-model"
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import { cn } from "@/lib/utils"

const STATUS_COLORS: Record<string, string> = {
  todo: "bg-slate-400",
  queued: "bg-slate-500",
  preparing: "bg-blue-400",
  running: "bg-blue-500",
  awaiting_input: "bg-amber-400",
  review: "bg-amber-500",
  merging: "bg-purple-500",
  done: "bg-green-500",
  failed: "bg-red-500",
  canceled: "bg-slate-300",
}

const STATUS_ICONS: Record<string, typeof CircleDot> = {
  running: Clock,
  awaiting_input: CircleDot,
  review: CircleDot,
  done: CheckCircle2,
  failed: XCircle,
}

function TaskTreeNodeCard({ data }: NodeProps<Node<TaskTreeCardData>>) {
  const {
    node,
    onSelect,
    onAddChild,
    onEdit,
    onDelete,
    onOpenSession,
    onOpenDocuments,
  } = data
  const statusColor = STATUS_COLORS[node.status] ?? "bg-slate-400"
  const StatusIcon = STATUS_ICONS[node.status]

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          className={cn(
            "w-[280px] rounded-lg border bg-card shadow-sm cursor-pointer",
            "hover:ring-2 hover:ring-primary/30 transition-shadow",
            node.parent_id == null && "border-primary/40"
          )}
          onClick={() => onSelect(node.id)}
        >
          {/* Header: title + action buttons */}
          <div className="flex items-start justify-between gap-1 px-3 pt-2.5">
            <div className="flex items-center gap-1.5 min-w-0">
              <span
                className={cn("size-2 rounded-full shrink-0", statusColor)}
              />
              <span className="text-sm font-medium truncate">{node.title}</span>
              {StatusIcon && (
                <StatusIcon className="size-3 text-muted-foreground shrink-0" />
              )}
            </div>
            <div className="flex items-center gap-0.5 shrink-0">
              <button
                className="p-0.5 rounded hover:bg-muted"
                title="Documents"
                onClick={(e) => {
                  e.stopPropagation()
                  onOpenDocuments(node.id)
                }}
              >
                <FileText className="size-3.5 text-muted-foreground" />
              </button>
              <button
                className="p-0.5 rounded hover:bg-muted"
                title="Edit"
                onClick={(e) => {
                  e.stopPropagation()
                  onEdit(node.id)
                }}
              >
                <Pencil className="size-3.5 text-muted-foreground" />
              </button>
            </div>
          </div>

          {/* Description */}
          {node.tree_description && (
            <p className="px-3 pt-1 text-xs text-muted-foreground line-clamp-2">
              {node.tree_description}
            </p>
          )}

          {/* Goals list */}
          {node.goals.length > 0 && (
            <div className="px-3 pb-2.5 pt-1.5 space-y-0.5">
              {node.goals.map((goal) => (
                <div
                  key={goal.id}
                  className="flex items-center gap-1.5 text-xs text-muted-foreground"
                >
                  <CircleDot className="size-2.5 shrink-0 text-primary/60" />
                  <span className="truncate">{goal.title}</span>
                </div>
              ))}
            </div>
          )}

          {/* Empty state */}
          {node.goals.length === 0 && !node.tree_description && (
            <div className="px-3 pb-2.5 pt-1">
              <span className="text-xs text-muted-foreground/50">
                No details
              </span>
            </div>
          )}
        </div>
      </ContextMenuTrigger>

      <ContextMenuContent className="w-48">
        <ContextMenuItem onClick={() => onAddChild(node.id)}>
          Add child node
        </ContextMenuItem>
        <ContextMenuItem onClick={() => onEdit(node.id)}>Edit</ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          onClick={() => onOpenSession(node.id)}
          className="gap-2"
        >
          <Play className="size-3.5" />
          Open session
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          onClick={() => onDelete(node.id)}
          className="text-destructive focus:text-destructive"
        >
          Delete
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}

export const TaskTreeCardNode = memo(TaskTreeNodeCard)
