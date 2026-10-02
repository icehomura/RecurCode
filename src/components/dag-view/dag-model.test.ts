import { describe, expect, it } from "vitest"
import {
  DAG_H_GAP,
  DAG_NODE_HEIGHT,
  DAG_NODE_WIDTH,
  DAG_V_GAP,
  buildDagEdges,
  computeLayerPositions,
} from "./dag-edges"
import {
  RA_DAG_META_KEY,
  RA_DAG_NODE_OUTPUT_SCHEMA,
  RA_DAG_NODE_STATE_SCHEMA,
  RA_DAG_NODE_STATES,
  RA_DAG_TOPOLOGY_SCHEMA,
  applyDagMessage,
  createDagGraph,
  dagGraphKey,
  reduceDag,
  selectDagNodeRelations,
  type RaDagMessage,
} from "./dag-model"
import {
  DAG_STATE_HINT,
  DAG_STATE_ICON,
  DAG_STATE_LABEL,
  DAG_STATE_TONE,
} from "./dag-state"
import { extractDagMessage, parseDagMessage } from "./dag-meta-parser"
import { mockTopology } from "./dag-mock"

function graph() {
  return createDagGraph("tc1", "g1")
}

function topo(overrides: Record<string, unknown> = {}) {
  return {
    ...mockTopology,
    ...overrides,
  } as unknown as RaDagMessage
}

function stateMsg(
  nodeId: number,
  state: string,
  seq: number,
  graphId = "g1",
  layer?: number
): RaDagMessage {
  return {
    schema: RA_DAG_NODE_STATE_SCHEMA,
    graphId,
    nodeId,
    state,
    seq,
    layer,
  } as unknown as RaDagMessage
}

describe("schema parsing", () => {
  it("parses all three ra.dag.* schemas", () => {
    expect(parseDagMessage(mockTopology)?.schema).toBe(RA_DAG_TOPOLOGY_SCHEMA)
    expect(
      parseDagMessage({
        schema: RA_DAG_NODE_STATE_SCHEMA,
        graphId: "g1",
        nodeId: 2,
        state: "running",
        seq: 7,
        layer: 1,
      })?.schema
    ).toBe(RA_DAG_NODE_STATE_SCHEMA)
    expect(
      parseDagMessage({
        schema: RA_DAG_NODE_OUTPUT_SCHEMA,
        graphId: "g1",
        nodeId: 2,
        delta: "…",
        seq: 8,
      })?.schema
    ).toBe(RA_DAG_NODE_OUTPUT_SCHEMA)
  })

  it("rejects non-ra.dag.* schemas and malformed payloads", () => {
    expect(
      parseDagMessage({ schema: "codex.dag.topology.v1", graphId: "g" })
    ).toBeNull()
    expect(parseDagMessage({ schema: "ra.dev/other" })).toBeNull()
    expect(parseDagMessage({ schema: "ra.dag.unknown.v9" })).toBeNull()
    expect(parseDagMessage(null)).toBeNull()
    expect(parseDagMessage("nope")).toBeNull()
    // 缺 seq 的 state 消息不合法
    expect(
      parseDagMessage({
        schema: RA_DAG_NODE_STATE_SCHEMA,
        graphId: "g1",
        nodeId: 1,
        state: "running",
      })
    ).toBeNull()
    // 未知状态值不合法
    expect(
      parseDagMessage({
        schema: RA_DAG_NODE_STATE_SCHEMA,
        graphId: "g1",
        nodeId: 1,
        state: "weird",
        seq: 1,
      })
    ).toBeNull()
  })

  it("extracts from meta under ra.dev/dag and ignores other meta keys", () => {
    const message = {
      schema: RA_DAG_NODE_STATE_SCHEMA,
      graphId: "g1",
      nodeId: 1,
      state: "running",
      seq: 1,
    }
    expect(extractDagMessage({ [RA_DAG_META_KEY]: message })?.schema).toBe(
      RA_DAG_NODE_STATE_SCHEMA
    )
    expect(extractDagMessage({ other: message })).toBeNull()
    expect(extractDagMessage(null)).toBeNull()
    expect(extractDagMessage({ [RA_DAG_META_KEY]: "junk" })).toBeNull()
  })
})

