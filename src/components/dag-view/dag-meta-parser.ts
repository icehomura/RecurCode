/**
 * 从 ACP `tool_call(_update).meta` 里取出 `ra.dev/dag` 命名空间的消息。
 *
 * `meta` 是开放字典，`meta["ra.dev/dag"]` 是本协议的消息体（单条）。
 * 解析分两层：
 *   1. `extractDagMessage` —— 从 meta 字典取 key，再按 `ra.dag.*.v1` schema 派发；
 *   2. `parseDagMessage`   —— 对裸消息体做 schema 判定 + 强类型收窄。
 *
 * 非 `ra.dag.` 前缀、未知 schema、字段不合法一律返回 null（静默忽略，
 * 因为 meta 是多命名空间共享字典，别的键不该让本模块报错）。
 */
import {
  RA_DAG_META_KEY,
  RA_DAG_NODE_OUTPUT_SCHEMA,
  RA_DAG_NODE_STATE_SCHEMA,
  RA_DAG_SCHEMA_PREFIX,
  RA_DAG_TOPOLOGY_SCHEMA,
  normalizeNodeOutputMessage,
  normalizeNodeStateMessage,
  normalizeTopologyMessage,
  type RaDagMessage,
} from "./dag-model"

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  return value as Record<string, unknown>
}

/**
 * 判定一个消息体是否属于本协议（`ra.dag.` 前缀 + 已知 schema）。
 * 只看前缀的宽松判定给日志/过滤用；真正解析仍走 `parseDagMessage`。
 */
export function isRaDagSchema(schema: unknown): boolean {
  return typeof schema === "string" && schema.startsWith(RA_DAG_SCHEMA_PREFIX)
}

/**
 * 解析一条裸的 `ra.dev/dag` 消息体。未知/非法返回 `null`。
 */
export function parseDagMessage(raw: unknown): RaDagMessage | null {
  const obj = asRecord(raw)
  if (!obj) return null
  const schema = obj.schema
  if (typeof schema !== "string" || !schema.startsWith(RA_DAG_SCHEMA_PREFIX)) {
    return null
  }
  switch (schema) {
    case RA_DAG_TOPOLOGY_SCHEMA:
      return normalizeTopologyMessage(obj)
    case RA_DAG_NODE_STATE_SCHEMA:
      return normalizeNodeStateMessage(obj)
    case RA_DAG_NODE_OUTPUT_SCHEMA:
      return normalizeNodeOutputMessage(obj)
    default:
      return null
  }
}

/**
 * 从 `tool_call_update.meta`（或 `tool_call.meta`）里取出并解析 DAG 消息。
 * `meta` 可能是 null / 非对象 / 不含该 key —— 全部返回 null。
 */
export function extractDagMessage(meta: unknown): RaDagMessage | null {
  const obj = asRecord(meta)
  if (!obj) return null
  return parseDagMessage(obj[RA_DAG_META_KEY])
}
