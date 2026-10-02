"use client"

import { useCallback, useMemo, useState } from "react"
import {
  Background,
  BackgroundVariant,
  Controls,
  ReactFlow,
  type Edge,
  type EdgeProps,
  type NodeChange,
} from "@xyflow/react"
import "@xyflow/react/dist/style.css"
import "./dag-edges.css"

import { DagNode } from "./dag-node"
import { DagNodeDetail } from "./dag-node-detail"
import {
  buildDagFlowElements,
  type DagEdgeData,
  type DagFlowEdge,
  type DagFlowNode,
} from "./dag-edges"
import type { DagGraphState } from "./dag-model"
import { DAG_STATE_LABEL } from "./dag-state"
import { cn } from "@/lib/utils"

const nodeTypes = { dagNode: DagNode }

/**
 * 自定义边：颜色按【源节点】状态，`dag-edge--flow` 只在目标（入边接收方）
 * 是 running 时挂上 —— React Flow 自带的 `animated` 是全局开/关，无法只让
 * 入边动，所以这里用 CSS `stroke-dashoffset` keyframes 精确控制。
 */
function DagEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  data,
  markerEnd,
}: EdgeProps<DagFlowEdge>) {
  const path = useMemo(() => {
    // smoothstep 折线：分层 DAG 里比贝塞尔更易读（走正交通道）。
    const dx = Math.abs(targetX - sourceX)
    const dy = Math.abs(targetY - sourceY)
    const radius = Math.min(12, dx / 2, dy / 2)
    const midX = (sourceX + targetX) / 2
    const sign = targetX >= sourceX ? 1 : -1
    if (dy < 2) return `M ${sourceX} ${sourceY} L ${targetX} ${targetY}`
    return [
      `M ${sourceX} ${sourceY}`,
      `L ${midX - sign * radius} ${sourceY}`,
      `Q ${midX} ${sourceY} ${midX} ${sourceY + Math.sign(targetY - sourceY) * radius}`,
      `L ${midX} ${targetY - Math.sign(targetY - sourceY) * radius}`,
      `Q ${midX} ${targetY} ${midX + sign * radius} ${targetY}`,
      `L ${targetX} ${targetY}`,
    ].join(" ")
  }, [sourceX, sourceY, targetX, targetY])

  const edgeData: DagEdgeData | undefined = data
  const flow = edgeData?.flow === true
  const tone = edgeData?.tone ?? "unknown"

  return (
    <>
      <path
        id={id}
        d={path}
        fill="none"
        className={cn(
          "react-flow__edge-path dag-edge",
          `dag-edge--${tone}`,
          flow && "dag-edge--flow"
        )}
        markerEnd={markerEnd}
      />
      {/* 透明加宽的命中区：1.75px 的线太难点。 */}
      <path
        d={path}
        fill="none"
        stroke="transparent"
        strokeWidth={14}
        className="react-flow__edge-interaction"
      />
    </>
  )
}

const edgeTypes = { dagEdge: DagEdge }

interface DagViewProps {
  graph: DagGraphState
  className?: string
}

/**
 * 完整渲染一张任务 DAG：所有节点 + 所有依赖边，`fitView` 初始可见整图。
 * 传入的 graph 来自 `useDagStore`，节点/边映射是纯函数，随 graph 引用重算。
 *
 * **选中与详情**：点节点（或 Tab 聚焦后 Enter/Space）会在左侧拉出
 * `DagNodeDetail`。选中态由本组件自己持有，再作为 `data.selected` / `data.onSelect`
 * 注入节点 data —— 这样节点卡片不依赖 React Flow 的 selection 语义，
 * 关闭方式为：✕ 按钮或点空白画布。
 *
 * 为什么不用 `onNodeClick` 单独扛：React Flow 12 的 `NodeWrapper` 只在
 * `isSelectable`（`elementsSelectable`）为真时才派发 `onNodeClick`，
 * 而且节点包裹层的 `pointerEvents` 也依赖同一个开关。因此这里打开
 * `elementsSelectable`，并从 `onNodesChange` 的 `select` 变更里同步选中，
 * 同时用 `onNodeClick` 兜住鼠标路径（两者都是「置位」而非「取反」，幂等）。
 *
 * 同时关掉 `nodesFocusable`：卡片自己就是 `role="button"` + `tabIndex=0`，
 * 若再让包裹层可聚焦，每个节点会在 Tab 顺序里占两站、读屏也会念两遍。
 * 代价是失去 React Flow 的方向键巡游与 Escape 取消选中 —— 键盘可达性由
 * 卡片自身的 Enter/Space 保证，仍需方向键巡游时再开这个开关。
 */
export function DagView({ graph, className }: DagViewProps) {
  const elements = useMemo(() => buildDagFlowElements(graph), [graph])
  const [selectedId, setSelectedId] = useState<number | null>(null)

  const select = useCallback((nodeId: number) => setSelectedId(nodeId), [])

  const handleNodesChange = useCallback(
    (changes: NodeChange<DagFlowNode>[]) => {
      // 只关心选中变更；位置/尺寸等变更本视图一律不回写（节点由图层算位）。
      for (const change of changes) {
        if (change.type !== "select") continue
        setSelectedId((prev) => {
          if (change.selected) return Number(change.id)
          return prev !== null && String(prev) === change.id ? null : prev
        })
      }
    },
    []
  )

  const nodes = useMemo(
    () =>
      elements.nodes.map((node) => {
        const runtime = node.data.node
        return {
          ...node,
          // aria 标签带上状态文案：屏幕阅读器聚焦节点时不必只靠颜色判断。
          ariaLabel: `${runtime.toolName || `node ${runtime.id}`} — ${DAG_STATE_LABEL[runtime.state]}`,
          data: {
            ...node.data,
            selected: node.id === String(selectedId),
            onSelect: select,
          },
        }
      }),
    [elements, selectedId, select]
  )

  // 选中的节点可能因 topology 换代而消失 —— 派生时校验，消失即视为未选中。
  const selectedNode =
    selectedId === null ? null : (graph.nodes.get(selectedId) ?? null)

  if (elements.nodes.length === 0) return null

  return (
    <div className={cn("relative h-full w-full", className)}>
      <ReactFlow
        nodes={nodes}
        edges={elements.edges as Edge[]}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        fitView
        fitViewOptions={{ padding: 0.2 }}
        minZoom={0.1}
        maxZoom={2}
        nodesDraggable={false}
        nodesConnectable={false}
        nodesFocusable={false}
        edgesFocusable={false}
        onNodeClick={(_, node) => select(Number(node.id))}
        onNodesChange={handleNodesChange}
        onPaneClick={() => setSelectedId(null)}
        proOptions={{ hideAttribution: true }}
      >
        <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
        {/* 控件挪到右下：详情面板从左侧覆盖，会挡住默认的左下角。 */}
        <Controls showInteractive={false} position="bottom-right" />
      </ReactFlow>

      {selectedNode && (
        <DagNodeDetail
          graph={graph}
          nodeId={selectedNode.id}
          onSelect={select}
          onClose={() => setSelectedId(null)}
        />
      )}
    </div>
  )
}
