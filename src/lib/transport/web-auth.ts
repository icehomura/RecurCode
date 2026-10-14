// Shared helpers for web-mode HTTP calls — the JSON transport in
// `web-transport.ts` and direct multipart/file callers in `lib/api.ts` both
// need consistent token retrieval and 401 redirect behavior. Keeping them in
// one place means a future move from `localStorage` to cookies (or rotation
// rules, multi-tenant prefixing, etc.) doesn't have to be remembered at every
// call site.

const TOKEN_KEY = "recur_code_token"

export function getRecurCodeToken(): string {
  return localStorage.getItem(TOKEN_KEY) ?? ""
}

export function redirectToRecurCodeLogin(): void {
  if (window.location.pathname.startsWith("/login")) return
  localStorage.removeItem(TOKEN_KEY)
  window.location.href = "/login"
}
