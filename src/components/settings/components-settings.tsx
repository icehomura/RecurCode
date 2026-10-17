"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { usePathname, useRouter, useSearchParams } from "next/navigation"
import { useTranslations } from "next-intl"
import {
  ArrowDownToLine,
  FolderOpen,
  Info,
  Loader2,
  Lock,
  Plus,
  RefreshCw,
  Terminal,
  Trash2,
} from "lucide-react"
import { toast } from "sonner"

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  SettingCard,
  SettingNote,
  SettingRow,
} from "@/components/shared/setting-card"
import { RcMcpPanel } from "@/components/layout/status-bar-mcp"
import { cn, randomUUID } from "@/lib/utils"
import { toErrorMessage } from "@/lib/app-error"
import {
  componentsApply,
  componentsList,
  componentsSetEnv,
  componentsUpdate,
  getSystemToolsSettings,
  setSystemToolsSettings,
  raProfileCreate,
  raProfileDelete,
  raProfileList,
  raProfileSetActive,
} from "@/lib/api"
import type {
  ComponentInfo,
  ComponentsAction,
  MulticallTool,
  RcMcpComponentState,
  RaProfileDetail,
  SkillState,
  SystemToolsSettings,
} from "@/lib/types"

const LEFT_MIN_WIDTH = 280
const RIGHT_MIN_WIDTH = 420

/** The four sub-tabs. `multicall` is a UI-only flavor of `binary` for the
 *  picker — categories that mirror the registry fill the rest. The profile
 *  manager is its own thing and never appears here. */
type SettingsTab = "binary" | "multicall" | "mcp" | "skill" | "profiles"

const TAB_ORDER: SettingsTab[] = [
  "binary",
  "multicall",
  "mcp",
  "skill",
  "profiles",
]

/** Sentinel for the profile-create "clone from" select: Radix forbids the
 *  empty string as an item value, so "current config" needs a real value. */
const PROFILE_CLONE_CURRENT = "__current__"

function normalizeTab(raw: string | null): SettingsTab {
  if (!raw) return "binary"
  return (TAB_ORDER as string[]).includes(raw)
    ? (raw as SettingsTab)
    : "binary"
}

// Radix emits `data-state="active"`, so the pill has to be re-declared — the
// Tabs primitive's own `data-active:` variant never matches (see
// skill-packs-settings.tsx, which documents the same workaround).
const TRIGGER_ACTIVE =
  "data-[state=active]:bg-background data-[state=active]:text-foreground data-[state=active]:shadow-sm"

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}

function toPercent(pixels: number, totalPixels: number): number {
  if (totalPixels <= 0) return 0
  return (pixels / totalPixels) * 100
}

/** The env editor works on an ordered row list so a key can be typed (and a
 *  row emptied) before it is a valid `Record` entry; it is folded back into the
 *  record only on save. */
interface EnvRow {
  key: string
  value: string
}

function envToRows(env: Record<string, string>): EnvRow[] {
  return Object.entries(env).map(([key, value]) => ({ key, value }))
}

function rowsToEnv(rows: EnvRow[]): Record<string, string> {
  const env: Record<string, string> = {}
  for (const row of rows) {
    const key = row.key.trim()
    if (!key) continue
    env[key] = row.value
  }
  return env
}

function rowsEqual(a: EnvRow[], b: EnvRow[]): boolean {
  if (a.length !== b.length) return false
  return a.every(
    (row, index) => row.key === b[index].key && row.value === b[index].value
  )
}

/** Flat view of the binary/multicall inputs the detail pane shows: every
 *  binary detail surfaces `tool` + version facts, and multicall extends with
 *  per-applet rows. Mapping a single-purpose binary onto the same shape
 *  means the JSX never branches on `state.kind` to read a version. */
interface BinaryLike {
  kind: "binary" | "multicall"
  tool: string
  installedVersion: string | null
  availableVersion: string | null
  updateAvailable: boolean
  /** Present only on multicall. */
  appletTools?: readonly MulticallTool[]
}

function asBinaryLike(component: ComponentInfo): BinaryLike | null {
  const state = component.state
  if (state.kind === "binary") {
    return {
      kind: "binary",
      tool: state.tool,
      installedVersion: state.installedVersion,
      availableVersion: state.availableVersion,
      updateAvailable: state.updateAvailable,
    }
  }
  if (state.kind === "multicall") {
    return {
      kind: "multicall",
      tool: state.binary.tool,
      installedVersion: state.binary.installedVersion,
      availableVersion: state.binary.availableVersion,
      updateAvailable: state.binary.updateAvailable,
      appletTools: state.tools,
    }
  }
  return null
}

function asMcpStatus(component: ComponentInfo): RcMcpComponentState | null {
  return component.state.kind === "mcp" ? component.state : null
}

function asSkill(component: ComponentInfo): SkillState | null {
  return component.state.kind === "skill" ? component.state : null
}

