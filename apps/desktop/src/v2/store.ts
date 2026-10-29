import { create } from 'zustand'

export type ToastKind = 'ok' | 'info' | 'warn'
import type { AdminProductId, FeatureId, Intent, ManagementResource } from '@prisma-airs/desktop-contracts'

/** What a contextual action asked the destination to select on arrival (C30), owned by the tenant it was followed in (I2). */
export interface Arrival {
    tenant: string
    feature: FeatureId
    resource: ManagementResource | null
    /** The item to filter to, by name. */
    item: string | null
    /** The Administration product to open (the rail's product children, C35). */
    product?: AdminProductId
    /** That product's tab to show, by the inventory's tab id (the tenant menu's workspace row, C35). */
    tab?: string
    /** Counts arrivals: a destination already open applies a new one. */
    sequence: number
}

/** A Harness message kept for the operator: refused or never sent, or not confirmed (C32). */
export interface HarnessKept {
    /** This record's own id: each attempt is its own record (C32). */
    id: number
    text: string
    unconfirmed: boolean
    /**
     * For an unconfirmed send: the conversation it was sent to and the send's
     * own ID. It is settled only when that conversation reports the core took
     * that ID (`ConversationView.submissions`), never by matching text (C32).
     */
    conversation?: string
    submission?: string
}

/** A Harness send the page dispatched, awaiting the core's word on it (C32): never shown, never sent again. */
export interface HarnessSent {
    submission: string
    conversation: string
    text: string
}

/**
 * A Runtime case handed to the Playground (C33): Administration opens Runtime
 * Security's Playground tab with the case's fields; the operator runs the
 * scan there. Bound to the tenant it was opened in.
 */
export interface PlaygroundCase {
    tenant: string
    caseId: string
    prefill: import('../features/library/library-view').CasePrefill
}

/** A delivery the desktop did not confirm: kept until the operator retries or dismisses it (DESIGN-V2 §7). */
export interface Unconfirmed {
    id: number
    what: string
    intent: Intent
    /** The context the intent was bound to, if any: once that context retires, the delivery cannot be revived. */
    context: string | null
}

export interface Toast {
    id: number
    kind: ToastKind
    text: string
    /** An optional undo the toast offers (label + action). */
    undo?: { label: string; run: () => void }
    /** The tenant the toast speaks for (an item, a read): retired on leaving it (I2). */
    tenant: string | null
    /** The context it was raised under, when the action was context-bound: retired with it. */
    context: string | null
}

/**
 * Transient shell presentation state (state-ownership.md "UI"): what is open,
 * the palette query, toasts and the modal stack. Nothing here survives a
 * reload and nothing here is authority: every durable fact is observed from
 * the Rust snapshot.
 */
