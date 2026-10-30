import { type FormEvent, useEffect, useId, useRef, useState } from 'react'
import type { Detection, FeatureId, ScanRecordView, ScanResultView, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { type ScanInput, VERDICT_TEXT, draftFrom, outcomeText, recordSummary } from '../../features/runtime/scan-text'
import { type Dispatch, outcomeMessage } from '../../features/tenants/dispatch'
import { Btn, Field } from '../controls'
import { Glyph, type GlyphName } from '../icons'
import { ADMIN_COPY as C } from './copy'

/** A Library case or starter to fill the form with (never scanned); a new nonce remounts the form. */
export interface CasePrefill {
    name: string
    profile: string
    prompt: string
    response: string
    appName: string
    expected: string
    nonce: number
}

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    onNavigate: (id: FeatureId) => void
    prefill: CasePrefill | null
    /** The library case the prefill came from (C33): its scan names it, so the core records its result. */
    caseId: string | null
}

const P = C.playground
const textarea = 'min-h-0 w-full resize-y rounded-[8px] border px-3 py-2.5 text-[14px] leading-[1.45]'
const textareaStyle = { borderColor: 'var(--v2-ctl)', background: 'var(--v2-bg)', color: 'var(--v2-ink)', minHeight: 72 }

/**
 * Runtime Security's Playground as R2App draws it (C39): the scan form at
 * the left (profile, prompt, response, application, Scan, Save as test
 * case), the verdict card at the right, the recent scans beneath. The C19
 * behaviour is unchanged: every field is read when Scan is pressed, each
 * press is one explicit submission, nothing is resubmitted, and scan records
 * come only from native snapshots.
 */
