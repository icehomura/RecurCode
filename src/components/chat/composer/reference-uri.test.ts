import { describe, expect, it } from "vitest"

import {
  buildEmbeddedReferenceUri,
  embeddedReferenceUriFor,
  isEmbeddedReferenceUri,
  parseRcReferenceUri,
  refOfEmbeddedReferenceUri,
} from "./reference-uri"

describe("parseRcReferenceUri", () => {
  it("returns null for non-reference schemes", () => {
    expect(parseRcReferenceUri("https://example.com", "x")).toBeNull()
    expect(parseRcReferenceUri("data:text/plain,abc", "x")).toBeNull()
    expect(parseRcReferenceUri("rc://unknown/1", "x")).toBeNull()
  })

  it("parses a file uri, falling back to the basename when label is empty", () => {
    expect(
      parseRcReferenceUri("file:///repo/deep/name.ts", "")
    ).toMatchObject({
      refType: "file",
      id: "name.ts",
      label: "name.ts",
      uri: "file:///repo/deep/name.ts",
      meta: { fileKind: "file" },
    })
  })

  it("parses an agent uri, stripping a leading @ from the label", () => {
    expect(
      parseRcReferenceUri("rc://agent/codex", "@Codex")
    ).toMatchObject({
      refType: "agent",
      id: "codex",
      label: "Codex",
      uri: "rc://agent/codex",
      meta: { agentType: "codex" },
    })
  })

  it("falls back to the agent type when the agent label is empty", () => {
    expect(
      parseRcReferenceUri("rc://agent/claude_code", "")
    ).toMatchObject({
      refType: "agent",
      id: "claude_code",
      label: "claude_code",
      meta: { agentType: "claude_code" },
    })
  })

  it("parses a new-format session uri, recovering the agent type", () => {
    expect(
      parseRcReferenceUri("rc://session/codex_abc123", "My chat")
    ).toMatchObject({
      refType: "session",
      id: "codex_abc123",
      label: "My chat",
      uri: "rc://session/codex_abc123",
      meta: { agentType: "codex" },
    })
  })

  it("never splits an agent type on its first underscore", () => {
    // claude_code / open_code / open_claw contain underscores; a naive first-`_`
    // split would yield "claude" / "open". The whole `<type>_<external_id>` is
    // the id and the full type is recovered by prefix match.
    expect(
      parseRcReferenceUri("rc://session/claude_code_sess-9", "")
    ).toMatchObject({
      id: "claude_code_sess-9",
      meta: { agentType: "claude_code" },
    })
    expect(
      parseRcReferenceUri("rc://session/open_code_x", "")?.meta
    ).toEqual({ agentType: "open_code" })
    expect(
      parseRcReferenceUri("rc://session/open_claw_y", "")?.meta
    ).toEqual({ agentType: "open_claw" })
  })

  it("treats a legacy numeric session id as opaque (no agent icon)", () => {
    expect(
      parseRcReferenceUri("rc://session/123", "Login")
    ).toMatchObject({
      refType: "session",
      id: "123",
      label: "Login",
      uri: "rc://session/123",
      meta: null,
    })
  })

  it("treats a non-agent-prefixed token as a plain session id", () => {
    expect(
      parseRcReferenceUri("rc://session/randomtoken", "")
    ).toMatchObject({ refType: "session", id: "randomtoken", meta: null })
  })

  it("falls back to #id for an empty session label", () => {
    expect(parseRcReferenceUri("rc://session/123", "")?.label).toBe(
      "#123"
    )
  })

  it("parses a commit uri, deriving the short hash", () => {
    expect(
      parseRcReferenceUri("rc://commit/%2Frepo@abc1234def5678", "abc1234")
    ).toMatchObject({
      refType: "commit",
      id: "abc1234def5678",
      label: "abc1234",
      uri: "rc://commit/%2Frepo@abc1234def5678",
      meta: { shortHash: "abc1234" },
    })
  })

  it("parses a skill uri, moving the label's leading `/`·`$` into the prefix", () => {
    expect(
      parseRcReferenceUri("rc://skill/review", "/review")
    ).toMatchObject({
      refType: "skill",
      id: "review",
      label: "review",
      uri: "rc://skill/review",
      // The stripped trigger is kept so re-serializing the badge emits the
      // same `/review` token it was parsed from.
      meta: { invocationPrefix: "/" },
    })
    // The `$` prefix ($skill / Codex expert) is stripped — and kept — the same
    // way, so a `$deploy` token never re-serializes to `/deploy`.
    expect(
      parseRcReferenceUri("rc://skill/deploy", "$deploy")
    ).toMatchObject({
      label: "deploy",
      meta: { invocationPrefix: "$" },
    })
  })

  it("falls back to the bare id for an empty skill label", () => {
    expect(parseRcReferenceUri("rc://skill/deploy", "")).toMatchObject({
      label: "deploy",
      // No literal token to read a trigger from → serializer's `/` default.
      meta: null,
    })
  })

  it("parses an embedded-attachment uri as an inert file badge", () => {
    expect(
      parseRcReferenceUri("rc://embedded/9f3c-uuid", "report.pdf")
    ).toMatchObject({
      refType: "file",
      label: "report.pdf",
      uri: "rc://embedded/9f3c-uuid",
      meta: { fileKind: "file" },
    })
  })

  it("falls back to a generic label for an empty embedded-attachment label", () => {
    expect(
      parseRcReferenceUri("rc://embedded/9f3c-uuid", "")?.label
    ).toBe("resource")
  })

  it("recognizes a freshly minted embedded reference uri", () => {
    const uri = buildEmbeddedReferenceUri()
    expect(isEmbeddedReferenceUri(uri)).toBe(true)
    expect(isEmbeddedReferenceUri("file:///rc-embedded/real.ts")).toBe(false)
    expect(isEmbeddedReferenceUri("rc://session/abc")).toBe(false)
  })
})

