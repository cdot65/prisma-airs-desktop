import { forwardRef, useCallback } from 'react'
import type { DesktopClient, FeatureEntry, FeatureId, ShellSnapshot, Theme } from '@prisma-airs/desktop-contracts'
import { SessionsView } from '../features/sessions/sessions-view'
import { ImportView } from '../features/import-v1/import-view'
import type { Dispatch } from '../features/tenants/dispatch'
import { SettingsView, UnavailableView } from './feature-view'

export interface FeatureBodyProps {
    feature: FeatureEntry
    snapshot: ShellSnapshot
    appearancePending: boolean
    onSetAppearance: (theme: Theme) => void
    onNavigate: (id: FeatureId) => void
    dispatch: Dispatch
    /** The native client, for management observations. */
    client: DesktopClient
}

/**
 * The destinations the v2 shell frames without a board of their own (C44):
 * Settings, Sessions & Reports and the reviewed v1 import, keyed by the
 * active context where a view must never outlive its credential. Every
 * other destination is a v2 screen; the legacy shell and its views are gone.
 */
export const FeatureBody = forwardRef<HTMLHeadingElement, FeatureBodyProps>(function FeatureBody(props, heading) {
    const { feature, snapshot } = props
    const client = props.client
    const observeManagement = useCallback((operation: string) => client.observeManagement(operation), [client])
    return (
        <>
            {feature.id === 'settings' ? (
                <SettingsView
                    ref={heading}
                    snapshot={snapshot}
                    pending={props.appearancePending}
                    onSetAppearance={props.onSetAppearance}
                    dispatch={props.dispatch}
                    observeManagement={observeManagement}
                />
            ) : feature.id === 'sessions' && feature.availability.state === 'available' ? (
                <SessionsView
                    // A new tenant or management credential remounts the view:
                    // revealed content never outlives its credential.
                    key={snapshot.active?.context ?? 'none'}
                    ref={heading}
                    snapshot={snapshot}
                    dispatch={props.dispatch}
                    observe={observeManagement}
                    onNavigate={props.onNavigate}
                />
            ) : feature.id === 'import' && feature.availability.state === 'available' ? (
                <ImportView ref={heading} snapshot={snapshot} dispatch={props.dispatch} />
            ) : (
                <UnavailableView ref={heading} feature={feature} />
            )}
        </>
    )
})
