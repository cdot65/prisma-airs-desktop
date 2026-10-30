/**
 * The Harness in the new shell (C32, screens/harness.md) over the scripted
 * native transport, with the three lifecycle fixtures from the contracts
 * package (blocks, mcp, keys): each read-only tool call is decided in the
 * page and its approve, decline, end and follow-up paths dispatch exactly
 * the core's intents (V2-T17); the keys lifecycle never dispatches a
 * mutation (V2-T18); a fresh tenant's first send starts a conversation
 * (V2-T13, I12); contextual navigation into an ended conversation shows the
 * hint; the prerequisites checklist stays until satisfied.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { DeliveryOutcomeUnknown, DesktopClient, type ConversationView, type HarnessHistoryView, type HarnessMcpView, type HarnessSignInView, type Intent, type McpServerView, parseShellSnapshot, type RotationView, type SavedConversationView, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import blocks from '@prisma-airs/desktop-contracts/fixtures/wire/harness-lifecycle-blocks.json'
import keys from '@prisma-airs/desktop-contracts/fixtures/wire/harness-lifecycle-keys.json'
import mcp from '@prisma-airs/desktop-contracts/fixtures/wire/harness-lifecycle-mcp.json'
import { App } from '../../app'
import { scriptedNative, tenant } from '../../test/native'
import { fxSwitcher } from '../fixtures'
import { resetShellStore, useShellStore } from '../store'

interface Lifecycle {
    lifecycle: string
    question: string
    steps: { asking: ConversationView; approved: ConversationView; declined: ConversationView; ended: ConversationView }
    saved: SavedConversationView
}
const LIFECYCLES = [blocks, mcp, keys] as unknown as Lifecycle[]

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const WS = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8297', name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }
const REF = 'gw-aaaaaaaaaaaaaaaaaaaaaaaa'

function fxHarness(conversation: ConversationView | null, over: Partial<ShellSnapshot> = {}, saved: SavedConversationView[] = []): ShellSnapshot {
    const base = fxSwitcher()
    const nw = tenant({
        id: 'tn-0123456789abcdef',
        label: 'Northwind Health',
        tsgId: '1762204418',
        management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1762204418.iam.panserviceaccount.com', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' },
        inference: { state: 'ready', mode: 'keychain', failure: null, target: { workspace: WS, endpoint: 'https://gateway.example/rt/v1' } },
    })
    return {
        ...base,
        tenants: [nw, base.tenants[1]!],
        active: { tenant: nw.id, context: CONTEXT },
        activeFeature: 'harness',
        gateway: {
            workspaces: [WS],
            complete: true,
            selected: { reference: REF, workspace: WS, restored: false },
            remembered: null,
            rememberedMissing: false,
            local: { state: 'ready', failure: null, drafts: [], records: [], playground: [], runs: [], files: [], problem: null, unsaved: false },
            playground: null,
            files: null,
        },
        harness: {
            ...base.harness,
            engine: { ...base.harness.engine, available: true, runtimeVersion: '0.104.0' },
            conversation,
            history: { local: false, state: 'ready', conversations: saved, unreadable: [], failure: null, full: false, opened: null, busy: false, problem: null, export: null },
        },
        ...over,
    }
}

async function launch(snapshot: ShellSnapshot) {
    const native = scriptedNative(snapshot)
    // Each start is a new operation (op-77, op-78, …), as the core issues them.
    let started = 76
    native.respond = (intent: Intent) =>
        intent.kind === 'startHarness'
            ? { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-9', operation: `op-${++started}` } }
            : { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } }
    const user = userEvent.setup()
    const view = render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('heading', { level: 1, name: 'Harness' })
    return { native, user, view }
}

const kinds = (dispatched: Intent[]) => dispatched.map((i) => i.kind)

/** A saved conversation opened for reading, as the core shows it. */
const opened = (saved: SavedConversationView, entries: ConversationView['entries'], opening: string | null) => ({
    id: saved.id,
    title: saved.title,
    updated: saved.updated,
    entries,
    source: saved.source,
    notContinuable: null,
    ...(opening === null ? {} : { opening }),
})
/** The core's receipts for settled opens are ordered: each test's start after any earlier one. */
let openSequence = 100
/** The history after an open of `view` settled, in one update (no separate busy step). */
const withOpened = (snapshot: ShellSnapshot, view: ReturnType<typeof opened>): ShellSnapshot => ({
    ...snapshot,
    harness: { ...snapshot.harness, history: { ...snapshot.harness.history!, opened: view, busy: false, lastOpen: { id: view.id, sequence: ++openSequence, failure: null } } },
})
/** The history after an open of `id` failed with `failure`, in one update. */
const withOpenFailed = (snapshot: ShellSnapshot, id: string, failure: 'unreadable' | 'missing'): ShellSnapshot => ({
    ...snapshot,
    harness: { ...snapshot.harness, history: { ...snapshot.harness.history!, busy: false, problem: failure, lastOpen: { id, sequence: ++openSequence, failure } } },
})
/** A long opening question over several lines: its title is a shortened single line. */
const LONG_OPENING = `Investigate the claims-triage blocks from last night.\nFor each blocked prompt, name the policy, the topic and the application, ${'and say whether the block was expected, '.repeat(3)}then summarise.`
const startHarnessCalls = (dispatched: Intent[]) => dispatched.filter((i) => i.kind === 'sendHarness' || i.kind === 'startHarness')

type Entry = ConversationView['entries'][number]
const op = (id: string, text: string): Entry => ({ id, role: 'operator', text, state: 'complete' })
const answer = (id: string, text: string): Entry => ({ id, role: 'assistant', text, state: 'complete' })
/** Appends as the core does: its oldest entries give way past 200 entries or 1 MiB (session.rs `push`). */
function pushed(entries: Entry[], ...added: Entry[]): Entry[] {
    const out = [...entries]
    for (const e of added) {
        out.push(e)
        while (out.length > 200 || out.reduce((n, x) => n + new TextEncoder().encode(x.text).length, 0) > 1024 * 1024) out.shift()
    }
    return out
}
/** The send IDs the page dispatched, in order (a client resend repeats its send's ID). */
const sendIds = (dispatched: Intent[]) => [...new Set(dispatched.flatMap((i) => (i.kind === 'sendHarness' && i.submission ? [i.submission] : [])))]
/** The conversation as the core shows it once it took these sends (ID, the message each became). */
const taking = (conversation: ConversationView, ...taken: [string, string][]): ConversationView => ({ ...conversation, submissions: taken.map(([submission, entry]) => ({ submission, entry, state: 'delivered' as const })) })
/** Every send's receipt is lost from now on (the client's one resend too); returns the previous responder. */
function lose(native: Awaited<ReturnType<typeof launch>>['native']) {
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind === 'sendHarness') throw new DeliveryOutcomeUnknown('lost')
        return respond(intent)
    }
    return respond
}
/** A full transcript: 100 questions and 100 answers, with `repeat` asked once already as question 60. */
const full = (repeat: string): Entry[] => Array.from({ length: 200 }, (_, i) => (i % 2 === 0 ? op(`u-${i / 2 + 1}`, i === 118 ? repeat : `Question ${i / 2 + 1}`) : answer(`a-${(i + 1) / 2}`, `Answer ${(i + 1) / 2}`)))
const MUTATIONS = ['requestApply', 'reviewChange', 'acknowledgeDeployment', 'revealSecret', 'confirmHarnessWrite']

beforeEach(() => resetShellStore())

