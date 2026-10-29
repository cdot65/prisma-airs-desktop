import { forwardRef, useEffect, useRef, useState } from 'react'
import { CheckCircle2, Circle } from 'lucide-react'
import type { ConversationView, FeatureId, HarnessApproval, HarnessEntry, Intent, ProposalView, RotationView, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { HARNESS_FAILURE_TEXT, TURN_TEXT, conversationText } from '../../features/harness/harness-view'
import { HISTORY_FAILURE_TEXT, HistoryPanel, savedAt } from '../../features/harness/history-panel'
import { McpPanel } from '../../features/harness/mcp-panel'
import { ProjectPanel } from '../../features/harness/project-panel'
import { SIGN_IN_FAILURE_TEXT, SignInPanel } from '../../features/harness/sign-in-panel'
import { type Dispatch, type DispatchOutcome, outcomeMessage } from '../../features/tenants/dispatch'
import { type Arrival, useShellStore } from '../store'
import { Verdict, type VerdictTone } from '../verdict'
import { HARNESS_COPY as C } from './copy'

export interface HarnessProps {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    onNavigate: (feature: FeatureId) => void
    /** A contextual action that named a conversation (its saved transcript id). */
    arrival: Arrival | null
}

/** Where the composer's text belongs when it starts the next conversation. */
const NEXT = 'next'
const MAX_TITLE = 72

/** A conversation's title: from its opening message when the core kept it, so it never changes as the transcript drops its oldest entries. */
const titleOf = (entries: HarnessEntry[], opening?: string) => {
    const first = (opening ?? entries.find((e) => e.role === 'operator')?.text ?? '').trim()
    if (!first) return C.untitled
    return first.length > MAX_TITLE ? `${first.slice(0, MAX_TITLE - 1)}…` : first
}

/** A send's own ID (32 lowercase hex): the core takes each at most once and reports it (C32). */
const newSubmission = () => Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, '0')).join('')

const live = (c: ConversationView | null) => c !== null && c.phase !== 'ended' && c.phase !== 'failed'

/**
 * One send into a new conversation (C32, I12), by submission: the text sent
 * (trimmed), the box's text as submitted (only that revision is cleared),
 * and where it stands. `ending` waits for the current conversation to end
 * (a new investigation); `clearing` and `requesting` are under way and
 * never re-entered; `starting` waits for exactly the conversation the start
 * receipt named. Nothing is replayed: a refusal, a failure, an end or a
 * replacement retires it and its text stays recoverable.
 */
type Queued = { id: number; text: string; raw: string } & (
    | { stage: 'ending' | 'clearing' | 'requesting' }
    | { stage: 'starting'; operation: string; prior: string | null }
)

/**
 * The Harness in the new shell (C32, screens/harness.md): a re-skin of the
 * existing engine, sessions, decisions, history, sign-in, project and MCP
 * modules (C19–C22). The core classifies each decision: a desktop read is
 * read-only and decided in the page; a change (files, a command with
 * changes allowed, a remote MCP tool) is allowed only through the native
 * confirmation the core opens. A proposal only ever opens the operator's
 * review in Administration. One conversation runs at a time; each keeps its
 * transcript. A fresh tenant's first send starts a conversation (I12); a
 * pending decision blocks a tenant switch (I1, the core's guard).
 *
 * The composer is one box per tenant and workspace (by its stable ID): it
 * survives navigation, tenant round trips and a conversation's retirement.
 * A message that reaches no conversation is kept in the "Not sent" list of
 * that scope, with "Put it in the box", unless the box still holds it;
 * nothing is ever sent by itself.
 */
