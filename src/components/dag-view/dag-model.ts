/**
 * RecurCode DAG 数据层：`ra.dev/dag` 协议消息的类型、seq 水位去重，以及按
 * `toolCallId + graphId` 索引的 store reducer。
 *
 * 协议三条消息（`meta["ra.dev/dag"]` 的 value）：
 *   ra.dag.topology.v1    整图拓扑（nodes / layers / limits）
 *   ra.dag.node_state.v1  单节点状态迁移
 *   ra.dag.node_output.v1 单节点输出 delta
 *
 * 顺序保证：同一个 graph 内 `seq` 单调递增，因此「同 graph 同消息类型
 * seq <= 已处理水位」直接丢弃（乱序 / 重复到达都不会让状态回退）。
 * topology 在线协议样例里不带 `seq`，此时无条件接受且不推进水位。
 *
 * 纯函数 `applyDagMessage` 与 zustand 壳 `ingestDagMessage` 分开，前者可单测。
 */
import { create } from "zustand"

// ── meta key 与 schema 常量 ──

/** ACP `tool_call(_update)._meta` 上承载 DAG 消息的命名空间 key。 */
export const RA_DAG_META_KEY = "ra.dev/dag"

export const RA_DAG_TOPOLOGY_SCHEMA = "ra.dag.topology.v1"
export const RA_DAG_NODE_STATE_SCHEMA = "ra.dag.node_state.v1"
export const RA_DAG_NODE_OUTPUT_SCHEMA = "ra.dag.node_output.v1"

/** 只接受 `ra.dag.` 前缀的 schema —— 其它命名空间一律拒绝。 */
export const RA_DAG_SCHEMA_PREFIX = "ra.dag."

export type RaDagNodeState =
  | "pending"
  | "running"
  | "succeeded"
  | "failed"
  | "skipped"
  | "cancelled"

export const RA_DAG_NODE_STATES: readonly RaDagNodeState[] = [
  "pending",
  "running",
  "succeeded",
  "failed",
  "skipped",
  "cancelled",
]

// ── 协议消息 ──

export interface RaDagTopologyNode {
  id: number
  toolName: string
  dependsOn: number[]
  layer: number
}

export interface RaDagLimits {
  nodes?: number
  layerWidth?: number
}

export interface RaDagTopologyMessage {
  schema: typeof RA_DAG_TOPOLOGY_SCHEMA
  graphId: string
  nodes: RaDagTopologyNode[]
  layers: number[][]
  maxDepth?: number
  limits?: RaDagLimits
  /** 在线样例不带 seq；后端一旦补上就走同一套水位去重。 */
  seq?: number
}

export interface RaDagNodeStateMessage {
  schema: typeof RA_DAG_NODE_STATE_SCHEMA
  graphId: string
  nodeId: number
  state: RaDagNodeState
  seq: number
  layer?: number
}

export interface RaDagNodeOutputMessage {
  schema: typeof RA_DAG_NODE_OUTPUT_SCHEMA
  graphId: string
  nodeId: number
  delta: string
  seq: number
}

export type RaDagMessage =
  | RaDagTopologyMessage
  | RaDagNodeStateMessage
  | RaDagNodeOutputMessage

// ── 归一化后的运行时状态 ──

/** 单节点运行时投影：拓扑字段来自 topology，状态/输出来自后续增量消息。 */
export interface DagNodeRuntime {
  id: number
  toolName: string
  dependsOn: number[]
  layer: number
  state: RaDagNodeState
  /** 最近一段输出（高频 delta 已在入口节流合并，这里只留尾部片段）。 */
  output: string
}

export interface DagGraphState {
  toolCallId: string
  graphId: string
  topology: RaDagTopologyMessage | null
  nodes: Map<number, DagNodeRuntime>
  /**
   * 每类消息各自的已处理 seq 水位。`-1` 表示该类型还没收到过带 seq 的消息。
   * 拓扑样例不带 seq，因此 topology 水位仅在将来后端补 seq 时才有意义。
   */
  seq: { topology: number; nodeState: number; nodeOutput: number }
  /** 单调递增的接收计数，用于挑「最近活跃」的图。 */
  updatedAt: number
}

export interface DagStoreState {
  /** key = `${toolCallId}::${graphId}` */
  graphs: Map<string, DagGraphState>
  /** 最近一次收到消息的图 key；视图默认渲染它。 */
  activeKey: string | null
  ingest: (toolCallId: string, message: RaDagMessage) => void
  reset: () => void
}

export function dagGraphKey(toolCallId: string, graphId: string): string {
  return `${toolCallId}::${graphId}`
}

/** 输出片段只保留尾部，避免长时间运行的节点把 store 撑爆。 */
export const DAG_OUTPUT_TAIL = 400

// ── 纯 reducer ──

function isKnownState(value: unknown): value is RaDagNodeState {
  return (
    typeof value === "string" &&
    (RA_DAG_NODE_STATES as readonly string[]).includes(value)
  )
}

