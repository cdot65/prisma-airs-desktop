import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import {
    DeliveryOutcomeUnknown,
    type DesktopClient,
    type FeatureId,
    type Intent,
    type MenuCommand,
    type Refusal,
    type ShellSnapshot,
    type Theme,
} from '@prisma-airs/desktop-contracts'
import { followAppearance } from './shell/appearance'
import { type DispatchOutcome, outcomeMessage } from './features/tenants/dispatch'
import { DesktopShell } from './shell/desktop-shell'
import { V2Shell } from './v2/v2-shell'
import { Wizard } from './v2/wizard/wizard'
import { ImportView } from './features/import-v1/import-view'
import { useShellStore } from './v2/store'
import { SHELL_COPY } from './v2/copy'
import { forgetAcademyPresentation } from './features/academy/presentation-storage'
import { SAVE_DOCUMENT_EVENT } from './features/workbench/document-editor'

export function App({ client }: { client: DesktopClient }) {
    const [snapshot, setSnapshot] = useState<ShellSnapshot | null>(null)
    const snapshotRef = useRef<ShellSnapshot | null>(null)
    snapshotRef.current = snapshot
    const [failure, setFailure] = useState<string | null>(null)
    const [active, setActive] = useState<FeatureId | null>(null)
    const [paletteOpen, setPaletteOpen] = useState(false)
    const [pending, setPending] = useState(false)
    const [status, setStatus] = useState('')

    const setAppearance = useCallback(
        async (theme: Theme) => {
            setPending(true)
            setStatus('')
            try {
                const response = await client.dispatch({ kind: 'setAppearance', theme })
                if (response.outcome === 'refusal') {
                    setStatus(`The desktop refused the appearance change (${response.refusal}).`)
                }
            } catch (error) {
                setStatus(
                    error instanceof DeliveryOutcomeUnknown
                        ? 'The desktop did not confirm the appearance change. The current setting is shown.'
                        : 'The appearance change could not be delivered.'
                )
            } finally {
                setPending(false)
            }
        },
        [client]
    )

    const dispatch = useCallback(
        async (intent: Intent): Promise<DispatchOutcome> => {
            try {
                const response = await client.dispatch(intent)
                return response.outcome === 'refusal'
                    ? { kind: 'refused', refusal: response.refusal }
                    : { kind: 'admitted', operation: response.receipt.operation }
            } catch (error) {
                return error instanceof DeliveryOutcomeUnknown ? { kind: 'unknown' } : { kind: 'failed' }
            }
        },
        [client]
    )

    // v2 (C28): navigation is Rust-owned. The native menu, the sidebar and
    // the palette all dispatch `navigate`; refusals reach the status surface.
    const v2 = snapshot?.uiVariant === 'v2'
    const v2Navigate = useCallback(
        async (feature: FeatureId) => {
            useShellStore.getState().closePalette()
            const intent: Intent = { kind: 'navigate', feature }
            const outcome = await dispatch(intent)
            if (outcome.kind === 'refused') useShellStore.getState().toast(SHELL_COPY.toastRefused('Navigation', outcome.refusal), 'warn')
            // Unknown delivery is kept inline with Retry; never an expiring toast alone.
            else if (outcome.kind === 'unknown') useShellStore.getState().addUnconfirmed('Navigation', intent)
            else if (outcome.kind === 'failed') useShellStore.getState().toast(SHELL_COPY.toastFailed('Navigation'), 'warn')
        },
        [dispatch]
    )
    // Every v2 action's outcome reaches the shell's surfaces (DESIGN-V2 §7):
    // a refusal or failed delivery as a warn toast naming what was asked, an
    // unconfirmed delivery inline with Retry, never replayed on its own.
    const v2Act = useCallback(
        async (what: string, intent: Intent, quiet: Refusal[] = []): Promise<DispatchOutcome> => {
            // A context-bound action speaks for the tenant it was sent in (I2).
            const boundContext = typeof (intent as { context?: unknown }).context === 'string' ? (intent as { context: string }).context : null
            const bound = boundContext !== null ? (snapshotRef.current?.active?.tenant ?? null) : null
            const outcome = await dispatch(intent)
            if (outcome.kind === 'refused' && quiet.includes(outcome.refusal)) return outcome
            if (outcome.kind === 'refused') useShellStore.getState().toast(SHELL_COPY.toastRefused(what, outcome.refusal, outcomeMessage(outcome)), 'warn', undefined, bound, boundContext)
            else if (outcome.kind === 'unknown') useShellStore.getState().addUnconfirmed(what, intent)
            else if (outcome.kind === 'failed') useShellStore.getState().toast(SHELL_COPY.toastFailed(what), 'warn', undefined, bound, boundContext)
            return outcome
        },
        [dispatch]
    )
    const v2Preference = useCallback(
        async (what: string, intent: Intent, quiet: Refusal[] = []) => {
            await v2Act(what, intent, quiet)
        },
        [v2Act]
    )

    const onMenu = useRef<(command: MenuCommand) => void>(() => undefined)
    onMenu.current = (command: MenuCommand) => {
        if (command.kind === 'navigate') {
            // Native navigation dismisses the modal palette first so focus can
            // move to the new page heading instead of behind an open dialog.
            if (v2) void v2Navigate(command.feature)
            else {
                setPaletteOpen(false)
                setActive(command.feature)
            }
        }
        else if (command.kind === 'commandPalette') {
            if (v2) useShellStore.getState().openPalette()
            else setPaletteOpen(true)
        }
        // ⌘T: the tenant switcher opens; nothing switches until chosen.
        else if (command.kind === 'tenantSwitcher') {
            if (v2) useShellStore.getState().setSwitcher(true)
            else {
                setPaletteOpen(false)
                setActive('settings')
            }
        }
        // The open API Client request saves (its editor listens).
        else if (command.kind === 'saveDocument') window.dispatchEvent(new Event(SAVE_DOCUMENT_EVENT))
        else if (v2) void v2Preference('The appearance change', { kind: 'setAppearance', theme: command.theme })
        else void setAppearance(command.theme)
    }

    useEffect(() => {
        let stop: (() => void) | undefined
        let cancelled = false
        client
            .observeShell({
                onSnapshot: (next) => {
                    // The shell's scope follows the snapshot before it renders: a
                    // notification for another tenant or context is never surfaced.
                    useShellStore.getState().setScope(next.active?.tenant ?? null, next.active?.context ?? null)
                    setSnapshot(next)
                },
                onMenu: (command) => onMenu.current(command),
                onError: () => setFailure('The desktop sent an unexpected update.'),
            })
            .then(
                (unsubscribe) => {
                    if (cancelled) unsubscribe()
                    else stop = unsubscribe
                },
                () => setFailure('The desktop shell could not be observed.')
            )
        return () => {
            cancelled = true
            stop?.()
        }
    }, [client])

    // A completed reset also forgets Academy's WebView presentation state
    // (completion latches keyed by account); a stopped reset keeps it.
    const storage = snapshot?.storage
    const resetFailed = Boolean(snapshot?.resetFailure)
    const previousStorage = useRef(storage)
    useEffect(() => {
        if (previousStorage.current === 'resetting' && storage !== 'resetting' && !resetFailed) {
            forgetAcademyPresentation()
        }
        previousStorage.current = storage
    }, [storage, resetFailed])

    const appearance = snapshot?.appearance
    // Apply the theme in the same commit as the snapshot, before paint, so
    // the shell never shows a frame in the previous theme.
    useLayoutEffect(() => (appearance ? followAppearance(appearance) : undefined), [appearance])

    if (failure && !snapshot) {
        return (
            <main role='alert' className='text-academy-ink p-8'>
                {failure}
            </main>
        )
    }
    if (!snapshot) {
        return (
            <main role='status' aria-live='polite' className='text-academy-ink-soft p-8'>
                Starting Prisma AIRS Desktop…
            </main>
        )
    }
    if (snapshot.uiVariant === 'v2' && snapshot.onboarding !== 'done') {
        // First run: the wizard owns the window. "Import from v1" is the one
        // way out of it before a tenant exists: the reviewed C23 import renders
        // on its own, and Home brings the wizard back (R5).
        if (snapshot.activeFeature === 'import') {
            return (
                <div data-v2-shell='' data-slot='first-run-import' className='flex min-h-screen flex-col' style={{ background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}>
                    <div className='flex items-center gap-3 border-b px-6 py-3' style={{ borderColor: 'var(--v2-rule)' }}>
                        <button type='button' onClick={() => void v2Navigate('home')} className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                            {SHELL_COPY.backToSetup}
                        </button>
                        <span className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                            {SHELL_COPY.importDuringSetup}
                        </span>
                    </div>
                    <main className='flex-grow p-6'>
                        <ImportView snapshot={snapshot} dispatch={dispatch} />
                    </main>
                </div>
            )
        }
        return (
            <div data-v2-shell=''>
                <Wizard
                    snapshot={snapshot}
                    dispatch={dispatch}
                    observeManagement={(operation) => client.observeManagement(operation)}
                    mode='firstRun'
                    onClose={() => undefined}
                    onNavigate={(feature) => void v2Navigate(feature)}
                />
            </div>
        )
    }
    if (snapshot.uiVariant === 'v2') {
        return (
            <V2Shell
                snapshot={snapshot}
                appearancePending={pending}
                status={failure ?? status}
                onNavigate={(feature) => void v2Navigate(feature)}
                onSetAppearance={(theme) => void v2Preference('The appearance change', { kind: 'setAppearance', theme })}
                onSetShowCalls={(on) => void v2Preference('The preference change', { kind: 'setShowCalls', on })}
                onReturnToClassic={() => void v2Preference('The interface change', { kind: 'setUiVariant', variant: 'legacy' })}
                onSwitchTenant={(tenant, resolution) => {
                    // The target is captured now: a late refusal still names it. A
                    // pending-decision refusal is not a toast: the snapshot carries
                    // the guard the shell renders (I1).
                    const label = snapshot.tenants.find((t) => t.id === tenant)?.label ?? tenant
                    void v2Preference(SHELL_COPY.switchTo(label), { kind: 'selectTenant', tenant, resolution: resolution ?? null }, ['decisionPending'])
                }}
                onReadNow={() => {
                    // The palette's Read now is the same finite read of every chosen product (I4).
                    const active = snapshot.active
                    if (!active) return
                    const label = snapshot.tenants.find((t) => t.id === active.tenant)?.label ?? active.tenant
                    void v2Preference(SHELL_COPY.readOf(label), { kind: 'readTenant', context: active.context })
                }}
                onRetry={(what, intent) => void v2Preference(what, intent)}
                onAct={v2Act}
                dispatch={dispatch}
                client={client}
            />
        )
    }
    return (
        <DesktopShell
            snapshot={snapshot}
            active={active ?? snapshot.features[0]?.id ?? 'settings'}
            paletteOpen={paletteOpen}
            appearancePending={pending}
            status={failure ?? status}
            onNavigate={setActive}
            onOpenPalette={() => setPaletteOpen(true)}
            onClosePalette={() => setPaletteOpen(false)}
            onSetAppearance={(theme) => void setAppearance(theme)}
            dispatch={dispatch}
            client={client}
        />
    )
}
