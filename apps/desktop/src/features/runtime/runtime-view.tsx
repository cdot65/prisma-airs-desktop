import { type FormEvent, forwardRef, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout, Input, PillBox, cn } from '@ai-security-academy/presentation'
import { AlertTriangle, CheckCircle2, CircleHelp, Eye, Info, ShieldAlert, ShieldX } from 'lucide-react'
import type { Detection, FeatureId, ScanRecordView, ScanResultView, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { PageHeading } from '../../shell/feature-view'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'
import { type ScanInput, VERDICT_TEXT, draftFrom, outcomeText, recordSummary } from './scan-text'
import type { CasePrefill } from '../library/library-view'
import { CASE_PROBLEM_TEXT } from '../library/library-view'
import { OverviewInbox } from './overview-inbox'

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    onNavigate: (id: FeatureId) => void
    /** A Library case or starter to fill the form with (never scanned). */
    prefill?: CasePrefill | null
    /** The library case the prefill came from (C33): its scan names it, so the core records its result. */
    caseId?: string | null
    /** v2 (C31): no page heading (the Administration accordion names the tab). */
    embedded?: boolean
}

const TEXTAREA = cn(
    'rounded-academy-md bg-academy-bg-inset border-academy-rule focus-visible:border-academy-accent-ring w-full border px-3.5 py-2.5',
    'font-body text-academy-ink min-h-28 resize-y text-[14px] leading-snug'
)

/**
 * Runtime Security synchronous scan for the active tenant. The shell keys
 * this view by context, so one instance only ever serves one tenant and
 * credential generation: its form, case name and save status, and any late
 * reply to it, retire with that context. Every field is read when Scan is
 * pressed (nothing is pre-captured), each press is one explicit submission,
 * and nothing is ever resubmitted automatically. Scan records come only
 * from native snapshots, which already exclude other tenants and discard
 * replies for a retired tenant or key.
 */
