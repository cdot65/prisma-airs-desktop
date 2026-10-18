import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import {
    DeliveryOutcomeUnknown,
    type DesktopClient,
    type FeatureId,
    type Intent,
    type MenuCommand,
    type ShellSnapshot,
    type Theme,
} from '@prisma-airs/desktop-contracts'
import { followAppearance } from './shell/appearance'
import type { DispatchOutcome } from './features/tenants/dispatch'
import { DesktopShell } from './shell/desktop-shell'
import { forgetAcademyPresentation } from './features/academy/presentation-storage'

export function App({ client }: { client: DesktopClient }) {
    const [snapshot, setSnapshot] = useState<ShellSnapshot | null>(null)
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

    const onMenu = useRef<(command: MenuCommand) => void>(() => undefined)
    onMenu.current = (command: MenuCommand) => {
        if (command.kind === 'navigate') {
            // Native navigation dismisses the modal palette first so focus can
            // move to the new page heading instead of behind an open dialog.
            setPaletteOpen(false)
            setActive(command.feature)
        }
        else if (command.kind === 'commandPalette') setPaletteOpen(true)
        else void setAppearance(command.theme)
    }

    useEffect(() => {
        let stop: (() => void) | undefined
        let cancelled = false
        client
            .observeShell({
                onSnapshot: (next) => setSnapshot(next),
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
