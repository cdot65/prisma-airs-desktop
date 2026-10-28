import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from 'react'
import { useShellStore } from './store'

interface ModalProps {
    label: string
    onClose: () => void
    children: ReactNode
    /** Width in CSS px (default 560). */
    width?: number
    'data-slot'?: string
}

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

/**
 * One topmost modal (I8): dialogs register on a shell-wide stack; only the
 * last is interactive. Lower dialogs are inert and aria-hidden, Tab and
 * Shift-Tab stay inside the topmost dialog, Escape closes the topmost only,
 * focus moves into the dialog on open and back to the opener on close. The
 * page behind every dialog is made inert by the shell (`useModalsOpen`).
 * No global key listener: the dialog element handles its own keys.
 */
export function Modal({ label, onClose, children, width = 560, ...rest }: ModalProps) {
    const id = useId()
    const pushModal = useShellStore((s) => s.pushModal)
    const popModal = useShellStore((s) => s.popModal)
    const modals = useShellStore((s) => s.modals)
    const topmost = modals.length > 0 && modals[modals.length - 1] === id
    const dialog = useRef<HTMLDivElement>(null)
    const opener = useRef<HTMLElement | null>(null)
    const focusedOnce = useRef(false)

    useEffect(() => {
        opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
        pushModal(id)
        return () => {
            popModal(id)
            // After the commit: the page behind is still inert during cleanup,
            // and an inert ancestor refuses focus (observed in WebKit).
            const target = opener.current
            queueMicrotask(() => {
                // Only when nothing else took focus meanwhile (⌘T replacing the
                // palette hands focus to the tenant list, which must keep it).
                const active = document.activeElement
                if (!active || active === document.body) target?.focus()
            })
        }
    }, [id, pushModal, popModal])

    useEffect(() => {
        // Initial focus goes into the dialog once, when it opens. Regaining the
        // top after a higher dialog closes must not refocus: that dialog is
        // restoring its own opener, which may be any control here.
        if (!topmost || focusedOnce.current) return
        const node = dialog.current
        if (!node) return
        focusedOnce.current = true
        if (!node.contains(document.activeElement)) {
            const first = node.querySelector<HTMLElement>('[autofocus], ' + FOCUSABLE)
            ;(first ?? node).focus()
        }
    }, [topmost])

    const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        if (!topmost) return
        if (event.key === 'Escape') {
            event.preventDefault()
            event.stopPropagation()
            onClose()
            return
        }
        if (event.key === 'Tab') {
            // Keyboard interaction stays inside the topmost dialog.
            const node = dialog.current
            if (!node) return
            const items = Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => !el.hasAttribute('hidden') && el.getAttribute('aria-hidden') !== 'true')
            if (items.length === 0) {
                event.preventDefault()
                node.focus()
                return
            }
            const first = items[0] as HTMLElement
            const last = items[items.length - 1] as HTMLElement
            const active = document.activeElement
            if (event.shiftKey && (active === first || active === node)) {
                event.preventDefault()
                last.focus()
            } else if (!event.shiftKey && active === last) {
                event.preventDefault()
                first.focus()
            }
        }
    }

    return (
        <div
            className='fixed inset-0 z-50 grid place-items-start justify-center pt-[4vh]'
            data-v2-scrim=''
            aria-hidden={topmost ? undefined : true}
            inert={topmost ? undefined : true}
            onMouseDown={() => topmost && onClose()}
        >
            <div
                ref={dialog}
                role='dialog'
                aria-modal={topmost ? 'true' : undefined}
                aria-label={label}
                tabIndex={-1}
                data-slot={rest['data-slot'] ?? 'modal'}
                data-topmost={topmost ? 'true' : 'false'}
                onKeyDown={onKeyDown}
                onMouseDown={(e) => e.stopPropagation()}
                className='rounded-[12px] border p-5 shadow-2xl'
                // Bounded to the viewport: a tall dialog scrolls inside itself, so its
                // actions stay reachable at the minimum window size and 200% zoom.
                style={{ width: `min(${width}px, 92vw)`, maxHeight: '92vh', overflowY: 'auto', background: 'var(--v2-paper)', borderColor: 'var(--v2-rule)', color: 'var(--v2-ink)' }}
            >
                {children}
            </div>
        </div>
    )
}

/** Whether any modal is open: the shell makes the page behind it inert. */
export function useModalsOpen(): boolean {
    return useShellStore((s) => s.modals.length > 0)
}
