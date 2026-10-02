/**
 * 开发用 mock：一组假 topology / node_state / node_output 消息。
 *
 * 后端还没发 `ra.dev/dag` 事件，联调前用它验证渲染与动画：
 *
 * ```ts
 * import { feedMockDag } from "@/components/dag-view/dag-mock"
 * feedMockDag()          // 立即灌入整条时间线（终态）
 * feedMockDag({ replay: true }) // 按 400ms 间隔重放，可看到 running 动画
 * ```
 *
 * `feedMockDag` 直接调 store 的 `ingest`，不经过 ACP，因此不依赖连接。
 */
import {
  RA_DAG_META_KEY,
  RA_DAG_NODE_OUTPUT_SCHEMA,
  RA_DAG_NODE_STATE_SCHEMA,
  RA_DAG_TOPOLOGY_SCHEMA,
  useDagStore,
  type RaDagMessage,
} from "./dag-model"
import { extractDagMessage } from "./dag-meta-parser"

export const MOCK_TOOL_CALL_ID = "mock-tool-call-1"
export const MOCK_GRAPH_ID = "g1"

export const mockTopology = {
  schema: RA_DAG_TOPOLOGY_SCHEMA,
  graphId: MOCK_GRAPH_ID,
  nodes: [
    { id: 1, toolName: "search", dependsOn: [], layer: 0 },
    { id: 2, toolName: "read", dependsOn: [1], layer: 1 },
    { id: 3, toolName: "grep", dependsOn: [1], layer: 1 },
    { id: 4, toolName: "synth", dependsOn: [2, 3], layer: 2 },
    { id: 5, toolName: "verify", dependsOn: [4], layer: 3 },
  ],
  layers: [[1], [2, 3], [4], [5]],
  maxDepth: 4,
  limits: { nodes: 256, layerWidth: 64 },
}

/** 每步的 (nodeId → state)，按时间顺序；seq 单调递增。 */
export const mockStateSteps: Array<{
  nodeId: number
  state: string
  seq: number
  layer?: number
}> = [
  { nodeId: 1, state: "running", seq: 1, layer: 0 },
  { nodeId: 1, state: "succeeded", seq: 2 },
  { nodeId: 2, state: "running", seq: 3, layer: 1 },
  { nodeId: 3, state: "running", seq: 4, layer: 1 },
  { nodeId: 3, state: "failed", seq: 6 },
  { nodeId: 2, state: "succeeded", seq: 7 },
  { nodeId: 4, state: "running", seq: 8, layer: 2 },
  { nodeId: 4, state: "succeeded", seq: 10 },
  { nodeId: 5, state: "skipped", seq: 11, layer: 3 },
]

export const mockOutputDeltas: Array<{
  nodeId: number
  delta: string
  seq: number
}> = [
  { nodeId: 2, delta: "reading src/app.ts", seq: 5 },
  { nodeId: 2, delta: "… 120 lines", seq: 9 },
]

/** 交错后的完整时间线：topology 先行，state/output 按 seq 混排。 */
export function mockMessageTimeline(): RaDagMessage[] {
  const events: Array<{ seq: number; message: RaDagMessage }> = []
  for (const step of mockStateSteps) {
    events.push({
      seq: step.seq,
      message: {
        schema: RA_DAG_NODE_STATE_SCHEMA,
        graphId: MOCK_GRAPH_ID,
        nodeId: step.nodeId,
        state: step.state,
        seq: step.seq,
        layer: step.layer,
      } as RaDagMessage,
    })
  }
  for (const out of mockOutputDeltas) {
    events.push({
      seq: out.seq,
      message: {
        schema: RA_DAG_NODE_OUTPUT_SCHEMA,
        graphId: MOCK_GRAPH_ID,
        nodeId: out.nodeId,
        delta: out.delta,
        seq: out.seq,
      },
    })
  }
  events.sort((a, b) => a.seq - b.seq)
  return [
    mockTopology as unknown as RaDagMessage,
    ...events.map((e) => e.message),
  ]
}

/** 把消息按 `_meta` 真实形状过一遍 parser 再 ingest，端到端覆盖两条路径。 */
function ingestRaw(message: RaDagMessage): void {
  const meta = { [RA_DAG_META_KEY]: message }
  const parsed = extractDagMessage(meta)
  if (parsed) useDagStore.getState().ingest(MOCK_TOOL_CALL_ID, parsed)
}

export interface FeedMockOptions {
  /** true = 按 400ms 步进重放（能看到 running 脉冲）；默认一步到位。 */
  replay?: boolean
  intervalMs?: number
}

export function feedMockDag(options: FeedMockOptions = {}): () => void {
  const { replay = false, intervalMs = 400 } = options
  useDagStore.getState().reset()
  const timeline = mockMessageTimeline()

  if (!replay) {
    for (const message of timeline) ingestRaw(message)
    return () => useDagStore.getState().reset()
  }

  let index = 0
  ingestRaw(timeline[0]) // topology 立刻到位，图先画出来
  index = 1
  const timer = setInterval(() => {
    if (index >= timeline.length) {
      clearInterval(timer)
      return
    }
    ingestRaw(timeline[index])
    index += 1
  }, intervalMs)

  return () => {
    clearInterval(timer)
    useDagStore.getState().reset()
  }
}
