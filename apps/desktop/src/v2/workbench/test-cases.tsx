import { forwardRef, useEffect, useId, useMemo, useRef, useState } from 'react'
import type { CaseProduct, CaseView, FeatureId, Intent, LibraryNotice, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { CASE_PROBLEM_TEXT } from '../../features/library/library-view'
import { type Dispatch, outcomeMessage } from '../../features/tenants/dispatch'
import { clockOf } from '../copy'
import { useShellStore } from '../store'
import { Verdict, type VerdictTone } from '../verdict'
import { CASES_COPY as C, PRODUCT_LABEL } from './copy'

export interface TestCasesProps {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    onNavigate: (feature: FeatureId) => void
}

/** Cases shown per page (prototype `caseRows`). */
export const PAGE_SIZE = 20

const PRODUCTS: CaseProduct[] = ['runtime-security', 'ai-gateway', 'ai-red-teaming', 'model-security', 'skill-scanning']

/** What a library notice says (the operation's established outcome). */
function noticeText(notice: LibraryNotice): string {
    switch (notice.kind) {
        case 'saved':
            return C.notice.saved(notice.name)
        case 'imported':
            return C.notice.imported(notice.added)
        case 'examplesAdded':
            return C.notice.examplesAdded(notice.added)
        case 'copied':
            return C.notice.copied(notice.tenant)
        case 'catalogUnavailable':
            return C.notice.catalogUnavailable
        case 'fileUnreadable':
            return C.notice.fileUnreadable
    }
}

const BUTTON = 'min-h-8 rounded-[7px] border px-3 text-[13px]'

const toneOf = (verdict: string): VerdictTone => (verdict === 'block' ? 'blocked' : verdict === 'alert' ? 'flagged' : verdict === 'allow' ? 'allowed' : 'info')

/** The text a case is searched by: its name, pack and input. */
const haystack = (c: CaseView) => `${c.name} ${c.pack ?? ''} ${c.prompt} ${c.response}`.toLowerCase()

/**
 * Test Cases in the v2 shell (C33): the tenant's library with search, a
 * product filter and pages of 20. A Runtime Security case opens in the
 * Playground, where the operator runs the scan; any other product's case is
 * inspected only. Nothing on this screen dispatches a scan (V2-T15).
 */
export const TestCases = forwardRef<HTMLHeadingElement, TestCasesProps>(function TestCases({ snapshot, dispatch, onNavigate }, ref) {
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) ?? null
    const context = snapshot.active?.context ?? null
    const library = snapshot.library
    const [query, setQuery] = useState('')
    const [product, setProduct] = useState<CaseProduct | null>(null)
    const [page, setPage] = useState(0)
    const [inspected, setInspected] = useState<string | null>(null)
    const [authoring, setAuthoring] = useState(false)
    const [copyTo, setCopyTo] = useState<string>('')
    const [message, setMessage] = useState<string | null>(null)
    const others = snapshot.tenants.filter((t) => t.id !== tenant?.id)
    const ready = library.state === 'ready'

    /** Dispatches a library intent; a refusal the library itself explains (its problem) is not said twice. */
    async function send(intent: Intent) {
        const outcome = await dispatch(intent)
        setMessage(outcome.kind === 'admitted' || (outcome.kind === 'refused' && outcome.refusal === 'invalidInput') ? null : outcomeMessage(outcome))
        return outcome
    }

    // The library opens for the tenant shown (as the legacy view does).
    useEffect(() => {
        if (context) void dispatch({ kind: 'openLibrary', context })
    }, [context, dispatch])

    const cases = library.cases
    const counts = useMemo(() => {
        const by = new Map<CaseProduct, number>()
        for (const c of cases) by.set(c.product, (by.get(c.product) ?? 0) + 1)
        return by
    }, [cases])
    const needle = query.trim().toLowerCase()
    const matching = cases.filter((c) => (product === null || c.product === product) && (needle === '' || haystack(c).includes(needle)))
    const pages = Math.max(1, Math.ceil(matching.length / PAGE_SIZE))
    const shownPage = Math.min(page, pages - 1)
    const rows = matching.slice(shownPage * PAGE_SIZE, shownPage * PAGE_SIZE + PAGE_SIZE)
    const detail = cases.find((c) => c.id === inspected) ?? null

    // A new search or filter starts at the first page.
    useEffect(() => setPage(0), [needle, product])

    function openInPlayground(c: CaseView) {
        if (!tenant) return
        useShellStore.getState().openPlayground({
            tenant: tenant.id,
            caseId: c.id,
            prefill: { name: c.name, profile: c.profile, prompt: c.prompt, response: c.response, appName: c.appName, expected: c.expected },
        })
        onNavigate('administration')
    }

    const heading = (
        <div className='flex flex-wrap items-end justify-between gap-3'>
            <div className='flex flex-col gap-1'>
                <h1 ref={ref} id='cases-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)' }}>
                    {C.title}
                </h1>
                {tenant ? (
                    <p className='text-[13px]' style={{ color: 'var(--v2-soft)' }} data-slot='cases-count'>
                        {C.count(cases.length, tenant.label)}
                    </p>
                ) : null}
            </div>
            {tenant && context ? (
                <div className='flex flex-wrap gap-2' data-slot='cases-actions'>
                    <button type='button' className={BUTTON} style={{ borderColor: 'var(--v2-rule)' }} disabled={!ready} aria-expanded={authoring} onClick={() => setAuthoring((a) => !a)}>
                        {C.newCase}
                    </button>
                    <button type='button' className={BUTTON} style={{ borderColor: 'var(--v2-rule)' }} disabled={!ready} onClick={() => void send({ kind: 'importExamples', context, source: 'bundled' })}>
                        {C.addPacks}
                    </button>
                    <button type='button' className={BUTTON} style={{ borderColor: 'var(--v2-rule)' }} disabled={!ready || library.fetching} onClick={() => void send({ kind: 'importExamples', context, source: 'public' })}>
                        {C.addPublic}
                    </button>
                    <button type='button' className={BUTTON} style={{ borderColor: 'var(--v2-rule)' }} disabled={!ready} aria-disabled={library.choosing} onClick={() => void send({ kind: 'importCases', context })}>
                        {C.importCases}
                    </button>
                    <button type='button' className={BUTTON} style={{ borderColor: 'var(--v2-rule)' }} disabled={!ready || cases.length === 0} onClick={() => void send({ kind: 'exportCases', context })}>
                        {C.exportCases}
                    </button>
                </div>
            ) : null}
        </div>
    )

    if (!tenant || !context) {
        return (
            <section aria-labelledby='cases-heading' data-slot='cases-v2' className='flex flex-col gap-4'>
                {heading}
                <p className='text-[14px]'>{C.noTenant}</p>
            </section>
        )
    }

    return (
        <section aria-labelledby='cases-heading' data-slot='cases-v2' className='flex min-w-0 flex-col gap-4'>
            {heading}
            {library.state === 'failed' && library.failure ? (
                <div role='alert' className='flex flex-col gap-2 rounded-[12px] border p-3 text-[13px]' style={{ borderColor: 'var(--v2-block)' }} data-slot='cases-failed'>
                    <p>{C.failure[library.failure] ?? C.failed}</p>
                    <button type='button' className={`${BUTTON} self-start`} style={{ borderColor: 'var(--v2-rule)' }} onClick={() => void send({ kind: 'openLibrary', context })}>
                        {C.reopen}
                    </button>
                </div>
            ) : null}
            {library.unsaved ? (
                <div role='alert' className='flex flex-col gap-2 rounded-[12px] border p-3 text-[13px]' style={{ borderColor: 'var(--v2-block)' }} data-slot='cases-unsaved'>
                    <p>{C.unsaved(C.failure[library.unsaved] ?? library.unsaved)}</p>
                    <button type='button' className={`${BUTTON} self-start`} style={{ borderColor: 'var(--v2-rule)' }} onClick={() => void send({ kind: 'saveLibraryAgain', context })}>
                        {C.saveAgain}
                    </button>
                </div>
            ) : null}
            {library.problem ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-block)' }} data-slot='cases-problem'>
                    {CASE_PROBLEM_TEXT[library.problem]}
                </p>
            ) : null}
            {library.notice ? (
                <p role='status' className='text-[13px]' style={{ color: 'var(--v2-soft)' }} data-slot='cases-notice'>
                    {noticeText(library.notice)}
                </p>
            ) : null}
            {message ? (
                <p role='alert' className='text-[13px]'>
                    {message}
                </p>
            ) : null}
            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }} data-slot='cases-transfer-note'>
                {C.transferNote}
            </p>
            {library.export ? (
                <p
                    role={library.export.state === 'failed' ? 'alert' : 'status'}
                    className='text-[13px]'
                    style={{ color: library.export.state === 'failed' ? 'var(--v2-block)' : 'var(--v2-soft)' }}
                    data-slot='cases-export'
                    data-state={library.export.state}
                >
                    {library.export.state === 'saving'
                        ? C.export.saving
                        : library.export.state === 'saved'
                          ? C.export.saved(library.export.name ?? '')
                          : library.export.state === 'cancelled'
                            ? C.export.cancelled
                            : C.export.failed}
                </p>
            ) : null}
            {authoring && ready ? (
                <CaseForm
                    initialProduct={product ?? 'runtime-security'}
                    onSave={async (draft) => {
                        const outcome = await send({ kind: 'saveCase', context, draft })
                        if (outcome.kind === 'admitted') setAuthoring(false)
                    }}
                    onCancel={() => setAuthoring(false)}
                />
            ) : null}
            <div className='flex flex-wrap items-center gap-2'>
                <input
                    type='search'
                    aria-label={C.search}
                    placeholder={C.search}
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    className='h-8 rounded-[7px] border px-2 text-[13px]'
                    style={{ borderColor: 'var(--v2-rule)', width: 'min(100%, 280px)', background: 'var(--v2-paper)' }}
                    data-slot='cases-search'
                />
                <div role='group' aria-label={C.filter} className='flex flex-wrap gap-1' data-slot='cases-filter'>
                    {[null, ...PRODUCTS].map((p) => (
                        <button
                            key={p ?? 'all'}
                            type='button'
                            aria-pressed={product === p}
                            onClick={() => setProduct(p)}
                            className='h-8 rounded-[7px] border px-3 text-[13px]'
                            style={{ borderColor: product === p ? 'var(--v2-acc)' : 'var(--v2-rule)' }}
                        >
                            {p === null ? `${C.all} (${cases.length})` : `${PRODUCT_LABEL[p]} (${counts.get(p) ?? 0})`}
                        </button>
                    ))}
                </div>
            </div>
            {others.length > 0 ? (
                <label className='flex flex-wrap items-center gap-2 text-[13px]' data-slot='cases-copy-to'>
                    {C.copyTo}
                    <select className='h-8 rounded-[7px] border px-2' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }} value={copyTo} onChange={(e) => setCopyTo(e.target.value)}>
                        <option value=''>—</option>
                        {others.map((t) => (
                            <option key={t.id} value={t.id}>
                                {t.label}
                            </option>
                        ))}
                    </select>
                </label>
            ) : null}
            {library.state === 'loading' ? <p role='status'>{C.loading}</p> : null}
            {library.state === 'ready' && rows.length === 0 ? <p data-slot='cases-empty'>{cases.length === 0 ? C.empty : C.noMatch}</p> : null}
            {rows.length > 0 ? (
                <table className='w-full table-fixed text-left text-[13px]' data-slot='cases-table'>
                    <caption className='sr-only'>{C.title}</caption>
                    <thead>
                        <tr style={{ color: 'var(--v2-soft)' }}>
                            <th scope='col' className='py-2 pr-3 font-medium' style={{ width: '24%' }}>
                                {C.columns.name}
                            </th>
                            <th scope='col' className='py-2 pr-3 font-medium' style={{ width: '16%' }}>
                                {C.columns.product}
                            </th>
                            <th scope='col' className='py-2 pr-3 font-medium'>
                                {C.columns.input}
                            </th>
                            <th scope='col' className='py-2 pr-3 font-medium' style={{ width: '17%' }}>
                                {C.columns.result}
                            </th>
                            <th scope='col' className='py-2 font-medium' style={{ width: '17%' }}>
                                {C.columns.action}
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        {rows.map((c) => (
                            <tr key={c.id} className='border-t align-top' style={{ borderColor: 'var(--v2-rule)' }} data-slot='case-row' data-product={c.product}>
                                <td className='py-2 pr-3'>
                                    <span className='block truncate font-medium'>{c.name}</span>
                                    {c.pack ? (
                                        <span className='block truncate text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                            {c.pack}
                                        </span>
                                    ) : null}
                                </td>
                                <td className='py-2 pr-3 break-words'>{PRODUCT_LABEL[c.product]}</td>
                                <td className='py-2 pr-3'>
                                    <span className='block truncate'>{c.prompt || c.response}</span>
                                </td>
                                <td className='py-2 pr-3'>
                                    <CaseResultPill c={c} />
                                </td>
                                <td className='flex flex-col items-start gap-1 py-2'>
                                    {c.product === 'runtime-security' ? (
                                        <button
                                            type='button'
                                            className='min-h-8 max-w-full rounded-[7px] px-2 py-1 text-[13px] font-semibold whitespace-normal'
                                            style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}
                                            aria-label={`${C.openInPlayground}: ${c.name}`}
                                            onClick={() => openInPlayground(c)}
                                        >
                                            {C.openInPlayground}
                                        </button>
                                    ) : (
                                        <button
                                            type='button'
                                            className='min-h-8 max-w-full rounded-[7px] border px-2 py-1 text-[13px] whitespace-normal'
                                            style={{ borderColor: 'var(--v2-rule)' }}
                                            aria-label={`${C.inspect}: ${c.name}`}
                                            aria-expanded={inspected === c.id}
                                            onClick={() => setInspected(inspected === c.id ? null : c.id)}
                                        >
                                            {C.inspect}
                                        </button>
                                    )}
                                    <span className='flex flex-wrap gap-1'>
                                        <button
                                            type='button'
                                            className='min-h-7 rounded-[7px] px-2 text-[12px] underline'
                                            aria-label={C.removeCase(c.name)}
                                            onClick={() => void send({ kind: 'deleteCase', context, id: c.id })}
                                        >
                                            {C.remove}
                                        </button>
                                        {copyTo ? (
                                            <button
                                                type='button'
                                                className='min-h-7 rounded-[7px] px-2 text-[12px] underline'
                                                aria-label={C.copyCase(c.name, others.find((t) => t.id === copyTo)?.label ?? '')}
                                                onClick={() => void send({ kind: 'copyCase', context, id: c.id, tenant: copyTo })}
                                            >
                                                {C.copy}
                                            </button>
                                        ) : null}
                                    </span>
                                </td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            ) : null}
            {matching.length > PAGE_SIZE ? (
                <div className='flex items-center justify-end gap-2 text-[13px]' data-slot='cases-pages'>
                    <span role='status'>{C.range(shownPage * PAGE_SIZE + 1, Math.min(matching.length, (shownPage + 1) * PAGE_SIZE), matching.length)}</span>
                    <button
                        type='button'
                        className='h-8 rounded-[7px] border px-3'
                        style={{ borderColor: 'var(--v2-rule)' }}
                        disabled={shownPage === 0}
                        onClick={() => setPage(shownPage - 1)}
                    >
                        {C.previous}
                    </button>
                    <button
                        type='button'
                        className='h-8 rounded-[7px] border px-3'
                        style={{ borderColor: 'var(--v2-rule)' }}
                        disabled={shownPage >= pages - 1}
                        onClick={() => setPage(shownPage + 1)}
                    >
                        {C.next}
                    </button>
                </div>
            ) : null}
            {detail ? <CaseDetails c={detail} tenant={tenant.label} onClose={() => setInspected(null)} /> : null}
        </section>
    )
})

