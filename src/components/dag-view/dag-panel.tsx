"use client"

import { useMemo, useState } from "react"
import { GitFork, ChevronDown, ChevronUp } from "lucide-react"
import { DagView } from "./dag-view"
import { useDagStore } from "./dag-model"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * DAG 浮层面板：检测到 topology（store 里出现图）才渲染。
 *
 * 挂在 `ConversationShell` —— 会话视图的公共外壳（conversation-detail-panel
 * 与 canvas conversation surface 都走它），是 tool_call_update 事件最自然的
 * 宿主：DAG 就是在当前会话的工具流里产生的。选它而不是消息流内的工具卡片，
 * 是因为一张 DAG 可能有几十个节点，卡片内嵌放不下，浮层既不挤占转录区也
 * 不依赖某个具体 tool_call 卡片存活。
 */
export function DagPanel() {
  const graphs = useDagStore((s) => s.graphs)
  const activeKey = useDagStore((s) => s.activeKey)
  const [collapsed, setCollapsed] = useState(false)

  const graph = useMemo(
    () => (activeKey ? (graphs.get(activeKey) ?? null) : null),
    [graphs, activeKey]
  )

  const runningCount = useMemo(() => {
    if (!graph) return 0
    let n = 0
    for (const node of graph.nodes.values()) {
      if (node.state === "running") n += 1
    }
    return n
  }, [graph])

  if (!graph || graph.nodes.size === 0) return null

  return (
    <div className="pointer-events-none absolute end-4 top-4 z-30 flex w-[min(34rem,calc(100%-2rem))] flex-col items-end gap-2">
      <div className="pointer-events-auto w-full overflow-hidden rounded-lg border bg-card/90 shadow-lg backdrop-blur supports-[backdrop-filter]:bg-card/70">
        <div className="flex items-center gap-2 border-b px-3 py-1.5">
          <GitFork className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="text-xs font-medium">Task DAG</span>
          <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] tabular-nums text-muted-foreground">
            {graph.nodes.size}
          </span>
          {runningCount > 0 && (
            <span className="rounded bg-blue-500/15 px-1.5 py-0.5 text-[10px] font-medium tabular-nums text-blue-600 dark:text-blue-400">
              {runningCount} running
            </span>
          )}
          <span className="ml-auto min-w-0 truncate text-[10px] text-muted-foreground">
            {graph.graphId}
          </span>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-6"
            aria-label={collapsed ? "Expand DAG" : "Collapse DAG"}
            onClick={() => setCollapsed((v) => !v)}
          >
            {collapsed ? (
              <ChevronDown className="size-3.5" />
            ) : (
              <ChevronUp className="size-3.5" />
            )}
          </Button>
        </div>
        <div
          className={cn(
            "overflow-hidden transition-[height] duration-200",
            collapsed ? "h-0" : "h-[min(22rem,50vh)]"
          )}
        >
          <DagView graph={graph} />
        </div>
      </div>
    </div>
  )
}
