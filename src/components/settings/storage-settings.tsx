"use client"

import { useCallback, useEffect, useState } from "react"
import { Loader2, Trash2, FolderTree, RefreshCw } from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
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
import { projectStructureDeleteAll, projectStructureList } from "@/lib/api"
import type { ProjectStructureNode } from "@/lib/types"
import { toErrorMessage } from "@/lib/app-error"

export default function StorageSettings() {
  const t = useTranslations("StorageSettings")

  const [folders, setFolders] = useState<
    Array<{ folderId: number; nodeCount: number }>
  >([])
  const [loading, setLoading] = useState(true)
  const [deleteTarget, setDeleteTarget] = useState<number | null>(null)
  const [deleting, setDeleting] = useState(false)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      // Discover which folders have project structure data by checking known
      // folder IDs from the sidebar. For now, show a message that data
      // management happens from the Project View itself.
      setFolders([])
    } catch {
      setFolders([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const handleDelete = useCallback(async () => {
    if (deleteTarget === null) return
    setDeleting(true)
    try {
      await projectStructureDeleteAll(deleteTarget)
      toast.success(t("toasts.cleared"))
      setFolders((prev) => prev.filter((f) => f.folderId !== deleteTarget))
      setDeleteTarget(null)
    } catch (err) {
      toast.error(t("toasts.clearFailed"), {
        description: toErrorMessage(err),
      })
    } finally {
      setDeleting(false)
    }
  }, [deleteTarget, t])

  return (
    <div className="h-full flex flex-col p-3 md:p-4">
      <div className="flex items-center justify-between gap-3 pb-4">
        <div>
          <h2 className="text-base font-semibold">{t("title")}</h2>
          <p className="text-xs text-muted-foreground mt-1">
            {t("description")}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            void refresh()
          }}
          disabled={loading}
        >
          {loading ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
        </Button>
      </div>

      <div className="flex-1 min-h-0">
        {loading ? (
          <div className="flex items-center justify-center text-sm text-muted-foreground h-32">
            <Loader2 className="h-4 w-4 mr-2 animate-spin" />
            {t("loading")}
          </div>
        ) : folders.length === 0 ? (
          <div className="flex flex-col items-center justify-center text-sm text-muted-foreground h-32 gap-3">
            <FolderTree className="h-8 w-8 text-muted-foreground/40" />
            <p>{t("emptyHint")}</p>
            <p className="text-xs text-muted-foreground/60">
              {t("emptyDetail")}
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {folders.map((f) => (
              <div
                key={f.folderId}
                className="flex items-center justify-between rounded-lg border bg-card px-4 py-3"
              >
                <div className="flex items-center gap-3">
                  <FolderTree className="h-4 w-4 text-muted-foreground" />
                  <div>
                    <div className="text-sm font-medium">
                      Folder #{f.folderId}
                    </div>
                    <div className="text-xs text-muted-foreground">
                      {t("nodeCount", { count: f.nodeCount })}
                    </div>
                  </div>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-destructive hover:text-destructive"
                  onClick={() => setDeleteTarget(f.folderId)}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>

      <AlertDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null)
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("confirmDelete.title")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("confirmDelete.message")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>
              {t("confirmDelete.cancel")}
            </AlertDialogCancel>
            <AlertDialogAction onClick={() => void handleDelete()} disabled={deleting}>
              {deleting ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin mr-1" />
              ) : null}
              {t("confirmDelete.confirm")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
