"use client"

import {
  Drawer,
  DrawerContent,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer"
import type { TaskTreeNode } from "@/lib/types"

interface Props {
  node: TaskTreeNode | null
  open: boolean
  onClose: () => void
}

/**
 * Session drawer: shows when a user clicks a task node to open its session.
 * Phase 1: displays the task info and a "Create Session" action.
 * Phase 2: will embed the actual conversation transcript viewer.
 */
export function TaskTreeSessionDrawer({ node, open, onClose }: Props) {
  if (!node) return null

  return (
    <Drawer open={open} onOpenChange={(v) => !v && onClose()}>
      <DrawerContent className="max-w-lg">
        <DrawerHeader>
          <DrawerTitle className="flex items-center gap-2">
            <span className="truncate">{node.title}</span>
            <span className="text-xs text-muted-foreground font-normal">
              {node.status}
            </span>
          </DrawerTitle>
        </DrawerHeader>

        <div className="px-4 pb-4 space-y-4">
          {node.tree_description && (
            <p className="text-sm text-muted-foreground">
              {node.tree_description}
            </p>
          )}

          {node.goals.length > 0 && (
            <div>
              <h4 className="text-sm font-medium mb-1">Goals</h4>
              <ul className="space-y-0.5">
                {node.goals.map((g) => (
                  <li key={g.id} className="text-sm text-muted-foreground">
                    {g.title}
                  </li>
                ))}
              </ul>
            </div>
          )}

          <div className="pt-2">
            {node.conversation_id ? (
              <p className="text-sm text-muted-foreground">
                Session #{node.conversation_id} — opening in workspace...
              </p>
            ) : (
              <p className="text-sm text-muted-foreground">
                No session yet. Create one from the task board to start working.
              </p>
            )}
          </div>
        </div>
      </DrawerContent>
    </Drawer>
  )
}
