import { useEffect, useRef } from 'react'
import { Eye, Moon, Sun, SunMoon, Undo2 } from 'lucide-react'
import type { DesktopClient, FeatureId, Intent, ShellSnapshot, Theme } from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../features/tenants/dispatch'
import { FeatureBody } from '../shell/feature-body'
import { FEATURE_ICONS } from '../shell/icons'
import { SHELL_COPY } from './copy'
import { Home } from './home'
import { Palette } from './palette'
import { useShellStore } from './store'
import { TenantSwitcher } from './tenant-switcher'
import { Toasts } from './toasts'
import { useModalsOpen } from './modal'

export interface V2ShellProps {
    snapshot: ShellSnapshot
    appearancePending: boolean
    status: string
    onNavigate: (id: FeatureId) => void
    onSetAppearance: (theme: Theme) => void
    onSetShowCalls: (on: boolean) => void
    onReturnToClassic: () => void
    onSwitchTenant: (tenant: string) => void
    onReadNow: () => void
    /** Re-sends a delivery the desktop did not confirm, only on the operator's click. */
    onRetry: (what: string, intent: Intent) => void
    dispatch: Dispatch
    client: DesktopClient
}

const THEMES: { value: Theme; label: string; icon: typeof Sun }[] = [
    { value: 'system', label: SHELL_COPY.appearanceSystem, icon: SunMoon },
    { value: 'light', label: SHELL_COPY.appearanceLight, icon: Sun },
    { value: 'dark', label: SHELL_COPY.appearanceDark, icon: Moon },
]

/**
 * The "Refined Native" shell (C28, DESIGN-V2): a 244px rail with the tenant
 * switcher on top, the Rust navigation inventory (labels and ⌘ hints come
 * from the same catalogue the native menu is built from), appearance and
 * "show the calls" preferences, one status surface and the modal stack.
 * Every destination other than Home renders the same FeatureBody the legacy
 * shell renders, so behaviour is identical whichever shell frames it.
 */