export const RuntimeView = forwardRef<HTMLHeadingElement, Props>(function RuntimeView({ snapshot, dispatch, onNavigate, prefill = null, caseId = null, embedded = false }, ref) {
    const ids = useId()
    const [tab, setTab] = useState<'scan' | 'overview'>('scan')
    const [caseName, setCaseName] = useState('')
    const [caseMessage, setCaseMessage] = useState<string | null>(null)
    // A save is reported when its outcome is established, not on admission.
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
        setTab('scan')
        setCaseName(prefill.name)
        setCaseMessage(null)
    }, [prefill?.nonce])

    async function saveCase() {
        const element = form.current
        if (!element || !context) return
        const data = new FormData(element)
        const read = (name: string) => (data.get(name) as string | null) ?? ''
        // A Runtime case from the form as it is now (v1 Playground save).
        const outcome = await dispatch({
            kind: 'saveCase',
            context,
            draft: {
                product: 'runtime-security',
                name: caseName,
                profile: read('profile'),
                prompt: read('prompt'),
                response: read('response'),
                appName: read('app-name'),
                expected: '',
                payload: '',
            },
        })
        if (outcome.kind === 'admitted') {
            // Only snapshots published after this admission can report it.
            setSavingCase({ name: caseName, after: BigInt(snapshot.revision) })
            setCaseMessage('Saving to this tenant’s case Library…')
        } else if (outcome.kind === 'refused' && outcome.refusal === 'invalidInput') {
            setCaseMessage('Check the case: it needs a name, a profile and an application within the limits.')
        } else setCaseMessage(outcomeMessage(outcome))
    }

    const notice = snapshot.library.notice
    const problem = snapshot.library.problem
    useEffect(() => {
        if (savingCase === null || BigInt(snapshot.revision) <= savingCase.after) return
        if (notice?.kind === 'saved' && notice.name === savingCase.name) {
            setCaseMessage('Saved to this tenant’s case Library.')
            setSavingCase(null)
        } else if (problem) {
            // The form keeps its fields so the save can be tried again.
            setCaseMessage(CASE_PROBLEM_TEXT[problem])
            setSavingCase(null)
        }
    }, [notice, problem, savingCase, snapshot.revision])

    const shown: ScanRecordView | undefined =
        snapshot.scans.find((s) => s.operation === selected) ?? snapshot.scans[0]
    const inFlight = snapshot.scans.some((s) => s.progress !== 'settled')

    async function submit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const element = form.current
        if (!element || !context || busy) return
        const data = new FormData(element)
        const read = (name: string) => (data.get(name) as string | null) ?? ''
        const input: ScanInput = {
            profileKind: read('profile-kind') === 'id' ? 'id' : 'name',
            profile: read('profile'),
            prompt: read('prompt'),
            response: read('response'),
            appName: read('app-name'),
        }
        const built = draftFrom(input)
        if ('problem' in built) {
            setMessage(built.problem)
            element.querySelector<HTMLElement>(`[data-field='${built.field}']`)?.focus()
            return
        }
        setBusy(true)
        setMessage(null)
        // A case's scan names it: the core records the verdict for the case
        // only if the case still holds what was scanned (C33).
        const outcome = await dispatch(caseId && prefill ? { kind: 'scan', context, draft: built.draft, case: caseId } : { kind: 'scan', context, draft: built.draft })
        setBusy(false)
        if (outcome.kind === 'admitted') setSelected(outcome.operation)
        else if (outcome.kind === 'unknown') {
            setMessage('The desktop did not confirm the scan. Check the history below before scanning again.')
        } else setMessage(outcomeMessage(outcome))
    }

    async function cancel(operation: string) {
        const outcome = await dispatch({ kind: 'cancelScan', operation })
        // Already settled is not a failure: the record shows what happened.
        if (outcome.kind !== 'admitted' && !(outcome.kind === 'refused' && outcome.refusal === 'invalidInput')) {
            setMessage(outcomeMessage(outcome))
        }
    }

    const heading = embedded ? null : <PageHeading ref={ref} id='runtime-heading' kicker='Runtime Security' title='Scan a prompt or response' />

    if (!tenant || !context) {
        return (
            <section aria-labelledby='runtime-heading' data-slot='runtime'>
                {heading}
                <Callout tone='default' icon={Info}>
                    <p>Add a tenant and its scan key in Settings to run scans.</p>
                    <Button type='button' tier='secondary' className='mt-3' onClick={() => onNavigate('settings')}>
                        Open Settings
                    </Button>
                </Callout>
            </section>
        )
    }
    const ready = tenant.scan.state === 'ready'
    const tabs = (
        <div role='tablist' aria-label='Runtime Security sections' className='flex gap-2'>
            {(
                [
                    ['scan', 'Playground'],
                    ['overview', 'Overview & Inbox'],
                ] as const
            ).map(([id, label]) => (
                <Button
                    key={id}
                    type='button'
                    role='tab'
                    aria-selected={tab === id}
                    size='sm'
                    tier={tab === id ? 'secondary' : 'tertiary'}
                    className={tab === id ? 'bg-academy-paper-deep font-semibold' : undefined}
                    onClick={() => setTab(id)}
                >
                    {label}
                </Button>
            ))}
        </div>
    )
    if (tab === 'overview') {
        return (
            <section aria-labelledby='runtime-heading' data-slot='runtime' className='flex flex-col gap-6'>
                {heading}
                {tabs}
                <OverviewInbox
                    context={context}
                    overview={snapshot.overview}
                    ready={tenant.management.state === 'ready'}
                    dispatch={dispatch}
                    onNavigate={onNavigate}
                />
            </section>
        )
    }

    return (
        <section aria-labelledby='runtime-heading' data-slot='runtime' className='flex flex-col gap-6'>
            {heading}
            {tabs}
            <p className='text-academy-ink-soft -mt-4 text-[13px]' data-slot='runtime-tenant'>
                Scanning with {tenant.label} at {tenant.origin}.
            </p>
            {prefill ? (
                <Callout tone='default' icon={Info} data-slot='runtime-loaded-case'>
                    <p>
                        {prefill.name} is loaded. {prefill.expected ? `Goal: ${prefill.expected} ` : ''}Choose Scan to evaluate it; nothing
                        was scanned.
                    </p>
                </Callout>
            ) : null}
            {ready ? null : (
                <Callout tone='warning' icon={AlertTriangle} data-slot='runtime-no-key'>
                    <p>{tenant.label} has no ready scan key. Enter one in Settings before scanning.</p>
                    <Button type='button' tier='secondary' className='mt-3' onClick={() => onNavigate('settings')}>
                        Open Settings
                    </Button>
                </Callout>
            )}
            <form
                key={prefill?.nonce ?? 'empty'}
                ref={form}
                onSubmit={(e) => void submit(e)}
                aria-busy={busy}
                aria-labelledby='runtime-heading'
                className='border-academy-rule flex flex-col gap-4 rounded-[12px] border p-5'
                data-slot='scan-form'
            >
                <fieldset className='flex flex-col gap-2'>
                    <legend className='text-academy-ink mb-1 text-[13px] font-medium'>AI security profile</legend>
                    <div className='flex flex-wrap gap-4'>
                        {(
                            [
                                ['name', 'By name'],
                                ['id', 'By ID'],
                            ] as const
                        ).map(([value, label]) => (
                            <label key={value} className='flex min-h-11 cursor-pointer items-center gap-2 text-[13px]'>
                                <input
                                    type='radio'
                                    name='profile-kind'
                                    value={value}
                                    defaultChecked={value === 'name'}
                                    className='size-4 accent-[var(--academy-accent)]'
                                />
                                {label}
                            </label>
                        ))}
                    </div>
                    <label htmlFor={`${ids}-profile`} className='sr-only'>
                        Profile name or ID
                    </label>
                    <Input
                        id={`${ids}-profile`}
                        name='profile'
                        defaultValue={prefill?.profile ?? ''}
                        data-field='profile'
                        autoComplete='off'
                        spellCheck={false}
                        placeholder='Profile name or ID'
                    />
                </fieldset>
                <div className='flex flex-col gap-2'>
                    <label htmlFor={`${ids}-prompt`} className='text-academy-ink text-[13px] font-medium'>
                        Prompt
                    </label>
                    <textarea id={`${ids}-prompt`} name='prompt' data-field='prompt' className={TEXTAREA} spellCheck={false} defaultValue={prefill?.prompt ?? ''} />
                </div>
                <div className='flex flex-col gap-2'>
                    <label htmlFor={`${ids}-response`} className='text-academy-ink text-[13px] font-medium'>
                        Response
                    </label>
                    <textarea id={`${ids}-response`} name='response' data-field='response' className={TEXTAREA} spellCheck={false} defaultValue={prefill?.response ?? ''} />
                </div>
                <div className='flex flex-col gap-2'>
                    <label htmlFor={`${ids}-app`} className='text-academy-ink text-[13px] font-medium'>
                        Application name (optional)
                    </label>
                    <Input id={`${ids}-app`} name='app-name' data-field='appName' autoComplete='off' spellCheck={false} defaultValue={prefill?.appName ?? ''} />
                </div>
                <p className='text-academy-ink-faint text-[12px]'>
                    Up to 2 MiB each for the prompt and response, 8 MiB per request. Sent only to {tenant.origin}.
                </p>
                <div className='flex flex-wrap items-center gap-2'>
                    <Button type='submit' tier='primary' disabled={!ready || busy || inFlight}>
                        Scan
                    </Button>
                    {inFlight ? <span className='text-academy-ink-soft text-[13px]'>A scan is in progress.</span> : null}
                </div>
                {message ? (
                    <p role='alert' data-slot='scan-message' className='text-academy-accent-warn text-[13px]'>
                        {message}
                    </p>
                ) : null}
                <div className='border-academy-rule flex flex-wrap items-end gap-2 border-t pt-3' data-slot='save-case'>
                    <label className='flex min-w-48 flex-1 flex-col gap-1 text-[13px] font-medium' htmlFor={`${ids}-case`}>
                        Case name
                        <Input id={`${ids}-case`} value={caseName} maxLength={128} onChange={(e) => setCaseName(e.target.value)} autoComplete='off' />
                    </label>
                    <Button type='button' tier='secondary' onClick={() => void saveCase()}>
                        Save this case
                    </Button>
                    {caseMessage ? (
                        <p role='status' className='text-academy-ink-soft w-full text-[13px]' data-slot='save-case-message'>
                            {caseMessage}
                        </p>
                    ) : null}
                </div>
            </form>
            {shown ? <ScanResult record={shown} onCancel={(op) => void cancel(op)} /> : null}
            {snapshot.scans.length > 0 ? (
                <section aria-labelledby={`${ids}-history`} data-slot='scan-history' className='flex flex-col gap-2'>
                    <h2 id={`${ids}-history`} className='text-academy-ink text-[15px] font-semibold'>
                        Recent scans for {tenant.label}
                    </h2>
                    <ul className='flex flex-col gap-1'>
                        {snapshot.scans.map((record, index) => (
                            <li key={record.operation}>
                                <button
                                    type='button'
                                    aria-current={record === shown ? 'true' : undefined}
                                    onClick={() => setSelected(record.operation)}
                                    className={cn(
                                        'rounded-academy-sm flex min-h-11 w-full items-center justify-between gap-3 px-3 py-2 text-left text-[13px]',
                                        record === shown ? 'bg-academy-accent-soft text-academy-ink' : 'text-academy-ink-soft hover:bg-academy-bg-inset'
                                    )}
                                >
                                    <span>{index === 0 ? 'Latest scan' : `Scan ${snapshot.scans.length - index} of ${snapshot.scans.length}`}</span>
                                    <span className='text-right'>{recordSummary(record)}</span>
                                </button>
                            </li>
                        ))}
                    </ul>
                </section>
            ) : null}
        </section>
    )
})