export function Playground({ snapshot, dispatch, onNavigate, prefill, caseId }: Props) {
    const ids = useId()
    const [caseName, setCaseName] = useState('')
    const [caseMessage, setCaseMessage] = useState<string | null>(null)
    const [savingCase, setSavingCase] = useState<{ name: string; after: bigint } | null>(null)
    const form = useRef<HTMLFormElement>(null)
    const [busy, setBusy] = useState(false)
    const [message, setMessage] = useState<string | null>(null)
    const [selected, setSelected] = useState<string | null>(null)
    const active = snapshot.active
    const tenant = snapshot.tenants.find((t) => t.id === active?.tenant)
    const context = active?.context ?? null

    useEffect(() => {
        if (!prefill) return
        setCaseName(prefill.name)
        setCaseMessage(null)
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once per handed case
    }, [prefill?.nonce])

    async function saveCase() {
        const element = form.current
        if (!element || !context) return
        const data = new FormData(element)
        const read = (name: string) => (data.get(name) as string | null) ?? ''
        const outcome = await dispatch({
            kind: 'saveCase',
            context,
            draft: { product: 'runtime-security', name: caseName, profile: read('profile'), prompt: read('prompt'), response: read('response'), appName: read('app-name'), expected: '', payload: '' },
        })
        if (outcome.kind === 'admitted') {
            setSavingCase({ name: caseName, after: BigInt(snapshot.revision) })
            setCaseMessage(P.saving)
        } else if (outcome.kind === 'refused' && outcome.refusal === 'invalidInput') setCaseMessage(P.invalidCase)
        else setCaseMessage(outcomeMessage(outcome))
    }

    const notice = snapshot.library.notice
    const problem = snapshot.library.problem
    useEffect(() => {
        if (savingCase === null || BigInt(snapshot.revision) <= savingCase.after) return
        if (notice?.kind === 'saved' && notice.name === savingCase.name) {
            setCaseMessage(P.saved)
            setSavingCase(null)
        } else if (problem) {
            setCaseMessage(P.caseProblem[problem] ?? problem)
            setSavingCase(null)
        }
    }, [notice, problem, savingCase, snapshot.revision])

    const shown: ScanRecordView | undefined = snapshot.scans.find((s) => s.operation === selected) ?? snapshot.scans[0]
    const inFlight = snapshot.scans.some((s) => s.progress !== 'settled')

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const element = form.current
        if (!element || !context || busy) return
        const data = new FormData(element)
        const read = (name: string) => (data.get(name) as string | null) ?? ''
        const input: ScanInput = { profileKind: read('profile-kind') === 'id' ? 'id' : 'name', profile: read('profile'), prompt: read('prompt'), response: read('response'), appName: read('app-name') }
        const built = draftFrom(input)
        if ('problem' in built) {
            setMessage(built.problem)
            element.querySelector<HTMLElement>(`[data-field='${built.field}']`)?.focus()
            return
        }
        setBusy(true)
        setMessage(null)
        const outcome = await dispatch(caseId && prefill ? { kind: 'scan', context, draft: built.draft, case: caseId } : { kind: 'scan', context, draft: built.draft })
        setBusy(false)
        if (outcome.kind === 'admitted') setSelected(outcome.operation)
        else if (outcome.kind === 'unknown') setMessage(P.unconfirmed)
        else setMessage(outcomeMessage(outcome))
    }

    async function cancel(operation: string) {
        const outcome = await dispatch({ kind: 'cancelScan', operation })
        if (outcome.kind !== 'admitted' && !(outcome.kind === 'refused' && outcome.refusal === 'invalidInput')) setMessage(outcomeMessage(outcome))
    }

    if (!tenant || !context) return null
    const ready = tenant.scan.state === 'ready'
    const faint = { color: 'var(--v2-faint)' }
    const soft = { color: 'var(--v2-soft)' }
    return (
        <div className='flex flex-col' style={{ gap: 18 }} data-slot='runtime'>
            <div className='grid' style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 340px), 1fr))', gap: 20 }} data-slot='playground-columns'>
                <form key={prefill?.nonce ?? 'empty'} ref={form} onSubmit={(e) => void submit(e)} aria-busy={busy} aria-label='Scan a prompt or response' className='flex min-w-0 flex-col' style={{ gap: 10 }} data-slot='scan-form'>
                    {prefill ? (
                        <p className='text-[12px]' style={faint} data-slot='runtime-loaded-case'>
                            {P.loaded(prefill.name, prefill.expected)}
                        </p>
                    ) : null}
                    <fieldset className='flex flex-col' style={{ gap: 6 }}>
                        <legend className='text-[12px] font-medium' style={soft}>
                            {P.profileKind}
                        </legend>
                        <div className='flex flex-wrap' style={{ gap: 14 }}>
                            {(
                                [
                                    ['name', P.byName],
                                    ['id', P.byId],
                                ] as const
                            ).map(([value, label]) => (
                                <label key={value} className='flex min-h-8 cursor-pointer items-center text-[13px]' style={{ gap: 6 }}>
                                    <input type='radio' name='profile-kind' value={value} defaultChecked={value === 'name'} className='size-4 min-h-0' />
                                    {label}
                                </label>
                            ))}
                        </div>
                        <label htmlFor={`${ids}-profile`} className='sr-only'>
                            {P.profile}
                        </label>
                        <Field id={`${ids}-profile`} name='profile' defaultValue={prefill?.profile ?? ''} data-field='profile' autoComplete='off' spellCheck={false} placeholder={P.profile} />
                    </fieldset>
                    <label className='flex flex-col text-[12px]' style={{ gap: 4, ...soft }}>
                        {P.prompt}
                        <textarea name='prompt' data-field='prompt' rows={3} className={textarea} style={textareaStyle} spellCheck={false} defaultValue={prefill?.prompt ?? ''} />
                    </label>
                    <label className='flex flex-col text-[12px]' style={{ gap: 4, ...soft }}>
                        {P.response}
                        <textarea name='response' data-field='response' rows={3} className={textarea} style={textareaStyle} spellCheck={false} defaultValue={prefill?.response ?? ''} />
                    </label>
                    <label className='flex flex-col text-[12px]' style={{ gap: 4, ...soft }}>
                        {P.app}
                        <Field name='app-name' data-field='appName' autoComplete='off' spellCheck={false} defaultValue={prefill?.appName ?? ''} />
                    </label>
                    <p className='text-[12px]' style={faint}>
                        {P.limits(tenant.origin)}
                    </p>
                    <div className='flex flex-wrap items-center' style={{ gap: 8 }}>
                        {ready ? (
                            <Btn type='submit' tier='primary' className='px-[18px]' disabled={busy || inFlight}>
                                {P.scan}
                            </Btn>
                        ) : (
                            <>
                                <span className='flex items-center text-[12px]' style={{ gap: 8, ...soft }} data-slot='runtime-no-key'>
                                    <Glyph name='alert' size={14} color='var(--v2-flag)' className='shrink-0' />
                                    {P.noKey(tenant.label)}
                                </span>
                                <Btn tier='primary' onClick={() => onNavigate('settings')}>
                                    {P.addCredentials}
                                </Btn>
                            </>
                        )}
                        {inFlight ? (
                            <span className='text-[13px]' style={soft}>
                                {P.inFlight}
                            </span>
                        ) : null}
                    </div>
                    {message ? (
                        <p role='alert' data-slot='scan-message' className='text-[13px]' style={{ color: 'var(--v2-flag)' }}>
                            {message}
                        </p>
                    ) : null}
                    <div className='flex flex-wrap items-end' style={{ gap: 8, paddingTop: 10, borderTop: '1px solid var(--v2-rule)' }} data-slot='save-case'>
                        <label className='flex min-w-48 flex-1 flex-col text-[12px]' style={{ gap: 4, ...soft }} htmlFor={`${ids}-case`}>
                            {P.caseName}
                            <Field id={`${ids}-case`} value={caseName} maxLength={128} onChange={(e) => setCaseName(e.target.value)} autoComplete='off' />
                        </label>
                        <Btn onClick={() => void saveCase()}>{P.saveCase}</Btn>
                        {caseMessage ? (
                            <p role='status' className='w-full text-[13px]' style={soft} data-slot='save-case-message'>
                                {caseMessage}
                            </p>
                        ) : null}
                    </div>
                </form>
                <div className='min-w-0'>
                    {shown ? (
                        <ScanResult record={shown} onCancel={(op) => void cancel(op)} />
                    ) : (
                        <div className='rounded-[12px] border border-dashed text-center text-[13px]' style={{ padding: 22, borderColor: 'var(--v2-rule)', color: 'var(--v2-faint)' }} data-slot='scan-not-yet'>
                            {P.notScanned}
                        </div>
                    )}
                </div>
            </div>
            {snapshot.scans.length > 0 ? (
                <section aria-labelledby={`${ids}-history`} data-slot='scan-history' className='flex flex-col' style={{ gap: 6 }}>
                    <h3 id={`${ids}-history`} className='text-[13px] font-semibold'>
                        {P.recent(tenant.label)}
                    </h3>
                    <ul className='flex flex-col' style={{ gap: 2 }}>
                        {snapshot.scans.map((record, index) => (
                            <li key={record.operation}>
                                <button
                                    type='button'
                                    aria-current={record === shown ? 'true' : undefined}
                                    onClick={() => setSelected(record.operation)}
                                    className='flex min-h-8 w-full items-center justify-between rounded-[7px] px-3 py-1.5 text-left text-[13px]'
                                    style={{ gap: 12, background: record === shown ? 'var(--v2-acc-soft)' : 'transparent', color: record === shown ? 'var(--v2-ink)' : 'var(--v2-soft)' }}
                                >
                                    <span>{index === 0 ? P.latest : P.nth(snapshot.scans.length - index, snapshot.scans.length)}</span>
                                    <span className='text-right'>{recordSummary(record)}</span>
                                </button>
                            </li>
                        ))}
                    </ul>
                </section>
            ) : null}
        </div>
    )
}