describe("seq dedup / out-of-order drop", () => {
  it("applies increasing seq and drops stale seq for the same type", () => {
    const g = graph()
    expect(applyDagMessage(g, stateMsg(1, "running", 3))).not.toBeNull()
    expect(g.nodes.get(1)?.state).toBe("running")

    // 旧 seq：丢弃，状态不回退
    expect(applyDagMessage(g, stateMsg(1, "pending", 2))).toBeNull()
    expect(g.nodes.get(1)?.state).toBe("running")

    // 同 seq：重复到达，也丢弃
    expect(applyDagMessage(g, stateMsg(1, "failed", 3))).toBeNull()
    expect(g.nodes.get(1)?.state).toBe("running")

    // 新 seq：接受
    expect(applyDagMessage(g, stateMsg(1, "succeeded", 4))).not.toBeNull()
    expect(g.nodes.get(1)?.state).toBe("succeeded")
  })

  it("tracks node_state and node_output watermarks independently", () => {
    const g = graph()
    applyDagMessage(g, stateMsg(1, "running", 5))
    applyDagMessage(g, {
      schema: RA_DAG_NODE_OUTPUT_SCHEMA,
      graphId: "g1",
      nodeId: 1,
      delta: "a",
      seq: 6,
    } as RaDagMessage)
    // output 侧 seq 6 已消费；state 侧水位仍是 5，state seq=7 通过
    expect(applyDagMessage(g, stateMsg(1, "succeeded", 7))).not.toBeNull()
    // output 侧 seq=4 属旧水位
    expect(
      applyDagMessage(g, {
        schema: RA_DAG_NODE_OUTPUT_SCHEMA,
        graphId: "g1",
        nodeId: 1,
        delta: "stale",
        seq: 4,
      } as RaDagMessage)
    ).toBeNull()
    expect(g.nodes.get(1)?.output).toBe("a")
  })

  it("keeps different graphIds in separate buckets", () => {
    let state = { graphs: new Map(), activeKey: null as string | null }
    const tick = { n: 0 }
    const step = (graphId: string) => {
      tick.n += 1
      state = reduceDag(
        state,
        "tc1",
        stateMsg(1, "running", 1, graphId),
        tick.n
      )
    }
    step("g1")
    step("g2")
    expect(state.graphs.size).toBe(2)
    expect(state.activeKey).toBe(dagGraphKey("tc1", "g2"))
  })
})

describe("layer preservation", () => {
  it("keeps the topology layer when state/output messages omit it", () => {
    const g = graph()
    applyDagMessage(g, topo()) // node 2/3 在 layer 1
    applyDagMessage(g, stateMsg(2, "running", 1)) // 不带 layer
    applyDagMessage(g, {
      schema: RA_DAG_NODE_OUTPUT_SCHEMA,
      graphId: "g1",
      nodeId: 3,
      delta: "x",
      seq: 1,
    } as RaDagMessage)

    expect(g.nodes.get(2)?.layer).toBe(1)
    expect(g.nodes.get(3)?.layer).toBe(1)
  })

  it("still accepts an explicit layer carried by a state message", () => {
    const g = graph()
    applyDagMessage(g, topo())
    applyDagMessage(g, stateMsg(2, "running", 2, "g1", 3))
    expect(g.nodes.get(2)?.layer).toBe(3)
  })
})

describe("dependsOn → edges", () => {
  it("inverts dependsOn into source→target edges", () => {
    const g = graph()
    applyDagMessage(g, topo())
    const edges = buildDagEdges(g)
    const ids = edges.map((e) => `${e.source}->${e.target}`).sort()
    expect(ids).toEqual(["1->2", "1->3", "2->4", "3->4", "4->5"])
    // 边 id 稳定、无重复
    expect(new Set(edges.map((e) => e.id)).size).toBe(edges.length)
  })

  it("marks incoming edges of a running node as flow, outgoing as static", () => {
    const g = graph()
    applyDagMessage(g, topo())
    applyDagMessage(g, stateMsg(1, "succeeded", 1))
    applyDagMessage(g, stateMsg(2, "running", 2))
    const edges = buildDagEdges(g)

    const intoRunning = edges.filter((e) => e.target === "2")
    expect(intoRunning).toHaveLength(1)
    expect(intoRunning[0].data?.flow).toBe(true)
    // 源节点 1 已 succeeded → 边着绿
    expect(intoRunning[0].data?.tone).toBe("succeeded")

    // 出边（2 → 4）目标没在 running，静态
    const outOfRunning = edges.filter((e) => e.source === "2")
    expect(outOfRunning).toHaveLength(1)
    expect(outOfRunning[0].data?.flow).toBe(false)
    // 出边按源节点状态着色 → running 蓝
    expect(outOfRunning[0].data?.tone).toBe("running")
  })

  it("drops edges whose endpoint is missing from the graph", () => {
    const g = graph()
    applyDagMessage(g, topo())
    // topology 换代只留节点 1、2，但 2 dependsOn 1 → 保留；再删掉 1
    applyDagMessage(
      g,
      topo({ nodes: [{ id: 2, toolName: "read", dependsOn: [1], layer: 0 }] })
    )
    const edges = buildDagEdges(g)
    expect(edges.find((e) => e.source === "1")).toBeUndefined()
  })
})

