# Components 重构实现 Spec（无技术债版）

状态：待实现。目标读者：实现代理 + 对抗审查代理。
范围：RecurCode 的 Components 设置面板及其后端真相模型。

---

## 0. 背景与事故记录（已用 git 核实）

- `1dc16527 feat(components): report and toggle real mcp and skill state`：
  一次性提交了「mcp/skill 真实状态 + 类型化动作」的重构，**同时**裹进了另一个改动的
  PATH shim 删除（`sync_component_shims` / `components_bin_dir` / `prepend_components_bin_path`，
  约 −45 行）。两种意图被混进同一个提交，HEAD 一度不可用。
- `f1b52b00 revert: drop the component mcp and skill wiring that deleted the PATH shims`：
  把 `1dc16527` 整体回退，PATH shim 机制**已恢复**，mcp/skill 接线也一并退回旧版。
- `bd344604 fix: require the exe suffix on a windows staged sidecar name`：
  补回被误回滚的合法修复（Windows 侧车文件名必须带 `.exe`）。当前 HEAD。
- 当前 `cargo check` 干净，components 测试 9/9 通过。

**本条 spec 的 PATH 决定：保留 PATH shim（已是当前状态）。**
注意这与更早的「方案 A：把 applet 从 agent PATH 上拿掉」相反。除非操作者另行说明，
本 spec 一律以「保留 shim」为准，不再改动 PATH 相关代码（见 §6 硬约束）。

### 为什么要重做（旧模型的结构性缺陷）

旧模型把每个组件压成**一个布尔 `enabled`**。对 rc-mcp 这是撒谎，因为 rc-mcp 不是
一个开关，而是「socket + 二进制 + 九组工具开关 + 启动状态」的复合体。由此产生四个真问题：

| 级别 | 问题 | 根因 |
|---|---|---|
| blocker | 15 个 applet 失去可达性 | 已由 `f1b52b00` revert 修复，本 spec 保证不复现 |
| major | `@rc-mcp` 在服务 `Stopped/Unavailable` 时仍显示为「开」 | 单一 `enabled` 读的是机制以外的东西 |
| major | 开启 rc-mcp 吞掉 broker 启动失败，返回成功 | `set_enabled` 无错误传播 |
| major | 关闭 rc-mcp 不可逆地清掉 7 个无关功能开关，重开也无法恢复 | 「一个开关切九组」 |
| minor | 技能与用户自有同名技能冲突时，冲突被丢弃但仍显示「可安装」 | conflict 未进模型 |

**结论**：为 binary/multicall 设计的扁平布尔模型，绝不能套用到 mcp/skill。必须让
`ComponentInfo` 携带**按类别打标签的联合**，并让动作**落到真实机制函数**。

---

## 1. 目标 / 非目标

目标
- `ComponentInfo` 不再共用一套扁平字段，改为按 `category` 打标签的 `state` 联合。
- 写操作从 `components_set_enabled(id, bool)` 改为类型化 `components_apply({kind, ...})`，
  每个 `kind` 落到真实函数。
- `component_setting` 只保存 binary/multicall 的用户选择；mcp/skill 以机制为唯一真相。
- 前端详情面板按 `state.kind` 分派渲染；MCP 面板 4 态 + 逐组开关；技能面板逐 agent 链接矩阵 + 冲突提示。

非目标
- 不改 PATH shim 机制（保留）。
- 不改 binary / multicall 的现有行为与测试（只允许字段搬家）。
- 不新增/删除内置组件定义本身。

---

## 2. 数据模型（后端）

`src-tauri/src/commands/components.rs`。

