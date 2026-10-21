/**
 * The native Harness (C19) through the production App and DesktopClient
 * against a scripted native host: a conversation is offered only with the
 * bundled engine, a chosen Gateway workspace and the inference key stored
 * for it; it streams, a message is sent with ⌘↩, an answer can be stopped
 * and the conversation ended, focus follows each step, and every failure is
 * named without the engine's own text.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { type ConversationView, DesktopClient, type GatewayView, type ListedWorkspace, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'
import { HARNESS_FAILURE_TEXT } from './harness-view'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const REF_A = 'gw-aaaaaaaaaaaaaaaaaaaaaaaa'
const A: ListedWorkspace = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8297', name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }
const B: ListedWorkspace = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8298', name: 'Team B', slug: 'team-b', scopeName: 'scope-team-b' }
const ENDPOINT = 'https://gateway.example/rt/v1'

const gateway = (selected: ListedWorkspace | null): GatewayView => ({
    workspaces: [A, B],
    complete: true,
    selected: selected ? { reference: REF_A, workspace: selected, restored: false } : null,
    remembered: null,
    rememberedMissing: false,
    local: { state: 'ready', failure: null, drafts: [], records: [], playground: [], runs: [], files: [], problem: null, unsaved: false },
    playground: null,
    files: null,
})

type Key = ShellSnapshot['tenants'][number]['inference']
const keyFor = (workspace: ListedWorkspace): Key => ({ state: 'ready', mode: 'keychain', failure: null, target: { workspace, endpoint: ENDPOINT } })

function snapshot(options: { engine?: boolean; selected?: ListedWorkspace | null; key?: Key; conversation?: ConversationView | null } = {}): ShellSnapshot {
    const s = withTenant(baseSnapshot, tenant({ inference: options.key ?? keyFor(A) }), CONTEXT)
    return {
        ...s,
        gateway: gateway(options.selected === undefined ? A : options.selected),
        harness: { engine: { ...baseSnapshot.harness.engine, available: options.engine ?? true }, conversation: options.conversation ?? null },
    }
}

const conversation = (overrides: Partial<ConversationView> = {}): ConversationView => ({
    id: 'op-7',
    workspace: A,
    endpoint: ENDPOINT,
    phase: 'starting',
    failure: null,
    stop: null,
    entries: [],
    lastTurn: null,
    usage: null,
    cleanup: null,
    runtimeVersion: null,
    ...overrides,
})

async function open(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Harness/ }))
    await screen.findByRole('heading', { level: 1, name: 'Harness' })
    return user
}

describe('the native Harness (C19)', () => {
    it('offers a conversation only with the bundled engine, a chosen workspace and its stored key', async () => {
        const native = scriptedNative(snapshot({ engine: false }))
        const user = await open(native)
        expect(screen.getByText('The Harness engine is not bundled with this build.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Start conversation' })).toBeDisabled()
        expect(screen.getByText(/Engine 0\.1\.7-desktop\.2 · 9331a5839bc6 · airs-desktop\/1/)).toBeInTheDocument()

        await native.push(snapshot({ selected: null }))
        expect(await screen.findByText('Choose a workspace in AI Gateway first; the Harness answers with its inference key.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Start conversation' })).toBeDisabled()

        await native.push(snapshot({ key: keyFor(B) }))
        expect(await screen.findByText('Store an inference key for Team A in the AI Gateway Playground to use the Harness.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Start conversation' })).toBeDisabled()

        await native.push(snapshot())
        const binding = await screen.findByText(ENDPOINT)
        expect(binding.closest('dl')).toHaveTextContent('WorkspaceTeam A')
        await user.click(screen.getByRole('button', { name: 'Start conversation' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'startHarness', context: CONTEXT, workspace: REF_A }))

        // Going to the AI Gateway is offered when the workspace or key is missing.
        await native.push(snapshot({ selected: null }))
        await user.click(await screen.findByRole('button', { name: 'Go to AI Gateway' }))
        expect(await screen.findByRole('heading', { level: 1, name: 'Gateway workspaces and configuration' })).toBeInTheDocument()
    })

    it('streams a conversation, sends with ⌘↩, stops an answer and ends it with focus following', async () => {
        const native = scriptedNative(snapshot({ conversation: conversation() }))
        const user = await open(native)
        expect(screen.getByText('Starting the engine…')).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: /Start/ })).toBeNull()

        await native.push(snapshot({ conversation: conversation({ phase: 'ready', runtimeVersion: '0.1.7-desktop.2' }) }))
        const box = await screen.findByLabelText('Message')
        await waitFor(() => expect(box).toHaveFocus())
        expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled()
        await user.type(box, 'Say hello')
        await user.keyboard('{Meta>}{Enter}{/Meta}')
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'sendHarness', conversation: 'op-7', prompt: 'Say hello' }))
        await waitFor(() => expect(box).toHaveValue(''))

        await native.push(
            snapshot({
                conversation: conversation({
                    phase: 'answering',
                    entries: [
                        { id: 'u-1', role: 'operator', text: 'Say hello', state: 'complete' },
                        { id: 'u-1/msg_1', role: 'assistant', text: 'Hello fr', state: 'streaming' },
                    ],
                }),
            })
        )
        const transcript = await screen.findByRole('list', { name: 'Messages' })
        expect(transcript).toHaveAttribute('aria-busy', 'true')
        const items = within(transcript).getAllByRole('listitem')
        expect(items.map((i) => i.textContent)).toEqual(['YouSay hello', 'HarnessHello fr'])
        // A draft can be written while answering but is not sent until ready.
        await user.type(box, 'Next')
        expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled()
        const sent = native.dispatched.length
        await user.keyboard('{Meta>}{Enter}{/Meta}')
        expect(native.dispatched.slice(sent)).toEqual([])
        await user.clear(box)
        await user.click(screen.getByRole('button', { name: 'Stop answer' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'interruptHarness', conversation: 'op-7' }))

        await native.push(
            snapshot({
                conversation: conversation({
                    phase: 'ready',
                    lastTurn: 'interrupted',
                    entries: [
                        { id: 'u-1', role: 'operator', text: 'Say hello', state: 'complete' },
                        { id: 'u-1/msg_1', role: 'assistant', text: 'Hello fr', state: 'interrupted' },
                    ],
                    usage: { input: 10, output: 3, total: 13 },
                }),
            })
        )
        expect(await screen.findByText('The answer was stopped.')).toBeInTheDocument()
        expect(screen.getByText('(stopped before it finished)')).toBeInTheDocument()
        expect(screen.getByText('Tokens so far: 10 in, 3 out.')).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Stop answer' })).toBeNull()

        // A message over 32 KiB is not sent.
        await user.click(box)
        await user.paste('é'.repeat(16 * 1024 + 1))
        expect(screen.getByText('This message is longer than 32 KiB. Shorten it to send.')).toBeInTheDocument()
        expect(box).toHaveAttribute('aria-invalid', 'true')
        expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled()
        await user.clear(box)

        await user.click(screen.getByRole('button', { name: 'End conversation' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'endHarness', conversation: 'op-7' }))
        await native.push(snapshot({ conversation: conversation({ phase: 'ending', stop: 'ended' }) }))
        expect(await screen.findByText('Ending the conversation…')).toBeInTheDocument()
        await native.push(snapshot({ conversation: conversation({ phase: 'ended', stop: 'ended', cleanup: 'graceful' }) }))
        const status = await screen.findByText('You ended this conversation.')
        await waitFor(() => expect(status).toHaveFocus())
        expect(screen.queryByLabelText('Message')).toBeNull()

        // A new conversation clears the ended one first, then starts.
        const before = native.dispatched.length
        await user.click(screen.getByRole('button', { name: 'Start a new conversation' }))
        await waitFor(() =>
            expect(native.dispatched.slice(before)).toEqual([
                { kind: 'clearHarness', conversation: 'op-7' },
                { kind: 'startHarness', context: CONTEXT, workspace: REF_A },
            ])
        )
    })

    it('names every failure and stop without the engine’s text and never shows the key', async () => {
        const native = scriptedNative(snapshot({ conversation: conversation({ phase: 'failed', failure: 'exited', cleanup: 'graceful' }) }))
        const user = await open(native)
        expect(screen.getAllByText(HARNESS_FAILURE_TEXT.exited).length).toBeGreaterThan(0)
        expect(screen.getByRole('alert')).toHaveTextContent(HARNESS_FAILURE_TEXT.exited)
        for (const [stop, text] of [
            ['sleep', 'This conversation ended when the Mac went to sleep.'],
            ['quit', 'This conversation ended when the app quit.'],
            ['deadline', 'This conversation reached its eight-hour limit and ended.'],
        ] as const) {
            await native.push(snapshot({ conversation: conversation({ phase: 'ended', stop, cleanup: 'forced' }) }))
            expect(await screen.findByText(text)).toBeInTheDocument()
        }
        await native.push(
            snapshot({
                conversation: conversation({
                    phase: 'ready',
                    lastTurn: 'truncated',
                    entries: [{ id: 'u-1/m', role: 'assistant', text: 'y'.repeat(100), state: 'truncated' }],
                }),
            })
        )
        expect(await screen.findByText('The answer reached 256 KiB and was stopped; the rest was not kept.')).toBeInTheDocument()
        expect(screen.getByText('(cut at 256 KiB)')).toBeInTheDocument()
        // A failed answer says the engine may have sent it more than once
        // (D046: up to five requests before the app stops its retries).
        await native.push(snapshot({ conversation: conversation({ phase: 'ready', lastTurn: 'failed' }) }))
        expect(
            await screen.findByText(
                'The answer failed: the Gateway or the model ended it with an error. The engine may have sent the message to the Gateway up to five times before this app stopped it, and each may have been processed; nothing was undone.'
            )
        ).toBeInTheDocument()
        expect(document.body.textContent).not.toContain('not sent again')
        // Clearing returns focus to Start.
        await native.push(snapshot({ conversation: conversation({ phase: 'ended', stop: 'ended', cleanup: 'graceful' }) }))
        await user.click(await screen.findByRole('button', { name: 'Clear conversation' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'clearHarness', conversation: 'op-7' }))
        await native.push(snapshot())
        await waitFor(() => expect(screen.getByRole('button', { name: 'Start conversation' })).toHaveFocus())
        expect(document.body.textContent).not.toContain('gw-')
    })
})
