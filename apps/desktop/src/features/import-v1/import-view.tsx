import { forwardRef, useEffect, useId, useRef, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle, Info } from 'lucide-react'
import type {
    Category,
    ImportActivity,
    ItemState,
    ItemStatus,
    PlanItemView,
    Problem,
    ReceiptView,
    ShellSnapshot,
    SourceState,
    TargetChoice,
    V1TenantChoice,
} from '@prisma-airs/desktop-contracts'
import { PageHeading } from '../../shell/feature-view'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
}

export const CATEGORY_TEXT: Record<Category, string> = {
    tenants: 'Tenants (names and TSG IDs)',
    endpoints: 'Scan endpoints',
    settings: 'Appearance',
    cases: 'Saved cases',
    reports: 'FP/FN reports',
    academyDrafts: 'Academy lab drafts',
    apiClient: 'API Client collections',
    conversations: 'Harness conversations',
}

/** Read from v1's Keychain: macOS asks before each is read. */
const PROTECTED: Category[] = ['endpoints', 'cases', 'academyDrafts', 'conversations']

export const PROBLEM_TEXT: Record<Problem, string> = {
    missing: 'v1 has none on this Mac',
    sourceLocked: 'v1 is running; quit it first',
    accessDenied: 'macOS did not allow it to be read',
    keychainLocked: 'the Keychain is locked',
    corrupt: 'it could not be read',
    unsupportedVersion: 'it was saved by a v1 version this import does not know',
    tooLarge: 'it is over its size limit',
    pathRejected: 'it is a link or not an ordinary file',
    sourceRetired: 'v1 was removing it',
    noStoredWork: 'this v1 tenant kept nothing on this Mac (session only)',
    accountMismatch: 'it belongs to another account',
    signInRequired: 'sign in to the same Academy account first',
    endpointNotMappable: 'its endpoint is not a plain HTTPS origin',
    noV2Equivalent: 'v2 has no equivalent',
    destinationFull: 'v2 has no room for it',
    sourceChanged: 'it changed after the review',
    noTarget: 'its v1 tenant is not imported',
    notRecorded: 'its record could not be saved, so nothing more was written',
}

const SOURCE_TEXT: Record<SourceState, string> = {
    notFound: 'No v1 data was found on this Mac.',
    locked: 'v1 is running. Quit Prisma AIRS Desktop (v1), then look again.',
    unavailable: 'v1’s data could not be read.',
    ready: 'v1’s data was found. Nothing has been read from its Keychain.',
}

const ACTIVITY_TEXT: Record<ImportActivity, string> = {
    inspecting: 'Looking at v1’s data…',
    planning: 'Reading the selected items and comparing them with this app…',
    importing: 'Importing…',
    confirming: 'Confirm the rollback in the dialog.',
    rollingBack: 'Rolling back…',
}

const ITEM_STATE_TEXT: Record<ItemState, string> = {
    planned: 'not started',
    writing: 'interrupted while writing',
    applied: 'imported',
    failed: 'not imported',
    reverting: 'rollback not finished',
    reverted: 'rolled back',
    keptChanged: 'kept (changed since)',
    gone: 'already removed',
}

function statusText(status: ItemStatus): string {
    switch (status.kind) {
        case 'ready':
            return 'Ready'
        case 'alreadyPresent':
            return 'Already here'
        case 'conflict':
            return 'Conflict: this app holds something different; nothing is overwritten'
        case 'blocked':
            return `Cannot import: ${PROBLEM_TEXT[status.problem]}`
    }
}

function countsText(item: PlanItemView): string {
    const c = item.counts
    const parts = [`${c.new} new`]
    if (c.present) parts.push(`${c.present} already here`)
    if (c.conflict) parts.push(`${c.conflict} in conflict (kept as they are)`)
    if (c.skipped) parts.push(`${c.skipped} not imported`)
    return parts.join(' · ')
}

/**
 * Reviewed import of v1 work (C23). The desktop reads v1 only; this view
 * shows what it found, sends the operator's explicit selection, and shows
 * each import's receipt with Roll Back (and Import Again for one that was
 * undone). An import shows only once it has finished whole: one that
 * fails, or that the app ended during, is undone. Nothing is imported
 * without a review, and no credential or sign-in is ever carried over.
 */
