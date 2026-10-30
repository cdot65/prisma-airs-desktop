import { useEffect, useState } from 'react'
import type { RunRecord, RunView, RunningView, SendReviewView } from '@prisma-airs/desktop-contracts'
import { type WorkbenchCall, resultText, runFailureText, runSummary, sizeText } from '../../features/workbench/workbench-call'
import { clockOf } from '../copy'
import { Btn } from '../controls'
import { Glyph } from '../icons'
import { API_COPY as C } from './copy'
import { prettyJson } from './request-rows'
import { requestName } from './request-tree'

type ResponseTab = 'body' | 'headers' | 'details' | 'history'
const TABS: ResponseTab[] = ['body', 'headers', 'details', 'history']

const faint = { color: 'var(--v2-faint)' }
const soft = { color: 'var(--v2-soft)' }
const MONO = "ui-monospace, 'SF Mono', Menlo, monospace"
const BOX = {
    margin: '0 14px 12px',
    border: '1px solid var(--v2-rule)',
    borderRadius: 8,
    background: 'var(--v2-paper)',
}

/** The status pill's tone: the code's class, a failure, or a run that never completed. */
function tone(record: RunRecord): {
    fg: string
    bg: string
    text: string
    glyph: 'check' | 'alert' | 'ban' | 'x'
} {
    if (record.state === 'completed' && record.status !== null) {
        const code = record.status
        return code < 300
            ? {
                  fg: 'var(--v2-ok)',
                  bg: 'var(--v2-ok-bg)',
                  text: `${code} ${code === 200 ? 'OK' : ''}`.trim(),
                  glyph: 'check',
              }
            : code < 500
              ? {
                    fg: 'var(--v2-flag)',
                    bg: 'var(--v2-flag-bg)',
                    text: String(code),
                    glyph: 'alert',
                }
              : {
                    fg: 'var(--v2-block)',
                    bg: 'var(--v2-block-bg)',
                    text: String(code),
                    glyph: 'ban',
                }
    }
    const text = record.state === 'interrupted' ? 'Interrupted' : record.state === 'cancelled' ? 'Cancelled' : record.state === 'running' ? 'Running' : 'Failed'
    return { fg: 'var(--v2-block)', bg: 'var(--v2-block-bg)', text, glyph: 'x' }
}

