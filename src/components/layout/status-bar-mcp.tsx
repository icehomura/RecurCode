"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { Plug, RotateCw, Settings2, Unplug } from "lucide-react"
import { useTranslations } from "next-intl"

import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import {
  getRcMcpServiceStatus,
  openSettingsWindow,
  setRcMcpToolGroup,
  startRcMcpService,
  type RcMcpServiceState,
  type RcMcpServiceStatus,
} from "@/lib/api"
import {
  AGENT_TOOLS_NAMESPACE,
  AGENT_TOOL_GROUPS,
} from "@/lib/agent-tool-groups"
import { toErrorMessage } from "@/lib/app-error"
import { cn } from "@/lib/utils"

/** Background refresh cadence. The status is a real socket round-trip plus a
 * binary lookup, so this is deliberately slow — the popover refetches on open,
 * which covers every moment someone is actually looking at it. */
const POLL_MS = 60_000

/** `unknown` is frontend-only: the status call itself failed, which says
 * nothing about the service and must not be painted as a service fault. */
type IndicatorState = RcMcpServiceState | "unknown"

/** Badge tint per state. `disabled` and `unknown` stay neutral on purpose: the
 * service is not faulty in either case, and colouring them would train people
 * to ignore the badge when it does go red. */
const BADGE_TONE: Record<IndicatorState, string> = {
  running: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
  stopped: "bg-red-500/10 text-red-600 dark:text-red-400",
  unavailable: "bg-yellow-500/10 text-yellow-700 dark:text-yellow-500",
  disabled: "bg-muted text-muted-foreground",
  unknown: "bg-muted text-muted-foreground",
}

const DOT_TONE: Record<IndicatorState, string> = {
  running: "bg-emerald-500",
  stopped: "bg-red-500",
  unavailable: "bg-yellow-500",
  disabled: "bg-muted-foreground/50",
  unknown: "bg-muted-foreground/50",
}

/** One cell of the stat strip: value on top, label under it. */
function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex min-w-0 flex-col items-center gap-0.5 px-1 text-center [&+&]:border-l">
      <span className="w-full truncate text-sm leading-none font-semibold tabular-nums">
        {value}
      </span>
      <span className="text-3xs leading-tight text-muted-foreground">
        {label}
      </span>
    </div>
  )
}

/**
 * Props shared by every rc-mcp surface. Two callers feed it today: the
 * popover in the bottom-right of the workspace (live status, polls the
 * backend), and the Components settings detail pane (already-fresh data from
 * `components_list`). Both need the same 4-state color/label mapping and the
 * same "write one group at a time" rule, so the chart lives in one place and
 * the data fetch lives at each call site.
 */
export interface RcMcpPanelProps {
  /** The current status snapshot; `null` while the caller is still loading. */
  status: RcMcpServiceStatus | null
  /** Loading the status failed. Distinct from the service being down. */
  loadError?: string | null
  /** Re-fetch the status. Called when the user clicks the refresh icon. */
  onRefresh: () => void
  /** Toggle one tool group. Receives the backend `key`, not the UI label. */
  onToggleGroup: (key: string, next: boolean) => Promise<void> | void
  /** Group keys whose in-flight write should keep their switch disabled. */
  pendingGroups: Record<string, boolean>
  /** Try to (re)bind the broker socket. */
  onStart: () => Promise<void> | void
  /** A start is currently in flight; disables the button. */
  starting: boolean
  /** The most recent action error, if any — outranks stale `last_error`. */
  actionError?: string | null
  /** Optional handler for the "open full settings" link. Omit for surfaces
   *  that ARE the full settings surface. */
  onOpenFullSettings?: () => void
}

/**
 * The shared rc-mcp renderer: 4-state badge, stat strip, per-group switches,
 * start button when the broker is repairable, and an "open full settings"
 * link that callers may hide. Additive: it does the same job the
 * `StatusBarMcp` popover always did, but with the data fetch hoisted out so
 * the Components settings pane can drive it from `componentsList`.
 */
