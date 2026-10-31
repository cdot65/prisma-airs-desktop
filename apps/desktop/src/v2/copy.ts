import type { TenantEnvironment } from '@prisma-airs/desktop-contracts'

/** A tenant's environment as the shell names it (C29). */
export const ENVIRONMENT_LABELS: Record<TenantEnvironment, string> = { production: 'Production', staging: 'Staging', other: 'Other' }

/** Every string the v2 shell shows (DESIGN-V2 §3: one copy file per screen family). */
export const SHELL_COPY = {
    brand: 'Prisma AIRS',
    tagline: 'Desktop v2',
    navLabel: 'Destinations',
    tenantSwitcher: 'Switch tenant',
    tenantNone: 'No tenant yet',
    addTenant: 'Add a tenant…',
    tenantAdded: (tenant: string) => `${tenant} is set up and active.`,
    palette: 'Go to…',
    paletteLabel: 'Command palette',
    paletteInput: 'Go to a destination, tenant or action',
    paletteEmpty: 'Nothing matches.',
    paletteSwitch: (name: string) => `Switch to ${name}`,
    paletteReadNow: 'Read now',
    unavailableHint: (reason: string) => `Not available: ${reason}`,
    appearance: 'Appearance',
    appearanceSystem: 'Match system',
    appearanceLight: 'Light',
    appearanceDark: 'Dark',
    showCalls: 'Show the calls',
    showCallsHint: 'Shows each request this app sends to the public APIs as you work.',
    // The header (R2App): the Explain switch is the show-the-calls preference.
    explain: 'Explain',
    searchOrGo: 'Search or go to…',
    askHarness: 'Ask Harness',
    tenantMenu: 'Tenants',
    tenantSettings: 'Tenant settings and credentials…',
    changeWorkspace: 'Change Gateway workspace…',
    switchTenantHeading: 'Switch tenant',
    addTenantRow: 'Add a tenant',
    workspaceOf: (name: string) => `Workspace ${name}`,
    tsgOf: (tsg: string) => `TSG ${tsg}`,
    signedInAs: (who: string) => `signed in as ${who}`,
    connected: 'Connected',
    harnessRow: 'Harness',
    harnessSignedIn: 'Signed in · SSO',
    harnessKey: 'Workspace key',
    harnessOff: 'Not signed in',
    harnessRenew: 'Sign in again',
    harnessPending: 'Sign in needed',
    harnessNoWorkspace: 'No workspace',
    changeWorkspaceGate: 'Choose a tenant first',
    keychainRow: 'Keychain',
    keychainUnlocked: 'Unlocked',
    keychainLocked: 'Locked',
    keychainUnavailable: 'Unavailable',
    jobsRow: 'Jobs',
    jobsNone: 'None running',
    jobsRunning: (n: number) => `${n} running`,
    settingsButton: 'Settings  ⌘,',
    themeToggle: 'Switch light or dark appearance',
    productsToggle: 'Show or hide products',
    newTag: 'New',
    productsLabel: 'Products',
    needAction: (n: number) => `${n} need${n === 1 ? 's' : ''} action`,
    // The prototype's third hint (product-scoped search) names a search this palette does not have; it is left out.
    paletteHints: { move: '↑ ↓ move', open: '↵ open' },
    paletteGroups: { destinations: 'Go to', tenants: 'Tenants', actions: 'Actions' },
    paletteAsk: 'Ask the Harness instead',
    classic: 'Return to the classic interface',
    fixture: 'Fixture build',
    status: 'Status',
    toastRefused: (what: string, reason: string, text: string | null = null) => `${what} was refused (${reason}).${text ? ` ${text}` : ''}`,
    switchTo: (tenant: string) => `The switch to ${tenant}`,
    readOf: (tenant: string) => `The read of ${tenant}`,
    recheckOf: (product: string, tenant: string) => `The recheck of ${product} in ${tenant}`,
    acknowledgmentOf: (title: string) => `The acknowledgment of “${title}”`,
    snoozeOf: (title: string) => `The snooze of “${title}”`,
    unsnoozeOf: (title: string) => `The unsnooze of “${title}”`,
    toastUnknown: (what: string) => `${what} was sent but not confirmed. The current state is shown.`,
    toastFailed: (what: string) => `${what} could not be delivered.`,
    toastPrefix: { ok: 'Done: ', info: 'Note: ', warn: 'Warning: ' } as Record<'ok' | 'info' | 'warn', string>,
    unconfirmed: (what: string) => `${what} was sent but the desktop did not confirm it. The current state is shown.`,
    retry: 'Retry',
    dismiss: 'Dismiss',
} as const

