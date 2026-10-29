import { Fragment, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import type { FeatureEntry, FeatureId, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { ENVIRONMENT_LABELS, SHELL_COPY } from './copy'
import { FEATURE_GLYPHS, Glyph, type GlyphName } from './icons'
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
            label: SHELL_COPY.paletteSwitch(`${t.label} · ${ENVIRONMENT_LABELS[t.environment]}${t.tsgId ? ` · TSG ${t.tsgId}` : ''}`),
            tenant: t.id,
        }))
    const actions: PaletteEntry[] = snapshot.active ? [{ kind: 'readNow', id: 'action:read', label: SHELL_COPY.paletteReadNow }] : []
    return [...destinations, ...tenants, ...actions]
}

const glyphOf = (entry: PaletteEntry): GlyphName => (entry.kind === 'navigate' ? (FEATURE_GLYPHS[entry.feature.id] ?? 'chevR') : entry.kind === 'switch' ? 'idcard' : 'play')
const groupOf = (entry: PaletteEntry) => (entry.kind === 'navigate' ? SHELL_COPY.paletteGroups.destinations : entry.kind === 'switch' ? SHELL_COPY.paletteGroups.tenants : SHELL_COPY.paletteGroups.actions)

/**
 * The command palette (R2App, C35): a 640px dialog with a 50px search row,
 * grouped rows (glyph, label, hint) and a footer of key hints. Entries are
 * the core's destinations, the other tenants and the read action.
 */
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
    let lastGroup = ''
    return (
        <Modal label={SHELL_COPY.paletteLabel} onClose={props.onClose} width={640} padding={0} top={90} radius={14} data-slot='command-palette'>
            <div className='flex h-[50px] items-center gap-2.5 px-4' style={{ borderBottom: '1px solid var(--v2-rule)' }}>
                <Glyph name='search' size={16} color='var(--v2-faint)' />
                <input
                    ref={input}
                    role='combobox'
                    aria-expanded='true'
                    aria-controls={listId}
                    aria-activedescendant={activeId}
                    aria-autocomplete='list'
                    aria-label={SHELL_COPY.paletteInput}
                    placeholder={SHELL_COPY.searchOrGo}
                    value={query}
                    onKeyDown={onKeyDown}
                    onChange={(e) => {
                        setQuery(e.target.value)
                        setIndex(0)
                    }}
                    className='min-h-0 flex-grow border-0 bg-transparent px-0 text-[15px] outline-none'
                    style={{ border: 0, background: 'transparent' }}
                />
                <kbd className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                    Esc
                </kbd>
            </div>
            <ul id={listId} role='listbox' aria-label={SHELL_COPY.paletteLabel} className='flex flex-col overflow-auto' style={{ padding: '6px 8px 8px', maxHeight: 'min(620px, 60vh)' }}>
                {matches.map((entry, i) => {
                    const unavailable = entry.kind === 'navigate' && entry.feature.availability.state === 'unavailable'
                    const group = groupOf(entry)
                    const heading = group !== lastGroup
                    lastGroup = group
                    return (
                        <Fragment key={entry.id}>
                            {heading && (
                                <li role='presentation' className='text-[12px] font-semibold' style={{ padding: '8px 8px 4px', color: 'var(--v2-faint)' }}>
                                    {group}
                                </li>
                            )}
                            <li
                                id={`${listId}-${entry.id}`}
                                role='option'
                                aria-selected={i === index}
                                aria-current={i === index ? 'true' : undefined}
                                aria-disabled={unavailable || undefined}
                                onMouseEnter={() => setIndex(i)}
                                onClick={() => choose(entry)}
                                className='flex h-[34px] cursor-pointer items-center gap-2.5 rounded-[7px] px-2.5 text-[13px]'
                                style={{ background: i === index ? 'var(--v2-inset)' : 'transparent', color: unavailable ? 'var(--v2-faint)' : 'var(--v2-ink)' }}
                            >
                                <Glyph name={glyphOf(entry)} size={14} color={unavailable ? 'var(--v2-faint)' : 'var(--v2-soft)'} />
                                <span className='font-semibold'>{entry.label}</span>
                                <span className='flex-grow' />
                                {entry.kind === 'navigate' && entry.feature.shortcut !== null && (
                                    <kbd className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                        ⌘{entry.feature.shortcut}
                                    </kbd>
                                )}
                                {unavailable && entry.feature.availability.state === 'unavailable' && (
                                    <span className='text-[11px] uppercase' style={{ color: 'var(--v2-faint)' }}>
                                        {entry.feature.availability.plannedChunk}
                                    </span>
                                )}
                            </li>
                        </Fragment>
                    )
                })}
                {matches.length === 0 && (
                    <li className='flex items-center gap-2.5' style={{ padding: '16px 10px', color: 'var(--v2-faint)' }}>
                        <span className='flex-grow'>{SHELL_COPY.paletteEmpty}</span>
                        <button
                            type='button'
                            onClick={() => {
                                props.onClose()
                                props.onNavigate('harness')
                            }}
                            className='h-8 rounded-[7px] border px-3 font-semibold'
                            style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}
                        >
                            {SHELL_COPY.paletteAsk}
                        </button>
                    </li>
                )}
            </ul>
            <div className='flex gap-4 px-4 py-2 text-[12px]' style={{ borderTop: '1px solid var(--v2-rule)', background: 'var(--v2-side)', color: 'var(--v2-faint)' }}>
                <span>{SHELL_COPY.paletteHints.move}</span>
                <span>{SHELL_COPY.paletteHints.open}</span>
            </div>
        </Modal>
    )
}