function normalizeTopologyNode(raw: unknown): RaDagTopologyNode | null {
  if (!raw || typeof raw !== "object") return null
  const o = raw as Record<string, unknown>
  if (typeof o.id !== "number" || !Number.isFinite(o.id)) return null
  const dependsOn = Array.isArray(o.dependsOn)
    ? o.dependsOn.filter((n): n is number => typeof n === "number")
    : []
  return {
    id: o.id,
    toolName: typeof o.toolName === "string" ? o.toolName : "",
    dependsOn,
    layer: typeof o.layer === "number" ? o.layer : 0,
  }
}

/** 把任意 unknown 的 topology 消息收窄成强类型；不合法返回 null。 */
export function normalizeTopologyMessage(
  raw: unknown
): RaDagTopologyMessage | null {
  if (!raw || typeof raw !== "object") return null
  const o = raw as Record<string, unknown>
  if (o.schema !== RA_DAG_TOPOLOGY_SCHEMA) return null
  if (typeof o.graphId !== "string" || o.graphId === "") return null
  const nodes = Array.isArray(o.nodes)
    ? o.nodes
        .map(normalizeTopologyNode)
        .filter((n): n is RaDagTopologyNode => n !== null)
    : []
  const layers = Array.isArray(o.layers)
    ? o.layers
        .filter((row): row is unknown[] => Array.isArray(row))
        .map((row) => row.filter((n): n is number => typeof n === "number"))
    : []
  return {
    schema: RA_DAG_TOPOLOGY_SCHEMA,
    graphId: o.graphId,
    nodes,
    layers,
    maxDepth: typeof o.maxDepth === "number" ? o.maxDepth : undefined,
    limits:
      o.limits && typeof o.limits === "object"
        ? (o.limits as RaDagLimits)
        : undefined,
    seq:
      typeof o.seq === "number" && Number.isFinite(o.seq) ? o.seq : undefined,
  }
}

/** 把任意 unknown 的 node_state 消息收窄成强类型；不合法返回 null。 */
export function normalizeNodeStateMessage(
  raw: unknown
): RaDagNodeStateMessage | null {
  if (!raw || typeof raw !== "object") return null
  const o = raw as Record<string, unknown>
  if (o.schema !== RA_DAG_NODE_STATE_SCHEMA) return null
  if (typeof o.graphId !== "string" || o.graphId === "") return null
  if (typeof o.nodeId !== "number" || !Number.isFinite(o.nodeId)) return null
  if (!isKnownState(o.state)) return null
  if (typeof o.seq !== "number" || !Number.isFinite(o.seq)) return null
  return {
    schema: RA_DAG_NODE_STATE_SCHEMA,
    graphId: o.graphId,
    nodeId: o.nodeId,
    state: o.state,
    seq: o.seq,
    layer: typeof o.layer === "number" ? o.layer : undefined,
  }
}

/** 把任意 unknown 的 node_output 消息收窄成强类型；不合法返回 null。 */
export function normalizeNodeOutputMessage(
  raw: unknown
): RaDagNodeOutputMessage | null {
  if (!raw || typeof raw !== "object") return null
  const o = raw as Record<string, unknown>
  if (o.schema !== RA_DAG_NODE_OUTPUT_SCHEMA) return null
  if (typeof o.graphId !== "string" || o.graphId === "") return null
  if (typeof o.nodeId !== "number" || !Number.isFinite(o.nodeId)) return null
  if (typeof o.delta !== "string") return null
  if (typeof o.seq !== "number" || !Number.isFinite(o.seq)) return null
  return {
    schema: RA_DAG_NODE_OUTPUT_SCHEMA,
    graphId: o.graphId,
    nodeId: o.nodeId,
    delta: o.delta,
    seq: o.seq,
  }
}

/**
 * 取（必要时创建）节点运行时投影。
 *
 * `layer` 是**可选**的：只有 topology 才权威地携带分层，node_state /
 * node_output 里的 `layer` 可能缺省。缺省时**必须保留已知的 layer** ——
 * 早先这里把缺省值写成 `0`，导致一条不带 layer 的 state 消息就把 topology
 * 排好的层号冲成 0，详情面板会显示错误的 layer。
 */
function ensureNode(
  graph: DagGraphState,
  id: number,
  layer?: number
): DagNodeRuntime {
  const existing = graph.nodes.get(id)
  if (existing) {
    if (layer !== undefined && layer !== existing.layer) {
      const next = { ...existing, layer }
      graph.nodes.set(id, next)
      return next
    }
    return existing
  }
  const fresh: DagNodeRuntime = {
    id,
    toolName: "",
    dependsOn: [],
    layer: layer ?? 0,
    state: "pending",
    output: "",
  }
  graph.nodes.set(id, fresh)
  return fresh
}

/**
 * 应用一条已收窄的消息。返回 `null` 表示该消息被丢弃（seq 旧了），
 * 否则返回（可能被就地更新的）graph。调用方负责放进新 Map 触发订阅。
 *
 * 就地改 graph/nodes 是刻意的：store 的 `graphs` Map 每次 ingest 都会换新，
 * 订阅方靠外层引用变化感知更新，内层无需逐节点复制。
 */
