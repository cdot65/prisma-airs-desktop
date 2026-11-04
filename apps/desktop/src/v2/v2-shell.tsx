import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import type { AdminProductId, SwitchResolution, DesktopClient, FeatureId, Intent, ShellSnapshot, Theme } from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../modules/tenants/dispatch'
import { FeatureBody } from '../shell/feature-body'
import { type Act, Inbox } from './inbox/inbox'
import { Administration } from './admin/administration'
import { harnessCredential } from './harness/credential'
import { Harness } from './harness/harness'
import { FEATURE_GLYPHS, Glyph, PRODUCT_GLYPHS } from './icons'
import { HOME_COPY, SHELL_COPY, SNOOZE_COPY } from './copy'
import { readLine } from './home'
import { Home } from './home'
import { Palette } from './palette'
import { unconfirmedInContext, useShellStore } from './store'
import { SwitchGuard } from './switch-guard'
import { TenantSwitcher } from './tenant-switcher'
import { Toasts } from './toasts'
import { Modal, useModalsOpen } from './modal'
import { Wizard } from './wizard/wizard'
import { Academy } from './academy/academy'
import { ApiClient } from './workbench/api-client'
import { ExplainDrawer } from './explain-drawer'
import { TestCases } from './workbench/test-cases'

/** C44: the product destinations the catalogue still lists (Go, the palette), opened as Administration's products. */
const PRODUCT_DESTINATION: Record<'runtime' | 'gateway' | 'redTeam', { product: AdminProductId; sequence: number }> = { runtime: { product: 'runtimeSecurity', sequence: -1 }, gateway: { product: 'gateway', sequence: -2 }, redTeam: { product: 'redTeam', sequence: -3 } }

export interface V2ShellProps {
    snapshot: ShellSnapshot
    appearancePending: boolean
    status: string
    onNavigate: (id: FeatureId) => void
    onSetAppearance: (theme: Theme) => void
    onSetShowCalls: (on: boolean) => void
    onSwitchTenant: (tenant: string, resolution?: SwitchResolution) => void
    onReadNow: () => void
    /** Re-sends a delivery the desktop did not confirm, only on the operator's click. */
    onRetry: (what: string, intent: Intent) => void
    /** Sends one intent and reports its outcome on the shell's surfaces (Home and Inbox actions). */
    onAct: Act
    dispatch: Dispatch
    client: DesktopClient
}

/** The products the Inbox files entries under (the inventory's ids that have an admitted service). */
const productOf = (id: AdminProductId): string | null => (id === 'agenticIdentity' || id === 'agenticEndpoint' ? null : id)

const darkQuery = () => (typeof window !== 'undefined' && typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null)
const subscribeDark = (notify: () => void) => {
    const query = darkQuery()
    if (!query) return () => undefined
    query.addEventListener('change', notify)
    return () => query.removeEventListener('change', notify)
}
// Without a media query the follower assumes dark (shell/appearance.ts): the toggle agrees with it.
const readDark = () => darkQuery()?.matches ?? true

/**
 * Whether the shown appearance is dark: the choice, or the system's while
 * following it, re-read on every OS appearance change (C35 R2) so the
 * toggle's state and its next click follow what the follower applied.
 */
function useShownDark(theme: Theme): boolean {
    const systemDark = useSyncExternalStore(subscribeDark, readDark, readDark)
    return theme === 'system' ? systemDark : theme === 'dark'
}

/**
 * The "Refined Native" shell as the prototype draws it (R2App, C35): a
 * 244px sidebar (tenant button, the Rust navigation inventory with the
 * Administration products as children and needs-action badges, the status
 * block) and a right column with the 56px header (title and subtitle, the
 * search button that opens the palette and the Explain switch)
 * over the destination. Labels and ⌘ hints come from the same catalogue
 * the native menu is built from. Destinations not yet ported render the
 * legacy FeatureBody inside the page frame.
 */