describe("node state visual tokens", () => {
  it("covers every protocol state with a label, hint, tone and icon", () => {
    for (const state of RA_DAG_NODE_STATES) {
      expect(DAG_STATE_LABEL[state]).toBeTruthy()
      expect(DAG_STATE_HINT[state]).toBeTruthy()
      expect(DAG_STATE_TONE[state]).toBeTruthy()
      expect(DAG_STATE_ICON[state]).toBeTruthy()
    }
  })

  it("keeps the states distinguishable without colour", () => {
    // 无障碍：不能只靠颜色。文案必须能区分状态（唯一），图标形状也必须各不相同。
    const labels = RA_DAG_NODE_STATES.map((s) => DAG_STATE_LABEL[s])
    expect(new Set(labels).size).toBe(RA_DAG_NODE_STATES.length)

    const icons = RA_DAG_NODE_STATES.map((s) => DAG_STATE_ICON[s])
    expect(new Set(icons).size).toBe(RA_DAG_NODE_STATES.length)
  })
})

describe("selectDagNodeRelations", () => {
  function graphWithTopology(mutate?: (g: ReturnType<typeof graph>) => void) {
    const g = graph()
    applyDagMessage(g, topo())
    mutate?.(g)
    return g
  }

  it("splits direct dependencies and dependents, sorted by id", () => {
    const g = graphWithTopology()
    const mid = selectDagNodeRelations(g, 4)
    expect(mid.dependencies.map((n) => n.id)).toEqual([2, 3])
    expect(mid.dependents.map((n) => n.id)).toEqual([5])

    const root = selectDagNodeRelations(g, 1)
    expect(root.dependencies).toEqual([])
    expect(root.dependents.map((n) => n.id)).toEqual([2, 3])

    const leaf = selectDagNodeRelations(g, 5)
    expect(leaf.dependencies.map((n) => n.id)).toEqual([4])
    expect(leaf.dependents).toEqual([])
  })

  it("returns runtime nodes (carrying state), not just ids", () => {
    const g = graphWithTopology((graph) =>
      applyDagMessage(graph, stateMsg(2, "succeeded", 1))
    )
    const [dep] = selectDagNodeRelations(g, 4).dependencies
    expect(dep.toolName).toBe("read")
    expect(dep.state).toBe("succeeded")
  })

  it("ignores self-references and dangling ids", () => {
    const g = graph()
    // 自引用 + 指向已被 topology 换代清掉的节点
    applyDagMessage(
      g,
      topo({
        nodes: [
          { id: 1, toolName: "a", dependsOn: [1, 99], layer: 0 },
          { id: 2, toolName: "b", dependsOn: [1], layer: 1 },
        ],
      })
    )
    const self = selectDagNodeRelations(g, 1)
    expect(self.dependencies).toEqual([])
    expect(self.dependents.map((n) => n.id)).toEqual([2])

    // 不存在的节点：两侧都空，不抛错
    const missing = selectDagNodeRelations(g, 42)
    expect(missing.dependencies).toEqual([])
    expect(missing.dependents).toEqual([])
  })
})

describe("layer layout coordinates", () => {
  it("maps layer to y and spreads same-layer nodes evenly on x", () => {
    const g = graph()
    applyDagMessage(g, topo())
    const pos = computeLayerPositions(g)

    // layer 0 → y=0；layer 1 → y = 60+40
    expect(pos.get(1)?.y).toBe(0)
    expect(pos.get(2)?.y).toBe(DAG_NODE_HEIGHT + DAG_V_GAP)
    expect(pos.get(3)?.y).toBe(DAG_NODE_HEIGHT + DAG_V_GAP)
    expect(pos.get(4)?.y).toBe(2 * (DAG_NODE_HEIGHT + DAG_V_GAP))

    // 同层两节点（2,3）等距 = 宽度 + 间距
    expect(pos.get(3)!.x - pos.get(2)!.x).toBe(DAG_NODE_WIDTH + DAG_H_GAP)
    // 居中：单节点层的 x 使得整体跨 0 对称
    expect(pos.get(1)!.x).toBe(-DAG_NODE_WIDTH / 2)
    expect(pos.get(4)!.x).toBe(-DAG_NODE_WIDTH / 2)
  })

  it("falls back to per-node layer when topology.layers is empty", () => {
    const g = graph()
    applyDagMessage(
      g,
      topo({
        layers: [],
        nodes: [
          { id: 10, toolName: "a", dependsOn: [], layer: 0 },
          { id: 11, toolName: "b", dependsOn: [10], layer: 2 },
        ],
      })
    )
    const pos = computeLayerPositions(g)
    expect(pos.get(10)?.y).toBe(0)
    // layer=2 的节点仍按分组顺序占第 2 行（只画了两行，索引 1）
    expect(pos.get(11)!.y).toBeGreaterThan(pos.get(10)!.y)
  })
})
