/**
 * Board fixtures (docs/architecture/ui-v2/screens/boards.md): every reviewed
 * prototype state as a native snapshot the real DesktopClient parses. Built
 * from the wire fixture so a drifted contract fails here first.
 */
import type { FeatureEntry, InboxEntry, ShellSnapshot, TenantView } from '@prisma-airs/desktop-contracts'
import { baseSnapshot, tenant, withTenant } from '../test/native'

/** The v2 catalogue exactly as desktop-core's `catalogue_for(V2)` lists it (C28). */
export const V2_FEATURES: FeatureEntry[] = [
    { id: 'home', label: 'Home', availability: { state: 'available' }, shortcut: 1, sidebar: true },
    { id: 'inbox', label: 'Inbox', availability: { state: 'available' }, shortcut: 2, sidebar: true },
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

const KEY_ITEM: InboxEntry = {
    id: 'runtime-key:k1',
    kind: 'runtime',
    needsAction: true,
    severity: 'warn',
    title: 'Runtime expires in 5 days',
    detail: 'The API key "Runtime" expires on 1 November.',
    source: 'API keys · read 07:45',
    product: 'runtimeSecurity',
    target: { feature: 'administration', product: 'runtimeSecurity', resource: 'apiKeys', item: 'Runtime' },
    actionLabel: 'Open API keys',
    facts: [{ label: 'Key', value: 'Runtime' }, { label: 'Expires', value: '2026-11-01' }],
    when: 1_761_490_000,
    hold: null,
}
const ACTIVITY_ITEM: InboxEntry = {
    id: 'runtime-activity',
    kind: 'runtime',
    needsAction: false,
    severity: 'ok',
    title: '15 sessions in the last day, 3 violating',
    detail: 'Runtime Security scanned 15 sessions.',
    source: 'Sessions chart · read 07:45',
    product: 'runtimeSecurity',
    target: { feature: 'sessions', product: 'runtimeSecurity', resource: null, item: null },
    actionLabel: 'Open Sessions',
    facts: [],
    when: 1_761_490_000,
    hold: null,
}
const SNOOZED_ITEM: InboxEntry = {
    id: 'gateway-ws-1',
    kind: 'gateway',
    needsAction: true,
    severity: 'bad',
    title: 'Gateway workspace Team A over quota',
    detail: 'The day is over its quota; requests are refused.',
    source: 'Gateway usage · read 07:45',
    product: 'gateway',
    target: { feature: 'gateway', product: 'gateway', resource: null, item: null },
    actionLabel: 'Open AI Gateway',
    facts: [],
    when: 1_761_490_000,
    hold: { kind: 'snooze', returnsAt: 1_761_576_400 },
}
const SETUP_ITEM: InboxEntry = {
    id: 'vw-1',
    kind: 'setupWarning',
    needsAction: true,
    severity: 'warn',
    title: 'AI Gateway: setup warning',
    detail: 'Needs the management client; not checked.',
    source: 'Setup · verify',
    product: 'gateway',
    target: { feature: 'gateway', product: 'gateway', resource: null, item: null },
    actionLabel: 'Open AI Gateway',
    facts: [{ label: 'Product', value: 'AI Gateway' }],
    when: 1_761_480_000,
    hold: null,
}

/** fx.inbox (R2Inbox): Northwind read with partial coverage; items of every kind, one snoozed. */
export function fxInbox(): ShellSnapshot {
    const base = fxSwitcher()
    const nw = tenant({ id: 'tn-0123456789abcdef', label: 'Northwind Health', tsgId: '1762204418', products: ['runtimeSecurity', 'gateway'] })
    return {
        ...base,
        tenants: [nw, base.tenants[1] as TenantView],
        activeFeature: 'inbox',
        // The read the entries came from, fresh at fixture time (within the ten-minute window); partial, so a gap.
        overview: {
            ...base.overview,
            read: { readAt: 1_761_490_000, chart: { bucketSeconds: 3600, buckets: [] }, chartFailure: null, summary: { sessions: 15, violating: 3, benign: 12, detectionEvents: 3 }, keysComplete: true, keysFailure: null, items: [], gaps: ['Gateway limit policies unavailable'], gateway: null },
            fresh: true,
            face: 'gap',
        },
        inbox: {
            items: [KEY_ITEM, ACTIVITY_ITEM, SNOOZED_ITEM, SETUP_ITEM],
            read: {
                state: 'done',
                lastReadAt: 1_761_490_000,
                failedAt: null,
                settledAt: 1_761_490_000,
                products: [
                    { product: 'runtimeSecurity', state: 'ok', sourcesRead: 2, sourcesTotal: 2, detail: null },
                    { product: 'gateway', state: 'incomplete', sourcesRead: 1, sourcesTotal: 2, detail: 'workspace Team A' },
                ],
                sourcesMissing: ['Gateway limits'],
                generation: 1,
                rechecking: null,
            },
            lastSnooze: null,
        },
    }
}

/** fx.fresh: a tenant that has never been read (I12). */
export function fxFresh(): ShellSnapshot {
    return { ...fxSwitcher(), activeFeature: 'home' }
}
