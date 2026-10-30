import { type ReactNode, useEffect, useRef, useState } from 'react'
import type { ChangeKind, ChangeResultView, ContextRef, GatewayResource, ManagementRequest, ManagementResource, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { MANAGEMENT_FAILURE_TEXT, RESOURCES, itemName, readManagement, type ObserveManagement } from '../../features/administration/management-read'
import { CHANGE_NOUN, type ChangeTarget, ChangeWorkbench, TEMPLATES, pretty, resultText } from '../../features/administration/runtime-changes'
import { type Dispatch, outcomeMessage } from '../../features/tenants/dispatch'
import { clockOf } from '../copy'
import { Modal } from '../modal'
import { useShellStore } from '../store'
import { Verdict } from '../verdict'
import { Glyph } from '../icons'
import { ADMIN_COPY as C } from './copy'
import { RotationPanel } from './rotation-panel'

export interface ReviewSheetProps {
    snapshot: ShellSnapshot
    context: ContextRef
    dispatch: Dispatch
    observe: ObserveManagement
    /** The change to review first (none when opening a rotation in progress). */
    change: ChangeTarget | null
    /** The application whose rotation this sheet follows (a key version, or "Finish rotation…"). */
    rotation: string | null
    /** A Gateway change keeps its edited text on this Mac as a draft (C17). */
    onDraft?: (body: string) => void
    note?: ReactNode
    onClose: () => void
}

const matches = (r: ChangeResultView, c: ChangeTarget) => r.kind === c.kind && r.action === c.action && r.target === c.target

const RUNTIME_RESOURCE: Partial<Record<ChangeKind, ManagementResource>> = {
    profiles: 'profiles',
    topics: 'topics',
    applications: 'applications',
    apiKeys: 'apiKeys',
    dataPatterns: 'dataPatterns',
    dataProfiles: 'dataProfiles',
    filteringProfiles: 'filteringProfiles',
    dictionaries: 'dictionaries',
}
const GATEWAY_RESOURCE: Partial<Record<ChangeKind, GatewayResource>> = {
    gatewayWorkspaces: 'workspaces',
    gatewayProviders: 'providers',
    gatewayRoutes: 'routes',
    gatewayGuardrails: 'guardrails',
    gatewayServiceKeys: 'serviceKeys',
    gatewayUserKeys: 'userKeys',
    gatewayIntegrations: 'integrations',
}

/** What an applied change established, for its action (C31): never success from a readback value alone. */
export type AppliedState = 'live' | 'removed' | 'unconfirmed' | 'failed'
export function appliedState(r: ChangeResultView): AppliedState {
    if (r.outcome === 'unknown') return 'unconfirmed'
    if (r.outcome !== 'completed') return 'failed'
    // The service accepted it; only the readback of this action confirms it.
    if (r.failure) return 'unconfirmed'
    if (r.action === 'delete') return r.readback === 'absent' ? 'removed' : 'unconfirmed'
    return r.readback === 'found' ? 'live' : 'unconfirmed'
}

/** What a reconciliation read of the reviewed item established now. */
type Reconciled = { state: 'reading' } | { state: 'read' } | { state: 'present' } | { state: 'absent' } | { state: 'partial' } | { state: 'failed'; message: string }

/**
 * The review surface (C31, I3, I6, I7): the reviewed change (diff, impact,
 * "Type <name> to confirm" with Apply disabled until it matches; the native
 * confirmation applies), then its applied panel from the core's result, or
 * the staged rotation panel for a Runtime key version. Every exit (Done,
 * Escape, the scrim, navigation, unmount) ends the reviews it opened in the
 * core, which retires any one-time key their changes returned or will
 * return; a key held for a rotation it resumed is forgotten too.
 */
export function ReviewSheet({ snapshot, context, dispatch, observe, change: initial, rotation: rotationApp, onDraft, note, onClose }: ReviewSheetProps) {
    const [change, setChange] = useState<ChangeTarget | null>(initial)
    const [application] = useState<string | null>(rotationApp ?? (initial?.kind === 'apiKeys' && initial.action === 'version' ? applicationOf(initial) : null))
    const [busy, setBusy] = useState(false)
    // "Application updated" waits for the core's recorded acknowledgement
    // before the old key's removal opens (Version → acknowledgement → Delete).
    const [ackWaiting, setAckWaiting] = useState(false)
    const [ackProblem, setAckProblem] = useState<string | null>(null)
    const [reconciled, setReconciled] = useState<Reconciled | null>(null)
    // Results present when this change was opened are not its outcome.
    const seen = useRef(new Set(snapshot.changes.results.map((r) => r.operation)))
    const [, setOpened] = useState(0)
    useEffect(() => {
        seen.current = new Set(snapshot.changes.results.map((r) => r.operation))
        setReconciled(null)
        // Render again without the results that belonged to the previous change.
        setOpened((n) => n + 1)
        // eslint-disable-next-line react-hooks/exhaustive-deps -- reset per opened change only
    }, [change])
    const result = change ? (snapshot.changes.results.find((r) => matches(r, change) && !seen.current.has(r.operation)) ?? null) : null
    const rotation = application ? (snapshot.rotations.find((r) => r.application === application) ?? null) : null
    // A key change of a rotation is followed by the rotation panel once it settles.
    const rotationStep = !!change && change.kind === 'apiKeys' && (change.action === 'version' || change.action === 'delete') && !!application
    const showWorkbench = !!change && !(result?.settled && rotationStep)

    // A completion is confirmed once more by a toast bound to this tenant and
    // context (I2, I10): it persists under Reduce Motion until dismissed.
    const announced = useRef(new Set<string>())
    const stage = rotation?.stage ?? null
    const settledState = result?.settled && !rotationStep ? appliedState(result) : null
    useEffect(() => {
        const say = (key: string, text: string) => {
            if (announced.current.has(key)) return
            announced.current.add(key)
            useShellStore.getState().toast(text, 'ok', undefined, snapshot.active?.tenant ?? null, context)
        }
        if (stage === 'complete' && rotation) say(`rotation-${rotation.application}`, C.rotation.completeToast(rotation.application))
        if (result && (settledState === 'live' || settledState === 'removed'))
            say(result.operation, settledState === 'live' ? C.applied.liveToast(change?.title ?? '') : C.applied.removedToast(change?.title ?? ''))
        // eslint-disable-next-line react-hooks/exhaustive-deps -- on each observed completion
    }, [stage, settledState])

    // Every review this sheet opened, so its exit can end each one in the core.
    const reviews = useRef(new Set<string>())
    const open = snapshot.changes.review?.review ?? null
    useEffect(() => {
        if (open && change) reviews.current.add(open)
    }, [open, change])
    const exit = useRef<() => void>(() => {})
    exit.current = () => {
        // I6: the core ends each review; an applied one retires the one-time
        // key its change returned, settled or still under way.
        for (const review of reviews.current) void dispatch({ kind: 'discardChange', review })
        reviews.current.clear()
        if (rotation?.issuedBy) void dispatch({ kind: 'forgetSecret', operation: rotation.issuedBy })
    }
    // Unmount (navigation, tenant switch, window close) is an exit too. The
    // deferred call lets a development re-mount cancel it.
    const pendingExit = useRef<number | null>(null)
    useEffect(() => {
        if (pendingExit.current !== null) window.clearTimeout(pendingExit.current)
        pendingExit.current = null
        return () => {
            pendingExit.current = window.setTimeout(() => exit.current(), 0)
        }
    }, [])

    function close() {
        exit.current()
        onClose()
    }

    async function updated() {
        if (!rotation) return
        setBusy(true)
        setAckProblem(null)
        const outcome = await dispatch({ kind: 'acknowledgeDeployment', context, application: rotation.application })
        setBusy(false)
        if (outcome.kind === 'refused' || outcome.kind === 'failed') {
            setAckProblem(outcomeMessage(outcome) ?? C.rotation.ackRefused)
            return
        }
        // Admitted, or not confirmed: only the observed acknowledgement opens the removal.
        if (outcome.kind === 'unknown') setAckProblem(C.rotation.ackUnknown)
        setAckWaiting(true)
    }
    const acknowledgedAt = rotation?.deploymentAcknowledgedAt ?? null
    useEffect(() => {
        if (!ackWaiting || acknowledgedAt === null || !rotation) return
        setAckWaiting(false)
        setAckProblem(null)
        setChange(removeOld(rotation.oldKey.id, rotation.oldKey.name))
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once the acknowledgement is observed
    }, [ackWaiting, acknowledgedAt])

    /** Reads the reviewed item itself (never a replay): its detail by ID, or the inventory by name for a creation. */
    async function reconcile(of: { kind: ChangeKind; target: string; name: string | null; workspace?: string | null }) {
        setBusy(true)
        setReconciled({ state: 'reading' })
        const runtime = RUNTIME_RESOURCE[of.kind]
        const gateway = GATEWAY_RESOURCE[of.kind]
        let request: ManagementRequest | null = null
        if (runtime) request = of.target ? { kind: 'detail', resource: runtime, id: of.target } : { kind: 'page', resource: runtime, offset: 0, limit: 100 }
        else if (gateway) {
            const workspace = gateway === 'workspaces' || gateway === 'integrations' ? null : (of.workspace ?? null)
            request = {
                kind: 'gateway',
                workspace,
                read: of.target ? { kind: 'detail', resource: gateway, id: of.target } : { kind: 'inventory', resource: gateway },
            }
        }
        if (!request) {
            setBusy(false)
            setReconciled({ state: 'failed', message: C.applied.cannotReconcile })
            return
        }
        const outcome = await readManagement(dispatch, observe, context, request)
        setBusy(false)
        if (outcome.kind === 'notAdmitted') return setReconciled({ state: 'failed', message: outcome.message })
        const r = outcome.result
        if (r.kind === 'failed')
            return setReconciled({
                state: 'failed',
                message: MANAGEMENT_FAILURE_TEXT[r.failure],
            })
        const found = r.kind === 'detail' ? r.detail.found : r.kind === 'gateway' && r.gateway.kind === 'detail' ? r.gateway.detail.found : null
        if (found)
            return setReconciled({
                state: found === 'found' ? 'present' : found === 'absent' ? 'absent' : 'partial',
            })
        // A rotation's issuance: the core matched the reviewed key name; the panel shows what it established.
        if (of.name === null) return setReconciled({ state: 'read' })
        // A creation: look for its name in what was read.
        let rows: Record<string, unknown>[] = []
        let complete = false
        if (r.kind === 'page') {
            rows = JSON.parse(r.page.items) as Record<string, unknown>[]
            complete = r.page.offset === 0 && r.page.complete
        } else if (r.kind === 'gateway' && r.gateway.kind === 'list') {
            rows = JSON.parse(r.gateway.list.rows) as Record<string, unknown>[]
            complete = r.gateway.list.complete
        }
        const info = RESOURCES.find((x) => x.id === runtime)
        const named = rows.some((row) => (info ? itemName(info, row) : String(row.name ?? '')) === of.name)
        setReconciled({
            state: named ? 'present' : complete ? 'absent' : 'partial',
        })
    }

    return (
        <Modal label={application ? C.rotation.title(application) : C.reviewTitle} onClose={close} width={620} padding={30} sheet data-slot='review-sheet'>
            <div className='flex flex-col' style={{ gap: 18 }}>
                {/* R2App's review sheet (C39b): the title with the review's state, a close control, and the change's path. */}
                <div className='flex items-center' style={{ gap: 10 }}>
                    <h2 className='text-[24px] font-bold' style={{ letterSpacing: '-0.01em' }}>
                        {application ? C.rotation.title(application) : C.reviewTitle}
                    </h2>
                    <span className='rounded-full text-[12px] font-semibold' style={{ padding: '2px 9px', background: settledState === 'live' || settledState === 'removed' ? 'var(--v2-ok-bg)' : settledState === 'failed' ? 'var(--v2-block-bg)' : 'var(--v2-inset)', color: settledState === 'live' || settledState === 'removed' ? 'var(--v2-ok)' : settledState === 'failed' ? 'var(--v2-block)' : 'var(--v2-soft)' }} data-slot='review-state'>
                        {settledState === 'live' ? 'Applied' : settledState === 'removed' ? 'Removed' : settledState === 'failed' ? 'Not applied' : settledState === 'unconfirmed' ? 'Outcome unknown' : snapshot.changes.review && change ? (snapshot.changes.review.state === 'ready' ? 'Ready for review' : snapshot.changes.review.state === 'confirming' ? 'Confirming' : snapshot.changes.review.state === 'preparing' ? 'Preparing' : 'Review') : 'Draft'}
                    </span>
                    <button type='button' aria-label='Close review' onClick={close} className='ml-auto grid shrink-0 place-items-center rounded-[7px] border' style={{ width: 28, height: 28, minHeight: 0, borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
                        <Glyph name='x' size={12} stroke={2.2} />
                    </button>
                </div>
                {change ? (
                    <p className='text-[14px]' style={{ color: 'var(--v2-soft)' }} data-slot='review-path'>
                        {`${CHANGE_NOUN[change.kind].charAt(0).toUpperCase()}${CHANGE_NOUN[change.kind].slice(1)} › `}
                        <b style={{ color: 'var(--v2-ink)' }}>{change.title}</b>
                    </p>
                ) : null}
                {rotation ? (
                    <RotationPanel
                        rotation={rotation}
                        tenant={snapshot.active?.tenant ?? null}
                        busy={busy}
                        onReveal={(operation) => void dispatch({ kind: 'revealSecret', operation })}
                        onFinishLater={close}
                        onUpdated={() => void updated()}
                        onIssueAnother={() => setChange(issueAnother(rotation.oldKey.id, rotation.application))}
                        onRemoveOld={() => setChange(removeOld(rotation.oldKey.id, rotation.oldKey.name))}
                        onReconcile={() =>
                            void reconcile(
                                rotation.stage === 'removalUnconfirmed'
                                    ? {
                                          kind: 'apiKeys',
                                          target: rotation.oldKey.id,
                                          name: rotation.oldKey.name,
                                      }
                                    : { kind: 'apiKeys', target: '', name: null }
                            )
                        }
                    />
                ) : null}
                {showWorkbench && change ? (
                    <ChangeWorkbench
                        key={`${change.kind}-${change.action}-${change.target}-${change.draftId ?? ''}`}
                        change={change}
                        review={snapshot.changes.review}
                        staged={snapshot.changes.staged}
                        context={context}
                        dispatch={dispatch}
                        onClose={close}
                        onDraft={onDraft}
                        note={note}
                        requireTyped
                    />
                ) : null}
                {result && !rotationStep ? (
                    <Applied
                        result={result}
                        tenant={snapshot.tenants.find((t) => t.id === snapshot.active?.tenant)?.label ?? null}
                        reconciled={reconciled}
                        busy={busy}
                        onDone={close}
                        onReconcile={() =>
                            void reconcile({
                                kind: result.kind,
                                target: result.target,
                                name: result.name,
                                workspace: change?.workspace ?? null,
                            })
                        }
                        onReveal={() =>
                            void dispatch({
                                kind: 'revealSecret',
                                operation: result.operation,
                            })
                        }
                    />
                ) : null}
                {result && rotationStep && !result.settled ? (
                    <p role='status' className='text-[13px]'>
                        {resultText(result)}
                    </p>
                ) : null}
                {result && rotationStep && result.settled && (!rotation || result.outcome === 'rejected' || result.outcome === 'notSent') && change ? (
                    // The latest key change was not made (or the rotation could not be named): the
                    // core's sanitized result, apart from any rotation it leaves as it was, and an
                    // explicit way to try again through a new review. Nothing is resent by itself.
                    <section role='alert' data-slot='rotation-step-failed' data-action={change.action} className='flex flex-col gap-2 text-[13px]' style={{ color: 'var(--v2-block)' }}>
                        <p>{resultText(result)}</p>
                        {rotation ? <p style={{ color: 'var(--v2-ink)' }}>{change.action === 'version' ? C.rotation.issueFailedKept : C.rotation.removeFailedKept}</p> : null}
                        <div>
                            <button
                                type='button'
                                className='h-8 rounded-[7px] border px-3 text-[13px]'
                                style={{ borderColor: 'var(--v2-rule)', color: 'var(--v2-ink)' }}
                                onClick={() => setChange({ ...change })}
                            >
                                {C.rotation.tryAgain}
                            </button>
                        </div>
                    </section>
                ) : null}
                {rotation && reconciled ? <ReconcileNote reconciled={reconciled} /> : null}
                {ackProblem ? (
                    <p role='alert' data-slot='acknowledge-problem' className='text-[13px]' style={{ color: 'var(--v2-block)' }}>
                        {ackProblem}
                    </p>
                ) : null}
            </div>
        </Modal>
    )
}

function ReconcileNote({ reconciled }: { reconciled: Reconciled }) {
    return (
        <p role='status' data-slot='reconciled' data-state={reconciled.state} className='text-[13px]'>
            {reconciled.state === 'reading'
                ? C.applied.reading
                : reconciled.state === 'read'
                  ? C.applied.readRotation
                  : reconciled.state === 'present'
                    ? C.applied.present
                    : reconciled.state === 'absent'
                      ? C.applied.absent
                      : reconciled.state === 'partial'
                        ? C.applied.partial
                        : C.applied.readFailed(reconciled.message)}
        </p>
    )
}

/** The applied panel of a non-rotation change: only what the core's readback established, for its action. */
function Applied({
    result,
    tenant,
    reconciled,
    busy,
    onDone,
    onReconcile,
    onReveal,
}: {
    result: ChangeResultView
    tenant: string | null
    reconciled: Reconciled | null
    busy: boolean
    onDone: () => void
    onReconcile: () => void
    onReveal: () => void
}) {
    if (!result.settled) {
        return (
            <p role='status' className='text-[13px]' data-slot='applied'>
                {resultText(result)}
            </p>
        )
    }
    const state = appliedState(result)
    return (
        <section role='status' data-slot='applied' data-state={state} className='flex flex-col gap-2 rounded-[12px] border p-4' style={{ borderColor: 'var(--v2-rule)' }}>
            {state === 'live' ? (
                <Verdict tone='allowed' word={C.applied.live} />
            ) : state === 'removed' ? (
                <Verdict tone='allowed' word={C.applied.removed} />
            ) : state === 'unconfirmed' ? (
                <Verdict tone='flagged' word={C.applied.notConfirmed} />
            ) : (
                <Verdict tone='blocked' word={C.applied.failed} />
            )}
            <p className='text-[13px]'>{resultText(result)}</p>
            {state === 'live' || state === 'removed' ? <Receipt result={result} tenant={tenant} removed={state === 'removed'} /> : null}
            {result.secret === 'held' ? (
                <p className='text-[13px]' data-slot='applied-secret'>
                    {C.rotation.issuedNote}
                </p>
            ) : null}
            {reconciled ? <ReconcileNote reconciled={reconciled} /> : null}
            <div className='flex gap-2'>
                {result.secret === 'held' ? (
                    <button type='button' className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }} onClick={onReveal}>
                        {C.rotation.reveal}
                    </button>
                ) : null}
                {state === 'unconfirmed' ? (
                    <button
                        type='button'
                        disabled={busy}
                        className='h-8 rounded-[7px] border px-3 text-[13px] disabled:opacity-60'
                        style={{ borderColor: 'var(--v2-rule)' }}
                        onClick={onReconcile}
                    >
                        {C.applied.reconcile}
                    </button>
                ) : null}
                <button
                    type='button'
                    className='h-8 rounded-[7px] px-3 text-[13px] font-semibold'
                    style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}
                    onClick={onDone}
                >
                    {C.applied.done}
                </button>
            </div>
        </section>
    )
}

