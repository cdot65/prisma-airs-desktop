import { create } from 'zustand'

export type ToastKind = 'ok' | 'info' | 'warn'
import type { FeatureId, Intent, ManagementResource } from '@prisma-airs/desktop-contracts'

/** What a contextual action asked the destination to select on arrival (C30), owned by the tenant it was followed in (I2). */
export interface Arrival {
    tenant: string
    feature: FeatureId
    resource: ManagementResource | null
    /** The item to filter to, by name. */
    item: string | null
    /** Counts arrivals: a destination already open applies a new one. */
    sequence: number
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
    toasts: Toast[]
    /** Open modal ids, bottom to top; only the last is interactive (I8). */
    modals: string[]
    /** The Inbox tab and item per tenant (I2: a switch round trip keeps it). */
    inboxSelection: Record<string, { tab: 'all' | 'action' | 'updates' | 'snoozed'; item: string | null }>
    selectInbox(tenant: string, selection: { tab: 'all' | 'action' | 'updates' | 'snoozed'; item: string | null }): void
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
    toasts: [],
    modals: [],
    inboxSelection: {},
    selectInbox: (tenant, selection) => set((s) => ({ inboxSelection: { ...s.inboxSelection, [tenant]: selection } })),
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
    useShellStore.setState({ scope: { tenant: null, context: null }, paletteOpen: false, switcherOpen: false, toasts: [], modals: [], unconfirmed: [], toastMs: 6000, inboxSelection: {}, arrival: null })
}