describe("an embedded reference uri that carries its ref", () => {
  const page = "https://shop.test/a b/(1)?q=x&y=%20#top"

  it("gives back the ref it was minted with, whatever is in it", () => {
    const uri = buildEmbeddedReferenceUri(page)
    expect(isEmbeddedReferenceUri(uri)).toBe(true)
    expect(refOfEmbeddedReferenceUri(uri)).toBe(page)
  })

  // Two badges for one page each hold their own block until send, keyed by
  // their display uri — the same uri twice would hand both the second block.
  it("still mints a uri of its own for every badge", () => {
    const first = buildEmbeddedReferenceUri(page)
    const second = buildEmbeddedReferenceUri(page)
    expect(first).not.toBe(second)
    expect(first.startsWith(`${embeddedReferenceUriFor(page)}#`)).toBe(true)
    expect(second.startsWith(`${embeddedReferenceUriFor(page)}#`)).toBe(true)
  })

  // What the transcript rebuilds from an agent's record names it the same way.
  it("reads the ref out of the form a transcript rebuilds", () => {
    expect(refOfEmbeddedReferenceUri(embeddedReferenceUriFor(page))).toBe(page)
  })

  it("carries no ref for a badge minted without one", () => {
    expect(refOfEmbeddedReferenceUri(buildEmbeddedReferenceUri())).toBeNull()
    expect(refOfEmbeddedReferenceUri(buildEmbeddedReferenceUri(""))).toBeNull()
    expect(refOfEmbeddedReferenceUri("rc://embedded/9f3c-uuid")).toBeNull()
    expect(refOfEmbeddedReferenceUri("rc://embedded/%E0%A4%A")).toBeNull()
    expect(refOfEmbeddedReferenceUri("rc://session/abc")).toBeNull()
  })

  it("is still an inert file badge with the label it was given", () => {
    const uri = buildEmbeddedReferenceUri(page)
    expect(parseRcReferenceUri(uri, "Page screenshot")).toMatchObject({
      refType: "file",
      label: "Page screenshot",
      uri,
    })
  })
})
