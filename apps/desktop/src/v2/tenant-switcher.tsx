import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { ChevronDown } from 'lucide-react'
import type { ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { ENVIRONMENT_LABELS, SHELL_COPY } from './copy'
import { useShellStore } from './store'
import { useModalsOpen } from './modal'

export interface TenantSwitcherProps {
    snapshot: ShellSnapshot
    onSwitch: (tenant: string) => void
    onAdd: () => void
    collapsed?: boolean
}

function environment(snapshot: ShellSnapshot, id: string | undefined): string {
    const tenant = snapshot.tenants.find((t) => t.id === id)
    if (!tenant) return ''
    return ENVIRONMENT_LABELS[tenant.environment]
}

/** The identity a tenant is told apart by: label, environment and TSG (labels may repeat). */
function identity(snapshot: ShellSnapshot, t: { id: string; label: string; tsgId: string | null }): string {
    return `${t.label} · ${environment(snapshot, t.id)}${t.tsgId ? ` · TSG ${t.tsgId}` : ''}`
}

/**
 * Slack-style tenant switcher at the top of the sidebar: the tenant is always
 * visible; switching dispatches the one `selectTenant` intent (the C30 guard
 * renders from the core's refusal). "Add a tenant…" opens the wizard (C29);
 * until then it leads to Settings.
 */
export function TenantSwitcher({ snapshot, onSwitch, onAdd, collapsed }: TenantSwitcherProps) {
    const open = useShellStore((s) => s.switcherOpen)
    const setOpen = useShellStore((s) => s.setSwitcher)
    const active = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const list = useRef<HTMLUListElement>(null)
    const trigger = useRef<HTMLButtonElement>(null)
    const wasOpen = useRef(false)
    const modalsOpen = useModalsOpen()
    const [anchor, setAnchor] = useState<{ top: number; left: number; width: number } | null>(null)
    // Focus the first tenant once the list is open AND nothing is inert any
    // more: ⌘T over the palette opens the list while the rail is still inert
    // (WebKit refuses focus inside an inert subtree), so this re-runs when the
    // palette has gone.
    useEffect(() => {
        if (open && !modalsOpen) {
            const first = list.current?.querySelector<HTMLElement>('button')
            first?.focus()
            if (first && document.activeElement !== first) window.setTimeout(() => first.focus(), 0)
        } else if (!open && wasOpen.current) trigger.current?.focus()
        wasOpen.current = open
    }, [open, modalsOpen, anchor])
    useLayoutEffect(() => {
        if (!open) return
        const rect = trigger.current?.getBoundingClientRect()
        setAnchor(rect ? { top: rect.bottom + 4, left: rect.left, width: rect.width } : { top: 56, left: 12, width: 220 })
    }, [open])
    return (
        <div data-slot='tenant-switcher' className='relative'>
            <button
                ref={trigger}
                type='button'
                aria-haspopup='true'
                aria-expanded={open}
                // The control's own name carries the current tenant (I2: the tenant is
                // always visible), expanded or collapsed, on every destination.
                aria-label={active ? `${SHELL_COPY.tenantSwitcher}: ${identity(snapshot, active)}` : `${SHELL_COPY.tenantSwitcher}: ${SHELL_COPY.tenantNone}`}
                title={active ? identity(snapshot, active) : SHELL_COPY.tenantNone}
                onClick={() => setOpen(!open)}
                className='flex min-h-11 w-full items-center gap-2 rounded-[7px] px-2.5 text-left'
                style={{ background: 'var(--v2-paper)', border: '1px solid var(--v2-rule)' }}
            >
                <span
                    aria-hidden='true'
                    className='grid size-7 shrink-0 place-items-center rounded-[7px] text-[12px] font-bold'
                    style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}
                >
                    {(active?.label ?? '?').slice(0, 1).toUpperCase()}
                </span>
                {!collapsed && (
                    <span data-v2-label className='flex min-w-0 flex-grow flex-col'>
                        <span className='truncate text-[14px] font-semibold'>{active?.label ?? SHELL_COPY.tenantNone}</span>
                        {active && (
                            <span className='text-[11px] uppercase tracking-wide' style={{ color: 'var(--v2-faint)' }}>
                                {environment(snapshot, active.id)}
                                {active.tsgId ? ` · TSG ${active.tsgId}` : ''}
                            </span>
                        )}
                    </span>
                )}
                <ChevronDown aria-hidden='true' size={14} />
            </button>
            {open &&
                anchor &&
                createPortal(
                <ul
                    ref={list}
                    role='group'
                    aria-label={SHELL_COPY.tenantSwitcher}
                    data-slot='tenant-list'
                    onKeyDown={(e) => {
                        if (e.key === 'Escape') {
                            e.preventDefault()
                            setOpen(false)
                        }
                    }}
                    // Outside the rail (a portal): the collapsed rail's width and
                    // overflow never constrain the selection surface (DESIGN-V2 §4).
                    className='fixed z-40 flex flex-col gap-0.5 overflow-y-auto overscroll-contain rounded-[12px] border p-1.5 shadow-xl'
                    style={{
                        top: anchor.top,
                        left: anchor.left,
                        minWidth: Math.max(280, anchor.width),
                        maxHeight: `calc(100vh - ${anchor.top + 16}px)`,
                        background: 'var(--v2-paper)',
                        borderColor: 'var(--v2-rule)',
                        color: 'var(--v2-ink)',
                    }}
                >
                    {snapshot.tenants.map((t) => {
                        const selected = t.id === snapshot.active?.tenant
                        return (
                            <li key={t.id}>
                                <button
                                    type='button'
                                    aria-pressed={selected}
                                    onFocus={(e) => e.currentTarget.scrollIntoView({ block: 'nearest' })}
                                    onClick={() => {
                                        setOpen(false)
                                        if (!selected) onSwitch(t.id)
                                    }}
                                    className='flex min-h-9 w-full items-center justify-between rounded-[7px] px-2.5 text-left text-[13px]'
                                    style={selected ? { background: 'var(--v2-sel)', color: 'var(--v2-sel-ink)' } : undefined}
                                >
                                    <span className='truncate font-medium'>{t.label}</span>
                                    <span className='shrink-0 text-[11px] uppercase'>
                                        {environment(snapshot, t.id)}
                                        {t.tsgId ? ` · TSG ${t.tsgId}` : ''}
                                    </span>
                                </button>
                            </li>
                        )
                    })}
                    <li>
                        <button
                            type='button'
                            onClick={() => {
                                setOpen(false)
                                onAdd()
                            }}
                            className='flex min-h-9 w-full items-center rounded-[7px] px-2.5 text-left text-[13px] font-semibold'
                            style={{ color: 'var(--v2-acc)' }}
                        >
                            {SHELL_COPY.addTenant}
                        </button>
                    </li>
                </ul>,
                document.body
                )}
        </div>
    )
}