export const ImportView = forwardRef<HTMLHeadingElement, Props>(function ImportView({ snapshot, dispatch }, ref) {
    const view = snapshot.v1Import
    const [categories, setCategories] = useState<Category[]>([])
    const [targets, setTargets] = useState<Record<number, string>>({})
    const [items, setItems] = useState<number[]>([])
    const [message, setMessage] = useState<string | null>(null)
    const resultRef = useRef<HTMLHeadingElement>(null)
    // Each receipt's Roll back button, and the receipt last asked to roll
    // back: a cancelled confirmation returns focus to its button.
    const rollBackRefs = useRef<Record<string, HTMLButtonElement | null>>({})
    const asked = useRef<string | null>(null)
    const categoriesId = useId()
    const planId = useId()
    const receiptsId = useId()
    const plan = view.plan
    const busy = view.activity !== null
    const previous = useRef<ImportActivity | null>(null)

    useEffect(() => {
        // A new review starts with every ready item chosen.
        if (plan) setItems(plan.items.filter((i) => i.status.kind === 'ready').map((i) => i.index))
    }, [plan?.review])

    useEffect(() => {
        // When an import or rollback settles, its result is where focus goes.
        const was = previous.current
        previous.current = view.activity
        if ((was === 'importing' || was === 'rollingBack') && view.activity === null) resultRef.current?.focus()
        if (was === 'confirming' && view.activity === null && asked.current) {
            rollBackRefs.current[asked.current]?.focus()
        }
    }, [view.activity])

    // Removing a record (Forget, or Remove damaged record) removes the
    // button that had focus: focus moves to the imports instead.
    const records = view.receipts.length + view.damagedReceipts.length
    const recordsBefore = useRef(records)
    useEffect(() => {
        if (records < recordsBefore.current) resultRef.current?.focus()
        recordsBefore.current = records
    }, [records])

    async function run(intent: Parameters<Dispatch>[0]) {
        setMessage(null)
        const outcome = await dispatch(intent)
        setMessage(outcomeMessage(outcome))
    }

    function targetOf(t: V1TenantChoice): TargetChoice {
        const chosen = targets[t.index] ?? (t.matches.length === 1 ? (t.matches[0] ?? 'new') : 'new')
        if (chosen === 'new' || chosen === 'skip') return { kind: chosen }
        return { kind: 'existing', tenant: chosen }
    }

    const heading = <PageHeading ref={ref} id='import-heading' kicker='Import' title='Import from v1' />
    if (!view.available) {
        return (
            <section aria-labelledby='import-heading' data-slot='v1-import'>
                {heading}
                <p className='text-academy-ink-soft text-[14px]'>Importing is not available in this build.</p>
            </section>
        )
    }
    const settled = view.receipts.slice().reverse()
    const open = view.receipts.find((r) => r.state !== 'applied' && r.state !== 'rolledBack')
    return (
        <section aria-labelledby='import-heading' data-slot='v1-import' aria-busy={busy} className='flex flex-col gap-6'>
            {heading}
            <p className='text-academy-ink-soft max-w-prose text-[14px]'>
                Copies your work from Prisma AIRS Desktop (v1) into this app after you review it. v1 and its data are
                never changed. Keys, sign-ins and approvals are never carried over: imported tenants need their own
                keys, Academy drafts need the same account signed in, and imported Harness conversations can be read
                and exported but not continued.
            </p>
            <p role='status' aria-live='polite' data-slot='import-status' className='text-academy-ink-soft text-[13px]'>
                {view.activity ? ACTIVITY_TEXT[view.activity] : view.source ? SOURCE_TEXT[view.source] : ''}
            </p>
            {view.problem ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert'>
                    The last step did not finish: {PROBLEM_TEXT[view.problem]}.
                </Callout>
            ) : null}
            {open ? (
                <Callout tone='default' icon={Info} data-slot='import-interrupted'>
                    An import or its rollback did not finish, and nothing of it is shown. What an import writes
                    (tenants, cases, reports and the rest) can't be changed until it is rolled back. Roll it back
                    below before starting another.
                </Callout>
            ) : null}
            {view.receiptsUnreadable ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='import-records-unreadable'>
                    The import records can't be read, so what an import writes (tenants, cases, reports and the
                    rest) is held and can't be changed. Restore access to the app's data folder, then reopen the
                    app.
                </Callout>
            ) : null}
            {view.damagedReceipts.length > 0 ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='import-records-damaged'>
                    <p>
                        {view.damagedReceipts.length === 1
                            ? 'An import record is damaged in both of its copies'
                            : `${view.damagedReceipts.length} import records are damaged in both of their copies`}
                        , so what an import writes (tenants, cases, reports and the rest) is held and can't be
                        changed. Removing a damaged record releases it: whatever that import wrote stays as it is
                        and can no longer be rolled back.
                    </p>
                    <div className='mt-2 flex flex-wrap gap-2'>
                        {view.damagedReceipts.map((id) => (
                            <Button
                                key={id}
                                type='button'
                                tier='secondary'
                                disabled={busy}
                                onClick={() => void run({ kind: 'forgetV1ImportReceipt', receipt: id })}
                            >
                                Remove damaged record {id}
                            </Button>
                        ))}
                    </div>
                </Callout>
            ) : null}
            {!plan ? (
                <div className='flex flex-col gap-4'>
                    <div>
                        <Button type='button' tier='secondary' disabled={busy} onClick={() => void run({ kind: 'inspectV1Import' })}>
                            {view.source ? 'Look again' : 'Look for v1 data'}
                        </Button>
                    </div>
                    {view.source === 'ready' ? (
                        <form
                            aria-labelledby={categoriesId}
                            className='flex flex-col gap-4'
                            onSubmit={(event) => {
                                event.preventDefault()
                                void run({
                                    kind: 'planV1Import',
                                    categories,
                                    targets: view.tenants.map((t) => ({ source: t.index, target: targetOf(t) })),
                                })
                            }}
                        >
                            <fieldset className='flex flex-col gap-1'>
                                <legend id={categoriesId} className='text-academy-ink text-[14px] font-semibold'>
                                    What to import
                                </legend>
                                {view.categories.map((c) => (
                                    <label key={c.category} className='flex min-h-11 items-start gap-3 py-1' data-slot='import-category'>
                                        <input
                                            type='checkbox'
                                            className='mt-1 size-4 accent-[var(--academy-accent)]'
                                            disabled={busy || (c.problem !== null && c.problem !== 'missing')}
                                            checked={categories.includes(c.category)}
                                            onChange={(e) =>
                                                setCategories((current) =>
                                                    e.target.checked ? [...current, c.category] : current.filter((x) => x !== c.category)
                                                )
                                            }
                                        />
                                        <span className='flex flex-col'>
                                            <span className='text-academy-ink text-[13px] font-medium'>{CATEGORY_TEXT[c.category]}</span>
                                            <span className='text-academy-ink-faint text-[12px]'>
                                                {c.problem
                                                    ? PROBLEM_TEXT[c.problem]
                                                    : c.found === null
                                                      ? 'Read from v1’s Keychain only if selected'
                                                      : `${c.found} found`}
                                            </span>
                                        </span>
                                    </label>
                                ))}
                                {view.notMapped.length > 0 ? (
                                    <p className='text-academy-ink-faint text-[12px]' data-slot='import-not-mapped'>
                                        Not imported (no equivalent here): {view.notMapped.join(', ')}.
                                    </p>
                                ) : null}
                            </fieldset>
                            {view.tenants.length > 0 ? (
                                <fieldset className='flex flex-col gap-2'>
                                    <legend className='text-academy-ink text-[14px] font-semibold'>Where each v1 tenant goes</legend>
                                    {view.tenants.map((t) => {
                                        const id = `import-target-${t.index}`
                                        const value = targets[t.index] ?? (t.matches.length === 1 ? (t.matches[0] ?? 'new') : 'new')
                                        return (
                                            <div key={t.index} className='flex flex-wrap items-center gap-3' data-slot='import-tenant'>
                                                <label htmlFor={id} className='text-academy-ink min-w-48 text-[13px]'>
                                                    {t.label}
                                                    {t.tsgId ? ` (TSG ${t.tsgId})` : ''}
                                                    {t.sessionOnly ? ' · session only in v1' : ''}
                                                </label>
                                                <select
                                                    id={id}
                                                    className='border-academy-rule min-h-9 rounded border px-2 text-[13px]'
                                                    value={value}
                                                    disabled={busy}
                                                    onChange={(e) => setTargets((cur) => ({ ...cur, [t.index]: e.target.value }))}
                                                >
                                                    <option value='new'>A new tenant (keys needed)</option>
                                                    {t.matches.map((m) => (
                                                        <option key={m} value={m}>
                                                            {snapshot.tenants.find((x) => x.id === m)?.label ?? m} (same TSG)
                                                        </option>
                                                    ))}
                                                    <option value='skip'>Skip this tenant</option>
                                                </select>
                                            </div>
                                        )
                                    })}
                                </fieldset>
                            ) : null}
                            {categories.some((c) => PROTECTED.includes(c)) ? (
                                <Callout tone='default' icon={Info} data-slot='import-keychain-note'>
                                    macOS will ask before each selected item is read from v1’s Keychain. Choose Allow, not
                                    Always Allow, so v1’s items stay exactly as they are.
                                </Callout>
                            ) : null}
                            <div>
                                <Button type='submit' tier='primary' disabled={busy || categories.length === 0 || open !== undefined}>
                                    Review what would be imported
                                </Button>
                            </div>
                        </form>
                    ) : null}
                </div>
            ) : (
                <form
                    aria-labelledby={planId}
                    data-slot='import-plan'
                    className='flex flex-col gap-3'
                    onSubmit={(event) => {
                        event.preventDefault()
                        void run({ kind: 'confirmV1Import', review: plan.review, items })
                    }}
                >
                    <h2 id={planId} className='text-academy-ink text-[16px] font-semibold'>
                        Review
                    </h2>
                    <ul className='flex flex-col gap-1'>
                        {plan.items.map((item) => (
                            <li key={item.index} data-slot='import-item'>
                                <label className='flex min-h-11 items-start gap-3 py-1'>
                                    <input
                                        type='checkbox'
                                        className='mt-1 size-4 accent-[var(--academy-accent)]'
                                        disabled={busy || item.status.kind !== 'ready'}
                                        checked={items.includes(item.index)}
                                        onChange={(e) =>
                                            setItems((cur) => (e.target.checked ? [...cur, item.index] : cur.filter((x) => x !== item.index)))
                                        }
                                    />
                                    <span className='flex flex-col'>
                                        <span className='text-academy-ink text-[13px] font-medium'>
                                            {CATEGORY_TEXT[item.category]}: {item.label}
                                            {item.target ? ` → ${item.target}` : ''}
                                        </span>
                                        <span className='text-academy-ink-faint text-[12px]'>
                                            {statusText(item.status)} · {countsText(item)}
                                        </span>
                                    </span>
                                </label>
                            </li>
                        ))}
                    </ul>
                    <div className='flex flex-wrap gap-2'>
                        <Button type='submit' tier='primary' disabled={busy || items.length === 0}>
                            Import {items.length} item{items.length === 1 ? '' : 's'}
                        </Button>
                        <Button type='button' tier='tertiary' disabled={busy} onClick={() => void run({ kind: 'dismissV1Import' })}>
                            Cancel
                        </Button>
                    </div>
                </form>
            )}
            <section aria-labelledby={receiptsId} data-slot='import-receipts' className='flex flex-col gap-3'>
                <h2 id={receiptsId} ref={resultRef} tabIndex={-1} className='text-academy-ink text-[16px] font-semibold outline-none'>
                    Imports
                </h2>
                {settled.length === 0 ? (
                    <p className='text-academy-ink-soft text-[13px]'>Nothing has been imported.</p>
                ) : (
                    <ul className='flex flex-col gap-3'>
                        {settled.map((r) => (
                            <ReceiptRow
                                key={r.id}
                                receipt={r}
                                busy={busy}
                                run={run}
                                rollBackRef={(el) => {
                                    rollBackRefs.current[r.id] = el
                                }}
                                onRollBack={() => {
                                    asked.current = r.id
                                }}
                            />
                        ))}
                    </ul>
                )}
            </section>
            {message ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {message}
                </p>
            ) : null}
        </section>
    )
})