export function applyDagMessage(
  graph: DagGraphState,
  message: RaDagMessage
): DagGraphState | null {
  if (message.schema === RA_DAG_TOPOLOGY_SCHEMA) {
    const seq = graph.seq.topology
    if (message.seq !== undefined && message.seq <= seq) return null
    if (message.seq !== undefined) graph.seq.topology = message.seq

    graph.topology = message
    const seen = new Set<number>()
    for (const node of message.nodes) {
      seen.add(node.id)
      const runtime = ensureNode(graph, node.id, node.layer)
      graph.nodes.set(node.id, {
        ...runtime,
        toolName: node.toolName || runtime.toolName,
        dependsOn: node.dependsOn,
        layer: node.layer,
      })
    }
    // topology 换代时清掉不再存在的节点，避免旧图残留。
    for (const id of Array.from(graph.nodes.keys())) {
      if (!seen.has(id)) graph.nodes.delete(id)
    }
    return graph
  }

  if (message.schema === RA_DAG_NODE_STATE_SCHEMA) {
    if (message.seq <= graph.seq.nodeState) return null
    graph.seq.nodeState = message.seq
    const runtime = ensureNode(graph, message.nodeId, message.layer)
    graph.nodes.set(message.nodeId, { ...runtime, state: message.state })
    return graph
  }

  // node_output
  if (message.seq <= graph.seq.nodeOutput) return null
  graph.seq.nodeOutput = message.seq
  const runtime = ensureNode(graph, message.nodeId)
  const merged = runtime.output + message.delta
  graph.nodes.set(message.nodeId, {
    ...runtime,
    output:
      merged.length > DAG_OUTPUT_TAIL
        ? merged.slice(merged.length - DAG_OUTPUT_TAIL)
        : merged,
  })
  return graph
}

// ── 视图选择器（纯函数，可在无 React 环境单测）──

/** 单节点的上下游关系（依赖 / 被依赖），供详情面板渲染。 */
export interface DagNodeRelations {
  /** 该节点直接依赖的节点，按 id 升序。 */
  dependencies: DagNodeRuntime[]
  /** 直接依赖该节点的节点（反向边），按 id 升序。 */
  dependents: DagNodeRuntime[]
}

/**
 * 取一个节点的直接上下游。
 *
 * - 只算**直接**关系，不做传递闭包（分层图里传递依赖会淹没面板）；
 * - 忽略自引用（`dependsOn` 含自己）与图里不存在的悬空引用；
 * - 结果按 id 升序，避免 Map 迭代顺序让 UI 每次重排。
 */
export function selectDagNodeRelations(
  graph: DagGraphState,
  nodeId: number
): DagNodeRelations {
  const dependencies: DagNodeRuntime[] = []
  const dependents: DagNodeRuntime[] = []

  const self = graph.nodes.get(nodeId)
  if (self) {
    for (const dep of self.dependsOn) {
      if (dep === nodeId) continue
      const node = graph.nodes.get(dep)
      if (node) dependencies.push(node)
    }
  }

  for (const node of graph.nodes.values()) {
    if (node.id === nodeId) continue
    if (node.dependsOn.includes(nodeId)) dependents.push(node)
  }

  dependencies.sort((a, b) => a.id - b.id)
  dependents.sort((a, b) => a.id - b.id)
  return { dependencies, dependents }
}

/** 建一张空图（供首条消息或单测直接构造）。 */
export function createDagGraph(
  toolCallId: string,
  graphId: string
): DagGraphState {
  return {
    toolCallId,
    graphId,
    topology: null,
    nodes: new Map(),
    seq: { topology: -1, nodeState: -1, nodeOutput: -1 },
    updatedAt: 0,
  }
}

/**
 * 纯函数式入口：给定现有 store 快照 + 一条消息，返回新快照。
 * 被丢弃（旧 seq / 不认识的图）时返回原快照引用，方便调用方判等。
 */
export function reduceDag(
  state: Pick<DagStoreState, "graphs" | "activeKey">,
  toolCallId: string,
  message: RaDagMessage,
  tick: number
): Pick<DagStoreState, "graphs" | "activeKey"> {
  const key = dagGraphKey(toolCallId, message.graphId)
  const current =
    state.graphs.get(key) ?? createDagGraph(toolCallId, message.graphId)
  const applied = applyDagMessage(current, message)
  if (applied === null) return state

  applied.updatedAt = tick
  const next = new Map(state.graphs)
  next.set(key, applied)
  return { graphs: next, activeKey: key }
}

// ── zustand store ──

let ingestTick = 0

export const useDagStore = create<DagStoreState>((set, get) => ({
  graphs: new Map(),
  activeKey: null,
  ingest: (toolCallId, message) => {
    ingestTick += 1
    const snapshot = get()
    const next = reduceDag(snapshot, toolCallId, message, ingestTick)
    if (next.graphs !== snapshot.graphs) set(next)
  },
  reset: () => set({ graphs: new Map(), activeKey: null }),
}))