/** A case's last result as a verdict (colour, icon and word), or "Not run". */
function CaseResultPill({ c }: { c: CaseView }) {
    if (!c.result) {
        return (
            <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }} data-slot='case-not-run'>
                {C.notRun}
            </span>
        )
    }
    return (
        <span className='flex flex-col gap-0.5' data-slot='case-result'>
            <Verdict tone={toneOf(c.result.verdict)} word={C.verdictWord[c.result.verdict] ?? c.result.verdict} />
            <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                {C.resultAt(clockOf(c.result.at))}
            </span>
        </span>
    )
}

/** Inspect: the case's specification and recorded result, and which product runs it. Nothing is dispatched. */
function CaseDetails({ c, tenant, onClose }: { c: CaseView; tenant: string; onClose: () => void }) {
    return (
        <section aria-label={C.details} data-slot='case-details' className='flex flex-col gap-2 rounded-[12px] border p-4 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
            <div className='flex items-start justify-between gap-2'>
                <h2 className='text-[15px] font-semibold'>{c.name}</h2>
                <button type='button' className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }} onClick={onClose}>
                    {C.closeDetails}
                </button>
            </div>
            <dl className='grid gap-x-4 gap-y-1' style={{ gridTemplateColumns: 'max-content 1fr' }}>
                <dt style={{ color: 'var(--v2-soft)' }}>{C.columns.product}</dt>
                <dd>
                    {PRODUCT_LABEL[c.product]} · {tenant}
                </dd>
                {c.pack ? (
                    <>
                        <dt style={{ color: 'var(--v2-soft)' }}>{C.pack}</dt>
                        <dd>{c.pack}</dd>
                    </>
                ) : null}
                <dt style={{ color: 'var(--v2-soft)' }}>{C.specification}</dt>
                <dd className='whitespace-pre-wrap break-words'>{c.payload ?? c.prompt}</dd>
                {c.expected ? (
                    <>
                        <dt style={{ color: 'var(--v2-soft)' }}>{C.expected}</dt>
                        <dd>{c.expected}</dd>
                    </>
                ) : null}
                <dt style={{ color: 'var(--v2-soft)' }}>{C.lastResult}</dt>
                <dd>
                    <CaseResultPill c={c} />
                </dd>
                <dt style={{ color: 'var(--v2-soft)' }}>{C.runsFrom}</dt>
                <dd>{C.runs[c.product]}</dd>
            </dl>
            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                {C.neverRunHere}
            </p>
        </section>
    )
}