把 `ComponentInfo` 的扁平字段替换为 `state: ComponentState`，`ComponentState` 用
`#[serde(tag = "kind", rename_all = "snake_case")]` 的 internally-tagged 联合。
保留 `id` / `category` / `name` / `description` / `required` / `env`（这些是类别无关的展示字段）。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentInfo {
    pub id: String,
    pub category: ComponentCategory, // "binary" | "multicall" | "mcp" | "skill"
    pub name: String,
    pub description: String,
    pub required: bool,
    pub env: HashMap<String, String>,
    pub state: ComponentState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComponentState {
    /// 单用途二进制（`ra`）。
    Binary(BinaryState),
    /// 多调用二进制（`rb`）：一个二进制 + 若干 applet。
    Multicall(MulticallState),
    /// rc-mcp：直接复用真实状态，不另造布尔。
    Mcp(Box<RcMcpServiceStatus>),
    /// 生成式 UI 技能。
    Skill(SkillState),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryState {
    pub tool: String,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub update_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticallState {
    pub binary: BinaryState,
    pub tools: Vec<MulticallTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticallTool {
    pub id: String,          // 稳定 id：`@recurbusybox:rg`
    pub name: String,        // `rg`
    pub enabled: bool,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillState {
    /// central 存储中心的实际状态。
    pub central: SkillCentral,      // present | missing | conflict
    pub linked_agents: Vec<String>, // 已链接的 agent wire slug
    pub bundled_hash: Option<String>,
    pub central_hash: Option<String>,
    pub update_available: bool,
}
```

要求
- `Mcp(Mcp)` 必须**逐字段**来自 `RcMcpServiceStatus`（`commands/mcp_service.rs`），
  包括 `state`（`stopped|unavailable|disabled|running`）、`listening`、`socket_path`、
  `binary_path`、`tool_groups[{key,enabled,requires}]`、`companion_count`、`session_count`、
  `active_delegations`、`depth_limit`、`started_at`、`last_error`、`can_start`。
  前端不得再自行推断「开/关」。
- `MulticallTool.installed` 反映 `locate_sidecar` / 缓存里 `rb` 是否真的可执行（复用现有
  `busybox_bundled` 相关判定），不是用户选择。
- `SkillState.central` 的 `conflict` 必须来自 `generative_ui.rs` 已有的
  `GenerativeUiSkillInfo.conflict`，不得伪造。

`category` 取值需要把 `multicall` 从 `binary` 里拆出来（`rb` 是 multicall，`ra` 是 binary）。
若担心破坏 `component_setting` 键，可保留内部 `CATEGORY_BINARY` 作为存储分类，
仅在 `ComponentInfo` 上按 `def.tools.is_empty()` 派生 `binary`/`multicall`。

---

## 3. 动作模型（后端）

新增类型化命令，替换 `components_set_enabled`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComponentsAction {
    /// 单用途/多调用二进制的整体用户开关（写 component_setting）。
    BinaryEnabled { id: String, enabled: bool },
    /// multicall 内单个 applet 的开关（写 component_setting，按 applet id）。
    ToolEnabled { id: String, tool: String, enabled: bool },
    /// rc-mcp 单个 tool group 开关 → `set_rc_mcp_tool_group_core`。
    McpToolGroup { group: String, enabled: bool },
    /// 启动 rc-mcp 服务 → `start_rc_mcp_service_core`，**必须传播失败**。
    McpStartService,
    /// 技能：把中心副本链接进某 agent / 从某 agent 解链。
    SkillLink { agent: String, linked: bool },
    /// 从打包内容刷新：binary(ra) 重探版本；multicall(rb) 下载；mcp 重绑 broker；skill 重写中心副本。
    Refresh { id: String, task_id: Option<String> },
}
```

新命令：`components_apply(action: ComponentsAction) -> Result<ComponentsListResult>`，
返回**应用后的完整列表**（前端一次刷新即可，避免竞态）。

逐 kind 的真实落点
- `BinaryEnabled` / `ToolEnabled`：`component_setting_service::set_enabled`（仅这两类写库）。
- `McpToolGroup`：`commands::mcp_service::set_rc_mcp_tool_group_core`。
- `McpStartService`：`commands::mcp_service::start_rc_mcp_service_core`。
  **禁止吞错**——错误必须原样上抛，命令返回 `Err`，列表里的 `state.mcp.last_error` 反映它。
- `SkillLink`：`generative_ui.rs` 里逐 agent 的 link/unlink（复用现有函数，不新造）。
- `Refresh`：按 category 分派到现有 refresh 路径。

约束
- 不再存在「一个开关切九组」。任何 `Mcp*` 动作只影响它明说的那一组/那一个动作。
- 关闭 rc-mcp 不得触碰与它无关的功能开关（browser / computer / authoring / tasks 等）。
  这些开关归各自的机制，rc-mcp 面板只显示 `requires` 关系，不代为写入。

---

## 4. 真相来源（component_setting）

- `all_setting_ids()` 只保留 binary / multicall 的 id（含每个 applet id）。
- 从 `all_setting_ids()` 移除 `@rc-mcp` 与 `@ui-render-skill`（及 skill 的旧名字）。
- 迁移：启动时清理这两类的旧 `component_setting` 行（一次性、幂等）。
- `components_list_core` 对 mcp/skill **不再读** `component_setting.enabled`，
  一律以机制状态为准。
- 「选择」与「实际状态」不一致时才留 `tracing::debug!`；对 mcp/skill 不再存在这种分歧。

---

## 5. 前端

`src/components/settings/components-settings.tsx` + `src/lib/types.ts` + `src/lib/api.ts`
（transport 命令名同步新增 `components_apply`）。

- `ComponentInfo` 类型改为联合：`state: { kind: "binary" | "multicall" | "mcp" | "skill", ... }`。
- 详情面板按 `state.kind` 分派：
  - `binary`：单个开关 + 版本/更新。
  - `multicall`：二进制整体开关 + 每个 applet 一个 `Switch`（逐 applet 写 `ToolEnabled`）。
  - `mcp`：**直接复用 `RcMcpServiceStatus` 的 4 态**（`status-bar-mcp.tsx` 已有配色与文案，
    抽成共享组件或复用其渲染），渲染 `tool_groups` 的逐组开关 + `requires` 提示，
    `can_start` 为真时才显示「启动」。**不得**再显示一个笼统的 `enabled` 布尔。
  - `skill`：`central` 三态（present/missing/conflict）+ 逐 agent 链接矩阵（勾选即 `SkillLink`）+
    `conflict` 时明确提示「用户自有同名技能，未覆盖，可只做链接或改名后安装」。
- 所有写操作走 `componentsApply(action)`，用返回的列表整体替换本地 state。
- i18n：新增/更新文案要覆盖全部 10 个语言文件；`toolsDescription` 现有措辞需与
  「保留 PATH shim」的现状一致（此前一次改动把它写成「invoked at its fixed path」，
  若保留 shim 应改回描述 PATH 的语义，或改成中性措辞）。

---

## 6. 硬约束（防复发，写进代理 spec）

1. **只允许改** `src-tauri/src/commands/components.rs`。
2. `generative_ui.rs` **只允许纯新增**（暴露已有的链接/冲突查询），不得改动既有行为。
3. 需要新命令时，只允许改：`src-tauri/src/web/handlers/components.rs`、`src-tauri/src/web/router.rs`、
   `src-tauri/src/lib.rs`（注册）以及前端 `src/lib/api.ts` / `src/lib/types.ts`。
4. **禁止**触碰 `src-tauri/src/acp/connection.rs`、`src-tauri/src/acp/binary_cache.rs`、
   `src-tauri/src/commands/acp.rs`、`src-tauri/src/web/handlers/acp.rs`。
   —— 上一轮事故正是因为把这些文件连同重构一起改了。
5. binary / multicall 的行为与现有测试**不得改变**（字段可搬家，语义与断言保持）。
6. 每个提交必须带 gate（见 §8）；任一 gate 失败即回滚该步，不得继续。
7. 遵守提交规范：约定式提交、单行英文、无 body、颗粒度尽量细。

---

## 7. 分步实施（每步一个原子提交）

1. **数据模型**：新增 `ComponentState` 联合 + 从现有扁平字段装配；`ComponentInfo` 携带 `state`，
   旧字段暂时保留为 `#[serde(skip)]` 或直接删除并在同一步更新前端类型。gate：cargo check + 前端 typecheck。
2. **动作模型**：新增 `components_apply` 命令 + `ComponentsAction`，内部按 kind 落真实函数；
   保留旧 `components_set_enabled` 直到前端迁移完。gate：cargo test（新增单测：McpStartService 传播失败、
   McpToolGroup 只动一组、SkillLink 只动一个 agent）。
3. **真相来源**：`all_setting_ids()` 去除 mcp/skill + 幂等清理旧行。gate：components 测试全绿。
4. **前端**：按 `state.kind` 分派；MCP 面板复用 4 态；skill 矩阵。gate：`pnpm lint` + 相关 vitest。
5. **移除旧 API**：删掉 `components_set_enabled` 及其 transport 路由与前端调用。gate：全量检查。

---

## 8. 验收标准与检查命令

必须全部通过：

```bash
# Rust
cd src-tauri
cargo check
cargo test --features test-utils
cargo clippy --all-targets --features test-utils -- -D warnings
# 服务器模式
cargo check --no-default-features --features server-bin --bin rc-server

# 前端
pnpm lint .
pnpm test
pnpm build
```

功能验收（对抗审查逐条验证）
- [ ] `@rc-mcp` 在 `Stopped` / `Unavailable` 时**不再**显示为「开」。
- [ ] `McpStartService` 在 broker 启动失败时返回 `Err`，且 `state.mcp.last_error` 有值。
- [ ] 关闭 rc-mcp 不再改动任何无关功能开关；重开后原状态可恢复。
- [ ] 技能 `conflict` 在前端可见，且不会再显示为「可安装」。
- [ ] `rb` 的 15 个 applet 仍可经 PATH shim 调用（回归 blocker）。
- [ ] binary / multicall 的既有测试未改语义且全绿。

---

## 9. 附带待决事项（本次不擅改，仅记录）

1. **Cranelift**：`RecurCode/.cargo/config.toml`（本地、未跟踪）把 dev profile 设为
   `codegen-backend = "cranelift"`。观测到同一断言失败在官方 LLVM 下是正常失败，
   在 Cranelift 下变成进程硬崩（`0xc0000409`），会**把真实测试失败伪装成基础设施崩溃**。
   建议：RecurCode 本地关闭（注释掉即可，可逆）；RecurBusyBox 侧保留（那边已验证正常）。
   该文件未被跟踪，改动不影响 CI。**待操作者确认后再改。**
2. **PATH shim 方向**：本 spec 按「保留」执行；若要恢复更早的「移除 shim」方案，
   需显式说明，届时 §5 i18n 与 §6 约束同步调整。

---

## 10. 审查清单（对抗审查代理）

以「证伪」为目标逐条检查，任一不能证伪即判为真：
1. 是否存在任何路径仍把 mcp/skill 的「用户选择」当真相？（§4 应为唯一真相）
2. `McpStartService` 的失败是否可能在任意分支被 `let _ =` 吞掉？
3. 关闭 rc-mcp 是否会间接触碰无关开关（browser/computer/authoring/tasks）？
4. `rb` 的 PATH shim 是否在改动后仍完整（15 个 applet 全部可达）？
5. binary/multicall 的既有测试断言是否被悄悄放宽？
6. 是否改动了 §6 明令禁止的文件？