export function elapsedText(ms: number): string {
    return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(2)} s`
}

interface Props {
    /** The run Send produced for this request (null before the first send). */
    run: RunView | null
    running: RunningView[]
    review: SendReviewView | null
    call: WorkbenchCall
    /** Changes when history may have changed (the collection, a send). */
    historyKey: string
    onApply: () => void
    onDismiss: () => void
}

/**
 * The response pane (R2App's right 40%): the status line, the Body,
 * Headers, Details and History tabs. A run shown from History is the
 * immutable record of what happened then; nothing here sends anything.
 */
export function ResponsePane({ run, running, review, call, historyKey, onApply, onDismiss }: Props) {
    const [tab, setTab] = useState<ResponseTab>('body')
    const [pretty, setPretty] = useState(true)
    const [fromHistory, setFromHistory] = useState<RunView | null>(null)
    const [message, setMessage] = useState('')
    // A new send shows its own run and returns to Body.
    const runId = run?.record.id ?? null
    useEffect(() => {
        setFromHistory(null)
        setMessage('')
        if (runId) setTab('body')
    }, [runId])
    const shown = fromHistory ?? run
    const record = shown?.record ?? null
    const formatted = shown?.preview != null ? prettyJson(shown.preview) : null

    const save = async () => {
        if (!record) return
        setMessage('')
        const r = await call({ kind: 'saveResponse', run: record.id })
        if (r.kind === 'responseSaved') setMessage(C.saved(sizeText(r.bytes), r.name))
        else if (r.kind !== 'cancelled') setMessage(resultText(r) ?? '')
    }
    const copy = async () => {
        if (shown?.preview == null) return
        try {
            await navigator.clipboard.writeText(shown.preview)
            setMessage(C.copied)
        } catch {
            setMessage(C.copyFailed)
        }
    }

    const first = running[0] ?? null
    const head = record ? (
        <>
            <span
                className='inline-flex items-center rounded-[6px] font-bold'
                style={{
                    gap: 4,
                    padding: '2px 9px',
                    background: tone(record).bg,
                    color: tone(record).fg,
                }}
                data-slot='response-status'
            >
                <Glyph name={tone(record).glyph} size={11} color={tone(record).fg} />
                {tone(record).text}
            </span>
            <span style={soft}>{elapsedText(record.elapsedMs)}</span>
            <span style={soft}>{sizeText(record.bytes)}</span>
            <span className='ml-auto truncate text-[12px]' style={faint}>
                {fromHistory ? `${requestName(record.relative)} · ${clockOf(record.at)}` : clockOf(record.at)}
            </span>
        </>
    ) : first ? (
        <span role='status' aria-live='polite' className='flex flex-wrap items-center text-[12.5px]' style={{ gap: 8 }} data-slot='workbench-running'>
            {C.sendingProgress(first.method, sizeText(first.bytes))}
            <Btn className='h-7' onClick={() => void call({ kind: 'cancel', run: first.run })}>
                {C.cancel_}
            </Btn>
        </span>
    ) : (
        <span style={faint}>{C.noResponseYet}</span>
    )

    return (
        <section
            aria-label={C.pane}
            data-slot='workbench-response-pane'
            className='flex min-h-0 min-w-0 flex-col overflow-auto'
            style={{ flex: '0 0 40%', minWidth: 'min(100%, 340px)', maxWidth: 440 }}
        >
            <div
                className='flex items-center text-[13px]'
                style={{
                    gap: 10,
                    minHeight: 48,
                    padding: '9px 14px',
                    borderBottom: '1px solid var(--v2-rule)',
                }}
                data-slot='response-head'
            >
                {head}
            </div>
            {review ? (
                <div
                    role='alert'
                    className='flex flex-col rounded-[10px] text-[13px]'
                    style={{
                        gap: 8,
                        margin: '12px 14px 0',
                        padding: '12px 14px',
                        background: 'var(--v2-flag-bg)',
                    }}
                    data-slot='workbench-review'
                >
                    <p className='font-semibold'>{C.review.title(review.operation)}</p>
                    <p className='break-all font-mono text-[12px]'>{`${review.method} ${review.url}`}</p>
                    <p>{C.review.goes(review.destination)}</p>
                    {review.headers.length ? (
                        <ul className='font-mono text-[12px]'>
                            {review.headers.map(([n, v], i) => (
                                <li key={i} className='break-all'>{`${n}: ${v}`}</li>
                            ))}
                        </ul>
                    ) : null}
                    {review.body ? (
                        <pre className='max-h-48 overflow-auto rounded-[8px] p-2 font-mono text-[12px] whitespace-pre-wrap' style={{ background: 'var(--v2-paper)' }}>
                            {review.body}
                        </pre>
                    ) : null}
                    {review.files.length ? <p>{C.review.files(review.files.join(', '))}</p> : null}
                    <p className='text-[12px]' style={faint}>
                        {C.review.expires(new Date(review.expires * 1000).toLocaleTimeString())}
                    </p>
                    <div className='flex flex-wrap' style={{ gap: 8 }}>
                        <Btn tier='primary' onClick={onApply}>
                            {C.review.apply}
                        </Btn>
                        <Btn onClick={onDismiss}>{C.review.dismiss}</Btn>
                    </div>
                </div>
            ) : null}
            <div
                role='tablist'
                aria-label={`${C.response} views`}
                className='flex flex-wrap'
                style={{
                    gap: 2,
                    padding: '0 10px',
                    borderBottom: '1px solid var(--v2-rule)',
                }}
            >
                {TABS.map((t) => (
                    <button
                        key={t}
                        type='button'
                        role='tab'
                        aria-selected={tab === t}
                        aria-controls={`response-panel-${t}`}
                        onClick={() => setTab(t)}
                        className='min-h-0 text-[12.5px] font-semibold'
                        style={{
                            height: 38,
                            padding: '0 10px',
                            border: 0,
                            borderBottom: `2px solid ${tab === t ? 'var(--v2-acc)' : 'transparent'}`,
                            background: 'transparent',
                            color: tab === t ? 'var(--v2-ink)' : 'var(--v2-soft)',
                        }}
                    >
                        {C.responseTabs[t]}
                        {t === 'headers' && record?.headers.length ? (
                            <span className='font-normal' style={{ fontSize: 12, ...faint }}>
                                {' '}
                                {record.headers.length}
                            </span>
                        ) : null}
                    </button>
                ))}
            </div>
            <div role='tabpanel' id={`response-panel-${tab}`} className='flex min-h-0 flex-grow flex-col'>
                {tab === 'history' ? (
                    <History
                        call={call}
                        revision={historyKey}
                        onShow={(view) => {
                            setFromHistory(view)
                            setTab('body')
                        }}
                    />
                ) : !shown || !record ? (
                    <p className='text-[13px]' style={{ margin: '20px 14px', ...faint }} data-slot='workbench-no-response'>
                        {C.noResponse}
                    </p>
                ) : (
                    <section aria-label={C.response} data-slot='workbench-response' className='flex min-h-0 flex-grow flex-col text-[13px]'>
                        <p className='sr-only' data-slot='workbench-run-summary'>
                            {runSummary(record)}
                        </p>
                        {record.failure ? (
                            <p role='alert' style={{ margin: '12px 14px 0' }}>
                                {runFailureText(record.failure)}
                            </p>
                        ) : null}
                        {record.uncertain ? (
                            <p
                                role='alert'
                                className='rounded-[8px]'
                                style={{
                                    margin: '12px 14px 0',
                                    padding: '10px 12px',
                                    background: 'var(--v2-flag-bg)',
                                }}
                            >
                                {C.uncertain}
                            </p>
                        ) : null}
                        {record.redirect ? <p style={{ margin: '12px 14px 0' }}>{C.redirect(record.redirect)}</p> : null}
                        {shown.tokenDropped ? <p style={{ margin: '12px 14px 0' }}>{C.tokenDropped}</p> : null}
                        {tab === 'body' ? (
                            <>
                                <div className='flex flex-wrap items-center' style={{ gap: 10, padding: '12px 18px' }}>
                                    <span className='text-[12px]' style={faint}>
                                        {formatted !== null ? 'JSON' : shown.preview === null ? '' : 'Text'}
                                    </span>
                                    <span className='flex rounded-[7px] text-[12px]' style={{ padding: 2, background: 'var(--v2-inset)' }} role='group' aria-label='Body format'>
                                        {(['pretty', 'raw'] as const).map((f) => (
                                            <button
                                                key={f}
                                                type='button'
                                                aria-pressed={pretty === (f === 'pretty')}
                                                onClick={() => setPretty(f === 'pretty')}
                                                className='min-h-0 rounded-[5px] font-semibold'
                                                style={{
                                                    padding: '2px 9px',
                                                    border: 0,
                                                    background: pretty === (f === 'pretty') ? 'var(--v2-paper)' : 'transparent',
                                                    color: 'var(--v2-ink)',
                                                }}
                                            >
                                                {f === 'pretty' ? C.pretty : C.raw}
                                            </button>
                                        ))}
                                    </span>
                                    <span className='ml-auto flex items-center' style={{ gap: 6 }}>
                                        <Btn className='h-7' disabled={shown.preview == null} onClick={() => void copy()}>
                                            {C.copy}
                                        </Btn>
                                        <Btn className='h-7' disabled={!record.stored} onClick={() => void save()}>
                                            {C.saveResponse}
                                        </Btn>
                                    </span>
                                </div>
                                {shown.preview !== null && shown.preview !== '' ? (
                                    pretty && formatted !== null ? (
                                        <pre
                                            role='region'
                                            aria-label={C.responseBody}
                                            data-slot='json-lines'
                                            className='min-h-0 flex-grow overflow-auto text-[12px] leading-[1.75]'
                                            style={{
                                                ...BOX,
                                                padding: '10px 0',
                                                fontFamily: MONO,
                                                color: 'var(--v2-ink)',
                                            }}
                                        >
                                            {formatted.split('\n').map((line, i) => (
                                                <span key={i} data-line=''>
                                                    {line}
                                                    {'\n'}
                                                </span>
                                            ))}
                                        </pre>
                                    ) : (
                                        <pre
                                            role='region'
                                            aria-label={C.responseBody}
                                            className='min-h-0 flex-grow overflow-auto text-[12px] leading-[1.6] break-all whitespace-pre-wrap'
                                            style={{
                                                ...BOX,
                                                padding: 12,
                                                fontFamily: MONO,
                                                color: 'var(--v2-ink)',
                                            }}
                                        >
                                            {shown.preview}
                                        </pre>
                                    )
                                ) : (
                                    <p style={{ margin: '0 14px 12px', ...faint }}>{shown.preview === null && record.bytes > 0 ? C.notText : C.noBody}</p>
                                )}
                                {record.bytes > shown.previewBytes && shown.preview !== null ? (
                                    <p className='text-[12px]' style={{ margin: '0 14px 12px', ...faint }}>
                                        {C.firstBytes(sizeText(shown.previewBytes))}
                                    </p>
                                ) : null}
                            </>
                        ) : tab === 'headers' ? (
                            record.headers.length ? (
                                <div role='table' aria-label={C.responseHeaders} className='overflow-hidden text-[12px]' style={{ ...BOX, margin: '12px 14px', fontFamily: MONO }}>
                                    {record.headers.map(([n, v], i) => (
                                        <div
                                            key={i}
                                            role='row'
                                            className='grid'
                                            style={{
                                                gridTemplateColumns: '1fr 1.6fr',
                                                borderTop: i ? '1px solid var(--v2-rule)' : 0,
                                            }}
                                        >
                                            <span role='cell' className='break-all' style={{ padding: '8px 10px', ...soft }}>
                                                {n}
                                            </span>
                                            <span
                                                role='cell'
                                                className='break-all'
                                                style={{
                                                    padding: '8px 10px',
                                                    borderLeft: '1px solid var(--v2-rule)',
                                                }}
                                            >
                                                {v}
                                            </span>
                                        </div>
                                    ))}
                                </div>
                            ) : (
                                <p style={{ margin: '20px 14px', ...faint }}>No headers were recorded.</p>
                            )
                        ) : (
                            <dl
                                className='flex flex-col text-[12.5px]'
                                style={{
                                    ...BOX,
                                    margin: '12px 14px',
                                    padding: '12px 14px',
                                    gap: 8,
                                }}
                                data-slot='response-details'
                            >
                                {(
                                    [
                                        [C.details.request, `${record.method} ${record.url}`],
                                        [C.details.status, runSummary(record)],
                                        [C.details.duration, elapsedText(record.elapsedMs)],
                                        [C.details.size, sizeText(record.bytes)],
                                        [C.details.when, new Date(record.at * 1000).toLocaleString()],
                                        [C.details.credential, record.class === 'anonymous' ? C.details.anonymous : record.class],
                                        [C.details.operation, record.operation ?? '—'],
                                        [C.details.stored, record.stored ? C.details.yes : C.details.no],
                                    ] as [string, string][]
                                ).map(([k, v]) => (
                                    <div key={k} className='flex justify-between' style={{ gap: 10 }}>
                                        <dt style={faint}>{k}</dt>
                                        <dd className='min-w-0 break-all text-right'>{v}</dd>
                                    </div>
                                ))}
                            </dl>
                        )}
                    </section>
                )}
            </div>
            <p role='status' aria-live='polite' className='text-[12px]' style={{ margin: '0 14px 10px', ...soft }} data-slot='workbench-response-saved'>
                {message}
            </p>
        </section>
    )
}

/** The collection's history: what was sent, never sent again. */
function History({ call, revision, onShow }: { call: WorkbenchCall; revision: string; onShow: (run: RunView) => void }) {
    const [runs, setRuns] = useState<RunRecord[]>([])
    const [more, setMore] = useState(false)
    const [message, setMessage] = useState('')
    useEffect(() => {
        let live = true
        void call({ kind: 'history', before: null }).then((r) => {
            if (!live) return
            if (r.kind === 'history') {
                setRuns(r.runs)
                setMore(r.more)
            } else setMessage(resultText(r) ?? '')
        })
        return () => {
            live = false
        }
    }, [call, revision])
    const older = async () => {
        const last = runs.at(-1)
        if (!last) return
        const r = await call({ kind: 'history', before: last.id })
        if (r.kind === 'history') {
            setRuns((x) => [...x, ...r.runs])
            setMore(r.more)
        }
    }
    const clear = async () => {
        const r = await call({ kind: 'clearHistory' })
        if (r.kind === 'done') {
            setRuns([])
            setMore(false)
        } else setMessage(resultText(r) ?? '')
    }
    return (
        <section aria-label='History' data-slot='workbench-history' className='flex flex-col text-[13px]' style={{ gap: 8, margin: '12px 14px' }}>
            <p className='text-[12px]' style={soft}>
                {C.historyLead}
            </p>
            {message ? <p role='alert'>{message}</p> : null}
            {runs.length === 0 ? <p style={faint}>{C.historyEmpty}</p> : null}
            <ul className='flex flex-col' style={{ gap: 8 }}>
                {runs.map((r) => {
                    const t = tone(r)
                    const summary = `${r.method} ${r.relative.replace(/\.request\.toml$/, '')} · ${runSummary(r)} · ${new Date(r.at * 1000).toLocaleString()}`
                    return (
                        <li key={r.id}>
                            <button
                                type='button'
                                aria-label={summary}
                                onClick={() =>
                                    void call({ kind: 'showRun', run: r.id }).then((x) => {
                                        if (x.kind === 'ran') onShow(x.run)
                                        else setMessage(resultText(x) ?? '')
                                    })
                                }
                                className='flex w-full min-h-0 items-center rounded-[8px] border text-left'
                                style={{
                                    gap: 10,
                                    padding: '10px 12px',
                                    borderColor: 'var(--v2-rule)',
                                    background: 'var(--v2-paper)',
                                    color: 'var(--v2-ink)',
                                }}
                                data-slot='history-run'
                            >
                                <span className='rounded-[6px] text-[12px] font-bold' style={{ padding: '2px 8px', background: t.bg, color: t.fg }}>
                                    {t.text}
                                </span>
                                <span className='min-w-0 flex-grow truncate'>{`${r.method} ${requestName(r.relative)} · ${new Date(r.at * 1000).toLocaleString()}`}</span>
                                <span className='text-[12px]' style={faint}>
                                    {elapsedText(r.elapsedMs)}
                                </span>
                            </button>
                        </li>
                    )
                })}
            </ul>
            <div className='flex flex-wrap' style={{ gap: 8 }}>
                {more ? <Btn onClick={() => void older()}>{C.older}</Btn> : null}
                {runs.length ? (
                    <Btn tier='tertiary' onClick={() => void clear()}>
                        {C.clearHistory}
                    </Btn>
                ) : null}
                <Btn tier='tertiary' onClick={() => void call({ kind: 'clearCookies' }).then((r) => setMessage(r.kind === 'done' ? C.cookiesCleared : (resultText(r) ?? '')))}>
                    {C.clearCookies}
                </Btn>
            </div>
        </section>
    )
}
