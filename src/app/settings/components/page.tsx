"use client"

import { Suspense } from "react"
import { useTranslations } from "next-intl"
import { ComponentsSettings } from "@/components/settings/components-settings"

export default function SettingsComponentsPage() {
  const t = useTranslations("SettingsPages")

  return (
    <Suspense
      fallback={
        <div className="h-full flex items-center justify-center text-sm text-muted-foreground">
          {t("componentsLoading")}
        </div>
      }
    >
      <ComponentsSettings />
    </Suspense>
  )
}
