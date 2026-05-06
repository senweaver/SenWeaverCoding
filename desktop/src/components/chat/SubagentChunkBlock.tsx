type Props = {
  agentId: string
  delta: string
  chunkKind: string
  taskId?: string
}

export function SubagentChunkBlock({ agentId, delta, chunkKind }: Props) {
  const isThinking = chunkKind === 'Thinking'
  const isStatus = chunkKind === 'Status'

  if (isStatus) {
    return (
      <div className="mb-1 ml-6 text-[10px] italic text-[var(--color-text-tertiary)]">
        [{agentId}] {delta}
      </div>
    )
  }

  return (
    <div className="mb-1 ml-6 rounded-md border border-[var(--color-border)]/40 bg-[var(--color-surface-container-lowest)]/70 px-3 py-1.5">
      <div className="flex items-baseline gap-1.5">
        <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] font-semibold text-[var(--color-text-secondary)]">
          {agentId}
        </span>
        <span
          className={`min-w-0 flex-1 whitespace-pre-wrap break-words font-[var(--font-mono)] text-[11px] leading-[1.45] ${
            isThinking
              ? 'italic text-[var(--color-text-tertiary)]'
              : 'text-[var(--color-text-secondary)]'
          }`}
        >
          {delta}
        </span>
      </div>
    </div>
  )
}