export const Harness = forwardRef<HTMLHeadingElement, HarnessProps>(function Harness({ snapshot, dispatch, onNavigate, arrival }, ref) {
    const context = snapshot.active?.context ?? null
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) ?? null
    const selected = snapshot.gateway.selected
    const engine = snapshot.harness.engine
    const conversation = snapshot.harness.conversation
    const history = snapshot.harness.history
    const signIn = snapshot.harness.signIn
    const key = tenant?.inference
    const keyHere = key?.state === 'ready' && selected !== null && key.target?.workspace.id === selected.workspace.id
    const signInHere = signIn !== null && selected !== null && signIn.workspace.id === selected.workspace.id && signIn.selected
    const credentialReady = signInHere ? signIn.account !== null : keyHere
    const checklist: [string, boolean, { label: string; go: FeatureId } | null][] = [
        [C.tenant, tenant !== null, { label: C.openSettings, go: 'settings' }],
        [C.management, tenant?.management.state === 'ready', { label: C.openSettings, go: 'settings' }],
        [C.workspace, selected !== null, { label: C.chooseWorkspace, go: 'administration' }],
        [C.credential, credentialReady, { label: C.openSettings, go: 'settings' }],
    ]
    const ready = engine.available && context !== null && checklist.every(([, ok]) => ok)

    const [problem, setProblem] = useState<string | null>(null)
    const [busy, setBusy] = useState(false)
    const [setupOpen, setSetupOpen] = useState(false)
    const composer = useRef<HTMLTextAreaElement>(null)
    const running = live(conversation)
    // The conversation that runs now, if one does.
    const current = running ? conversation : null
    const scope = `${tenant?.id ?? '-'}|${selected?.workspace.id ?? '-'}`
    const draft = useShellStore((s) => s.harnessDrafts[scope] ?? '')
    const kept = useShellStore((s) => s.harnessUnsent[scope])
    const unsentKept = kept ?? []
    const fresh = useShellStore((s) => s.harnessFresh[scope] ?? false) && current !== null
    const box = () => useShellStore.getState().harnessDrafts[scope] ?? ''
    const setDraft = (text: string) => useShellStore.getState().setHarnessDraft(scope, text)
    const setFresh = (on: boolean) => useShellStore.getState().setHarnessFresh(scope, on)
    // The latest conversation, for a receipt that completes after newer snapshots.
    const latest = useRef(conversation)
    latest.current = conversation
    /**
     * The core took a send: the box is cleared of exactly what was sent (later
     * edits stay), unless the core has already given that send back, when its
     * text stays (attempt-18 R1). Otherwise it stays tracked until reported.
     */
    const admitted = (raw: string, conversationId: string, submission: string) => {
        const c = latest.current
        const returned = c !== null && c.id === conversationId && (c.submissions ?? []).some((t) => t.submission === submission && t.state === 'returned')
        if (returned) return
        if (box() === raw) setDraft('')
    }
    /** Keeps a message the core did not take, unless the box still holds it; says which. */
    const recover = (text: string): 'inBox' | 'kept' => {
        if (box().includes(text)) return 'inBox'
        useShellStore.getState().keepHarnessUnsent(scope, { text, unconfirmed: false })
        return 'kept'
    }
    /**
     * A message whose delivery the desktop did not confirm: it may have been
     * delivered. It is listed under "Not confirmed" (never "not sent") until
     * its conversation reports the core took its send ID; nothing is resent.
     */
    const uncertain = (text: string, conversation: string, submission: string) => {
        useShellStore.getState().keepHarnessUnsent(scope, { text, unconfirmed: true, conversation, submission })
        setProblem(C.unconfirmedSend)
    }
    const pendingDecision = current !== null && current.approvals.length > 0

    // The submission in flight: a ref for completions, state for the page.
    const queuedRef = useRef<Queued | null>(null)
    const [queued, setQueuedState] = useState<Queued | null>(null)
    const setQueued = (q: Queued | null) => {
        queuedRef.current = q
        setQueuedState(q)
    }
    const submissions = useRef(0)
    const stillQueued = (q: Queued) => queuedRef.current?.id === q.id

    async function run(intent: Intent): Promise<DispatchOutcome> {
        setProblem(null)
        setBusy(true)
        try {
            const outcome = await dispatch(intent)
            if (outcome.kind !== 'admitted') setProblem(outcomeMessage(outcome) ?? C.failed)
            return outcome
        } finally {
            setBusy(false)
        }
    }

    /** Retires a submission: nothing is sent; its text stays in the box or in "Not sent". */
    function retire(q: Queued, why: (where: 'inBox' | 'kept') => string, keepProblem = false) {
        if (!stillQueued(q)) return
        setQueued(null)
        setFresh(false)
        const where = recover(q.text)
        setProblem((p) => (keepProblem && p ? `${p} ${why(where)}` : why(where)))
    }

    // The saved conversations of the chosen workspace are listed once per workspace.
    const listed = useRef<string | null>(null)
    useEffect(() => {
        if (!context || !selected || listed.current === selected.reference) return
        listed.current = selected.reference
        void dispatch({ kind: 'listHarnessHistory', context, workspace: selected.reference })
    }, [context, selected, dispatch])

    // A message given back by the core (a failed renewal, changes not allowed
    // again) fills an empty box; otherwise it is kept in "Not sent" beside the
    // operator's draft. Nothing is resent (R4).
    const unsent = conversation?.unsent ?? null
    /** Where a returned message is, said as it is: in the box, under Not sent, or (edited away by the operator) neither. */
    const unsentWhere = (text: string) => (draft.includes(text) ? C.unsentInBox : unsentKept.some((k) => !k.unconfirmed && k.text === text) ? C.unsentKept : C.unsentReturned)
    useEffect(() => {
        if (unsent === null) return
        if (!box()) {
            setDraft(unsent)
        } else if (!box().includes(unsent)) {
            useShellStore.getState().keepHarnessUnsent(scope, { text: unsent, unconfirmed: false })
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once per returned message
    }, [unsent, conversation?.id])

    /** Starts the next conversation for submission `q` (its stage already `clearing`): clears an ended one first. */
    async function begin(q: Queued, prior: ConversationView | null) {
        if (!context || !selected || !ready) return retire(q, (w) => C.notStarted[w])
        if (prior && !live(prior)) {
            const cleared = await run({ kind: 'clearHarness', conversation: prior.id })
            if (!stillQueued(q)) return
            if (cleared.kind !== 'admitted') return retire(q, (w) => (cleared.kind === 'unknown' ? C.unconfirmedStep[w] : C.notStarted[w]), true)
        }
        setQueued({ ...q, stage: 'requesting' })
        const started = await run({ kind: 'startHarness', context, workspace: selected.reference })
        if (!stillQueued(q)) return
        if (started.kind === 'admitted' && started.operation) setQueued({ id: q.id, text: q.text, raw: q.raw, stage: 'starting', operation: started.operation, prior: prior?.id ?? null })
        // Not confirmed: a conversation may have started; the message was not sent to it.
        else retire(q, (w) => (started.kind === 'unknown' ? C.unconfirmedStep[w] : C.notStarted[w]), true)
    }

    // The submission follows exactly its conversation (R2–R4).
    useEffect(() => {
        const q = queuedRef.current
        if (!q) return
        if (q.stage === 'ending') {
            if (live(conversation)) return
            // Advanced before anything asynchronous: a later snapshot never starts twice.
            const next: Queued = { id: q.id, text: q.text, raw: q.raw, stage: 'clearing' }
            setQueued(next)
            void begin(next, conversation)
            return
        }
        if (q.stage !== 'starting') return
        // The conversation it replaces may still be shown until the next snapshot.
        if (conversation && conversation.id === q.prior) return
        if (!conversation) return
        if (conversation.id !== q.operation) return retire(q, (w) => C.replaced[w])
        if (conversation.phase === 'failed') return retire(q, (w) => C.startFailed[w])
        if (conversation.phase === 'ended' || conversation.phase === 'ending') return retire(q, (w) => C.endedBeforeReady[w])
        if (conversation.phase !== 'ready') return
        setQueued(null)
        const submission = newSubmission()
        useShellStore.getState().trackHarnessSend(scope, { submission, conversation: conversation.id, text: q.text })
        void run({ kind: 'sendHarness', conversation: conversation.id, prompt: q.text, submission }).then((outcome) => {
            setFresh(false)
            if (outcome.kind !== 'admitted') useShellStore.getState().untrackHarnessSend(scope, submission)
            if (outcome.kind === 'admitted') {
                // Only the submitted revision is cleared: later edits stay (R3).
                admitted(q.raw, conversation.id, submission)
            } else if (outcome.kind === 'unknown') {
                uncertain(q.text, conversation.id, submission)
            } else {
                const where = recover(q.text)
                setProblem((p) => `${p ?? C.failed} ${C.notSent[where]}`)
            }
        })
        // eslint-disable-next-line react-hooks/exhaustive-deps -- driven by the observed conversation
    }, [queued, conversation?.id, conversation?.phase])

    // A message not confirmed is reconciled with what the core shows: once its
    // conversation reports the core took its send ID, it was delivered; it
    // leaves the list, and the box if it still holds exactly that text (R2).
    // Only the core's word on that one send settles it: never an identical
    // message, another send's entry, or another conversation (R1).
    // A send the core gave back (a failed sign-in renewal, changes not allowed
    // again) was not delivered: it leaves "Not confirmed" without clearing
    // anything, and its exact text stays in the box or is kept under Not sent.
    // Only the core's final word settles a send: delivered (it entered the
    // transcript) or given back. A pending send (a sign-in renewal or the write
    // confirmation under way) may still be given back (attempt-19 R1).
    const reported = new Map((conversation?.submissions ?? []).filter((t) => t.state !== 'pending').map((t) => [t.submission, t.state === 'returned']))
    const settled = (kept ?? []).filter((k) => k.unconfirmed && conversation !== null && k.conversation === conversation.id && k.submission !== undefined && reported.has(k.submission))
    // Sends dispatched (admitted, or still awaiting their receipt) that their
    // conversation now reports: delivered, they are done; given back, their
    // text stays in the box or is kept under Not sent, whichever order the
    // receipt and the core's word arrive in (attempt-18 R1).
    const sentRecords = useShellStore((s) => s.harnessSent[scope])
    const sentReported = (sentRecords ?? []).filter((t) => conversation !== null && t.conversation === conversation.id && reported.has(t.submission))
    // A tracked send whose conversation is no longer shown (retired, ended and
    // cleared, or another tenant's or workspace's meanwhile) can never be
    // reported here: it is kept as Not confirmed, with its text, never lost.
    const sentOrphaned = (sentRecords ?? []).filter((t) => conversation === null || t.conversation !== conversation.id)
    useEffect(() => {
        for (const t of sentOrphaned) {
            useShellStore.getState().untrackHarnessSend(scope, t.submission)
            useShellStore.getState().keepHarnessUnsent(scope, { text: t.text, unconfirmed: true, conversation: t.conversation, submission: t.submission })
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps -- when a conversation goes
    }, [sentOrphaned.map((t) => t.submission).join(',')])
    useEffect(() => {
        for (const t of sentReported) {
            useShellStore.getState().untrackHarnessSend(scope, t.submission)
            const notSent = (useShellStore.getState().harnessUnsent[scope] ?? []).some((k) => !k.unconfirmed && k.text === t.text)
            if (reported.get(t.submission) === true && !box().includes(t.text) && !notSent) useShellStore.getState().keepHarnessUnsent(scope, { text: t.text, unconfirmed: false })
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps -- when the core reports them
    }, [sentReported.map((t) => `${t.submission}:${reported.get(t.submission)}`).join(',')])
    useEffect(() => {
        if (settled.length === 0) return
        for (const record of settled) {
            const returned = reported.get(record.submission!) === true
            useShellStore.getState().dropHarnessUnsent(scope, record.id)
            if (returned) {
                if (!box().includes(record.text)) useShellStore.getState().keepHarnessUnsent(scope, { text: record.text, unconfirmed: false })
            } else if (box().trim() === record.text.trim() && !(useShellStore.getState().harnessUnsent[scope] ?? []).some((k) => k.text === record.text)) setDraft('')
        }
        setProblem((p) => (p === C.unconfirmedSend ? null : p))
        // eslint-disable-next-line react-hooks/exhaustive-deps -- when the core reports them
    }, [settled.map((d) => `${d.id}:${reported.get(d.submission!)}`).join(',')])

    // An ended conversation is confirmed once by a toast bound to its tenant
    // and context (I2, I10): it persists under Reduce Motion until dismissed.
    const endedSeen = useRef<string | null>(null)
    useEffect(() => {
        if (!conversation || conversation.phase !== 'ended' || endedSeen.current === conversation.id) return
        endedSeen.current = conversation.id
        const saved = conversation.savedAt !== null && conversation.saveFailure === null
        useShellStore.getState().toast(saved ? C.endedToastSaved(conversation.historyId.slice(0, 8)) : C.endedToast, 'info', undefined, tenant?.id ?? null, context)
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once per ended conversation
    }, [conversation?.id, conversation?.phase])

    async function send() {
        const raw = draft
        const text = raw.trim()
        if (!text || busy || queuedRef.current) return
        // Sending something else retires a contextual restart still reading its record.
        if (awaitingOpening) {
            setAwaitingOpening(null)
            setHint(null)
        }
        if (current && !fresh) {
            if (current.phase !== 'ready') return
            // The box is cleared only once the core took the message, and only
            // if it still holds what was sent (R2, R3).
            // Each send carries its own ID: the core takes it at most once and
            // reports it, which alone settles it if its receipt is lost (R1).
            const submission = newSubmission()
            useShellStore.getState().trackHarnessSend(scope, { submission, conversation: current.id, text })
            const outcome = await run({ kind: 'sendHarness', conversation: current.id, prompt: text, submission })
            if (outcome.kind !== 'admitted') useShellStore.getState().untrackHarnessSend(scope, submission)
            if (outcome.kind === 'admitted') {
                admitted(raw, current.id, submission)
            } else if (outcome.kind === 'unknown') {
                uncertain(text, current.id, submission)
            } else {
                // Refused or never delivered: the exact text stays, beside any later edit (R1).
                const where = recover(text)
                setProblem((p) => `${p ?? C.failed} ${C.notSent[where]}`)
            }
            return
        }
        const q: Queued = { id: ++submissions.current, text, raw, stage: current ? 'ending' : 'clearing' }
        if (current) {
            // A new investigation: the current conversation ends (its transcript is kept), then the next one starts.
            if (pendingDecision) {
                setProblem(C.decideFirst)
                return
            }
            setQueued(q)
            const ended = await run({ kind: 'endHarness', conversation: current.id })
            if (ended.kind !== 'admitted') retire(q, (w) => (ended.kind === 'unknown' ? C.unconfirmedStep[w] : C.notStarted[w]), true)
            return
        }
        setQueued(q)
        await begin(q, conversation)
    }

    // Contextual navigation into an ended conversation: a hint, cleared on any navigation.
    // Its opening question is the conversation's opening message, whole (never
    // its shortened title): the ended conversation's own, or read from the
    // saved record when the restart is asked for (`null`: not known).
    const [hint, setHint] = useState<{ id: string; saved: boolean; opening: string | null } | null>(null)
    // The saved record being read for it, and the last open that had settled when it was asked for.
    const [awaitingOpening, setAwaitingState] = useState<{ id: string; after: number } | null>(null)
    // The read in progress, for its own completion: a read retired meanwhile reports nothing.
    const awaitingRef = useRef<{ id: string; after: number } | null>(null)
    const setAwaitingOpening = (next: { id: string; after: number } | null) => {
        awaitingRef.current = next
        setAwaitingState(next)
    }
    // A contextual arrival's conversation, until the history resolves it: the
    // saved conversations may still be listing when it arrives.
    // Bound to the tenant and workspace it arrived in: another's history never resolves it.
    const [arriving, setArriving] = useState<{ id: string; scope: string } | null>(null)
    // Another tenant or workspace: the hint, its read and a pending arrival belong to the one left.
    useEffect(() => {
        setHint(null)
        setAwaitingOpening(null)
        setArriving(null)
    }, [scope])

    function newInvestigation(question?: string) {
        // Held back by a pending decision: nothing is chosen, so the hint and any read for it stay.
        if (pendingDecision) {
            setProblem(C.decideFirst)
            return
        }
        setHint(null)
        setArriving(null)
        // Another investigation chosen: a contextual restart still reading its record is retired (its late result is ignored).
        setAwaitingOpening(null)
        setFresh(running)
        if (question !== undefined) {
            // The opening question goes in the box; a draft already there is kept in "Not sent".
            const before = box().trim()
            if (before && before !== question) useShellStore.getState().keepHarnessUnsent(scope, { text: before, unconfirmed: false })
            setDraft(question)
        }
        composer.current?.focus()
    }

    const seenArrival = useRef(0)
    useEffect(() => {
        if (!arrival || arrival.sequence === seenArrival.current) return
        seenArrival.current = arrival.sequence
        // A new arrival replaces the hint and any read for it; it is resolved below.
        setHint(null)
        setAwaitingOpening(null)
        setArriving(arrival.item === null ? null : { id: arrival.item, scope })
        // eslint-disable-next-line react-hooks/exhaustive-deps -- once per arrival
    }, [arrival])
    // Resolved once the conversation or the history shows it, or says it cannot: the
    // ended conversation here, a listed saved one, or (listed without it, or not
    // listable) a statement why; while the list loads it stays pending.
    useEffect(() => {
        if (arriving === null) return
        if (arriving.scope !== scope) return setArriving(null)
        const id = arriving.id
        if (conversation && conversation.historyId === id) {
            setArriving(null)
            if (!live(conversation)) setHint({ id, saved: false, opening: conversation.opening ?? null })
            return
        }
        if (history?.conversations.some((c) => c.id === id)) {
            setArriving(null)
            return setHint({ id, saved: true, opening: null })
        }
        if (history?.state === 'ready') {
            setArriving(null)
            setProblem(C.arrivalNotFound)
        } else if (history?.state === 'failed') {
            setArriving(null)
            setProblem(C.arrivalUnlisted)
        }
    }, [arriving, history, conversation, scope])

    /** A new investigation prefilled with the opening question, whole; when it is not known, an empty one that says so. */
    function restartWith(opening: string | null) {
        if (opening !== null) return newInvestigation(opening)
        if (pendingDecision) return newInvestigation()
        newInvestigation()
        setProblem(C.openingUnknown)
    }
    function startFromHint() {
        if (!hint || awaitingOpening) return
        if (pendingDecision) return setProblem(C.decideFirst)
        if (!hint.saved) return restartWith(hint.opening)
        if (history?.opened?.id === hint.id) return restartWith(history.opened.opening ?? null)
        if (!context) return
        // The saved record is read for its opening question; nothing starts until it is known.
        // Its read settles by the core's receipt for that record, newer than
        // any open settled before it was asked for: never by a busy edge or a
        // changed error, which a repeated failure or a batched update hides.
        const id = hint.id
        const mine = { id, after: history?.lastOpen?.sequence ?? 0 }
        setAwaitingOpening(mine)
        // A read, dispatched apart from run(): it never holds the page busy, and
        // only it reports its outcome, and only while it is still the read in
        // progress (a retired read's refusal says nothing at all, attempt-16 S1).
        void dispatch({ kind: 'openHarnessHistory', context, workspace: selected?.reference ?? null, id }).then((outcome) => {
            if (outcome.kind === 'admitted') return
            if (awaitingRef.current !== mine) return
            setAwaitingOpening(null)
            // Refused, or not confirmed (it may still be read): never said to have failed when that is not known.
            setProblem(outcome.kind === 'unknown' ? C.openingUnconfirmed : C.openingUnread)
        })
    }
    const settledOpen = history?.lastOpen
    useEffect(() => {
        const a = awaitingOpening
        if (!a || !settledOpen || settledOpen.id !== a.id || settledOpen.sequence <= a.after) return
        setAwaitingOpening(null)
        // Only for the hint that asked: once the operator moved on, a late read changes nothing.
        if (hint?.id !== a.id) return
        if (settledOpen.failure !== null) return setProblem(C.openingUnread)
        restartWith(history?.opened?.id === a.id ? (history.opened.opening ?? null) : null)
        // eslint-disable-next-line react-hooks/exhaustive-deps -- settles on the core's receipt
    }, [awaitingOpening, settledOpen?.id, settledOpen?.sequence])
    // Leaving the Harness consumes its arrival: coming back shows no stale hint.
    useEffect(
        () => () => {
            if (useShellStore.getState().arrival?.feature === 'harness') useShellStore.setState({ arrival: null })
        },
        []
    )

    function openSaved(id: string) {
        if (!context) return
        setHint(null)
        setAwaitingOpening(null)
        setArriving(null)
        void run({ kind: 'openHarnessHistory', context, workspace: selected?.reference ?? null, id })
    }

    const saved = (history?.conversations ?? []).filter((c) => !conversation || c.id !== conversation.historyId)
    const outcome = conversation ? outcomeOf(conversation) : null

    return (
        <section aria-labelledby='harness-heading' data-slot='harness-v2' className='flex min-w-0 flex-col gap-4'>
            <div>
                <h1 ref={ref} id='harness-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)' }}>
                    {C.title}
                </h1>
                <p className='mt-1 text-[13px]' data-slot='harness-status' style={{ color: 'var(--v2-soft)' }}>
                    {[tenant?.label, engine.available ? C.engineReady(engine.runtimeVersion) : C.engineMissing, selected?.workspace.name ?? C.noWorkspace].filter(Boolean).join(' · ')}
                </p>
            </div>
            {checklist.some(([, ok]) => !ok) ? (
                <section aria-label={C.prerequisites} data-slot='harness-prerequisites' className='rounded-[12px] border p-4' style={{ borderColor: 'var(--v2-rule)' }}>
                    <h2 className='text-[14px] font-semibold'>{C.prerequisites}</h2>
                    <p className='mt-1 text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                        {C.prerequisitesNote}
                    </p>
                    <ul className='mt-3 flex flex-col gap-2 text-[13px]'>
                        {checklist.map(([label, ok, fix]) => (
                            <li key={label} data-slot='prerequisite' data-done={ok} className='flex flex-wrap items-center gap-2'>
                                {ok ? <CheckCircle2 aria-hidden size={16} style={{ color: 'var(--v2-ok)' }} /> : <Circle aria-hidden size={16} style={{ color: 'var(--v2-flag)' }} />}
                                <span className='flex-grow'>{label}</span>
                                <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                    {ok ? C.done : C.needed}
                                </span>
                                {!ok && fix ? (
                                    <button type='button' className='h-8 rounded-[7px] border px-3 text-[12px]' style={{ borderColor: 'var(--v2-rule)' }} onClick={() => onNavigate(fix.go)}>
                                        {fix.label}
                                    </button>
                                ) : null}
                            </li>
                        ))}
                    </ul>
                </section>
            ) : null}
            {/* Two columns while they fit; the rail stacks above the conversation when they do not (900x600 at 200%). */}
            <div className='flex min-w-0 flex-wrap gap-4'>
                <nav aria-label={C.conversations} data-slot='harness-rail' className='flex min-w-0 flex-col gap-2' style={{ flex: '1 1 200px', maxWidth: '100%' }}>
                    <button type='button' className='h-8 rounded-[7px] border px-3 text-left text-[13px]' style={{ borderColor: 'var(--v2-rule)' }} onClick={() => newInvestigation()}>
                        {C.newInvestigation}
                    </button>
                    <ul className='flex flex-col gap-1'>
                        {conversation ? (
                            <li data-slot='rail-conversation' data-state={running ? 'live' : 'ended'}>
                                <div className='rounded-[8px] px-2 py-2' style={{ background: 'var(--v2-sel)', color: 'var(--v2-sel-ink)' }} aria-current='true'>
                                    <span className='block truncate text-[13px] font-medium'>{titleOf(conversation.entries, conversation.opening)}</span>
                                    <span className='text-[12px]'>{C.phase[conversation.phase] ?? conversation.phase}</span>
                                </div>
                            </li>
                        ) : null}
                        {saved.map((c) => (
                            <li key={c.id} data-slot='rail-conversation' data-state='ended'>
                                <button type='button' className='w-full rounded-[8px] px-2 py-2 text-left' onClick={() => openSaved(c.id)}>
                                    <span className='block truncate text-[13px]'>{c.title}</span>
                                    <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                        {C.ended}
                                    </span>
                                </button>
                            </li>
                        ))}
                    </ul>
                    {history && history.state === 'ready' && saved.length === 0 && !conversation ? (
                        <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                            {C.savedNone}
                        </p>
                    ) : null}
                </nav>
                <div className='flex min-w-0 flex-col gap-3' style={{ flex: '999 1 360px', minWidth: 'min(100%, 360px)' }}>
                    {arriving !== null && !hint ? (
                        <p role='status' data-slot='harness-arriving' className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                            {C.findingArrival}
                        </p>
                    ) : null}
                    {hint ? (
                        <div role='status' data-slot='harness-hint' className='flex flex-col gap-2 rounded-[12px] border p-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                            <p>{C.endedHint}</p>
                            <div className='flex flex-wrap gap-2'>
                                <button type='button' className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }} onClick={() => openSaved(hint.id)}>
                                    {C.openSaved}
                                </button>
                                <button
                                    type='button'
                                    className='h-8 rounded-[7px] px-3 text-[13px] font-semibold'
                                    style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}
                                    aria-busy={awaitingOpening !== null}
                                    onClick={startFromHint}
                                >
                                    {awaitingOpening ? C.readingOpening : C.startNew}
                                </button>
                            </div>
                        </div>
                    ) : null}
                    <section aria-label={C.transcript} data-slot='harness-transcript' className='min-w-0'>
                        {conversation && conversation.entries.length > 0 ? (
                            <Transcript entries={conversation.entries} />
                        ) : (
                            <p className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                                {C.empty}
                            </p>
                        )}
                    </section>
                    {outcome ? (
                        <div role={outcome.tone === 'blocked' || outcome.tone === 'flagged' ? 'alert' : 'status'} data-slot='harness-outcome' data-tone={outcome.tone} className='flex flex-col gap-1 text-[13px]'>
                            <Verdict tone={outcome.tone} word={outcome.word} />
                            <span>{outcome.text}</span>
                        </div>
                    ) : null}
                    {conversation && (conversation.unsent !== null || conversation.signInFailure !== null) ? (
                        <div role='alert' data-slot='harness-unsent' className='flex flex-col gap-1 text-[13px]' style={{ color: 'var(--v2-block)' }}>
                            {conversation.signInFailure ? <p>{SIGN_IN_FAILURE_TEXT[conversation.signInFailure]}</p> : null}
                            {conversation.unsent !== null ? <p>{unsentWhere(conversation.unsent)}</p> : null}
                        </div>
                    ) : null}
                    {([false, true] as const).map((unconfirmed) => {
                        const items = unsentKept.filter((k) => k.unconfirmed === unconfirmed)
                        if (items.length === 0) return null
                        const title = unconfirmed ? C.notConfirmedTitle : C.notSentTitle
                        return (
                            <section key={title} aria-label={title} data-slot={unconfirmed ? 'harness-not-confirmed' : 'harness-not-sent'} className='flex flex-col gap-2 rounded-[12px] border p-3 text-[13px]' style={{ borderColor: 'var(--v2-flag)' }}>
                                <h2 className='text-[14px] font-semibold'>{title}</h2>
                                <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                    {unconfirmed ? C.notConfirmedNote : C.notSentNote}
                                </p>
                                <ul className='flex flex-col gap-2'>
                                    {items.map(({ text, id }) => (
                                        <li key={id} data-slot='not-sent' className='flex flex-col gap-1'>
                                            <blockquote className='rounded-[8px] border p-2 whitespace-pre-wrap' style={{ borderColor: 'var(--v2-rule)' }}>
                                                {text}
                                            </blockquote>
                                            <div className='flex flex-wrap gap-2'>
                                                <button
                                                    type='button'
                                                    className='h-8 rounded-[7px] border px-3 text-[13px]'
                                                    style={{ borderColor: 'var(--v2-rule)' }}
                                                    onClick={() => {
                                                        const now = box()
                                                        if (!now.includes(text)) setDraft(now ? `${now}\n\n${text}` : text)
                                                        useShellStore.getState().dropHarnessUnsent(scope, id)
                                                    }}
                                                >
                                                    {C.putBack}
                                                </button>
                                                <button type='button' className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }} onClick={() => useShellStore.getState().dropHarnessUnsent(scope, id)}>
                                                    {C.discardUnsent}
                                                </button>
                                            </div>
                                        </li>
                                    ))}
                                </ul>
                            </section>
                        )
                    })}
                    {conversation && conversation.phase === 'confirmWrite' && conversation.project ? (
                        // A renewal restarts the engine; allowing changes again is the operator's call (C21).
                        <section aria-label={C.confirmWrite} data-slot='harness-confirm-write' className='flex flex-col gap-2 rounded-[12px] border p-3' style={{ borderColor: 'var(--v2-flag)' }}>
                            <Verdict tone='flagged' word={C.confirmWrite} />
                            <p className='text-[13px]'>{C.confirmWriteNote(conversation.project)}</p>
                            <div className='flex flex-wrap gap-2'>
                                <button
                                    type='button'
                                    disabled={busy}
                                    className='h-8 rounded-[7px] border px-3 text-[13px] disabled:opacity-60'
                                    style={{ borderColor: 'var(--v2-rule)' }}
                                    onClick={() => void run({ kind: 'confirmHarnessWrite', conversation: conversation.id, allow: true })}
                                >
                                    {C.allowChanges}
                                </button>
                                <button
                                    type='button'
                                    disabled={busy}
                                    className='h-8 rounded-[7px] px-3 text-[13px] font-semibold disabled:opacity-60'
                                    style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}
                                    onClick={() => void run({ kind: 'confirmHarnessWrite', conversation: conversation.id, allow: false })}
                                >
                                    {C.notNow}
                                </button>
                            </div>
                            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                {C.notNowNote}
                            </p>
                        </section>
                    ) : null}
                    {current
                        ? current.approvals.map((a) => (
                              <Decision
                                  key={a.id}
                                  approval={a}
                                  busy={busy}
                                  onDecide={(allow) => void run({ kind: 'decideHarnessApproval', conversation: current.id, approval: a.id, allow })}
                              />
                          ))
                        : null}
                    {conversation && conversation.proposals.length > 0 ? (
                        <FollowUps
                            proposals={conversation.proposals}
                            rotations={snapshot.rotations}
                            busy={busy}
                            open={running}
                            onOpen={async (p) => {
                                if (p.state === 'staged' && (await run({ kind: 'reviewHarnessProposal', conversation: conversation.id, proposal: p.id })).kind !== 'admitted') return
                                onNavigate('administration')
                            }}
                            onDismiss={(p) => void run({ kind: 'dismissHarnessProposal', conversation: conversation.id, proposal: p.id })}
                        />
                    ) : null}
                    {conversation ? (
                        <p className='text-[12px]' data-slot='harness-saved-status' style={{ color: conversation.saveFailure ? 'var(--v2-block)' : 'var(--v2-soft)' }}>
                            {conversation.saveFailure
                                ? C.notSaved(HISTORY_FAILURE_TEXT[conversation.saveFailure])
                                : conversation.savedAt !== null
                                  ? C.saved(savedAt(conversation.savedAt), conversation.historyId.slice(0, 8))
                                  : C.notSavedYet}
                        </p>
                    ) : null}
                    <form
                        className='flex flex-col gap-2'
                        onSubmit={(e) => {
                            e.preventDefault()
                            void send()
                        }}
                    >
                        <label htmlFor='harness-message' className='text-[13px] font-medium'>
                            {C.message}
                        </label>
                        <textarea
                            id='harness-message'
                            ref={composer}
                            value={draft}
                            onChange={(e) => setDraft(e.target.value)}
                            rows={3}
                            className='rounded-[8px] border p-2 text-[13px]'
                            style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}
                        />
                        {!running || fresh ? (
                            <p className='text-[12px]' data-slot='harness-composer-note' style={{ color: 'var(--v2-soft)' }}>
                                {queued ? C.sending : running && fresh ? C.composerFresh : C.composerNew}
                            </p>
                        ) : null}
                        <div className='flex flex-wrap gap-2'>
                            <button
                                type='submit'
                                disabled={busy || queued !== null || draft.trim() === '' || (current && !fresh ? current.phase !== 'ready' : !ready)}
                                className='h-8 rounded-[7px] px-3 text-[13px] font-semibold disabled:opacity-60'
                                style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}
                            >
                                {C.send}
                            </button>
                            {running && fresh ? (
                                <button type='button' className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }} onClick={() => setFresh(false)}>
                                    {C.backToConversation}
                                </button>
                            ) : null}
                            {conversation && conversation.phase === 'answering' ? (
                                <button
                                    type='button'
                                    className='h-8 rounded-[7px] border px-3 text-[13px]'
                                    style={{ borderColor: 'var(--v2-rule)' }}
                                    onClick={() => void run({ kind: 'interruptHarness', conversation: conversation.id })}
                                >
                                    {C.interrupt}
                                </button>
                            ) : null}
                            {current ? (
                                <button
                                    type='button'
                                    className='h-8 rounded-[7px] border px-3 text-[13px]'
                                    style={{ borderColor: 'var(--v2-rule)' }}
                                    onClick={() => void run({ kind: 'endHarness', conversation: current.id })}
                                >
                                    {C.end}
                                </button>
                            ) : null}
                        </div>
                    </form>
                    {problem ? (
                        <p role='alert' data-slot='harness-problem' className='text-[13px]' style={{ color: 'var(--v2-block)' }}>
                            {problem}
                        </p>
                    ) : null}
                    {context ? (
                        <section data-slot='harness-setup' aria-labelledby='harness-setup-toggle' className='rounded-[12px] border p-3' style={{ borderColor: 'var(--v2-rule)' }}>
                            <h2 className='m-0'>
                                <button
                                    id='harness-setup-toggle'
                                    type='button'
                                    aria-expanded={setupOpen}
                                    aria-controls='harness-setup-panels'
                                    className='w-full text-left text-[14px] font-semibold'
                                    onClick={() => setSetupOpen((o) => !o)}
                                >
                                    {C.setup}
                                </button>
                            </h2>
                            <div id='harness-setup-panels' hidden={!setupOpen} className='mt-3 flex flex-col gap-4'>
                                {selected ? (
                                    <SignInPanel context={context} selected={selected} signIn={signIn} keyHere={keyHere} running={running} dispatch={dispatch} onMessage={setProblem} />
                                ) : null}
                                <ProjectPanel
                                    context={context}
                                    project={snapshot.harness.project}
                                    refused={snapshot.harness.projectRefused}
                                    running={running}
                                    dispatch={dispatch}
                                    onMessage={setProblem}
                                />
                                {selected ? <McpPanel context={context} selected={selected} mcp={snapshot.harness.mcp} running={running} dispatch={dispatch} onMessage={setProblem} /> : null}
                            </div>
                        </section>
                    ) : null}
                    {context ? (
                        <div data-slot='harness-history'>
                            <HistoryPanel context={context} selected={selected} history={history} running={running} dispatch={dispatch} onMessage={setProblem} />
                        </div>
                    ) : null}
                </div>
            </div>
        </section>
    )
})

