export const RC_WS_PROTOCOL = "rc-events"
const RC_WS_TOKEN_PROTOCOL_PREFIX = "rc-token."

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

export function buildRcWebSocketProtocols(token: string): string[] {
  const trimmed = token.trim()
  if (!trimmed) return [RC_WS_PROTOCOL]
  return [
    RC_WS_PROTOCOL,
    `${RC_WS_TOKEN_PROTOCOL_PREFIX}${base64UrlEncode(trimmed)}`,
  ]
}
