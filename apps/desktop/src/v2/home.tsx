import { forwardRef } from 'react'
import type { FeatureId, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { FEATURE_ICONS } from '../shell/icons'
import { HOME_COPY } from './copy'
import { Verdict } from './verdict'

export interface HomeProps {
    snapshot: ShellSnapshot
    onNavigate: (id: FeatureId) => void
}

function clock(seconds: number): string {
    const d = new Date(seconds * 1000)
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

/**
 * Home (C28 skeleton; attention items and product read cards land in C30):
 * the destination title row, the tenant line, the one read-status line
 * derived from the observed overview, the tenant's capabilities, and the
 * destinations. Nothing here is invented: every line reads the snapshot.
 */
export const Home = forwardRef<HTMLHeadingElement, HomeProps>(function Home({ snapshot, onNavigate }, ref) {
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) ?? null
    const overview = snapshot.overview
    const readLine = !tenant
        ? HOME_COPY.noTenant
        : overview.refreshing && !overview.read
          ? HOME_COPY.reading
          : overview.face === 'firstLaunch' || !overview.read
            ? HOME_COPY.firstRead
            : overview.failure && overview.failedAt
              ? HOME_COPY.readFailed(clock(overview.failedAt))
              : HOME_COPY.readAt(clock(overview.read.readAt))
    const readTone = !tenant || overview.face === 'firstLaunch' || !overview.read ? 'info' : overview.failure ? 'flagged' : 'allowed'
    const capabilities = tenant
        ? [
              { label: HOME_COPY.scan, state: tenant.scan.state },
              { label: HOME_COPY.management, state: tenant.management.state },
              { label: HOME_COPY.inference, state: tenant.inference.state },
          ]
        : []
    return (
        <section aria-labelledby='home-heading' data-slot='home' style={{ maxWidth: 'var(--v2-reading)' }}>
            <div className='flex flex-col gap-1'>
                <h1 ref={ref} id='home-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)' }}>
                    {HOME_COPY.title}
                </h1>
                {tenant && (
                    <p className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                        {tenant.label}
                        {tenant.tsgId ? ` · TSG ${tenant.tsgId}` : ''}
                    </p>
                )}
                <p role='status' aria-live='polite' data-slot='read-status' className='mt-1 text-[13px]'>
                    <Verdict tone={readTone} word={readLine} />
                </p>
            </div>
            <div className='mt-6 rounded-[12px] border p-5' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                <h2 className='text-[15px] font-semibold'>{HOME_COPY.attention}</h2>
                <p className='mt-2 text-[13px]' style={{ color: 'var(--v2-faint)' }}>
                    {overview.attention.length === 0 ? HOME_COPY.allClear : HOME_COPY.inboxSoon}
                </p>
            </div>
            {capabilities.length > 0 && (
                <div className='mt-6'>
                    <h2 className='text-[15px] font-semibold'>{HOME_COPY.capabilities}</h2>
                    <ul className='mt-3 grid gap-3' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))' }}>
                        {capabilities.map((c) => (
                            <li key={c.label} className='rounded-[12px] border p-4' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                                <p className='text-[13px] font-semibold'>{c.label}</p>
                                <p className='mt-2'>
                                    <Verdict
                                        tone={c.state === 'ready' ? 'allowed' : c.state === 'failed' ? 'blocked' : 'info'}
                                        word={HOME_COPY.state[c.state] ?? c.state}
                                    />
                                </p>
                            </li>
                        ))}
                    </ul>
                </div>
            )}
            <div className='mt-6'>
                <h2 className='text-[15px] font-semibold'>{HOME_COPY.destinations}</h2>
                <ul className='mt-3 grid gap-3' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))' }}>
                    {snapshot.features
                        .filter((f) => f.sidebar && f.id !== 'home')
                        .map((f) => {
                            const Icon = FEATURE_ICONS[f.id]
                            const unavailable = f.availability.state === 'unavailable'
                            return (
                                <li key={f.id}>
                                    <button
                                        type='button'
                                        disabled={unavailable}
                                        onClick={() => onNavigate(f.id)}
                                        className='flex min-h-11 w-full items-center gap-3 rounded-[12px] border px-4 text-left text-[14px] font-medium disabled:opacity-60'
                                        style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}
                                    >
                                        <Icon aria-hidden='true' size={16} />
                                        <span className='flex-grow'>{f.label}</span>
                                        {unavailable && f.availability.state === 'unavailable' && (
                                            <span className='text-[11px] uppercase' style={{ color: 'var(--v2-faint)' }}>
                                                {f.availability.plannedChunk}
                                            </span>
                                        )}
                                    </button>
                                </li>
                            )
                        })}
                </ul>
            </div>
        </section>
    )
})