interface ShellStore {
    /** The scope the shell shows now (set from each snapshot before it renders): what a notification must belong to. */
    scope: { tenant: string | null; context: string | null }
    setScope(tenant: string | null, context: string | null): void
    paletteOpen: boolean
    switcherOpen: boolean
    /** The rail's Administration children shown or hidden (C35; the prototype's `adminNav`). */
    adminNavOpen: boolean
    setAdminNav(open: boolean): void
    toasts: Toast[]
    /** Open modal ids, bottom to top; only the last is interactive (I8). */
    modals: string[]
    /** The Inbox tab and item per tenant (I2: a switch round trip keeps it). */
    inboxSelection: Record<string, { tab: 'all' | 'action' | 'updates' | 'snoozed'; item: string | null }>
    selectInbox(tenant: string, selection: { tab: 'all' | 'action' | 'updates' | 'snoozed'; item: string | null }): void
    /** Harness drafts, by `tenant|workspace|conversation` (C32): kept across navigation and tenant round trips, never shared between scopes, never sent by themselves. */
    harnessDrafts: Record<string, string>
    setHarnessDraft(scope: string, text: string): void
    /** Per `tenant|workspace`: the operator chose a new investigation while one runs (C32). */
    harnessFresh: Record<string, boolean>
    setHarnessFresh(scope: string, fresh: boolean): void
    /**
     * Per `tenant|workspace`: messages kept for the operator (C32). `unconfirmed`
     * ones may have been delivered (the desktop did not confirm it); the others
     * were refused or never sent. Never sent by themselves.
     */
    harnessUnsent: Record<string, HarnessKept[]>
    keepHarnessUnsent(scope: string, kept: Omit<HarnessKept, 'id'>): void
    dropHarnessUnsent(scope: string, id: number): void
    /**
     * Per `tenant|workspace`: sends dispatched and not yet reported by their
     * conversation (C32), so a send the core gives back keeps its text whatever
     * order its receipt and the core's word arrive in. Newest 64.
     */
    harnessSent: Record<string, HarnessSent[]>
    /** The case last handed to the Playground (C33); never runs anything by itself. */
    playground: PlaygroundCase | null
    openPlayground(target: Omit<PlaygroundCase, 'prefill'> & { prefill: Omit<PlaygroundCase['prefill'], 'nonce'> }): void
    trackHarnessSend(scope: string, sent: HarnessSent): void
    untrackHarnessSend(scope: string, submission: string): void
    /** The last contextual action's target, consumed by the destination it names. */
    arrival: Arrival | null
    arrive(target: Omit<Arrival, 'sequence'>): void
    /** Forgets an arrival that belongs to another tenant than `tenant`. */
    scopeArrival(tenant: string | null): void
    /** Drops unconfirmed deliveries bound to a context other than `context` (their authority retired). */
    retireUnconfirmed(context: string | null): void
    unconfirmed: Unconfirmed[]
    /** Toast lifetime in ms (6 s; tests shorten it). */
    toastMs: number
    addUnconfirmed(what: string, intent: Intent): number
    clearUnconfirmed(id: number): void
    openPalette(): void
    closePalette(): void
    setSwitcher(open: boolean): void
    toast(text: string, kind?: ToastKind, undo?: Toast['undo'], tenant?: string | null, context?: string | null): number
    /** Retires toasts that speak for another tenant than `tenant`. */
    retireToasts(tenant: string | null): void
    dismissToast(id: number): void
    pushModal(id: string): void
    popModal(id: string): void
}

let nextToast = 1
let nextKept = 1

