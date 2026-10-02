"use client"

import dynamic from "next/dynamic"
import { Loader2 } from "lucide-react"
import { WorkbenchPageTitle } from "@/components/workbench/workbench-page-title"

const ProjectView = dynamic(() => import("./project-view"), {
  ssr: false,
  loading: () => (
    <div className="flex h-full items-center justify-center">
      <Loader2 className="size-5 animate-spin text-muted-foreground/60" />
    </div>
  ),
})

export function ProjectViewPage() {
  return (
    <div className="h-full min-h-0 w-full">
      <ProjectView />
    </div>
  )
}

export function ProjectViewPageTitle() {
  return <WorkbenchPageTitle title="Project View" />
}