const TONE: Record<'success' | 'danger' | 'warning' | 'default', { bg: string; fg: string; glyph: GlyphName }> = {
    success: { bg: 'var(--v2-ok-bg)', fg: 'var(--v2-ok)', glyph: 'check' },
    danger: { bg: 'var(--v2-block-bg)', fg: 'var(--v2-block)', glyph: 'ban' },
    warning: { bg: 'var(--v2-flag-bg)', fg: 'var(--v2-flag)', glyph: 'alert' },
    default: { bg: 'var(--v2-inset)', fg: 'var(--v2-soft)', glyph: 'info' },
}

function ScanResult({ record, onCancel }: { record: ScanRecordView; onCancel: (operation: string) => void }) {
    const result = record.result
    const verdict = result ? VERDICT_TEXT[result.verdict] : null
    const settled = record.progress === 'settled'
    const tone = !settled ? TONE.default : result && verdict ? TONE[verdict.tone] : record.outcome === 'unknown' ? TONE.warning : TONE.danger
    const word = !settled ? recordSummary(record) : result && verdict ? verdict.label : recordSummary(record)
    const reason = !settled
        ? ''
        : result?.verdict === 'unknown'
          ? result.timeout || result.serviceError || result.contentErrors.length > 0
              ? P.unknownIncomplete
              : P.unknownUnrecognized
          : result
            ? `${result.action} · ${result.category}`
            : record.failure || record.discarded
              ? outcomeText(record.outcome, record.stop)
              : ''
    return (
        <section aria-label='Scan result' data-slot='scan-result' className='flex flex-col' style={{ gap: 12 }}>
            <div role='status' aria-live='polite' aria-atomic='true' className='overflow-hidden rounded-[12px] border' style={{ borderColor: 'var(--v2-rule)' }}>
                <div className='flex items-center' style={{ gap: 12, padding: '18px 20px', background: tone.bg, color: tone.fg }} data-slot={!settled ? 'scan-progress' : result ? 'scan-verdict' : 'scan-failure'} data-verdict={result?.verdict}>
                    <Glyph name={tone.glyph} size={28} />
                    <div className='flex min-w-0 flex-col'>
                        <span className='text-[20px] font-bold'>{word}</span>
                        {reason ? <span className='text-[12.5px] font-semibold'>{reason}</span> : null}
                    </div>
                    {result ? (
                        <span className='ml-auto shrink-0 text-[12px]'>{result.profileName ?? result.profileId ?? ''}</span>
                    ) : !settled ? (
                        <Btn className='ml-auto' onClick={() => onCancel(record.operation)}>
                            {P.cancel}
                        </Btn>
                    ) : null}
                </div>
                {settled && !result && record.outcome === 'unknown' ? (
                    <p className='text-[13px]' style={{ padding: '12px 16px', color: 'var(--v2-soft)' }}>
                        {P.resend}
                    </p>
                ) : null}
                {result ? (
                    <dl className='grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-[12.5px]' style={{ padding: '12px 16px', color: 'var(--v2-soft)' }} data-slot='scan-fields'>
                        <dt>Action</dt>
                        <dd className='font-mono' style={{ color: 'var(--v2-ink)' }}>
                            {result.action}
                        </dd>
                        <dt>Category</dt>
                        <dd className='font-mono' style={{ color: 'var(--v2-ink)' }}>
                            {result.category}
                        </dd>
                        <dt>Profile</dt>
                        <dd style={{ color: 'var(--v2-ink)' }}>{result.profileName ?? result.profileId ?? 'Not reported'}</dd>
                        <dt>Scan ID</dt>
                        <dd className='font-mono break-all' style={{ color: 'var(--v2-ink)' }}>
                            {result.scanId}
                        </dd>
                        <dt>Report ID</dt>
                        <dd className='font-mono break-all' style={{ color: 'var(--v2-ink)' }}>
                            {result.reportId}
                        </dd>
                        {result.timeout ? (
                            <>
                                <dt>Incomplete</dt>
                                <dd>The service reported a timeout.</dd>
                            </>
                        ) : null}
                        {result.serviceError ? (
                            <>
                                <dt>Incomplete</dt>
                                <dd>The service reported an error.</dd>
                            </>
                        ) : null}
                        {result.contentErrors.length > 0 ? (
                            <>
                                <dt>Content errors</dt>
                                <dd data-slot='scan-content-errors'>
                                    <ul>
                                        {result.contentErrors.map((e, i) => (
                                            <li key={i}>{[e.contentType, e.feature, e.status].filter(Boolean).join(' · ') || 'unspecified'}</li>
                                        ))}
                                    </ul>
                                </dd>
                            </>
                        ) : null}
                    </dl>
                ) : null}
            </div>
            {result ? <Detections title='Prompt detections' items={result.promptDetected.filter((d) => d.detector !== 'topic_violation')} /> : null}
            {result ? <Detections title='Response detections' items={result.responseDetected.filter((d) => d.detector !== 'topic_violation')} /> : null}
            {result ? <TopicVerdict result={result} topics={result.topics} direction='prompt' /> : null}
            {result && (result.responseDetected.length > 0 || result.responseTopics.violation !== null || result.responseTopics.blocked.length > 0 || result.responseTopics.allowed.length > 0) ? (
                <TopicVerdict result={result} topics={result.responseTopics} direction='response' />
            ) : null}
        </section>
    )
}

