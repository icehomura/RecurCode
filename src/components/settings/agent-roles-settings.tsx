"use client"

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent,
  type ReactNode,
} from "react"
import { GripVertical, Loader2, Plus, Save, Trash2 } from "lucide-react"
import { Reorder, useDragControls } from "motion/react"
import { useTranslations } from "next-intl"
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
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Slider } from "@/components/ui/slider"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import {
  agentRoleCreate,
  agentRoleDelete,
  agentRoleList,
  agentRoleReorder,
  agentRoleUpdate,
} from "@/lib/api"
import type { AgentRole } from "@/lib/types"
import { toErrorMessage } from "@/lib/app-error"

const LEFT_MIN_WIDTH = 280
const RIGHT_MIN_WIDTH = 420

/** A picker entry is either the i18n key of the "any agent" default or a
 *  literal product name. The explicit union is what lets `"labelKey" in at`
 *  narrow to the keyed arm — as a widened object type the key reads as
 *  `string | undefined` and the `t()` call below stops type-checking. */
type AgentTypeEntry =
  | { value: string; labelKey: "anyAgent" }
  | { value: string; label: string }

const AGENT_TYPES: AgentTypeEntry[] = [
  { value: "", labelKey: "anyAgent" },
  { value: "ra", label: "RecurAgent" },
  { value: "claude_code", label: "Claude Code" },
  { value: "codex", label: "Codex CLI" },
  { value: "open_code", label: "OpenCode" },
  { value: "gemini", label: "Gemini CLI" },
  { value: "open_claw", label: "OpenClaw" },
  { value: "cline", label: "Cline" },
  { value: "hermes", label: "Hermes Agent" },
  { value: "code_buddy", label: "CodeBuddy" },
  { value: "kimi_code", label: "Kimi Code" },
  { value: "pi", label: "Pi" },
  { value: "grok", label: "Grok" },
  { value: "cursor", label: "Cursor" },
  { value: "deepseek", label: "DeepSeek" },
  { value: "qoder", label: "Qoder" },
  { value: "antigravity", label: "Google Antigravity" },
]

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}

function toPercent(pixels: number, totalPixels: number): number {
  if (totalPixels <= 0) return 0
  return (pixels / totalPixels) * 100
}

interface AgentRoleReorderItemProps {
  role: AgentRole
  selected: boolean
  reordering: boolean
  onSelect: (id: number) => void
  onDragStart: () => void
  onDragEnd: () => void
  children: (
    startDrag: (event: PointerEvent<HTMLButtonElement>) => void
  ) => ReactNode
}

function AgentRoleReorderItem({
  role,
  selected,
  reordering,
  onSelect,
  onDragStart,
  onDragEnd,
  children,
}: AgentRoleReorderItemProps) {
  const dragControls = useDragControls()

  const startDrag = useCallback(
    (event: PointerEvent<HTMLButtonElement>) => {
      event.preventDefault()
      event.stopPropagation()
      dragControls.start(event)
    },
    [dragControls]
  )

  return (
    <Reorder.Item
      as="section"
      value={role}
      data-agent-role-id={role.id}
      drag={reordering ? false : "y"}
      dragListener={false}
      dragControls={dragControls}
      dragMomentum={false}
      layout="position"
      className={cn(
        "rounded-lg border bg-card p-2.5 transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40",
        selected && "border-primary/60 bg-primary/5"
      )}
      tabIndex={0}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onClick={() => onSelect(role.id)}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return
        if (event.key !== "Enter" && event.key !== " ") return
        event.preventDefault()
        onSelect(role.id)
      }}
    >
      {children(startDrag)}
    </Reorder.Item>
  )
}

