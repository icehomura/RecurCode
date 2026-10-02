"use client"

import dynamic from "next/dynamic"
import { Loader2 } from "lucide-react"
import { WorkbenchPageTitle } from "@/components/workbench/workbench-page-title"

const TaskTreeView = dynamic(() => import("./task-tree-view"), {
  ssr: false,
  loading: () => (
    <div className="flex h-full items-center justify-center">
      <Loader2 className="size-5 animate-spin text-muted-foreground/60" />
    </div>
  ),
})

export function TaskTreePage() {
  return (
    <div className="h-full min-h-0 w-full">
      <TaskTreeView />
    </div>
  )
}

export function TaskTreePageTitle() {
  return <WorkbenchPageTitle title="Task Tree" />
}
