"use client"

import { memo, type KeyboardEvent } from "react"
import type { NodeProps } from "@xyflow/react"
import { DAG_STATE_LABEL, DAG_STATE_TONE, DagStateIcon } from "./dag-state"
import type { DagFlowNode } from "./dag-edges"
import { cn } from "@/lib/utils"

/**
 * DAG 节点卡片。
 *
 * 状态表达走 `dag-state.tsx` 的共享令牌：**图标 + 文案 + 配色**三通道，
 * 不允许只靠颜色区分（色觉障碍下仍可读）。running 额外挂
 * `dag-node--running` 的 CSS 脉冲光晕，cancelled 挂斜纹底（见 dag-edges.css）。
 *
 * 选中/点击：本组件**不依赖 React Flow 的 selection**。React Flow 只在
 * `elementsSelectable` 为真时才会派发 `onNodeClick`（见 NodeWrapper 的
 * `onSelectNodeHandler`），而本视图刻意关掉了元素选中，所以选中态与激活回调
 * 都由视图层通过 `data.selected` / `data.onSelect` 注入，键盘可达性也由这里
 * 自己保证（`role="button"` + Enter/Space）。
 */
function DagNodeCard({ data }: NodeProps<DagFlowNode>) {
  const { node, selected = false, onSelect } = data
  const state = node.state
  const label = DAG_STATE_LABEL[state]
  const title = node.toolName || `node ${node.id}`

  const activate = () => onSelect?.(node.id)

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // 空格默认会滚动画布，必须拦截。
    if (event.key !== "Enter" && event.key !== " ") return
    event.preventDefault()
    activate()
  }

  return (
    <div
      data-dag-node-state={state}
      data-dag-node-selected={selected || undefined}
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      aria-label={`${title} — ${label}`}
      onClick={activate}
      onKeyDown={handleKeyDown}
      className={cn(
        "dag-node flex h-[60px] w-[180px] flex-col justify-center gap-0.5 rounded-md border px-2.5 py-1.5 shadow-sm",
        "cursor-pointer transition-colors duration-200",
        "focus-visible:ring-ring/50 outline-none focus-visible:ring-[3px]",
        selectedRing(selected),
        DAG_STATE_TONE[state],
        state === "running" && "dag-node--running",
        state === "cancelled" && "dag-node--cancelled"
      )}
      title={node.output || undefined}
    >
      <div className="flex min-w-0 items-center gap-1.5">
        <DagStateIcon state={state} />
        <span className="truncate text-xs font-medium leading-tight text-foreground">
          {title}
        </span>
        <span className="ml-auto shrink-0 rounded bg-background/60 px-1 text-[10px] font-medium tabular-nums text-muted-foreground">
          #{node.id}
        </span>
      </div>
      <div className="flex min-w-0 items-center gap-1.5">
        <span className="truncate text-[10px] leading-tight opacity-80">
          {label}
        </span>
        {node.output && state === "running" && (
          <span className="min-w-0 flex-1 truncate text-[10px] leading-tight opacity-60">
            {node.output}
          </span>
        )}
      </div>
    </div>
  )
}

/** 选中态描边。抽成函数是为了让「选中」与「状态色」两层样式互不干扰。 */
function selectedRing(selected: boolean): string {
  return selected ? "ring-primary ring-2 ring-offset-1 ring-offset-card" : ""
}

export const DagNode = memo(DagNodeCard)