export function RcMcpPanel({
  status,
  loadError,
  onRefresh,
  onToggleGroup,
  pendingGroups,
  onStart,
  starting,
  actionError,
  onOpenFullSettings,
}: RcMcpPanelProps) {
  // Translations live on the rc-mcp namespace (`Folder.statusBar.mcp`) and
  // the tools catalog (`AgentTools`). The component owns its own translators
  // rather than having callers thread the functions through, since every
  // surface already calls into them.
  const t = useTranslations("Folder.statusBar.mcp")
  const tools = useTranslations(AGENT_TOOLS_NAMESPACE)

  const state: IndicatorState = loadError
    ? "unknown"
    : (status?.state ?? "unknown")
  // Only the socket is repairable from here; a missing binary needs a
  // reinstall and switched-off groups are the switches below.
  const canStart = state === "stopped" && !!status?.can_start

  const pathsTitle = status
    ? [
        `${t("socketPath")}: ${status.socket_path || "—"}`,
        `${t("binary")}: ${status.binary_path ?? t("binaryMissing")}`,
      ].join("\n")
    : undefined

  const errorBanner = actionError
    ? t("actionFailed", { message: actionError })
    : !loadError && status?.last_error
      ? t("lastError", { message: status.last_error })
      : null

  return (
    <div className="w-full gap-2 space-y-2">
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-xs font-medium">{t("title")}</span>
        <div className="flex shrink-0 items-center gap-1.5">
          <span
            title={pathsTitle}
            className={cn(
              "flex items-center gap-1 rounded-full px-1.5 py-0.5 text-3xs font-medium",
              BADGE_TONE[state]
            )}
          >
            <span
              aria-hidden="true"
              className={cn("size-1.5 rounded-full", DOT_TONE[state])}
            />
            {t(`state.${state}`)}
          </span>
          <button
            type="button"
            onClick={() => void onRefresh()}
            title={t("refresh")}
            aria-label={t("refresh")}
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <RotateCw className="h-3 w-3" />
          </button>
        </div>
      </div>

      <p className="truncate text-2xs text-muted-foreground">
        {loadError
          ? t("loadFailed", { message: loadError })
          : t(`hint.${state}`)}
      </p>

      {status && !loadError && (
        // No status column: `stopped` is exactly `not listening`, so the
        // badge above already carries that reading. These three are the ones
        // it does not.
        <div className="grid grid-cols-3 rounded-lg border bg-muted/30 py-2">
          <Stat label={t("sessions")} value={String(status.session_count)} />
          <Stat
            label={t("activeDelegations")}
            value={String(status.active_delegations)}
          />
          <Stat label={t("depthLimit")} value={String(status.depth_limit)} />
        </div>
      )}

      {status && !loadError && (
        // One bordered, divided block rather than six floating rows: the
        // switches are a single control surface and read as one.
        <div className="divide-y overflow-hidden rounded-lg border">
          {status.tool_groups.map((group) => {
            const meta = AGENT_TOOL_GROUPS[group.key]
            const label = meta ? tools(meta.label) : group.key
            const Icon = meta?.icon ?? Plug
            // A switch that lives inside another is shown off and left
            // untouchable until its parent is on — the same treatment the
            // settings panel gives it. Turning it on from here while the
            // group is off would write a `true` that nothing acts on, and
            // show an agent a tool it cannot call. Not indented for it,
            // though: the settings list draws its rows on one rail, and a
            // single stepped-in row here read as a misaligned one rather
            // than as a nested one. Being off and untouchable while the
            // group is off already says where it belongs.
            const parent = group.requires
            const available =
              !parent ||
              (pendingGroups[parent] ??
                status.tool_groups.find((g) => g.key === parent)?.enabled ??
                false)
            const checked =
              available && (pendingGroups[group.key] ?? group.enabled)
            return (
              <label
                key={group.key}
                className={cn(
                  "flex items-center gap-2 px-2 py-1.5 transition-colors",
                  available
                    ? "cursor-pointer hover:bg-accent/40"
                    : "cursor-not-allowed opacity-55"
                )}
              >
                {/* One tile tint for every row — the theme's accent, so the
                    list follows whatever `[data-theme]` is on. An unmapped
                    slug gets the same tile, keeping the column even. */}
                <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                  <Icon className="size-3.5" />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-2xs font-medium">
                    {label}
                  </span>
                  {meta && (
                    <span className="block text-3xs leading-snug text-muted-foreground">
                      {tools(meta.short)}
                    </span>
                  )}
                </span>
                <Switch
                  checked={checked}
                  disabled={!available || group.key in pendingGroups}
                  aria-label={label}
                  onCheckedChange={(next) =>
                    void onToggleGroup(group.key, next)
                  }
                />
              </label>
            )
          })}
        </div>
      )}

      {errorBanner && (
        <div className="rounded-md border border-red-500/30 bg-red-500/5 px-2 py-1.5 text-2xs break-words text-red-400">
          {errorBanner}
        </div>
      )}

      {canStart && (
        <Button
          size="sm"
          className="w-full"
          onClick={() => void onStart()}
          disabled={starting}
        >
          <Plug className="h-3.5 w-3.5" />
          {starting ? t("starting") : t("start")}
        </Button>
      )}
      {onOpenFullSettings && (
        <Button
          size="sm"
          variant="outline"
          className="w-full"
          onClick={onOpenFullSettings}
        >
          <Settings2 className="h-3.5 w-3.5" />
          {t("openSettings")}
        </Button>
      )}
    </div>
  )
}

