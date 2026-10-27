import { create } from 'zustand'

export type ToastKind = 'ok' | 'info' | 'warn'
import type { Intent } from '@prisma-airs/desktop-contracts'

/** A delivery the desktop did not confirm: kept until the operator retries or dismisses it (DESIGN-V2 §7). */
export interface Unconfirmed {
    id: number
    what: string
    intent: Intent
}

export interface Toast {
    id: number
    kind: ToastKind
    text: string
    /** An optional undo the toast offers (label + action). */
    undo?: { label: string; run: () => void }
}

/**
 * Transient shell presentation state (state-ownership.md "UI"): what is open,
 * the palette query, toasts and the modal stack. Nothing here survives a
 * reload and nothing here is authority: every durable fact is observed from
 * the Rust snapshot.
 */
interface ShellStore {
    paletteOpen: boolean
    switcherOpen: boolean
    toasts: Toast[]
    /** Open modal ids, bottom to top; only the last is interactive (I8). */
    modals: string[]
    unconfirmed: Unconfirmed[]
    /** Toast lifetime in ms (6 s; tests shorten it). */
    toastMs: number
    addUnconfirmed(what: string, intent: Intent): number
    clearUnconfirmed(id: number): void
    openPalette(): void
    closePalette(): void
    setSwitcher(open: boolean): void
    toast(text: string, kind?: ToastKind, undo?: Toast['undo']): number
    dismissToast(id: number): void
    pushModal(id: string): void
    popModal(id: string): void
}

let nextToast = 1

export const useShellStore = create<ShellStore>((set) => ({
    paletteOpen: false,
    switcherOpen: false,
    toasts: [],
    modals: [],
    unconfirmed: [],
    toastMs: 6000,
    addUnconfirmed: (what, intent) => {
        const id = nextToast++
        set((s) => ({ unconfirmed: [...s.unconfirmed, { id, what, intent }] }))
        return id
    },
    clearUnconfirmed: (id) => set((s) => ({ unconfirmed: s.unconfirmed.filter((u) => u.id !== id) })),
    openPalette: () => set({ paletteOpen: true, switcherOpen: false }),
    closePalette: () => set({ paletteOpen: false }),
    setSwitcher: (open) => set({ switcherOpen: open, ...(open ? { paletteOpen: false } : {}) }),
    toast: (text, kind = 'ok', undo) => {
        const id = nextToast++
        // One toast at a time: the newest replaces the rest (I10).
        set({ toasts: [{ id, kind, text, undo }] })
        return id
    },
    dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
    pushModal: (id) => set((s) => ({ modals: s.modals.includes(id) ? s.modals : [...s.modals, id] })),
    popModal: (id) => set((s) => ({ modals: s.modals.filter((m) => m !== id) })),
}))

/** Test seam: forget everything (each test starts from an empty store). */
export function resetShellStore() {
    useShellStore.setState({ paletteOpen: false, switcherOpen: false, toasts: [], modals: [], unconfirmed: [], toastMs: 6000 })
}