function Pill({ tone, glyph, children, ...rest }: { tone: 'danger' | 'warning' | 'default'; glyph?: GlyphName; children: React.ReactNode; 'data-verdict'?: string }) {
    const t = TONE[tone]
    return (
        <span className='inline-flex items-center rounded-full text-[12px] font-semibold' style={{ gap: 4, padding: '2px 8px', background: t.bg, color: t.fg }} {...rest}>
            {glyph ? <Glyph name={glyph} size={10} /> : null}
            {children}
        </span>
    )
}

/**
 * The custom topic result, keyed apart from the detectors (C31, DESIGN-V2
 * §10 gap 2): a topic matched under the scan's profile is named; "Not
 * detected" says no assigned topic matched; an unreported verdict says so.
 * An applied topic never suppresses an enabled detector: both are shown.
 */
function TopicVerdict({ result, topics, direction }: { result: ScanResultView; topics: ScanResultView['topics']; direction: 'prompt' | 'response' }) {
    const profile = result.profileName ?? result.profileId ?? 'the scan profile'
    const matched = topics.blocked.length > 0 || topics.allowed.length > 0 || topics.violation === true
    return (
        <div className='flex flex-col' style={{ gap: 4 }} data-slot='scan-topics' data-direction={direction} data-topic-verdict={topics.violation === null && !matched ? 'unreported' : matched ? 'flagged' : 'clear'}>
            <h3 className='text-[13px] font-medium'>{direction === 'prompt' ? 'Custom topics · prompt' : 'Custom topics · response'}</h3>
            {matched ? (
                <ul className='flex flex-wrap' style={{ gap: 8 }}>
                    {topics.blocked.map((t) => (
                        <li key={`${direction}-b-${t}`}>
                            <Pill tone='danger' glyph='ban' data-verdict='blocked'>
                                {t}: flagged (blocked)
                            </Pill>
                        </li>
                    ))}
                    {topics.allowed.map((t) => (
                        <li key={`${direction}-a-${t}`}>
                            <Pill tone='warning' glyph='eye' data-verdict='monitor'>
                                {t}: flagged (monitor only)
                            </Pill>
                        </li>
                    ))}
                    {topics.blocked.length === 0 && topics.allowed.length === 0 ? (
                        <li>
                            <Pill tone='danger' glyph='alert' data-verdict='unnamed'>
                                A topic matched (the service named none)
                            </Pill>
                        </li>
                    ) : null}
                </ul>
            ) : topics.violation === false ? (
                <p className='flex items-center text-[12px]' style={{ gap: 4, color: 'var(--v2-soft)' }} data-verdict='clear'>
                    <Glyph name='check' size={12} />
                    Not detected: no custom topic assigned to {profile} matched.
                </p>
            ) : (
                <p className='flex items-center text-[12px]' style={{ gap: 4, color: 'var(--v2-soft)' }} data-verdict='unreported'>
                    <Glyph name='info' size={12} />
                    Not reported by the service for this scan.
                </p>
            )}
        </div>
    )
}

function Detections({ title, items }: { title: string; items: Detection[] }) {
    if (items.length === 0) return null
    return (
        <div className='flex flex-col' style={{ gap: 4 }} data-slot='scan-detections'>
            <h3 className='text-[13px] font-medium'>{title}</h3>
            <ul className='flex flex-wrap' style={{ gap: 8 }}>
                {items.map((d) => (
                    <li key={d.detector}>
                        <Pill tone={d.detected ? 'danger' : 'default'}>
                            {d.detector}: {d.detected ? 'detected' : 'not detected'}
                        </Pill>
                    </li>
                ))}
            </ul>
        </div>
    )
}