export function AgentRolesSettings() {
  const t = useTranslations("AgentRolesSettings")

  const [roles, setRoles] = useState<AgentRole[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<number | null>(null)

  const [draftName, setDraftName] = useState("")
  const [draftDescription, setDraftDescription] = useState("")
  const [draftSystemPrompt, setDraftSystemPrompt] = useState("")
  const [draftAgentType, setDraftAgentType] = useState("")
  const [draftModelId, setDraftModelId] = useState("")
  const [draftTemperature, setDraftTemperature] = useState(1.0)
  const [draftToolsJson, setDraftToolsJson] = useState("")

  const [saving, setSaving] = useState(false)
  const [creating, setCreating] = useState(false)
  const [deleteTargetId, setDeleteTargetId] = useState<number | null>(null)
  const [deleting, setDeleting] = useState(false)

  const [reordering, setReordering] = useState(false)
  const pendingOrderRef = useRef<number[] | null>(null)

  const panelContainerRef = useRef<HTMLDivElement | null>(null)
  const [panelContainerWidth, setPanelContainerWidth] = useState(0)
  const nameInputRef = useRef<HTMLInputElement | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setLoadError(null)
    try {
      const list = await agentRoleList()
      setRoles(list)
    } catch (err) {
      const message = toErrorMessage(err)
      setLoadError(message)
      setRoles([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refresh().catch((err) => {
      console.error("[AgentRolesSettings] initial refresh failed:", err)
    })
  }, [refresh])

  useEffect(() => {
    const container = panelContainerRef.current
    if (!container) return
    const updateWidth = (next: number) => {
      setPanelContainerWidth((prev) =>
        Math.abs(prev - next) < 1 ? prev : next
      )
    }
    updateWidth(container.getBoundingClientRect().width)
    const observer = new ResizeObserver((entries) => {
      updateWidth(
        entries[0]?.contentRect.width ?? container.getBoundingClientRect().width
      )
    })
    observer.observe(container)
    return () => {
      observer.disconnect()
    }
  }, [])

  const selectedRole = useMemo(
    () => roles.find((r) => r.id === selectedId) ?? null,
    [roles, selectedId]
  )

  useEffect(() => {
    if (selectedRole) {
      setDraftName(selectedRole.name)
      setDraftDescription(selectedRole.description)
      setDraftSystemPrompt(selectedRole.systemPrompt)
      setDraftAgentType(selectedRole.agentType ?? "")
      setDraftModelId(selectedRole.modelId ?? "")
      setDraftTemperature(selectedRole.temperature ?? 1.0)
      setDraftToolsJson(selectedRole.toolsJson ?? "")
    } else {
      setDraftName("")
      setDraftDescription("")
      setDraftSystemPrompt("")
      setDraftAgentType("")
      setDraftModelId("")
      setDraftTemperature(1.0)
      setDraftToolsJson("")
    }
  }, [selectedRole])

  useEffect(() => {
    if (selectedId === null && roles.length > 0) {
      setSelectedId(roles[0].id)
    }
  }, [selectedId, roles])

  const isDirty = useMemo(() => {
    if (!selectedRole) return false
    return (
      draftName !== selectedRole.name ||
      draftDescription !== selectedRole.description ||
      draftSystemPrompt !== selectedRole.systemPrompt ||
      draftAgentType !== (selectedRole.agentType ?? "") ||
      draftModelId !== (selectedRole.modelId ?? "") ||
      draftTemperature !== (selectedRole.temperature ?? 1.0) ||
      draftToolsJson !== (selectedRole.toolsJson ?? "")
    )
  }, [
    selectedRole,
    draftName,
    draftDescription,
    draftSystemPrompt,
    draftAgentType,
    draftModelId,
    draftTemperature,
    draftToolsJson,
  ])

  const persistReorder = useCallback(
    async (ids: number[]) => {
      if (ids.length === 0) return
      setReordering(true)
      try {
        await agentRoleReorder(ids)
      } catch (err) {
        const message = toErrorMessage(err)
        toast.error(t("toasts.saveOrderFailed"), { description: message })
        await refresh()
      } finally {
        setReordering(false)
      }
    },
    [refresh, t]
  )

  const handleReorder = useCallback((next: AgentRole[]) => {
    const reordered = next.map((r, index) => ({ ...r, sortOrder: index }))
    setRoles(reordered)
    pendingOrderRef.current = reordered.map((r) => r.id)
  }, [])

  const handleCreate = useCallback(async () => {
    setCreating(true)
    try {
      const created = await agentRoleCreate({
        name: "",
        description: "",
        systemPrompt: "",
      })
      setRoles((prev) => [...prev, created])
      setSelectedId(created.id)
      toast.success(t("toasts.created"))
      requestAnimationFrame(() => {
        nameInputRef.current?.focus()
      })
    } catch (err) {
      const message = toErrorMessage(err)
      toast.error(t("toasts.createFailed"), { description: message })
    } finally {
      setCreating(false)
    }
  }, [t])

  const handleSave = useCallback(async () => {
    if (!selectedRole) return
    setSaving(true)
    try {
      const updated = await agentRoleUpdate({
        id: selectedRole.id,
        name: draftName,
        description: draftDescription,
        systemPrompt: draftSystemPrompt,
        agentType: draftAgentType || null,
        modelId: draftModelId || null,
        temperature: draftTemperature,
        toolsJson: draftToolsJson || null,
      })
      setRoles((prev) =>
        prev.map((r) => (r.id === updated.id ? updated : r))
      )
      toast.success(t("toasts.saved"))
    } catch (err) {
      const message = toErrorMessage(err)
      toast.error(t("toasts.saveFailed"), { description: message })
    } finally {
      setSaving(false)
    }
  }, [
    selectedRole,
    draftName,
    draftDescription,
    draftSystemPrompt,
    draftAgentType,
    draftModelId,
    draftTemperature,
    draftToolsJson,
    t,
  ])

  const handleDelete = useCallback(async () => {
    if (deleteTargetId === null) return
    const target = deleteTargetId
    setDeleting(true)
    try {
      await agentRoleDelete(target)
      setRoles((prev) => {
        const next = prev.filter((r) => r.id !== target)
        if (selectedId === target) {
          setSelectedId(next[0]?.id ?? null)
        }
        return next
      })
      toast.success(t("toasts.deleted"))
      setDeleteTargetId(null)
    } catch (err) {
      const message = toErrorMessage(err)
      toast.error(t("toasts.deleteFailed"), { description: message })
    } finally {
      setDeleting(false)
    }
  }, [deleteTargetId, selectedId, t])

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

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 mr-2 animate-spin" />
        {t("loading")}
      </div>
    )
  }

  const deleteTargetRole =
    deleteTargetId !== null
      ? (roles.find((r) => r.id === deleteTargetId) ?? null)
      : null

  return (
    <div className="h-full flex flex-col p-3 md:p-4">
      <div className="flex items-center justify-between gap-3 pb-4">
        <div>
          <h2 className="text-base font-semibold">{t("title")}</h2>
          <p className="text-xs text-muted-foreground mt-1">
            {t("description")}
          </p>
        </div>
      </div>

      {loadError && (
        <div className="mb-3 rounded-md border border-red-500/30 bg-red-500/5 px-3 py-2 text-xs text-red-400">
          {loadError}
        </div>
      )}

      <div ref={panelContainerRef} className="flex-1 min-h-0 min-w-0">
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
              <div className="border-b p-3">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-medium text-muted-foreground">
                    {t("title")}
                  </span>
                  <Button
                    size="sm"
                    onClick={() => {
                      handleCreate().catch((err) => {
                        console.error(
                          "[AgentRolesSettings] create failed:",
                          err
                        )
                      })
                    }}
                    disabled={creating}
                  >
                    {creating ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Plus className="h-3.5 w-3.5" />
                    )}
                    {t("actions.new")}
                  </Button>
                </div>
              </div>

              {roles.length === 0 ? (
                <div className="flex-1 flex items-center justify-center text-xs text-muted-foreground px-4 text-center">
                  {t("emptyList")}
                </div>
              ) : (
                <Reorder.Group
                  as="div"
                  axis="y"
                  values={roles}
                  onReorder={handleReorder}
                  className="flex-1 min-h-0 overflow-y-auto space-y-2 p-2"
                >
                  {roles.map((r) => (
                    <AgentRoleReorderItem
                      key={r.id}
                      role={r}
                      selected={selectedId === r.id}
                      reordering={reordering}
                      onSelect={(id) => setSelectedId(id)}
                      onDragStart={() => {
                        /* no-op: list re-render handles dragging state */
                      }}
                      onDragEnd={() => {
                        const order = pendingOrderRef.current
                        pendingOrderRef.current = null
                        if (order && !reordering) {
                          persistReorder(order).catch((err) => {
                            console.error(
                              "[AgentRolesSettings] reorder failed:",
                              err
                            )
                          })
                        }
                      }}
                    >
                      {(startDrag) => (
                        <div className="flex items-center gap-2 overflow-hidden">
                          <button
                            type="button"
                            className="text-muted-foreground cursor-grab active:cursor-grabbing rounded p-0.5 hover:bg-muted"
                            title={t("actions.dragSort")}
                            aria-label={t("actions.dragSortMessage", {
                              name: r.name || t("untitled"),
                            })}
                            onPointerDown={startDrag}
                            onClick={(event) => event.stopPropagation()}
                            disabled={reordering}
                          >
                            <GripVertical className="h-3.5 w-3.5" />
                          </button>
                          <div className="min-w-0 flex-1">
                            <div className="text-sm font-medium truncate">
                              {r.name || (
                                <span className="italic text-muted-foreground">
                                  {t("untitled")}
                                </span>
                              )}
                            </div>
                            {r.description && (
                              <div className="text-2xs text-muted-foreground truncate mt-0.5">
                                {r.description}
                              </div>
                            )}
                          </div>
                        </div>
                      )}
                    </AgentRoleReorderItem>
                  ))}
                </Reorder.Group>
              )}
            </div>
          </ResizablePanel>

          <ResizableHandle withHandle />

          <ResizablePanel defaultSize={66} minSize={rightMinSize}>
            <div className="h-full flex-1 min-h-0 min-w-0 rounded-lg border bg-card overflow-hidden lg:rounded-l-none lg:border-l-0">
              {selectedRole ? (
                <div className="h-full flex flex-col">
                  <div className="flex-1 overflow-y-auto p-4 space-y-4">
                    <div className="space-y-1.5">
                      <Label htmlFor="agent-role-name" className="text-xs">
                        {t("fields.name")}
                      </Label>
                      <Input
                        id="agent-role-name"
                        ref={nameInputRef}
                        value={draftName}
                        onChange={(event) => setDraftName(event.target.value)}
                        placeholder={t("fields.namePlaceholder")}
                      />
                    </div>
                    <div className="space-y-1.5">
                      <Label htmlFor="agent-role-description" className="text-xs">
                        {t("fields.description")}
                      </Label>
                      <Input
                        id="agent-role-description"
                        value={draftDescription}
                        onChange={(event) =>
                          setDraftDescription(event.target.value)
                        }
                        placeholder={t("fields.descriptionPlaceholder")}
                      />
                    </div>
                    <div className="space-y-1.5">
                      <Label
                        htmlFor="agent-role-system-prompt"
                        className="text-xs"
                      >
                        {t("fields.systemPrompt")}
                      </Label>
                      <Textarea
                        id="agent-role-system-prompt"
                        value={draftSystemPrompt}
                        onChange={(event) =>
                          setDraftSystemPrompt(event.target.value)
                        }
                        placeholder={t("fields.systemPromptPlaceholder")}
                        className="min-h-[10rem]"
                      />
                    </div>
                    <div className="space-y-1.5">
                      <Label className="text-xs">
                        {t("fields.targetAgent")}
                      </Label>
                      <Select
                        value={draftAgentType}
                        onValueChange={setDraftAgentType}
                      >
                        <SelectTrigger className="w-full">
                          <SelectValue
                            placeholder={t("fields.anyAgent")}
                          />
                        </SelectTrigger>
                        <SelectContent>
                          {AGENT_TYPES.map((at) => (
                            <SelectItem key={at.value} value={at.value}>
                              {"labelKey" in at
                                ? t(`fields.${at.labelKey}`)
                                : at.label}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-1.5">
                      <Label htmlFor="agent-role-model" className="text-xs">
                        {t("fields.model")}
                      </Label>
                      <Input
                        id="agent-role-model"
                        value={draftModelId}
                        onChange={(event) => setDraftModelId(event.target.value)}
                        placeholder={t("fields.modelPlaceholder")}
                      />
                    </div>
                    <div className="space-y-1.5">
                      <div className="flex items-center justify-between">
                        <Label className="text-xs">
                          {t("fields.temperature")}
                        </Label>
                        <span className="text-xs text-muted-foreground tabular-nums">
                          {draftTemperature.toFixed(1)}
                        </span>
                      </div>
                      <Slider
                        value={[draftTemperature]}
                        onValueChange={([value]) => setDraftTemperature(value)}
                        min={0}
                        max={2}
                        step={0.1}
                      />
                    </div>
                    <div className="space-y-1.5">
                      <Label
                        htmlFor="agent-role-tool-restrictions"
                        className="text-xs"
                      >
                        {t("fields.toolRestrictions")}
                      </Label>
                      <Input
                        id="agent-role-tool-restrictions"
                        value={draftToolsJson}
                        onChange={(event) =>
                          setDraftToolsJson(event.target.value)
                        }
                        placeholder={t("fields.toolRestrictionsPlaceholder")}
                      />
                    </div>
                  </div>
                  <div className="border-t px-4 py-3 flex items-center justify-between gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setDeleteTargetId(selectedRole.id)}
                      className="text-red-500 hover:text-red-500"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                      {t("actions.delete")}
                    </Button>
                    <Button
                      size="sm"
                      onClick={() => {
                        handleSave().catch((err) => {
                          console.error(
                            "[AgentRolesSettings] save failed:",
                            err
                          )
                        })
                      }}
                      disabled={saving || !isDirty}
                    >
                      {saving ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Save className="h-3.5 w-3.5" />
                      )}
                      {t("actions.save")}
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="h-full flex items-center justify-center text-xs text-muted-foreground">
                  {t("emptySelection")}
                </div>
              )}
            </div>
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>

      <AlertDialog
        open={deleteTargetId !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTargetId(null)
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("confirmDelete.title")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("confirmDelete.message", {
                name: deleteTargetRole?.name || t("untitled"),
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>
              {t("confirmDelete.cancel")}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(event) => {
                event.preventDefault()
                handleDelete().catch((err) => {
                  console.error("[AgentRolesSettings] delete failed:", err)
                })
              }}
              disabled={deleting}
            >
              {deleting ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : null}
              {t("confirmDelete.confirm")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