/**
 * How the conversation stands, in words the C19–C22 Harness used (R5): a
 * failure names itself; a stopped, cut, failed or refused answer is never
 * shown as complete; an engine that could not be removed says so.
 */
function outcomeOf(c: ConversationView): { tone: VerdictTone; word: string; text: string } | null {
    if (c.phase === 'failed') return { tone: 'blocked', word: C.outcome.failed, text: c.failure ? HARNESS_FAILURE_TEXT[c.failure] : conversationText(c) }
    if (c.cleanup === 'failed') return { tone: 'blocked', word: C.outcome.cleanup, text: HARNESS_FAILURE_TEXT.cleanupFailed }
    if (c.phase === 'ended') return { tone: 'info', word: C.outcome.ended, text: conversationText(c) }
    if (c.lastTurn && c.lastTurn !== 'completed' && c.phase === 'ready') {
        return { tone: c.lastTurn === 'failed' ? 'blocked' : 'flagged', word: C.outcome.turn[c.lastTurn] ?? c.lastTurn, text: TURN_TEXT[c.lastTurn] }
    }
    return null
}

function Transcript({ entries }: { entries: HarnessEntry[] }) {
    return (
        <ol className='flex flex-col gap-3'>
            {entries.map((e) => (
                <li key={e.id} data-slot='harness-entry' data-role={e.role} data-state={e.state} className='flex flex-col gap-1'>
                    <span className='text-[12px] font-medium' style={{ color: 'var(--v2-soft)' }}>
                        {C.role[e.role] ?? e.role}
                        {C.entryState[e.state] ? ` · ${C.entryState[e.state]}` : ''}
                    </span>
                    <p className='text-[13px] whitespace-pre-wrap'>{e.text}</p>
                </li>
            ))}
        </ol>
    )
}

