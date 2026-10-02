"use client"

import { useMemo, type ReactNode } from "react"
import { X } from "lucide-react"
import {
  DAG_OUTPUT_TAIL,
  selectDagNodeRelations,
  type DagGraphState,
  type DagNodeRuntime,
} from "./dag-model"
import {
  DAG_STATE_HINT,
  DAG_STATE_LABEL,
  DAG_STATE_TONE,
  DagStateIcon,
} from "./dag-state"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

interface DagNodeDetailProps {
  graph: DagGraphState
  nodeId: number
  /** 点依赖/下游芯片时切换选中节点。 */
  onSelect: (nodeId: number) => void
  onClose: () => void
}

/**
 * 节点详情面板：DAG 浮层内左侧的抽屉，展示单个节点的元信息、上下游关系与输出。
 *
 * 数据边界（重要）：详情**只来自前端已收到的三类 `ra.dag.*` 消息**。
 * `ra.dag.node_output.v1` 只携带 `delta: string`，且 store 为控制内存只保留
 * 尾部 `DAG_OUTPUT_TAIL` 个字符，因此这里显示的是「输出尾部片段」，不是完整
 * 日志；工具入参、退出码、耗时、stdout/stderr 之分在协议里都还不存在。
 * 面板底部有一行显式说明，避免把片段误读成全量。
 */
export function DagNodeDetail({
  graph,
  nodeId,
  onSelect,
  onClose,
}: DagNodeDetailProps) {
  // hooks 必须在早退之前 —— node 不存在时仍按规则调用。
  const relations = useMemo(
    () => selectDagNodeRelations(graph, nodeId),
    [graph, nodeId]
  )

  const node = graph.nodes.get(nodeId)
  if (!node) return null

  const state = node.state
  const label = DAG_STATE_LABEL[state]
  const title = node.toolName || `node ${node.id}`

  return (
    <aside
      aria-label={`Details for ${title}`}
      data-dag-node-detail={node.id}
      className={cn(
        "pointer-events-auto absolute inset-y-0 start-0 z-10 flex w-[min(16rem,65%)] flex-col",
        "overflow-y-auto overscroll-contain border-e bg-card/95 p-2.5 text-xs shadow-sm backdrop-blur",
        "supports-[backdrop-filter]:bg-card/85"
      )}
    >
      {/* ── 头部：状态图标 + 名称 + 关闭 ── */}
      <div className="flex items-start gap-1.5">
        <DagStateIcon state={state} className="mt-0.5" />
        <div className="min-w-0 flex-1">
          <div className="truncate text-xs font-medium leading-tight text-foreground">
            {title}
          </div>
          <div className="text-[10px] tabular-nums text-muted-foreground">
            #{node.id} · layer {node.layer}
          </div>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label="Close node details"
          onClick={onClose}
        >
          <X />
        </Button>
      </div>

      {/* ── 状态徽标：颜色之外还有图标 + 文案 + 语义说明 ── */}
      <div
        data-dag-node-detail-state={state}
        className={cn(
          "mt-2 flex items-center gap-1.5 rounded border px-2 py-1",
          DAG_STATE_TONE[state]
        )}
      >
        <DagStateIcon state={state} />
        <span className="font-medium text-foreground">{label}</span>
        <span className="min-w-0 flex-1 truncate opacity-80">
          {DAG_STATE_HINT[state]}
        </span>
      </div>

      <Section title="Dependencies">
        {relations.dependencies.length === 0 ? (
          <Empty>No dependencies</Empty>
        ) : (
          <ChipList nodes={relations.dependencies} onSelect={onSelect} />
        )}
      </Section>

      <Section title="Dependents">
        {relations.dependents.length === 0 ? (
          <Empty>Nothing depends on this node</Empty>
        ) : (
          <ChipList nodes={relations.dependents} onSelect={onSelect} />
        )}
      </Section>

      <Section title="Output">
        {node.output ? (
          <>
            <pre
              data-dag-node-output
              className="max-h-32 overflow-auto rounded border bg-muted/40 p-1.5 font-mono text-[10px] leading-snug break-words whitespace-pre-wrap"
            >
              {node.output}
            </pre>
            <Empty>
              Tail of the streamed output (last {DAG_OUTPUT_TAIL} characters).
            </Empty>
          </>
        ) : (
          <Empty>
            {state === "running"
              ? "Waiting for output…"
              : "No output captured for this node."}
          </Empty>
        )}
      </Section>

      <p className="mt-auto border-t pt-2 text-[10px] leading-snug text-muted-foreground italic">
        Built from the live <code>ra.dag.*</code> stream. Tool arguments, exit
        code, duration and the full log aren&apos;t carried by the protocol yet.
      </p>
    </aside>
  )
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="mt-2.5">
      <h3 className="mb-1 text-[10px] font-medium tracking-wide text-muted-foreground uppercase">
        {title}
      </h3>
      {children}
    </section>
  )
}

function Empty({ children }: { children: ReactNode }) {
  return (
    <p className="text-[10px] leading-snug text-muted-foreground">{children}</p>
  )
}

/** 关系芯片：同样带图标 + 名称 + 配色，点一下即跳转到该节点。 */
function ChipList({
  nodes,
  onSelect,
}: {
  nodes: DagNodeRuntime[]
  onSelect: (nodeId: number) => void
}) {
  return (
    <ul className="flex flex-col gap-1">
      {nodes.map((node) => (
        <li key={node.id}>
          <button
            type="button"
            data-dag-relation={node.id}
            onClick={() => onSelect(node.id)}
            className={cn(
              "focus-visible:ring-ring/50 flex w-full items-center gap-1.5 rounded border px-1.5 py-1 text-start transition-colors outline-none hover:brightness-95 focus-visible:ring-[3px]",
              DAG_STATE_TONE[node.state]
            )}
          >
            <DagStateIcon state={node.state} />
            <span className="min-w-0 flex-1 truncate font-medium text-foreground">
              {node.toolName || `node ${node.id}`}
            </span>
            <span className="shrink-0 tabular-nums text-muted-foreground">
              #{node.id}
            </span>
          </button>
        </li>
      ))}
    </ul>
  )
}
