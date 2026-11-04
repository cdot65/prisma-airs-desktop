import type { SetupWarning, VerifyState } from '@prisma-airs/desktop-contracts'
import { READ_COPY } from './copy'

const CHECK_STATE: Record<VerifyState, string> = { pending: 'Checking', pass: 'Passed', warning: 'Warning', fail: 'Failed', stored: 'Stored', skipped: 'Skipped', unavailable: 'Unavailable' }

function RecordedTime({ seconds }: { seconds: number }) {
    const date = new Date(seconds * 1000)
    return Number.isFinite(date.valueOf()) ? <time dateTime={date.toISOString()}>{date.toLocaleString()}</time> : <span>Time unavailable</span>
}

/** Native outcomes only: this presentation never infers resolution from a product read. */
export function SetupWarningResult({ warning, live = false }: { warning: SetupWarning; live?: boolean }) {
    return <div className='flex flex-col text-[13px]' style={{ gap: 5, color: 'var(--v2-soft)' }} role={live ? 'status' : undefined} aria-live={live ? 'polite' : undefined} data-slot='setup-warning-result'>
        <p>Originally recorded <RecordedTime seconds={warning.recordedSecs} />.</p>
        {warning.recheck ? <>
            <p>Latest check: {CHECK_STATE[warning.recheck.state]} · <RecordedTime seconds={warning.recheck.checkedSecs} />.</p>
            <p>{warning.recheck.reason}</p>
            {warning.recheck.state !== 'pending' && (warning.recheck.basis === 'unknown' || !warning.recheck.equivalent) ? <p>The original scope could not be matched. This result does not resolve the original warning.</p> : null}
        </> : <p>No later setup recheck is recorded.</p>}
        {warning.resolvedSecs !== undefined
            ? <p data-state='resolved'>Resolved <RecordedTime seconds={warning.resolvedSecs} />.</p>
            : <p data-state='active'>Warning active.</p>}
    </div>
}

/** Tenant configuration history is separate from the redacted diagnostic event log/export. */
export function SetupWarningHistory({ warnings }: { warnings: SetupWarning[] }) {
    if (warnings.length === 0) return null
    return <section aria-label='Setup check history' className='flex flex-col' style={{ gap: 10, borderTop: '1px solid var(--v2-rule)', paddingTop: 12 }}>
        <h3 className='text-[14px] font-semibold' style={{ color: 'var(--v2-ink)' }}>Setup check history</h3>
        <p className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>These tenant setup records are separate from the redacted diagnostic events above. Exporting or clearing diagnostic events does not export or clear this history.</p>
        <ol className='flex flex-col' style={{ gap: 14 }}>
            {[...warnings].sort((a, b) => b.recordedSecs - a.recordedSecs).map((warning) => <li key={warning.id} data-warning={warning.id} className='flex flex-col' style={{ gap: 5 }}>
                <h4 className='text-[13px] font-semibold' style={{ color: 'var(--v2-ink)' }}>{READ_COPY.products[warning.product]}</h4>
                <p className='text-[13px] whitespace-pre-wrap' style={{ color: 'var(--v2-soft)' }}>{warning.reason}</p>
                <SetupWarningResult warning={warning} />
            </li>)}
        </ol>
    </section>
}