function ScanResult({ record, onCancel }: { record: ScanRecordView; onCancel: (operation: string) => void }) {
    const result = record.result
    const verdict = result ? VERDICT_TEXT[result.verdict] : null
    const settled = record.progress === 'settled'
    return (
        <section aria-label='Scan result' data-slot='scan-result' className='flex flex-col gap-3'>
            <div role='status' aria-live='polite'>
                {!settled ? (
                    <Callout tone='default' icon={Info} data-slot='scan-progress'>
                        <p>{recordSummary(record)}</p>
                        <Button type='button' tier='secondary' className='mt-3' onClick={() => onCancel(record.operation)}>
                            Cancel scan
                        </Button>
                    </Callout>
                ) : result && verdict ? (
                    <Callout
                        tone={verdict.tone}
                        icon={verdict.tone === 'success' ? CheckCircle2 : verdict.tone === 'danger' ? ShieldAlert : AlertTriangle}
                        data-slot='scan-verdict'
                        data-verdict={result.verdict}
                    >
                        <p className='font-semibold'>{verdict.label}</p>
                        {result.verdict === 'unknown' ? (
                            <p className='mt-1'>
                                {result.timeout || result.serviceError || result.contentErrors.length > 0
                                    ? 'The service reported an incomplete scan. Do not treat this as allowed.'
                                    : 'The service returned an action or category this desktop does not recognize. Do not treat this as allowed.'}
                            </p>
                        ) : null}
                    </Callout>
                ) : (
                    <Callout tone={record.outcome === 'unknown' ? 'warning' : 'danger'} icon={AlertTriangle} data-slot='scan-failure'>
                        <p>{recordSummary(record)}</p>
                        {record.failure || record.discarded ? <p className='mt-1'>{outcomeText(record.outcome, record.stop)}</p> : null}
                        {record.outcome === 'unknown' ? (
                            <p className='mt-1'>Press Scan to send it again only if a second scan is acceptable.</p>
                        ) : null}
                    </Callout>
                )}
            </div>
            {result ? (
                <dl className='grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-[13px]' data-slot='scan-fields'>
                    <dt className='text-academy-ink-faint'>Action</dt>
                    <dd className='text-academy-ink font-mono'>{result.action}</dd>
                    <dt className='text-academy-ink-faint'>Category</dt>
                    <dd className='text-academy-ink font-mono'>{result.category}</dd>
                    <dt className='text-academy-ink-faint'>Profile</dt>
                    <dd className='text-academy-ink'>{result.profileName ?? result.profileId ?? 'Not reported'}</dd>
                    <dt className='text-academy-ink-faint'>Scan ID</dt>
                    <dd className='text-academy-ink font-mono break-all'>{result.scanId}</dd>
                    <dt className='text-academy-ink-faint'>Report ID</dt>
                    <dd className='text-academy-ink font-mono break-all'>{result.reportId}</dd>
                    {result.timeout ? (
                        <>
                            <dt className='text-academy-ink-faint'>Incomplete</dt>
                            <dd className='text-academy-ink'>The service reported a timeout.</dd>
                        </>
                    ) : null}
                    {result.serviceError ? (
                        <>
                            <dt className='text-academy-ink-faint'>Incomplete</dt>
                            <dd className='text-academy-ink'>The service reported an error.</dd>
                        </>
                    ) : null}
                    {result.contentErrors.length > 0 ? (
                        <>
                            <dt className='text-academy-ink-faint'>Content errors</dt>
                            <dd className='text-academy-ink' data-slot='scan-content-errors'>
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
            {result ? <Detections title='Prompt detections' items={result.promptDetected.filter((d) => d.detector !== 'topic_violation')} /> : null}
            {result ? <Detections title='Response detections' items={result.responseDetected.filter((d) => d.detector !== 'topic_violation')} /> : null}
            {result ? <TopicVerdict result={result} topics={result.topics} direction='prompt' /> : null}
            {result && (result.responseDetected.length > 0 || result.responseTopics.violation !== null || result.responseTopics.blocked.length > 0 || result.responseTopics.allowed.length > 0) ? (
                <TopicVerdict result={result} topics={result.responseTopics} direction='response' />
            ) : null}
        </section>
    )
}

/**
 * The custom topic result, keyed apart from the detectors (C31, DESIGN-V2
 * §10 gap 2): a topic matched under the scan's profile is named; "Not
 * detected" says no assigned topic matched; an unreported verdict says so.
 * An applied topic never suppresses an enabled detector: both are shown.
 * The prompt's and the response's results are separate, each labelled.
 */
function TopicVerdict({ result, topics, direction }: { result: ScanResultView; topics: ScanResultView['topics']; direction: 'prompt' | 'response' }) {
    const profile = result.profileName ?? result.profileId ?? 'the scan profile'
    const matched = topics.blocked.length > 0 || topics.allowed.length > 0 || topics.violation === true
    return (
        <div
            className='flex flex-col gap-1'
            data-slot='scan-topics'
            data-direction={direction}
            data-topic-verdict={topics.violation === null && !matched ? 'unreported' : matched ? 'flagged' : 'clear'}
        >
            <h3 className='text-academy-ink text-[13px] font-medium'>{direction === 'prompt' ? 'Custom topics · prompt' : 'Custom topics · response'}</h3>
            {matched ? (
                <ul className='flex flex-wrap gap-2'>
                    {topics.blocked.map((t) => (
                        <li key={`${direction}-b-${t}`}>
                            <PillBox tone='danger' data-verdict='blocked'>
                                <ShieldX aria-hidden className='mr-1 inline size-3.5 align-[-2px]' />
                                {t}: flagged (blocked)
                            </PillBox>
                        </li>
                    ))}
                    {topics.allowed.map((t) => (
                        <li key={`${direction}-a-${t}`}>
                            <PillBox tone='warning' data-verdict='monitor'>
                                <Eye aria-hidden className='mr-1 inline size-3.5 align-[-2px]' />
                                {t}: flagged (monitor only)
                            </PillBox>
                        </li>
                    ))}
                    {topics.blocked.length === 0 && topics.allowed.length === 0 ? (
                        <li>
                            <PillBox tone='danger' data-verdict='unnamed'>
                                <AlertTriangle aria-hidden className='mr-1 inline size-3.5 align-[-2px]' />
                                A topic matched (the service named none)
                            </PillBox>
                        </li>
                    ) : null}
                </ul>
            ) : topics.violation === false ? (
                <p className='text-academy-ink-soft text-[12px]' data-verdict='clear'>
                    <CheckCircle2 aria-hidden className='mr-1 inline size-3.5 align-[-2px]' />
                    Not detected: no custom topic assigned to {profile} matched.
                </p>
            ) : (
                <p className='text-academy-ink-soft text-[12px]' data-verdict='unreported'>
                    <CircleHelp aria-hidden className='mr-1 inline size-3.5 align-[-2px]' />
                    Not reported by the service for this scan.
                </p>
            )}
        </div>
    )
}

function Detections({ title, items }: { title: string; items: Detection[] }) {
    if (items.length === 0) return null
    return (
        <div className='flex flex-col gap-1' data-slot='scan-detections'>
            <h3 className='text-academy-ink text-[13px] font-medium'>{title}</h3>
            <ul className='flex flex-wrap gap-2'>
                {items.map((d) => (
                    <li key={d.detector}>
                        <PillBox tone={d.detected ? 'danger' : 'default'}>
                            {d.detector}: {d.detected ? 'detected' : 'not detected'}
                        </PillBox>
                    </li>
                ))}
            </ul>
        </div>
    )
}
