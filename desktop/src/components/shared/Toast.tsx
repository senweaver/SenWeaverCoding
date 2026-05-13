import { useUIStore, type Toast as ToastType } from '../../stores/uiStore'
import { useDockEdgeOffset } from '../../hooks/useDockEdgeOffset'

const typeStyles: Record<ToastType['type'], string> = {
  success: 'border-l-4 border-l-[var(--color-success)]',
  error: 'border-l-4 border-l-[var(--color-error)]',
  warning: 'border-l-4 border-l-[var(--color-warning)]',
  info: 'border-l-4 border-l-[var(--color-text-accent)]',
}

function ToastItem({ toast }: { toast: ToastType }) {
  const removeToast = useUIStore((s) => s.removeToast)

  return (
    <div
      className={`
        bg-[var(--color-surface)] rounded-[var(--radius-md)] shadow-[var(--shadow-dropdown)]
        px-4 py-3 text-sm text-[var(--color-text-primary)]
        ${typeStyles[toast.type]}
        animate-in slide-in-from-right fade-in duration-200
      `}
    >
      <div className="flex items-start justify-between gap-2">
        <span className="flex-1">{toast.message}</span>
        <button
          onClick={() => removeToast(toast.id)}
          className="text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] text-lg leading-none shrink-0"
        >
          ×
        </button>
      </div>
      {toast.action && (
        <div className="mt-2 flex justify-end">
          <button
            type="button"
            onClick={() => {
              toast.action?.onClick()
              removeToast(toast.id)
            }}
            className="rounded-[var(--radius-sm)] border border-[var(--color-border)] px-2.5 py-1 text-xs text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
          >
            {toast.action.label}
          </button>
        </div>
      )}
    </div>
  )
}

export function ToastContainer() {
  const toasts = useUIStore((s) => s.toasts)
  const rightInset = useDockEdgeOffset()

  if (toasts.length === 0) return null

  return (
    <div
      className="fixed bottom-4 z-[100] flex flex-col gap-2 max-w-sm"
      style={{ right: `${rightInset + 16}px` }}
    >
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </div>
  )
}