/**
 * One decision, as the core classified it (R1): a read-only call is decided
 * here; anything that may change things says what, and approving it opens
 * the system confirmation, which alone lets it run.
 */
function Decision({ approval: a, busy, onDecide }: { approval: HarnessApproval; busy: boolean; onDecide: (allow: boolean) => void }) {
    const what = a.server && a.tool ? `${a.server}.${a.tool}` : (a.command ?? a.files.join(', '))
    // Absent (a snapshot before C32) counts as may-change: never a false assurance.
    const readOnly = a.effect === 'readOnly'
    return (
        <section aria-label={C.decision} data-slot='harness-decision' data-kind={a.kind} data-effect={a.effect ?? 'mayChange'} className='flex flex-col gap-2 rounded-[12px] border p-3' style={{ borderColor: 'var(--v2-flag)' }}>
            <Verdict tone={readOnly ? 'info' : 'flagged'} word={readOnly ? C.readOnlyWord : C.mayChangeWord} />
            <p className='text-[13px]'>{C.asks(C.kind[a.kind] ?? a.kind, what)}</p>
            {a.arguments ? (
                <pre className='overflow-x-auto rounded-[8px] p-2 text-[12px]' style={{ background: 'var(--v2-side)' }}>
                    {a.arguments}
                </pre>
            ) : null}
            {a.cwd ? <p className='text-[12px]'>{C.inFolder(a.cwd)}</p> : null}
            {a.reason ? <p className='text-[12px]'>{a.reason}</p> : null}
            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                {readOnly ? (C.readOnlyNote[a.kind] ?? C.readOnlyNote.desktopRead) : (C.mayChangeNote[a.kind] ?? C.mayChangeNote.mcpTool)}
            </p>
            <div className='flex flex-wrap gap-2'>
                <button
                    type='button'
                    disabled={busy}
                    className='h-8 rounded-[7px] px-3 text-[13px] font-semibold disabled:opacity-60'
                    style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}
                    onClick={() => onDecide(true)}
                >
                    {readOnly ? C.approveOnce : C.approveNative}
                </button>
                <button type='button' disabled={busy} className='h-8 rounded-[7px] border px-3 text-[13px] disabled:opacity-60' style={{ borderColor: 'var(--v2-rule)' }} onClick={() => onDecide(false)}>
                    {C.decline}
                </button>
            </div>
        </section>
    )
}

