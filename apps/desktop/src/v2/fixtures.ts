/**
 * Board fixtures (docs/architecture/ui-v2/screens/boards.md): every reviewed
 * prototype state as a native snapshot the real DesktopClient parses. Built
 * from the wire fixture so a drifted contract fails here first.
 */
import type { FeatureEntry, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { baseSnapshot, tenant, withTenant } from '../test/native'

const unavailable = (reason: string, plannedChunk: string): FeatureEntry['availability'] => ({ state: 'unavailable', reason, plannedChunk })

/** The v2 catalogue exactly as desktop-core's `catalogue_for(V2)` lists it (C28). */
export const V2_FEATURES: FeatureEntry[] = [
    { id: 'home', label: 'Home', availability: { state: 'available' }, shortcut: 1, sidebar: true },
    { id: 'inbox', label: 'Inbox', availability: unavailable('The Inbox arrives in C30', 'C30'), shortcut: 2, sidebar: true },
    { id: 'harness', label: 'Harness', availability: { state: 'available' }, shortcut: 3, sidebar: true },
    { id: 'academy', label: 'Academy', availability: { state: 'available' }, shortcut: 4, sidebar: true },
    { id: 'library', label: 'Test Cases', availability: { state: 'available' }, shortcut: 5, sidebar: true },
    { id: 'apiClient', label: 'API Client', availability: { state: 'available' }, shortcut: 6, sidebar: true },
    { id: 'administration', label: 'Administration', availability: { state: 'available' }, shortcut: 7, sidebar: true },
    { id: 'runtime', label: 'Runtime Security', availability: { state: 'available' }, shortcut: null, sidebar: false },
    { id: 'gateway', label: 'AI Gateway', availability: { state: 'available' }, shortcut: null, sidebar: false },
    { id: 'redTeam', label: 'Red Team & Models', availability: { state: 'available' }, shortcut: null, sidebar: false },
    { id: 'sessions', label: 'Sessions & Reports', availability: { state: 'available' }, shortcut: null, sidebar: false },
    { id: 'import', label: 'Import from v1', availability: { state: 'available' }, shortcut: null, sidebar: false },
    { id: 'settings', label: 'Settings', availability: { state: 'available' }, shortcut: null, sidebar: false },
]

/** fx.home: the v2 shell on Home with no tenant yet. */
export function fxHome(): ShellSnapshot {
    return { ...baseSnapshot, uiVariant: 'v2', onboarding: 'done', activeFeature: 'home', showCalls: true, features: V2_FEATURES }
}

/** fx.home.dark */
export function fxHomeDark(): ShellSnapshot {
    return { ...fxHome(), appearance: 'dark' }
}

/** fx.switcher: two tenants, the first active. */
export function fxSwitcher(): ShellSnapshot {
    const eu = tenant({ id: 'tn-0123456789abcdef', label: 'Northwind Health', tsgId: '1762204418' })
    const us = tenant({ id: 'tn-fedcba9876543210', label: 'Meridian Retail', tsgId: '1001464285', destination: { kind: 'region', region: 'unitedStates' } })
    const snapshot = withTenant(fxHome(), eu)
    return { ...snapshot, tenants: [eu, us] }
}

/** fx.palette: Home with the palette opened by the native menu in the test. */
export const fxPalette = fxSwitcher