export const useShellStore = create<ShellStore>((set, get) => ({
    scope: { tenant: null, context: null },
    setScope: (tenant, context) =>
        set((s) => ({
            scope: { tenant, context },
            // Leaving a scope retires what spoke for it, however late it would have arrived.
            toasts: s.toasts.filter((t) => (t.tenant === null || t.tenant === tenant) && (t.context === null || t.context === context)),
            unconfirmed: s.unconfirmed.filter((u) => u.context === null || u.context === context),
            arrival: s.arrival && s.arrival.tenant !== tenant ? null : s.arrival,
        })),
    paletteOpen: false,
    switcherOpen: false,
    adminNavOpen: true,
    setAdminNav: (open) => set({ adminNavOpen: open }),
    toasts: [],
    modals: [],
    inboxSelection: {},
    selectInbox: (tenant, selection) => set((s) => ({ inboxSelection: { ...s.inboxSelection, [tenant]: selection } })),
    harnessDrafts: {},
    setHarnessDraft: (scope, text) => set((s) => ({ harnessDrafts: { ...s.harnessDrafts, [scope]: text } })),
    harnessFresh: {},
    setHarnessFresh: (scope, fresh) => set((s) => ({ harnessFresh: { ...s.harnessFresh, [scope]: fresh } })),
    harnessUnsent: {},
    keepHarnessUnsent: (scope, item) =>
        set((s) => {
            const kept = s.harnessUnsent[scope] ?? []
            const record = { ...item, id: nextKept++ }
            // An uncertain attempt is never removed or downgraded by a later one:
            // only the core's word on its own send ID, or the operator, settles it.
            // A not-sent record of the same text is superseded by a later attempt;
            // not-sent records are kept once per text.
            const others = kept.filter((k) => k.unconfirmed || k.text !== item.text)
            return { harnessUnsent: { ...s.harnessUnsent, [scope]: [...others, record] } }
        }),
    dropHarnessUnsent: (scope, id) => set((s) => ({ harnessUnsent: { ...s.harnessUnsent, [scope]: (s.harnessUnsent[scope] ?? []).filter((k) => k.id !== id) } })),
    harnessSent: {},
    playground: null,
    openPlayground: (target) => set((s) => ({ playground: { ...target, prefill: { ...target.prefill, nonce: (s.playground?.prefill.nonce ?? 0) + 1 } } })),
    trackHarnessSend: (scope, sent) => set((s) => ({ harnessSent: { ...s.harnessSent, [scope]: [...(s.harnessSent[scope] ?? []), sent].slice(-64) } })),
    untrackHarnessSend: (scope, submission) => set((s) => ({ harnessSent: { ...s.harnessSent, [scope]: (s.harnessSent[scope] ?? []).filter((t) => t.submission !== submission) } })),
    arrival: null,
    arrive: (target) => set((s) => ({ arrival: { ...target, sequence: (s.arrival?.sequence ?? 0) + 1 } })),
    unconfirmed: [],
    toastMs: 6000,
    addUnconfirmed: (what, intent) => {
        const id = nextToast++
        const context = typeof (intent as { context?: unknown }).context === 'string' ? ((intent as { context: string }).context ?? null) : null
        // A delivery bound to a context the shell no longer shows is moot: never surfaced (I2).
        if (context !== null && context !== get().scope.context) return id
        set((s) => ({ unconfirmed: [...s.unconfirmed, { id, what, intent, context }] }))
        return id
    },
    scopeArrival: (tenant) => set((s) => (s.arrival && s.arrival.tenant !== tenant ? { arrival: null } : {})),
    retireUnconfirmed: (context) => set((s) => ({ unconfirmed: s.unconfirmed.filter((u) => u.context === null || u.context === context) })),
    clearUnconfirmed: (id) => set((s) => ({ unconfirmed: s.unconfirmed.filter((u) => u.id !== id) })),
    openPalette: () => set({ paletteOpen: true, switcherOpen: false }),
    closePalette: () => set({ paletteOpen: false }),
    setSwitcher: (open) => set({ switcherOpen: open, ...(open ? { paletteOpen: false } : {}) }),
    toast: (text, kind = 'ok', undo, tenant = null, context = null) => {
        const id = nextToast++
        // A toast for a tenant or context the shell no longer shows is never surfaced (I2).
        if (tenant !== null && tenant !== get().scope.tenant) return id
        if (context !== null && context !== get().scope.context) return id
        // One toast at a time: the newest replaces the rest (I10).
        set({ toasts: [{ id, kind, text, undo, tenant, context }] })
        return id
    },
    retireToasts: (tenant) => set((s) => ({ toasts: s.toasts.filter((t) => t.tenant === null || t.tenant === tenant) })),
    dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
    pushModal: (id) => set((s) => ({ modals: s.modals.includes(id) ? s.modals : [...s.modals, id] })),
    popModal: (id) => set((s) => ({ modals: s.modals.filter((m) => m !== id) })),
}))

/** Test seam: forget everything (each test starts from an empty store). */
export function resetShellStore() {
    useShellStore.setState({ scope: { tenant: null, context: null }, paletteOpen: false, switcherOpen: false, adminNavOpen: true, toasts: [], modals: [], unconfirmed: [], toastMs: 6000, inboxSelection: {}, arrival: null, harnessDrafts: {}, harnessFresh: {}, harnessUnsent: {}, harnessSent: {}, playground: null })
}
