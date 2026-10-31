import { forwardRef, useEffect, useId, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { CaseProduct, CaseView, FeatureId, Intent, LibraryNotice, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../../features/tenants/dispatch'
import { clockOf } from '../copy'
import { Btn, Field } from '../controls'
import { Glyph, type GlyphName } from '../icons'
import { Modal } from '../modal'
import { useShellStore } from '../store'
import { ADMIN_COPY } from '../admin/copy'
import { CASES_COPY as C, PRODUCT_LABEL } from './copy'

export interface TestCasesProps {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    onNavigate: (feature: FeatureId) => void
}

/** Cases shown per page (prototype `caseRows`). */
export const PAGE_SIZE = 20

const PRODUCTS: CaseProduct[] = ['runtime-security', 'ai-gateway', 'ai-red-teaming', 'model-security', 'skill-scanning']
const CASE_PROBLEM_TEXT = ADMIN_COPY.playground.caseProblem

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

/** The result pill's colours and glyph by verdict (R2App: blocked, flagged, allowed; not determined as a flag). */
const TONE: Record<string, { fg: string; bg: string; glyph: GlyphName }> = {
    block: { fg: 'var(--v2-block)', bg: 'var(--v2-block-bg)', glyph: 'ban' },
    alert: { fg: 'var(--v2-flag)', bg: 'var(--v2-flag-bg)', glyph: 'alert' },
    allow: { fg: 'var(--v2-ok)', bg: 'var(--v2-ok-bg)', glyph: 'check' },
    unknown: { fg: 'var(--v2-flag)', bg: 'var(--v2-flag-bg)', glyph: 'info' },
}

/** The text a case is searched by: its name, pack and input. */
const haystack = (c: CaseView) => `${c.name} ${c.pack ?? ''} ${c.prompt} ${c.response}`.toLowerCase()

const faint = { color: 'var(--v2-faint)' }
const soft = { color: 'var(--v2-soft)' }
const GRID = 'minmax(0, 1.6fr) minmax(0, 1.2fr) minmax(0, 2.2fr) minmax(0, 176px) minmax(0, 190px)'

/**
 * Test Cases as R2App draws it (C40): the title row with Load starter
 * packs… and New case, the Explain banner, one table card (search, product
 * pills with counts, the rows with the result pill and the action, the
 * footer with the range and pages), the packs and new-case dialogs and the
 * detail sheet. The C33 behaviour is unchanged: a Runtime Security case
 * opens in the Playground where the operator runs the scan; any other
 * product's case is inspected only. Nothing on this screen dispatches a
 * scan (V2-T15).
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
    const [packs, setPacks] = useState(false)
    const [menu, setMenu] = useState<string | null>(null)
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
        <div className='flex flex-wrap items-end' style={{ gap: 12 }}>
            <div className='min-w-0'>
                <h1 ref={ref} id='cases-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)', letterSpacing: '-0.01em' }}>
                    {C.title}
                </h1>
                {tenant ? (
                    <p className='text-[14px]' style={{ marginTop: 4, ...soft }} data-slot='cases-count'>
                        {C.count(cases.length, tenant.label)}
                    </p>
                ) : null}
            </div>
            {tenant && context ? (
                <div className='ml-auto flex shrink-0 flex-wrap' style={{ gap: 8 }} data-slot='cases-actions'>
                    <Btn disabled={!ready} aria-haspopup='dialog' onClick={() => setPacks(true)}>
                        {C.loadPacks}
                    </Btn>
                    <Btn tier='primary' className='px-4' disabled={!ready} aria-haspopup='dialog' onClick={() => setAuthoring(true)}>
                        {C.newCase}
                    </Btn>
                </div>
            ) : null}
        </div>
    )

    if (!tenant || !context) {
        return (
            <section aria-labelledby='cases-heading' data-slot='cases-v2' className='flex flex-col' style={{ gap: 20 }}>
                {heading}
                <p className='text-[14px]'>{C.noTenant}</p>
            </section>
        )
    }

    const pill = (p: CaseProduct | null) => {
        const on = product === p
        const n = p === null ? cases.length : (counts.get(p) ?? 0)
        return (
            <button
                key={p ?? 'all'}
                type='button'
                aria-pressed={on}
                onClick={() => setProduct(p)}
                className='flex h-7 min-h-0 items-center rounded-full border text-[12px]'
                style={{ gap: 4, padding: '0 12px', borderColor: on ? 'var(--v2-acc)' : 'var(--v2-rule)', background: on ? 'var(--v2-acc-soft)' : 'var(--v2-paper)', color: 'var(--v2-ink)' }}
            >
                {p === null ? C.all : PRODUCT_LABEL[p]}
                <span style={faint}>{n}</span>
            </button>
        )
    }

    // The library's problem (why the last change was refused) and a delivery that did not apply: on the page,
    // and inside the open dialog whose action met it (the page behind an open dialog is inert); the dialog stays
    // open with its fields, so the operator can correct and try again.
    const problems = (
        <>
            {library.unsaved ? (
                // The last change (an import included) is shown but not stored: said with its reason and Save again
                // wherever the operator is, the open dialog included (attempt-02 R2).
                <div role='alert' className='flex flex-col rounded-[10px] text-[13px]' style={{ gap: 8, padding: '12px 14px', background: 'var(--v2-block-bg)' }} data-slot='cases-unsaved'>
                    <p>{C.unsaved(C.failure[library.unsaved] ?? library.unsaved)}</p>
                    <Btn className='self-start' onClick={() => void send({ kind: 'saveLibraryAgain', context })}>
                        {C.saveAgain}
                    </Btn>
                </div>
            ) : null}
            {library.problem ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-block)' }} data-slot='cases-problem'>
                    {CASE_PROBLEM_TEXT[library.problem] ?? library.problem}
                </p>
            ) : null}
            {message ? (
                <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-block)' }} data-slot='cases-message'>
                    {message}
                </p>
            ) : null}
        </>
    )
    // The import/export status lines: on the page, and inside the packs dialog while it is open (the page behind
    // an open dialog is inert, so a save or cancel from the dialog is answered where the operator is looking).
    const transfer = (
        <>
            {library.notice ? (
                <p role='status' className='text-[13px]' style={soft} data-slot='cases-notice'>
                    {noticeText(library.notice)}
                </p>
            ) : null}
            {library.export ? (
                <p role={library.export.state === 'failed' ? 'alert' : 'status'} className='text-[13px]' style={{ color: library.export.state === 'failed' ? 'var(--v2-block)' : 'var(--v2-soft)' }} data-slot='cases-export' data-state={library.export.state}>
                    {library.export.state === 'saving' ? C.export.saving : library.export.state === 'saved' ? C.export.saved(library.export.name ?? '') : library.export.state === 'cancelled' ? C.export.cancelled : C.export.failed}
                </p>
            ) : null}
        </>
    )
    return (
        <section aria-labelledby='cases-heading' data-slot='cases-v2' className='flex min-w-0 flex-col' style={{ gap: 20 }}>
            {heading}
            {snapshot.showCalls ? (
                <div className='flex rounded-[10px] text-[13px] leading-[1.45]' style={{ gap: 8, padding: '12px 14px', background: 'var(--v2-acc-soft)', maxWidth: 860 }} data-slot='cases-explain'>
                    <Glyph name='info' size={15} color='var(--v2-acc)' className='mt-[1px] shrink-0' />
                    <span>{C.explain}</span>
                </div>
            ) : null}
            {library.state === 'failed' && library.failure ? (
                <div role='alert' className='flex flex-col rounded-[10px] text-[13px]' style={{ gap: 8, padding: '12px 14px', background: 'var(--v2-block-bg)' }} data-slot='cases-failed'>
                    <p>{C.failure[library.failure] ?? C.failed}</p>
                    <Btn className='self-start' onClick={() => void send({ kind: 'openLibrary', context })}>
                        {C.reopen}
                    </Btn>
                </div>
            ) : null}
            {problems}
            {transfer}
            <section className='shrink-0 overflow-hidden rounded-[12px] border' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }} data-slot='cases-card'>
                <div className='flex flex-wrap items-center' style={{ gap: 10, padding: '14px 18px', borderBottom: '1px solid var(--v2-rule)' }}>
                    <label className='flex items-center rounded-[7px]' style={{ gap: 7, width: 280, maxWidth: '100%', height: 32, padding: '0 9px', background: 'var(--v2-inset)' }}>
                        <Glyph name='search' size={12} color='var(--v2-faint)' />
                        <input
                            type='search'
                            aria-label={C.search}
                            placeholder={C.search}
                            value={query}
                            onChange={(e) => setQuery(e.target.value)}
                            className='min-h-0 min-w-0 flex-grow border-0 bg-transparent p-0 text-[13px]'
                            style={{ border: 0, minHeight: 0, borderRadius: 0, background: 'transparent', color: 'var(--v2-ink)', height: 24 }}
                            data-slot='cases-search'
                        />
                    </label>
                    <div role='group' aria-label={C.filter} className='flex flex-wrap' style={{ gap: 5 }} data-slot='cases-filter'>
                        {[null, ...PRODUCTS].map(pill)}
                    </div>
                </div>
                <div role='table' aria-label={C.title} data-slot='cases-table'>
                    <div role='row' className='grid text-[12px] font-semibold' style={{ gridTemplateColumns: GRID, gap: 12, padding: '12px 18px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}>
                        <span role='columnheader'>{C.columns.name}</span>
                        <span role='columnheader'>{C.columns.product}</span>
                        <span role='columnheader'>{C.columns2.input}</span>
                        <span role='columnheader'>{C.columns.result}</span>
                        <span role='columnheader'>
                            <span className='sr-only'>{C.columns.action}</span>
                        </span>
                    </div>
                    {rows.map((c) => {
                        const open = menu === c.id
                        return (
                            <div key={c.id} role='row' className='grid items-center' style={{ gridTemplateColumns: GRID, gap: 12, padding: '12px 18px', borderTop: '1px solid var(--v2-rule)' }} data-slot='case-row' data-product={c.product}>
                                <span role='cell' className='min-w-0'>
                                    <b className='block truncate font-semibold'>{c.name}</b>
                                    {c.pack ? (
                                        <span className='block truncate text-[12px]' style={faint}>
                                            {c.pack}
                                        </span>
                                    ) : null}
                                </span>
                                <span role='cell' className='min-w-0 truncate' style={soft}>
                                    {PRODUCT_LABEL[c.product]}
                                </span>
                                <span role='cell' className='min-w-0 truncate font-mono text-[12px]' style={soft}>
                                    {c.prompt || c.response}
                                </span>
                                <span role='cell' className='flex min-w-0 flex-col' style={{ gap: 3 }}>
                                    <CaseResultPill c={c} onOpen={() => setInspected(c.id)} />
                                </span>
                                <span role='cell' className='relative flex justify-end' style={{ gap: 6 }}>
                                    {c.product === 'runtime-security' ? (
                                        <Btn className='text-[12px]' aria-label={`${C.openInPlayground}: ${c.name}`} onClick={() => openInPlayground(c)}>
                                            {C.openInPlayground}
                                        </Btn>
                                    ) : (
                                        <Btn className='text-[12px]' aria-label={`${C.inspect}: ${c.name}`} aria-haspopup='dialog' onClick={() => setInspected(c.id)}>
                                            {C.inspect}
                                        </Btn>
                                    )}
                                    <button
                                        type='button'
                                        aria-label={C.more(c.name)}
                                        aria-expanded={open}
                                        aria-haspopup='menu'
                                        className='grid size-8 min-h-0 shrink-0 place-items-center rounded-[7px] text-[16px]'
                                        style={{ color: 'var(--v2-faint)', background: open ? 'var(--v2-inset)' : 'transparent' }}
                                        onClick={() => setMenu(open ? null : c.id)}
                                    >
                                        ⋯
                                    </button>
                                    {open ? (
                                        <div role='menu' aria-label={C.more(c.name)} className='absolute right-0 top-full z-10 mt-1 flex min-w-44 flex-col rounded-[10px] border p-1 text-[13px]' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', boxShadow: '0 12px 30px rgba(0,0,0,0.18)' }} data-slot='case-menu'>
                                            <button type='button' role='menuitem' className='rounded-[7px] px-3 py-1.5 text-left' aria-label={C.removeCase(c.name)} onClick={() => { setMenu(null); void send({ kind: 'deleteCase', context, id: c.id }) }}>
                                                {C.remove}
                                            </button>
                                            {others.map((t) => (
                                                <button key={t.id} type='button' role='menuitem' className='rounded-[7px] px-3 py-1.5 text-left' aria-label={C.copyCase(c.name, t.label)} onClick={() => { setMenu(null); void send({ kind: 'copyCase', context, id: c.id, tenant: t.id }) }}>
                                                    {`${C.copy} to ${t.label}`}
                                                </button>
                                            ))}
                                        </div>
                                    ) : null}
                                </span>
                            </div>
                        )
                    })}
                </div>
                {library.state === 'loading' ? (
                    <p role='status' className='text-[13px]' style={{ padding: '28px 18px', borderTop: '1px solid var(--v2-rule)', ...faint }}>
                        {C.loading}
                    </p>
                ) : null}
                {library.state === 'ready' && rows.length === 0 ? (
                    <div className='flex flex-wrap items-center text-[13px]' style={{ gap: 12, padding: '28px 18px', borderTop: '1px solid var(--v2-rule)', ...faint }} data-slot='cases-empty'>
                        <span className='flex-grow'>{cases.length === 0 ? C.empty : C.noMatch}</span>
                        {cases.length > 0 ? (
                            <Btn onClick={() => { setQuery(''); setProduct(null) }}>{C.clearFilters}</Btn>
                        ) : (
                            <>
                                <Btn onClick={() => setPacks(true)}>{C.loadPacks}</Btn>
                                <Btn tier='primary' onClick={() => setAuthoring(true)}>{C.newCase}</Btn>
                            </>
                        )}
                    </div>
                ) : null}
                <div className='flex flex-wrap items-center text-[12px]' style={{ gap: 10, padding: '10px 18px', borderTop: '1px solid var(--v2-rule)', ...faint }} data-slot='cases-pages'>
                    <span role='status'>{matching.length === 0 ? '' : C.range(shownPage * PAGE_SIZE + 1, Math.min(matching.length, (shownPage + 1) * PAGE_SIZE), matching.length)}</span>
                    <span className='ml-auto flex items-center' style={{ gap: 8 }}>
                        <Btn tier='tertiary' className='h-7 text-[12px]' disabled={shownPage === 0} onClick={() => setPage(shownPage - 1)}>
                            {C.previous}
                        </Btn>
                        <span>{C.page(shownPage + 1, pages)}</span>
                        <Btn tier='tertiary' className='h-7 text-[12px]' disabled={shownPage >= pages - 1} onClick={() => setPage(shownPage + 1)}>
                            {C.next}
                        </Btn>
                    </span>
                </div>
            </section>
            {detail
                ? createPortal(
                      <Modal label={C.details} onClose={() => setInspected(null)} width={520} padding={30} sheet data-slot='case-details'>
                          <CaseDetails c={detail} tenant={tenant.label} onClose={() => setInspected(null)} />
                      </Modal>,
                      document.body
                  )
                : null}
            {packs && ready
                ? createPortal(
                      <Modal label={C.packs.title} onClose={() => setPacks(false)} width={560} padding={22} data-slot='packs-dialog'>
                          <PacksDialog
                              tenant={tenant.label}
                              fetching={library.fetching}
                              choosing={library.choosing}
                              hasCases={cases.length > 0}
                              onLoad={async (sources) => {
                                  for (const source of sources) await send({ kind: 'importExamples', context, source })
                                  setPacks(false)
                              }}
                              onImport={() => void send({ kind: 'importCases', context })}
                              onExport={() => void send({ kind: 'exportCases', context })}
                              onCancel={() => setPacks(false)}
                              status={
                                  <>
                                      {problems}
                                      {transfer}
                                  </>
                              }
                          />
                      </Modal>,
                      document.body
                  )
                : null}
            {authoring && ready
                ? createPortal(
                      <Modal label={C.newCaseTitle} onClose={() => setAuthoring(false)} width={560} padding={22} data-slot='new-case-dialog'>
                          <CaseForm
                              tenant={tenant.label}
                              initialProduct={product ?? 'runtime-security'}
                              onSave={async (draft) => {
                                  const outcome = await send({ kind: 'saveCase', context, draft })
                                  if (outcome.kind === 'admitted') setAuthoring(false)
                              }}
                              onCancel={() => setAuthoring(false)}
                              status={problems}
                          />
                      </Modal>,
                      document.body
                  )
                : null}
        </section>
    )
})

/** A case's last result as the prototype's pill (colour, glyph and word) with its time, or "Not run". */
function CaseResultPill({ c, onOpen }: { c: CaseView; onOpen: () => void }) {
    if (!c.result) {
        return (
            <span className='inline-flex items-center self-start rounded-full text-[12px] font-semibold' style={{ gap: 4, padding: '2px 8px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }} data-slot='case-not-run'>
                <Glyph name='dot' size={10} />
                {C.notRun}
            </span>
        )
    }
    const t = TONE[c.result.verdict] ?? TONE.unknown!
    return (
        <span className='flex flex-col' style={{ gap: 3 }} data-slot='case-result' data-verdict={c.result.verdict}>
            <button
                type='button'
                aria-label={`${C.verdictWord[c.result.verdict] ?? c.result.verdict}: ${C.showResult(c.name)}`}
                aria-haspopup='dialog'
                onClick={onOpen}
                className='inline-flex min-h-0 items-center self-start rounded-full border text-[12px] font-semibold whitespace-nowrap'
                style={{ gap: 4, height: 28, padding: '0 10px', borderColor: t.fg, background: t.bg, color: t.fg }}
            >
                <Glyph name={t.glyph} size={10} stroke={2.6} />
                {C.verdictWord[c.result.verdict] ?? c.result.verdict}
            </button>
            <span className='text-[12px]' style={faint}>
                {C.resultAt(clockOf(c.result.at))}
            </span>
        </span>
    )
}

/** Inspect, as the prototype's detail sheet: the verdict banner, the case's rows, the foot note. Nothing is dispatched. */
function CaseDetails({ c, tenant, onClose }: { c: CaseView; tenant: string; onClose: () => void }) {
    const t = c.result ? (TONE[c.result.verdict] ?? TONE.unknown!) : null
    const rows: [string, string][] = [
        [C.columns.product, `${PRODUCT_LABEL[c.product]} · ${tenant}`],
        ...(c.pack ? ([[C.pack, c.pack]] as [string, string][]) : []),
        ...(c.prompt ? ([[C.input, c.prompt]] as [string, string][]) : []),
        ...(c.response ? ([[C.responseRow, c.response]] as [string, string][]) : []),
        ...(c.payload && !/^\s*\{\s*\}\s*$/.test(c.payload) ? ([[C.payloadRow, c.payload]] as [string, string][]) : []),
        ...(c.expected ? ([[C.expected, c.expected]] as [string, string][]) : []),
        [C.lastResult, c.result ? `${C.verdictWord[c.result.verdict] ?? c.result.verdict} ${C.resultAt(clockOf(c.result.at))}` : C.notRun],
        ...(c.result?.scanId ? ([[C.scanId, c.result.scanId]] as [string, string][]) : []),
        [C.runsFrom, C.runs[c.product]],
    ]
    return (
        <div className='flex flex-col' style={{ gap: 18 }}>
            <div className='flex items-center' style={{ gap: 10 }}>
                <h2 className='min-w-0 truncate text-[22px] font-bold' style={{ letterSpacing: '-0.01em' }}>
                    {c.name}
                </h2>
                <button type='button' aria-label={C.closeDetails} onClick={onClose} className='ml-auto grid shrink-0 place-items-center rounded-[7px] border' style={{ width: 32, height: 32, minHeight: 0, borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
                    <Glyph name='x' size={12} stroke={2.2} />
                </button>
            </div>
            {c.result && t ? (
                <div className='flex items-center rounded-[12px]' style={{ gap: 12, padding: '14px 16px', background: t.bg, color: t.fg }} data-slot='case-detail-verdict'>
                    <Glyph name={t.glyph} size={22} stroke={2.2} />
                    <span className='text-[18px] font-bold'>{C.verdictWord[c.result.verdict] ?? c.result.verdict}</span>
                    <span className='ml-auto text-[12px]'>{C.resultAt(clockOf(c.result.at))}</span>
                </div>
            ) : null}
            <section className='flex flex-col rounded-[12px] border text-[13px]' style={{ gap: 12, padding: '18px 20px', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                {rows.map(([k, v]) => (
                    <div key={k} className='flex flex-col' style={{ gap: 3 }}>
                        <span className='text-[12px]' style={faint}>
                            {k}
                        </span>
                        <span className='leading-[1.5] break-words whitespace-pre-wrap'>{v}</span>
                    </div>
                ))}
            </section>
            <p className='text-[12px] leading-[1.5]' style={faint}>
                {C.neverRunHere}
            </p>
        </div>
    )
}

/** Load starter packs: the bundled packs and the public examples as checks, loaded together; the operator's own cases move by file. */
function PacksDialog({ tenant, fetching, choosing, hasCases, onLoad, onImport, onExport, onCancel, status }: { tenant: string; fetching: boolean; choosing: boolean; hasCases: boolean; onLoad: (sources: ('bundled' | 'public')[]) => Promise<void>; onImport: () => void; onExport: () => void; onCancel: () => void; status: React.ReactNode }) {
    const [bundled, setBundled] = useState(true)
    const [pub, setPub] = useState(false)
    const [busy, setBusy] = useState(false)
    const P = C.packs
    const option = (on: boolean, toggle: () => void, name: string, note: string, disabled = false) => (
        <button type='button' role='checkbox' aria-checked={on} aria-disabled={disabled || undefined} onClick={() => !disabled && toggle()} className='flex items-center text-left text-[13px]' style={{ gap: 12, padding: '12px 16px', borderTop: '1px solid var(--v2-rule)', color: 'var(--v2-ink)', opacity: disabled ? 0.6 : 1 }}>
            <span className='grid shrink-0 place-items-center rounded-[5px]' style={{ width: 18, height: 18, border: `1.5px solid ${on ? 'var(--v2-acc)' : 'var(--v2-ctl)'}`, background: on ? 'var(--v2-acc)' : 'transparent', color: 'var(--v2-acc-ink)' }}>
                {on ? <Glyph name='check' size={11} stroke={3.2} /> : null}
            </span>
            <span className='flex-grow font-semibold'>{name}</span>
            <span className='text-[12px]' style={faint}>
                {note}
            </span>
        </button>
    )
    return (
        <div className='flex flex-col' style={{ gap: 14 }}>
            <div>
                <h2 className='text-[20px] font-bold' style={{ letterSpacing: '-0.01em' }}>
                    {P.title}
                </h2>
                <p className='text-[13px]' style={{ marginTop: 4, ...soft }}>
                    {P.lead(tenant)}
                </p>
            </div>
            <div className='flex flex-col overflow-hidden rounded-[12px] border' style={{ borderColor: 'var(--v2-rule)', marginTop: -1 }}>
                {option(bundled, () => setBundled(!bundled), P.bundled, P.bundledNote)}
                {option(pub, () => setPub(!pub), P.public, fetching ? 'Fetching…' : P.publicNote, fetching)}
            </div>
            <p className='text-[12px]' style={faint} data-slot='cases-transfer-note'>
                {C.transferNote}
            </p>
            {status}
            <div className='flex flex-wrap items-center' style={{ gap: 8 }}>
                <span className='text-[12px]' style={faint}>
                    {P.transfer}
                </span>
                <Btn tier='tertiary' aria-disabled={choosing} onClick={onImport}>
                    {C.importCases}
                </Btn>
                <Btn tier='tertiary' disabled={!hasCases} onClick={onExport}>
                    {C.exportCases}
                </Btn>
                <span className='ml-auto flex' style={{ gap: 8 }}>
                    <Btn onClick={onCancel}>{P.cancel}</Btn>
                    <Btn
                        tier='primary'
                        className='px-4'
                        disabled={busy || (!bundled && !pub)}
                        onClick={async () => {
                            setBusy(true)
                            try {
                                await onLoad([...(bundled ? (['bundled'] as const) : []), ...(pub ? (['public'] as const) : [])])
                            } finally {
                                setBusy(false)
                            }
                        }}
                    >
                        {P.load}
                    </Btn>
                </span>
            </div>
        </div>
    )
}

type CaseDraft = { product: CaseProduct; name: string; profile: string; prompt: string; response: string; appName: string; expected: string; payload: string }

const LABEL = 'flex flex-col text-[12px] font-semibold'
const labelStyle = { gap: 5, color: 'var(--v2-soft)' }
const control = { borderColor: 'var(--v2-ctl)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }

/** A new case (C12's authoring) in the prototype's dialog: every field is read when Save is pressed; saving runs nothing. */
function CaseForm({ tenant, initialProduct, onSave, onCancel, status }: { tenant: string; initialProduct: CaseProduct; onSave: (draft: CaseDraft) => Promise<void>; onCancel: () => void; status: React.ReactNode }) {
    const ids = useId()
    const form = useRef<HTMLFormElement>(null)
    const [product, setProduct] = useState<CaseProduct>(initialProduct)
    const runtime = product === 'runtime-security'
    const F = C.form
    return (
        <form
            ref={form}
            aria-label={F.title}
            className='flex flex-col'
            style={{ gap: 14 }}
            data-slot='case-form'
            onSubmit={(e) => {
                e.preventDefault()
                const data = new FormData(form.current ?? undefined)
                const read = (n: string) => (data.get(n) as string | null) ?? ''
                void onSave({ product, name: read('name'), profile: runtime ? read('profile') : '', prompt: read('prompt'), response: read('response'), appName: runtime ? read('app') : '', expected: read('expected'), payload: runtime ? '' : read('payload') })
            }}
        >
            <div>
                <h2 className='text-[20px] font-bold' style={{ letterSpacing: '-0.01em' }}>
                    {C.newCaseTitle}
                </h2>
                <p className='text-[13px] font-normal' style={{ marginTop: 4, ...soft }}>
                    {C.newCaseLead(tenant)}
                </p>
            </div>
            <div className='grid' style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 200px), 1fr))', gap: 12 }}>
                <label className={LABEL} style={labelStyle}>
                    {F.product}
                    <select className='h-8 min-h-0 rounded-[7px] border px-2 text-[13px] font-normal' style={control} value={product} onChange={(e) => setProduct(e.target.value as CaseProduct)}>
                        {PRODUCTS.map((p) => (
                            <option key={p} value={p}>
                                {PRODUCT_LABEL[p]}
                            </option>
                        ))}
                    </select>
                </label>
                <label className={LABEL} style={labelStyle} htmlFor={`${ids}-name`}>
                    {F.name}
                    <Field id={`${ids}-name`} name='name' maxLength={128} autoComplete='off' className='font-normal' />
                </label>
            </div>
            {runtime ? (
                <div className='grid' style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 200px), 1fr))', gap: 12 }}>
                    <label className={LABEL} style={labelStyle} htmlFor={`${ids}-profile`}>
                        {F.profile}
                        <Field id={`${ids}-profile`} name='profile' maxLength={256} autoComplete='off' spellCheck={false} className='font-normal' />
                    </label>
                    <label className={LABEL} style={labelStyle} htmlFor={`${ids}-app`}>
                        {F.app}
                        <Field id={`${ids}-app`} name='app' maxLength={256} autoComplete='off' spellCheck={false} className='font-normal' />
                    </label>
                </div>
            ) : null}
            <label className={LABEL} style={labelStyle} htmlFor={`${ids}-prompt`}>
                {F.prompt}
                <textarea id={`${ids}-prompt`} name='prompt' rows={4} spellCheck={false} className='min-h-0 resize-none rounded-[8px] border px-3 py-2.5 text-[13px] font-normal leading-[1.45]' style={control} />
            </label>
            <label className={LABEL} style={labelStyle} htmlFor={`${ids}-response`}>
                {F.response}
                <textarea id={`${ids}-response`} name='response' rows={2} spellCheck={false} className='min-h-0 resize-none rounded-[8px] border px-3 py-2.5 text-[13px] font-normal leading-[1.45]' style={control} />
            </label>
            <label className={LABEL} style={labelStyle} htmlFor={`${ids}-expected`}>
                {F.expected}
                <Field id={`${ids}-expected`} name='expected' maxLength={1024} autoComplete='off' aria-describedby={`${ids}-expected-hint`} className='font-normal' />
                <span id={`${ids}-expected-hint`} className='text-[12px] font-normal' style={faint}>
                    {F.expectedHint}
                </span>
            </label>
            {runtime ? null : (
                <label className={LABEL} style={labelStyle} htmlFor={`${ids}-payload`}>
                    {F.payload}
                    <textarea id={`${ids}-payload`} name='payload' defaultValue='{}' rows={3} spellCheck={false} className='min-h-0 resize-none rounded-[8px] border px-3 py-2.5 font-mono text-[12px] font-normal leading-[1.45]' style={control} />
                </label>
            )}
            <p className='text-[12px]' style={faint}>
                {F.note}
            </p>
            {status}
            <div className='flex justify-end' style={{ gap: 8 }}>
                <Btn onClick={onCancel}>{F.cancel}</Btn>
                <Btn type='submit' tier='primary' className='px-4'>
                    {F.save}
                </Btn>
            </div>
        </form>
    )
}
