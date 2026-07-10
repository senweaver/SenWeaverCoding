// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { MinimalBar } from './components/minimal/MinimalBar'
import { ComputerBar } from './components/minimal/computer/ComputerBar'
import { useMinimalStore } from './stores/minimalStore'
import { useMinimalWindowBridge } from './hooks/useMinimalWindowBridge'

export function MinimalApp() {
  useMinimalWindowBridge()
  return <MinimalRoot />
}

function MinimalRoot() {
  const variant = useMinimalStore((s) => s.variant)
  return variant === 'computer' ? <ComputerBar /> : <MinimalBar />
}