export function V2Shell(props: V2ShellProps) {
    const { snapshot } = props
    const paletteOpen = useShellStore((s) => s.paletteOpen)
    const openPalette = useShellStore((s) => s.openPalette)
    const closePalette = useShellStore((s) => s.closePalette)
    const unconfirmed = useShellStore((s) => s.unconfirmed)
    const clearUnconfirmed = useShellStore((s) => s.clearUnconfirmed)
    const modalOpen = useModalsOpen()
    const heading = useRef<HTMLHeadingElement>(null)
    const firstRender = useRef(true)
    const active = snapshot.activeFeature
    const feature = snapshot.features.find((f) => f.id === active) ?? snapshot.features[0]

    useEffect(() => {
        // Move focus to the new page heading after navigation (not on launch).
        if (firstRender.current) {
            firstRender.current = false
            return
        }
        heading.current?.focus()
    }, [active])

    if (!feature) return null
    return (
        <div data-v2-shell='' className='flex min-h-screen'>
            <nav
                data-v2-rail=''
                aria-label={SHELL_COPY.navLabel}
                aria-hidden={modalOpen ? true : undefined}
                inert={modalOpen ? true : undefined}
                className='sticky top-0 flex h-screen shrink-0 flex-col gap-2 overflow-y-auto p-3'
                style={{ width: 'var(--v2-rail)', background: 'var(--v2-side)', borderRight: '1px solid var(--v2-rule)' }}
            >
                <TenantSwitcher snapshot={snapshot} onSwitch={props.onSwitchTenant} onAdd={() => props.onNavigate('settings')} />
                <ul className='mt-2 flex flex-col gap-0.5'>
                    {snapshot.features
                        .filter((f) => f.sidebar)
                        .map((f) => {
                            const Icon = FEATURE_ICONS[f.id]
                            const current = f.id === feature.id
                            const unavailable = f.availability.state === 'unavailable'
                            const reason = f.availability.state === 'unavailable' ? f.availability.reason : ''
                            return (
                                <li key={f.id}>
                                    <button
                                        type='button'
                                        aria-current={current ? 'page' : undefined}
                                        aria-disabled={unavailable || undefined}
                                        aria-describedby={unavailable ? `${f.id}-hint` : undefined}
                                        title={unavailable ? SHELL_COPY.unavailableHint(reason) : f.shortcut !== null ? `${f.label} (⌘${f.shortcut})` : f.label}
                                        onClick={() => !unavailable && props.onNavigate(f.id)}
                                        data-inset-focus=''
                                        className='flex min-h-9 w-full items-center gap-2.5 rounded-[7px] px-2.5 text-left text-[14px] font-medium'
                                        style={{
                                            background: current ? 'var(--v2-sel)' : 'transparent',
                                            color: current ? 'var(--v2-sel-ink)' : unavailable ? 'var(--v2-faint)' : 'var(--v2-ink)',
                                        }}
                                    >
                                        <Icon aria-hidden='true' size={16} />
                                        <span data-v2-label className='flex-grow truncate'>
                                            {f.label}
                                        </span>
                                        {f.shortcut !== null && (
                                            <kbd data-v2-label className='text-[11px]' aria-label={`Command ${f.shortcut}`} style={{ opacity: 0.75 }}>
                                                ⌘{f.shortcut}
                                            </kbd>
                                        )}
                                    </button>
                                    {unavailable && (
                                        <span id={`${f.id}-hint`} className='sr-only'>
                                            {SHELL_COPY.unavailableHint(reason)}
                                        </span>
                                    )}
                                </li>
                            )
                        })}
                </ul>
                <button
                    type='button'
                    data-slot='palette-trigger'
                    onClick={openPalette}
                    className='mt-1 flex min-h-9 items-center justify-between rounded-[7px] border px-2.5 text-[12px]'
                    style={{ borderColor: 'var(--v2-rule)', color: 'var(--v2-faint)' }}
                >
                    <span data-v2-label>{SHELL_COPY.palette}</span>
                    <kbd aria-label='Command K'>⌘K</kbd>
                </button>
                <div className='mt-auto flex flex-col gap-2 pt-3' style={{ borderTop: '1px solid var(--v2-rule)' }}>
                    <fieldset aria-busy={props.appearancePending} data-v2-stack='' className='flex gap-1'>
                        <legend className='sr-only'>{SHELL_COPY.appearance}</legend>
                        {THEMES.map((t) => (
                            <button
                                key={t.value}
                                type='button'
                                aria-pressed={snapshot.appearance === t.value}
                                aria-label={t.label}
                                title={t.label}
                                onClick={() => props.onSetAppearance(t.value)}
                                className='grid size-8 place-items-center rounded-[7px] border'
                                style={{
                                    borderColor: 'var(--v2-rule)',
                                    background: snapshot.appearance === t.value ? 'var(--v2-acc-soft)' : 'transparent',
                                }}
                            >
                                <t.icon aria-hidden='true' size={14} />
                            </button>
                        ))}
                    </fieldset>
                    <label className='flex min-h-8 items-center gap-2 text-[12px]' title={SHELL_COPY.showCallsHint}>
                        <Eye aria-hidden='true' size={14} className='shrink-0' />
                        <input
                            type='checkbox'
                            aria-label={SHELL_COPY.showCalls}
                            aria-describedby='v2-show-calls-hint'
                            checked={snapshot.showCalls}
                            onChange={(e) => props.onSetShowCalls(e.target.checked)}
                            className='size-4 min-h-0'
                        />
                        <span data-v2-label>{SHELL_COPY.showCalls}</span>
                        <span id='v2-show-calls-hint' className='sr-only'>
                            {SHELL_COPY.showCallsHint}
                        </span>
                    </label>
                    <button
                        type='button'
                        onClick={props.onReturnToClassic}
                        title={SHELL_COPY.classic}
                        aria-label={SHELL_COPY.classic}
                        className='flex min-h-8 items-center gap-2 rounded-[7px] text-left text-[12px]'
                        style={{ color: 'var(--v2-faint)' }}
                    >
                        <Undo2 aria-hidden='true' size={14} className='shrink-0' />
                        <span data-v2-label className='underline'>
                            {SHELL_COPY.classic}
                        </span>
                    </button>
                    {snapshot.fixtureBuild && (
                        <p data-slot='build-identity' className='font-mono text-[10px] tracking-[0.14em] uppercase' style={{ color: 'var(--v2-flag)' }}>
                            {SHELL_COPY.fixture}
                        </p>
                    )}
                </div>
            </nav>
            <main
                data-slot='app-main'
                className='min-w-0 flex-grow'
                style={{ padding: 'var(--v2-page-top) var(--v2-page-x) 28px' }}
                aria-hidden={modalOpen ? true : undefined}
                inert={modalOpen ? true : undefined}
            >
                {unconfirmed.length > 0 && (
                    <ul data-slot='unconfirmed' aria-label='Not confirmed' className='mb-4 flex flex-col gap-2'>
                        {unconfirmed.map((u) => (
                            <li key={u.id} role='status' className='flex items-center gap-3 rounded-[12px] border px-4 py-2 text-[13px]' style={{ borderColor: 'var(--v2-flag)', background: 'var(--v2-flag-bg)', color: 'var(--v2-ink)' }}>
                                <span className='flex-grow'>{SHELL_COPY.unconfirmed(u.what)}</span>
                                <button
                                    type='button'
                                    onClick={() => {
                                        clearUnconfirmed(u.id)
                                        props.onRetry(u.what, u.intent)
                                    }}
                                    className='h-8 rounded-[7px] px-3 text-[12px] font-semibold'
                                    style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}
                                >
                                    {SHELL_COPY.retry}
                                </button>
                                <button type='button' onClick={() => clearUnconfirmed(u.id)} className='h-8 rounded-[7px] border px-3 text-[12px]' style={{ borderColor: 'var(--v2-rule)' }}>
                                    {SHELL_COPY.dismiss}
                                </button>
                            </li>
                        ))}
                    </ul>
                )}
                {feature.id === 'home' ? (
                    <Home ref={heading} snapshot={snapshot} onNavigate={props.onNavigate} />
                ) : (
                    <FeatureBody
                        ref={heading}
                        feature={feature}
                        snapshot={snapshot}
                        appearancePending={props.appearancePending}
                        onSetAppearance={props.onSetAppearance}
                        onNavigate={props.onNavigate}
                        dispatch={props.dispatch}
                        client={props.client}
                    />
                )}
                <p role='status' aria-live='polite' data-slot='shell-status' className='mt-4 pb-6 text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                    {props.status}
                </p>
            </main>
            {paletteOpen && (
                <Palette snapshot={snapshot} onNavigate={props.onNavigate} onSwitchTenant={props.onSwitchTenant} onReadNow={props.onReadNow} onClose={closePalette} />
            )}
            {/* Toasts are background siblings of a modal too (I8): inert and hidden while one is open. */}
            <div aria-hidden={modalOpen ? true : undefined} inert={modalOpen ? true : undefined}>
                <Toasts />
            </div>
        </div>
    )
}
