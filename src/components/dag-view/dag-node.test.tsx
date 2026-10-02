import { describe, expect, it, vi } from "vitest"
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import type { NodeProps } from "@xyflow/react"
import { DagNode } from "./dag-node"
import type { DagFlowNode } from "./dag-edges"
import {
  RA_DAG_NODE_STATES,
  type DagNodeRuntime,
  type RaDagNodeState,
} from "./dag-model"
import { DAG_STATE_LABEL } from "./dag-state"

/**
 * 节点卡片的无障碍契约：**状态不能只靠颜色**。每个状态都必须渲染出
 * 文案（可读文本）+ 图标（形状），并把它写进 aria-label。
 * 同时覆盖鼠标点击与键盘 Enter/Space 两条激活路径。
 */
function nodeProps(
  state: RaDagNodeState,
  options: {
    runtime?: Partial<DagNodeRuntime>
    selected?: boolean
    onSelect?: (nodeId: number) => void
  } = {}
) {
  const node: DagNodeRuntime = {
    id: 7,
    toolName: "grep",
    dependsOn: [],
    layer: 1,
    state,
    output: "",
    ...options.runtime,
  }
  return {
    id: String(node.id),
    data: { node, selected: options.selected, onSelect: options.onSelect },
  } as unknown as NodeProps<DagFlowNode>
}

describe("DagNode state styling", () => {
  it.each(RA_DAG_NODE_STATES)(
    "renders %s with a text label, an icon and an aria-label",
    (state) => {
      const { container } = render(<DagNode {...nodeProps(state)} />)

      // 文案通道：状态在视觉上永远有文字，不依赖颜色。
      expect(screen.getByText(DAG_STATE_LABEL[state])).toBeInTheDocument()
      // 形状通道：每个状态都带一个图标。
      expect(container.querySelector("svg")).toBeTruthy()

      const card = screen.getByRole("button")
      expect(card).toHaveAttribute("data-dag-node-state", state)
      expect(card.getAttribute("aria-label")).toContain(DAG_STATE_LABEL[state])
      expect(card).toHaveAttribute("tabindex", "0")
    }
  )

  it("marks the selected node for assistive tech", () => {
    const { container } = render(<DagNode {...nodeProps("running")} />)
    expect(container.firstElementChild).not.toHaveAttribute(
      "data-dag-node-selected"
    )
    expect(screen.getByRole("button")).toHaveAttribute("aria-pressed", "false")

    const { container: selected } = render(
      <DagNode {...nodeProps("running", { selected: true })} />
    )
    expect(selected.firstElementChild).toHaveAttribute(
      "data-dag-node-selected",
      "true"
    )
  })

  it("falls back to the node id when the tool name is missing", () => {
    render(<DagNode {...nodeProps("pending", { runtime: { toolName: "" } })} />)
    expect(screen.getByText("node 7")).toBeInTheDocument()
  })
})

describe("DagNode activation", () => {
  it("reports the node id on click", async () => {
    const user = userEvent.setup()
    const onSelect = vi.fn()
    render(<DagNode {...nodeProps("succeeded", { onSelect })} />)

    await user.click(screen.getByRole("button"))
    expect(onSelect).toHaveBeenCalledWith(7)
  })

  it("is activatable from the keyboard with Enter and Space", async () => {
    const user = userEvent.setup()
    const onSelect = vi.fn()
    render(<DagNode {...nodeProps("failed", { onSelect })} />)

    screen.getByRole("button").focus()
    await user.keyboard("{Enter}")
    await user.keyboard(" ")
    expect(onSelect).toHaveBeenCalledTimes(2)
    expect(onSelect).toHaveBeenNthCalledWith(1, 7)
  })

  it("stays inert when no handler is wired", async () => {
    const user = userEvent.setup()
    render(<DagNode {...nodeProps("pending")} />)
    // 没有 onSelect 时点击不应抛错（宿主未注入回调的降级路径）。
    await user.click(screen.getByRole("button"))
    expect(screen.getByRole("button")).toBeInTheDocument()
  })
})
