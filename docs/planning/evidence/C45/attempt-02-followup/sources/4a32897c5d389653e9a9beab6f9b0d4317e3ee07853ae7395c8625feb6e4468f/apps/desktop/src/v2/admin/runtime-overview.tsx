import type { ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { clockOf } from '../copy'
import { Btn } from '../controls'
import { ADMIN_COPY as C } from './copy'

/**
 * Runtime Security's Overview as R2App draws it (C39): three KPI tiles and
 * the 24-bar activity sparkline, every figure from the tenant's read
 * (never invented), and the read's time; "Read now" is the same readTenant.
 */
export function RuntimeOverview({ snapshot, onRead }: { snapshot: ShellSnapshot; onRead: () => void }) {
    const read = snapshot.overview.read
    const n = (v: number | null | undefined) => (v === null || v === undefined ? '—' : v.toLocaleString())
    const buckets = read?.chart?.buckets ?? []
    const max = Math.max(1, ...buckets.map((b) => b.violating))
    const bars = buckets.slice(-24).map((b) => ({ h: Math.max(2, Math.round((b.violating / max) * 76)), hot: b.violating > 0, time: b.time }))
    const chartMissing = !read || read.chart === null || read.chartFailure !== null
    const events = read?.summary.detectionEvents ?? null
    const keys = !read ? C.overviewRuntime.notRead : read.keysFailure ? C.overviewRuntime.keysUnavailable : read.keysComplete ? C.overviewRuntime.keysComplete : C.overviewRuntime.keysPartial
    const readLine = read ? C.overviewRuntime.fromRead(clockOf(read.readAt)) : C.overviewRuntime.notRead
    const tiles: { label: string; value: string; sub: string; subColor: string; valueColor?: string }[] = [
        { label: C.overviewRuntime.events, value: n(events), sub: chartMissing ? C.overviewRuntime.chartNotRead : readLine, subColor: chartMissing ? 'var(--v2-flag)' : 'var(--v2-soft)', valueColor: events !== null && events > 0 ? 'var(--v2-block)' : undefined },
        { label: C.overviewRuntime.sessions, value: n(read?.summary.sessions), sub: readLine, subColor: 'var(--v2-soft)' },
        { label: C.overviewRuntime.violating, value: n(read?.summary.violating), sub: `${C.overviewRuntime.keys}: ${keys}`, subColor: read?.keysFailure ? 'var(--v2-flag)' : 'var(--v2-soft)' },
    ]
    return (
        <div className='flex flex-col' style={{ gap: 16 }} data-slot='admin-runtime-overview'>
            <div className='grid' style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 150px), 1fr))', gap: 16 }}>
                {tiles.map((t) => (
                    <div key={t.label} className='rounded-[12px]' style={{ padding: '16px 18px', background: 'var(--v2-inset)' }} data-slot='admin-kpi'>
                        <div className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                            {t.label}
                        </div>
                        <div className='text-[22px] font-bold tabular-nums' style={{ color: t.valueColor ?? 'var(--v2-ink)' }}>
                            {t.value}
                        </div>
                        <div className='text-[12px] font-semibold' style={{ color: t.subColor }}>
                            {t.sub}
                        </div>
                    </div>
                ))}
                <div
                    role='img'
                    aria-label={chartMissing ? C.overviewRuntime.chartNotRead : C.overviewRuntime.chart}
                    className='flex items-end'
                    style={{ gap: 3, height: 82, padding: '0 4px', borderBottom: '1px solid var(--v2-rule)', gridColumn: 'span 1', minWidth: 150 }}
                    data-slot='admin-sparkline'
                    data-bars={bars.length}
                >
                    {bars.map((b, i) => (
                        <span key={`${b.time}-${i}`} style={{ flex: 1, height: b.h, borderRadius: '2px 2px 0 0', background: b.hot ? 'var(--v2-acc)' : 'var(--v2-rule)' }} />
                    ))}
                </div>
            </div>
            <div>
                <Btn onClick={onRead}>{C.overviewRuntime.readNow}</Btn>
            </div>
        </div>
    )
}
