import type { ReactNode } from 'react'
import type { AdminProduct, AdminTab } from '@prisma-airs/desktop-contracts'
import { Glyph, PRODUCT_GLYPHS, type GlyphName } from '../icons'
import { ADMIN_COPY as C } from './copy'

export type PillTone = 'ok' | 'flag' | 'faint'

interface Props {
    product: AdminProduct
    open: boolean
    onToggle: () => void
    /** The headline figure the card shows at its right, from the core's read ("—" when not read). */
    stat: { value: string; label: string; hot?: boolean }
    pill: { word: string; tone: PillTone; glyph: GlyphName }
    /** Entries needing action for this product (the Inbox's, minus holds). */
    alerts: number
    tabs: AdminTab[]
    current: AdminTab | undefined
    onTab: (id: string) => void
    /** The product section's own note above its panel (read-only products). */
    note?: string | null
    children?: ReactNode
}

const PILL: Record<PillTone, { fg: string; bg: string }> = {
    ok: { fg: 'var(--v2-ok)', bg: 'var(--v2-ok-bg)' },
    flag: { fg: 'var(--v2-flag)', bg: 'var(--v2-flag-bg)' },
    faint: { fg: 'var(--v2-soft)', bg: 'var(--v2-inset)' },
}

/**
 * One product section of Administration as R2App draws it (C39): the header
 * button (chevron, icon tile, name, description, headline figure, status
 * pill, alerts), then the tab strip and the panel. The header's accessible
 * name is its whole text, as the C31 suites address it.
 */
export function ProductCard({ product, open, onToggle, stat, pill, alerts, tabs, current, onTab, note = null, children }: Props) {
    const unavailable = product.availability.state === 'unavailable'
    const expanded = open && !unavailable
    const panel = `admin-${product.id}`
    const look = PILL[pill.tone]
    const isNew = product.id === 'agenticIdentity' || product.id === 'agenticEndpoint'
    return (
        <li
            data-slot='admin-product'
            data-product={product.id}
            className='min-w-0 overflow-hidden rounded-[12px] border'
            style={{ borderColor: expanded ? 'var(--v2-acc)' : 'var(--v2-rule)', background: 'var(--v2-paper)' }}
        >
            <h2 className='m-0'>
                <button
                    type='button'
                    aria-expanded={unavailable ? undefined : expanded}
                    aria-controls={unavailable ? undefined : panel}
                    aria-disabled={unavailable || undefined}
                    onClick={() => !unavailable && onToggle()}
                    className='flex w-full flex-wrap items-center text-left text-[13px] font-normal'
                    style={{ gap: 14, padding: '18px 22px', color: 'var(--v2-ink)' }}
                >
                    <Glyph name={expanded ? 'chev' : 'chevR'} size={12} color='var(--v2-faint)' className='shrink-0' />
                    <span
                        className='grid shrink-0 place-items-center rounded-[9px]'
                        style={{ width: 32, height: 32, background: expanded ? 'var(--v2-acc-soft)' : 'var(--v2-inset)', color: expanded ? 'var(--v2-acc)' : 'var(--v2-soft)' }}
                    >
                        <Glyph name={PRODUCT_GLYPHS[product.id] ?? 'sliders'} size={16} />
                    </span>
                    <span className='min-w-0' style={{ flex: '1 1 200px' }}>
                        <span className='flex items-center' style={{ gap: 7 }}>
                            <b className='text-[14px]'>{product.label}</b>
                            {product.readOnly && !unavailable ? (
                                <span className='text-[12px] font-normal' style={{ color: 'var(--v2-faint)' }}>
                                    Read-only
                                </span>
                            ) : null}
                            {isNew ? (
                                <span className='rounded-full px-1.5 text-[12px] font-bold' style={{ background: 'var(--v2-acc-soft)', color: 'var(--v2-acc)', padding: '1px 6px' }}>
                                    New
                                </span>
                            ) : null}
                        </span>
                        <span className='block truncate text-[12px]' style={{ color: 'var(--v2-faint)' }} data-slot='admin-product-description'>
                            {C.description[product.id] ?? ''}
                        </span>
                    </span>
                    {/* The figure, pill and alerts wrap together under the name when the card is narrow (200% zoom). */}
                    <span className='ml-auto flex shrink-0 items-center' style={{ gap: 14 }}>
                    <span className='shrink-0 text-right' data-slot='admin-product-stat'>
                        <b className='text-[17px] tabular-nums' style={{ color: stat.hot ? 'var(--v2-block)' : 'var(--v2-ink)' }}>
                            {stat.value}
                        </b>
                        <br />
                        <span className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                            {stat.label}
                        </span>
                    </span>
                    <span className='flex shrink-0 justify-end' style={{ width: 140 }}>
                        <span className='flex items-center rounded-full text-[12px] font-semibold' style={{ gap: 4, padding: '2px 8px', background: look.bg, color: look.fg }} data-slot='admin-product-pill'>
                            <Glyph name={pill.glyph} size={10} />
                            {pill.word}
                        </span>
                    </span>
                    <span className='shrink-0 text-right text-[12px]' style={{ width: 64, color: 'var(--v2-faint)' }} data-slot='admin-product-alerts'>
                        {alerts > 0 ? C.alerts(alerts) : ''}
                    </span>
                    </span>
                </button>
            </h2>
            {unavailable && product.availability.state === 'unavailable' ? (
                <p data-slot='admin-product-gate' className='text-[13px]' style={{ padding: '0 22px 18px 72px', color: 'var(--v2-soft)' }}>
                    {product.availability.reason}
                </p>
            ) : null}
            {expanded && current ? (
                <div style={{ borderTop: '1px solid var(--v2-rule)' }}>
                    <div
                        role='tablist'
                        aria-label={C.tabs(product.label)}
                        className='flex flex-wrap'
                        style={{ gap: 2, padding: '0 18px 0 72px', borderBottom: '1px solid var(--v2-rule)', background: 'var(--v2-side)' }}
                        data-slot='admin-tabs'
                    >
                        {tabs.map((t) => {
                            const on = t.id === current.id
                            return (
                                <button
                                    key={t.id}
                                    type='button'
                                    role='tab'
                                    aria-selected={on}
                                    aria-controls={`${panel}-tab`}
                                    onClick={() => onTab(t.id)}
                                    className='h-10 min-h-0 shrink-0 border-0 px-[11px] text-[12.5px] font-semibold whitespace-nowrap'
                                    style={{
                                        borderBottom: `2px solid ${on ? 'var(--v2-acc)' : 'transparent'}`,
                                        background: 'transparent',
                                        color: on ? 'var(--v2-ink)' : t.disposition.kind === 'unavailable' ? 'var(--v2-faint)' : 'var(--v2-soft)',
                                    }}
                                >
                                    {t.label}
                                </button>
                            )
                        })}
                    </div>
                    <div
                        role='tabpanel'
                        id={`${panel}-tab`}
                        aria-label={current.label}
                        data-slot='admin-tab'
                        data-tab={current.id}
                        data-disposition={current.disposition.kind}
                        className='flex min-w-0 flex-col'
                        style={{ gap: 18, padding: '22px 26px 26px 72px' }}
                    >
                        {note ? (
                            <div className='flex rounded-[8px] text-[12px]' style={{ gap: 8, padding: '10px 12px', background: 'var(--v2-inset)', color: 'var(--v2-soft)', maxWidth: 860 }} data-slot='admin-note'>
                                <Glyph name='eye' size={14} className='shrink-0' />
                                {note}
                            </div>
                        ) : null}
                        {children}
                    </div>
                </div>
            ) : null}
        </li>
    )
}
