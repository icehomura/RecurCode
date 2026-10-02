/**
 * DAG → React Flow 的纯映射：dependsOn 反转成边，按 `layers` 分层算坐标。
 *
 * 布局是后端算好的分层（LLMCompiler 式），前端只做「layer → y、同层均分 x」
 * 的几十行坐标计算，不引入任何布局库。
 */
import type { Edge, Node } from "@xyflow/react"
import type { DagGraphState, DagNodeRuntime, RaDagNodeState } from "./dag-model"

export const DAG_NODE_WIDTH = 180
export const DAG_NODE_HEIGHT = 60
export const DAG_H_GAP = 40
export const DAG_V_GAP = 40

/** 节点 data 载荷：渲染组件只读这些字段。 */
export interface DagNodeData extends Record<string, unknown> {
  node: DagNodeRuntime
  /**
   * 该节点是否为当前选中（详情面板已打开）。**视图层注入，非协议字段** ——
   * 见 `dag-node.tsx` 顶部关于「为何不用 React Flow selection」的说明。
   */
  selected?: boolean
  /** 点击 / 键盘激活回调。同样由视图层注入。 */
  onSelect?: (nodeId: number) => void
}

/** 边 data 载荷：由源/目标节点状态推导出颜色与是否流动。 */
export interface DagEdgeData extends Record<string, unknown> {
  /** 目标节点（入边的接收方）正在运行 → 该边播放流动动画。 */
  flow: boolean
  /** 边颜色，按【源节点】状态取。 */
  tone: RaDagNodeState | "unknown"
}

export type DagFlowNode = Node<DagNodeData>
export type DagFlowEdge = Edge<DagEdgeData>

/**
 * `dependsOn:[a, b]` on node `n` → 边 `a→n`、`b→n`（依赖指向被依赖者）。
 * 源/目标任一不在图内时跳过该边（topology 尚未覆盖的悬空引用）。
 */
export function buildDagEdges(graph: DagGraphState): DagFlowEdge[] {
  const edges: DagFlowEdge[] = []
  const seen = new Set<string>()

  for (const node of graph.nodes.values()) {
    for (const dep of node.dependsOn) {
      if (dep === node.id) continue
      if (!graph.nodes.has(dep) || !graph.nodes.has(node.id)) continue
      const id = `e-${dep}-${node.id}`
      if (seen.has(id)) continue
      seen.add(id)

      const source = graph.nodes.get(dep)
      const flow = node.state === "running"
      edges.push({
        id,
        source: String(dep),
        target: String(node.id),
        type: "smoothstep",
        // 边不参与选中：本视图用节点选中驱动详情面板，能选中的边只会带来
        // 无意义的选中高亮与多余 tab 停靠点。
        selectable: false,
        data: {
          flow,
          tone: source ? source.state : "unknown",
        },
      })
    }
  }
  return edges
}

/**
 * layer → y（每层一行），同层节点均分 x 并整体水平居中。
 *
 * `graph.topology.layers` 优先（后端已排好顺序）；缺失时退回按节点
 * 自己的 `layer` 字段分组，保证 topology 不完整也能画。
 */
export function computeLayerPositions(
  graph: DagGraphState
): Map<number, { x: number; y: number }> {
  const rows: number[][] = []
  const placed = new Set<number>()

  const layers = graph.topology?.layers ?? []
  for (const row of layers) {
    const ids = row.filter((id) => graph.nodes.has(id))
    if (ids.length === 0) continue
    rows.push(ids)
    for (const id of ids) placed.add(id)
  }

  if (placed.size < graph.nodes.size) {
    const byLayer = new Map<number, number[]>()
    for (const id of Array.from(graph.nodes.keys()).sort((a, b) => a - b)) {
      if (placed.has(id)) continue
      const layer = graph.nodes.get(id)?.layer ?? 0
      const bucket = byLayer.get(layer)
      if (bucket) bucket.push(id)
      else byLayer.set(layer, [id])
    }
    for (const layer of Array.from(byLayer.keys()).sort((a, b) => a - b)) {
      rows.push(byLayer.get(layer)!)
    }
  }

  const positions = new Map<number, { x: number; y: number }>()
  rows.forEach((ids, rowIndex) => {
    const y = rowIndex * (DAG_NODE_HEIGHT + DAG_V_GAP)
    const totalWidth =
      ids.length * DAG_NODE_WIDTH + (ids.length - 1) * DAG_H_GAP
    let x = -totalWidth / 2
    for (const id of ids) {
      positions.set(id, { x, y })
      x += DAG_NODE_WIDTH + DAG_H_GAP
    }
  })
  return positions
}

/** 整图映射：nodes（带坐标 + data）+ edges（带 flow/tone data）。 */
export function buildDagFlowElements(graph: DagGraphState): {
  nodes: DagFlowNode[]
  edges: DagFlowEdge[]
} {
  const positions = computeLayerPositions(graph)
  const nodes: DagFlowNode[] = []
  for (const node of graph.nodes.values()) {
    const pos = positions.get(node.id) ?? { x: 0, y: 0 }
    nodes.push({
      id: String(node.id),
      type: "dagNode",
      position: pos,
      data: { node },
    })
  }
  return { nodes, edges: buildDagEdges(graph) }
}