export function V2Shell(props: V2ShellProps) {
    const { snapshot } = props
    const paletteOpen = useShellStore((s) => s.paletteOpen)
    const openPalette = useShellStore((s) => s.openPalette)
    const closePalette = useShellStore((s) => s.closePalette)
    const adminNavOpen = useShellStore((s) => s.adminNavOpen)
    const setAdminNav = useShellStore((s) => s.setAdminNav)
    const unconfirmedAll = useShellStore((s) => s.unconfirmed)
    // A captured null switch source is a scope too; only truly unbound actions survive (I2).
    const unconfirmed = unconfirmedAll.filter((u) => unconfirmedInContext(u, snapshot.active?.context ?? null))
    const clearUnconfirmed = useShellStore((s) => s.clearUnconfirmed)
    const arrival = useShellStore((s) => s.arrival)
    const modalOpen = useModalsOpen()
    const heading = useRef<HTMLHeadingElement>(null)
    const explainButton = useRef<HTMLButtonElement>(null)
    const explainOpener = useRef<HTMLElement | null>(null)
    const firstRender = useRef(true)
    const [adding, setAdding] = useState(false)
    // The sheet closes only when the wizard says so: a committed tenant (toast)
    // or a discard the core admitted. Escape and the scrim are close requests
    // the wizard answers with the same Cancel as its button.
    const [closeRequests, setCloseRequests] = useState(0)
    const closeSheet = (added?: string) => {
        setAdding(false)
        setCloseRequests(0)
        if (added) useShellStore.getState().toast(SHELL_COPY.tenantAdded(added), 'ok')
    }
    const active = snapshot.activeFeature
    const feature = snapshot.features.find((f) => f.id === active) ?? snapshot.features[0]
    // Completion toasts are derived from the core's ReadStatus (I4): a new
    // generation is one finished read; its kind follows the coverage. A
    // tenant change re-baselines (the other tenant's generation is its own).
    const read = snapshot.inbox.read
    const tenantNow = snapshot.active?.tenant ?? null
    const tenant = snapshot.tenants.find((t) => t.id === tenantNow) ?? null
    // A tenant change retires the drawer until an explicit open in the new tenant.
    // A late reply to the old tenant's preference cannot reopen it.
    const [explainTenant, setExplainTenant] = useState<string | null | undefined>(tenantNow)
    const explainOpen = snapshot.showCalls && explainTenant === tenantNow
    const pendingExplainOpen = useRef<string | null | undefined>(undefined)
    const seenExplain = useRef({ tenant: tenantNow, open: snapshot.showCalls && snapshot.calls !== undefined })
    const setExplain = (on: boolean) => {
        pendingExplainOpen.current = on ? tenantNow : undefined
        if (on) {
            setExplainTenant(tenantNow)
            explainOpener.current = explainButton.current
        }
        props.onSetShowCalls(on)
    }
    useEffect(() => {
        const previous = seenExplain.current
        seenExplain.current = { tenant: tenantNow, open: explainOpen && snapshot.calls !== undefined }
        if (previous.tenant !== tenantNow) {
            explainOpener.current = null
            const wasOpening = pendingExplainOpen.current === previous.tenant
            pendingExplainOpen.current = undefined
            setExplainTenant(undefined)
            if (previous.open || wasOpening) props.onSetShowCalls(false)
        }
    }, [tenantNow, snapshot.calls, explainOpen, props.onSetShowCalls])
    const seenGeneration = useRef<{ tenant: string | null; generation: number }>({ tenant: tenantNow, generation: read.generation })
    useEffect(() => {
        const seen = seenGeneration.current
        seenGeneration.current = { tenant: tenantNow, generation: read.generation }
        if (seen.tenant !== tenantNow || read.generation === seen.generation) return
        const tenant = snapshot.tenants.find((t) => t.id === tenantNow)
        const line = readLine(snapshot, tenant?.label ?? null)
        useShellStore.getState().toast(line.text, line.tone === 'allowed' ? 'ok' : 'warn', undefined, tenantNow, snapshot.active?.context ?? null)
    }, [read.generation, tenantNow, snapshot])
    // A snooze is announced once, from the receipt the core issues for it
    // (I11): the return time is the core's, wherever the snooze was asked
    // for. Holds a tenant already had, or a read rediscovers, are not news.
    const lastSnooze = snapshot.inbox.lastSnooze
    const seenSnooze = useRef<{ tenant: string | null; sequence: number }>({ tenant: tenantNow, sequence: lastSnooze?.sequence ?? 0 })
    useEffect(() => {
        const seen = seenSnooze.current
        const sequence = lastSnooze?.sequence ?? 0
        seenSnooze.current = { tenant: tenantNow, sequence }
        if (seen.tenant !== tenantNow || !lastSnooze || sequence === seen.sequence) return
        useShellStore.getState().toast(SNOOZE_COPY.toast(lastSnooze.title, lastSnooze.returnsAt), 'info', undefined, tenantNow, snapshot.active?.context ?? null)
    }, [lastSnooze, tenantNow, snapshot.active])
    // Presentation state follows its tenant (I2): an arrival another tenant
    // asked for is forgotten, a toast that spoke for another tenant's item or
    // read is retired (however long Reduce Motion would have kept it), and an
    // unconfirmed delivery bound to a retired context is dropped (it can
    // never be revived, let alone rebound).
    const contextNow = snapshot.active?.context ?? null
    useEffect(() => {
        useShellStore.getState().scopeArrival(tenantNow)
        useShellStore.getState().retireToasts(tenantNow)
        useShellStore.getState().retireUnconfirmed(contextNow)
    }, [tenantNow, contextNow])
    // The guard for a switch asked here (I1); a setup draft's completion
    // renders the same guard inside the wizard, which re-sends the completion.
    const guard = snapshot.switchBlocked && !snapshot.switchBlocked.completion ? snapshot.switchBlocked : null
    const guardTarget = guard ? snapshot.tenants.find((t) => t.id === guard.to)?.label ?? guard.to : null

    useEffect(() => {
        // Move focus to the new page heading after navigation (not on launch).
        if (firstRender.current) {
            firstRender.current = false
            return
        }
        heading.current?.focus()
    }, [active])

    const observeWorkbench = useCallback((operation: string) => props.client.observeWorkbench(operation), [props.client])

    if (!feature) return null

    // The rail's badges: items needing action, in all and per product (the prototype's NAV badge and PRODUCTS counts).
    const needing = snapshot.inbox.items.filter((e) => e.needsAction && !e.hold)
    const products = snapshot.administration?.products ?? []
    const productsKnown = products.length > 0
    const admin = snapshot.features.find((f) => f.id === 'administration')
    const adminAvailable = admin?.availability.state === 'available'
    const productShown = arrival && arrival.tenant === tenantNow && arrival.feature === 'administration' ? arrival.product ?? null : null
    const openProduct = (id: AdminProductId) => {
        if (!tenantNow) return
        useShellStore.getState().arrive({ tenant: tenantNow, feature: 'administration', resource: null, item: null, product: id })
        props.onNavigate('administration')
    }
    const subtitle = feature.id === 'administration' && productShown ? products.find((p) => p.id === productShown)?.label ?? '' : ''

    // The status block (R2App): the connection and read status, the Harness credential as the
    // Harness itself would use it for the selected workspace (R1), the keychain, jobs.
    const readStatus = readLine(snapshot, tenant?.label ?? null)
    const credential = harnessCredential(snapshot)
    const harnessText =
        credential.source === 'sso'
            ? credential.ready
                ? SHELL_COPY.harnessSignedIn
                : credential.failed
                  ? SHELL_COPY.harnessRenew
                  : SHELL_COPY.harnessPending
            : credential.source === 'key'
              ? SHELL_COPY.harnessKey
              : credential.workspace
                ? SHELL_COPY.harnessOff
                : SHELL_COPY.harnessNoWorkspace
    const keychainText = snapshot.storage === 'ready' ? SHELL_COPY.keychainUnlocked : snapshot.storage === 'unavailable' ? SHELL_COPY.keychainUnavailable : SHELL_COPY.keychainLocked
    const running = read.state === 'reading' ? 1 : 0
    const jobsText = running === 0 ? SHELL_COPY.jobsNone : SHELL_COPY.jobsRunning(running)
    const account = credential.source === 'sso' && credential.ready ? credential.account : snapshot.academy.state === 'signedIn' ? snapshot.academy.displayName : null
    const dark = useShownDark(snapshot.appearance)
    // The tenant menu's workspace row: the Gateway's Workspaces tab in Administration, while a tenant is active.
    const gatewayProduct = products.find((p) => p.id === 'gateway')
    const changeWorkspace =
        tenantNow && adminAvailable && gatewayProduct && gatewayProduct.availability.state === 'available'
            ? () => {
                  useShellStore.getState().arrive({ tenant: tenantNow, feature: 'administration', resource: null, item: null, product: 'gateway', tab: 'workspaces' })
                  props.onNavigate('administration')
              }
            : null

    const navRow = (current: boolean, unavailable: boolean) => ({
        background: current ? 'var(--v2-sel)' : 'transparent',
        color: current ? 'var(--v2-sel-ink)' : unavailable ? 'var(--v2-faint)' : 'var(--v2-ink)',
    })

    return (
        <div data-v2-shell='' className='flex h-screen overflow-hidden'>
            <nav
                data-v2-rail=''
                data-desktop-theme=''
                aria-label={SHELL_COPY.navLabel}
                aria-hidden={modalOpen ? true : undefined}
                inert={modalOpen ? true : undefined}
                // The prototype reserves 14px above the tenant button for the window's traffic lights, which this
                // window's own title bar carries; the rail starts under it (noted in the parity notes).
                className='flex h-screen shrink-0 flex-col overflow-y-auto'
                style={{ width: 'var(--v2-rail)', padding: '12px 10px 12px', background: 'var(--v2-side)', borderRight: '1px solid var(--v2-rule)' }}
            >
                <TenantSwitcher snapshot={snapshot} onSwitch={props.onSwitchTenant} onAdd={() => setAdding(true)} onNavigate={props.onNavigate} onChangeWorkspace={changeWorkspace} />
                <ul className='flex flex-grow flex-col overflow-y-auto overscroll-contain' style={{ gap: 2, minHeight: 0 }} data-slot='nav-list'>
                    {snapshot.features
                        .filter((f) => f.sidebar)
                        .map((f) => {
                            const glyph = FEATURE_GLYPHS[f.id] ?? 'chevR'
                            const current = f.id === feature.id
                            const unavailable = f.availability.state === 'unavailable'
                            const reason = f.availability.state === 'unavailable' ? f.availability.reason : ''
                            const isAdmin = f.id === 'administration'
                            const badge = f.id === 'inbox' && needing.length > 0 ? String(needing.length) : ''
                            return (
                                <li key={f.id} className='flex flex-col' style={{ gap: 1 }}>
                                    <div className='flex items-center' style={{ gap: 2 }}>
                                        <button
                                            type='button'
                                            aria-current={current ? 'page' : undefined}
                                            aria-disabled={unavailable || undefined}
                                            aria-describedby={unavailable ? `${f.id}-hint` : undefined}
                                            title={unavailable ? SHELL_COPY.unavailableHint(reason) : f.shortcut !== null ? `${f.label} (⌘${f.shortcut})` : f.label}
                                            onClick={() => !unavailable && props.onNavigate(f.id)}
                                            data-inset-focus=''
                                            className='flex h-[34px] min-h-0 flex-grow items-center rounded-[8px] px-2.5 text-left text-[13px]'
                                            style={{ ...navRow(current, unavailable), gap: 9, fontWeight: current ? 600 : 500 }}
                                        >
                                            <Glyph name={glyph} size={16} color={current ? 'var(--v2-sel-ink)' : 'var(--v2-soft)'} />
                                            <span data-v2-label className='flex-grow truncate'>
                                                {f.label}
                                            </span>
                                            {badge && (
                                                <span
                                                    data-v2-label
                                                    data-slot='nav-badge'
                                                    aria-label={SHELL_COPY.needAction(needing.length)}
                                                    className='grid h-[18px] min-w-[18px] place-items-center rounded-full px-[5px] text-[11px] font-bold'
                                                    style={{ background: current ? 'rgba(255,255,255,0.22)' : 'var(--v2-acc)', color: current ? 'var(--v2-sel-ink)' : 'var(--v2-acc-ink)' }}
                                                >
                                                    {badge}
                                                </span>
                                            )}
                                            {f.shortcut !== null && (
                                                <kbd data-v2-label className='text-[12px]' aria-label={`Command ${f.shortcut}`} style={{ color: current ? 'var(--v2-sel-ink)' : 'var(--v2-faint)', opacity: current ? 0.85 : 1 }}>
                                                    ⌘{f.shortcut}
                                                </kbd>
                                            )}
                                        </button>
                                        {isAdmin && adminAvailable && productsKnown && (
                                            <button
                                                type='button'
                                                data-v2-collapse=''
                                                aria-expanded={adminNavOpen}
                                                aria-label={SHELL_COPY.productsToggle}
                                                aria-controls='v2-nav-products'
                                                onClick={() => setAdminNav(!adminNavOpen)}
                                                className='grid h-[34px] min-h-0 w-6 shrink-0 place-items-center rounded-[6px]'
                                                style={{ background: 'transparent', color: 'var(--v2-faint)' }}
                                            >
                                                <Glyph name={adminNavOpen ? 'chev' : 'chevR'} size={12} stroke={2.2} />
                                            </button>
                                        )}
                                    </div>
                                    {unavailable && (
                                        <span id={`${f.id}-hint`} className='sr-only'>
                                            {SHELL_COPY.unavailableHint(reason)}
                                        </span>
                                    )}
                                    {isAdmin && adminAvailable && productsKnown && adminNavOpen && (
                                        <ul id='v2-nav-products' aria-label={SHELL_COPY.productsLabel} data-slot='nav-products' data-v2-collapse='' className='flex flex-col' style={{ gap: 1, padding: '2px 0 4px 18px' }}>
                                            {products.map((p) => {
                                                const key = productOf(p.id)
                                                const count = key ? needing.filter((e) => e.product === key).length : 0
                                                const shown = current && productShown === p.id
                                                const gated = p.availability.state === 'unavailable'
                                                return (
                                                    <li key={p.id}>
                                                        <button
                                                            type='button'
                                                            aria-current={shown ? 'true' : undefined}
                                                            aria-disabled={gated || undefined}
                                                            title={p.availability.state === 'unavailable' ? p.availability.reason : p.label}
                                                            onClick={() => !gated && openProduct(p.id)}
                                                            className='flex h-[31px] min-h-0 w-full items-center gap-2 rounded-[7px] px-2 text-left text-[12.5px]'
                                                            style={{ background: shown ? 'var(--v2-inset)' : 'transparent', color: 'var(--v2-ink)', fontWeight: shown ? 600 : 400 }}
                                                        >
                                                            <Glyph name={PRODUCT_GLYPHS[p.id]} size={14} color='var(--v2-soft)' />
                                                            <span data-v2-label className='flex-grow truncate'>
                                                                {p.label}
                                                            </span>
                                                            {gated && (
                                                                <span data-v2-label className='rounded-full px-1.5 text-[12px] font-bold' style={{ padding: '1px 6px', background: 'var(--v2-acc-soft)', color: 'var(--v2-acc)' }}>
                                                                    {SHELL_COPY.newTag}
                                                                </span>
                                                            )}
                                                            {count > 0 && (
                                                                <span data-v2-label data-slot='product-badge' aria-label={SHELL_COPY.needAction(count)} className='text-[11px] font-semibold' style={{ color: 'var(--v2-faint)' }}>
                                                                    {count}
                                                                </span>
                                                            )}
                                                        </button>
                                                    </li>
                                                )
                                            })}
                                        </ul>
                                    )}
                                </li>
                            )
                        })}
                </ul>
                <div className='flex flex-col text-[12px]' data-slot='status-block' style={{ gap: 6, padding: '12px 10px 4px', marginTop: 8, borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-soft)' }}>
                    <button
                        type='button'
                        data-v2-collapse=''
                        data-slot='connection'
                        data-tone={readStatus.tone}
                        title={`${readStatus.text} · ${HOME_COPY.readNow}`}
                        onClick={props.onReadNow}
                        disabled={!tenant}
                        className='flex min-h-0 w-full items-center rounded-[6px] text-left text-[12px]'
                        style={{ gap: 7, color: 'var(--v2-soft)' }}
                    >
                        <span aria-hidden='true' className='size-2 shrink-0 rounded-full' style={{ background: readStatus.tone === 'allowed' ? 'var(--v2-ok)' : readStatus.tone === 'blocked' ? 'var(--v2-block)' : readStatus.tone === 'flagged' ? 'var(--v2-flag)' : 'var(--v2-faint)' }} />
                        <span className='flex-grow'>{tenant ? SHELL_COPY.connected : SHELL_COPY.tenantNone}</span>
                        <span className='truncate text-right font-semibold' style={{ color: readStatus.tone === 'allowed' ? 'var(--v2-faint)' : readStatus.tone === 'info' ? 'var(--v2-faint)' : 'var(--v2-flag)' }}>
                            {readStatus.text}
                        </span>
                    </button>
                    {(
                        [
                            ['bot', SHELL_COPY.harnessRow, harnessText, 'harness-status'],
                            ['lock', SHELL_COPY.keychainRow, keychainText, 'keychain-status'],
                            ['clock', SHELL_COPY.jobsRow, jobsText, 'jobs-status'],
                        ] as const
                    ).map(([glyph, label, value, slot]) => (
                        <div key={slot} data-slot={slot} data-v2-collapse='' className='flex items-center text-[12px]' style={{ gap: 7, color: 'var(--v2-soft)' }}>
                            <Glyph name={glyph} size={12} stroke={2} />
                            <span className='flex-grow'>{label}</span>
                            <span className='truncate' style={{ color: 'var(--v2-faint)' }}>
                                {value}
                            </span>
                        </div>
                    ))}
                    <div className='flex' data-v2-stack='' style={{ gap: 6, paddingTop: 4 }}>
                        <button
                            type='button'
                            onClick={() => props.onNavigate('settings')}
                            title={SHELL_COPY.settingsButton}
                            className='flex h-8 min-h-0 flex-grow items-center justify-center gap-1.5 rounded-[7px] border text-[12px]'
                            style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}
                        >
                            <Glyph name='sliders' size={14} className='hidden [@media(max-width:1279px)]:block' />
                            <span data-v2-label>{SHELL_COPY.settingsButton}</span>
                        </button>
                        <button
                            type='button'
                            role='switch'
                            aria-checked={dark}
                            aria-label={SHELL_COPY.themeToggle}
                            aria-busy={props.appearancePending || undefined}
                            title={SHELL_COPY.themeToggle}
                            onClick={() => props.onSetAppearance(dark ? 'light' : 'dark')}
                            className='grid h-8 w-9 min-h-0 shrink-0 place-items-center rounded-[7px] border'
                            style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}
                        >
                            <Glyph name={dark ? 'sun' : 'moon'} size={14} />
                        </button>
                    </div>
                    {account && (
                        <div data-slot='user-row' data-v2-collapse='' className='flex items-center gap-2 text-[12px]' style={{ paddingTop: 6, color: 'var(--v2-ink)' }}>
                            <span aria-hidden='true' className='grid size-[22px] place-items-center rounded-full text-[10px] font-bold' style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}>
                                {account
                                    .split(/\s+/)
                                    .slice(0, 2)
                                    .map((w) => w[0]?.toUpperCase() ?? '')
                                    .join('')}
                            </span>
                            <span className='truncate'>{account}</span>
                        </div>
                    )}
                    {snapshot.fixtureBuild && (
                        <p data-slot='build-identity' data-v2-collapse='' className='font-mono text-[10px] tracking-[0.14em] uppercase' style={{ paddingTop: 6, color: 'var(--v2-flag)' }}>
                            {SHELL_COPY.fixture}
                        </p>
                    )}
                </div>
            </nav>
            <div className='flex min-w-0 flex-grow flex-col' aria-hidden={modalOpen ? true : undefined} inert={modalOpen ? true : undefined}>
                <header
                    data-slot='app-header'
                    data-v2-header=''
                    data-desktop-theme=''
                    className='flex shrink-0 items-center'
                    style={{ height: 'var(--v2-header)', gap: 14, padding: '0 20px 0 44px', borderBottom: '1px solid var(--v2-rule)', background: 'var(--v2-bg)' }}
                >
                    <div className='flex min-w-0 items-baseline gap-2'>
                        <h1 ref={heading} id='page-heading' data-slot='page-heading' tabIndex={-1} className='min-w-0 truncate text-[18px] font-bold'>
                            <span data-slot='header-title'>{feature.label}</span>
                        </h1>
                        {subtitle && (
                            <span data-slot='header-subtitle' className='truncate text-[13px] font-normal' style={{ color: 'var(--v2-faint)' }}>
                                {'  ›  '}
                                {subtitle}
                            </span>
                        )}
                    </div>
                    <button
                        type='button'
                        data-slot='palette-trigger'
                        data-v2-search=''
                        onClick={openPalette}
                        className='ml-auto flex h-8 min-h-0 w-[260px] shrink-0 items-center gap-2 rounded-[7px] px-2.5 text-[13px]'
                        style={{ border: 0, background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}
                    >
                        <Glyph name='search' size={14} />
                        <span className='flex-grow truncate text-left'>{SHELL_COPY.searchOrGo}</span>
                        <kbd aria-label='Command K' className='text-[12px]'>
                            ⌘K
                        </kbd>
                    </button>
                    <button
                        type='button'
                        role='switch'
                        ref={explainButton}
                        aria-checked={explainOpen}
                        aria-controls='explain-drawer'
                        aria-label={SHELL_COPY.showCalls}
                        aria-describedby='v2-show-calls-hint'
                        title={SHELL_COPY.showCallsHint}
                        onClick={() => setExplain(!explainOpen)}
                        onKeyDown={(event) => {
                            if (event.key === 'Escape' && explainOpen && !modalOpen) {
                                event.preventDefault()
                                event.stopPropagation()
                                setExplain(false)
                            }
                        }}
                        className='flex h-8 min-h-0 shrink-0 items-center rounded-[7px] px-2 text-[12px]'
                        style={{ gap: 7, border: 0, background: 'transparent', color: 'var(--v2-soft)' }}
                    >
                        <span
                            aria-hidden='true'
                            className='relative inline-block h-4 w-[26px] rounded-full'
                            style={{ background: explainOpen ? 'var(--v2-acc)' : 'var(--v2-inset)', boxShadow: `inset 0 0 0 1px ${explainOpen ? 'var(--v2-acc)' : 'var(--v2-ctl)'}` }}
                        >
                            <span className='absolute top-0.5 size-3 rounded-full' style={{ left: explainOpen ? 12 : 2, background: explainOpen ? 'var(--v2-acc-ink)' : 'var(--v2-ctl)' }} />
                        </span>
                        <span data-v2-label>{SHELL_COPY.explain}</span>
                        <span id='v2-show-calls-hint' className='sr-only'>
                            {SHELL_COPY.showCallsHint}
                        </span>
                    </button>
                </header>
                <div className='relative flex min-h-0 flex-grow'>
                    <main data-slot='app-main' data-v2-page='' aria-labelledby='page-heading' data-desktop-theme={feature.id === 'academy' ? undefined : ''} className='flex min-w-0 flex-grow flex-col'>
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
                            <Home snapshot={snapshot} onAct={props.onAct} onNavigate={props.onNavigate} />
                        ) : feature.id === 'inbox' ? (
                            <Inbox snapshot={snapshot} onAct={props.onAct} onNavigate={props.onNavigate} />
                        ) : feature.id === 'administration' && feature.availability.state === 'available' ? (
                            <Administration
                                key={snapshot.active?.context ?? 'none'}

                                snapshot={snapshot}
                                dispatch={props.dispatch}
                                client={props.client}
                                onAct={props.onAct}
                                onNavigate={props.onNavigate}
                                arrival={arrival && arrival.tenant === tenantNow && arrival.feature === 'administration' ? arrival : null}
                            />
                        ) : feature.id === 'harness' && feature.availability.state === 'available' ? (
                            <Harness
                                key={snapshot.active?.context ?? 'none'}

                                snapshot={snapshot}
                                dispatch={props.dispatch}
                                onNavigate={props.onNavigate}
                                arrival={arrival && arrival.tenant === tenantNow && arrival.feature === 'harness' ? arrival : null}
                            />
                        ) : feature.id === 'library' && feature.availability.state === 'available' ? (
                            // Keyed by context: a new tenant starts at its first page, nothing inspected.
                            <TestCases key={snapshot.active?.context ?? 'none'} snapshot={snapshot} dispatch={props.dispatch} onNavigate={props.onNavigate} />
                        ) : feature.id === 'academy' && feature.availability.state === 'available' ? (
                            // Not keyed by tenant: the learner generation is the account's, not the tenant's.
                            <Academy snapshot={snapshot} client={props.client} onNavigate={props.onNavigate} />
                        ) : feature.id === 'apiClient' && feature.availability.state === 'available' ? (
                            // Not keyed by tenant: API Client documents are on this Mac.
                            <ApiClient snapshot={snapshot} dispatch={props.dispatch} observe={observeWorkbench} />
                        ) : (feature.id === 'runtime' || feature.id === 'gateway' || feature.id === 'redTeam') && feature.availability.state === 'available' ? (
                            // C44: the product destinations (Go, the palette) open Administration on their product; the legacy views are gone.
                            <Administration
                                key={snapshot.active?.context ?? 'none'}

                                snapshot={snapshot}
                                dispatch={props.dispatch}
                                client={props.client}
                                onAct={props.onAct}
                                onNavigate={props.onNavigate}
                                arrival={tenantNow ? { tenant: tenantNow, feature: 'administration', resource: null, item: null, ...PRODUCT_DESTINATION[feature.id] } : null}
                            />
                        ) : (
                            <FeatureBody

                                feature={feature}
                                snapshot={snapshot}
                                appearancePending={props.appearancePending}
                                onSetAppearance={props.onSetAppearance}
                                onNavigate={props.onNavigate}
                                dispatch={props.dispatch}
                                client={props.client}
                            />
                        )}
                        <p role='status' aria-live='polite' data-slot='shell-status' className='mt-4 text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                            {props.status}
                        </p>
                    </main>
                    {explainOpen && snapshot.calls !== undefined && <ExplainDrawer snapshot={snapshot} dispatch={props.dispatch} onClose={() => setExplain(false)} returnFocus={explainOpener.current} modalOpen={modalOpen} />}
                </div>
            </div>
            {paletteOpen && (
                <Palette snapshot={snapshot} onNavigate={props.onNavigate} onSwitchTenant={props.onSwitchTenant} onReadNow={props.onReadNow} onClose={closePalette} />
            )}
            {guard && guardTarget && (
                <SwitchGuard target={guardTarget} completion={false} binding={guard} onStay={() => void props.dispatch({ kind: 'dismissSwitchGuard' })} onResolve={(resolution) => props.onSwitchTenant(guard.to, resolution)} />
            )}
            {adding && (
                <Modal label='Add a tenant' onClose={() => setCloseRequests((n) => n + 1)} width={1160} padding={0} data-slot='add-tenant-sheet'>
                    <Wizard
                        snapshot={snapshot}
                        dispatch={props.dispatch}
                        observeManagement={(operation) => props.client.observeManagement(operation)}
                        mode='add'
                        onClose={closeSheet}
                        closeRequests={closeRequests}
                        onNavigate={(feature) => {
                            setAdding(false)
                            props.onNavigate(feature)
                        }}
                    />
                </Modal>
            )}
            {/* Toasts are background siblings of a modal too (I8): inert and hidden while one is open. */}
            <div aria-hidden={modalOpen ? true : undefined} inert={modalOpen ? true : undefined}>
                <Toasts />
            </div>
        </div>
    )
}
