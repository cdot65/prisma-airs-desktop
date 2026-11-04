import { useEffect, useRef, type KeyboardEvent } from 'react'
import type { ShellSnapshot } from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../modules/tenants/dispatch'
import { useShellStore } from './store'
import { CallsPanel } from './workbench/calls-panel'

/** Nonmodal: only this drawer owns its Escape; navigation and the page stay live. */
export function ExplainDrawer({ snapshot, dispatch, onClose, returnFocus, modalOpen }: { snapshot: ShellSnapshot; dispatch: Dispatch; onClose: () => void; returnFocus: HTMLElement | null; modalOpen: boolean }) {
    const panel = useRef<HTMLDivElement>(null)
    const restoreFocus = useRef(false)
    const close = () => {
        restoreFocus.current = Boolean(panel.current?.contains(document.activeElement))
        onClose()
    }
    useEffect(() => {
        // An explicit open moves focus; a persisted preference on launch does not.
        if (returnFocus && useShellStore.getState().modals.length === 0) panel.current?.querySelector<HTMLButtonElement>('button')?.focus()
        return () => {
            if (!restoreFocus.current) return
            queueMicrotask(() => {
                if (useShellStore.getState().modals.length > 0 || (document.activeElement && document.activeElement !== document.body)) return
                if (returnFocus?.isConnected) returnFocus.focus()
                else document.querySelector<HTMLElement>('[data-slot="page-heading"]')?.focus()
            })
        }
    }, [returnFocus])
    const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        if (event.key !== 'Escape' || event.defaultPrevented || modalOpen) return
        event.preventDefault()
        event.stopPropagation()
        close()
    }
    return (
        <div
            ref={panel}
            id='explain-drawer'
            data-slot='explain-drawer'
            data-desktop-theme=''
            onKeyDown={onKeyDown}
            style={{ position: 'absolute', zIndex: 30, top: 0, right: 0, bottom: 0, width: 'min(420px, 100%)', overflowY: 'auto', background: 'var(--v2-paper)', borderLeft: '1px solid var(--v2-rule)', boxShadow: '-12px 0 28px rgba(0,0,0,0.16)' }}
        >
            <CallsPanel snapshot={snapshot} dispatch={dispatch} onClose={close} />
        </div>
    )
}
