"use client"

import {
  Check,
  CircleDashed,
  Loader2,
  MinusCircle,
  Slash,
  X,
  type LucideIcon,
} from "lucide-react"
import type { RaDagNodeState } from "./dag-model"
import { cn } from "@/lib/utils"

/**
 * DAG 节点状态的视觉令牌**单一来源**：文案 / 语义说明 / 配色 / 图标。
 *
 * 无障碍约定：状态**不能只靠颜色**区分。每个状态必须同时具备
 *   1. 图标（`DAG_STATE_ICON`）——形状差异，色觉障碍下仍可辨认；
 *   2. 文案（`DAG_STATE_LABEL`）——给屏幕阅读器、放大/低对比场景兜底；
 *   3. 语义说明（`DAG_STATE_HINT`）——配色在详情面板里的等价表述。
 * 节点卡片、详情面板、关系芯片全部从这里取令牌；新增状态若漏配任一项，
 * `dag-model.test.ts` 里的完整性断言会直接失败。
 */

export const DAG_STATE_LABEL: Record<RaDagNodeState, string> = {
  pending: "Pending",
  running: "Running",
  succeeded: "Succeeded",
  failed: "Failed",
  skipped: "Skipped",
  cancelled: "Cancelled",
}

/** 颜色之外的第二条信息通道：解释状态含义，供详情面板展示。 */
export const DAG_STATE_HINT: Record<RaDagNodeState, string> = {
  pending: "Waiting for dependencies",
  running: "Executing",
  succeeded: "Completed",
  failed: "Failed",
  skipped: "Skipped (dependency failed or branch not taken)",
  cancelled: "Cancelled",
}

/** 状态配色（Tailwind 类）。节点卡片与详情面板徽标共用同一套 tone。 */
export const DAG_STATE_TONE: Record<RaDagNodeState, string> = {
  pending: "border-border bg-muted/40 text-muted-foreground",
  running: "border-blue-500/70 bg-blue-500/10 text-blue-600 dark:text-blue-400",
  succeeded:
    "border-emerald-500/70 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
  failed: "border-red-500/70 bg-red-500/10 text-red-600 dark:text-red-400",
  skipped: "border-border bg-muted/30 text-muted-foreground",
  cancelled: "border-border bg-muted/30 text-muted-foreground",
}

/**
 * 状态图标。同一状态在节点、徽标、关系芯片里始终是同一个形状 —— 图标本身
 * 就是一条独立于颜色的信息通道。
 */
export const DAG_STATE_ICON: Record<RaDagNodeState, LucideIcon> = {
  pending: CircleDashed,
  running: Loader2,
  succeeded: Check,
  failed: X,
  skipped: MinusCircle,
  cancelled: Slash,
}

interface DagStateIconProps {
  state: RaDagNodeState
  className?: string
}

/**
 * 状态图标。装饰性元素（`aria-hidden`）：状态语义由相邻文案承担，
 * 屏幕阅读器不必重复朗读图标。
 */
export function DagStateIcon({ state, className }: DagStateIconProps) {
  const Icon = DAG_STATE_ICON[state]
  return (
    <Icon
      aria-hidden="true"
      className={cn(
        "size-3.5 shrink-0",
        state === "running" && "animate-spin",
        className
      )}
      // succeeded/failed 的勾叉画细了看不清，单独加粗。
      strokeWidth={state === "succeeded" || state === "failed" ? 3 : undefined}
    />
  )
}
