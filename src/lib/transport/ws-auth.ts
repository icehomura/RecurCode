export const RECUR_CODE_WS_PROTOCOL = "recur-code-events"
const RECUR_CODE_WS_TOKEN_PROTOCOL_PREFIX = "recur-code-token."

function base64UrlEncode(value: string): string {
  const bytes = new TextEncoder().encode(value)
  let binary = ""
  for (const byte of bytes) {
    binary += String.fromCharCode(byte)
  }
  return btoa(binary)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/g, "")
}

export function buildRecurCodeWebSocketProtocols(token: string): string[] {
  const trimmed = token.trim()
  if (!trimmed) return [RECUR_CODE_WS_PROTOCOL]
  return [
    RECUR_CODE_WS_PROTOCOL,
    `${RECUR_CODE_WS_TOKEN_PROTOCOL_PREFIX}${base64UrlEncode(trimmed)}`,
  ]
}
