import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import type { FeatureEntry, FeatureId, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { SHELL_COPY } from './copy'
import { Modal } from './modal'

export type PaletteEntry =
    | { kind: 'navigate'; id: string; label: string; feature: FeatureEntry }
    | { kind: 'switch'; id: string; label: string; tenant: string }
    | { kind: 'readNow'; id: string; label: string }

export interface PaletteProps {
    snapshot: ShellSnapshot
    onNavigate: (id: FeatureId) => void
    onSwitchTenant: (tenant: string) => void
    onReadNow: () => void
    onClose: () => void
}

/** The palette's entries come from the Rust inventory and the tenant list: never a TS-only registry. */
export function paletteEntries(snapshot: ShellSnapshot): PaletteEntry[] {
    const destinations: PaletteEntry[] = snapshot.features.map((feature) => ({
        kind: 'navigate',
        id: `go:${feature.id}`,
        label: feature.label,
        feature,
    }))
    // Tenants are identified as the switcher identifies them: label, environment and TSG (labels may repeat).
    const tenants: PaletteEntry[] = snapshot.tenants
        .filter((t) => t.id !== snapshot.active?.tenant)
        .map((t) => ({
            kind: 'switch',
            id: `tenant:${t.id}`,
            label: SHELL_COPY.paletteSwitch(`${t.label} · ${t.destination.kind === 'region' ? t.destination.region : 'custom'}${t.tsgId ? ` · TSG ${t.tsgId}` : ''}`),
            tenant: t.id,
        }))
    const actions: PaletteEntry[] = snapshot.active ? [{ kind: 'readNow', id: 'action:read', label: SHELL_COPY.paletteReadNow }] : []
    return [...destinations, ...tenants, ...actions]
}

export function Palette(props: PaletteProps) {
    const [query, setQuery] = useState('')
    const [index, setIndex] = useState(0)
    const input = useRef<HTMLInputElement>(null)
    const listId = useId()
    const entries = useMemo(() => paletteEntries(props.snapshot), [props.snapshot])
    const matches = useMemo(() => {
        const q = query.trim().toLowerCase()
        return q ? entries.filter((e) => e.label.toLowerCase().includes(q)) : entries
    }, [entries, query])
    useEffect(() => input.current?.focus(), [])
    const activeId = matches[index] ? `${listId}-${matches[index].id}` : undefined
    useEffect(() => {
        if (activeId) document.getElementById(activeId)?.scrollIntoView({ block: 'nearest' })
    }, [activeId])

    const choose = (entry: PaletteEntry | undefined) => {
        if (!entry) return
        if (entry.kind === 'navigate' && entry.feature.availability.state !== 'available') return
        props.onClose()
        if (entry.kind === 'navigate') props.onNavigate(entry.feature.id)
        else if (entry.kind === 'switch') props.onSwitchTenant(entry.tenant)
        else props.onReadNow()
    }
    const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
        if (event.key === 'ArrowDown') {
            event.preventDefault()
            setIndex((i) => Math.min(i + 1, Math.max(matches.length - 1, 0)))
        } else if (event.key === 'ArrowUp') {
            event.preventDefault()
            setIndex((i) => Math.max(i - 1, 0))
        } else if (event.key === 'End') {
            event.preventDefault()
            setIndex(Math.max(matches.length - 1, 0))
        } else if (event.key === 'Home') {
            event.preventDefault()
            setIndex(0)
        } else if (event.key === 'Enter') {
            event.preventDefault()
            choose(matches[index])
        } else if (event.key === 'Tab') {
            event.preventDefault()
            input.current?.focus()
        }
    }
    return (
        <Modal label={SHELL_COPY.paletteLabel} onClose={props.onClose} data-slot='command-palette'>
            <input
                ref={input}
                role='combobox'
                aria-expanded='true'
                aria-controls={listId}
                aria-activedescendant={activeId}
                aria-autocomplete='list'
                aria-label={SHELL_COPY.paletteInput}
                placeholder={SHELL_COPY.palette}
                value={query}
                onKeyDown={onKeyDown}
                onChange={(e) => {
                    setQuery(e.target.value)
                    setIndex(0)
                }}
                className='w-full px-3 text-[15px]'
            />
            <ul id={listId} role='listbox' aria-label={SHELL_COPY.paletteLabel} className='mt-2 flex max-h-[50vh] flex-col overflow-auto'>
                {matches.map((entry, i) => {
                    const unavailable = entry.kind === 'navigate' && entry.feature.availability.state === 'unavailable'
                    return (
                        <li
                            key={entry.id}
                            id={`${listId}-${entry.id}`}
                            role='option'
                            aria-selected={i === index}
                            aria-current={i === index ? 'true' : undefined}
                            aria-disabled={unavailable || undefined}
                            onMouseEnter={() => setIndex(i)}
                            onClick={() => choose(entry)}
                            className='flex min-h-10 cursor-pointer items-center justify-between rounded-[7px] px-3 text-[14px]'
                            style={{ background: i === index ? 'var(--v2-inset)' : 'transparent', color: unavailable ? 'var(--v2-faint)' : 'var(--v2-ink)' }}
                        >
                            <span>{entry.label}</span>
                            {entry.kind === 'navigate' && entry.feature.shortcut !== null && (
                                <kbd className='text-[11px]' style={{ color: 'var(--v2-faint)' }}>
                                    ⌘{entry.feature.shortcut}
                                </kbd>
                            )}
                            {unavailable && entry.feature.availability.state === 'unavailable' && (
                                <span className='text-[11px] uppercase' style={{ color: 'var(--v2-faint)' }}>
                                    {entry.feature.availability.plannedChunk}
                                </span>
                            )}
                        </li>
                    )
                })}
                {matches.length === 0 && (
                    <li className='px-3 py-2 text-[13px]' style={{ color: 'var(--v2-faint)' }}>
                        {SHELL_COPY.paletteEmpty}
                    </li>
                )}
            </ul>
        </Modal>
    )
}