describe('v2 Harness', () => {
    it('ships lifecycle fixtures the core contract accepts at every step', () => {
        for (const l of LIFECYCLES) {
            for (const [step, conversation] of Object.entries(l.steps)) {
                expect(() => parseShellSnapshot(fxHarness(conversation)), `${l.lifecycle}/${step}`).not.toThrow()
            }
            // Each lifecycle read is one of the desktop's own read tools: read-only, decided in the page.
            expect(l.steps.asking.approvals.every((a) => a.kind === 'desktopRead' && a.effect === 'readOnly' && a.server === 'airs_desktop')).toBe(true)
        }
    })

    for (const l of LIFECYCLES) {
        it(`${l.lifecycle}: the read-only call is approved in the page, its follow-up opens only a review, and ending seals the conversation (V2-T17)`, async () => {
            const { native, user } = await launch(fxHarness(l.steps.asking))
            const decision = await screen.findByRole('region', { name: 'Decision needed' })
            const a = l.steps.asking.approvals[0]!
            expect(within(decision).getByText(`The Harness asks to read the tenant with ${a.server}.${a.tool}`)).toBeInTheDocument()
            expect(within(decision).getByText('Read-only')).toBeInTheDocument()
            expect(decision).toHaveAttribute('data-effect', 'readOnly')
            expect(within(decision).getByText(a.arguments!)).toBeInTheDocument()
            await user.click(within(decision).getByRole('button', { name: 'Approve once' }))
            expect(native.dispatched).toContainEqual({ kind: 'decideHarnessApproval', conversation: l.steps.asking.id, approval: a.id, allow: true })
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: l.steps.approved } })
            expect(screen.queryByRole('region', { name: 'Decision needed' })).toBeNull()
            const answer = l.steps.approved.entries.find((e) => e.role === 'assistant')!
            expect(await screen.findByText(answer.text)).toBeInTheDocument()
            // The follow-up opens the operator's own review, then Administration.
            const followUps = screen.getByRole('region', { name: 'Follow-ups' })
            const p = l.steps.approved.proposals[0]!
            expect(within(followUps).getByText(p.summary)).toBeInTheDocument()
            await user.click(within(followUps).getByRole('button', { name: 'Open the review' }))
            expect(native.dispatched).toContainEqual({ kind: 'reviewHarnessProposal', conversation: l.steps.approved.id, proposal: 'P-1' })
            await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'navigate', feature: 'administration' }))
            expect(kinds(native.dispatched).filter((k) => MUTATIONS.includes(k))).toEqual([])
            // Ending seals it: the rail marks it ended and names its saved transcript.
            await native.push({ ...native.current, activeFeature: 'harness' })
            await user.click(screen.getByRole('button', { name: 'End conversation' }))
            expect(native.dispatched).toContainEqual({ kind: 'endHarness', conversation: l.steps.approved.id })
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: l.steps.ended } })
            await waitFor(() => expect(document.querySelector('[data-slot="harness-outcome"]')).toHaveAttribute('data-tone', 'info'))
            expect(document.querySelector('[data-slot="harness-saved-status"]')?.textContent).toContain(`(${l.steps.ended.historyId.slice(0, 8)})`)
            // Its follow-ups can no longer be opened.
            expect(within(screen.getByRole('region', { name: 'Follow-ups' })).queryByRole('button', { name: 'Open the review' })).toBeNull()
            const rail = screen.getByRole('navigation', { name: 'Conversations' })
            expect(rail.querySelector('[data-slot="rail-conversation"]')).toHaveAttribute('data-state', 'ended')
            expect(within(rail).getByText('Ended')).toBeInTheDocument()
        })

        it(`${l.lifecycle}: declining records the decision and the assistant's declined text, with no follow-up (V2-T17)`, async () => {
            const { native, user } = await launch(fxHarness(l.steps.asking))
            const decision = await screen.findByRole('region', { name: 'Decision needed' })
            await user.click(within(decision).getByRole('button', { name: 'Decline' }))
            expect(native.dispatched).toContainEqual({ kind: 'decideHarnessApproval', conversation: l.steps.asking.id, approval: l.steps.asking.approvals[0]!.id, allow: false })
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: l.steps.declined } })
            const declined = l.steps.declined.entries.find((e) => e.role === 'assistant')!
            expect(await screen.findByText(declined.text)).toBeInTheDocument()
            expect(document.querySelector('[data-slot="harness-entry"][data-state="declined"]')).toHaveTextContent('Declined')
            expect(screen.queryByRole('region', { name: 'Follow-ups' })).toBeNull()
        })
    }

    it('keys: the Harness refuses to rotate and hands off; the follow-up follows the rotation stage and never dispatches a mutation (V2-T18)', async () => {
        const l = LIFECYCLES.find((x) => x.lifecycle === 'keys')!
        const { native, user } = await launch(fxHarness(l.steps.approved))
        expect(await screen.findByText(/rotation is a write; it goes through Administration and a review/)).toBeInTheDocument()
        const followUp = screen.getByRole('region', { name: 'Follow-ups' })
        expect(within(followUp).getByText('No rotation started yet: the review issues the additional key.')).toBeInTheDocument()
        const rotation = (stage: RotationView['stage']): RotationView => ({
            application: 'support-copilot',
            oldKey: { id: 'key-1', name: 'Support key' },
            newKey: stage === 'issuanceUnconfirmed' ? null : { id: 'key-2', name: 'Support key 2' },
            stage,
            issuedBy: null,
            issuedAt: 1_793_230_100,
            deploymentAcknowledgedAt: stage === 'acknowledged' || stage === 'complete' ? 1_793_230_200 : null,
            oldKeyRemovedAt: stage === 'complete' ? 1_793_230_300 : null,
            removalRejected: null,
            unsaved: false,
        })
        await native.push({ ...native.current, rotations: [rotation('resumed')] })
        expect(within(followUp).getByText('Additional key issued · finish the rotation in Administration')).toBeInTheDocument()
        await native.push({ ...native.current, rotations: [rotation('complete')] })
        expect(within(followUp).getByText('Old key removed · rotation complete')).toBeInTheDocument()
        await user.click(within(followUp).getByRole('button', { name: 'Open the review' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'navigate', feature: 'administration' }))
        const fromHarness = native.dispatched.filter((i) => MUTATIONS.includes(i.kind) || JSON.stringify(i).includes('"apiKeys"'))
        expect(fromHarness).toEqual([])
    })

    it('a fresh tenant shows no seeded conversation; its first send starts a user conversation and sends the message once it is ready (V2-T13, I12)', async () => {
        const { native, user } = await launch(fxHarness(null))
        expect(document.querySelectorAll('[data-slot="rail-conversation"]')).toHaveLength(0)
        await user.type(screen.getByRole('textbox', { name: 'Message' }), 'Which profiles block prompt injection?')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        expect(native.dispatched).toContainEqual({ kind: 'startHarness', context: CONTEXT, workspace: REF })
        expect(kinds(native.dispatched)).not.toContain('sendHarness')
        // A new conversation: nothing sent yet, so no opening.
        const { opening: _none, ...base } = LIFECYCLES[0]!.steps.declined
        const started = { ...base, id: 'op-77', phase: 'starting' as const, entries: [], lastTurn: null, savedAt: null }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: started } })
        expect(kinds(native.dispatched)).not.toContain('sendHarness')
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...started, phase: 'ready' } } })
        await waitFor(() => expect(native.dispatched).toContainEqual(expect.objectContaining({ kind: 'sendHarness', conversation: 'op-77', prompt: 'Which profiles block prompt injection?' })))
        expect(kinds(native.dispatched).filter((k) => k === 'sendHarness')).toHaveLength(1)
        await native.push({
            ...native.current,
            harness: {
                ...native.current.harness,
                conversation: { ...started, phase: 'answering', opening: 'Which profiles block prompt injection?', entries: [{ id: 'u-1', role: 'operator', text: 'Which profiles block prompt injection?', state: 'complete' }] },
            },
        })
        expect(within(screen.getByRole('navigation', { name: 'Conversations' })).getByText('Which profiles block prompt injection?')).toBeInTheDocument()
    })

    it('sending after a conversation ended clears it and starts a new one', async () => {
        const l = LIFECYCLES[0]!
        const { native, user } = await launch(fxHarness(l.steps.ended))
        await user.type(screen.getByRole('textbox', { name: 'Message' }), 'And yesterday?')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'startHarness', context: CONTEXT, workspace: REF }))
        const order = kinds(native.dispatched)
        expect(order.indexOf('clearHarness')).toBeLessThan(order.indexOf('startHarness'))
        expect(native.dispatched).toContainEqual({ kind: 'clearHarness', conversation: l.steps.ended.id })
    })

    it('contextual navigation into an ended conversation shows the hint; it opens the saved transcript or prefills a new investigation, and clears on navigation', async () => {
        const l = LIFECYCLES[1]!
        const { native, user } = await launch(fxHarness(null, {}, [l.saved]))
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: l.saved.id })
        const hint = await waitFor(() => {
            const found = document.querySelector('[data-slot="harness-hint"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        expect(hint).toHaveAttribute('role', 'status')
        await user.click(within(hint).getByRole('button', { name: 'Open the saved transcript' }))
        expect(native.dispatched).toContainEqual({ kind: 'openHarnessHistory', context: CONTEXT, workspace: REF, id: l.saved.id })
        expect(document.querySelector('[data-slot="harness-hint"]')).toBeNull()
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: l.saved.id })
        const again = await waitFor(() => {
            const found = document.querySelector('[data-slot="harness-hint"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        await user.click(within(again).getByRole('button', { name: 'Start a new investigation' }))
        // Its opening question is read from the saved record, then prefilled.
        await native.push(withOpened(native.current, opened(l.saved, l.steps.ended.entries, l.question)))
        await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue(l.question))
        // Any navigation clears it.
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: l.saved.id })
        await waitFor(() => expect(document.querySelector('[data-slot="harness-hint"]')).not.toBeNull())
        await native.push({ ...native.current, activeFeature: 'home' })
        await native.push({ ...native.current, activeFeature: 'harness' })
        expect(document.querySelector('[data-slot="harness-hint"]')).toBeNull()
    })

    it('a contextual restart of the ended conversation prefills its whole opening question, never its shortened title; a draft is kept and nothing is sent (attempt-12 R1)', async () => {
        const l = LIFECYCLES[0]!
        expect(LONG_OPENING.length).toBeGreaterThan(160)
        const entries = l.steps.ended.entries.map((e) => (e.id === 'u-1' ? { ...e, text: LONG_OPENING } : e))
        const ended = { ...l.steps.ended, entries, opening: LONG_OPENING }
        const { native, user } = await launch(fxHarness(ended))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'A draft of my own')
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: ended.historyId })
        const hint = await waitFor(() => {
            const found = document.querySelector('[data-slot="harness-hint"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        await user.click(within(hint).getByRole('button', { name: 'Start a new investigation' }))
        expect(box()).toHaveValue(LONG_OPENING)
        expect(within(screen.getByRole('region', { name: 'Not sent' })).getByText('A draft of my own')).toBeInTheDocument()
        await new Promise((r) => setTimeout(r, 50))
        expect(startHarnessCalls(native.dispatched)).toEqual([])
    })

    it('a contextual restart of an archived conversation reads its record for the whole opening question; a draft is kept and nothing is sent (attempt-12 R1)', async () => {
        const l = LIFECYCLES[2]!
        const title = `${LONG_OPENING.replace(/\s+/g, ' ').slice(0, 79)}…`
        const saved = { ...l.saved, title }
        const { native, user } = await launch(fxHarness(null, {}, [saved]))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'Unrelated draft')
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: saved.id })
        const hint = await waitFor(() => {
            const found = document.querySelector('[data-slot="harness-hint"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        await user.click(within(hint).getByRole('button', { name: 'Start a new investigation' }))
        expect(native.dispatched).toContainEqual({ kind: 'openHarnessHistory', context: CONTEXT, workspace: REF, id: saved.id })
        // Nothing is filled in while the record is read: never the title in its place.
        expect(within(hint).getByRole('button', { name: 'Reading its opening question…' })).toHaveAttribute('aria-busy', 'true')
        expect(box()).toHaveValue('Unrelated draft')
        await native.push(withOpened(native.current, opened(saved, l.steps.ended.entries, LONG_OPENING)))
        await waitFor(() => expect(box()).toHaveValue(LONG_OPENING))
        expect(within(screen.getByRole('region', { name: 'Not sent' })).getByText('Unrelated draft')).toBeInTheDocument()
        await new Promise((r) => setTimeout(r, 50))
        expect(startHarnessCalls(native.dispatched)).toEqual([])
    })

    it('says when the opening question is not known, or its record could not be read, and never substitutes the title (attempt-12 R1)', async () => {
        const l = LIFECYCLES[1]!
        // Restored from a transcript that no longer held its first message: no opening.
        const { opening: _dropped, ...ended } = { ...l.steps.ended, entries: l.steps.ended.entries.filter((e) => e.id !== 'u-1') }
        const { native, user, view } = await launch(fxHarness(ended))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: ended.historyId })
        let hint = await waitFor(() => {
            const found = document.querySelector('[data-slot="harness-hint"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        await user.click(within(hint).getByRole('button', { name: 'Start a new investigation' }))
        expect(box()).toHaveValue('')
        expect(screen.getByText('The opening question of that investigation is no longer in its transcript, so it was not filled in. Nothing was sent.')).toBeInTheDocument()
        view.unmount()
        // An archived one whose record cannot be read: nothing is started.
        resetShellStore()
        const second = await launch(fxHarness(null, {}, [l.saved]))
        const respond = second.native.respond
        second.native.respond = (intent: Intent) => (intent.kind === 'openHarnessHistory' ? { outcome: 'refusal', refusal: 'busy' } : respond(intent))
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: l.saved.id })
        hint = await waitFor(() => {
            const found = document.querySelector('[data-slot="harness-hint"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        await second.user.click(within(hint).getByRole('button', { name: 'Start a new investigation' }))
        expect(await screen.findByText('The saved investigation could not be read, so its opening question is not known. No new investigation was started.')).toBeInTheDocument()
        expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('')
        expect(document.querySelector('[data-slot="harness-hint"]')).not.toBeNull()
        // Read, but saved before the opening was kept and without its first message: said so, never the title.
        second.native.respond = respond
        await second.user.click(within(document.querySelector('[data-slot="harness-hint"]') as HTMLElement).getByRole('button', { name: 'Start a new investigation' }))
        await second.native.push(withOpened(second.native.current, opened(l.saved, l.steps.ended.entries.filter((e) => e.id !== 'u-1'), null)))
        expect(await screen.findByText('The opening question of that investigation is no longer in its transcript, so it was not filled in. Nothing was sent.')).toBeInTheDocument()
        expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('')
        expect(startHarnessCalls([...native.dispatched, ...second.native.dispatched])).toEqual([])
    })

    it('a repeated read failure of an archived record settles each read by its receipt: loading ends, the failure is said, retry works, the draft stays and nothing starts (attempt-13 R1)', async () => {
        const l = LIFECYCLES[2]!
        const base = fxHarness(null, {}, [l.saved])
        // The record failed to read before: the same problem is already shown.
        const failedBefore = withOpenFailed(base, l.saved.id, 'unreadable')
        const { native, user } = await launch(failedBefore)
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'Keep me')
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: l.saved.id })
        const hint = () => document.querySelector('[data-slot="harness-hint"]') as HTMLElement
        await waitFor(() => expect(hint()).not.toBeNull())
        await user.click(within(hint()).getByRole('button', { name: 'Start a new investigation' }))
        expect(within(hint()).getByRole('button', { name: 'Reading its opening question…' })).toBeInTheDocument()
        // It fails exactly as before, busy and settled in one update: the receipt settles it.
        await native.push(withOpenFailed(native.current, l.saved.id, 'unreadable'))
        expect(await screen.findByText('The saved investigation could not be read, so its opening question is not known. No new investigation was started.')).toBeInTheDocument()
        expect(within(hint()).getByRole('button', { name: 'Start a new investigation' })).toHaveAttribute('aria-busy', 'false')
        expect(box()).toHaveValue('Keep me')
        // A retry asks again and, read this time, prefills the opening question whole.
        await user.click(within(hint()).getByRole('button', { name: 'Start a new investigation' }))
        expect(native.dispatched.filter((i) => i.kind === 'openHarnessHistory')).toHaveLength(2)
        await native.push(withOpened(native.current, opened(l.saved, l.steps.ended.entries, LONG_OPENING)))
        await waitFor(() => expect(box()).toHaveValue(LONG_OPENING))
        expect(within(screen.getByRole('region', { name: 'Not sent' })).getByText('Keep me')).toBeInTheDocument()
        await new Promise((r) => setTimeout(r, 50))
        expect(startHarnessCalls(native.dispatched)).toEqual([])
    })

    it('a contextual arrival waits for the saved conversations to be listed: the hint appears once its record is, the draft stays and nothing starts (attempt-14 R1)', async () => {
        const l = LIFECYCLES[1]!
        const base = fxHarness(null)
        const listing = (state: 'loading' | 'ready' | 'failed', conversations: SavedConversationView[]): ShellSnapshot => ({
            ...base,
            harness: { ...base.harness, history: { ...base.harness.history!, state, conversations, failure: state === 'failed' ? 'unavailable' : null } },
        })
        const { native, user } = await launch(listing('loading', []))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'My draft')
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: l.saved.id })
        expect(await screen.findByText('Finding that investigation among the saved conversations…')).toBeInTheDocument()
        expect(document.querySelector('[data-slot="harness-hint"]')).toBeNull()
        // The list lands with the record: the hint and both actions appear.
        await native.push({ ...native.current, harness: listing('ready', [l.saved]).harness })
        const hint = await waitFor(() => {
            const found = document.querySelector('[data-slot="harness-hint"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        expect(within(hint).getByRole('button', { name: 'Open the saved transcript' })).toBeInTheDocument()
        expect(within(hint).getByRole('button', { name: 'Start a new investigation' })).toBeInTheDocument()
        expect(screen.queryByText('Finding that investigation among the saved conversations…')).toBeNull()
        expect(box()).toHaveValue('My draft')
        await new Promise((r) => setTimeout(r, 50))
        expect(native.dispatched.filter((i) => i.kind === 'openHarnessHistory')).toEqual([])
        expect(startHarnessCalls(native.dispatched)).toEqual([])
    })

    it('a pending contextual arrival is cancelled by navigation or another workspace, and says when its record is not listed or cannot be (attempt-14 R1)', async () => {
        const l = LIFECYCLES[1]!
        const base = fxHarness(null)
        const history = (state: 'loading' | 'ready' | 'failed', conversations: SavedConversationView[]) => ({
            ...base.harness.history!,
            state,
            conversations,
            failure: state === 'failed' ? ('unavailable' as const) : null,
        })
        const { native } = await launch({ ...base, harness: { ...base.harness, history: history('loading', []) } })
        const arrive = () => useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: l.saved.id })
        const listed = async (state: 'loading' | 'ready' | 'failed', conversations: SavedConversationView[]) =>
            native.push({ ...native.current, harness: { ...native.current.harness, history: history(state, conversations) } })
        // Navigation away and back: the pending arrival is gone.
        arrive()
        await screen.findByText('Finding that investigation among the saved conversations…')
        await native.push({ ...native.current, activeFeature: 'home' })
        await native.push({ ...native.current, activeFeature: 'harness' })
        await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toBeInTheDocument())
        await listed('ready', [l.saved])
        expect(document.querySelector('[data-slot="harness-hint"]')).toBeNull()
        expect(screen.queryByText('Finding that investigation among the saved conversations…')).toBeNull()
        // Another workspace while it is pending: cancelled too.
        await listed('loading', [])
        arrive()
        await screen.findByText('Finding that investigation among the saved conversations…')
        const other = { id: '26f7e90d-382a-4e78-b577-1b01eb5f8298', name: 'Team B', slug: 'team-b', scopeName: 'scope-team-b' }
        await native.push({
            ...native.current,
            gateway: { ...native.current.gateway, workspaces: [WS, other], selected: { reference: 'gw-bbbbbbbbbbbbbbbbbbbbbbbb', workspace: other, restored: false } },
            harness: { ...native.current.harness, history: history('ready', [l.saved]) },
        })
        await new Promise((r) => setTimeout(r, 50))
        expect(document.querySelector('[data-slot="harness-hint"]')).toBeNull()
        expect(screen.queryByText('Finding that investigation among the saved conversations…')).toBeNull()
        // Back on the first workspace: listed without it, or not listable, it is said so.
        await native.push({ ...native.current, gateway: base.gateway, harness: { ...native.current.harness, history: history('loading', []) } })
        arrive()
        await screen.findByText('Finding that investigation among the saved conversations…')
        await listed('ready', [])
        expect(await screen.findByText("That investigation is not among this workspace's saved conversations.")).toBeInTheDocument()
        await listed('loading', [])
        arrive()
        await screen.findByText('Finding that investigation among the saved conversations…')
        await listed('failed', [])
        expect(await screen.findByText("This workspace's saved conversations could not be listed, so that investigation cannot be shown.")).toBeInTheDocument()
        expect(document.querySelector('[data-slot="harness-hint"]')).toBeNull()
        expect(startHarnessCalls(native.dispatched)).toEqual([])
    })

    it('choosing New investigation while an archived record is read retires the contextual restart: its late success or failure changes neither the draft nor the choice (attempt-15 R1)', async () => {
        const l = LIFECYCLES[0]!
        const m = LIFECYCLES[1]!
        const { native, user } = await launch(fxHarness(l.steps.approved, {}, [m.saved]))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        const hint = () => document.querySelector('[data-slot="harness-hint"]') as HTMLElement | null
        for (const late of ['success', 'failure'] as const) {
            // The record is not open yet: it is read for its opening question.
            await native.push({ ...native.current, harness: { ...native.current.harness, history: { ...native.current.harness.history!, opened: null } } })
            useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: m.saved.id })
            await waitFor(() => expect(hint()).not.toBeNull())
            await user.click(within(hint()!).getByRole('button', { name: 'Start a new investigation' }))
            expect(within(hint()!).getByRole('button', { name: 'Reading its opening question…' })).toBeInTheDocument()
            // The operator chooses another investigation before the read lands, and types its question.
            await user.click(screen.getByRole('button', { name: 'New investigation' }))
            expect(hint()).toBeNull()
            await user.clear(box())
            await user.type(box(), `Replacement question (${late})`)
            expect(screen.getByText(/Sending ends the current conversation/)).toBeInTheDocument()
            await native.push(late === 'success' ? withOpened(native.current, opened(m.saved, m.steps.ended.entries, m.question)) : withOpenFailed(native.current, m.saved.id, 'unreadable'))
            await new Promise((r) => setTimeout(r, 50))
            expect(box()).toHaveValue(`Replacement question (${late})`)
            expect(screen.queryByRole('region', { name: 'Not sent' })).toBeNull()
            expect(screen.getByText(/Sending ends the current conversation/)).toBeInTheDocument()
            expect(screen.queryByText('The saved investigation could not be read, so its opening question is not known. No new investigation was started.')).toBeNull()
            expect(hint()).toBeNull()
        }
        expect(native.dispatched.filter((i) => i.kind === 'openHarnessHistory')).toHaveLength(2)
        expect(native.dispatched.filter((i) => ['sendHarness', 'startHarness', 'endHarness', 'clearHarness'].includes(i.kind))).toEqual([])
    })

    it('a read refused after the operator chose another investigation says nothing over it (attempt-16 S1)', async () => {
        const l = LIFECYCLES[0]!
        const m = LIFECYCLES[1]!
        const { native, user } = await launch(fxHarness(l.steps.approved, {}, [m.saved]))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        const respond = native.respond
        native.respond = (intent: Intent) => (intent.kind === 'openHarnessHistory' ? { outcome: 'refusal', refusal: 'busy' } : respond(intent))
        // The read's answer is held until the operator has moved on.
        let release: () => void = () => undefined
        const dispatch = native.transport.dispatch
        native.transport.dispatch = async (bytes) => {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
            if (intent.kind === 'openHarnessHistory') await new Promise<void>((resolve) => (release = resolve))
            return dispatch(bytes)
        }
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: m.saved.id })
        const hint = await waitFor(() => {
            const found = document.querySelector('[data-slot="harness-hint"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        await user.click(within(hint).getByRole('button', { name: 'Start a new investigation' }))
        await user.click(screen.getByRole('button', { name: 'New investigation' }))
        await user.type(box(), 'My replacement')
        release()
        native.transport.dispatch = dispatch
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'openHarnessHistory')).toHaveLength(1))
        await new Promise((r) => setTimeout(r, 50))
        expect(screen.queryByText(/could not be read|did not confirm reading|busy/)).toBeNull()
        expect(screen.queryByRole('alert')).toBeNull()
        expect(box()).toHaveValue('My replacement')
        expect(screen.getByText(/Sending ends the current conversation/)).toBeInTheDocument()
        expect(native.dispatched.filter((i) => ['sendHarness', 'startHarness', 'endHarness'].includes(i.kind))).toEqual([])
    })

    it('keeps the prerequisites checklist until every one is satisfied', async () => {
        const { native } = await launch(fxHarness(null, { gateway: { ...fxHarness(null).gateway, selected: null } }))
        const list = screen.getByRole('region', { name: 'Before you start' })
        const items = Array.from(list.querySelectorAll('[data-slot="prerequisite"]')).map((li) => [li.textContent?.includes('A Gateway workspace'), li.getAttribute('data-done')])
        expect(items).toContainEqual([true, 'false'])
        expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled()
        await native.push({ ...native.current, gateway: fxHarness(null).gateway })
        await waitFor(() => expect(screen.queryByRole('region', { name: 'Before you start' })).toBeNull())
    })

    it('keeps the C19–C22 controls: the project, sign-in and MCP panels, saved-conversation management, and the write confirmation after a renewal', async () => {
        const l = LIFECYCLES[0]!
        const confirming: ConversationView = { ...l.steps.approved, phase: 'confirmWrite', project: '/Users/demo/agent-demo', write: true, proposals: [] }
        const { native, user } = await launch(fxHarness(confirming, {}, [l.saved]))
        const setup = document.querySelector('[data-slot="harness-setup"]') as HTMLElement
        const toggle = within(setup).getByRole('button', { name: 'Project, sign-in and workspace tools' })
        expect(toggle).toHaveAttribute('aria-expanded', 'false')
        await user.click(toggle)
        expect(toggle).toHaveAttribute('aria-expanded', 'true')
        expect(within(setup).getByRole('button', { name: /Choose a folder/ })).toBeInTheDocument()
        expect(within(document.querySelector('[data-slot="harness-history"]') as HTMLElement).getByRole('region', { name: 'Saved conversations' })).toBeInTheDocument()
        const confirm = screen.getByRole('region', { name: 'Allow changes again?' })
        expect(within(confirm).getByText(/nothing is sent and your message returns to the box\. The folder access stays as it was chosen/)).toBeInTheDocument()
        await user.click(within(confirm).getByRole('button', { name: 'Not now' }))
        expect(native.dispatched).toContainEqual({ kind: 'confirmHarnessWrite', conversation: confirming.id, allow: false })
        await user.click(within(confirm).getByRole('button', { name: 'Allow changes' }))
        expect(native.dispatched).toContainEqual({ kind: 'confirmHarnessWrite', conversation: confirming.id, allow: true })
    })

    it('says when a decision may change things: approving asks for the system confirmation, never in-page consent alone (R1)', async () => {
        const l = LIFECYCLES[0]!
        const change: ConversationView = {
            ...l.steps.asking,
            approvals: [
                { id: 'aa0a1b2c3d4e5f60718293a4', kind: 'mcpTool', command: null, cwd: null, reason: null, files: [], server: 'ticketing', tool: 'close_ticket', arguments: '{"id":"T-1"}', effect: 'mayChange' },
            ],
        }
        const { native, user } = await launch(fxHarness(change))
        const decision = await screen.findByRole('region', { name: 'Decision needed' })
        expect(decision).toHaveAttribute('data-effect', 'mayChange')
        expect(within(decision).getByText('May change things')).toBeInTheDocument()
        expect(within(decision).getByText(/cannot tell whether it changes anything there\. Approving opens the system confirmation; only that lets it run/)).toBeInTheDocument()
        await user.click(within(decision).getByRole('button', { name: 'Approve…' }))
        // The core opens the native sheet from this intent; the page grants nothing itself.
        expect(native.dispatched).toContainEqual({ kind: 'decideHarnessApproval', conversation: change.id, approval: 'aa0a1b2c3d4e5f60718293a4', allow: true })
        // A snapshot written before C32 (no effect) is never shown as read-only.
        const legacy = { ...change, approvals: [{ ...change.approvals[0]!, effect: undefined }] } as unknown as ConversationView
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: legacy } })
        await waitFor(() => expect(screen.getByRole('region', { name: 'Decision needed' })).toHaveAttribute('data-effect', 'mayChange'))
    })

    it('keeps the first message until a conversation takes it: refused, failed startup and a replacement never lose it or send it elsewhere (R2)', async () => {
        const { native, user } = await launch(fxHarness(null))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'First question')
        // The start itself is refused: nothing queued, the text stays, Send works again.
        const respond = native.respond
        native.respond = (intent: Intent) => (intent.kind === 'startHarness' ? { outcome: 'refusal', refusal: 'busy' } : respond(intent))
        await user.click(screen.getByRole('button', { name: 'Send' }))
        expect(await screen.findByRole('alert')).toBeInTheDocument()
        expect(box()).toHaveValue('First question')
        expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled()
        // It starts, then fails: the message is not sent and stays in the box.
        native.respond = respond
        await user.click(screen.getByRole('button', { name: 'Send' }))
        const base = { ...LIFECYCLES[0]!.steps.declined, entries: [], lastTurn: null, savedAt: null, proposals: [] }
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(2))
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-77', phase: 'failed', failure: 'startFailed', cleanup: 'graceful' } } })
        expect(await screen.findByText('The conversation could not start, so your message was not sent. It is still in the box.')).toBeInTheDocument()
        expect(box()).toHaveValue('First question')
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
        // Started again, but another conversation appears: nothing is sent to it.
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(3))
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-91', phase: 'ready' } } })
        expect(await screen.findByText(/Another conversation started meanwhile/)).toBeInTheDocument()
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
        // The send itself refused: the text moves to the new conversation's box, unsent.
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: null } })
        native.respond = (intent: Intent) =>
            intent.kind === 'sendHarness' ? { outcome: 'refusal', refusal: 'busy' } : intent.kind === 'startHarness' ? { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-9', operation: 'op-95' } } : respond(intent)
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(4))
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-95', phase: 'ready' } } })
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toHaveLength(1))
        await waitFor(() => expect(box()).toHaveValue('First question'))
    })

    it('a new investigation while one runs ends it and sends to a different conversation; the contextual restart shows its opening question (R3)', async () => {
        const l = LIFECYCLES[0]!
        const { native, user } = await launch(fxHarness(l.steps.approved, {}, [LIFECYCLES[1]!.saved]))
        await user.click(screen.getByRole('button', { name: 'New investigation' }))
        expect(screen.getByText(/Sending ends the current conversation/)).toBeInTheDocument()
        await user.type(screen.getByRole('textbox', { name: 'Message' }), 'A different question')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        expect(native.dispatched).toContainEqual({ kind: 'endHarness', conversation: l.steps.approved.id })
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: l.steps.ended } })
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'startHarness', context: CONTEXT, workspace: REF }))
        expect(native.dispatched).toContainEqual({ kind: 'clearHarness', conversation: l.steps.ended.id })
        const next = { ...l.steps.declined, id: 'op-77', phase: 'ready' as const, entries: [], lastTurn: null, proposals: [] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: next } })
        await waitFor(() => expect(native.dispatched).toContainEqual(expect.objectContaining({ kind: 'sendHarness', conversation: 'op-77', prompt: 'A different question' })))
        // The contextual restart while a conversation runs: its question is visible in the box.
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: LIFECYCLES[1]!.saved.id })
        const hint = await waitFor(() => {
            const found = document.querySelector('[data-slot="harness-hint"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        await user.click(within(hint).getByRole('button', { name: 'Start a new investigation' }))
        const m = LIFECYCLES[1]!
        await native.push(withOpened(native.current, opened(m.saved, m.steps.ended.entries, m.question)))
        await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue(m.question))
        expect(screen.getByText(/Sending ends the current conversation/)).toBeInTheDocument()
    })

    it('a pending decision holds a new investigation back', async () => {
        const { user } = await launch(fxHarness(LIFECYCLES[0]!.steps.asking))
        await user.click(screen.getByRole('button', { name: 'New investigation' }))
        expect(await screen.findByText('Decide or decline the waiting request first.')).toBeInTheDocument()
        expect(screen.queryByText(/Sending ends the current conversation/)).toBeNull()
    })

    it('a pending decision holds a contextual restart back without losing its hint; an unconfirmed read is never called a failure', async () => {
        const m = LIFECYCLES[1]!
        const { native, user } = await launch(fxHarness(LIFECYCLES[0]!.steps.asking, {}, [m.saved]))
        const hint = () => document.querySelector('[data-slot="harness-hint"]') as HTMLElement | null
        useShellStore.getState().arrive({ tenant: 'tn-0123456789abcdef', feature: 'harness', resource: null, item: m.saved.id })
        await waitFor(() => expect(hint()).not.toBeNull())
        // Neither the hint's restart nor the rail's starts anything, reads anything or drops the hint.
        await user.click(within(hint()!).getByRole('button', { name: 'Start a new investigation' }))
        expect(await screen.findByText('Decide or decline the waiting request first.')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'New investigation' }))
        expect(hint()).not.toBeNull()
        expect(native.dispatched.filter((i) => i.kind === 'openHarnessHistory')).toEqual([])
        // Decided: the restart reads the record; its receipt is lost, which is said as not confirmed.
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...LIFECYCLES[0]!.steps.approved, proposals: [] } } })
        const respond = native.respond
        native.respond = (intent: Intent) => {
            if (intent.kind === 'openHarnessHistory') throw new DeliveryOutcomeUnknown('lost')
            return respond(intent)
        }
        await user.click(within(hint()!).getByRole('button', { name: 'Start a new investigation' }))
        expect(await screen.findByText('The desktop did not confirm reading the saved investigation, so its opening question is not known yet. No new investigation was started; try again.')).toBeInTheDocument()
        expect(screen.queryByText(/could not be read/)).toBeNull()
        expect(within(hint()!).getByRole('button', { name: 'Start a new investigation' })).toHaveAttribute('aria-busy', 'false')
        expect(native.dispatched.filter((i) => ['sendHarness', 'startHarness', 'endHarness'].includes(i.kind))).toEqual([])
    })

    it('returns an unsent message to the box with the sign-in failure, and resends nothing (R4)', async () => {
        const l = LIFECYCLES[0]!
        const { native } = await launch(fxHarness({ ...l.steps.approved, unsent: 'Please summarise again.', signInFailure: 'sessionExpired' }))
        expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Please summarise again.')
        const alert = document.querySelector('[data-slot="harness-unsent"]') as HTMLElement
        expect(alert).toHaveAttribute('role', 'alert')
        expect(alert.textContent).toMatch(/Your sign-in has expired/)
        expect(alert.textContent).toMatch(/Your last message was not sent and is back in the box\. Nothing was resent\./)
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
    })

    it('shows how the conversation stands: a failed start, a cut or failed answer, a failed cleanup and an unsaved transcript are never presented as complete or saved (R5)', async () => {
        const l = LIFECYCLES[0]!
        const { native } = await launch(fxHarness({ ...l.steps.approved, phase: 'failed', failure: 'startFailed', cleanup: 'graceful', savedAt: null }))
        const outcome = () => document.querySelector('[data-slot="harness-outcome"]') as HTMLElement
        expect(outcome()).toHaveAttribute('data-tone', 'blocked')
        expect(outcome()).toHaveAttribute('role', 'alert')
        expect(document.querySelector('[data-slot="harness-saved-status"]')?.textContent).toBe('Not saved yet.')
        for (const lastTurn of ['truncated', 'interrupted', 'failed'] as const) {
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...l.steps.approved, lastTurn } } })
            await waitFor(() => expect(outcome()).not.toBeNull())
            expect(outcome().getAttribute('data-tone')).toBe(lastTurn === 'failed' ? 'blocked' : 'flagged')
        }
        await native.push({
            ...native.current,
            harness: { ...native.current.harness, conversation: { ...l.steps.approved, entries: [{ id: 'u-1/a-9', role: 'assistant', text: 'Partial', state: 'truncated' }] } },
        })
        await waitFor(() => expect(document.querySelector('[data-slot="harness-entry"][data-state="truncated"]')).toHaveTextContent('cut at 256 KiB'))
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...l.steps.ended, cleanup: 'failed' } } })
        await waitFor(() => expect(outcome()).toHaveAttribute('data-tone', 'blocked'))
        expect(outcome().textContent).toMatch(/could not be stopped and removed/)
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...l.steps.ended, saveFailure: 'locked' } } })
        await waitFor(() => expect(document.querySelector('[data-slot="harness-saved-status"]')?.textContent).toMatch(/^Not saved: The Keychain is locked/))
    })

    it('keeps each draft in its tenant, workspace and conversation across navigation and a tenant round trip, sending nothing by itself (R6)', async () => {
        const l = LIFECYCLES[0]!
        const { native, user } = await launch(fxHarness(l.steps.approved))
        await user.type(screen.getByRole('textbox', { name: 'Message' }), 'Half a thought')
        await native.push({ ...native.current, activeFeature: 'administration' })
        await native.push({ ...native.current, activeFeature: 'harness' })
        expect(await screen.findByRole('textbox', { name: 'Message' })).toHaveValue('Half a thought')
        // Tenant B has its own (empty) scope; A's draft is back on return.
        const a = native.current
        const other = a.tenants[1]!
        await native.push({ ...a, active: { tenant: other.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' }, harness: { ...a.harness, conversation: null } })
        expect(await screen.findByRole('textbox', { name: 'Message' })).toHaveValue('')
        await native.push(a)
        expect(await screen.findByRole('textbox', { name: 'Message' })).toHaveValue('Half a thought')
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
    })

    it('keeps the box and the new-investigation choice by workspace ID across navigation and an A/B/A that retires the conversation (attempt-02/03 R1)', async () => {
        const l = LIFECYCLES[0]!
        const { native, user } = await launch(fxHarness(l.steps.approved))
        await user.click(screen.getByRole('button', { name: 'New investigation' }))
        await user.type(screen.getByRole('textbox', { name: 'Message' }), 'A new one')
        // Navigation away and back keeps the box and the choice.
        await native.push({ ...native.current, activeFeature: 'administration' })
        await native.push({ ...native.current, activeFeature: 'harness' })
        expect(await screen.findByRole('textbox', { name: 'Message' })).toHaveValue('A new one')
        expect(screen.getByText(/Sending ends the current conversation/)).toBeInTheDocument()
        // A/B/A as the core does it: B shows its own (empty) box; on return the conversation is retired
        // (none observed) and the context and workspace reference are reissued. The box is the workspace's.
        const a = native.current
        await native.push({ ...a, active: { tenant: a.tenants[1]!.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' }, harness: { ...a.harness, conversation: null } })
        expect(await screen.findByRole('textbox', { name: 'Message' })).toHaveValue('')
        const reissued = { ...a.gateway, selected: { ...a.gateway.selected!, reference: 'gw-cccccccccccccccccccccccc' } }
        await native.push({ ...a, active: { tenant: a.active!.tenant, context: 'cx-cccccccccccccccccccccccc' }, gateway: reissued, harness: { ...a.harness, conversation: null } })
        expect(await screen.findByRole('textbox', { name: 'Message' })).toHaveValue('A new one')
        expect(screen.getByText('Sending starts a new conversation.')).toBeInTheDocument()
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
    })

    it('retires a first send whose conversation ends before it is ready, or whose start is unconfirmed: text kept, controls usable, nothing sent (attempt-02 R2)', async () => {
        const { native, user } = await launch(fxHarness(null))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'Ended early')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(1))
        const base = { ...LIFECYCLES[0]!.steps.declined, entries: [], lastTurn: null, savedAt: null, proposals: [] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-77', phase: 'starting' } } })
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-77', phase: 'ended', stop: 'ended', cleanup: 'graceful' } } })
        expect(await screen.findByText('The conversation ended before it was ready, so your message was not sent. It is still in the box.')).toBeInTheDocument()
        expect(box()).toHaveValue('Ended early')
        expect(screen.queryByText('Starting a conversation…')).toBeNull()
        expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled()
        // Unconfirmed admission of the start: nothing queued, the text stays, Send works.
        const dispatch = native.transport.dispatch
        native.transport.dispatch = async () => {
            throw new Error('lost')
        }
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled())
        native.transport.dispatch = dispatch
        expect(box()).toHaveValue('Ended early')
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
    })

    it('clears only the submitted text: an edit made while the first send waits, or while a message is delivered, stays (attempt-02 R3)', async () => {
        const { native, user } = await launch(fxHarness(null))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'Submitted question')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(1))
        await user.type(box(), ' and a later edit')
        const base = { ...LIFECYCLES[0]!.steps.declined, entries: [], lastTurn: null, savedAt: null, proposals: [] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-77', phase: 'ready' } } })
        await waitFor(() => expect(native.dispatched).toContainEqual(expect.objectContaining({ kind: 'sendHarness', conversation: 'op-77', prompt: 'Submitted question' })))
        await waitFor(() => expect(box()).toHaveValue('Submitted question and a later edit'))
        // In a running conversation: delivery waits; an edit made meanwhile stays.
        await user.clear(box())
        await user.type(box(), 'Second')
        let release: () => void = () => undefined
        const dispatch = native.transport.dispatch
        native.transport.dispatch = async (bytes) => {
            await new Promise<void>((resolve) => (release = resolve))
            return dispatch(bytes)
        }
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await user.type(box(), ' more')
        release()
        native.transport.dispatch = dispatch
        await waitFor(() => expect(native.dispatched).toContainEqual(expect.objectContaining({ kind: 'sendHarness', conversation: 'op-77', prompt: 'Second' })))
        expect(box()).toHaveValue('Second more')
    })

    it('keeps both a returned message and the draft typed meanwhile: the message goes under Not sent with a way back into the box (attempt-02/03 R4)', async () => {
        const l = LIFECYCLES[0]!
        const { native, user } = await launch(fxHarness(l.steps.approved))
        await user.type(screen.getByRole('textbox', { name: 'Message' }), 'Typed meanwhile')
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...l.steps.approved, unsent: 'Please summarise again.', signInFailure: 'sessionExpired' } } })
        const alert = await waitFor(() => document.querySelector('[data-slot="harness-unsent"]') as HTMLElement)
        expect(alert.textContent).toMatch(/Your last message was not sent\. Nothing was resent; it is kept under Not sent\./)
        const notSent = screen.getByRole('region', { name: 'Not sent' })
        expect(within(notSent).getByText('Please summarise again.')).toBeInTheDocument()
        expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Typed meanwhile')
        await user.click(within(notSent).getByRole('button', { name: 'Put it in the box' }))
        expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Typed meanwhile\n\nPlease summarise again.')
        expect(screen.queryByRole('region', { name: 'Not sent' })).toBeNull()
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
    })


    it('keeps every edit across the starting snapshot: edits before and after it stay in the one box (attempt-03 R2)', async () => {
        const { native, user } = await launch(fxHarness(null))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'Question')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(1))
        await user.type(box(), ' first edit')
        const base = { ...LIFECYCLES[0]!.steps.declined, entries: [], lastTurn: null, savedAt: null, proposals: [] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-77', phase: 'starting' } } })
        expect(box()).toHaveValue('Question first edit')
        await user.type(box(), ' second edit')
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-77', phase: 'ready' } } })
        await waitFor(() => expect(native.dispatched).toContainEqual(expect.objectContaining({ kind: 'sendHarness', conversation: 'op-77', prompt: 'Question' })))
        await waitFor(() => expect(box()).toHaveValue('Question first edit second edit'))
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toHaveLength(1)
    })

    it('says truthfully where an unsent first message is: still in the box, or kept under Not sent when the box changed (attempt-03 R3)', async () => {
        const { native, user } = await launch(fxHarness(null))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'Original question')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(1))
        // The operator replaces the text while it starts; then another conversation appears.
        await user.clear(box())
        await user.type(box(), 'Something else')
        const base = { ...LIFECYCLES[0]!.steps.declined, entries: [], lastTurn: null, savedAt: null, proposals: [] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-91', phase: 'ready' } } })
        expect(await screen.findByText('Another conversation started meanwhile, so your message was not sent to it. It is kept under Not sent.')).toBeInTheDocument()
        expect(within(screen.getByRole('region', { name: 'Not sent' })).getByText('Original question')).toBeInTheDocument()
        expect(box()).toHaveValue('Something else')
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
        // Ending before ready, with the box unchanged: it is said to be in the box, and it is.
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-91', phase: 'ended', stop: 'ended', cleanup: 'graceful' } } })
        await user.clear(box())
        await user.type(box(), 'Third try')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(2))
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-78', phase: 'ending' } } })
        expect(await screen.findByText('The conversation ended before it was ready, so your message was not sent. It is still in the box.')).toBeInTheDocument()
        expect(box()).toHaveValue('Third try')
        // Once it has ended, the next Send starts anew.
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-78', phase: 'ended', stop: 'ended', cleanup: 'graceful' } } })
        await waitFor(() => expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled())
    })

    it('one Send starts at most one conversation and delivers once, even when the clear publishes before its receipt (attempt-03 R4)', async () => {
        const l = LIFECYCLES[0]!
        const { native, user } = await launch(fxHarness(l.steps.approved))
        // The core publishes the cleared state (no conversation) before it answers ClearHarness.
        const dispatch = native.transport.dispatch
        native.transport.dispatch = async (bytes) => {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
            if (intent.kind === 'clearHarness') {
                // Published first; the receipt follows a tick later.
                void native.push({ ...native.current, harness: { ...native.current.harness, conversation: null } })
                await new Promise((r) => setTimeout(r, 20))
            }
            return dispatch(bytes)
        }
        await user.click(screen.getByRole('button', { name: 'New investigation' }))
        await user.type(screen.getByRole('textbox', { name: 'Message' }), 'Only once')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: l.steps.ended } })
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(1))
        const base = { ...l.steps.declined, entries: [], lastTurn: null, savedAt: null, proposals: [] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-77', phase: 'ready' } } })
        await waitFor(() => expect(native.dispatched).toContainEqual(expect.objectContaining({ kind: 'sendHarness', conversation: 'op-77', prompt: 'Only once' })))
        await new Promise((r) => setTimeout(r, 50))
        expect(native.dispatched.filter((i) => i.kind === 'startHarness')).toHaveLength(1)
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toHaveLength(1)
        native.transport.dispatch = dispatch
    })

    it('keeps a refused or undelivered follow-up beside the edit made while it was delivered, and resends nothing (attempt-04 R1)', async () => {
        const l = LIFECYCLES[0]!
        const { native, user } = await launch(fxHarness({ ...l.steps.approved, proposals: [] }))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        const dispatch = native.transport.dispatch
        for (const how of ['refused', 'undelivered'] as const) {
            await user.clear(box())
            await user.type(box(), `Original follow-up ${how}`)
            let release: () => void = () => undefined
            const respond = native.respond
            native.transport.dispatch = async (bytes) => {
                const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
                if (intent.kind !== 'sendHarness') return dispatch(bytes)
                await new Promise<void>((resolve) => (release = resolve))
                if (how === 'undelivered') {
                    // Delivered nowhere: the client gives up and reports the delivery as failed or unknown.
                    native.respond = () => ({ outcome: 'refusal', refusal: 'busy' })
                }
                return dispatch(bytes)
            }
            native.respond = (intent: Intent) => (intent.kind === 'sendHarness' ? { outcome: 'refusal', refusal: 'busy' } : respond(intent))
            await user.click(screen.getByRole('button', { name: 'Send' }))
            await user.clear(box())
            await user.type(box(), 'A different draft')
            release()
            const notSent = await screen.findByRole('region', { name: 'Not sent' })
            expect(within(notSent).getByText(`Original follow-up ${how}`)).toBeInTheDocument()
            expect(box()).toHaveValue('A different draft')
            native.respond = respond
            native.transport.dispatch = dispatch
        }
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toHaveLength(2)
    })

    it('never calls an unconfirmed send "not sent": only the core\'s word that it took that send settles it, never matching text, and nothing is resent (attempt-04 R2, attempt-16 R1)', async () => {
        const l = LIFECYCLES[0]!
        const ready = { ...l.steps.approved, proposals: [] }
        const { native, user } = await launch(fxHarness(ready))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        lose(native)
        await user.type(box(), 'Follow-up question')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(screen.getByRole('region', { name: 'Not confirmed' })).toBeInTheDocument())
        expect(document.body.textContent).not.toMatch(/Your message was not sent/)
        // A transcript entry with the same words is not the core's word on this send.
        const shown = { ...ready, phase: 'answering' as const, entries: [...ready.entries, op('u-2', 'Follow-up question')] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: shown } })
        expect(screen.getByRole('region', { name: 'Not confirmed' })).toBeInTheDocument()
        // The core reports it took this send: it leaves the list and the box.
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: taking(shown, [sendIds(native.dispatched)[0]!, 'u-2']) } })
        await waitFor(() => expect(screen.queryByRole('region', { name: 'Not confirmed' })).toBeNull())
        expect(box()).toHaveValue('')
        // One send (the client's one resend repeats the same ID, which the core takes once).
        expect(sendIds(native.dispatched)).toHaveLength(1)
    })

    it('identical unconfirmed sends in two conversations are settled each by its own conversation only (attempt-06 R1)', async () => {
        const l = LIFECYCLES[0]!
        const a = { ...l.steps.approved, proposals: [] }
        const { native, user } = await launch(fxHarness(a))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        lose(native)
        await user.type(box(), 'Same words')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await screen.findByRole('region', { name: 'Not confirmed' })
        const b = { ...l.steps.declined, id: 'op-99', phase: 'ready' as const, proposals: [] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: b } })
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getAllByText('Same words')).toHaveLength(2))
        const [first, second] = sendIds(native.dispatched)
        // Conversation B naming A's send settles nothing; its own settles only its own.
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: taking(b, [first!, 'u-5']) } })
        expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getAllByText('Same words')).toHaveLength(2)
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: taking(b, [second!, 'u-5']) } })
        await waitFor(() => expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getAllByText('Same words')).toHaveLength(1))
        // Back on A, A's word settles A's.
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: taking(a, [first!, 'u-2']) } })
        await waitFor(() => expect(screen.queryByRole('region', { name: 'Not confirmed' })).toBeNull())
    })

    it('two identical unconfirmed sends are settled each by its own ID, once, across renders and a remount (attempt-07 R1, attempt-10 R1)', async () => {
        const l = LIFECYCLES[0]!
        const ready = { ...l.steps.approved, proposals: [] }
        const { native, user } = await launch(fxHarness(ready))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        lose(native)
        await user.type(box(), 'Twice')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await screen.findByRole('region', { name: 'Not confirmed' })
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getAllByText('Twice')).toHaveLength(2))
        const [first, second] = sendIds(native.dispatched)
        const one = taking({ ...ready, entries: [...ready.entries, op('u-2', 'Twice')] }, [first!, 'u-2'])
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: one } })
        await native.push({ ...native.current })
        await native.push({ ...native.current, activeFeature: 'administration' })
        await native.push({ ...native.current, activeFeature: 'harness' })
        await waitFor(() => expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getAllByText('Twice')).toHaveLength(1))
        const two = taking({ ...one, entries: [...one.entries, op('u-3', 'Twice')] }, [first!, 'u-2'], [second!, 'u-3'])
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: two } })
        await waitFor(() => expect(screen.queryByRole('region', { name: 'Not confirmed' })).toBeNull())
        expect(sendIds(native.dispatched)).toHaveLength(2)
    })

    it('an admitted retry never settles the unconfirmed original: only the original\'s own ID does (attempt-08 R1, attempt-09 R1)', async () => {
        const l = LIFECYCLES[0]!
        const ready = { ...l.steps.approved, proposals: [] }
        const { native, user } = await launch(fxHarness(ready))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        const respond = lose(native)
        await user.type(box(), 'Same again')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await screen.findByRole('region', { name: 'Not confirmed' })
        native.respond = respond
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(sendIds(native.dispatched)).toHaveLength(2))
        const [original, retry] = sendIds(native.dispatched)
        const one = taking({ ...ready, entries: [...ready.entries, op('u-9', 'Same again')] }, [retry!, 'u-9'])
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: one } })
        await native.push({ ...native.current, activeFeature: 'administration' })
        await native.push({ ...native.current, activeFeature: 'harness' })
        await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toBeInTheDocument())
        expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getByText('Same again')).toBeInTheDocument()
        const two = taking({ ...one, entries: [...one.entries, op('u-10', 'Same again')] }, [retry!, 'u-9'], [original!, 'u-10'])
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: two } })
        await waitFor(() => expect(screen.queryByRole('region', { name: 'Not confirmed' })).toBeNull())
    })

    it('an admitted send\'s delayed entry never settles a later identical unconfirmed send; that send\'s own ID settles it once (attempt-16 R1)', async () => {
        const l = LIFECYCLES[0]!
        const ready = { ...l.steps.approved, proposals: [] }
        const { native, user } = await launch(fxHarness(ready))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        // The first send is admitted; the core's snapshot showing it is delayed.
        await user.type(box(), 'Order reversed')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(sendIds(native.dispatched)).toHaveLength(1))
        expect(box()).toHaveValue('')
        // The same words again from the still-ready snapshot; both its deliveries are lost.
        lose(native)
        await user.type(box(), 'Order reversed')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await screen.findByRole('region', { name: 'Not confirmed' })
        const [first, second] = sendIds(native.dispatched)
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness' && i.submission === second)).toHaveLength(2)
        // Only the first send's entry is published: the second stays unresolved and recoverable.
        const one = taking({ ...ready, phase: 'answering' as const, entries: [...ready.entries, op('u-2', 'Order reversed')] }, [first!, 'u-2'])
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: one } })
        await native.push({ ...native.current, activeFeature: 'administration' })
        await native.push({ ...native.current, activeFeature: 'harness' })
        await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toBeInTheDocument())
        const notConfirmed = screen.getByRole('region', { name: 'Not confirmed' })
        expect(within(notConfirmed).getAllByText('Order reversed')).toHaveLength(1)
        expect(box()).toHaveValue('Order reversed')
        // Its own ID, reported once, settles it once.
        const two = taking({ ...one, entries: [...one.entries, op('u-3', 'Order reversed')] }, [first!, 'u-2'], [second!, 'u-3'])
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: two } })
        await waitFor(() => expect(screen.queryByRole('region', { name: 'Not confirmed' })).toBeNull())
        expect(box()).toHaveValue('')
        await native.push({ ...native.current })
        expect(screen.queryByRole('region', { name: 'Not confirmed' })).toBeNull()
        // Two sends, nothing resent.
        expect(sendIds(native.dispatched)).toHaveLength(2)
    })

    it('a send whose receipt was lost and that the core then gave back is never taken as delivered: its exact text stays in the box or under Not sent (attempt-17 R1)', async () => {
        const l = LIFECYCLES[0]!
        const ready = { ...l.steps.approved, proposals: [] }
        // The core's word in one coalesced snapshot: it took the send, then gave it back.
        const gaveBack = (submission: string, text: string, why: 'renewal' | 'write', unsent: boolean): ConversationView => ({
            ...ready,
            phase: 'ready',
            unsent: unsent ? text : null,
            signInFailure: why === 'renewal' ? 'sessionExpired' : null,
            submissions: [{ submission, entry: 'u-2', state: 'returned' as const }],
        })
        for (const why of ['renewal', 'write'] as const) {
            resetShellStore()
            const { native, user, view } = await launch(fxHarness(ready))
            const box = () => screen.getByRole('textbox', { name: 'Message' })
            lose(native)
            const text = `Returned after a ${why}`
            await user.type(box(), text)
            await user.click(screen.getByRole('button', { name: 'Send' }))
            await screen.findByRole('region', { name: 'Not confirmed' })
            if (why === 'write') {
                // A newer draft before the core's word arrives.
                await user.clear(box())
                await user.type(box(), 'Something newer')
            }
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: gaveBack(sendIds(native.dispatched)[0]!, text, why, true) } })
            await waitFor(() => expect(screen.queryByRole('region', { name: 'Not confirmed' })).toBeNull())
            if (why === 'renewal') {
                // Still in the box, exactly; the banner says so.
                expect(box()).toHaveValue(text)
                expect(screen.getByText('Your last message was not sent and is back in the box. Nothing was resent.')).toBeInTheDocument()
                expect(screen.queryByRole('region', { name: 'Not sent' })).toBeNull()
                // A newer edit stays across renders and navigation.
                await user.type(box(), ' and more')
                await native.push({ ...native.current })
                await native.push({ ...native.current, activeFeature: 'administration' })
                await native.push({ ...native.current, activeFeature: 'harness' })
                await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue(`${text} and more`))
            } else {
                // The newer draft stays; the returned message is kept, once, under Not sent.
                expect(box()).toHaveValue('Something newer')
                expect(within(screen.getByRole('region', { name: 'Not sent' })).getAllByText(text)).toHaveLength(1)
                expect(screen.getByText('Your last message was not sent. Nothing was resent; it is kept under Not sent.')).toBeInTheDocument()
                await native.push({ ...native.current, activeFeature: 'administration' })
                await native.push({ ...native.current, activeFeature: 'harness' })
                await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Something newer'))
                expect(within(screen.getByRole('region', { name: 'Not sent' })).getAllByText(text)).toHaveLength(1)
            }
            expect(sendIds(native.dispatched)).toHaveLength(1)
            view.unmount()
        }
        // Given back while its text was never observed as unsent (the field already cleared): still kept, never lost.
        resetShellStore()
        const { native, user } = await launch(fxHarness(ready))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        lose(native)
        await user.type(box(), 'Never observed')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await screen.findByRole('region', { name: 'Not confirmed' })
        await user.clear(box())
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: gaveBack(sendIds(native.dispatched)[0]!, 'Never observed', 'write', false) } })
        await waitFor(() => expect(within(screen.getByRole('region', { name: 'Not sent' })).getByText('Never observed')).toBeInTheDocument())
        expect(screen.queryByRole('region', { name: 'Not confirmed' })).toBeNull()
        expect(sendIds(native.dispatched)).toHaveLength(1)
    })

    it('a receipt that arrives after the core gave its send back never clears the returned text, on either send path (attempt-18 R1)', async () => {
        const l = LIFECYCLES[0]!
        const ready = { ...l.steps.approved, proposals: [] }
        /** Holds every send's receipt until released; `held` names the sends held. */
        const hold = (native: Awaited<ReturnType<typeof launch>>['native']) => {
            let release: () => void = () => undefined
            const held: Intent[] = []
            const dispatch = native.transport.dispatch
            native.transport.dispatch = async (bytes) => {
                const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
                if (intent.kind === 'sendHarness') {
                    held.push(intent)
                    await new Promise<void>((resolve) => (release = resolve))
                }
                return dispatch(bytes)
            }
            return {
                held,
                release: () => {
                    native.transport.dispatch = dispatch
                    release()
                },
            }
        }
        const gaveBack = (c: ConversationView, submission: string, text: string, unsent: boolean): ConversationView => ({
            ...c,
            phase: 'ready',
            unsent: unsent ? text : null,
            signInFailure: 'sessionExpired',
            submissions: [{ submission, entry: 'u-2', state: 'returned' as const }],
        })
        // An existing conversation: the core's word first, then the receipt.
        {
            const { native, user, view } = await launch(fxHarness(ready))
            const box = () => screen.getByRole('textbox', { name: 'Message' })
            const { held, release } = hold(native)
            await user.type(box(), 'Held then returned')
            await user.click(screen.getByRole('button', { name: 'Send' }))
            await waitFor(() => expect(sendIds(held)).toHaveLength(1))
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: gaveBack(ready, sendIds(held)[0]!, 'Held then returned', true) } })
            release()
            await new Promise((r) => setTimeout(r, 50))
            expect(box()).toHaveValue('Held then returned')
            expect(screen.getByText('Your last message was not sent and is back in the box. Nothing was resent.')).toBeInTheDocument()
            await user.type(box(), ' (edited)')
            await native.push({ ...native.current, activeFeature: 'administration' })
            await native.push({ ...native.current, activeFeature: 'harness' })
            await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Held then returned (edited)'))
            expect(sendIds(native.dispatched)).toHaveLength(1)
            view.unmount()
        }
        // The first send of a new conversation: the core's word first, then the receipt.
        {
            resetShellStore()
            const { native, user, view } = await launch(fxHarness(null))
            const box = () => screen.getByRole('textbox', { name: 'Message' })
            await user.type(box(), 'First, returned')
            await user.click(screen.getByRole('button', { name: 'Send' }))
            const { opening: _none, ...base } = l.steps.declined
            const started = { ...base, id: 'op-77', phase: 'ready' as const, entries: [], lastTurn: null, savedAt: null, proposals: [] }
            const { held, release } = hold(native)
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: started } })
            await waitFor(() => expect(sendIds(held)).toHaveLength(1))
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: gaveBack(started, sendIds(held)[0]!, 'First, returned', true) } })
            release()
            await new Promise((r) => setTimeout(r, 50))
            expect(box()).toHaveValue('First, returned')
            expect(sendIds(native.dispatched)).toHaveLength(1)
            view.unmount()
        }
        // The receipt first (the box is cleared), then the core's word without its unsent text: kept under Not sent.
        {
            resetShellStore()
            const { native, user } = await launch(fxHarness(ready))
            const box = () => screen.getByRole('textbox', { name: 'Message' })
            await user.type(box(), 'Admitted then returned')
            await user.click(screen.getByRole('button', { name: 'Send' }))
            await waitFor(() => expect(box()).toHaveValue(''))
            await user.type(box(), 'Next draft')
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: gaveBack(ready, sendIds(native.dispatched)[0]!, 'Admitted then returned', false) } })
            await waitFor(() => expect(within(screen.getByRole('region', { name: 'Not sent' })).getAllByText('Admitted then returned')).toHaveLength(1))
            expect(box()).toHaveValue('Next draft')
            await native.push({ ...native.current, activeFeature: 'administration' })
            await native.push({ ...native.current, activeFeature: 'harness' })
            await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Next draft'))
            expect(within(screen.getByRole('region', { name: 'Not sent' })).getAllByText('Admitted then returned')).toHaveLength(1)
            expect(sendIds(native.dispatched)).toHaveLength(1)
        }
    })

    it('a pending report is never final: a send later given back keeps its exact text beside newer edits, on both send paths and through an A/B/A that retires its conversation (attempt-19 R1)', async () => {
        const l = LIFECYCLES[0]!
        const ready = { ...l.steps.approved, proposals: [] }
        const report = (c: ConversationView, submission: string, state: 'pending' | 'returned'): ConversationView => ({
            ...c,
            phase: state === 'pending' ? 'renewing' : 'ready',
            // The core's transient unsent text is never observed here.
            unsent: null,
            submissions: [{ submission, entry: 'u-2', state }],
        })
        for (const path of ['current', 'first'] as const) {
            resetShellStore()
            const { native, user, view } = await launch(fxHarness(path === 'current' ? ready : null))
            const box = () => screen.getByRole('textbox', { name: 'Message' })
            const text = `Renewing then returned (${path})`
            await user.type(box(), text)
            await user.click(screen.getByRole('button', { name: 'Send' }))
            let conversation: ConversationView = ready
            if (path === 'first') {
                const { opening: _none, ...base } = l.steps.declined
                conversation = { ...base, id: 'op-77', phase: 'ready', entries: [], lastTurn: null, savedAt: null, proposals: [] }
                await native.push({ ...native.current, harness: { ...native.current.harness, conversation } })
            }
            await waitFor(() => expect(sendIds(native.dispatched)).toHaveLength(1))
            const submission = sendIds(native.dispatched)[0]!
            // Admitted while the renewal runs: pending, so the text is still tracked once the box is cleared.
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: report(conversation, submission, 'pending') } })
            await waitFor(() => expect(box()).toHaveValue(''))
            await user.type(box(), 'A newer edit')
            // Then given back.
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: report(conversation, submission, 'returned') } })
            await waitFor(() => expect(within(screen.getByRole('region', { name: 'Not sent' })).getAllByText(text)).toHaveLength(1))
            expect(box()).toHaveValue('A newer edit')
            await native.push({ ...native.current, activeFeature: 'administration' })
            await native.push({ ...native.current, activeFeature: 'harness' })
            await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('A newer edit'))
            expect(within(screen.getByRole('region', { name: 'Not sent' })).getAllByText(text)).toHaveLength(1)
            expect(sendIds(native.dispatched)).toHaveLength(1)
            view.unmount()
        }
        // Admitted while renewing, then A/B/A retires the conversation before the core's final word.
        resetShellStore()
        const { native, user } = await launch(fxHarness(ready))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'Renewing when retired')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(sendIds(native.dispatched)).toHaveLength(1))
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: report(ready, sendIds(native.dispatched)[0]!, 'pending') } })
        await waitFor(() => expect(box()).toHaveValue(''))
        await user.type(box(), 'Typed after')
        const a = native.current
        await native.push({ ...a, active: { tenant: a.tenants[1]!.id, context: 'cx-bbbbbbbbbbbbbbbbbbbbbbbb' }, harness: { ...a.harness, conversation: null } })
        const reissued = { ...a.gateway, selected: { ...a.gateway.selected!, reference: 'gw-cccccccccccccccccccccccc' } }
        await native.push({ ...a, active: { tenant: a.active!.tenant, context: 'cx-cccccccccccccccccccccccc' }, gateway: reissued, harness: { ...a.harness, conversation: null } })
        await waitFor(() => expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getByText('Renewing when retired')).toBeInTheDocument())
        expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Typed after')
        await native.push({ ...native.current, activeFeature: 'administration' })
        await native.push({ ...native.current, activeFeature: 'harness' })
        await waitFor(() => expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getByText('Renewing when retired')).toBeInTheDocument())
        expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Typed after')
        expect(sendIds(native.dispatched)).toHaveLength(1)
    })

    it('the rail keeps the conversation\'s title from its opening after the transcript dropped it, also once ended (attempt-18 R2)', async () => {
        const l = LIFECYCLES[0]!
        const big = (n: number) => 'x'.repeat(n)
        const byCount = pushed(full('Original investigation').map((e) => (e.id === 'u-1' ? { ...e, text: 'Original investigation' } : e)), op('u-101', 'Later unrelated question'))
        const byBytes = pushed([op('u-1', 'Original investigation'), answer('a-1', big(260_000)), op('u-2', 'Later unrelated question'), answer('a-2', big(260_000)), answer('a-3', big(260_000)), answer('a-4', big(260_000))], answer('a-5', big(260_000)))
        for (const entries of [byCount, byBytes]) {
            expect(entries.some((e) => e.id === 'u-1')).toBe(false)
            expect(entries.find((e) => e.role === 'operator')!.text).not.toBe('Original investigation')
            for (const step of [l.steps.approved, l.steps.ended]) {
                resetShellStore()
                const { view } = await launch(fxHarness({ ...step, proposals: [], entries, opening: 'Original investigation' }))
                const rail = screen.getByRole('navigation', { name: 'Conversations' })
                expect(within(rail).getByText('Original investigation')).toBeInTheDocument()
                expect(within(rail).queryByText('Later unrelated question')).toBeNull()
                view.unmount()
            }
        }
    })

    it('settles an unconfirmed send by the core\'s word even when the transcript is at its bounds and dropped its oldest entries (attempt-11 R1)', async () => {
        const l = LIFECYCLES[0]!
        for (const bound of ['entries', 'bytes'] as const) {
            resetShellStore()
            const big = (n: number) => 'x'.repeat(n)
            const start = bound === 'entries' ? full('Bound') : [op('u-1', 'Bound'), answer('a-1', big(260_000)), answer('a-2', big(260_000)), op('u-2', 'Else'), answer('a-3', big(260_000)), answer('a-4', big(260_000))]
            const ready = { ...l.steps.approved, proposals: [], entries: start }
            const { native, user, view } = await launch(fxHarness(ready))
            const box = () => screen.getByRole('textbox', { name: 'Message' })
            lose(native)
            await user.type(box(), 'Bound')
            await user.click(screen.getByRole('button', { name: 'Send' }))
            await screen.findByRole('region', { name: 'Not confirmed' })
            const entry = bound === 'entries' ? 'u-101' : 'u-3'
            const delivered = pushed(start, op(entry, 'Bound'), answer('a-x', big(bound === 'entries' ? 10 : 260_000)))
            expect(delivered.some((e) => e.id === 'u-1')).toBe(false)
            await native.push({ ...native.current, harness: { ...native.current.harness, conversation: taking({ ...ready, entries: delivered }, [sendIds(native.dispatched)[0]!, entry]) } })
            await waitFor(() => expect(screen.queryByRole('region', { name: 'Not confirmed' })).toBeNull())
            expect(box()).toHaveValue('')
            view.unmount()
        }
    })

    it('a lost start receipt is never "no conversation started": the message stays, sent nowhere (attempt-04 R2, lost start receipt)', async () => {
        const { native, user } = await launch(fxHarness(null))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        await user.type(box(), 'First question')
        native.respond = (intent: Intent) => {
            if (intent.kind === 'startHarness') throw new DeliveryOutcomeUnknown('lost')
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } }
        }
        await user.click(screen.getByRole('button', { name: 'Send' }))
        expect(await screen.findByText(/whether a conversation started is not known\. Your message was not sent to any conversation; it is still in the box\./)).toBeInTheDocument()
        expect(document.body.textContent).not.toMatch(/No conversation started/)
        // A conversation then appears (perhaps the one started): nothing is sent to it by itself.
        const base = { ...LIFECYCLES[0]!.steps.declined, entries: [], lastTurn: null, savedAt: null, proposals: [] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: { ...base, id: 'op-80', phase: 'ready' } } })
        await new Promise((r) => setTimeout(r, 50))
        expect(native.dispatched.filter((i) => i.kind === 'sendHarness')).toEqual([])
        expect(box()).toHaveValue('First question')
    })

    it('settles an unconfirmed send only by a message its own conversation added after it: a repeated prompt or another conversation settles nothing (attempt-05 R1)', async () => {
        const l = LIFECYCLES[0]!
        const ready = { ...l.steps.approved, proposals: [] }
        const opening = ready.entries.find((e) => e.role === 'operator')!.text
        const { native, user } = await launch(fxHarness(ready))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        native.respond = (intent: Intent) => {
            if (intent.kind === 'sendHarness') throw new DeliveryOutcomeUnknown('lost')
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } }
        }
        // The same words as the opening question, receipt lost, no new transcript entry.
        await user.type(box(), opening)
        await user.click(screen.getByRole('button', { name: 'Send' }))
        const notConfirmed = await screen.findByRole('region', { name: 'Not confirmed' })
        expect(within(notConfirmed).getByText(opening)).toBeInTheDocument()
        await native.push({ ...native.current })
        expect(screen.getByRole('region', { name: 'Not confirmed' })).toBeInTheDocument()
        expect(box()).toHaveValue(opening)
        expect(screen.getByText(/It may have been delivered: check the transcript/)).toBeInTheDocument()
        // Another conversation showing those words settles nothing either.
        const other = { ...l.steps.declined, id: 'op-99', phase: 'ready' as const, proposals: [], entries: [{ id: 'u-1', role: 'operator' as const, text: opening, state: 'complete' as const }, { id: 'u-2', role: 'operator' as const, text: opening, state: 'complete' as const }] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: other } })
        expect(screen.getByRole('region', { name: 'Not confirmed' })).toBeInTheDocument()
        // One send (the client's single transport resend of the same delivery aside); nothing replayed.
        expect(new Set(native.dispatched.filter((i) => i.kind === 'sendHarness').map((i) => JSON.stringify(i))).size).toBe(1)
    })

    it('a retry whose receipt is lost moves a refused message from Not sent to Not confirmed, truthfully (attempt-05 R2)', async () => {
        const l = LIFECYCLES[0]!
        const { native, user } = await launch(fxHarness({ ...l.steps.approved, proposals: [] }))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        const respond = native.respond
        // Refused while the operator types something else: kept under Not sent.
        let release: () => void = () => undefined
        const dispatch = native.transport.dispatch
        native.transport.dispatch = async (bytes) => {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
            if (intent.kind === 'sendHarness') await new Promise<void>((resolve) => (release = resolve))
            return dispatch(bytes)
        }
        native.respond = (intent: Intent) => (intent.kind === 'sendHarness' ? { outcome: 'refusal', refusal: 'busy' } : respond(intent))
        await user.type(box(), 'Retry me')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await user.clear(box())
        await user.type(box(), 'Something else')
        release()
        expect(within(await screen.findByRole('region', { name: 'Not sent' })).getByText('Retry me')).toBeInTheDocument()
        native.transport.dispatch = dispatch
        // The operator retries the same words; this time the receipt is lost.
        native.respond = (intent: Intent) => {
            if (intent.kind === 'sendHarness') throw new DeliveryOutcomeUnknown('lost')
            return respond(intent)
        }
        await user.clear(box())
        await user.type(box(), 'Retry me')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        const notConfirmed = await screen.findByRole('region', { name: 'Not confirmed' })
        expect(within(notConfirmed).getByText('Retry me')).toBeInTheDocument()
        expect(screen.queryByRole('region', { name: 'Not sent' })).toBeNull()
        expect(screen.getByText(/It is listed under Not confirmed/)).toBeInTheDocument()
        expect(box()).toHaveValue('Retry me')
    })

    it('a later refused retry never downgrades an earlier unconfirmed send (attempt-06 R1)', async () => {
        const l = LIFECYCLES[0]!
        const { native, user } = await launch(fxHarness({ ...l.steps.approved, proposals: [] }))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        const respond = native.respond
        native.respond = (intent: Intent) => {
            if (intent.kind === 'sendHarness') throw new DeliveryOutcomeUnknown('lost')
            return respond(intent)
        }
        await user.type(box(), 'Retry me')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        expect(within(await screen.findByRole('region', { name: 'Not confirmed' })).getByText('Retry me')).toBeInTheDocument()
        // An explicit retry, edited while it waits, then refused.
        let release: () => void = () => undefined
        const dispatch = native.transport.dispatch
        native.transport.dispatch = async (bytes) => {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
            if (intent.kind === 'sendHarness') await new Promise<void>((resolve) => (release = resolve))
            return dispatch(bytes)
        }
        native.respond = (intent: Intent) => (intent.kind === 'sendHarness' ? { outcome: 'refusal', refusal: 'busy' } : respond(intent))
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await user.clear(box())
        await user.type(box(), 'Edited meanwhile')
        release()
        expect(within(await screen.findByRole('region', { name: 'Not sent' })).getByText('Retry me')).toBeInTheDocument()
        // The first attempt is still not confirmed: a refusal of the second establishes nothing about it.
        expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getByText('Retry me')).toBeInTheDocument()
        expect(box()).toHaveValue('Edited meanwhile')
        native.transport.dispatch = dispatch
    })

    it('confirms an ended conversation with a toast bound to its tenant and context (I10)', async () => {
        const l = LIFECYCLES[0]!
        const { native } = await launch(fxHarness(l.steps.approved))
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: l.steps.ended } })
        await waitFor(() =>
            expect(useShellStore.getState().toasts.map((t) => [t.text, t.tenant, t.context])).toContainEqual([
                `Conversation ended. Its transcript is saved on this Mac (${l.steps.ended.historyId.slice(0, 8)}).`,
                'tn-0123456789abcdef',
                CONTEXT,
            ])
        )
    })

    it('a retry whose entry arrives before its receipt never settles the unconfirmed original either (attempt-09 R1)', async () => {
        const l = LIFECYCLES[0]!
        const ready = { ...l.steps.approved, proposals: [] }
        const { native, user } = await launch(fxHarness(ready))
        const box = () => screen.getByRole('textbox', { name: 'Message' })
        const respond = native.respond
        native.respond = (intent: Intent) => {
            if (intent.kind === 'sendHarness') throw new DeliveryOutcomeUnknown('lost')
            return respond(intent)
        }
        await user.type(box(), 'Order matters')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        await screen.findByRole('region', { name: 'Not confirmed' })
        // The retry: its receipt is held while its entry is published.
        native.respond = respond
        let release: () => void = () => undefined
        const dispatch = native.transport.dispatch
        native.transport.dispatch = async (bytes) => {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
            if (intent.kind === 'sendHarness') await new Promise<void>((resolve) => (release = resolve))
            return dispatch(bytes)
        }
        await user.click(screen.getByRole('button', { name: 'Send' }))
        const one = { ...ready, entries: [...ready.entries, { id: 'u-9', role: 'operator' as const, text: 'Order matters', state: 'complete' as const }] }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: one } })
        expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getByText('Order matters')).toBeInTheDocument()
        release()
        native.transport.dispatch = dispatch
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'sendHarness').length).toBeGreaterThanOrEqual(3))
        await native.push({ ...native.current })
        await native.push({ ...native.current, activeFeature: 'administration' })
        await native.push({ ...native.current, activeFeature: 'harness' })
        await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toBeInTheDocument())
        expect(within(screen.getByRole('region', { name: 'Not confirmed' })).getByText('Order matters')).toBeInTheDocument()
    })


    // ---- C38: the board (R2App start=harness) over the same behaviour ----

    it('draws the board: the title band with End conversation, the 244px rail of saved conversations, the transcript with its composer, and the Decisions aside holding the decision (C38)', async () => {
        const l = LIFECYCLES[0]!
        // An earlier, separately saved conversation: the live one's own saved copy is not listed twice.
        const earlier = { ...l.saved, id: '7b1c9d2e-3f4a-4b5c-8d6e-7f8091a2b3c4', title: 'Earlier investigation' }
        const { user, native } = await launch(fxHarness(l.steps.approved, {}, [earlier]))
        // The band: the title, the status line and End conversation at its right.
        const band = document.querySelector('[data-slot="harness-band"]') as HTMLElement
        expect(within(band).getByRole('heading', { level: 1, name: 'Harness' })).toBeInTheDocument()
        expect(within(band).getByRole('button', { name: 'End conversation' })).toBeInTheDocument()
        // The rail: the encrypted-saved label, New investigation, the live conversation and the saved one.
        const rail = screen.getByRole('navigation', { name: 'Conversations' })
        expect(rail).toHaveTextContent('Saved conversations · encrypted')
        expect(within(rail).getByRole('button', { name: 'New investigation' })).toBeInTheDocument()
        expect(rail.querySelectorAll('[data-slot="rail-conversation"]')).toHaveLength(2)
        // The transcript: operator entries as bubbles at the right, the Harness's at the left; the composer is a pill with an icon Send.
        const transcript = screen.getByRole('region', { name: 'Transcript' })
        const entries = transcript.querySelectorAll('[data-slot="harness-entry"]')
        expect(entries.length).toBeGreaterThan(0)
        expect((entries[0] as HTMLElement).style.alignSelf).toBe('flex-end')
        expect(screen.getByRole('textbox', { name: 'Message' })).toHaveAttribute('placeholder', 'Ask the Harness…')
        expect(screen.getByRole('button', { name: 'Send' })).toBeInTheDocument()
        // The aside: Decisions, with the decision card inside it when one waits.
        const aside = screen.getByRole('complementary', { name: 'Decisions' })
        expect(within(aside).getByRole('heading', { level: 2, name: 'Decisions' })).toBeInTheDocument()
        const pending = { ...l.steps.approved, approvals: l.steps.asking.approvals, phase: 'ready' as const }
        await native.push({ ...native.current, harness: { ...native.current.harness, conversation: pending } })
        expect(within(screen.getByRole('complementary', { name: 'Decisions' })).getByRole('region', { name: 'Decision needed' })).toBeInTheDocument()
        expect(document.querySelector('[data-slot="harness-pending-line"]')).not.toBeNull()
        // End conversation from the band is the same intent.
        await user.click(within(band).getByRole('button', { name: 'End conversation' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'endHarness', conversation: pending.id })
    })

    it('tells the aside what it cannot decide yet, and names the workspace MCP servers with their standing (C38)', async () => {
        const base = fxHarness(null)
        await launch({
            ...base,
            harness: {
                ...base.harness,
                mcp: {
                    workspace: WS.id,
                    state: 'ready',
                    complete: true,
                    failure: null,
                    servers: [
                        { id: 'srv-1', name: 'cloud-inventory', namespace: 'inventory', url: 'https://mcp.example/inventory', status: 'active', supported: true, enabled: true, signedIn: true, stale: false, expired: null, step: null, failure: null },
                        { id: 'srv-2', name: 'ticketing', namespace: 'ticketing', url: 'https://mcp.example/ticketing', status: 'active', supported: true, enabled: false, signedIn: false, stale: false, expired: null, step: null, failure: null },
                    ],
                },
            },
        })
        const aside = screen.getByRole('complementary', { name: 'Decisions' })
        expect(aside).toHaveTextContent('Nothing to decide yet. Tool calls appear here and wait for your OK.')
        const summary = aside.querySelector('[data-slot="harness-mcp-summary"]') as HTMLElement
        expect(summary).toHaveTextContent('Workspace MCP servers · 1 of 2')
        expect(summary).toHaveTextContent('cloud-inventory')
        expect(summary).toHaveTextContent('Signed in')
        expect(summary).toHaveTextContent('ticketing')
        expect(summary).toHaveTextContent('Not signed in')
    })

    // ---- C38 attempt-01 R1 / E2: the MCP summary's truth and the v2 setup panels' workflows ----

    const server = (over: Partial<McpServerView> = {}): McpServerView => ({ id: 'srv-tools-0001', name: 'Tools', namespace: 'tools', url: 'https://mcp.example/tools/mcp', status: 'active', supported: true, enabled: false, signedIn: false, stale: false, expired: null, step: null, failure: null, ...over })
    const mcpOf = (servers: McpServerView[], over: Partial<HarnessMcpView> = {}): HarnessMcpView => ({ workspace: WS.id, state: 'ready', complete: true, failure: null, servers, ...over })
    const withHarness = (over: Partial<ShellSnapshot['harness']>): ShellSnapshot => {
        const base = fxHarness(null)
        return { ...base, harness: { ...base.harness, ...over } }
    }
    const summary = () => document.querySelector('[data-slot="harness-mcp-summary"]') as HTMLElement
    async function openSetup(user: ReturnType<typeof userEvent.setup>) {
        await user.click(screen.getByRole('button', { name: 'Project, sign-in and workspace tools' }))
    }

    it('says what the MCP summary knows: not read, reading, failed, partial; every server that is on is listed and the rest are counted (attempt-01 R1)', async () => {
        const { native } = await launch(withHarness({ mcp: null }))
        expect(summary()).toHaveTextContent('Workspace MCP servers · not read yet')
        expect(summary()).not.toHaveTextContent('0 of 0')
        await native.push({ ...native.current, harness: { ...native.current.harness, mcp: mcpOf([], { state: 'loading' }) } })
        await waitFor(() => expect(summary()).toHaveTextContent('Workspace MCP servers · reading…'))
        await native.push({ ...native.current, harness: { ...native.current.harness, mcp: mcpOf([], { state: 'failed', failure: 'denied' }) } })
        await waitFor(() => expect(summary()).toHaveTextContent('Workspace MCP servers · could not be read: This management client is not allowed to read this.'))
        expect(summary()).toHaveAttribute('data-state', 'failed')
        await native.push({ ...native.current, harness: { ...native.current.harness, mcp: mcpOf([server()], { complete: false }) } })
        await waitFor(() => expect(summary()).toHaveTextContent('Workspace MCP servers · 0 of 1 · only the first 500 servers were read'))
        // Nine servers, the ninth the only one on and signed in: it is listed first; one server is named as not shown.
        const nine = Array.from({ length: 9 }, (_, i) => server({ id: `srv-${i}`, name: `S${i}`, namespace: `s${i}`, enabled: i === 8, signedIn: i === 8 }))
        await native.push({ ...native.current, harness: { ...native.current.harness, mcp: mcpOf(nine) } })
        await waitFor(() => expect(summary()).toHaveTextContent('Workspace MCP servers · 1 of 9'))
        const rows = Array.from(summary().querySelectorAll('div.flex.items-center')).map((r) => r.textContent)
        expect(rows[0]).toBe('S8Signed in')
        expect(rows).toHaveLength(8)
        expect(summary().querySelector('[data-slot="harness-mcp-more"]')).toHaveTextContent('1 more in the setup below')
    })

    it('chooses a folder, names a refused one, allows changes in it and removes it, through the v2 project panel (C21, attempt-01 E2)', async () => {
        const { native, user } = await launch(withHarness({ project: null }))
        await openSetup(user)
        const panel = screen.getByRole('region', { name: 'Project folder' })
        expect(within(panel).getByText(/cannot run commands/)).toBeInTheDocument()
        await user.click(within(panel).getByRole('button', { name: 'Choose a folder…' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'chooseHarnessProject', context: CONTEXT }))
        await native.push({ ...native.current, harness: { ...native.current.harness, projectRefused: '/usr' } })
        expect(await within(panel).findByRole('alert')).toHaveTextContent('/usr cannot be used. Folders that hold system files, credentials or this app’s own data cannot be used.')
        await native.push({ ...native.current, harness: { ...native.current.harness, projectRefused: null, project: { path: '/Users/someone/work/agent-demo', write: false } } })
        expect(await within(panel).findByText('/Users/someone/work/agent-demo')).toBeInTheDocument()
        const write = within(panel).getByRole('checkbox', { name: 'Allow changes in this folder' })
        expect(write).not.toBeChecked()
        expect(write).toHaveAccessibleDescription(/Read-only/)
        await user.click(write)
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'setHarnessWrite', context: CONTEXT, write: true }))
        await native.push({ ...native.current, harness: { ...native.current.harness, project: { path: '/Users/someone/work/agent-demo', write: true } } })
        await waitFor(() => expect(write).toBeChecked())
        // A refused delivery is said, and nothing changes.
        native.respond = (intent: Intent) => (intent.kind === 'clearHarnessProject' ? { outcome: 'refusal', refusal: 'busy' } : { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        await user.click(within(panel).getByRole('button', { name: 'Remove folder' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'clearHarnessProject', context: CONTEXT }))
        expect(await screen.findByText('That was not done. The desktop is busy with a previous request. Try again in a moment.')).toBeInTheDocument()
        expect(within(panel).getByText('/Users/someone/work/agent-demo')).toBeInTheDocument()
    })

    it('keeps a lost receipt as Not confirmed in the v2 panels, naming the action; the next snapshot settles what happened and the page resends nothing (attempt-02 R1)', async () => {
        const saved = { id: '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6d', title: 'Remember periwinkle', updated: 1_790_000_000, entries: 4, source: 'workspaceKey' as const }
        const signedIn: HarnessSignInView = { workspace: WS, endpoint: 'https://gateway.example/rt/v1', issuer: 'https://id.example/oauth2/default', clientId: 'desktop-client', audience: 'https://gateway.example', selected: true, account: 'alice@example.com', step: null, failure: null }
        const tools = server({ enabled: true, signedIn: true })
        const history: HarnessHistoryView = { local: false, state: 'ready', conversations: [saved], unreadable: [], failure: null, full: false, opened: null, busy: false, problem: null, export: null }
        const { native, user } = await launch(withHarness({ project: { path: '/Users/someone/work/agent-demo', write: false }, signIn: signedIn, mcp: mcpOf([tools]), history }))
        await openSetup(user)
        // Every receipt for these actions is lost (the client's one resend included).
        const lost = new Set<Intent['kind']>()
        const respond = native.respond
        native.respond = (intent: Intent) => {
            if (lost.has(intent.kind)) throw new DeliveryOutcomeUnknown('lost')
            return respond(intent)
        }
        const sentOf = (kind: Intent['kind']) => native.dispatched.filter((i) => i.kind === kind).length
        const problem = () => document.querySelector('[data-slot="harness-problem"]')
        const notConfirmed = async (kind: Intent['kind'], text: string) => {
            expect(await screen.findByText(text)).toBeInTheDocument()
            expect(problem()?.textContent).not.toMatch(/not done/)
            // The page dispatched once; only the client's own single resend may follow it.
            expect(sentOf(kind)).toBeGreaterThanOrEqual(1)
            expect(sentOf(kind)).toBeLessThanOrEqual(2)
            return sentOf(kind)
        }

        // The folder permission: Not confirmed names the change; the box follows the snapshot, not the click.
        lost.add('setHarnessWrite')
        const project = screen.getByRole('region', { name: 'Project folder' })
        const write = within(project).getByRole('checkbox', { name: 'Allow changes in this folder' })
        await user.click(write)
        const writes = await notConfirmed('setHarnessWrite', 'Not confirmed: allowing changes in this folder. The desktop did not confirm it and it may have been done. Check the folder and the permission shown here before trying again.')
        expect(write).not.toBeChecked()
        await native.push({ ...native.current, harness: { ...native.current.harness, project: { path: '/Users/someone/work/agent-demo', write: true } } })
        await waitFor(() => expect(write).toBeChecked())
        expect(sentOf('setHarnessWrite')).toBe(writes)

        // An MCP sign-out: Not confirmed names the server; the catalog's next read shows it signed out.
        lost.add('signOutMcpServer')
        const mcp = screen.getByRole('region', { name: 'Workspace MCP servers' })
        await user.click(within(mcp).getByRole('button', { name: 'Sign out of Tools' }))
        const signOuts = await notConfirmed('signOutMcpServer', 'Not confirmed: signing out of Tools. The desktop did not confirm it and it may have been done. Check the servers shown here before trying again.')
        expect(within(mcp).getByRole('button', { name: 'Sign out of Tools' })).toBeInTheDocument()
        await native.push({ ...native.current, harness: { ...native.current.harness, mcp: mcpOf([{ ...tools, signedIn: false }]) } })
        await waitFor(() => expect(within(mcp).queryByRole('button', { name: 'Sign out of Tools' })).toBeNull())
        expect(sentOf('signOutMcpServer')).toBe(signOuts)

        // A permanent deletion: Not confirmed names it; the list keeps the conversation until the core's next word removes it.
        lost.add('deleteHarnessHistory')
        const list = screen.getByRole('region', { name: 'Saved conversations' })
        await user.click(within(list).getByRole('button', { name: 'Open Remember periwinkle' }))
        const opened = { id: saved.id, title: saved.title, updated: saved.updated, entries: [{ id: 'u-1', role: 'operator' as const, text: 'Remember periwinkle', state: 'complete' as const }], source: 'workspaceKey' as const, notContinuable: null }
        await native.push({ ...native.current, harness: { ...native.current.harness, history: { ...history, opened } } })
        await user.click(await screen.findByRole('button', { name: 'Delete…' }))
        await user.click(screen.getByRole('button', { name: 'Delete permanently' }))
        const deletes = await notConfirmed('deleteHarnessHistory', 'Not confirmed: deleting the conversation. The desktop did not confirm it and it may have been done. Check the conversations shown here before trying again.')
        expect(screen.getByRole('button', { name: 'Open Remember periwinkle' })).toBeInTheDocument()
        await native.push({ ...native.current, harness: { ...native.current.harness, history: { ...history, conversations: [], opened: null } } })
        await waitFor(() => expect(screen.queryByRole('button', { name: 'Open Remember periwinkle' })).toBeNull())
        expect(sentOf('deleteHarnessHistory')).toBe(deletes)

        // The Harness sign-out: Not confirmed, never "not done"; the next snapshot shows the account gone.
        lost.add('signOutHarness')
        await user.click(screen.getByRole('button', { name: 'Sign out' }))
        const outs = await notConfirmed('signOutHarness', 'Not confirmed: signing out. The desktop did not confirm it and it may have been done. Check the sign-in state shown here before trying again.')
        expect(screen.getByText('Signed in as alice@example.com')).toBeInTheDocument()
        await native.push({ ...native.current, harness: { ...native.current.harness, signIn: { ...signedIn, account: null } } })
        await waitFor(() => expect(screen.queryByText('Signed in as alice@example.com')).toBeNull())
        expect(sentOf('signOutHarness')).toBe(outs)
        // A real refusal still says its own reason, not the folder wording and not Not confirmed.
        native.respond = (intent: Intent) => (intent.kind === 'removeHarnessSignIn' ? { outcome: 'refusal', refusal: 'busy' } : respond(intent))
        await user.click(screen.getByRole('button', { name: 'Remove company sign-in' }))
        expect(await screen.findByText('That was not done. The desktop is busy with a previous request. Try again in a moment.')).toBeInTheDocument()
        native.respond = respond
    })

    it('sets up company sign-in, signs in through the browser, chooses the credential and signs out, through the v2 sign-in panel; a refused save is named (C20, attempt-01 E2)', async () => {
        const { native, user } = await launch(withHarness({ signIn: null }))
        await openSetup(user)
        await user.click(screen.getByRole('button', { name: 'Set up company sign-in' }))
        const form = screen.getByRole('button', { name: 'Save company sign-in' }).closest('form') as HTMLFormElement
        await user.type(within(form).getByLabelText('Gateway endpoint'), 'https://gateway.example/rt/v1')
        await user.type(within(form).getByLabelText('Identity provider (issuer URL)'), 'https://id.example/oauth2/default')
        await user.type(within(form).getByLabelText('Client ID'), 'desktop-client')
        await user.type(within(form).getByLabelText('Gateway audience'), 'https://gateway.example')
        await user.click(screen.getByRole('button', { name: 'Save company sign-in' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({ kind: 'configureHarnessSignIn', context: CONTEXT, workspace: REF, endpoint: 'https://gateway.example/rt/v1', issuer: 'https://id.example/oauth2/default', clientId: 'desktop-client', audience: 'https://gateway.example' })
        )
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'chooseHarnessCredential', context: CONTEXT, source: 'companySignIn' }))
        expect(native.dispatched.some((i) => i.kind === 'signInHarness')).toBe(false)
        const signIn = (over: Partial<HarnessSignInView> = {}): HarnessSignInView => ({ workspace: WS, endpoint: 'https://gateway.example/rt/v1', issuer: 'https://id.example/oauth2/default', clientId: 'desktop-client', audience: 'https://gateway.example', selected: true, account: null, step: null, failure: null, ...over })
        await native.push({ ...native.current, harness: { ...native.current.harness, signIn: signIn() } })
        const choice = await screen.findByRole('group', { name: 'Answer with' })
        expect(within(choice).getByRole('radio', { name: /Company sign-in/ })).toBeChecked()
        await user.click(screen.getByRole('button', { name: 'Sign in with your browser' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'signInHarness', context: CONTEXT }))
        await native.push({ ...native.current, harness: { ...native.current.harness, signIn: signIn({ step: 'browser' }) } })
        expect(await screen.findByText('Continue in your browser. This app is waiting for it to come back.')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Cancel sign-in' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'cancelHarnessSignIn', context: CONTEXT }))
        await native.push({ ...native.current, harness: { ...native.current.harness, signIn: signIn({ failure: 'invalidToken' }) } })
        expect(await screen.findByText(/A token from the identity provider did not verify/)).toBeInTheDocument()
        await native.push({ ...native.current, harness: { ...native.current.harness, signIn: signIn({ account: 'alice@example.com' }) } })
        expect(await screen.findByText('Signed in as alice@example.com')).toBeInTheDocument()
        await user.click(within(screen.getByRole('group', { name: 'Answer with' })).getByRole('radio', { name: /The workspace inference key/ }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'chooseHarnessCredential', context: CONTEXT, source: 'workspaceKey' }))
        await user.click(screen.getByRole('button', { name: 'Sign out' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'signOutHarness', context: CONTEXT }))
        expect(document.body.textContent).not.toContain('eyJ')
        // A refused save names why and chooses nothing.
        await native.push({ ...native.current, harness: { ...native.current.harness, signIn: null } })
        const chosen = native.dispatched.filter((i) => i.kind === 'chooseHarnessCredential').length
        native.respond = (intent: Intent) => (intent.kind === 'configureHarnessSignIn' ? { outcome: 'refusal', refusal: 'invalidInput' } : { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        await user.click(await screen.findByRole('button', { name: 'Set up company sign-in' }))
        const again = screen.getByRole('button', { name: 'Save company sign-in' }).closest('form') as HTMLFormElement
        await user.type(within(again).getByLabelText('Gateway endpoint'), 'https://gateway.example/rt/v1')
        await user.type(within(again).getByLabelText('Identity provider (issuer URL)'), 'http://id.example/?x=1')
        await user.type(within(again).getByLabelText('Client ID'), 'desktop-client')
        await user.type(within(again).getByLabelText('Gateway audience'), 'https://gateway.example')
        await user.click(screen.getByRole('button', { name: 'Save company sign-in' }))
        expect(await screen.findByText('Issuer and Gateway endpoint must be https URLs without a query string.')).toBeInTheDocument()
        expect(native.dispatched.filter((i) => i.kind === 'chooseHarnessCredential')).toHaveLength(chosen)
    })

    it('reads the MCP catalog, turns servers on, signs in and out, says why a server cannot be used and keeps at most eight on, through the v2 MCP panel (C22, attempt-01 E2)', async () => {
        const { native, user } = await launch(withHarness({ mcp: null }))
        await openSetup(user)
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'listMcpServers', context: CONTEXT, workspace: REF }))
        const panel = screen.getByRole('region', { name: 'Workspace MCP servers' })
        expect(within(panel).getByText('Reading the workspace’s MCP servers…')).toBeInTheDocument()
        await native.push({ ...native.current, harness: { ...native.current.harness, mcp: mcpOf([server(), server({ id: 'odd id', name: 'Odd', namespace: 'odd', supported: false })]) } })
        const use = await within(panel).findByRole('checkbox', { name: 'Use Tools in new conversations' })
        expect(use).toHaveAccessibleDescription('Not signed in: its tools will be unavailable until you sign in.')
        expect(within(panel).getByRole('checkbox', { name: 'Use Odd in new conversations' })).toBeDisabled()
        await user.click(use)
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'enableMcpServer', context: CONTEXT, workspace: REF, server: 'srv-tools-0001', enabled: true }))
        await user.click(within(panel).getByRole('button', { name: 'Sign in to Tools' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'signInMcpServer', context: CONTEXT, workspace: REF, server: 'srv-tools-0001' }))
        await native.push({ ...native.current, harness: { ...native.current.harness, mcp: mcpOf([server({ enabled: true, step: 'browser' })]) } })
        expect(await within(panel).findByText('Continue in your browser. This app is waiting for it to come back.')).toBeInTheDocument()
        await user.click(within(panel).getByRole('button', { name: 'Cancel sign-in' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'cancelMcpSignIn', context: CONTEXT }))
        await native.push({ ...native.current, harness: { ...native.current.harness, mcp: mcpOf([server({ enabled: true, signedIn: true })]) } })
        expect(await within(panel).findByText('Signed in.')).toBeInTheDocument()
        expect(summary()).toHaveTextContent('ToolsSigned in')
        await user.click(within(panel).getByRole('button', { name: 'Sign out of Tools' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'signOutMcpServer', context: CONTEXT, workspace: REF, server: 'srv-tools-0001' }))
        const many = Array.from({ length: 9 }, (_, i) => server({ id: `srv-${i}`, name: `S${i}`, namespace: `s${i}`, enabled: i < 8, signedIn: i === 0 }))
        many[1] = { ...many[1]!, stale: true }
        many[2] = { ...many[2]!, expired: 'sessionExpired' }
        many[3] = { ...many[3]!, failure: 'registrationNotOffered' }
        await native.push({ ...native.current, harness: { ...native.current.harness, mcp: mcpOf(many, { failure: 'unavailable', complete: false }) } })
        expect(await within(panel).findByRole('checkbox', { name: 'Use S8 in new conversations' })).toBeDisabled()
        expect(within(panel).getByRole('checkbox', { name: 'Use S7 in new conversations' })).toBeEnabled()
        expect(within(panel).getByText('Signed in for an earlier version of this server. Sign in again to use it.')).toBeInTheDocument()
        expect(within(panel).getByText('The sign-in has expired. Sign in again to use its tools.')).toBeInTheDocument()
        expect(within(panel).getByText('The server does not let this app register for a sign-in.')).toBeInTheDocument()
        expect(within(panel).getByText(/^The MCP servers could not be read:/)).toBeInTheDocument()
        expect(within(panel).getByText('Only the first 500 servers are shown.')).toBeInTheDocument()
        expect(within(panel).getByRole('button', { name: 'Sign in to S1 again' })).toBeInTheDocument()
        // A refused enable is said.
        native.respond = (intent: Intent) => (intent.kind === 'enableMcpServer' ? { outcome: 'refusal', refusal: 'invalidInput' } : { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } })
        await user.click(within(panel).getByRole('checkbox', { name: 'Use S7 in new conversations' }))
        expect(await screen.findByText(/That was not done\. At most eight servers can be on/)).toBeInTheDocument()
    })

    it('lists, opens, continues, exports and deletes saved conversations, says why one cannot be continued, names unreadable ones and a locked store, through the v2 history panel (C20, attempt-01 E2)', async () => {
        const SAVED = { id: '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6d', title: 'Remember periwinkle', updated: 1_790_000_000, entries: 4, source: 'workspaceKey' as const }
        const historyOf = (over: Partial<HarnessHistoryView> = {}): HarnessHistoryView => ({ local: false, state: 'ready', conversations: [SAVED], unreadable: [], failure: null, full: false, opened: null, busy: false, problem: null, export: null, ...over })
        const { native, user } = await launch(withHarness({ history: null }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'listHarnessHistory', context: CONTEXT, workspace: REF }))
        expect(screen.getByText('Reading saved conversations…')).toBeInTheDocument()
        await native.push({ ...native.current, harness: { ...native.current.harness, history: historyOf() } })
        const list = await screen.findByRole('list', { name: 'Saved conversations' })
        expect(within(list).getByText('4 messages', { exact: false })).toBeInTheDocument()
        await user.click(within(list).getByRole('button', { name: 'Open Remember periwinkle' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'openHarnessHistory', context: CONTEXT, workspace: REF, id: SAVED.id }))
        const opened = { id: SAVED.id, title: SAVED.title, updated: SAVED.updated, entries: [{ id: 'u-1', role: 'operator' as const, text: 'Remember periwinkle', state: 'complete' as const }, { id: 'u-1/msg_1', role: 'assistant' as const, text: 'Noted.', state: 'complete' as const }], source: 'workspaceKey' as const, notContinuable: null }
        await native.push({ ...native.current, harness: { ...native.current.harness, history: historyOf({ opened }) } })
        const heading = await screen.findByRole('heading', { name: 'Remember periwinkle', level: 3 })
        await waitFor(() => expect(heading).toHaveFocus())
        expect(within(screen.getByRole('list', { name: 'Saved messages' })).getAllByRole('listitem')).toHaveLength(2)
        await user.click(screen.getByRole('button', { name: 'Continue this conversation' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'resumeHarness', context: CONTEXT, workspace: REF, id: SAVED.id }))
        await user.selectOptions(screen.getByLabelText('Export as'), 'json')
        await user.click(screen.getByRole('button', { name: 'Export…' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'exportHarnessHistory', context: CONTEXT, workspace: REF, id: SAVED.id, format: 'json' }))
        await native.push({ ...native.current, harness: { ...native.current.harness, history: historyOf({ opened, export: { kind: 'harnessConversation', state: 'saved', name: 'harness-x.json' } as never }) } })
        expect(await screen.findByText('Saved harness-x.json.')).toBeInTheDocument()
        const before = native.dispatched.length
        await user.click(screen.getByRole('button', { name: 'Delete…' }))
        expect(screen.getByText('Delete this saved conversation from this Mac? It cannot be recovered.')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Keep it' }))
        expect(native.dispatched.slice(before)).toEqual([])
        await user.click(screen.getByRole('button', { name: 'Delete…' }))
        await user.click(screen.getByRole('button', { name: 'Delete permanently' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'deleteHarnessHistory', context: CONTEXT, workspace: REF, id: SAVED.id }))
        // Not continuable, unreadable and full, then a locked store.
        const unreadable = '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6e'
        await native.push({ ...native.current, harness: { ...native.current.harness, history: historyOf({ opened: { ...opened, title: 'Old key', entries: [], notContinuable: 'otherCredential' }, unreadable: [unreadable], full: true }) } })
        expect(await screen.findByText('It ran with another key, sign-in or endpoint than this workspace uses now, so it can be read and exported but not continued.')).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Continue this conversation' })).toBeNull()
        expect(screen.getAllByText('This workspace already keeps 16 saved conversations. Delete one to keep new conversations.').length).toBeGreaterThan(0)
        const sent = native.dispatched.length
        await user.click(screen.getByRole('button', { name: 'Delete unreadable conversation…' }))
        await user.click(screen.getByRole('button', { name: 'Keep it' }))
        expect(native.dispatched.slice(sent).some((i) => i.kind === 'deleteHarnessHistory')).toBe(false)
        await user.click(screen.getByRole('button', { name: 'Delete unreadable conversation…' }))
        await user.click(screen.getByRole('button', { name: 'Delete permanently' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'deleteHarnessHistory', context: CONTEXT, workspace: REF, id: unreadable }))
        await native.push({ ...native.current, harness: { ...native.current.harness, history: historyOf({ state: 'failed', failure: 'locked', conversations: [] }) } })
        expect(await screen.findByText(/The Keychain is locked, so saved conversations cannot be read or saved\./)).toBeInTheDocument()
    })
})