export function ComponentsSettings() {
  const t = useTranslations("ComponentsSettings")
  const router = useRouter()
  const pathname = usePathname()
  const searchParams = useSearchParams()

  // Local state is the visual source of truth so the tab always switches; the
  // `?tab=` param is a best-effort deep-link mirror, seeded once from the URL.
  const [tab, setTab] = useState<SettingsTab>(() =>
    normalizeTab(searchParams.get("tab"))
  )

  const [components, setComponents] = useState<ComponentInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [updating, setUpdating] = useState(false)
  const [envRows, setEnvRows] = useState<EnvRow[]>([])

  // Per-request state only: keeps mcp/skill switches and the multicall
  // applets from snapping back to stale state during the round trip.
  const [pendingMcpGroups, setPendingMcpGroups] = useState<
    Record<string, boolean>
  >({})
  const [mcpStarting, setMcpStarting] = useState(false)
  const [mcpActionError, setMcpActionError] = useState<string | null>(null)

  // The bundled-tools toggle lives on the RecurBusyBox detail panel.
  const [toolsSettings, setToolsSettings] =
    useState<SystemToolsSettings | null>(null)
  const [savingToolsSettings, setSavingToolsSettings] = useState(false)

  // Ra profiles (the fifth sub-tab). Loaded independently of the component
  // list so a profile backend failure can't blank the whole panel.
  const [profiles, setProfiles] = useState<RaProfileDetail[]>([])
  const [profilesError, setProfilesError] = useState<string | null>(null)
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(
    null
  )
  const [profileBusy, setProfileBusy] = useState(false)
  const [createOpen, setCreateOpen] = useState(false)
  const [createName, setCreateName] = useState("")
  const [cloneFrom, setCloneFrom] = useState(PROFILE_CLONE_CURRENT)
  const [deleteTarget, setDeleteTarget] = useState<RaProfileDetail | null>(null)
  const [deleting, setDeleting] = useState(false)

  const panelContainerRef = useRef<HTMLDivElement | null>(null)
  const [panelContainerWidth, setPanelContainerWidth] = useState(0)

  useEffect(() => {
    const element = panelContainerRef.current
    if (!element || typeof ResizeObserver === "undefined") return
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? 0
      setPanelContainerWidth(width)
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  // Apply an action and replace the entire local list with the backend's
  // freshly built one. Every typed action lands here so the UI is always in
  // sync with the mechanism without the read-after-write race of
  // `apply(); list();`.
  const dispatchAction = useCallback(
    async (action: ComponentsAction): Promise<ComponentInfo[] | null> => {
      try {
        const result = await componentsApply(action)
        setComponents(result.components)
        setLoadError(null)
        return result.components
      } catch (err) {
        toast.error(t("toasts.actionFailed"), {
          description: toErrorMessage(err),
        })
        return null
      }
    },
    [t]
  )

  const refresh = useCallback(async () => {
    try {
      const list = await componentsList()
      setComponents(list)
      setLoadError(null)
      return list
    } catch (err) {
      setLoadError(toErrorMessage(err))
      return null
    }
  }, [])

  const refreshProfiles = useCallback(async () => {
    try {
      const list = await raProfileList()
      setProfiles(list)
      setProfilesError(null)
      return list
    } catch (err) {
      setProfilesError(toErrorMessage(err))
      return null
    }
  }, [])

  useEffect(() => {
    void refreshProfiles()
  }, [refreshProfiles])

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const list = await componentsList()
        if (cancelled) return
        setComponents(list)
        setLoadError(null)
        const toolsSettingsValue = await getSystemToolsSettings()
        if (!cancelled) setToolsSettings(toolsSettingsValue)
      } catch (err) {
        if (cancelled) return
        setLoadError(toErrorMessage(err))
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  const inCategory = useMemo(
    () => components.filter((item) => item.category === tab),
    [components, tab]
  )

  const selected = useMemo(
    () => inCategory.find((item) => item.id === selectedId) ?? null,
    [inCategory, selectedId]
  )

  // `_main` is RecurAgent's internal profile and never appears in the list.
  const visibleProfiles = useMemo(
    () => profiles.filter((profile) => profile.id !== "_main"),
    [profiles]
  )

  const selectedProfile = useMemo(
    () =>
      visibleProfiles.find((profile) => profile.id === selectedProfileId) ??
      null,
    [visibleProfiles, selectedProfileId]
  )

  // Mirror the component-selection rule: a freshly loaded profile list (or a
  // deletion) picks the first remaining entry rather than leaving the detail
  // pane blank.
  useEffect(() => {
    if (visibleProfiles.length === 0) {
      if (selectedProfileId !== null) setSelectedProfileId(null)
      return
    }
    if (!visibleProfiles.some((profile) => profile.id === selectedProfileId)) {
      setSelectedProfileId(visibleProfiles[0].id)
    }
  }, [visibleProfiles, selectedProfileId])

  // Keep a selection per category: a fresh list (or a tab switch) picks the
  // first entry instead of leaving the detail pane empty.
  useEffect(() => {
    if (inCategory.length === 0) {
      if (selectedId !== null) setSelectedId(null)
      return
    }
    if (!inCategory.some((item) => item.id === selectedId)) {
      setSelectedId(inCategory[0].id)
    }
  }, [inCategory, selectedId])

  // The env editor reseeds from the selected component so it never shows the
  // previous component's rows.
  useEffect(() => {
    setEnvRows(envToRows(selected?.env ?? {}))
  }, [selected?.id, selected?.env])

  const handleTabChange = useCallback(
    (next: string) => {
      const value = normalizeTab(next)
      setTab(value)
      // Preserve every other param (esp. ?remoteConnectionId=N) when mirroring
      // the active tab into the URL.
      const params = new URLSearchParams(searchParams.toString())
      params.set("tab", value)
      router.replace(`${pathname}?${params.toString()}`)
    },
    [router, pathname, searchParams]
  )

  const handleBinaryToggle = useCallback(
    async (component: ComponentInfo, enabled: boolean) => {
      if (component.required) return
      setBusy(true)
      await dispatchAction({
        kind: "binary_enabled",
        id: component.id,
        enabled,
      })
      setBusy(false)
    },
    [dispatchAction]
  )

  const handleAppletToggle = useCallback(
    async (_component: ComponentInfo, tool: MulticallTool, enabled: boolean) => {
      setBusy(true)
      await dispatchAction({
        kind: "tool_enabled",
        id: tool.id,
        tool: tool.name,
        enabled,
      })
      setBusy(false)
    },
    [dispatchAction]
  )

  const handleMcpToggleGroup = useCallback(
    async (key: string, enabled: boolean) => {
      setPendingMcpGroups((prev) => ({ ...prev, [key]: enabled }))
      setMcpActionError(null)
      const next = await dispatchAction({
        kind: "mcp_tool_group",
        group: key,
        enabled,
      })
      setPendingMcpGroups((prev) => {
        const copy = { ...prev }
        delete copy[key]
        return copy
      })
      void next
    },
    [dispatchAction]
  )

  const handleMcpStartService = useCallback(async () => {
    setMcpStarting(true)
    setMcpActionError(null)
    await dispatchAction({ kind: "mcp_start_service" })
    setMcpStarting(false)
  }, [dispatchAction])

  const handleSkillLinkToggle = useCallback(
    async (_component: ComponentInfo, agent: string, linked: boolean) => {
      setBusy(true)
      await dispatchAction({
        kind: "skill_link",
        agent,
        linked,
      })
      setBusy(false)
    },
    [dispatchAction]
  )

  const handleSkillRefresh = useCallback(
    async (component: ComponentInfo) => {
      setBusy(true)
      const next = await dispatchAction({
        kind: "refresh",
        id: component.id,
        taskId: null,
      })
      setBusy(false)
      if (next) toast.success(t("toasts.skillRefreshed"))
    },
    [dispatchAction, t]
  )

  const handleUpdate = useCallback(
    async (component: ComponentInfo) => {
      setUpdating(true)
      try {
        // The backend streams agent-install progress events keyed by this id;
        // the panel only needs the completion, so it re-reads the list after.
        await componentsUpdate(component.id, randomUUID())
        await refresh()
        toast.success(t("toasts.updated", { name: component.name }))
      } catch (err) {
        toast.error(t("toasts.updateFailed", { name: component.name }), {
          description: toErrorMessage(err),
        })
        await refresh()
      } finally {
        setUpdating(false)
      }
    },
    [refresh, t]
  )

  const seedRows = useMemo(
    () => envToRows(selected?.env ?? {}),
    [selected?.env]
  )
  const envDirty = !rowsEqual(envRows, seedRows)

  const handleSaveEnv = useCallback(async () => {
    if (!selected) return
    setBusy(true)
    try {
      await componentsSetEnv(selected.id, rowsToEnv(envRows))
      await refresh()
      toast.success(t("toasts.saved"))
    } catch (err) {
      toast.error(t("toasts.saveFailed"), {
        description: toErrorMessage(err),
      })
    } finally {
      setBusy(false)
    }
  }, [selected, envRows, refresh, t])

  const handleToolsSettingToggle = useCallback(
    async (next: boolean) => {
      // Optimistic: the toggle must not lag behind the finger.
      setToolsSettings((prev) =>
        prev
          ? { ...prev, preferBundledTools: next }
          : { preferBundledTools: next }
      )
      setSavingToolsSettings(true)
      try {
        const saved = await setSystemToolsSettings({
          preferBundledTools: next,
        })
        setToolsSettings(saved)
      } catch (err) {
        // Roll the optimistic value back to whatever the backend holds now.
        try {
          const fresh = await getSystemToolsSettings()
          setToolsSettings(fresh)
        } catch {
          /* leave the optimistic value; the toast is the more useful signal */
        }
        toast.error(t("toasts.saveFailed"), {
          description: toErrorMessage(err),
        })
      } finally {
        setSavingToolsSettings(false)
      }
    },
    [t]
  )

  const handleSetActiveProfile = useCallback(
    async (profile: RaProfileDetail) => {
      setProfileBusy(true)
      try {
        await raProfileSetActive(profile.id)
        await refreshProfiles()
        toast.success(t("toasts.profileActivated"))
      } catch (err) {
        toast.error(t("toasts.profileActionFailed"), {
          description: toErrorMessage(err),
        })
      } finally {
        setProfileBusy(false)
      }
    },
    [refreshProfiles, t]
  )

  const handleCreateProfile = useCallback(async () => {
    const name = createName.trim()
    if (!name) return
    setProfileBusy(true)
    try {
      await raProfileCreate({
        name,
        cloneFrom: cloneFrom === PROFILE_CLONE_CURRENT ? null : cloneFrom,
      })
      await refreshProfiles()
      toast.success(t("toasts.profileCreated"))
      setCreateOpen(false)
      setCreateName("")
      setCloneFrom(PROFILE_CLONE_CURRENT)
    } catch (err) {
      toast.error(t("toasts.profileActionFailed"), {
        description: toErrorMessage(err),
      })
    } finally {
      setProfileBusy(false)
    }
  }, [createName, cloneFrom, refreshProfiles, t])

  const handleDeleteProfile = useCallback(async () => {
    if (!deleteTarget) return
    setDeleting(true)
    try {
      await raProfileDelete(deleteTarget.id)
      await refreshProfiles()
      toast.success(t("toasts.profileDeleted"))
      setDeleteTarget(null)
    } catch (err) {
      toast.error(t("toasts.profileActionFailed"), {
        description: toErrorMessage(err),
      })
    } finally {
      setDeleting(false)
    }
  }, [deleteTarget, refreshProfiles, t])

  const safeContainerWidth =
    panelContainerWidth > 0 ? panelContainerWidth : 1200
  const leftMinSize = clamp(
    toPercent(LEFT_MIN_WIDTH, safeContainerWidth),
    5,
    95
  )
  const rightMinSize = clamp(
    toPercent(RIGHT_MIN_WIDTH, safeContainerWidth),
    5,
    95
  )
  const leftMaxSize = Math.max(leftMinSize, 100 - rightMinSize)

  const renderMcpDetail = (component: ComponentInfo) => {
    const status = asMcpStatus(component)
    return (
      <div className="space-y-4">
        <div>
          <h3 className="text-sm font-semibold">{component.name}</h3>
          {component.description && (
            <p className="mt-1 text-xs text-muted-foreground">
              {component.description}
            </p>
          )}
        </div>
        <SettingCard>
          <RcMcpPanel
            status={status}
            loadError={null}
            onRefresh={() => {
              // The settings list is rebuilt by every `componentsApply`, so
              // an explicit refresh is the same as re-reading the list.
              void refresh()
            }}
            onToggleGroup={(key, value) =>
              void handleMcpToggleGroup(key, value)
            }
            pendingGroups={pendingMcpGroups}
            onStart={() => void handleMcpStartService()}
            starting={mcpStarting}
            actionError={mcpActionError}
          />
        </SettingCard>
      </div>
    )
  }

  const renderSkillDetail = (component: ComponentInfo) => {
    const skill = asSkill(component)
    if (!skill) return null
    const centralLabel =
      skill.central === "present"
        ? t("skillCentralPresent")
        : skill.central === "conflict"
          ? t("skillCentralConflict")
          : t("skillCentralMissing")
    const isConflict = skill.central === "conflict"
    const updateLabel = skill.updateAvailable
      ? t("skillUpdateAvailable")
      : t("skillUpToDate")
    // The link matrix uses a fixed agent set driven from the linked list:
    // any agent already linked is shown checked; everything else unchecked.
    // A conflict means we cannot install, but linking the existing user
    // copy is still possible and is what the matrix shows.
    const linkedSet = new Set(skill.linkedAgents)
    return (
      <div className="space-y-4">
        <div>
          <h3 className="text-sm font-semibold">{component.name}</h3>
          {component.description && (
            <p className="mt-1 text-xs text-muted-foreground">
              {component.description}
            </p>
          )}
        </div>

        <SettingCard>
          <SettingRow
            title={t("skillCentral")}
            control={
              <Badge variant={isConflict ? "destructive" : "outline"}>
                {centralLabel}
              </Badge>
            }
          />
          {skill.centralHash && (
            <SettingRow
              title={t("skillCentralHash")}
              control={
                <code className="rounded bg-muted px-1.5 py-0.5 text-2xs text-muted-foreground">
                  {skill.centralHash}
                </code>
              }
            />
          )}
          {skill.bundledHash && (
            <SettingRow
              title={t("skillBundledHash")}
              control={
                <code className="rounded bg-muted px-1.5 py-0.5 text-2xs text-muted-foreground">
                  {skill.bundledHash}
                </code>
              }
            />
          )}
          <SettingRow
            title={t("skillStatus")}
            control={
              <span className="text-xs text-muted-foreground">{updateLabel}</span>
            }
          >
            <Button
              size="sm"
              variant="outline"
              disabled={busy || isConflict}
              onClick={() => {
                handleSkillRefresh(component).catch((err) => {
                  console.error("[ComponentsSettings] skill refresh failed:", err)
                })
              }}
            >
              <RefreshCw className="h-3.5 w-3.5" />
              {t("skillRefresh")}
            </Button>
          </SettingRow>
        </SettingCard>

        {isConflict && (
          <SettingNote icon={Info}>
            {t("skillConflictCallout")}
          </SettingNote>
        )}

        <SettingCard>
          <SettingRow
            title={t("skillAgentsTitle")}
            description={t("skillAgentsDescription")}
          />
          <div className="mt-2 space-y-2">
            {KNOWN_AGENTS.map((agent) => {
              const checked = linkedSet.has(agent.id)
              return (
                <label
                  key={agent.id}
                  className="flex items-center justify-between gap-2 rounded-md border px-2.5 py-1.5"
                >
                  <div>
                    <div className="text-xs font-medium">{agent.name}</div>
                    <div className="text-2xs text-muted-foreground">
                      {agent.id}
                    </div>
                  </div>
                  <Switch
                    checked={checked}
                    disabled={busy}
                    onCheckedChange={(next) => {
                      handleSkillLinkToggle(component, agent.id, next).catch(
                        (err) => {
                          console.error(
                            "[ComponentsSettings] skill link failed:",
                            err
                          )
                        }
                      )
                    }}
                    aria-label={agent.name}
                  />
                </label>
              )
            })}
          </div>
        </SettingCard>
      </div>
    )
  }

  const renderBinaryDetail = (component: ComponentInfo) => {
    const state = asBinaryLike(component)
    if (!state) return null
    const appletTools = state.appletTools ?? []
    return (
      <div className="space-y-4">
        <div>
          <h3 className="text-sm font-semibold">{component.name}</h3>
          {component.description && (
            <p className="mt-1 text-xs text-muted-foreground">
              {component.description}
            </p>
          )}
        </div>
        <SettingCard>
          <SettingRow
            title={t("enabled")}
            control={
              <Switch
                checked={component.required}
                disabled={component.required || busy}
                onCheckedChange={(checked) => {
                  handleBinaryToggle(component, checked).catch((err) => {
                    console.error(
                      "[ComponentsSettings] binary toggle failed:",
                      err
                    )
                  })
                }}
                aria-label={t("enabled")}
              />
            }
          />
          <SettingRow
            icon={Info}
            title={t("componentId")}
            control={
              <code className="rounded bg-muted px-1.5 py-0.5 text-2xs text-muted-foreground">
                {component.id}
              </code>
            }
          />
          <SettingRow
            icon={Terminal}
            title={t("versionInstalled")}
            control={
              <span className="text-xs text-muted-foreground">
                {state.installedVersion ?? t("notInstalled")}
              </span>
            }
          />
          <SettingRow
            icon={Terminal}
            title={t("versionAvailable")}
            control={
              <span className="text-xs text-muted-foreground">
                {state.availableVersion ?? "—"}
              </span>
            }
          >
            {(state.kind === "binary" ||
              (state.kind === "multicall" &&
                component.id === "@recurbusybox")) && (
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  disabled={updating}
                  onClick={() => {
                    handleUpdate(component).catch((err) => {
                      console.error(
                        "[ComponentsSettings] update failed:",
                        err
                      )
                    })
                  }}
                >
                  {updating ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <ArrowDownToLine className="h-3.5 w-3.5" />
                  )}
                  {updating
                    ? t("updating")
                    : state.installedVersion
                      ? t("update")
                      : t("install")}
                </Button>
              </div>
            )}
          </SettingRow>
        </SettingCard>

        {appletTools.length > 0 && (
          <SettingCard>
            <SettingRow
              title={t("appletsTitle")}
              description={t("appletsDescription")}
            />
            <div className="mt-2 space-y-1">
              {appletTools.map((tool) => (
                <label
                  key={tool.id}
                  className={cn(
                    "flex items-center justify-between gap-2 rounded-md border px-2.5 py-1.5",
                    !tool.installed && "opacity-60"
                  )}
                >
                  <div>
                    <div className="font-mono text-xs">{tool.name}</div>
                    <div className="text-2xs text-muted-foreground">
                      {tool.id}
                    </div>
                  </div>
                  <Switch
                    checked={tool.enabled}
                    disabled={busy || !tool.installed}
                    onCheckedChange={(next) => {
                      handleAppletToggle(component, tool, next).catch((err) => {
                        console.error(
                          "[ComponentsSettings] applet toggle failed:",
                          err
                        )
                      })
                    }}
                    aria-label={tool.name}
                  />
                </label>
              ))}
            </div>
          </SettingCard>
        )}

        {state.kind === "multicall" && component.id === "@recurbusybox" && (
          <SettingCard>
            <SettingRow
              title={t("toolsPreferBundledLabel")}
              description={t("toolsPreferBundledDescription")}
              control={
                <Switch
                  checked={toolsSettings?.preferBundledTools ?? true}
                  disabled={savingToolsSettings}
                  onCheckedChange={(next) => {
                    handleToolsSettingToggle(next).catch((err) => {
                      console.error(
                        "[ComponentsSettings] tools preference failed:",
                        err
                      )
                    })
                  }}
                  aria-label={t("toolsPreferBundledLabel")}
                />
              }
            />
          </SettingCard>
        )}

        {component.required && (
          <SettingNote icon={Lock}>
            {t("requiredNote")}
          </SettingNote>
        )}

        <SettingCard>
          <SettingRow
            title={t("envTitle")}
            description={t("envDescription")}
            control={
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    setEnvRows((prev) => [...prev, { key: "", value: "" }])
                  }
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("envAdd")}
                </Button>
                <Button
                  size="sm"
                  disabled={!envDirty || savingToolsSettings}
                  onClick={() => {
                    handleSaveEnv().catch((err) => {
                      console.error(
                        "[ComponentsSettings] env save failed:",
                        err
                      )
                    })
                  }}
                >
                  {t("envSave")}
                </Button>
              </div>
            }
          >
            <div className="space-y-2">
              {envRows.length === 0 ? (
                <p className="text-xs text-muted-foreground">{t("envHint")}</p>
              ) : (
                envRows.map((row, index) => (
                  <div
                    // Rows are positional and may be empty, so the
                    // index is the only stable identity here.
                    key={index}
                    className="flex items-center gap-2"
                  >
                    <Input
                      value={row.key}
                      placeholder={t("envKey")}
                      aria-label={t("envKey")}
                      className="h-8 flex-1 font-mono text-xs"
                      onChange={(event) => {
                        const next = event.target.value
                        setEnvRows((prev) =>
                          prev.map((item, i) =>
                            i === index ? { ...item, key: next } : item
                          )
                        )
                      }}
                    />
                    <Input
                      value={row.value}
                      placeholder={t("envValue")}
                      aria-label={t("envValue")}
                      className="h-8 flex-[2] font-mono text-xs"
                      onChange={(event) => {
                        const next = event.target.value
                        setEnvRows((prev) =>
                          prev.map((item, i) =>
                            i === index ? { ...item, value: next } : item
                          )
                        )
                      }}
                    />
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={t("envRemove")}
                      onClick={() =>
                        setEnvRows((prev) =>
                          prev.filter((_, i) => i !== index)
                        )
                      }
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                ))
              )}
            </div>
          </SettingRow>
        </SettingCard>
      </div>
    )
  }

  const renderDetail = (component: ComponentInfo) => {
    const kind = component.state.kind
    if (kind === "mcp") return renderMcpDetail(component)
    if (kind === "skill") return renderSkillDetail(component)
    return renderBinaryDetail(component)
  }

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 mr-2 animate-spin" />
        {t("title")}
      </div>
    )
  }

  return (
    <div className="h-full flex flex-col p-3 md:p-4">
      <div className="flex items-center justify-between gap-3 pb-4">
        <div>
          <h2 className="text-base font-semibold">{t("title")}</h2>
          <p className="text-xs text-muted-foreground mt-1">
            {tab === "profiles" ? t("profilesDescription") : t("description")}
          </p>
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={() => {
            refresh().catch((err) => {
              console.error("[ComponentsSettings] refresh failed:", err)
            })
            refreshProfiles().catch((err) => {
              console.error("[ComponentsSettings] profile refresh failed:", err)
            })
          }}
        >
          <RefreshCw className="h-3.5 w-3.5" />
          {t("recheck")}
        </Button>
      </div>

      <Tabs
        value={tab}
        onValueChange={handleTabChange}
        className="flex flex-col flex-1 min-h-0"
      >
        <TabsList className="w-fit shrink-0">
          <TabsTrigger value="binary" className={TRIGGER_ACTIVE}>
            {t("tabs.binary")}
          </TabsTrigger>
          <TabsTrigger value="multicall" className={TRIGGER_ACTIVE}>
            {t("tabs.multicall")}
          </TabsTrigger>
          <TabsTrigger value="mcp" className={TRIGGER_ACTIVE}>
            {t("tabs.mcp")}
          </TabsTrigger>
          <TabsTrigger value="skill" className={TRIGGER_ACTIVE}>
            {t("tabs.skill")}
          </TabsTrigger>
          <TabsTrigger value="profiles" className={TRIGGER_ACTIVE}>
            {t("tabs.profiles")}
          </TabsTrigger>
        </TabsList>

        {loadError && (
          <div className="mt-3 rounded-md border border-red-500/30 bg-red-500/5 px-3 py-2 text-xs text-red-400">
            {t("loadFailed")}: {loadError}
          </div>
        )}

        <div ref={panelContainerRef} className="mt-3 flex-1 min-h-0 min-w-0">
          <ResizablePanelGroup
            direction="horizontal"
            className="h-full min-h-0 min-w-0"
          >
            <ResizablePanel
              defaultSize={34}
              minSize={leftMinSize}
              maxSize={leftMaxSize}
            >
              <div className="min-h-0 h-full min-w-0 rounded-lg border bg-card flex flex-col overflow-hidden lg:rounded-r-none">
                {tab === "profiles" ? (
                  <div className="flex items-center justify-between gap-2 border-b p-3">
                    <span className="text-xs font-medium text-muted-foreground">
                      {t("profilesTitle")}
                    </span>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setCreateOpen(true)}
                    >
                      <Plus className="h-3.5 w-3.5" />
                      {t("profilesCreate")}
                    </Button>
                  </div>
                ) : (
                  <div className="border-b p-3">
                    <span className="text-xs font-medium text-muted-foreground">
                      {t("title")}
                    </span>
                  </div>
                )}

                {profilesError && tab === "profiles" && (
                  <div className="border-b bg-destructive/5 px-3 py-2 text-2xs text-destructive">
                    {t("loadFailed")}: {profilesError}
                  </div>
                )}

                {tab === "profiles" ? (
                  <div className="flex-1 min-h-0 flex flex-col">
                    {visibleProfiles.length === 0 ? (
                      <div className="flex-1 flex items-center justify-center text-xs text-muted-foreground px-4 text-center">
                        {t("profilesEmpty")}
                      </div>
                    ) : (
                      <ScrollArea className="flex-1 min-h-0">
                        <div className="space-y-1 p-2">
                          {visibleProfiles.map((profile) => (
                            <button
                              key={profile.id}
                              type="button"
                              onClick={() => setSelectedProfileId(profile.id)}
                              className={cn(
                                "w-full rounded-md border px-2.5 py-2 text-left transition-colors",
                                selectedProfileId === profile.id
                                  ? "border-primary/40 bg-primary/5"
                                  : "border-transparent hover:bg-muted/60"
                              )}
                            >
                              <div className="flex items-center justify-between gap-2">
                                <span className="truncate text-sm font-medium">
                                  {profile.name}
                                </span>
                                {profile.active && (
                                  <Badge variant="secondary">
                                    {t("profilesActive")}
                                  </Badge>
                                )}
                              </div>
                              <div className="mt-0.5 truncate text-2xs text-muted-foreground">
                                {[profile.provider, profile.model]
                                  .filter(Boolean)
                                  .join(" · ") || "—"}
                              </div>
                            </button>
                          ))}
                        </div>
                      </ScrollArea>
                    )}
                    <p className="border-t px-3 py-2 text-2xs text-muted-foreground">
                      {t("profilesMainHidden")}
                    </p>
                  </div>
                ) : inCategory.length === 0 ? (
                  <div className="flex-1 flex items-center justify-center text-xs text-muted-foreground px-4 text-center">
                    {t("empty")}
                  </div>
                ) : (
                  <ScrollArea className="flex-1 min-h-0">
                    <div className="space-y-1 p-2">
                    {inCategory.map((component) => {
                      const version = asBinaryLike(component)?.installedVersion ?? null
                      return (
                        <button
                          key={component.id}
                          type="button"
                          onClick={() => setSelectedId(component.id)}
                          className={cn(
                            "w-full rounded-md border px-2.5 py-2 text-left transition-colors",
                            selectedId === component.id
                              ? "border-primary/40 bg-primary/5"
                              : "border-transparent hover:bg-muted/60"
                          )}
                        >
                          <div className="flex items-center justify-between gap-2">
                            <span className="truncate text-sm font-medium">
                              {component.name}
                            </span>
                            {component.state.kind !== "mcp" &&
                              component.state.kind !== "skill" &&
                              "updateAvailable" in component.state &&
                              component.state.updateAvailable && (
                                <Badge variant="secondary">
                                  {t("updateAvailable")}
                                </Badge>
                              )}
                          </div>
                          {component.description && (
                            <div className="mt-0.5 truncate text-2xs text-muted-foreground">
                              {component.description}
                            </div>
                          )}
                          <div className="mt-1 flex items-center gap-1.5">
                            <Badge variant="outline">
                              {version ?? t("notInstalled")}
                            </Badge>
                          </div>
                        </button>
                      )
                    })}
                    </div>
                  </ScrollArea>
                )}
              </div>
            </ResizablePanel>

            <ResizableHandle withHandle />

            <ResizablePanel defaultSize={66} minSize={rightMinSize}>
              <div className="h-full flex-1 min-h-0 min-w-0 rounded-lg border bg-card overflow-hidden lg:rounded-l-none lg:border-l-0">
                {tab === "profiles" ? (
                  selectedProfile ? (
                    <div className="h-full flex flex-col">
                      <ScrollArea className="flex-1 min-h-0">
                        <div className="p-4 space-y-4">
                          <div>
                            <div className="flex items-center gap-2">
                              <h3 className="text-sm font-semibold">
                                {selectedProfile.name}
                              </h3>
                              {selectedProfile.active && (
                                <Badge variant="secondary">
                                  {t("profilesActive")}
                                </Badge>
                              )}
                            </div>
                            <code className="mt-1 inline-block rounded bg-muted px-1.5 py-0.5 text-2xs text-muted-foreground">
                              {selectedProfile.id}
                            </code>
                          </div>

                          <SettingCard>
                            <SettingRow icon={Info} title={t("profilesPath")}>
                              <code className="block truncate rounded bg-muted px-1.5 py-0.5 text-2xs text-muted-foreground">
                                {selectedProfile.path}
                              </code>
                            </SettingRow>
                            {selectedProfile.dataDir && (
                              <SettingRow
                                icon={FolderOpen}
                                title={t("profilesDataDir")}
                              >
                                <code className="block truncate rounded bg-muted px-1.5 py-0.5 text-2xs text-muted-foreground">
                                  {selectedProfile.dataDir}
                                </code>
                              </SettingRow>
                            )}
                          </SettingCard>

                          <div className="flex items-center gap-2">
                            <Button
                              size="sm"
                              disabled={selectedProfile.active || profileBusy}
                              onClick={() => {
                                handleSetActiveProfile(selectedProfile).catch(
                                  (err) => {
                                    console.error(
                                      "[ComponentsSettings] set active failed:",
                                      err
                                    )
                                  }
                                )
                              }}
                            >
                              {profileBusy ? (
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                              ) : null}
                              {t("profilesSetActive")}
                            </Button>
                            <Button
                              size="sm"
                              variant="outline"
                              className="text-destructive"
                              disabled={
                                selectedProfile.active ||
                                selectedProfile.id === "_main" ||
                                deleting
                              }
                              onClick={() => setDeleteTarget(selectedProfile)}
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                              {t("profilesDelete")}
                            </Button>
                          </div>
                        </div>
                      </ScrollArea>
                    </div>
                  ) : (
                    <div className="h-full flex items-center justify-center px-4 text-center text-xs text-muted-foreground">
                      {t("selectPrompt")}
                    </div>
                  )
                ) : selected ? (
                  <div className="h-full flex flex-col">
                    <ScrollArea className="flex-1 min-h-0">
                      <div className="p-4">{renderDetail(selected)}</div>
                    </ScrollArea>
                  </div>
                ) : (
                  <div className="h-full flex items-center justify-center px-4 text-center text-xs text-muted-foreground">
                    {t("selectPrompt")}
                  </div>
                )}
              </div>
            </ResizablePanel>
          </ResizablePanelGroup>
        </div>
      </Tabs>

      <Dialog
        open={createOpen}
        onOpenChange={(open) => {
          setCreateOpen(open)
          if (!open) setCreateName("")
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("profilesCreateTitle")}</DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1.5">
              <Label htmlFor="ra-profile-name">{t("profilesName")}</Label>
              <Input
                id="ra-profile-name"
                value={createName}
                placeholder={t("profilesNamePlaceholder")}
                onChange={(event) => setCreateName(event.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label>{t("profilesCloneFrom")}</Label>
              <Select value={cloneFrom} onValueChange={setCloneFrom}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={PROFILE_CLONE_CURRENT}>
                    {t("profilesCloneFromCurrent")}
                  </SelectItem>
                  {visibleProfiles.map((profile) => (
                    <SelectItem key={profile.id} value={profile.id}>
                      {profile.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button
              disabled={!createName.trim() || profileBusy}
              onClick={() => {
                handleCreateProfile().catch((err) => {
                  console.error(
                    "[ComponentsSettings] create profile failed:",
                    err
                  )
                })
              }}
            >
              {profileBusy ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : null}
              {t("profilesCreate")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null)
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t("profilesDeleteConfirmTitle")}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t("profilesDeleteConfirmBody", {
                name: deleteTarget?.name ?? "",
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>
              {t("profilesCancel")}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(event) => {
                event.preventDefault()
                handleDeleteProfile().catch((err) => {
                  console.error(
                    "[ComponentsSettings] delete profile failed:",
                    err
                  )
                })
              }}
              disabled={deleting}
            >
              {deleting ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : null}
              {t("profilesDelete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

// Skill agents the matrix can show. The backend only sends back the
// currently-linked subset, but the UI needs a fixed column list so a user
// can link to an agent they have not used before. The `id` is the wire slug
// the backend parses via `AgentType::from_wire`, which is snake_case.
const KNOWN_AGENTS: ReadonlyArray<{ id: string; name: string }> = [
  { id: "claude_code", name: "Claude Code" },
  { id: "codex", name: "Codex" },
  { id: "open_code", name: "OpenCode" },
  { id: "gemini", name: "Gemini CLI" },
  { id: "cline", name: "Cline" },
  { id: "open_claw", name: "OpenClaw" },
  { id: "pi", name: "Pi" },
  { id: "ra", name: "RecurAgent" },
]
