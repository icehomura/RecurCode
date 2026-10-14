import { describe, expect, it } from "vitest"

import { parseCodexScriptMeta } from "./codex-code-mode"

/**
 * `ToolUse.meta` is shared with other conventions (the delegation lifecycle
 * writes `RecurCode.delegation` to the same object), and it survives a DB round
 * trip, so it may arrive as anything JSON can hold.
 */
describe("parseCodexScriptMeta", () => {
  it("reads the marks the parser writes for a fanned-out command", () => {
    expect(
      parseCodexScriptMeta({
        "RecurCode.codexScript": {
          label: "formula-service",
          sharedWith: ["vo", "formula-splice"],
          truncated: true,
        },
      })
    ).toEqual({
      label: "formula-service",
      outputMissing: false,
      sharedWith: ["vo", "formula-splice"],
      truncated: true,
    })
  })

  it("defaults every mark that is absent", () => {
    expect(parseCodexScriptMeta({ "RecurCode.codexScript": {} })).toEqual({
      label: null,
      outputMissing: false,
      sharedWith: [],
      truncated: false,
    })
  })

  it("ignores meta belonging to another convention", () => {
    expect(
      parseCodexScriptMeta({ "RecurCode.delegation": { status: "done" } })
    ).toBeNull()
  })

  it("returns null rather than throwing on anything else", () => {
    for (const value of [null, undefined, "text", 7, [], { a: 1 }]) {
      expect(parseCodexScriptMeta(value)).toBeNull()
    }
  })

  it("drops non-string entries instead of rendering them", () => {
    const meta = parseCodexScriptMeta({
      "RecurCode.codexScript": { label: 12, sharedWith: ["vo", 3, null] },
    })
    expect(meta?.label).toBeNull()
    expect(meta?.sharedWith).toEqual(["vo"])
  })
})
