import { describe, expect, it, vi } from "vitest"
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import {
  RA_DAG_NODE_OUTPUT_SCHEMA,
  RA_DAG_NODE_STATE_SCHEMA,
  applyDagMessage,
  createDagGraph,
  type DagGraphState,
  type RaDagMessage,
} from "./dag-model"
import { DagNodeDetail } from "./dag-node-detail"
import { mockTopology } from "./dag-mock"
import { DAG_STATE_HINT, DAG_STATE_LABEL } from "./dag-state"

/** node 1 完成 → node 2 运行中并产出两段输出；用它验证详情面板的每条信息通路。 */
function buildGraph(): DagGraphState {
  const graph = createDagGraph("tc1", "g1")
  applyDagMessage(graph, mockTopology as unknown as RaDagMessage)
  const state = (nodeId: number, value: string, seq: number) =>
    applyDagMessage(graph, {
      schema: RA_DAG_NODE_STATE_SCHEMA,
      graphId: "g1",
      nodeId,
      state: value,
      seq,
    } as RaDagMessage)
  const output = (nodeId: number, delta: string, seq: number) =>
    applyDagMessage(graph, {
      schema: RA_DAG_NODE_OUTPUT_SCHEMA,
      graphId: "g1",
      nodeId,
      delta,
      seq,
    } as RaDagMessage)

  state(1, "succeeded", 1)
  state(2, "running", 2)
  output(2, "reading src/app.ts ", 3)
  output(2, "... 120 lines", 4)
  return graph
}

interface Rendered {
  onSelect: ReturnType<typeof vi.fn>
  onClose: ReturnType<typeof vi.fn>
}

function renderDetail(graph: DagGraphState, nodeId: number): Rendered {
  const onSelect = vi.fn()
  const onClose = vi.fn()
  render(
    <DagNodeDetail
      graph={graph}
      nodeId={nodeId}
      onSelect={onSelect}
      onClose={onClose}
    />
  )
  return { onSelect, onClose }
}

describe("DagNodeDetail", () => {
  it("shows the node identity, state wording and the state hint", () => {
    const graph = buildGraph()
    renderDetail(graph, 2)

    expect(screen.getByRole("complementary")).toHaveAttribute(
      "data-dag-node-detail",
      "2"
    )
    expect(screen.getByText("read")).toBeInTheDocument()
    expect(screen.getByText("#2 · layer 1")).toBeInTheDocument()
    // 状态同时以「文案 + 提示句」呈现，颜色只是第三条通道。
    expect(screen.getByText(DAG_STATE_LABEL.running)).toBeInTheDocument()
    expect(screen.getByText(DAG_STATE_HINT.running)).toBeInTheDocument()
    expect(screen.getByText("#2 · layer 1")).toBeInTheDocument()
  })

  it("lists direct dependencies and dependents as navigable chips", async () => {
    const user = userEvent.setup()
    const graph = buildGraph()
    const { onSelect } = renderDetail(graph, 2)

    expect(screen.getByText("Dependencies")).toBeInTheDocument()
    expect(screen.getByText("Dependents")).toBeInTheDocument()
    expect(screen.getByText("search")).toBeInTheDocument()
    expect(screen.getByText("synth")).toBeInTheDocument()

    await user.click(screen.getByText("search"))
    expect(onSelect).toHaveBeenCalledWith(1)
  })

  it("renders the streamed output and flags that it is only the tail", () => {
    const graph = buildGraph()
    renderDetail(graph, 2)

    const pre = document.querySelector("[data-dag-node-output]")
    expect(pre?.textContent).toBe("reading src/app.ts ... 120 lines")
    expect(
      screen.getByText(/Tail of the streamed output \(last \d+ characters\)/)
    ).toBeInTheDocument()
  })

  it("explains the empty state instead of leaving a blank box", () => {
    const graph = createDagGraph("tc1", "g1")
    applyDagMessage(graph, mockTopology as unknown as RaDagMessage)
    applyDagMessage(graph, {
      schema: RA_DAG_NODE_STATE_SCHEMA,
      graphId: "g1",
      nodeId: 3,
      state: "running",
      seq: 1,
    } as RaDagMessage)

    renderDetail(graph, 3)
    expect(screen.getByText("Waiting for output…")).toBeInTheDocument()
    expect(document.querySelector("[data-dag-node-output]")).toBeNull()
  })

  it("states the protocol limits so the tail is not read as a full log", () => {
    renderDetail(buildGraph(), 2)
    expect(
      screen.getByText(/aren't carried by the protocol yet/)
    ).toBeInTheDocument()
  })

  it("closes on the ✕ button and degrades to null for an unknown node", async () => {
    const user = userEvent.setup()
    const graph = buildGraph()
    const { onClose } = renderDetail(graph, 2)

    await user.click(screen.getByRole("button", { name: "Close node details" }))
    expect(onClose).toHaveBeenCalledTimes(1)

    const empty = createDagGraph("tc1", "g1")
    const { container } = render(
      <DagNodeDetail
        graph={empty}
        nodeId={999}
        onSelect={vi.fn()}
        onClose={vi.fn()}
      />
    )
    expect(container).toBeEmptyDOMElement()
  })
})