type CaseDraft = { product: CaseProduct; name: string; profile: string; prompt: string; response: string; appName: string; expected: string; payload: string }

const FIELD = 'flex flex-col gap-1 text-[13px] font-medium'
const INPUT = 'min-h-8 rounded-[7px] border px-2 text-[13px] font-normal'
const AREA = 'min-h-20 rounded-[7px] border px-2 py-1 text-[13px] font-normal'

/** A new case (C12's authoring): every field is read when Save is pressed; saving runs nothing. */
function CaseForm({ initialProduct, onSave, onCancel }: { initialProduct: CaseProduct; onSave: (draft: CaseDraft) => Promise<void>; onCancel: () => void }) {
    const ids = useId()
    const form = useRef<HTMLFormElement>(null)
    const [product, setProduct] = useState<CaseProduct>(initialProduct)
    const runtime = product === 'runtime-security'
    const F = C.form
    const box = { borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }
    return (
        <form
            ref={form}
            aria-label={F.title}
            className='flex flex-col gap-3 rounded-[12px] border p-4'
            style={{ borderColor: 'var(--v2-rule)' }}
            data-slot='case-form'
            onSubmit={(e) => {
                e.preventDefault()
                const data = new FormData(form.current ?? undefined)
                const read = (n: string) => (data.get(n) as string | null) ?? ''
                void onSave({
                    product,
                    name: read('name'),
                    profile: runtime ? read('profile') : '',
                    prompt: read('prompt'),
                    response: read('response'),
                    appName: runtime ? read('app') : '',
                    expected: read('expected'),
                    payload: runtime ? '' : read('payload'),
                })
            }}
        >
            <label className={FIELD}>
                {F.product}
                <select className={INPUT} style={box} value={product} onChange={(e) => setProduct(e.target.value as CaseProduct)}>
                    {PRODUCTS.map((p) => (
                        <option key={p} value={p}>
                            {PRODUCT_LABEL[p]}
                        </option>
                    ))}
                </select>
            </label>
            <label className={FIELD} htmlFor={`${ids}-name`}>
                {F.name}
                <input id={`${ids}-name`} name='name' maxLength={128} autoComplete='off' className={INPUT} style={box} />
            </label>
            {runtime ? (
                <>
                    <label className={FIELD} htmlFor={`${ids}-profile`}>
                        {F.profile}
                        <input id={`${ids}-profile`} name='profile' maxLength={256} autoComplete='off' spellCheck={false} className={INPUT} style={box} />
                    </label>
                    <label className={FIELD} htmlFor={`${ids}-app`}>
                        {F.app}
                        <input id={`${ids}-app`} name='app' maxLength={256} autoComplete='off' spellCheck={false} className={INPUT} style={box} />
                    </label>
                </>
            ) : null}
            <label className={FIELD} htmlFor={`${ids}-prompt`}>
                {F.prompt}
                <textarea id={`${ids}-prompt`} name='prompt' spellCheck={false} className={AREA} style={box} />
            </label>
            <label className={FIELD} htmlFor={`${ids}-response`}>
                {F.response}
                <textarea id={`${ids}-response`} name='response' spellCheck={false} className={AREA} style={box} />
            </label>
            <label className={FIELD} htmlFor={`${ids}-expected`}>
                {F.expected}
                <input id={`${ids}-expected`} name='expected' maxLength={1024} autoComplete='off' aria-describedby={`${ids}-expected-hint`} className={INPUT} style={box} />
            </label>
            <p id={`${ids}-expected-hint`} className='-mt-2 text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                {F.expectedHint}
            </p>
            {runtime ? null : (
                <label className={FIELD} htmlFor={`${ids}-payload`}>
                    {F.payload}
                    <textarea id={`${ids}-payload`} name='payload' defaultValue='{}' spellCheck={false} className={`${AREA} font-mono`} style={box} />
                </label>
            )}
            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                {F.note}
            </p>
            <div className='flex gap-2'>
                <button type='submit' className='min-h-8 rounded-[7px] px-3 text-[13px] font-semibold' style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}>
                    {F.save}
                </button>
                <button type='button' className={BUTTON} style={{ borderColor: 'var(--v2-rule)' }} onClick={onCancel}>
                    {F.cancel}
                </button>
            </div>
        </form>
    )
}
