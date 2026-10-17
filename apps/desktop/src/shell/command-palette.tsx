import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import type { FeatureEntry, FeatureId } from '@prisma-airs/desktop-contracts'

interface Props {
    open: boolean
    features: FeatureEntry[]
    onSelect: (id: FeatureId) => void
    onClose: () => void
}

/**
 * Modal destination switcher. Focus moves into the field on open, stays in
 * the dialog, and returns to the previously focused element on close.
 */
export function CommandPalette({ open, features, onSelect, onClose }: Props) {
    const [query, setQuery] = useState('')
    const [index, setIndex] = useState(0)
    const input = useRef<HTMLInputElement>(null)
    const restore = useRef<HTMLElement | null>(null)
    const listId = useId()

    const matches = useMemo(() => {
        const q = query.trim().toLowerCase()
        return q ? features.filter((f) => f.label.toLowerCase().includes(q)) : features
    }, [features, query])

    useEffect(() => {
        if (!open) return
        restore.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
        setQuery('')
        setIndex(0)
        input.current?.focus()
        return () => {
            restore.current?.focus()
        }
    }, [open])

    if (!open) return null

    const choose = (feature: FeatureEntry | undefined) => {
        if (!feature) return
        onClose()
        onSelect(feature.id)
    }

    const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        if (event.key === 'Escape') {
            event.preventDefault()
            onClose()
        } else if (event.key === 'ArrowDown') {
            event.preventDefault()
            setIndex((i) => Math.min(i + 1, Math.max(matches.length - 1, 0)))
        } else if (event.key === 'ArrowUp') {
            event.preventDefault()
            setIndex((i) => Math.max(i - 1, 0))
        } else if (event.key === 'Enter') {
            event.preventDefault()
            choose(matches[index])
        } else if (event.key === 'Tab') {
            // The field is the only focus stop; keep focus inside the dialog.
            event.preventDefault()
            input.current?.focus()
        }
    }

    const activeId = matches[index] ? `${listId}-${matches[index].id}` : undefined
    return (
        <div className='fixed inset-0 z-50 grid place-items-start justify-center bg-black/40 pt-[12vh]' onMouseDown={onClose}>
            <div
                role='dialog'
                aria-modal='true'
                aria-label='Command palette'
                data-slot='command-palette'
                onKeyDown={onKeyDown}
                onMouseDown={(e) => e.stopPropagation()}
                className='bg-academy-paper border-academy-rule w-[min(560px,90vw)] rounded-[14px] border p-3 shadow-2xl'
            >
                <input
                    ref={input}
                    role='combobox'
                    aria-expanded='true'
                    aria-controls={listId}
                    aria-activedescendant={activeId}
                    aria-autocomplete='list'
                    aria-label='Go to destination'
                    placeholder='Go to…'
                    value={query}
                    onChange={(e) => {
                        setQuery(e.target.value)
                        setIndex(0)
                    }}
                    className='bg-academy-bg-inset text-academy-ink placeholder:text-academy-ink-faint w-full rounded-[10px] px-3 py-2.5 text-[15px] outline-none'
                />
                <ul id={listId} role='listbox' aria-label='Destinations' className='mt-2 flex max-h-[50vh] flex-col overflow-auto'>
                    {matches.map((feature, i) => (
                        <li
                            key={feature.id}
                            id={`${listId}-${feature.id}`}
                            role='option'
                            aria-selected={i === index}
                            onMouseEnter={() => setIndex(i)}
                            onClick={() => choose(feature)}
                            className={
                                'flex min-h-11 cursor-pointer items-center justify-between rounded-[8px] px-3 text-[14px] ' +
                                (i === index ? 'bg-academy-veil-4 text-academy-ink' : 'text-academy-ink-soft')
                            }
                        >
                            <span>{feature.label}</span>
                            {feature.availability.state === 'unavailable' && (
                                <span className='text-academy-ink-faint font-mono text-[10px] uppercase'>Not available</span>
                            )}
                        </li>
                    ))}
                    {matches.length === 0 && <li className='text-academy-ink-faint px-3 py-2 text-[13px]'>No destinations match.</li>}
                </ul>
            </div>
        </div>
    )
}
