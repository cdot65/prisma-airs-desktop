import { forwardRef, useCallback, useEffect, useState } from 'react'
import type { DesktopClient, FeatureEntry, FeatureId, ShellSnapshot, Theme } from '@prisma-airs/desktop-contracts'
import { AcademyView } from '../features/academy/academy-view'
import { AdministrationView } from '../features/administration/administration-view'
import { SessionsView } from '../features/sessions/sessions-view'
import { type CasePrefill, LibraryView } from '../features/library/library-view'
import { RedTeamView } from '../features/red-team/red-team-view'
import { GatewayView } from '../features/gateway/gateway-view'
import { HarnessView } from '../features/harness/harness-view'
import { ImportView } from '../features/import-v1/import-view'
import { RuntimeView } from '../features/runtime/runtime-view'
import { WorkbenchView } from '../features/workbench/workbench-view'
import type { Dispatch } from '../features/tenants/dispatch'
import type { Arrival } from '../v2/store'
import { SettingsView, UnavailableView } from './feature-view'

export interface FeatureBodyProps {
    feature: FeatureEntry
    snapshot: ShellSnapshot
    appearancePending: boolean
    onSetAppearance: (theme: Theme) => void
    onNavigate: (id: FeatureId) => void
    dispatch: Dispatch
    /** The native client, for the Academy learner transport and observations. */
    client: DesktopClient
    /** What a contextual action asked this destination to select (C30); v2 only. */
    arrival?: Arrival | null
}

/**
 * The body of one destination: every feature view the desktop ships, keyed
 * by the active context where a view must never outlive its credential.
 * Shared by the legacy shell and the v2 shell (C28), so a destination
 * behaves identically whichever shell frames it.
 */
export const FeatureBody = forwardRef<HTMLHeadingElement, FeatureBodyProps>(function FeatureBody(props, heading) {
    const { feature, snapshot } = props
    const client = props.client
    const observeManagement = useCallback((operation: string) => client.observeManagement(operation), [client])
    const observeWorkbench = useCallback((operation: string) => client.observeWorkbench(operation), [client])
    // A Library case or starter to fill the scan form with, bound to the
    // context it was loaded in: another tenant or credential generation
    // never renders it (checked while rendering) and it is dropped when the
    // context changes, so returning never revives it.
    const context = snapshot.active?.context ?? null
    const [loaded, setLoaded] = useState<(CasePrefill & { context: string }) | null>(null)
    const prefill = loaded !== null && loaded.context === context ? loaded : null
    useEffect(() => setLoaded(null), [context])
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
            ) : feature.id === 'administration' && feature.availability.state === 'available' ? (
                <AdministrationView
                    // A new tenant or management credential remounts the view,
                    // so no render ever pairs it with the previous one's rows.
                    key={snapshot.active?.context ?? 'none'}
                    ref={heading}
                    snapshot={snapshot}
                    dispatch={props.dispatch}
                    observe={observeManagement}
                    onNavigate={props.onNavigate}
                    arrival={props.arrival ?? null}
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
            ) : feature.id === 'library' && feature.availability.state === 'available' ? (
                <LibraryView
                    key={snapshot.active?.context ?? 'none'}
                    ref={heading}
                    snapshot={snapshot}
                    dispatch={props.dispatch}
                    onNavigate={props.onNavigate}
                    onLoad={(fields) => {
                        if (context) setLoaded((p) => ({ ...fields, context, nonce: (p?.nonce ?? 0) + 1 }))
                    }}
                />
            ) : feature.id === 'redTeam' && feature.availability.state === 'available' ? (
                <RedTeamView
                    key={context ?? 'none'}
                    ref={heading}
                    snapshot={snapshot}
                    dispatch={props.dispatch}
                    observe={observeManagement}
                    onNavigate={props.onNavigate}
                />
            ) : feature.id === 'gateway' && feature.availability.state === 'available' ? (
                <GatewayView
                    key={context ?? 'none'}
                    ref={heading}
                    snapshot={snapshot}
                    dispatch={props.dispatch}
                    observe={observeManagement}
                    onNavigate={props.onNavigate}
                />
            ) : feature.id === 'harness' && feature.availability.state === 'available' ? (
                // Keyed by context: a new tenant starts with nothing held.
                <HarnessView key={context ?? 'none'} ref={heading} snapshot={snapshot} dispatch={props.dispatch} onNavigate={props.onNavigate} />
            ) : feature.id === 'import' && feature.availability.state === 'available' ? (
                <ImportView ref={heading} snapshot={snapshot} dispatch={props.dispatch} />
            ) : feature.id === 'apiClient' && feature.availability.state === 'available' ? (
                // Not keyed by tenant: API Client documents are on this Mac,
                // independent of the selected tenant.
                <WorkbenchView ref={heading} snapshot={snapshot} dispatch={props.dispatch} observe={observeWorkbench} />
            ) : feature.id === 'academy' && feature.availability.state === 'available' ? (
                <AcademyView ref={heading} snapshot={snapshot} client={props.client} onNavigate={props.onNavigate} />
            ) : feature.id === 'runtime' && feature.availability.state === 'available' ? (
                // Keyed by context like the other views: a new tenant or
                // credential generation starts a fresh form, case name and
                // save status, and a late reply for the old one has nowhere
                // to land.
                <RuntimeView
                    key={context ?? 'none'}
                    ref={heading}
                    snapshot={snapshot}
                    dispatch={props.dispatch}
                    onNavigate={props.onNavigate}
                    prefill={prefill}
                />
            ) : (
                <UnavailableView ref={heading} feature={feature} />
            )}
        </>
    )
})
