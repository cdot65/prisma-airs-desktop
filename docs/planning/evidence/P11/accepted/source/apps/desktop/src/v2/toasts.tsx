import { useEffect } from 'react'
import { CircleAlert, CircleCheck, Info } from 'lucide-react'
import { SHELL_COPY } from './copy'
import { useShellStore, type Toast } from './store'

const reducedMotion = () =>
    typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches

const ICONS = { ok: CircleCheck, info: Info, warn: CircleAlert } as const
const COLORS = { ok: 'var(--v2-ok)', info: 'var(--v2-get)', warn: 'var(--v2-flag)' } as const

/**
 * The one status surface's transient layer (I10): kinds ok/info/warn with
 * icon and word, a 6 s expiry, and persistence until dismissed when the OS
 * asks for reduced motion. Everything a toast says is derivable from an
 * observed scope; a toast never carries authority.
 */
export function Toasts() {
    const toasts = useShellStore((s) => s.toasts)
    const scope = useShellStore((s) => s.scope)
    const dismiss = useShellStore((s) => s.dismissToast)
    return (
        <div data-slot='toasts' className='pointer-events-none fixed inset-x-0 bottom-6 z-40 flex justify-center px-4'>
            {/* Only what speaks for the shown tenant (I2), whatever reached the store. */}
            {toasts.filter((t) => (t.tenant === null || t.tenant === scope.tenant) && (t.context === null || t.context === scope.context)).map((toast) => (
                <ToastView key={toast.id} toast={toast} onDismiss={() => dismiss(toast.id)} />
            ))}
        </div>
    )
}

function ToastView({ toast, onDismiss }: { toast: Toast; onDismiss: () => void }) {
    // The deadline is the toast's, not the render's: re-renders (snapshot
    // updates) never restart it.
    useEffect(() => {
        if (reducedMotion()) return
        const timer = window.setTimeout(() => useShellStore.getState().dismissToast(toast.id), useShellStore.getState().toastMs)
        return () => window.clearTimeout(timer)
    }, [toast.id])
    const Icon = ICONS[toast.kind]
    return (
        <div
            role='status'
            aria-live='polite'
            data-slot='toast'
            data-kind={toast.kind}
            className='pointer-events-auto flex max-w-[640px] items-center gap-3 rounded-[12px] border px-4 py-3 text-[13px] shadow-lg'
            style={{ background: 'var(--v2-paper)', borderColor: 'var(--v2-rule)', color: 'var(--v2-ink)' }}
        >
            <Icon aria-hidden='true' size={16} strokeWidth={2.4} style={{ color: COLORS[toast.kind] }} />
            <span className='sr-only'>{SHELL_COPY.toastPrefix[toast.kind]}</span>
            <span className='flex-grow'>{toast.text}</span>
            {toast.undo && (
                <button
                    type='button'
                    onClick={() => {
                        toast.undo?.run()
                        onDismiss()
                    }}
                    className='h-8 rounded-[7px] border px-3 text-[12px] font-semibold'
                    style={{ borderColor: 'var(--v2-rule)' }}
                >
                    {toast.undo.label}
                </button>
            )}
            <button type='button' aria-label='Dismiss' onClick={onDismiss} className='h-8 w-8 rounded-[7px] text-[16px] leading-none'>
                ×
            </button>
        </div>
    )
}