function ReceiptRow({
    receipt,
    busy,
    run,
    rollBackRef,
    onRollBack,
}: {
    receipt: ReceiptView
    busy: boolean
    run: (i: Parameters<Dispatch>[0]) => Promise<void>
    rollBackRef: (el: HTMLButtonElement | null) => void
    onRollBack: () => void
}) {
    const when = new Date(receipt.created * 1000).toLocaleString()
    const state =
        receipt.state === 'applied'
            ? 'Imported'
            : receipt.state === 'rolledBack'
              ? receipt.interrupted
                  ? 'Did not finish, so it was undone'
                  : 'Rolled back'
              : receipt.state === 'rollingBack'
                ? 'Rollback not finished'
                : 'Did not finish'
    return (
        <li data-slot='import-receipt' className='border-academy-rule flex flex-col gap-2 border-t pt-2'>
            <p className='text-academy-ink text-[13px] font-medium'>
                {state} · {when}
            </p>
            <ul className='text-academy-ink-soft flex flex-col text-[12px]'>
                {receipt.items.map((i, n) => (
                    <li key={n}>
                        {CATEGORY_TEXT[i.category]}: {i.label} · {ITEM_STATE_TEXT[i.state]}
                        {i.written ? ` · ${i.written} entr${i.written === 1 ? 'y' : 'ies'}` : ''}
                        {i.kept ? ` · ${i.kept} kept` : ''}
                    </li>
                ))}
            </ul>
            <div className='flex flex-wrap gap-2'>
                {receipt.canResume ? (
                    <Button type='button' tier='secondary' disabled={busy} onClick={() => void run({ kind: 'resumeV1Import', receipt: receipt.id })}>
                        Import again
                    </Button>
                ) : null}
                {receipt.canRollBack ? (
                    <Button
                        ref={rollBackRef}
                        type='button'
                        tier='tertiary'
                        disabled={busy}
                        onClick={() => {
                            onRollBack()
                            void run({ kind: 'rollBackV1Import', receipt: receipt.id })
                        }}
                    >
                        Roll back…
                    </Button>
                ) : null}
                {receipt.state === 'applied' || receipt.state === 'rolledBack' ? (
                    <Button
                        type='button'
                        tier='tertiary'
                        disabled={busy}
                        onClick={() => void run({ kind: 'forgetV1ImportReceipt', receipt: receipt.id })}
                    >
                        Forget this record
                    </Button>
                ) : null}
            </div>
        </li>
    )
}