function applicationOf(p: ProposalView): string | null {
    try {
        const body = JSON.parse(p.draft.body) as { cust_app?: unknown }
        return typeof body.cust_app === 'string' ? body.cust_app : null
    } catch {
        return null
    }
}

/** The Harness's proposals as follow-ups: each opens only as the operator's own review, and only while its conversation runs. */
function FollowUps({
    proposals,
    rotations,
    busy,
    open,
    onOpen,
    onDismiss,
}: {
    proposals: ProposalView[]
    rotations: RotationView[]
    busy: boolean
    open: boolean
    onOpen: (p: ProposalView) => void
    onDismiss: (p: ProposalView) => void
}) {
    return (
        <section aria-label={C.followUps} data-slot='harness-follow-ups' className='flex flex-col gap-2 rounded-[12px] border p-3' style={{ borderColor: 'var(--v2-rule)' }}>
            <h2 className='text-[14px] font-semibold'>{C.followUps}</h2>
            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                {open ? C.followUpNote : C.followUpEnded}
            </p>
            <ul className='flex flex-col gap-2'>
                {proposals.map((p) => {
                    // A key rotation's hand-off says where the rotation stands (I7).
                    const app = p.draft.kind === 'apiKeys' && p.draft.action === 'version' ? applicationOf(p) : null
                    const rotation = app ? (rotations.find((r) => r.application === app) ?? null) : null
                    const stateTone: VerdictTone = p.state === 'staged' ? 'info' : p.state === 'inReview' ? 'flagged' : 'allowed'
                    return (
                        <li key={p.id} data-slot='follow-up' data-kind={p.draft.kind} data-action={p.draft.action} data-state={p.state} className='flex flex-col gap-1 text-[13px]'>
                            <span className='font-medium'>{p.summary}</span>
                            {app ? (
                                <span data-slot='follow-up-rotation' className='text-[12px]'>
                                    {rotation ? (C.rotation[rotation.stage] ?? rotation.stage) : C.rotation.none}
                                </span>
                            ) : null}
                            {p.state !== 'staged' ? <Verdict tone={stateTone} word={p.state === 'inReview' ? C.inReview : p.state === 'closed' ? C.closed : C.dismissed} /> : null}
                            {open && (p.state === 'staged' || p.state === 'inReview') ? (
                                <div className='flex flex-wrap gap-2'>
                                    <button
                                        type='button'
                                        disabled={busy}
                                        className='h-8 rounded-[7px] px-3 text-[13px] font-semibold disabled:opacity-60'
                                        style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}
                                        onClick={() => onOpen(p)}
                                    >
                                        {p.state === 'staged' ? C.openReview : C.goToReview}
                                    </button>
                                    {p.state === 'staged' ? (
                                        <button type='button' disabled={busy} className='h-8 rounded-[7px] border px-3 text-[13px] disabled:opacity-60' style={{ borderColor: 'var(--v2-rule)' }} onClick={() => onDismiss(p)}>
                                            {C.dismiss}
                                        </button>
                                    ) : null}
                                </div>
                            ) : null}
                        </li>
                    )
                })}
            </ul>
        </section>
    )
}
