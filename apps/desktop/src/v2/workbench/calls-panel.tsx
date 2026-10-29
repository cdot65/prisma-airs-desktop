import type { CallView, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../../features/tenants/dispatch'
import { CALLS_COPY as C } from './copy'

/** How long a call took, as the API Client's completion says it. */
const durationOf = (ms: number) => (ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`)

/**
 * "Show the calls" (C33, DESIGN-V2 §3): beside every destination while the
 * Rust-owned preference is on, the calls this app made for the active
 * tenant, newest last, as the core recorded them: the request line without
 * its query, header names (never values), the credential by kind (the core
 * attached it), and the response status, duration and content type. No body
 * is ever kept. Rendered like the API Client's response line.
 */
export function CallsPanel({ snapshot, dispatch }: { snapshot: ShellSnapshot; dispatch: Dispatch }) {
    const calls = snapshot.calls
    if (!snapshot.showCalls || calls === undefined) return null
    return (
        <aside aria-label={C.title} data-slot='calls-panel' className='flex min-w-0 flex-col gap-3 border-l p-4 text-[13px]' style={{ borderColor: 'var(--v2-rule)', width: 'min(100%, 340px)' }}>
            <div className='flex items-center justify-between gap-2'>
                <h2 className='text-[15px] font-semibold'>{C.title}</h2>
                <button type='button' className='h-8 rounded-[7px] border px-3 text-[12px]' style={{ borderColor: 'var(--v2-rule)' }} onClick={() => void dispatch({ kind: 'setShowCalls', on: false })}>
                    {C.hide}
                </button>
            </div>
            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                {C.note}
            </p>
            {calls.length === 0 ? (
                <p data-slot='calls-empty'>{C.empty}</p>
            ) : (
                <ol className='flex flex-col gap-2' data-slot='calls-list'>
                    {calls.map((call) => (
                        <CallItem key={call.sequence} call={call} />
                    ))}
                </ol>
            )}
        </aside>
    )
}

function CallItem({ call }: { call: CallView }) {
    const outcome = call.status !== null ? String(call.status) : C.failed
    return (
        <li className='flex flex-col gap-1 rounded-[10px] border p-3' style={{ borderColor: 'var(--v2-rule)' }} data-slot='call' data-purpose={call.purpose}>
            <div className='flex flex-wrap items-baseline gap-x-2 text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                <span>{C.purpose[call.purpose]}</span>
                <span>
                    {outcome} · {durationOf(call.durationMs)}
                </span>
            </div>
            <p className='break-all font-mono text-[12px]'>
                <span className='font-semibold' style={{ color: 'var(--v2-get)' }}>
                    {call.method}
                </span>{' '}
                {call.origin}
                {call.path}
                {call.query ? <span style={{ color: 'var(--v2-soft)' }}> ({C.query})</span> : null}
            </p>
            <p className='text-[12px]'>{C.credential[call.credential]}</p>
            <details className='text-[12px]'>
                <summary className='cursor-pointer'>
                    {C.requestHeaders} · {C.bytes(call.requestBytes)}
                </summary>
                <p className='break-all font-mono'>{call.requestHeaders.join(', ')}</p>
                <p className='mt-1' style={{ color: 'var(--v2-soft)' }}>
                    {C.responseHeaders}
                    {call.contentType ? ` · ${call.contentType}` : ''}
                </p>
                <p className='break-all font-mono'>{call.responseHeaders.join(', ')}</p>
            </details>
        </li>
    )
}
