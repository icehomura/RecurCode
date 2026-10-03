"use client"

import { memo, useCallback } from "react"
import { Handle, Position, type Node, type NodeProps } from "@xyflow/react"
import { ChevronRight, ChevronDown } from "lucide-react"
import type { ProjectNodeData } from "../project-view-model"
import { getIconForKind, getColorForKind } from "../project-view-model"
import { cn } from "@/lib/utils"

function ProjectNodeInner({ data }: NodeProps<Node<ProjectNodeData>>) {
  const { node, hasChildren, isExpanded, onToggle, onSelect } = data
  const Icon = getIconForKind(node.kind)
  const colorClass = getColorForKind(node.kind)
  const isFolder = node.kind === "folder"

  const handleClick = useCallback(() => {
    if (isFolder && hasChildren) {
      onToggle(node.id)
    }
    onSelect(node.id)
  }, [isFolder, hasChildren, node.id, onToggle, onSelect])

  return (
    <>
      {node.parent_id != null && (
        <Handle
          type="target"
          position={Position.Top}
          className="!bg-muted-foreground/30"
        />
      )}
      <div
        className={cn(
          "w-[240px] rounded-lg border bg-card shadow-sm cursor-pointer transition-all",
          "hover:ring-2 hover:ring-primary/30 hover:shadow-md",
          isFolder && "border-amber-500/30",
          !isFolder && "border-border",
        )}
        onClick={handleClick}
      >
        <div className="flex items-center gap-2 px-3 py-2.5">
          {/* Expand/collapse chevron for folders */}
          {isFolder && hasChildren ? (
            <button
              className="shrink-0 p-0.5 -ml-1 rounded hover:bg-muted"
              onClick={(e) => {
                e.stopPropagation()
                onToggle(node.id)
              }}
            >
              {isExpanded ? (
                <ChevronDown className="size-3.5 text-muted-foreground" />
              ) : (
                <ChevronRight className="size-3.5 text-muted-foreground" />
              )}
            </button>
          ) : (
            <div className="shrink-0 w-[18px]" />
          )}

          {/* Icon */}
          <Icon className={cn("size-4 shrink-0", colorClass)} />

          {/* Name + summary */}
          <div className="min-w-0 flex-1">
            <div className="text-sm font-medium truncate">{node.name}</div>
            {node.summary && (
              <div className="text-2xs text-muted-foreground truncate mt-0.5">
                {node.summary}
              </div>
            )}
          </div>

          {/* Language badge */}
          {node.language && (
            <span className="text-2xs text-muted-foreground bg-muted px-1.5 py-0.5 rounded shrink-0">
              {node.language}
            </span>
          )}
        </div>
      </div>
      <Handle
        type="source"
        position={Position.Bottom}
        className="!bg-muted-foreground/30"
      />
    </>
  )
}

export const ProjectNode = memo(ProjectNodeInner)