/** Wall-clock HH:MM for a seconds-since-epoch stamp. */
export function clockOf(seconds: number): string {
    const d = new Date(seconds * 1000)
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

/** Day-relative wording for a return time (today, tomorrow, or the date). */
export function returnsWording(seconds: number): string {
    const d = new Date(seconds * 1000)
    const today = new Date()
    const dayOf = (x: Date) => `${x.getFullYear()}-${x.getMonth()}-${x.getDate()}`
    const tomorrow = new Date(today.getTime() + 86_400_000)
    const day = dayOf(d) === dayOf(today) ? 'today' : dayOf(d) === dayOf(tomorrow) ? 'tomorrow' : d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
    return `${day} ${clockOf(seconds)}`
}

/**
 * The one snooze label (I11): the core computes the return time; Home, the
 * Inbox row, the detail pane and the toast all say exactly this.
 */
export const SNOOZE_COPY = {
    /** Before snoozing: what a snooze will do (24 hours, per P20). */
    offer: 'Snooze 24 hours',
    /** After snoozing: when the core says it returns. */
    returns: (returnsAt: number) => `Returns ${returnsWording(returnsAt)}`,
    toast: (title: string, returnsAt: number) => `${title} snoozed · returns ${returnsWording(returnsAt)}`,
}

export const INBOX_COPY = {
    title: 'Inbox',
    tabs: { all: 'All', action: 'Needs action', updates: 'Updates', snoozed: 'Snoozed' } as const,
    severity: { bad: 'Critical', warn: 'Warning', ok: 'Info' } as Record<'bad' | 'warn' | 'ok', string>,
    kind: { runtime: 'AI Runtime Security', gateway: 'AI Gateway', setupWarning: 'Setup', rotation: 'Key rotation' } as Record<'runtime' | 'gateway' | 'setupWarning' | 'rotation', string>,
    emptyPending: 'First read pending. Nothing has been read for this tenant yet.',
    emptyReading: 'Reading this tenant…',
    emptyClear: 'Nothing needs action.',
    /** A partial read with nothing listed: what was read says nothing needs action; the unread sources are unknown (I4). */
    emptyPartial: (read: number, total: number, missing: string[]) => `Nothing needs action in what was read (${read} of ${total} sources). Activity for ${missing.join(', ')} is unknown until it can be read.`,
    emptyFailed: (when: string) => `The read at ${when} failed. Nothing has been read for this tenant yet.`,
    emptyStale: (when: string) => `Nothing needed action at ${when}; that read is more than 10 minutes old. Read again to confirm.`,
    stale: (when: string) => `The read from ${when} is more than 10 minutes old.`,
    readingKept: (when: string) => `A read is in progress; the read from ${when} is shown.`,
    readingStale: (when: string) => `A read is in progress; the read from ${when} shown is more than 10 minutes old.`,
    emptyUnread: (when: string, sources: number) => `The read at ${when} established nothing: ${sources} source${sources === 1 ? '' : 's'} still unread. Nothing has been read for this tenant yet.`,
    failedKept: (when: string, shown: string) => `The read at ${when} failed; the read from ${shown} is shown.`,
    emptyTab: 'Nothing here.',
    partial: (read: number, total: number, when: string) => `${read} of ${total} sources were read at ${when}.`,
    acknowledged: 'Acknowledged',
    acknowledge: 'Acknowledge',
    needsAction: 'Mark as needing action',
    unsnooze: 'Unsnooze',
    why: 'Why this matters',
    facts: 'Facts',
    whyText: {
        runtime: 'Runtime Security reads this from the tenant’s sessions and API keys; an item here changes what the scan key can do or how prompts are being handled.',
        gateway: 'The chosen AI Gateway workspace reported errors or a quota for the day; routes and keys depend on it.',
        setupWarning: 'A check during setup did not pass. The product reads only partially until it is fixed in Administration.',
        rotation: 'An additional API key was issued for this application. Until the old key is removed, both keys work; finish the rotation once the application uses the new key.',
    } as Record<'runtime' | 'gateway' | 'setupWarning' | 'rotation', string>,
    listCard: 'Inbox list',
    sub: (tenant: string) => `Everything that needs you in ${tenant} · newest first`,
    reading: 'Reading…',
    filter: 'Inbox filter',
    openRoleIssue: 'Open role issue',
    footNoReads: (tenant: string) => `No source reads yet for ${tenant}.`,
    footPartial: (read: number, total: number, when: string) => `${read} of ${total} sources read at ${when}.`,
    footAll: (when: string) => `All sources read at ${when}.`,
    footGateway: (workspace: string, policies: number | null) => `AI Gateway ${workspace}${policies === null ? '' : ` · ${policies} limit polic${policies === 1 ? 'y' : 'ies'}`}`,
    readNow: 'Read now',
    detail: 'Details',
    selectHint: 'Select an item to see its details.',
    ariaList: 'Inbox items',
}

export const READ_COPY = {
    finished: (when: string) => `Read finished at ${when} · all sources complete`,
    finishedIncomplete: (when: string, partial: number) => `Read finished at ${when} · ${partial} source${partial === 1 ? '' : 's'} partial`,
    finishedNeedsRole: (when: string, products: number) => `Read finished at ${when} · ${products} product${products === 1 ? '' : 's'} still need${products === 1 ? 's' : ''} a role`,
    finishedNoActivity: (when: string, tenant: string) => `Read finished at ${when} · no activity yet in ${tenant}`,
    failed: (when: string) => `Read failed at ${when} · nothing read yet`,
    failedKept: (when: string, shown: string) => `Read failed at ${when} · showing the read from ${shown}`,
    unread: (sources: number) => `${sources} source${sources === 1 ? ' was' : 's were'} not read.`,
    stale: (when: string) => `Read at ${when} · more than 10 minutes old; read again to confirm`,
    staleQualifier: (when: string) => `The read from ${when} is more than 10 minutes old.`,
    readingKept: (when: string) => `Reading… · showing the read from ${when}`,
    readingStale: (when: string) => `Reading… · the read from ${when} shown is more than 10 minutes old`,
    readingQualifier: (when: string) => `A read is in progress; the read from ${when} is shown.`,
    readingStaleQualifier: (when: string) => `A read is in progress; the read from ${when} shown is more than 10 minutes old.`,
    recheck: (product: string) => `Rechecking ${product}…`,
    reading: (tenant: string) => `Reading ${tenant}…`,
    rechecked: (product: string) => `${product} read again.`,
    productState: { pending: 'Pending', ok: 'Read', incomplete: 'Incomplete', needsRole: 'Needs a role', failed: 'Failed', notRead: 'Not read here' } as Record<string, string>,
    products: { runtimeSecurity: 'AI Runtime Security', gateway: 'AI Gateway', redTeam: 'AI Red Teaming', modelSkill: 'Model and skill scans' } as Record<string, string>,
    sources: (read: number, total: number) => (total === 0 ? '' : `${read} of ${total} sources`),
}

export const GUARD_COPY = {
    title: 'A Harness decision is waiting',
    body: (tenant: string) => `The conversation in this tenant is waiting for your approval. Switching to ${tenant} ends it. Keep its transcript, drop it, or stay.`,
    bodyCompletion: (tenant: string) => `The conversation in this tenant is waiting for your approval. Finishing the setup of ${tenant} makes it active and ends the conversation. Keep its transcript, drop it, or stay.`,
    archive: 'Archive and switch',
    discard: 'Discard and switch',
    stay: 'Stay',
}

export const HOME_COPY = {
    title: 'Home',
    greeting: (account: string) => `Good ${(() => { const h = new Date().getHours(); return h < 12 ? 'morning' : h < 18 ? 'afternoon' : 'evening' })()}, ${account.split(/\s+/)[0]}`,
    refresh: 'Refresh',
    recheck: 'Recheck',
    cards: 'Product cards',
    sub: {
        products: (n: number) => `${n} product${n === 1 ? '' : 's'}`,
        needAction: (n: number) => `${n} need${n === 1 ? 's' : ''} action`,
        partial: (n: number) => `${n} source${n === 1 ? '' : 's'} partial`,
    },
    explain: 'Each card is one product: its headline figure for the period shown and how fresh it is. "Partial" means a source answered only in part, so the figure may be low. Open a card to manage that product in Administration.',
    pill: { connected: 'Connected', partial: 'Partial', needsRole: 'Needs a role', connecting: 'Connecting', failed: 'Failed', unavailable: 'Not available' },
    metric: { runtimeSecurity: 'detection events · last read', gateway: 'requests · today', redTeam: 'sources read', modelSkill: 'sources read', agenticIdentity: '', agenticEndpoint: '' } as Record<import('@prisma-airs/desktop-contracts').AdminProductId, string>,
    cardNeedsRole: (detail: string | null) => `${detail ?? 'Missing read role'} · check again in Administration`,
    runtimeViolating: (n: number) => `${n} violating session${n === 1 ? '' : 's'}`,
    noDetections: 'No detections in this read',
    gatewayErrorRate: (rate: number) => `Error rate ${rate.toFixed(1)}% today`,
    gatewayNoRequests: (workspace: string) => `No requests today · ${workspace}`,
    gatewayNoWorkspace: 'No Gateway workspace chosen · choose one in Administration',
    usageNotRead: (workspace: string) => `Usage not read · ${workspace}`,
    errorsUnknown: (workspace: string) => `Error count unknown · ${workspace}`,
    chartNotRead: 'Sessions chart not read · figure may be low',
    activityUnknown: 'Activity unknown · check again in Administration',
    partialRead: (read: number, total: number) => `Partial · ${read} of ${total} sources`,
    readOnly: 'Read-only here',
    openRoleIssue: 'Open role issue',
    acknowledge: 'Acknowledge',
    quickActions: 'Quick actions',
    quick: {
        prompt: 'Test a prompt',
        promptSub: 'AI Runtime Security · Playground',
        harness: 'Ask the Harness',
        harnessSub: 'Investigate with your tools',
        request: 'Send a request',
        requestSub: 'API Client · built-in catalog',
        learn: 'Keep learning',
        learnSub: 'Academy courses and labs',
    },
    noTenant: 'Add a tenant to start reading it.',
    firstRead: 'First read pending',
    reading: 'Reading…',
    readAt: (when: string) => `Read finished at ${when}`,
    readFailed: (when: string) => `Last read failed at ${when}; the previous read is shown`,
    attention: 'Needs attention',
    allClear: 'Nothing needs action.',
    clearSoFar: (qualifier: string) => `Nothing needs action in what was read. ${qualifier}`,
    inboxSoon: 'Items that need action appear here once the Inbox is available.',
    openInbox: 'Open the Inbox',
    products: 'Products',
    readNow: 'Read now',
    capabilities: 'Capabilities in this tenant',
    scan: 'Runtime Security scan key',
    management: 'Management client',
    inference: 'Gateway workspace key',
    state: {
        missing: 'Not configured',
        storing: 'Storing…',
        ready: 'Ready',
        failed: 'Failed',
    } as Record<string, string>,
    destinations: 'Destinations',
} as const
