"use client"

import { ComputerMarker } from "@/components/computer/computer-marker"

// Loaded inside the desktop-only `computer-marker` window, which RecurCode moves
// onto the spot where an agent's action just landed.
export default function ComputerMarkerPage() {
  return <ComputerMarker />
}
