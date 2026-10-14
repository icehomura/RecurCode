"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { useTranslations } from "next-intl"
import {
  CheckCircle2,
  Cpu,
  Download,
  HardDrive,
  Loader2,
  Plus,
  RotateCw,
  Save,
  Server,
  Trash2,
  XCircle,
} from "lucide-react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import {
  acpDetectAgentLocalVersion,
  acpDownloadAgentBinary,
  acpLoadRecurConfig,
  acpUninstallAgent,
  acpUpdateRecurConfig,
  type RecurConfigProjection,
  type RecurConfigUpdate,
} from "@/lib/api"
import { useAgentInstallStream } from "@/hooks/use-agent-install-stream"
import type { AcpAgentInfo } from "@/lib/types"
import { cn, randomUUID } from "@/lib/utils"

/** Coerce a nullable projection string to a controlled-input value. */
function toStr(value: string | null): string {
  return value ?? ""
}

/** Coerce a nullable projection number to a controlled-input string. */
function toNumStr(value: number | null): string {
  return value === null ? "" : String(value)
}

/** Serialize an env map into `KEY=value` lines for the raw editor. */
function envMapToText(env: Record<string, string>): string {
  return Object.entries(env)
    .map(([key, value]) => `${key}=${value}`)
    .join("\n")
}

/** Parse `KEY=value` lines back into an env map, skipping blanks/comments. */
function parseEnvText(text: string): Record<string, string> {
  const map: Record<string, string> = {}
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim()
    if (!line || line.startsWith("#")) continue
    const idx = line.indexOf("=")
    if (idx <= 0) continue
    const key = line.slice(0, idx).trim()
    const value = line.slice(idx + 1).trim()
    if (!key) continue
    map[key] = value
  }
  return map
}

/** A raw MCP server entry paired with its editable JSON text. */
interface McpDraft {
  /** Stable React key (not persisted). */
  key: string
  /** JSON serialization of the entry, edited in place. */
  text: string
}

/** Build the line label for an MCP entry: its `command` or `url`. */
function mcpLabel(entry: Record<string, unknown>): string {
  const command = typeof entry.command === "string" ? entry.command : ""
  const url = typeof entry.url === "string" ? entry.url : ""
  return command || url || "(unnamed)"
}

/**
 * Settings panel for the RecurAgent (`ra`) proxy. Bound to RecurAgent's native
 * config via `acpLoadRecurConfig` / `acpUpdateRecurConfig`, split into five
 * concerns:
 *  - Model & provider — flat model-config keys written to config.json.
 *  - Profile — selects the active serve-profile pointer.
 *  - MCP servers — raw JSON entries round-tripped verbatim.
 *  - Config location — read-only path + exists indicator.
 *  - Environment — the per-agent `env_json` (API keys / `RA_*` vars) RecurAgent
 *    reads from the launched process env.
 *  - Install — the `ra` binary, managed through the generic agent-binary API.
 */
