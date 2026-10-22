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
import {
    type ConversationView,
    DesktopClient,
    type GatewayView,
    type HarnessHistoryView,
    type HarnessProjectView,
    type HarnessSignInView,
    type ListedWorkspace,
    type ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
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

function snapshot(
    options: {
        engine?: boolean
        selected?: ListedWorkspace | null
        key?: Key
        conversation?: ConversationView | null
        signIn?: HarnessSignInView | null
        history?: HarnessHistoryView | null
        project?: HarnessProjectView | null
        projectRefused?: string | null
    } = {}
): ShellSnapshot {
    const s = withTenant(baseSnapshot, tenant({ inference: options.key ?? keyFor(A) }), CONTEXT)
    return {
        ...s,
        gateway: gateway(options.selected === undefined ? A : options.selected),
        harness: {
            engine: { ...baseSnapshot.harness.engine, available: options.engine ?? true },
            conversation: options.conversation ?? null,
            signIn: options.signIn ?? null,
            history: options.history ?? null,
            project: options.project ?? null,
            projectRefused: options.projectRefused ?? null,
        },
    }
}

const ISSUER = 'https://id.example/oauth2/default'
const signIn = (overrides: Partial<HarnessSignInView> = {}): HarnessSignInView => ({
    workspace: A,
    endpoint: ENDPOINT,
    issuer: ISSUER,
    clientId: 'desktop-client',
    audience: 'https://gateway.example',
    selected: true,
    account: null,
    step: null,
    failure: null,
    ...overrides,
})

const SAVED_ID = '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6d'
const history = (overrides: Partial<HarnessHistoryView> = {}): HarnessHistoryView => ({
    state: 'ready',
    conversations: [{ id: SAVED_ID, title: 'Remember periwinkle', updated: 1_790_000_000, entries: 4, source: 'workspaceKey' }],
    unreadable: [],
    failure: null,
    full: false,
    opened: null,
    busy: false,
    problem: null,
    export: null,
    ...overrides,
})

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
    source: 'workspaceKey',
    historyId: SAVED_ID,
    savedAt: null,
    saveFailure: null,
    unsent: null,
    signInFailure: null,
    project: null,
    write: false,
    approvals: [],
    proposals: [],
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
        expect(
            await screen.findByText('Store an inference key for Team A in the AI Gateway Playground, or set up company sign-in below, to use the Harness.')
        ).toBeInTheDocument()
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

describe('company sign-in and protected history (C20)', () => {
    it('sets up company sign-in, signs in through the browser, chooses it and signs out', async () => {
        const native = scriptedNative(snapshot())
        const user = await open(native)
        await user.click(screen.getByRole('button', { name: 'Set up company sign-in' }))
        const form = screen.getByRole('button', { name: 'Save company sign-in' }).closest('form') as HTMLFormElement
        await user.type(within(form).getByLabelText('Gateway endpoint'), ENDPOINT)
        await user.type(within(form).getByLabelText('Identity provider (issuer URL)'), ISSUER)
        await user.type(within(form).getByLabelText('Client ID'), 'desktop-client')
        await user.type(within(form).getByLabelText('Gateway audience'), 'https://gateway.example')
        await user.click(screen.getByRole('button', { name: 'Save company sign-in' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'configureHarnessSignIn',
                context: CONTEXT,
                workspace: REF_A,
                endpoint: ENDPOINT,
                issuer: ISSUER,
                clientId: 'desktop-client',
                audience: 'https://gateway.example',
            })
        )

        await native.push(snapshot({ signIn: signIn({ selected: false }) }))
        const choice = await screen.findByRole('group', { name: 'Answer with' })
        expect(within(choice).getByRole('radio', { name: /The workspace inference key/ })).toBeChecked()
        expect(screen.getByRole('button', { name: 'Start conversation' })).toBeEnabled()
        await user.click(screen.getByRole('button', { name: 'Sign in with your browser' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'signInHarness', context: CONTEXT }))
        await native.push(snapshot({ signIn: signIn({ step: 'browser' }) }))
        expect(await screen.findByText('Continue in your browser. This app is waiting for it to come back.')).toBeInTheDocument()
        // Not signed in yet: no conversation with company sign-in.
        expect(screen.getByRole('button', { name: 'Start conversation' })).toBeDisabled()
        await user.click(screen.getByRole('button', { name: 'Cancel sign-in' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'cancelHarnessSignIn', context: CONTEXT }))

        await native.push(snapshot({ signIn: signIn({ failure: 'invalidToken' }) }))
        expect(await screen.findByRole('alert')).toHaveTextContent('A token from the identity provider did not verify')

        await native.push(snapshot({ signIn: signIn({ account: 'alice@example.com' }) }))
        expect(await screen.findByText('Company sign-in (alice@example.com)')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Start conversation' })).toBeEnabled()
        await user.click(within(screen.getByRole('group', { name: 'Answer with' })).getByRole('radio', { name: /The workspace inference key/ }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({ kind: 'chooseHarnessCredential', context: CONTEXT, source: 'workspaceKey' })
        )
        await user.click(screen.getByRole('button', { name: 'Sign out' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'signOutHarness', context: CONTEXT }))
        expect(document.body.textContent).not.toContain('eyJ')
    })

    it('gives a message back when its renewal fails and shows the conversation as saved or not', async () => {
        const native = scriptedNative(
            snapshot({
                signIn: signIn({ account: 'alice@example.com' }),
                conversation: conversation({ phase: 'ready', source: 'companySignIn', savedAt: 1_790_000_000 }),
            })
        )
        await open(native)
        expect(await screen.findByText(/^Saved /, { selector: '[data-slot="harness-saved"]' })).toBeInTheDocument()
        await native.push(
            snapshot({
                signIn: signIn({ account: 'alice@example.com', failure: 'sessionExpired' }),
                conversation: conversation({
                    phase: 'ready',
                    source: 'companySignIn',
                    unsent: 'What did I ask?',
                    signInFailure: 'sessionExpired',
                    saveFailure: 'locked',
                }),
            })
        )
        const box = await screen.findByLabelText('Message')
        await waitFor(() => expect(box).toHaveValue('What did I ask?'))
        await waitFor(() => expect(box).toHaveFocus())
        expect(screen.getByText('Your sign-in has expired. Sign in again to send. Your message was not sent; it is back in the box.')).toBeInTheDocument()
        expect(screen.getByText(/^Not saved: The Keychain is locked/)).toBeInTheDocument()
        await native.push(snapshot({ conversation: conversation({ phase: 'renewing', source: 'companySignIn' }) }))
        expect(await screen.findByText('Renewing your sign-in and restarting the engine with the conversation…')).toBeInTheDocument()
    })

    it('lists, opens, continues, exports and deletes saved conversations', async () => {
        const native = scriptedNative(snapshot({ history: null }))
        const user = await open(native)
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'listHarnessHistory', context: CONTEXT, workspace: REF_A }))
        expect(screen.getByText('Reading saved conversations…')).toBeInTheDocument()
        await native.push(snapshot({ history: history() }))
        const list = await screen.findByRole('list', { name: 'Saved conversations' })
        expect(within(list).getByText('4 messages', { exact: false })).toBeInTheDocument()
        await user.click(within(list).getByRole('button', { name: 'Open Remember periwinkle' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'openHarnessHistory', context: CONTEXT, workspace: REF_A, id: SAVED_ID }))
        const opened = {
            id: SAVED_ID,
            title: 'Remember periwinkle',
            updated: 1_790_000_000,
            entries: [
                { id: 'u-1', role: 'operator' as const, text: 'Remember periwinkle', state: 'complete' as const },
                { id: 'u-1/msg_1', role: 'assistant' as const, text: 'Noted.', state: 'complete' as const },
            ],
            source: 'workspaceKey' as const,
            notContinuable: null,
        }
        await native.push(snapshot({ history: history({ opened }) }))
        const heading = await screen.findByRole('heading', { name: 'Remember periwinkle' })
        await waitFor(() => expect(heading).toHaveFocus())
        expect(within(screen.getByRole('list', { name: 'Saved messages' })).getAllByRole('listitem')).toHaveLength(2)
        await user.click(screen.getByRole('button', { name: 'Continue this conversation' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'resumeHarness', context: CONTEXT, workspace: REF_A, id: SAVED_ID }))
        await user.selectOptions(screen.getByLabelText('Export as'), 'json')
        await user.click(screen.getByRole('button', { name: 'Export…' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({ kind: 'exportHarnessHistory', context: CONTEXT, workspace: REF_A, id: SAVED_ID, format: 'json' })
        )
        await native.push(snapshot({ history: history({ opened, export: { kind: 'harnessConversation', state: 'saved', name: 'harness-x.json' } }) }))
        expect(await screen.findByText('Saved harness-x.json.')).toBeInTheDocument()
        // Deleting asks first.
        const before = native.dispatched.length
        await user.click(screen.getByRole('button', { name: 'Delete…' }))
        expect(screen.getByText('Delete this saved conversation from this Mac? It cannot be recovered.')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Keep it' }))
        expect(native.dispatched.slice(before)).toEqual([])
        await user.click(screen.getByRole('button', { name: 'Delete…' }))
        await user.click(screen.getByRole('button', { name: 'Delete permanently' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'deleteHarnessHistory', context: CONTEXT, workspace: REF_A, id: SAVED_ID }))
        await native.push(snapshot({ history: history({ conversations: [] }) }))
        expect(await screen.findByText('No saved conversations in this workspace.')).toBeInTheDocument()
        await waitFor(() => expect(screen.getByRole('heading', { name: 'Saved conversations' })).toHaveFocus())
    })

    it('says why a saved conversation cannot be continued, names unreadable ones and a full workspace', async () => {
        const opened = {
            id: SAVED_ID,
            title: 'Old key',
            updated: 1_790_000_000,
            entries: [],
            source: 'workspaceKey' as const,
            notContinuable: 'otherCredential' as const,
        }
        const unreadable = '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6e'
        const native = scriptedNative(snapshot({ history: history({ opened, unreadable: [unreadable], full: true }) }))
        const user = await open(native)
        expect(
            await screen.findByText(
                'It ran with another key, sign-in or endpoint than this workspace uses now, so it can be read and exported but not continued.'
            )
        ).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Continue this conversation' })).toBeNull()
        expect(screen.getAllByText('This workspace already keeps 16 saved conversations. Delete one to keep new conversations.').length).toBeGreaterThan(0)
        await user.click(screen.getByRole('button', { name: 'Delete unreadable conversation' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'deleteHarnessHistory', context: CONTEXT, workspace: REF_A, id: unreadable }))
        await native.push(snapshot({ history: history({ state: 'failed', failure: 'locked', conversations: [] }) }))
        expect(await screen.findByRole('alert')).toHaveTextContent('The Keychain is locked, so saved conversations cannot be read or saved.')
    })
})

describe('Harness tools, project and approvals (C21)', () => {
    const PROJECT = '/Users/someone/work/agent-demo'
    const project = (write = false): HarnessProjectView => ({ path: PROJECT, write })

    it('chooses a folder, allows changes in it and removes it', async () => {
        const native = scriptedNative(snapshot())
        const user = await open(native)
        const panel = screen.getByRole('region', { name: 'Project folder' })
        expect(within(panel).getByText(/cannot run commands/)).toBeInTheDocument()
        await user.click(within(panel).getByRole('button', { name: 'Choose a folder…' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'chooseHarnessProject', context: CONTEXT }))

        // A refused folder is named with the reason; the sheet has closed.
        await native.push(snapshot({ projectRefused: '/usr' }))
        expect(await within(panel).findByRole('alert')).toHaveTextContent(
            '/usr cannot be used. Folders that hold system files, credentials or this app’s own data cannot be used.'
        )
        await native.push(snapshot({ project: project() }))
        expect(await within(panel).findByText(PROJECT)).toBeInTheDocument()
        expect(within(panel).queryByRole('alert')).toBeNull()
        const write = within(panel).getByRole('checkbox', { name: 'Allow changes in this folder' })
        expect(write).not.toBeChecked()
        expect(write).toHaveAccessibleDescription(/Read-only/)
        await user.click(write)
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'setHarnessWrite', context: CONTEXT, write: true }))

        await native.push(snapshot({ project: project(true), conversation: conversation({ phase: 'ready', project: PROJECT, write: true }) }))
        await waitFor(() => expect(write).toBeChecked())
        expect(within(panel).getByText('Changing the folder or its access ends the running conversation.')).toBeInTheDocument()
        expect(screen.getByText(`Folder ${PROJECT} · changes allowed, each with your approval`)).toBeInTheDocument()
        await user.click(within(panel).getByRole('button', { name: 'Remove folder' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'clearHarnessProject', context: CONTEXT }))
    })

    it('shows what the engine asks to do and sends the operator’s decision', async () => {
        const command = { id: '0123456789abcdef01234567', kind: 'command' as const, command: "/bin/zsh -c 'ls'", cwd: PROJECT, reason: 'List the files', files: [] }
        const change = { id: '89abcdef0123456789abcdef', kind: 'fileChange' as const, command: null, cwd: null, reason: null, files: [`${PROJECT}/notes.txt`] }
        const native = scriptedNative(
            snapshot({
                project: project(true),
                conversation: conversation({
                    phase: 'answering',
                    project: PROJECT,
                    write: true,
                    entries: [
                        { id: 'u-1', role: 'operator', text: 'Tidy up', state: 'complete' },
                        { id: 'u-1/c1', role: 'command', text: "/bin/zsh -c 'ls'", state: 'streaming' },
                        { id: 'u-1/c2', role: 'tool', text: 'Read the security profiles', state: 'complete' },
                        { id: 'u-1/c3', role: 'fileChange', text: `${PROJECT}/old.txt`, state: 'declined' },
                    ],
                    approvals: [command, change],
                }),
            })
        )
        const user = await open(native)
        const transcript = screen.getByRole('list', { name: 'Messages' })
        expect(within(transcript).getAllByRole('listitem').map((i) => i.textContent)).toEqual([
            'YouTidy up',
            "Command/bin/zsh -c 'ls'",
            'App toolRead the security profiles',
            `File change${PROJECT}/old.txt(declined)`,
        ])
        const waiting = screen.getByRole('region', { name: 'Waiting for your approval' })
        const [run, write] = within(waiting).getAllByRole('listitem').filter((li) => li.parentElement?.getAttribute('aria-label') !== 'Files')
        expect(run).toHaveTextContent('Run a command')
        expect(run).toHaveTextContent('Reason given: List the files')
        expect(write).toHaveTextContent(`Change files${PROJECT}/notes.txt`)
        await user.click(within(run!).getByRole('button', { name: 'Allow' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({ kind: 'decideHarnessApproval', conversation: 'op-7', approval: command.id, allow: true })
        )
        await user.click(within(write!).getByRole('button', { name: 'Decline' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({ kind: 'decideHarnessApproval', conversation: 'op-7', approval: change.id, allow: false })
        )
    })

    it('opens a proposal only in Administration’s review, and dismisses one', async () => {
        const proposal = (id: string, state: 'staged' | 'inReview' | 'dismissed', review: string | null = null) => ({
            id,
            summary: `Proposal ${id}`,
            draft: { kind: 'profiles' as const, action: 'update' as const, target: 'default', body: '{"name":"default"}' },
            state,
            review,
        })
        const native = scriptedNative(
            snapshot({
                conversation: conversation({ phase: 'ready', proposals: [proposal('P-1', 'staged'), proposal('P-2', 'staged'), proposal('P-3', 'dismissed')] }),
            })
        )
        const user = await open(native)
        const list = screen.getByRole('region', { name: 'Proposed changes' })
        const items = within(list).getAllByRole('listitem')
        // Dismissed proposals are not shown.
        expect(items).toHaveLength(2)
        expect(items[0]).toHaveTextContent('Update security profile default')
        await user.click(within(items[1]!).getByRole('button', { name: 'Dismiss' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'dismissHarnessProposal', conversation: 'op-7', proposal: 'P-2' }))
        await user.click(within(items[0]!).getByRole('button', { name: 'Review' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'reviewHarnessProposal', conversation: 'op-7', proposal: 'P-1' }))
        expect(await screen.findByRole('heading', { level: 1, name: 'Tenant inventories' })).toBeInTheDocument()

        // Administration shows the opened review with the proposal's exact draft.
        const opened = snapshot({ conversation: conversation({ phase: 'ready', proposals: [proposal('P-1', 'inReview', 'rv-h1')] }) })
        await native.push({
            ...opened,
            tenants: opened.tenants.map((t) => ({
                ...t,
                management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1.iam.panserviceaccount.com', tokenOrigin: 'https://auth.example' },
            })),
            changes: {
                ...opened.changes,
                review: {
                    review: 'rv-h1',
                    kind: 'profiles',
                    action: 'update',
                    target: 'default',
                    state: 'ready',
                    failure: null,
                    before: '{"name":"default"}',
                    after: '{"name":"default"}',
                    sourceKey: null,
                    impact: 'Applies only to this tenant.',
                    confirmation: 'default',
                    workspace: null,
                    references: [],
                },
            },
        })
        expect(await screen.findByRole('heading', { name: 'Proposed by the Harness: update security profile default' })).toBeInTheDocument()
        expect(screen.getByLabelText('security profile draft (JSON)')).toHaveValue('{"name":"default"}')
    })

    it('says why a proposal could not be opened', async () => {
        const native = scriptedNative(
            snapshot({
                conversation: conversation({
                    phase: 'ready',
                    proposals: [
                        {
                            id: 'P-1',
                            summary: 'Add a topic',
                            draft: { kind: 'topics', action: 'create', target: '', body: '{}' },
                            state: 'staged',
                            review: null,
                        },
                    ],
                }),
            })
        )
        native.respond = (intent) =>
            intent.kind === 'reviewHarnessProposal'
                ? { outcome: 'refusal', refusal: 'busy' }
                : { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-x', operation: null } }
        const user = await open(native)
        await user.click(screen.getByRole('button', { name: 'Review' }))
        expect(await screen.findByRole('alert')).toHaveTextContent('Finish or discard the change already in review in Administration first.')
        expect(screen.getByRole('heading', { level: 1, name: 'Harness' })).toBeInTheDocument()
    })

    it('asks again before a restarted engine may change the folder', async () => {
        const native = scriptedNative(
            snapshot({ project: project(true), conversation: conversation({ phase: 'confirmWrite', project: PROJECT, write: true }) })
        )
        const user = await open(native)
        expect(screen.getByText('Waiting for you to allow changes in the folder again.')).toHaveAttribute('role', 'status')
        const ask = screen.getByText(new RegExp(`Allow it to change files in ${PROJECT} again\\?`))
        expect(ask).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Not now' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'confirmHarnessWrite', conversation: 'op-7', allow: false }))
        await user.click(screen.getByRole('button', { name: 'Allow changes and continue' }))
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'confirmHarnessWrite', conversation: 'op-7', allow: true }))
    })
})
