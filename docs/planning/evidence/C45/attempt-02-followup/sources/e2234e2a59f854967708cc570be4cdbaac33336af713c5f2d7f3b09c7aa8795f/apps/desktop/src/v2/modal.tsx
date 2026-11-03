import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from 'react'
import { useShellStore } from './store'

interface ModalProps {
    label: string
    onClose: () => void
    children: ReactNode
    /** Width in CSS px (default 560). */
    width?: number
    /** Inner padding in CSS px (default 20); 0 for a dialog that lays itself out edge to edge. */
    padding?: number
    /** Distance from the top of the window in CSS px (default 4vh); the prototype's palette sits at 90. */
    top?: number
    /** Corner radius in CSS px (default 12; the palette's frame is 14). */
    radius?: number
    /** A sheet docked to the window's right edge under the header (R2App's review sheet, 620px wide, full height). */
    sheet?: boolean
    /**
     * The control focus returns to on close when the opener itself will be gone
     * by then (a menu item that closes with its menu as the dialog opens): an
     * explicit surviving target instead of whatever had focus at mount.
     */
    returnFocus?: HTMLElement | null
    'data-slot'?: string
}

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

/**
 * One topmost modal (I8): dialogs register on a shell-wide stack; only the
 * last is interactive. Lower dialogs are inert and aria-hidden, Tab and
 * Shift-Tab stay inside the topmost dialog, Escape closes the topmost only,
 * focus moves into the dialog on open and back to the opener on close. The
 * page behind every dialog is made inert by the shell (`useModalsOpen`).
 * The dialog element handles its own keys; the topmost dialog also listens
 * on the window for Escape alone, for when focus has left it (a native
 * sheet returned focus elsewhere), and closes only itself.
 */
export function Modal({ label, onClose, children, width = 560, padding = 20, top, radius = 12, sheet = false, returnFocus = null, ...rest }: ModalProps) {
    const id = useId()
    const pushModal = useShellStore((s) => s.pushModal)
    const popModal = useShellStore((s) => s.popModal)
    const modals = useShellStore((s) => s.modals)
    const topmost = modals.length > 0 && modals[modals.length - 1] === id
    const dialog = useRef<HTMLDivElement>(null)
    const opener = useRef<HTMLElement | null>(null)
    const returnRef = useRef(returnFocus)
    returnRef.current = returnFocus
    const focusedOnce = useRef(false)

    useEffect(() => {
        opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
        pushModal(id)
        return () => {
            popModal(id)
            // After the commit: the page behind is still inert during cleanup,
            // and an inert ancestor refuses focus (observed in WebKit).
            const target = returnRef.current ?? opener.current
            queueMicrotask(() => {
                // Only when nothing else took focus meanwhile (⌘T replacing the
                // palette hands focus to the tenant list, which must keep it).
                const active = document.activeElement
                if (active && active !== document.body) return
                // The opener may be gone (a "Finish rotation…" that completed):
                // focus the page heading rather than leave it on nothing.
                if (target?.isConnected) target.focus()
                else document.querySelector<HTMLElement>('main h1[tabindex="-1"]')?.focus()
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
            // A declared initial control first (wherever it sits), else the first focusable.
            const first = node.querySelector<HTMLElement>('[autofocus]') ?? node.querySelector<HTMLElement>(FOCUSABLE)
            ;(first ?? node).focus()
        }
    }, [topmost])

    // Escape still closes the topmost dialog when focus has left it (a
    // native sheet returning focus to the page body, a click on the scrim's
    // edge): the dialog's own handler stops the event first when it has focus.
    const closeRef = useRef(onClose)
    closeRef.current = onClose
    useEffect(() => {
        if (!topmost) return
        const onWindowKey = (event: globalThis.KeyboardEvent) => {
            if (event.key !== 'Escape' || event.defaultPrevented) return
            const node = dialog.current
            if (node && node.contains(document.activeElement)) return
            event.preventDefault()
            closeRef.current()
        }
        window.addEventListener('keydown', onWindowKey)
        return () => window.removeEventListener('keydown', onWindowKey)
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
            className={sheet ? 'fixed z-50 flex justify-end' : 'fixed inset-0 z-50 grid place-items-start justify-center'}
            style={sheet ? { inset: 'var(--v2-header, 56px) 0 0 0' } : { paddingTop: top ?? '4vh' }}
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
                className='border'
                // Bounded to the viewport: a tall dialog scrolls inside itself, so its
                // actions stay reachable at the minimum window size and 200% zoom.
                style={
                    sheet
                        ? { width: `min(${width}px, 100vw)`, height: '100%', overflowY: 'auto', padding, borderRadius: 0, borderLeft: '1px solid var(--v2-rule)', boxShadow: '-18px 0 40px rgba(0,0,0,0.18)', background: 'var(--v2-bg)', borderWidth: 0, color: 'var(--v2-ink)' }
                        : { width: `min(${width}px, 92vw)`, maxHeight: '92vh', overflowY: 'auto', padding, borderRadius: radius, boxShadow: '0 24px 60px rgba(0,0,0,0.3)', background: 'var(--v2-paper)', borderColor: 'var(--v2-rule)', color: 'var(--v2-ink)' }
                }
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