/**
 * rc-mcp service indicator, bottom-right of the workspace.
 *
 * The companion is what carries RecurCode's own tools into an agent session —
 * delegation, live feedback, ask-user-question, session lookup, the two
 * create-from-chat writers. Its three failure modes are all silent from the
 * workspace: a missing companion binary logs one line at spawn time, a dead
 * broker socket logs nothing, and "every tool group switched off" looks
 * identical to both from inside a conversation.
 *
 * So: a badge that says which of those is true, a strip of live counts, a
 * switch per tool group, and — for the one failure this process can repair —
 * a button that rebinds the socket without an app restart.
 */
export function StatusBarMcp() {
  const t = useTranslations("Folder.statusBar.mcp")
  const [open, setOpen] = useState(false)
  const [status, setStatus] = useState<RcMcpServiceStatus | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [starting, setStarting] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)
  /** Group key → the value being written, held only while the write is in
   * flight. Without it the switch would snap back to the stale `status` for
   * the length of the round trip. */
  const [pending, setPending] = useState<Record<string, boolean>>({})
  // Guards against a slow in-flight refresh landing after unmount, and against
  // the 60s tick overwriting a fresher result the popover just fetched.
  const seqRef = useRef(0)
  const aliveRef = useRef(true)

  const refresh = useCallback(async () => {
    const seq = ++seqRef.current
    try {
      const next = await getRcMcpServiceStatus()
      if (!aliveRef.current || seq !== seqRef.current) return
      setStatus(next)
      setLoadError(null)
    } catch (e) {
      if (!aliveRef.current || seq !== seqRef.current) return
      setLoadError(toErrorMessage(e))
    }
  }, [])

  useEffect(() => {
    aliveRef.current = true
    void refresh()
    const id = setInterval(() => void refresh(), POLL_MS)
    return () => {
      aliveRef.current = false
      clearInterval(id)
    }
  }, [refresh])

  const handleOpenChange = (next: boolean) => {
    setOpen(next)
    if (next) {
      setActionError(null)
      void refresh()
    }
  }

  const handleStart = async () => {
    setStarting(true)
    setActionError(null)
    try {
      await startRcMcpService()
      await refresh()
    } catch (e) {
      if (aliveRef.current) setActionError(toErrorMessage(e))
    } finally {
      if (aliveRef.current) setStarting(false)
    }
  }

  const handleToggle = async (key: string, next: boolean) => {
    setPending((prev) => ({ ...prev, [key]: next }))
    setActionError(null)
    try {
      await setRcMcpToolGroup(key, next)
      // Re-read before releasing the optimistic value, so the switch hands
      // over to a `status` that already reflects the write.
      await refresh()
    } catch (e) {
      // No refresh on failure: `status` still holds the pre-toggle truth, so
      // dropping the override below snaps the switch back to it.
      if (aliveRef.current) setActionError(toErrorMessage(e))
    } finally {
      if (!aliveRef.current) return
      setPending((prev) => {
        // Delete rather than write `undefined`: the switch row reads
        // `key in pending` to stay disabled, so the key has to actually go.
        const next = { ...prev }
        delete next[key]
        return next
      })
    }
  }

  const state: IndicatorState = loadError
    ? "unknown"
    : (status?.state ?? "unknown")
  const down = state === "stopped" || state === "unavailable"

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        {/* Deliberately as plain as every other glyph on the bar: no tone, no
            count. The state is carried by the icon's shape (a pulled plug
            reads at a glance and stays monochrome), the tooltip, and the
            popover — a permanently coloured, permanently numbered indicator in
            the corner is noise the other 95% of the time. */}
        <button
          aria-label={t("title")}
          title={`${t("title")} · ${t(`state.${state}`)}`}
          className="flex items-center transition-colors hover:text-foreground"
        >
          {down ? (
            <Unplug className="size-3.5" />
          ) : (
            <Plug className="size-3.5" />
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent side="top" align="end" className="w-88 p-2.5">
        <RcMcpPanel
          status={status}
          loadError={loadError}
          onRefresh={() => void refresh()}
          onToggleGroup={handleToggle}
          pendingGroups={pending}
          onStart={handleStart}
          starting={starting}
          actionError={actionError}
          onOpenFullSettings={() => {
            // The Collaboration page is where these same switches live in
            // full, with the depth limit and the per-agent defaults beside
            // them.
            openSettingsWindow("collaboration").catch((err) => {
              console.error("[StatusBarMcp] failed to open settings:", err)
            })
          }}
        />
      </PopoverContent>
    </Popover>
  )
}
