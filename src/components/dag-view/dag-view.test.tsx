import { describe, expect, it } from "vitest"
import { fireEvent, render, screen } from "@testing-library/react"
import {
  RA_DAG_NODE_OUTPUT_SCHEMA,
  RA_DAG_NODE_STATE_SCHEMA,
  applyDagMessage,
  createDagGraph,
  type DagGraphState,
  type RaDagMessage,
} from "./dag-model"
import { DagView } from "./dag-view"
import { mockTopology } from "./dag-mock"

/**
 * DagView 的接线冒烟测试。
 *
 * 这一层单测的价值在于验证 **React Flow 的事件接线假设**成立：节点包裹层的
 * `pointerEvents` 只在 `isSelectable` 为真时打开，`onNodeClick` 也只在
 * `elementsSelectable` 为真时才派发（见 dag-view.tsx 顶部注释）。这些都写在
 * 库源码里，纯函数单测覆盖不到，只能靠真实渲染 ReactFlow 来兜。
 */
function buildGraph(): DagGraphState {
  const graph = createDagGraph("tc1", "g1")
  applyDagMessage(graph, mockTopology as unknown as RaDagMessage)
  applyDagMessage(graph, {
    schema: RA_DAG_NODE_STATE_SCHEMA,
    graphId: "g1",
    nodeId: 2,
    state: "running",
    seq: 1,
  } as RaDagMessage)
  applyDagMessage(graph, {
    schema: RA_DAG_NODE_OUTPUT_SCHEMA,
    graphId: "g1",
    nodeId: 2,
    delta: "reading",
    seq: 2,
  } as RaDagMessage)
  return graph
}

describe("DagView selection wiring", () => {
  it("opens the node detail when a node card is clicked", () => {
    render(<DagView graph={buildGraph()} />)

    // 详情面板初始不存在。
    expect(screen.queryByRole("complementary")).toBeNull()

    // 节点卡片本身就是触发器，aria-label 里带状态文案（无障碍契约的一部分）。
    const card = nodeCard()
    expect(card).toHaveAttribute("data-dag-node-state", "running")
    expect(card).toHaveAttribute("aria-label", "read — Running")

    fireEvent.click(card)

    const detail = screen.getByRole("complementary")
    expect(detail).toHaveAttribute("data-dag-node-detail", "2")
    // 输出流进了详情面板（节点卡片上也会内联显示 running 输出，故限定在面板内查）。
    expect(detail.querySelector("[data-dag-node-output]")?.textContent).toBe(
      "reading"
    )
  })

  it("closes the detail when the pane background is clicked", () => {
    const { container } = render(<DagView graph={buildGraph()} />)
    fireEvent.click(nodeCard())
    expect(screen.getByRole("complementary")).toBeInTheDocument()

    const pane = container.querySelector(".react-flow__pane")
    expect(pane).toBeTruthy()
    fireEvent.click(pane!)

    expect(screen.queryByRole("complementary")).toBeNull()
  })

  it("renders nothing when the graph has no nodes yet", () => {
    const { container } = render(
      <DagView graph={createDagGraph("tc1", "empty")} />
    )
    expect(container).toBeEmptyDOMElement()
  })
})

/**
 * 取节点 2 的卡片。
 *
 * 这里刻意**不用 `getByRole(..., { name })`**：jsdom 测得的容器是 0×0，
 * React Flow 因此把节点包裹层置为 `visibility: hidden`（真实浏览器里有尺寸，
 * 节点是可见的），而 `dom-accessibility-api` 对隐藏节点算出的 accessible name
 * 是空串 —— 按名字查必然落空。角色与 `aria-label` 属性本身仍然可用，所以直接
 * 按属性取元素，`aria-label` 另用属性断言覆盖（见下面的用例）。
 */
function nodeCard(): HTMLElement {
  const card = document.querySelector<HTMLElement>(
    '[data-dag-node-state="running"]'
  )
  if (!card) throw new Error("running DAG node card not found")
  return card
}