export function RecurConfigPanel({
  agent,
  saving,
  onSaveEnv,
  onSaved,
}: {
  agent: AcpAgentInfo
  saving: boolean
  onSaveEnv: (env: Record<string, string>, enabled: boolean) => Promise<unknown>
  onSaved: () => Promise<void>
}) {
  const t = useTranslations("AcpAgentSettings")

  const mountedRef = useRef(true)
  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  // --- config projection (source of truth for the baseline diff) ---
  const [projection, setProjection] = useState<RecurConfigProjection | null>(
    null
  )
  const [loading, setLoading] = useState(true)

  // --- model & provider form ---
  const [provider, setProvider] = useState("")
  const [model, setModel] = useState("")
  const [baseUrl, setBaseUrl] = useState("")
  const [apiType, setApiType] = useState("")
  const [apiKeyEnv, setApiKeyEnv] = useState("")
  const [contextWindow, setContextWindow] = useState("")
  const [maxIterations, setMaxIterations] = useState("")
  const [formatAfterEdit, setFormatAfterEdit] = useState(false)
  const [savingModel, setSavingModel] = useState(false)

  // --- profile ---
  const [activeProfile, setActiveProfile] = useState("")
  const [savingProfile, setSavingProfile] = useState(false)

  // --- MCP servers (raw JSON drafts) ---
  const [mcpDrafts, setMcpDrafts] = useState<McpDraft[]>([])
  const [savingMcp, setSavingMcp] = useState(false)

  // --- environment (per-agent env_json, raw KEY=value editor) ---
  const [envText, setEnvText] = useState(() => envMapToText(agent.env))
  const [savingEnv, setSavingEnv] = useState(false)

  // --- binary ---
  const installStream = useAgentInstallStream()
  const {
    status: installStatus,
    logs: installLogs,
    start: startInstall,
    reset: resetInstall,
  } = installStream
  const installLogEndRef = useRef<HTMLDivElement | null>(null)
  const [binaryVersion, setBinaryVersion] = useState<string | null>(null)
  const [binaryKnown, setBinaryKnown] = useState(false)
  const [checkingBinary, setCheckingBinary] = useState(true)
  const [binaryOp, setBinaryOp] = useState<"install" | "uninstall" | null>(null)

  // Populate the form from a freshly loaded projection.
  const applyProjection = useCallback((proj: RecurConfigProjection) => {
    setProjection(proj)
    setProvider(toStr(proj.model.provider))
    setModel(toStr(proj.model.model))
    setBaseUrl(toStr(proj.model.baseUrl))
    setApiType(toStr(proj.model.apiType))
    setApiKeyEnv(toStr(proj.model.apiKeyEnv))
    setContextWindow(toNumStr(proj.model.contextWindow))
    setMaxIterations(toNumStr(proj.model.maxIterations))
    setFormatAfterEdit(proj.model.formatAfterEdit === true)
    setActiveProfile(proj.activeProfile)
    setMcpDrafts(
      proj.mcpServers.map((entry, i) => ({
        key: `${i}-${randomUUID()}`,
        text: JSON.stringify(entry, null, 2),
      }))
    )
  }, [])

  useEffect(() => {
    acpLoadRecurConfig()
      .then((proj) => {
        if (mountedRef.current) applyProjection(proj)
      })
      .catch((error) => {
        console.error("[RecurAgent] load config failed", error)
        if (mountedRef.current) toast.error(t("recur_agent.loadFailed"))
      })
      .finally(() => {
        if (mountedRef.current) setLoading(false)
      })
  }, [applyProjection, t])

  // Detect the installed `ra` binary version once on mount.
  const detectBinary = useCallback(async () => {
    setCheckingBinary(true)
    try {
      const version = await acpDetectAgentLocalVersion(agent.agent_type)
      if (!mountedRef.current) return
      setBinaryVersion(version)
      setBinaryKnown(true)
    } catch (error) {
      console.error("[RecurAgent] detect binary failed", error)
      if (mountedRef.current) {
        setBinaryVersion(null)
        setBinaryKnown(true)
      }
    } finally {
      if (mountedRef.current) setCheckingBinary(false)
    }
  }, [agent.agent_type])

  useEffect(() => {
    void detectBinary()
  }, [detectBinary])

  // Tear down the install-event subscription on unmount (reset is stable).
  useEffect(() => () => resetInstall(), [resetInstall])

  // Keep the streaming log pinned to its latest line.
  useEffect(() => {
    const container = installLogEndRef.current?.parentElement
    if (container) container.scrollTop = container.scrollHeight
  }, [installLogs])

  // --- model save: send only the fields that changed vs the baseline ---
  const handleSaveModel = useCallback(async () => {
    if (!projection) return
    const base = projection.model
    const update: RecurConfigUpdate = {}

    // Provider/model/endpoint/protocol/env-name: only send changed, trimmed
    // values; an empty field clears it (null).
    if (provider.trim() !== toStr(base.provider))
      update.provider = provider.trim() || null
    if (model.trim() !== toStr(base.model))
      update.model = model.trim() || null
    if (baseUrl.trim() !== toStr(base.baseUrl))
      update.baseUrl = baseUrl.trim() || null
    if (apiType.trim() !== toStr(base.apiType))
      update.apiType = apiType.trim() || null
    if (apiKeyEnv.trim() !== toStr(base.apiKeyEnv))
      update.apiKeyEnv = apiKeyEnv.trim() || null

    // Numbers: empty means null (unset), otherwise the parsed value.
    const ctx = contextWindow.trim()
    const baseCtx = toNumStr(base.contextWindow)
    if (ctx !== baseCtx) {
      update.contextWindow = ctx === "" ? null : Number(ctx) || null
    }
    const iters = maxIterations.trim()
    const baseIters = toNumStr(base.maxIterations)
    if (iters !== baseIters) {
      update.maxIterations = iters === "" ? null : Number(iters) || null
    }

    // Boolean is always explicit.
    if (formatAfterEdit !== (base.formatAfterEdit === true))
      update.formatAfterEdit = formatAfterEdit

    if (Object.keys(update).length === 0) {
      await onSaved()
      return
    }

    setSavingModel(true)
    try {
      await acpUpdateRecurConfig(update)
      await onSaved()
      toast.success(t("recur_agent.saved"))
    } catch (error) {
      console.error("[RecurAgent] save config failed", error)
      toast.error(t("recur_agent.updateFailed"))
    } finally {
      if (mountedRef.current) setSavingModel(false)
    }
  }, [
    projection,
    provider,
    model,
    baseUrl,
    apiType,
    apiKeyEnv,
    contextWindow,
    maxIterations,
    formatAfterEdit,
    onSaved,
    t,
  ])

  // --- profile save ---
  const handleProfileChange = useCallback(
    async (value: string) => {
      if (!projection || value === activeProfile) return
      setSavingProfile(true)
      try {
        await acpUpdateRecurConfig({ activeProfile: value })
        setActiveProfile(value)
        await onSaved()
        toast.success(t("recur_agent.saved"))
      } catch (error) {
        console.error("[RecurAgent] save profile failed", error)
        toast.error(t("recur_agent.updateFailed"))
      } finally {
        if (mountedRef.current) setSavingProfile(false)
      }
    },
    [projection, activeProfile, onSaved, t]
  )

  // --- MCP draft mutations ---
  const handleMcpTextChange = useCallback((key: string, text: string) => {
    setMcpDrafts((prev) =>
      prev.map((draft) => (draft.key === key ? { ...draft, text } : draft))
    )
  }, [])

  const handleMcpAdd = useCallback(() => {
    setMcpDrafts((prev) => [
      ...prev,
      {
        key: randomUUID(),
        text: JSON.stringify({ command: "", args: [] }, null, 2),
      },
    ])
  }, [])

  const handleMcpRemove = useCallback((key: string) => {
    setMcpDrafts((prev) => prev.filter((draft) => draft.key !== key))
  }, [])

  const handleSaveMcp = useCallback(async () => {
    const entries: Record<string, unknown>[] = []
    for (const draft of mcpDrafts) {
      let parsed: unknown
      try {
        parsed = JSON.parse(draft.text)
      } catch {
        toast.error(t("recur_agent.mcpInvalidJson"))
        return
      }
      if (
        typeof parsed !== "object" ||
        parsed === null ||
        Array.isArray(parsed)
      ) {
        toast.error(t("recur_agent.mcpInvalidJson"))
        return
      }
      entries.push(parsed as Record<string, unknown>)
    }
    setSavingMcp(true)
    try {
      await acpUpdateRecurConfig({ mcpServers: entries })
      await onSaved()
      toast.success(t("recur_agent.saved"))
    } catch (error) {
      console.error("[RecurAgent] save mcp servers failed", error)
      toast.error(t("recur_agent.updateFailed"))
    } finally {
      if (mountedRef.current) setSavingMcp(false)
    }
  }, [mcpDrafts, onSaved, t])

  // --- environment save ---
  const handleSaveEnv = useCallback(async () => {
    setSavingEnv(true)
    try {
      await onSaveEnv(parseEnvText(envText), agent.enabled)
      await onSaved()
      toast.success(t("recur_agent.saved"))
    } catch (error) {
      console.error("[RecurAgent] save env failed", error)
      toast.error(t("recur_agent.updateFailed"))
    } finally {
      if (mountedRef.current) setSavingEnv(false)
    }
  }, [envText, onSaveEnv, agent.enabled, onSaved, t])

  // --- binary install / uninstall ---
  const handleInstall = useCallback(async () => {
    const taskId = randomUUID()
    setBinaryOp("install")
    await startInstall(taskId)
    try {
      await acpDownloadAgentBinary(agent.agent_type, taskId)
      await detectBinary()
    } catch (error) {
      console.error("[RecurAgent] install binary failed", error)
    } finally {
      if (mountedRef.current) setBinaryOp(null)
    }
  }, [agent.agent_type, startInstall, detectBinary])

  const handleUninstall = useCallback(async () => {
    const taskId = randomUUID()
    setBinaryOp("uninstall")
    await startInstall(taskId)
    try {
      await acpUninstallAgent(agent.agent_type, taskId)
      await detectBinary()
    } catch (error) {
      console.error("[RecurAgent] uninstall binary failed", error)
    } finally {
      if (mountedRef.current) setBinaryOp(null)
    }
  }, [agent.agent_type, startInstall, detectBinary])

  const profiles = projection?.profiles ?? []
  const installed = binaryVersion !== null

  return (
    <div className="space-y-4">
      {/* 1. Model & provider */}
      <div className="space-y-3 rounded-md border bg-muted/10 p-3">
        <div>
          <label className="flex items-center gap-1.5 text-xs font-medium">
            <Cpu className="h-3.5 w-3.5 text-muted-foreground" />
            {t("recur_agent.modelSectionTitle")}
          </label>
          <p className="mt-1 text-2xs text-muted-foreground">
            {t("recur_agent.configDescription")}
          </p>
        </div>

        <div className="grid grid-cols-2 gap-2">
          <div className="space-y-1.5">
            <label className="text-2xs text-muted-foreground">
              {t("recur_agent.providerLabel")}
            </label>
            <Input
              value={provider}
              onChange={(event) => setProvider(event.target.value)}
              placeholder="anthropic"
              spellCheck={false}
              disabled={loading || savingModel}
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-2xs text-muted-foreground">
              {t("recur_agent.modelLabel")}
            </label>
            <Input
              value={model}
              onChange={(event) => setModel(event.target.value)}
              placeholder="claude-sonnet-5"
              spellCheck={false}
              disabled={loading || savingModel}
            />
          </div>
        </div>

        <div className="space-y-1.5">
          <label className="text-2xs text-muted-foreground">
            {t("recur_agent.baseUrlLabel")}
          </label>
          <Input
            value={baseUrl}
            onChange={(event) => setBaseUrl(event.target.value)}
            placeholder="https://api.example.com/v1"
            spellCheck={false}
            disabled={loading || savingModel}
          />
        </div>

        <div className="grid grid-cols-2 gap-2">
          <div className="space-y-1.5">
            <label className="text-2xs text-muted-foreground">
              {t("recur_agent.apiTypeLabel")}
            </label>
            <Input
              value={apiType}
              onChange={(event) => setApiType(event.target.value)}
              placeholder="openai-completions"
              spellCheck={false}
              disabled={loading || savingModel}
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-2xs text-muted-foreground">
              {t("recur_agent.apiKeyEnvLabel")}
            </label>
            <Input
              value={apiKeyEnv}
              onChange={(event) => setApiKeyEnv(event.target.value)}
              placeholder="ANTHROPIC_API_KEY"
              spellCheck={false}
              disabled={loading || savingModel}
            />
            <p className="text-3xs text-muted-foreground">
              {t("recur_agent.apiKeyEnvHint")}
            </p>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-2">
          <div className="space-y-1.5">
            <label className="text-2xs text-muted-foreground">
              {t("recur_agent.contextWindowLabel")}
            </label>
            <Input
              type="number"
              value={contextWindow}
              onChange={(event) => setContextWindow(event.target.value)}
              placeholder="200000"
              disabled={loading || savingModel}
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-2xs text-muted-foreground">
              {t("recur_agent.maxIterationsLabel")}
            </label>
            <Input
              type="number"
              value={maxIterations}
              onChange={(event) => setMaxIterations(event.target.value)}
              placeholder="30"
              disabled={loading || savingModel}
            />
          </div>
        </div>

        <div className="flex items-center justify-between gap-2">
          <label className="text-2xs text-muted-foreground">
            {t("recur_agent.formatAfterEditLabel")}
          </label>
          <Switch
            checked={formatAfterEdit}
            onCheckedChange={setFormatAfterEdit}
            disabled={loading || savingModel}
            aria-label={t("recur_agent.formatAfterEditLabel")}
          />
        </div>

        <div className="flex justify-end">
          <Button
            type="button"
            size="sm"
            onClick={handleSaveModel}
            disabled={loading || savingModel}
            className="gap-1.5"
          >
            {savingModel ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("actions.saving")}
              </>
            ) : (
              <>
                <Save className="h-3.5 w-3.5" />
                {t("recur_agent.saveModel")}
              </>
            )}
          </Button>
        </div>
      </div>

      {/* 2. Profile */}
      <div className="space-y-3 rounded-md border bg-muted/10 p-3">
        <div>
          <label className="text-xs font-medium">
            {t("recur_agent.profileLabel")}
          </label>
          <p className="mt-1 text-2xs text-muted-foreground">
            {t("recur_agent.profileHint")}
          </p>
        </div>

        {profiles.length === 0 ? (
          <p className="text-2xs text-muted-foreground">
            {t("recur_agent.profileNone")}
          </p>
        ) : (
          <Select
            value={activeProfile}
            onValueChange={handleProfileChange}
            disabled={loading || savingProfile}
          >
            <SelectTrigger className="w-full">
              <SelectValue placeholder={t("recur_agent.profileNone")} />
            </SelectTrigger>
            <SelectContent align="start">
              {profiles.map((profile) => (
                <SelectItem key={profile.id} value={profile.id}>
                  {profile.name || profile.id}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
      </div>

      {/* 3. MCP servers */}
      <div className="space-y-3 rounded-md border bg-muted/10 p-3">
        <div>
          <label className="flex items-center gap-1.5 text-xs font-medium">
            <Server className="h-3.5 w-3.5 text-muted-foreground" />
            {t("recur_agent.mcpServersTitle")}
          </label>
          <p className="mt-1 text-2xs text-muted-foreground">
            {t("recur_agent.mcpServersHint")}
          </p>
        </div>

        {mcpDrafts.length === 0 ? (
          <p className="text-2xs text-muted-foreground">
            {t("recur_agent.mcpServersHint")}
          </p>
        ) : (
          <ul className="space-y-2">
            {mcpDrafts.map((draft) => {
              let label = "(unnamed)"
              try {
                label = mcpLabel(JSON.parse(draft.text))
              } catch {
                label = t("recur_agent.mcpInvalidJson")
              }
              return (
                <li
                  key={draft.key}
                  className="space-y-1.5 rounded-md border border-dashed p-2"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span
                      className="min-w-0 flex-1 truncate font-mono text-2xs text-muted-foreground"
                      title={label}
                    >
                      {label}
                    </span>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      className="h-6 shrink-0 gap-1 px-2 text-2xs"
                      onClick={() => handleMcpRemove(draft.key)}
                      disabled={loading || savingMcp}
                    >
                      <Trash2 className="h-3 w-3" />
                      {t("recur_agent.mcpRemove")}
                    </Button>
                  </div>
                  <Textarea
                    value={draft.text}
                    onChange={(event) =>
                      handleMcpTextChange(draft.key, event.target.value)
                    }
                    spellCheck={false}
                    className="min-h-24 font-mono text-2xs"
                    disabled={loading || savingMcp}
                  />
                </li>
              )
            })}
          </ul>
        )}

        <div className="flex items-center justify-between gap-2">
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={handleMcpAdd}
            disabled={loading || savingMcp}
            className="gap-1.5"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("recur_agent.mcpAdd")}
          </Button>
          <Button
            type="button"
            size="sm"
            onClick={handleSaveMcp}
            disabled={loading || savingMcp}
            className="gap-1.5"
          >
            {savingMcp ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("actions.saving")}
              </>
            ) : (
              <>
                <Save className="h-3.5 w-3.5" />
                {t("recur_agent.saveModel")}
              </>
            )}
          </Button>
        </div>
      </div>

      {/* 4. Config location */}
      <div className="space-y-2 rounded-md border bg-muted/10 p-3">
        <label className="flex items-center gap-1.5 text-xs font-medium">
          <HardDrive className="h-3.5 w-3.5 text-muted-foreground" />
          {t("recur_agent.configPathLabel")}
        </label>
        {projection ? (
          <>
            <code className="block break-all rounded bg-muted px-2 py-1 font-mono text-2xs text-muted-foreground">
              {projection.configPath}
            </code>
            {projection.exists ? (
              <span className="flex items-center gap-1.5 text-2xs text-emerald-600">
                <CheckCircle2 className="h-3.5 w-3.5" />
                {t("recur_agent.saved")}
              </span>
            ) : (
              <span className="flex items-center gap-1.5 text-2xs text-amber-600 dark:text-amber-400">
                <XCircle className="h-3.5 w-3.5" />
                {t("recur_agent.configMissing")}
              </span>
            )}
          </>
        ) : (
          <p className="flex items-center gap-1.5 text-2xs text-muted-foreground">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            {t("recur_agent.binaryChecking")}
          </p>
        )}
      </div>

      {/* 5. Environment (per-agent env_json, raw KEY=value editor) */}
      <div className="space-y-2 rounded-md border bg-muted/10 p-3">
        <label className="text-xs font-medium">{t("envVars")}</label>
        <Textarea
          value={envText}
          onChange={(event) => setEnvText(event.target.value)}
          spellCheck={false}
          className="min-h-32 font-mono text-2xs"
          disabled={savingEnv}
        />
        <div className="flex justify-end">
          <Button
            type="button"
            size="sm"
            onClick={handleSaveEnv}
            disabled={saving || savingEnv}
            className="gap-1.5"
          >
            {savingEnv ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("actions.saving")}
              </>
            ) : (
              <>
                <Save className="h-3.5 w-3.5" />
                {t("actions.saveEnvVars")}
              </>
            )}
          </Button>
        </div>
      </div>

      {/* 6. Install (`ra` binary, generic agent-binary API) */}
      <div className="space-y-3 rounded-md border bg-muted/10 p-3">
        <div>
          <label className="flex items-center gap-1.5 text-xs font-medium">
            <Cpu className="h-3.5 w-3.5 text-muted-foreground" />
            {t("recur_agent.binaryTitle")}
          </label>
          <p className="mt-1 text-2xs text-muted-foreground">
            {t("recur_agent.binaryDescription")}
          </p>
        </div>

        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0 flex-1 text-2xs">
            {checkingBinary ? (
              <span className="flex items-center gap-1.5 text-muted-foreground">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("recur_agent.binaryChecking")}
              </span>
            ) : installed ? (
              <span className="flex items-start gap-1.5 text-emerald-600">
                <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                <span className="min-w-0 font-medium">
                  {t("recur_agent.binaryInstalled")}
                  {binaryVersion ? ` · ${binaryVersion}` : ""}
                </span>
              </span>
            ) : (
              <span className="flex items-center gap-1.5 text-muted-foreground">
                <XCircle className="h-3.5 w-3.5 shrink-0" />
                {t("recur_agent.binaryMissing")}
              </span>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => void detectBinary()}
              disabled={checkingBinary || binaryOp !== null}
              title={t("recur_agent.recheck")}
              className="h-7 px-2"
            >
              <RotateCw
                className={cn("h-3.5 w-3.5", checkingBinary && "animate-spin")}
              />
            </Button>
            {!checkingBinary &&
              (installed ? (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={handleUninstall}
                  disabled={binaryOp !== null}
                  className="gap-1.5"
                >
                  {binaryOp === "uninstall" ? (
                    <>
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      {t("actions.uninstalling")}
                    </>
                  ) : (
                    <>
                      <Trash2 className="h-3.5 w-3.5" />
                      {t("actions.uninstall")}
                    </>
                  )}
                </Button>
              ) : (
                <Button
                  type="button"
                  size="sm"
                  onClick={handleInstall}
                  disabled={binaryOp !== null || !binaryKnown}
                  className="gap-1.5"
                >
                  {binaryOp === "install" ? (
                    <>
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      {t("recur_agent.installing")}
                    </>
                  ) : (
                    <>
                      <Download className="h-3.5 w-3.5" />
                      {t("recur_agent.installBinary")}
                    </>
                  )}
                </Button>
              ))}
          </div>
        </div>

        {installStatus !== "idle" && (
          <div className="max-h-[12.5rem] overflow-y-auto rounded-md border bg-muted/50 p-3 font-mono text-2xs leading-relaxed text-muted-foreground">
            {installLogs.map((line, i) => (
              <div
                key={i}
                className={line.startsWith("ERROR:") ? "text-destructive" : ""}
              >
                {line}
              </div>
            ))}
            <div ref={installLogEndRef} />
          </div>
        )}
      </div>
    </div>
  )
}
