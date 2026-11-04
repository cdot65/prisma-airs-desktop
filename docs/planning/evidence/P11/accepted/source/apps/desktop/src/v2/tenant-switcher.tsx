import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { FeatureId, ShellSnapshot, TenantView } from '@prisma-airs/desktop-contracts'
import { ENVIRONMENT_LABELS, SHELL_COPY } from './copy'
import { harnessCredential } from './harness/credential'
import { Glyph } from './icons'
import { useShellStore } from './store'
import { useModalsOpen } from './modal'

export interface TenantSwitcherProps {
    snapshot: ShellSnapshot
    onSwitch: (tenant: string) => void
    onAdd: () => void
    onNavigate: (feature: FeatureId) => void
    /** Opens the Gateway's workspace choice in Administration; null while no tenant is active (the row is gated). */
    onChangeWorkspace: (() => void) | null
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

/** A tenant's initials, as the prototype's avatar shows them. */
export const initials = (label: string) =>
    label
        .split(/\s+/)
        .filter(Boolean)
        .slice(0, 3)
        .map((w) => w[0]!.toUpperCase())
        .join('')

/** The avatar colours the prototype gives its tenants, chosen by a stable hash of the tenant id. */
const AVATARS = ['#a33d15', '#245e8f', '#1f6f3a', '#6b3fa0', '#8a5300', '#2f6f7a']
export function avatarColor(id: string): string {
    let h = 0
    for (const ch of id) h = (h * 31 + ch.charCodeAt(0)) >>> 0
    return AVATARS[h % AVATARS.length]!
}

/** The environment dot: production in the block colour, anything else flagged (R2App). */
const envDot = (t: TenantView) => (t.environment === 'production' ? 'var(--v2-block)' : 'var(--v2-flag)')

/**
 * The tenant button at the top of the sidebar and its menu (R2App, C35): the
 * tenant is always visible; the menu shows the tenant's identity, leads to
 * its settings and to the Gateway workspace, lists the other tenants with
 * ⌃n hints, and ends with "Add a tenant". Switching dispatches the one
 * `selectTenant` intent (the C30 guard renders from the core's refusal).
 */
export function TenantSwitcher({ snapshot, onSwitch, onAdd, onNavigate, onChangeWorkspace, collapsed }: TenantSwitcherProps) {
    const open = useShellStore((s) => s.switcherOpen)
    const setOpen = useShellStore((s) => s.setSwitcher)
    const active = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)
    const list = useRef<HTMLDivElement>(null)
    const trigger = useRef<HTMLButtonElement>(null)
    const wasOpen = useRef(false)
    const modalsOpen = useModalsOpen()
    const [anchor, setAnchor] = useState<{ top: number; left: number } | null>(null)
    // Focus the first tenant once the list is open AND nothing is inert any
    // more: ⌘T over the palette opens the list while the rail is still inert
    // (WebKit refuses focus inside an inert subtree), so this re-runs when the
    // palette has gone.
    useEffect(() => {
        if (open && !modalsOpen) {
            const first = list.current?.querySelector<HTMLElement>('[data-slot="tenant-list"] button')
            first?.focus()
            if (first && document.activeElement !== first) window.setTimeout(() => first.focus(), 0)
        } else if (!open && wasOpen.current) trigger.current?.focus()
        wasOpen.current = open
    }, [open, modalsOpen, anchor])
    useLayoutEffect(() => {
        if (!open) return
        const rect = trigger.current?.getBoundingClientRect()
        setAnchor(rect ? { top: rect.bottom + 6, left: rect.left } : { top: 96, left: 10 })
    }, [open])
    const workspace = snapshot.gateway.selected?.workspace.name ?? null
    // The account shown is the one the Harness would send as, for the selected workspace (R1).
    const credential = harnessCredential(snapshot)
    const signedIn = credential.source === 'sso' && credential.ready ? credential.account : null
    const close = () => setOpen(false)
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
                className='flex w-full items-center gap-2.5 rounded-[9px] text-left'
                style={{ padding: '7px 8px', margin: '0 0 10px', background: open ? 'var(--v2-inset)' : 'transparent', color: 'var(--v2-ink)' }}
            >
                <span
                    aria-hidden='true'
                    className='grid size-[34px] shrink-0 place-items-center rounded-[9px] text-[13px] font-bold'
                    style={{ background: active ? avatarColor(active.id) : 'var(--v2-ctl)', color: '#ffffff' }}
                >
                    {active ? initials(active.label) : '?'}
                </span>
                {!collapsed && (
                    <span data-v2-label className='flex min-w-0 flex-grow flex-col'>
                        <span className='flex items-center gap-[5px] text-[14px] font-bold'>
                            <span className='truncate'>{active?.label ?? SHELL_COPY.tenantNone}</span>
                            <Glyph name='chev' size={11} stroke={2.4} />
                        </span>
                        {active && (
                            <span className='flex items-center gap-[5px] text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                <span aria-hidden='true' className='size-1.5 shrink-0 rounded-full' style={{ background: envDot(active) }} />
                                <span className='truncate'>
                                    {environment(snapshot, active.id)}
                                    {workspace ? ` · ${workspace}` : active.tsgId ? ` · TSG ${active.tsgId}` : ''}
                                </span>
                            </span>
                        )}
                    </span>
                )}
            </button>
            {open &&
                anchor &&
                createPortal(
                    <div
                        ref={list}
                        role='group'
                        aria-label={SHELL_COPY.tenantMenu}
                        data-slot='tenant-menu'
                        onKeyDown={(e) => {
                            if (e.key === 'Escape') {
                                e.preventDefault()
                                close()
                                return
                            }
                            // ⌃1–⌃9 pick the nth tenant, as the rows' hints say (R2App).
                            if (e.ctrlKey && !e.metaKey && !e.altKey && /^[1-9]$/.test(e.key)) {
                                const t = snapshot.tenants[Number(e.key) - 1]
                                if (!t) return
                                e.preventDefault()
                                close()
                                if (t.id !== snapshot.active?.tenant) onSwitch(t.id)
                            }
                        }}
                        // Outside the rail (a portal): the collapsed rail's width and
                        // overflow never constrain the selection surface (DESIGN-V2 §4).
                        className='fixed z-40 flex flex-col overflow-hidden rounded-[12px] border'
                        style={{
                            top: anchor.top,
                            left: anchor.left,
                            width: 'min(var(--v2-menu), calc(100vw - 20px))',
                            maxHeight: `calc(100vh - ${anchor.top + 16}px)`,
                            background: 'var(--v2-paper)',
                            borderColor: 'var(--v2-rule)',
                            color: 'var(--v2-ink)',
                            boxShadow: '0 16px 40px rgba(0,0,0,0.22)',
                        }}
                    >
                        {active && (
                            <div className='flex items-center gap-3 p-3.5'>
                                <span aria-hidden='true' className='grid size-10 shrink-0 place-items-center rounded-[10px] text-[15px] font-bold' style={{ background: avatarColor(active.id), color: '#ffffff' }}>
                                    {initials(active.label)}
                                </span>
                                <span className='min-w-0 flex-grow'>
                                    <b className='block truncate text-[14px]'>{active.label}</b>
                                    <span className='block text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                        {environment(snapshot, active.id)}
                                        {workspace ? ` · ${SHELL_COPY.workspaceOf(workspace).toLowerCase()}` : ''}
                                    </span>
                                    <span className='block text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                        {active.tsgId ? SHELL_COPY.tsgOf(active.tsgId) : ''}
                                        {active.tsgId && signedIn ? ' · ' : ''}
                                        {signedIn ? SHELL_COPY.signedInAs(signedIn) : ''}
                                    </span>
                                </span>
                            </div>
                        )}
                        <div className='flex flex-col gap-px p-1.5' style={{ borderTop: '1px solid var(--v2-rule)' }}>
                            <button
                                type='button'
                                onClick={() => {
                                    close()
                                    onNavigate('settings')
                                }}
                                className='flex h-8 items-center rounded-[6px] px-2.5 text-left'
                            >
                                {SHELL_COPY.tenantSettings}
                            </button>
                            <button
                                type='button'
                                disabled={!onChangeWorkspace}
                                title={onChangeWorkspace ? undefined : SHELL_COPY.changeWorkspaceGate}
                                onClick={() => {
                                    if (!onChangeWorkspace) return
                                    close()
                                    onChangeWorkspace()
                                }}
                                className='flex h-8 items-center justify-between gap-2 rounded-[6px] px-2.5 text-left disabled:cursor-not-allowed'
                                style={{ color: onChangeWorkspace ? 'var(--v2-ink)' : 'var(--v2-faint)' }}
                            >
                                <span>{SHELL_COPY.changeWorkspace}</span>
                                {!onChangeWorkspace && <span className='text-[12px]'>{SHELL_COPY.changeWorkspaceGate}</span>}
                            </button>
                        </div>
                        <div className='flex flex-col gap-px overflow-y-auto overscroll-contain p-1.5' style={{ borderTop: '1px solid var(--v2-rule)' }}>
                            <div className='px-2 pt-1.5 pb-1 text-[12px] font-semibold' style={{ color: 'var(--v2-faint)' }}>
                                {SHELL_COPY.switchTenantHeading}
                            </div>
                            <div role='group' aria-label={SHELL_COPY.tenantSwitcher} data-slot='tenant-list' className='flex flex-col gap-px'>
                                {snapshot.tenants.map((t, i) => {
                                    const selected = t.id === snapshot.active?.tenant
                                    return (
                                        <button
                                            key={t.id}
                                            type='button'
                                            aria-pressed={selected}
                                            aria-label={identity(snapshot, t)}
                                            onFocus={(e) => e.currentTarget.scrollIntoView({ block: 'nearest' })}
                                            onClick={() => {
                                                close()
                                                if (!selected) onSwitch(t.id)
                                            }}
                                            className='flex min-h-[42px] w-full items-center gap-2.5 rounded-[7px] text-left'
                                            style={{ padding: '4px 8px', background: selected ? 'var(--v2-inset)' : 'transparent' }}
                                        >
                                            <span aria-hidden='true' className='grid size-7 shrink-0 place-items-center rounded-[7px] text-[11px] font-bold' style={{ background: avatarColor(t.id), color: '#ffffff' }}>
                                                {initials(t.label)}
                                            </span>
                                            <span className='min-w-0 flex-grow'>
                                                <span className='font-semibold'>{t.label}</span> <span style={{ color: 'var(--v2-faint)' }}>{environment(snapshot, t.id)}</span>
                                                <span className='block text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                                    {selected && workspace ? SHELL_COPY.workspaceOf(workspace) : t.tsgId ? SHELL_COPY.tsgOf(t.tsgId) : ''}
                                                </span>
                                            </span>
                                            <span aria-hidden='true' className='text-[12px] font-bold' style={{ color: 'var(--v2-acc)' }}>
                                                {selected ? '✓' : ''}
                                            </span>
                                            <span className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                                {i < 9 ? `⌃${i + 1}` : ''}
                                            </span>
                                        </button>
                                    )
                                })}
                            </div>
                        </div>
                        <div className='p-1.5' style={{ borderTop: '1px solid var(--v2-rule)' }}>
                            <button
                                type='button'
                                onClick={() => {
                                    close()
                                    onAdd()
                                }}
                                className='flex h-9 w-full items-center gap-2.5 rounded-[7px] px-2 text-left'
                            >
                                <span aria-hidden='true' className='grid size-7 place-items-center rounded-[7px] text-[16px]' style={{ border: '1px dashed var(--v2-faint)', color: 'var(--v2-soft)' }}>
                                    +
                                </span>
                                {SHELL_COPY.addTenant}
                            </button>
                        </div>
                    </div>,
                    document.body
                )}
        </div>
    )
}
