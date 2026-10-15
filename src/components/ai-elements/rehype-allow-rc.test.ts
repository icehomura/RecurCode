import { describe, expect, it } from "vitest"
import { defaultRehypePlugins } from "streamdown"

import { rehypePluginsAllowingRecurCode } from "./rehype-allow-rc"

/** Pull the href protocol allow-list out of a `[rehypeSanitize, schema]` tuple. */
function hrefProtocols(plugin: unknown): string[] | undefined {
  if (!Array.isArray(plugin)) return undefined
  const schema = plugin[1] as { protocols?: { href?: string[] } } | undefined
  return schema?.protocols?.href
}

/** Pull the `span` attribute allow-list out of the same tuple. */
function spanAttributes(plugin: unknown): unknown[] | undefined {
  if (!Array.isArray(plugin)) return undefined
  const schema = plugin[1] as { attributes?: { span?: unknown[] } } | undefined
  return schema?.attributes?.span
}

describe("rehypePluginsAllowingRecurCode", () => {
  it("adds `RecurCode` to the sanitize schema's href protocol allow-list", () => {
    // Guards against an upstream rename of the `sanitize` key — the whole fix
    // hinges on this entry existing.
    const sanitizeIndex = Object.keys(defaultRehypePlugins).indexOf("sanitize")
    expect(sanitizeIndex).toBeGreaterThanOrEqual(0)

    const href = hrefProtocols(
      rehypePluginsAllowingRecurCode(defaultRehypePlugins)[sanitizeIndex]
    )
    expect(href).toContain("RecurCode")
    // Exactly once — no duplicate even if re-derived.
    expect(href?.filter((p) => p === "RecurCode")).toHaveLength(1)
    // Pre-existing protocols are preserved (https is always present).
    expect(href).toContain("https")
  })

  it("allows the local-image span attributes under both hast spellings", () => {
    const sanitizeIndex = Object.keys(defaultRehypePlugins).indexOf("sanitize")
    const span = spanAttributes(
      rehypePluginsAllowingRecurCode(defaultRehypePlugins)[sanitizeIndex]
    )
    // The camelCased names are what sanitize sees while `rehype-raw` runs
    // ahead of it; the dashed ones are what it would see without that
    // round-trip. Neither spelling alone is safe to rely on across upgrades.
    expect(span).toEqual(
      expect.arrayContaining([
        "dataRcLocalImage",
        "dataRcImageLinked",
        "data-rc-local-image",
        "data-rc-image-linked",
      ])
    )
    // The shipped schema's own `span` entries (if any) are preserved.
    for (const attribute of spanAttributes(defaultRehypePlugins.sanitize) ??
      []) {
      expect(span).toContainEqual(attribute)
    }
  })

  it("preserves plugin count and order, passing raw/harden through by reference", () => {
    const keys = Object.keys(defaultRehypePlugins)
    const result = rehypePluginsAllowingRecurCode(defaultRehypePlugins)
    expect(result).toHaveLength(keys.length)
    keys.forEach((key, i) => {
      if (key !== "sanitize") {
        expect(result[i]).toBe(defaultRehypePlugins[key])
      }
    })
  })

  it("clones rather than mutating the shipped sanitize schema", () => {
    // The shipped default must not already contain RecurCode, else the fix is moot.
    expect(hrefProtocols(defaultRehypePlugins.sanitize)).not.toContain("RecurCode")
    rehypePluginsAllowingRecurCode(defaultRehypePlugins)
    // Still absent on the original after deriving — we built a new schema.
    expect(hrefProtocols(defaultRehypePlugins.sanitize)).not.toContain("RecurCode")
  })
})