/**
 * What a confirmed change established, from the core's result only: the
 * tenant, the resource, the returned name and revision, and when the core
 * recorded the readback (its own clock, never recomputed). A field the
 * service did not return says so.
 */
function Receipt({ result, tenant, removed }: { result: ChangeResultView; tenant: string | null; removed: boolean }) {
    let item: Record<string, unknown> = {}
    try {
        item = result.current ? (JSON.parse(result.current) as Record<string, unknown>) : {}
    } catch {
        item = {}
    }
    const revision = typeof item.revision === 'number' || typeof item.revision === 'string' ? String(item.revision) : null
    const rows: [string, string][] = [
        [C.applied.tenant, tenant ?? C.applied.notReported],
        [C.applied.resource, CHANGE_NOUN[result.kind]],
        [C.applied.name, result.name ?? C.applied.notReported],
        ...(removed ? [] : ([[C.applied.revision, revision ?? C.applied.notReported]] as [string, string][])),
        [C.applied.readAt, result.settledAt !== undefined ? clockOf(result.settledAt) : C.applied.notReported],
    ]
    return (
        <dl data-slot='applied-receipt' className='grid gap-x-4 gap-y-1 text-[12px]' style={{ gridTemplateColumns: 'max-content 1fr' }}>
            {rows.map(([label, value]) => (
                <div key={label} className='contents'>
                    <dt style={{ color: 'var(--v2-soft)' }}>{label}</dt>
                    <dd className='break-all'>{value}</dd>
                </div>
            ))}
        </dl>
    )
}

function applicationOf(change: ChangeTarget): string | null {
    try {
        const body = JSON.parse(change.body) as { cust_app?: unknown }
        return typeof body.cust_app === 'string' && body.cust_app ? body.cust_app : null
    } catch {
        return null
    }
}

const issueAnother = (oldKey: string, application: string): ChangeTarget => ({
    kind: 'apiKeys',
    action: 'version',
    target: oldKey,
    title: `Issue additional key for ${application}`,
    body: pretty({ ...TEMPLATES.key, cust_app: application }),
})

const removeOld = (oldKey: string, name: string): ChangeTarget => ({
    kind: 'apiKeys',
    action: 'delete',
    target: oldKey,
    title: `Remove ${name}`,
    body: '{}',
})
