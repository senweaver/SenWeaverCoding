// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback } from 'react'

type Direction =
  | 'North'
  | 'South'
  | 'East'
  | 'West'
  | 'NorthEast'
  | 'NorthWest'
  | 'SouthEast'
  | 'SouthWest'

const GUTTER_PX = 18

const HANDLE_STYLES: Record<Direction, React.CSSProperties> = {

  North: {
    top: 0,
    left: GUTTER_PX,
    right: GUTTER_PX,
    height: GUTTER_PX,
    cursor: 'ns-resize',
  },
  South: {
    bottom: 0,
    left: GUTTER_PX,
    right: GUTTER_PX,
    height: GUTTER_PX,
    cursor: 'ns-resize',
  },
  West: {
    top: GUTTER_PX,
    bottom: GUTTER_PX,
    left: 0,
    width: GUTTER_PX,
    cursor: 'ew-resize',
  },
  East: {
    top: GUTTER_PX,
    bottom: GUTTER_PX,
    right: 0,
    width: GUTTER_PX,
    cursor: 'ew-resize',
  },

  NorthWest: {
    top: 0,
    left: 0,
    width: GUTTER_PX,
    height: GUTTER_PX,
    cursor: 'nwse-resize',
  },
  NorthEast: {
    top: 0,
    right: 0,
    width: GUTTER_PX,
    height: GUTTER_PX,
    cursor: 'nesw-resize',
  },
  SouthWest: {
    bottom: 0,
    left: 0,
    width: GUTTER_PX,
    height: GUTTER_PX,
    cursor: 'nesw-resize',
  },
  SouthEast: {
    bottom: 0,
    right: 0,
    width: GUTTER_PX,
    height: GUTTER_PX,
    cursor: 'nwse-resize',
  },
}

const DIRECTIONS: Direction[] = [
  'North',
  'South',
  'East',
  'West',
  'NorthWest',
  'NorthEast',
  'SouthWest',
  'SouthEast',
]

export function ResizeHandles({ disabled }: { disabled: boolean }) {
  const handleMouseDown = useCallback(
    async (direction: Direction, event: React.MouseEvent<HTMLDivElement>) => {

      if (event.button !== 0) {
        return
      }

      event.preventDefault()
      event.stopPropagation()
      try {
        const { getCurrentWindow } = await import(
          /* @vite-ignore */ '@tauri-apps/api/window'
        )
        await getCurrentWindow().startResizeDragging(direction)
      } catch {

      }
    },
    [],
  )

  if (disabled) {
    return null
  }

  return (
    <>
      {DIRECTIONS.map((dir) => (
        <div
          key={dir}
          aria-hidden
          data-resize-direction={dir}
          onMouseDown={(event) => {
            void handleMouseDown(dir, event)
          }}
          style={{
            position: 'fixed',
            ...HANDLE_STYLES[dir],

            zIndex: 100,
          }}
        />
      ))}
    </>
  )
}
