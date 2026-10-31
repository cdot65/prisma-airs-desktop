/**
 * C41: the API Client as R2App draws it (start=api), through the real App,
 * DesktopClient and parser over scripted workbench results. The board (the
 * chips, the tree, the tabs, the request line, the sub-tabs, the response
 * pane), the Params table writing the file's lines, the response tabs and
 * History, the collection menu and its sheet (opened from the keyboard,
 * focus returned to the chip), nested folders, re-opening a collection,
 * creating a request, sends from dirty and new requests, and closing with
 * unprotected edits. Nothing here sends by itself: every request the view
 * makes is listed and asserted.
 */
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { DesktopClient, type Intent, type RunView, type ShellSnapshot, type WorkbenchRequest, type WorkbenchResult } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { type Native, scriptedNative } from '../../test/native'
import { fxApi } from '../fixtures'
import { resetShellStore } from '../store'
import { PROTECT_DELAY_MS } from './request-editor'

beforeEach(() => resetShellStore())

function workbench(native: Native, answer: (request: WorkbenchRequest) => WorkbenchResult): WorkbenchRequest[] {
    const requests: WorkbenchRequest[] = []
    const pending = new Map<string, WorkbenchRequest>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind !== 'workbench') return respond(intent)
        requests.push(intent.request)
        const operation = `op-${requests.length}`
        pending.set(operation, intent.request)
        return {
            outcome: 'receipt',
            receipt: {
                kind: 'admitted',
                request: `rq-${requests.length}`,
                operation,
            },
        }
    }
    native.observeWorkbench = async (operation) => {
        const request = pending.get(operation)
        if (!request) return { kind: 'refusal', refusal: 'invalidInput' }
        return { kind: 'workbench', result: answer(request) }
    }
    return requests
}

const R1 = '1'.repeat(32)
const fields = {
    method: 'GET',
    url: 'http://127.0.0.1:8080/topics',
    headers: 'Accept: application/json',
    query: '',
    body: '',
    bodyKind: 'text',
    settings: '',
}
const document = (over: Partial<typeof fields> = {}) => ({
    kind: 'document' as const,
    document: {
        id: 'list',
        relative: 'topics/list.request.toml',
        revision: R1,
        fields: { ...fields, ...over },
        saved: { ...fields, ...over },
        new: false,
        recovered: false,
        bodyEditable: true,
        schema: null,
        conflict: null,
        interrupted: false,
        metadataDirty: false,
    },
    notes: [],
})
const ran: RunView = {
    record: {
        id: 'run-0123456789abcdef',
        request: 'list',
        relative: 'topics/list.request.toml',
        at: 1_760_000_000,
        method: 'GET',
        url: 'http://127.0.0.1:8080/topics',
        class: 'anonymous',
        operation: null,
        state: 'completed',
        failure: null,
        uncertain: false,
        status: 200,
        headers: [
            ['content-type', 'application/json'],
            ['content-length', '29'],
        ],
        redirect: null,
        elapsedMs: 42,
        bytes: 29,
        stored: true,
    },
    preview: '{"custom_topics":["Finance"]}',
    previewBytes: 29,
    tokenDropped: false,
}
const older: RunView = {
    ...ran,
    record: {
        ...ran.record,
        id: 'run-00000000000000ff',
        at: 1_759_000_000,
        elapsedMs: 17,
        bytes: 20,
    },
    preview: '{"custom_topics":[]}',
    previewBytes: 20,
}

/** A collection with a folder, a root request and two tabs. */
function board(): ShellSnapshot {
    const snapshot = fxApi()
    const collection = snapshot.workbench.collection!
    return {
        ...snapshot,
        workbench: {
            ...snapshot.workbench,
            collection: {
                ...collection,
                requests: [
                    ...collection.requests,
                    {
                        relative: 'health.request.toml',
                        id: 'health',
                        method: 'GET',
                        url: 'http://127.0.0.1:8080/health',
                    },
                ],
                environments: [{ name: 'local', origin: 'local' }],
                environment: 'local',
                archives: 2,
            },
        },
    }
}

const answers =
    (extra: (request: WorkbenchRequest) => WorkbenchResult | null = () => null) =>
    (request: WorkbenchRequest): WorkbenchResult => {
        const e = extra(request)
        if (e) return e
        switch (request.kind) {
            case 'openRequest':
                return document()
            case 'send':
                return { kind: 'ran', run: ran }
            case 'protect':
                return { kind: 'protected', id: request.id }
            case 'history':
                return {
                    kind: 'history',
                    runs: [ran.record, older.record],
                    more: false,
                }
            case 'showRun':
                return {
                    kind: 'ran',
                    run: request.run === older.record.id ? older : ran,
                }
            case 'preview':
                return {
                    kind: 'preview',
                    preview: {
                        method: 'GET',
                        url: request.fields.url,
                        headers: [['accept', 'application/json']],
                        query: [['page', '1']],
                        bodyKind: 'text',
                        body: '',
                        bodyBytes: 0,
                        variables: [],
                        unresolved: [],
                        missingSecrets: [],
                        auth: null,
                        authReferences: [],
                        problem: null,
                        jsonWarning: false,
                    },
                }
            case 'save':
                return {
                    kind: 'saved',
                    document: {
                        ...document(request.fields).document,
                        revision: '2'.repeat(32),
                    },
                }
            default:
                return { kind: 'done' }
        }
    }

async function open(snapshot = board(), extra?: (request: WorkbenchRequest) => WorkbenchResult | null) {
    const native = scriptedNative(snapshot)
    const requests = workbench(native, answers(extra))
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('heading', { level: 1, name: 'API Client' })
    // The tree by its slot: the list request's editor is a form named "Request list" too, and it may already be there.
    const tree = await waitFor(() => {
        const el = document_().querySelector('[data-slot="request-tree"]') as HTMLElement | null
        expect(el).not.toBeNull()
        return el as HTMLElement
    })
    const line = () => document_().querySelector('[data-slot="workbench-request-line"]') as HTMLElement
    await waitFor(() => expect(line()).not.toBeNull())
    return { native, requests, user, tree, line }
}
const document_ = () => globalThis.document

describe('C41 API Client board', () => {
    it('draws the title row with the chips, the tree, the tabs, the request line, the sub-tabs, the response pane and the footer (V2-T14)', async () => {
        const { tree, line } = await open()
        expect(document_().querySelector('[data-slot="api-client-v2"]')).not.toBeNull()
        expect(document_().querySelector('[data-slot="api-lead"]')).toHaveTextContent('Collection AIRS · api · Northwind Health · requests with tenant credentials go only to known operations')
        expect(screen.getByRole('button', { name: 'Collection AIRS' })).toHaveTextContent('AIRS')
        expect(screen.getByRole('combobox', { name: 'Environment' })).toHaveValue('local')
        // The tree: the folder with its count and the requests under it, the root request after it.
        expect(within(tree).getByRole('button', { name: 'Collapse folder topics' })).toHaveTextContent('topics1')
        expect(
            within(tree).getByRole('button', {
                name: 'Open GET topics/list.request.toml',
            })
        ).toHaveAttribute('aria-current', 'true')
        expect(
            within(tree).getByRole('button', {
                name: 'Open GET health.request.toml',
            })
        ).toBeInTheDocument()
        expect(within(tree).getByRole('button', { name: 'New request' })).toHaveAttribute('aria-haspopup', 'dialog')
        const tabs = screen.getByRole('tablist', { name: 'Open requests' })
        expect(within(tabs).getByRole('tab', { name: 'GET list' })).toHaveAttribute('aria-selected', 'true')
        expect(within(tabs).getByRole('button', { name: 'Close tab list' })).toBeInTheDocument()
        expect(within(line()).getByRole('combobox', { name: 'Method' })).toHaveValue('GET')
        expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics')
        expect(within(line()).getByRole('button', { name: 'Send' })).toBeInTheDocument()
        expect(within(line()).getByRole('button', { name: 'Save' })).toBeInTheDocument()
        expect(within(line()).getByRole('status')).toHaveTextContent('Saved')
        const sub = screen.getByRole('tablist', { name: 'Request' })
        expect(
            within(sub)
                .getAllByRole('tab')
                .map((t) => t.textContent)
        ).toEqual(['Params', 'Headers 1', 'Body', 'Auth', 'Variables', 'Preview'])
        expect(within(sub).getByRole('tab', { name: 'Params' })).toHaveAttribute('aria-selected', 'true')
        const pane = screen.getByRole('region', { name: 'Response pane' })
        expect(within(pane).getByText('No response yet')).toBeInTheDocument()
        expect(within(pane).getByText('Send to see the response here.')).toBeInTheDocument()
        expect(
            within(pane)
                .getAllByRole('tab')
                .map((t) => t.textContent)
        ).toEqual(['Body', 'Headers', 'Details', 'History'])
        expect(document_().querySelector('[data-slot="api-footer"]')).toHaveTextContent('Ready2 requests2 earlier versions kept privately')
    })

    it('searches the tree flat, says when nothing matches and clears; a collapsed folder hides its requests', async () => {
        const { tree, user } = await open()
        await user.click(within(tree).getByRole('button', { name: 'Collapse folder topics' }))
        expect(
            within(tree).queryByRole('button', {
                name: 'Open GET topics/list.request.toml',
            })
        ).toBeNull()
        expect(within(tree).getByRole('button', { name: 'Expand folder topics' })).toBeInTheDocument()
        await user.type(within(tree).getByRole('searchbox', { name: 'Find request' }), 'health')
        expect(
            within(tree).getByRole('button', {
                name: 'Open GET health.request.toml',
            })
        ).toBeInTheDocument()
        expect(within(tree).queryByRole('button', { name: /folder topics/ })).toBeNull()
        await user.clear(within(tree).getByRole('searchbox', { name: 'Find request' }))
        await user.type(within(tree).getByRole('searchbox', { name: 'Find request' }), 'zzz')
        expect(within(tree).getByText('No requests match “zzz”. 2 requests are in this collection.')).toBeInTheDocument()
        await user.click(within(tree).getByRole('button', { name: 'Clear search' }))
        expect(within(tree).getByRole('button', { name: 'Expand folder topics' })).toBeInTheDocument()
    })

    it('edits Params as rows that are written back as the file’s name=value lines, protected after the typing pause, and shown as text on request', async () => {
        const { user, requests } = await open()
        await user.click(screen.getByRole('button', { name: '+ Add parameter' }))
        const table = screen.getByRole('table', { name: 'Params' })
        await user.type(within(table).getByRole('textbox', { name: 'Name 1' }), 'page')
        await user.type(within(table).getByRole('textbox', { name: 'Value 1' }), '1')
        await new Promise((r) => setTimeout(r, PROTECT_DELAY_MS + 100))
        const protects = requests.filter((r) => r.kind === 'protect')
        expect(protects.at(-1)).toMatchObject({
            kind: 'protect',
            id: 'list',
            fields: { query: 'page=1' },
        })
        expect(screen.getByRole('tab', { name: 'Params 1' })).toBeInTheDocument()
        // Disabled rows keep their line behind "# ".
        await user.click(within(table).getByRole('checkbox', { name: 'Row page enabled' }))
        await new Promise((r) => setTimeout(r, PROTECT_DELAY_MS + 100))
        expect(requests.filter((r) => r.kind === 'protect').at(-1)).toMatchObject({
            fields: { query: '# page=1' },
        })
        await user.click(screen.getByRole('button', { name: 'Text' }))
        expect(screen.getByRole('textbox', { name: /^Params/ })).toHaveValue('# page=1')
        expect(document_().querySelector('[data-slot="workbench-save-status"]')).toHaveTextContent('Unsaved changes')
        // Save takes the fields the editor holds now.
        await user.click(screen.getByRole('button', { name: 'Save' }))
        await waitFor(() => expect(document_().querySelector('[data-slot="workbench-save-status"]')).toHaveTextContent('Saved'))
        expect(requests.filter((r) => r.kind === 'save').at(-1)).toMatchObject({
            kind: 'save',
            fields: { query: '# page=1' },
        })
        // Editing again replaces the save's word with the state (the native run saw "Saved" stay after a new row).
        await user.type(screen.getByRole('textbox', { name: /^Params/ }), '{Enter}limit=5')
        expect(document_().querySelector('[data-slot="workbench-save-status"]')).toHaveTextContent('Unsaved changes')
        expect(document_().querySelector('[data-slot="workbench-save-status"]')).toHaveAttribute('data-state', 'unsaved')
        // The Headers rows read the file's "Name: value" lines.
        await user.click(screen.getByRole('tab', { name: 'Headers 1' }))
        expect(within(screen.getByRole('table', { name: 'Headers' })).getByRole('textbox', { name: 'Name 1' })).toHaveValue('Accept')
    })

    it('keeps a value as typed, word by word, in Params and Headers, and leaves a padded query value verbatim when another row is edited: Save and Send carry the exact lines (attempt-02 R1)', async () => {
        const { user, requests, line } = await open(board(), (request) => (request.kind === 'openRequest' ? document({ query: 'q=  padded  ' }) : null))
        const status = () => document_().querySelector('[data-slot="workbench-save-status"]') as HTMLElement
        const params = screen.getByRole('table', { name: 'Params' })
        // The padded value is shown as written, not trimmed.
        expect(within(params).getByRole('textbox', { name: 'Value 1' })).toHaveValue('  padded  ')
        await user.click(screen.getByRole('button', { name: '+ Add parameter' }))
        await user.type(within(params).getByRole('textbox', { name: 'Name 2' }), 'note')
        // Character by character, the space included: the controlled table must keep it for the next word.
        await user.type(within(params).getByRole('textbox', { name: 'Value 2' }), 'hello world')
        expect(within(params).getByRole('textbox', { name: 'Value 2' })).toHaveValue('hello world')
        expect(within(params).getByRole('textbox', { name: 'Value 1' })).toHaveValue('  padded  ')
        await user.click(screen.getByRole('tab', { name: 'Headers 1' }))
        const headers = screen.getByRole('table', { name: 'Headers' })
        await user.click(screen.getByRole('button', { name: '+ Add header' }))
        await user.type(within(headers).getByRole('textbox', { name: 'Name 2' }), 'X-Note')
        await user.type(within(headers).getByRole('textbox', { name: 'Value 2' }), 'hello world')
        expect(within(headers).getByRole('textbox', { name: 'Value 2' })).toHaveValue('hello world')
        expect(within(headers).getByRole('textbox', { name: 'Value 1' })).toHaveValue('application/json')
        expect(status()).toHaveTextContent('Unsaved changes')
        await user.click(within(line()).getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'send')).toHaveLength(1))
        expect(requests.filter((r) => r.kind === 'send').at(-1)).toMatchObject({
            fields: { query: 'q=  padded  \nnote=hello world', headers: 'Accept: application/json\nX-Note: hello world' },
        })
        await user.click(within(line()).getByRole('button', { name: 'Save' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'save')).toHaveLength(1))
        expect(requests.filter((r) => r.kind === 'save').at(-1)).toMatchObject({
            fields: { query: 'q=  padded  \nnote=hello world', headers: 'Accept: application/json\nX-Note: hello world' },
        })
        // Text mode shows the same lines.
        await user.click(screen.getByRole('button', { name: 'Text' }))
        expect(screen.getByRole('textbox', { name: /^Headers/ })).toHaveValue('Accept: application/json\nX-Note: hello world')
    })

    it('shows the response under Body (pretty, then Raw), Headers, Details and History, where an older run is shown as recorded (I5)', async () => {
        const { user, requests, line } = await open()
        await user.click(within(line()).getByRole('button', { name: 'Send' }))
        const status = document_().querySelector('[data-slot="workbench-save-status"]') as HTMLElement
        await waitFor(() => expect(status).toHaveTextContent('list · 200 · 42 ms'))
        // An edit after the send: the line says the state, not the last send (the native run saw the send's word stay).
        await user.type(within(line()).getByRole('textbox', { name: 'URL' }), '?x=1')
        expect(status).toHaveTextContent('Unsaved changes')
        expect(status).toHaveAttribute('data-state', 'unsaved')
        await user.clear(within(line()).getByRole('textbox', { name: 'URL' }))
        await user.type(within(line()).getByRole('textbox', { name: 'URL' }), 'http://127.0.0.1:8080/topics')
        const pane = screen.getByRole('region', { name: 'Response pane' })
        expect(within(pane).getByText('200 OK')).toBeInTheDocument()
        expect(within(pane).getByText('42 ms')).toBeInTheDocument()
        expect(within(pane).getByText('29 B')).toBeInTheDocument()
        const response = within(pane).getByRole('region', { name: 'Response' })
        expect(within(response).getByLabelText('Response body')).toHaveTextContent('"custom_topics": [ "Finance" ]')
        await user.click(within(response).getByRole('button', { name: 'Raw' }))
        expect(within(response).getByLabelText('Response body')).toHaveTextContent('{"custom_topics":["Finance"]}')
        await user.click(within(pane).getByRole('tab', { name: 'Headers 2' }))
        const headers = within(pane).getByRole('table', {
            name: 'Response headers',
        })
        expect(within(headers).getAllByRole('row')).toHaveLength(2)
        expect(headers).toHaveTextContent('content-typeapplication/json')
        await user.click(within(pane).getByRole('tab', { name: 'Details' }))
        expect(document_().querySelector('[data-slot="response-details"]')).toHaveTextContent('GET http://127.0.0.1:8080/topics')
        expect(document_().querySelector('[data-slot="response-details"]')).toHaveTextContent('HTTP 200 · 29 B · 42 ms')
        expect(document_().querySelector('[data-slot="response-details"]')).toHaveTextContent('none (anonymous request)')
        await user.click(within(pane).getByRole('tab', { name: 'History' }))
        const history = within(pane).getByRole('region', { name: 'History' })
        const runs = within(history).getAllByRole('button', {
            name: /^GET topics\/list · HTTP 200/,
        })
        expect(runs).toHaveLength(2)
        await user.click(runs[1]!)
        await waitFor(() => expect(within(pane).getByRole('region', { name: 'Response' })).toBeInTheDocument())
        expect(requests.filter((r) => r.kind === 'showRun').at(-1)).toMatchObject({
            kind: 'showRun',
            run: older.record.id,
        })
        expect(within(pane).getByText('17 ms')).toBeInTheDocument()
        await user.click(within(pane).getByRole('button', { name: 'Raw' }))
        expect(within(pane).getByLabelText('Response body')).toHaveTextContent('{"custom_topics":[]}')
        // Showing history sent nothing.
        expect(requests.filter((r) => r.kind === 'send')).toHaveLength(1)
    })

    it('keeps a transfer in flight, with Cancel request, in view while an older run from History is being inspected; Cancel dispatches the active run once, history stays as recorded, nothing is resent (attempt-03 R1)', async () => {
        const { user, requests, native, line } = await open()
        // Hold the send's result: the transfer stays in flight for the rest of the test.
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const sent = requests.find((r, i) => r.kind === 'send' && `op-${i + 1}` === operation)
            return sent ? new Promise(() => {}) : observe(operation)
        }
        const pane = screen.getByRole('region', { name: 'Response pane' })
        await user.click(within(pane).getByRole('tab', { name: 'History' }))
        const runs = await within(pane).findAllByRole('button', { name: /^GET topics\/list · HTTP 200/ })
        await user.click(runs[1]!)
        await waitFor(() => expect(within(pane).getByRole('region', { name: 'Response' })).toBeInTheDocument())
        expect(within(pane).getByText('17 ms')).toBeInTheDocument()
        await user.click(within(line()).getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'send')).toHaveLength(1))
        const base = board()
        await native.push({ ...base, workbench: { ...base.workbench, running: [{ run: 'run-00000000000000bb', request: 'list', method: 'GET', bytes: 2048 }] } })
        // The old run stays shown as recorded; the transfer's progress and Cancel are in view above it.
        const running = await waitFor(() => {
            const el = pane.querySelector('[data-slot="workbench-running"]') as HTMLElement | null
            expect(el).not.toBeNull()
            return el as HTMLElement
        })
        expect(running).toHaveAttribute('role', 'status')
        expect(running).toHaveTextContent('Sending GET')
        expect(within(pane).getByText('17 ms')).toBeInTheDocument()
        expect(within(line()).getByRole('button', { name: 'Send' })).toBeDisabled()
        await user.click(within(running).getByRole('button', { name: 'Cancel request' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'cancel')).toEqual([{ kind: 'cancel', run: 'run-00000000000000bb' }]))
        expect(requests.filter((r) => r.kind === 'send')).toHaveLength(1)
        expect(within(pane).getByText('17 ms')).toBeInTheDocument()
    })

    it('shows every transfer in flight for the request with its own Cancel: two deferred sends across a tab round trip, each cancelled by its own id exactly once, nothing resent (attempt-04 R1)', async () => {
        const health = {
            kind: 'document' as const,
            document: { ...document().document, id: 'health', relative: 'health.request.toml', fields: { ...fields, url: 'http://127.0.0.1:8080/health' }, saved: { ...fields, url: 'http://127.0.0.1:8080/health' } },
            notes: [],
        }
        const { user, requests, native, line } = await open(board(), (request) => (request.kind === 'openRequest' && request.relative === 'health.request.toml' ? health : null))
        // Every send's result is held: the transfers stay in flight.
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const sent = requests.find((r, i) => r.kind === 'send' && `op-${i + 1}` === operation)
            return sent ? new Promise(() => {}) : observe(operation)
        }
        const base = board()
        const collection = base.workbench.collection!
        const healthTab = { id: 'health', relative: 'health.request.toml', method: 'GET', draft: false, new: false, recovered: false, conflict: null, interrupted: false }
        const snap = (selected: string, running: { run: string; request: string; method: string; bytes: number }[]) => ({
            ...base,
            workbench: { ...base.workbench, running, collection: { ...collection, tabs: [...collection.tabs, healthTab], selected } },
        })
        await user.click(within(line()).getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'send')).toHaveLength(1))
        const first = { run: 'run-000000000000aaaa', request: 'list', method: 'GET', bytes: 512 }
        // The round trip: the core selects the health tab, then the list tab again (the editor mounts afresh).
        await native.push(snap('health', [first]))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/health'))
        await user.click(within(screen.getByRole('tablist', { name: 'Open requests' })).getByRole('tab', { name: 'GET list' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'selectTab')).toHaveLength(1))
        await native.push(snap('list', [first]))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics'))
        await user.click(within(line()).getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'send')).toHaveLength(2))
        const second = { run: 'run-000000000000bbbb', request: 'list', method: 'GET', bytes: 0 }
        await native.push(snap('list', [first, second]))
        const pane = screen.getByRole('region', { name: 'Response pane' })
        await waitFor(() => expect(pane.querySelectorAll('[data-slot="workbench-running"]')).toHaveLength(2))
        const cancels = within(pane).getAllByRole('button', { name: /^Cancel request/ })
        expect(cancels.map((b) => b.getAttribute('aria-label'))).toEqual(['Cancel request 1 of 2', 'Cancel request 2 of 2'])
        // The second transfer is cancelled by its own id; the first stays untouched.
        await user.click(cancels[1]!)
        await waitFor(() => expect(requests.filter((r) => r.kind === 'cancel')).toEqual([{ kind: 'cancel', run: second.run }]))
        await native.push(snap('list', [first]))
        await waitFor(() => expect(pane.querySelectorAll('[data-slot="workbench-running"]')).toHaveLength(1))
        await user.click(within(pane).getByRole('button', { name: 'Cancel request' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'cancel').map((r) => (r.kind === 'cancel' ? r.run : ''))).toEqual([second.run, first.run]))
        expect(requests.filter((r) => r.kind === 'send')).toHaveLength(2)
    })

    it('an obsolete document result never replaces the current selection: two selections in flight resolve out of order and the later selection stays usable (attempt-05 R1)', async () => {
        const docFor = (id: string, relative: string, url: string) => ({ kind: 'document' as const, document: { ...document().document, id, relative, fields: { ...fields, url }, saved: { ...fields, url } }, notes: [] })
        const { requests, native, line } = await open()
        // Deferred results for the two later loads: each is resolved by hand.
        const pending = new Map<string, (r: WorkbenchResult) => void>()
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'openRequest' && req.relative !== 'topics/list.request.toml') {
                return new Promise<{ kind: 'workbench'; result: WorkbenchResult }>((resolve) => pending.set(req.relative, (result) => resolve({ kind: 'workbench', result })))
            }
            return observe(operation)
        }
        const base = board()
        const collection = base.workbench.collection!
        const tab = (id: string, relative: string) => ({ id, relative, method: 'GET', draft: false, new: false, recovered: false, conflict: null, interrupted: false })
        const snap = (selected: string) => ({ ...base, workbench: { ...base.workbench, collection: { ...collection, tabs: [...collection.tabs, tab('b', 'b.request.toml'), tab('c', 'c.request.toml')], selected } } })
        await native.push(snap('b'))
        await waitFor(() => expect(pending.has('b.request.toml')).toBe(true))
        await native.push(snap('c'))
        await waitFor(() => expect(pending.has('c.request.toml')).toBe(true))
        pending.get('c.request.toml')!(docFor('c', 'c.request.toml', 'http://127.0.0.1:8080/c'))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/c'))
        // B's late result: C stays selected and its editor, Send and response pane stay.
        pending.get('b.request.toml')!(docFor('b', 'b.request.toml', 'http://127.0.0.1:8080/b'))
        await new Promise((r) => setTimeout(r, 50))
        expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/c')
        expect(within(line()).getByRole('button', { name: 'Send' })).toBeInTheDocument()
        expect(screen.getByRole('region', { name: 'Response pane' })).toBeInTheDocument()
        expect(within(screen.getByRole('tablist', { name: 'Open requests' })).getByRole('tab', { name: 'GET c' })).toHaveAttribute('aria-selected', 'true')
    })

    it('a selected request whose load fails says why and offers Try again, which reopens it once; a lost delivery is said too (attempt-05 R2)', async () => {
        let mode: 'refused' | 'lost' | 'ok' = 'refused'
        const { user, requests, native, line } = await open(board(), (request) => (request.kind === 'openRequest' && request.relative === 'health.request.toml' && mode === 'refused' ? { kind: 'refused', problem: { kind: 'unprotected' } } : null))
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'openRequest' && req.relative === 'health.request.toml' && mode === 'lost') return Promise.reject(new Error('observation lost'))
            if (req?.kind === 'openRequest' && req.relative === 'health.request.toml' && mode === 'ok') {
                return Promise.resolve({ kind: 'workbench' as const, result: { ...document(), document: { ...document().document, id: 'health', relative: 'health.request.toml', fields: { ...fields, url: 'http://127.0.0.1:8080/health' }, saved: { ...fields, url: 'http://127.0.0.1:8080/health' } } } })
            }
            return observe(operation)
        }
        const base = board()
        const collection = base.workbench.collection!
        const healthTab = { id: 'health', relative: 'health.request.toml', method: 'GET', draft: false, new: false, recovered: false, conflict: null, interrupted: false }
        await native.push({ ...base, workbench: { ...base.workbench, collection: { ...collection, tabs: [...collection.tabs, healthTab], selected: 'health' } } })
        await waitFor(() => expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toHaveTextContent('health could not be opened:'))
        expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toHaveAttribute('role', 'alert')
        expect(screen.queryByRole('textbox', { name: 'URL' })).toBeNull()
        const opens = () => requests.filter((r) => r.kind === 'openRequest' && r.relative === 'health.request.toml').length
        expect(opens()).toBe(1)
        // No automatic retry: the count stays until Try again.
        await new Promise((r) => setTimeout(r, 100))
        expect(opens()).toBe(1)
        mode = 'lost'
        await user.click(screen.getByRole('button', { name: 'Try again' }))
        await waitFor(() => expect(opens()).toBe(2))
        await waitFor(() => expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toHaveTextContent('health could not be opened: The desktop did not report how the request ended. Check the request before trying again.'))
        mode = 'ok'
        await user.click(screen.getByRole('button', { name: 'Try again' }))
        await waitFor(() => expect(opens()).toBe(3))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/health'))
        expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toBeNull()
    })

    it('a tool\'s failure is said inside its sheet, with the entered work and focus kept (attempt-05 R3)', async () => {
        const { user } = await open(board(), (request) => (request.kind === 'importCurl' ? { kind: 'refused', problem: { kind: 'unprotected' } } : null))
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Import…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Import' })
        const curl = within(sheet).getByRole('textbox', { name: 'cURL command' })
        await user.type(curl, 'curl http://127.0.0.1:8080/x')
        await user.click(within(sheet).getByRole('button', { name: 'Create draft' }))
        const alert = await within(sheet).findByRole('alert')
        expect(alert).toHaveTextContent(/./)
        expect(screen.getByRole('dialog', { name: 'Import' })).toBeInTheDocument()
        expect(curl).toHaveValue('curl http://127.0.0.1:8080/x')
        expect(sheet.contains(document_().activeElement)).toBe(true)
    })

    it('a tree click\'s late reply never replaces the current selection: the selection moves on while the reply is held, and its release changes nothing (attempt-06 R1)', async () => {
        const { user, requests, native, tree, line } = await open()
        const pending = new Map<string, (r: WorkbenchResult) => void>()
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'openRequest' && req.relative === 'health.request.toml') {
                return new Promise<{ kind: 'workbench'; result: WorkbenchResult }>((resolve) => pending.set(req.relative, (result) => resolve({ kind: 'workbench', result })))
            }
            return observe(operation)
        }
        const base = board()
        const collection = base.workbench.collection!
        const healthTab = { id: 'health', relative: 'health.request.toml', method: 'GET', draft: false, new: false, recovered: false, conflict: null, interrupted: false }
        const snap = (selected: string) => ({ ...base, workbench: { ...base.workbench, collection: { ...collection, tabs: [...collection.tabs, healthTab], selected } } })
        // The tree opens health; its reply is held while the core selects health, then list again.
        await user.click(within(tree).getByRole('button', { name: 'Open GET health.request.toml' }))
        await waitFor(() => expect(pending.has('health.request.toml')).toBe(true))
        await native.push(snap('health'))
        await native.push(snap('list'))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics'))
        // One load of health only (the selection following it saw the load in flight), and its late reply is ignored.
        expect(requests.filter((r) => r.kind === 'openRequest' && r.relative === 'health.request.toml')).toHaveLength(1)
        pending.get('health.request.toml')!({ ...document(), document: { ...document().document, id: 'health', relative: 'health.request.toml', fields: { ...fields, url: 'http://127.0.0.1:8080/health' }, saved: { ...fields, url: 'http://127.0.0.1:8080/health' } } })
        await new Promise((r) => setTimeout(r, 50))
        expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics')
        expect(within(line()).getByRole('button', { name: 'Send' })).toBeInTheDocument()
        expect(within(screen.getByRole('tablist', { name: 'Open requests' })).getByRole('tab', { name: 'GET list' })).toHaveAttribute('aria-selected', 'true')
    })

    it('closing and re-opening a collection while its selected request\'s first load is held loads it afresh, and the old reply is ignored (attempt-06 R2)', async () => {
        const snapshot = board()
        const native = scriptedNative(snapshot)
        const requests = workbench(native, answers())
        const pending: ((r: WorkbenchResult) => void)[] = []
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'openRequest' && pending.length === 0) {
                return new Promise<{ kind: 'workbench'; result: WorkbenchResult }>((resolve) => pending.push((result) => resolve({ kind: 'workbench', result })))
            }
            return observe(operation)
        }
        render(<App client={new DesktopClient(native.transport)} />)
        await screen.findByRole('heading', { level: 1, name: 'API Client' })
        await waitFor(() => expect(pending).toHaveLength(1))
        expect(screen.queryByRole('textbox', { name: 'URL' })).toBeNull()
        // The collection closes and the same one re-opens with list still selected: one fresh load.
        await native.push({ ...snapshot, workbench: { ...snapshot.workbench, collection: null } })
        expect(await screen.findByText('No collection is open.')).toBeInTheDocument()
        await native.push(board())
        await waitFor(() => expect(requests.filter((r) => r.kind === 'openRequest')).toHaveLength(2))
        await waitFor(() => expect(screen.getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics'))
        // The first lifetime's reply settles late: nothing changes, no failure is shown.
        pending[0]!({ ...document(), document: { ...document().document, fields: { ...fields, url: 'http://127.0.0.1:8080/stale' }, saved: { ...fields, url: 'http://127.0.0.1:8080/stale' } } })
        await new Promise((r) => setTimeout(r, 50))
        expect(screen.getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics')
        expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toBeNull()
    })

    it('a refused or undelivered creation is said inside the New request dialog, for a request and for a folder, with the path and focus kept (attempt-06 R3)', async () => {
        const { user, tree } = await open(board(), (request) =>
            request.kind === 'newRequest' ? { kind: 'refused', problem: { kind: 'project', detail: 'exists' } } : request.kind === 'newFolder' ? { kind: 'refused', problem: { kind: 'project', detail: 'badPath' } } : null
        )
        await user.click(within(tree).getByRole('button', { name: 'New request' }))
        const dialog = await screen.findByRole('dialog', { name: 'New request' })
        const path = within(dialog).getByRole('textbox', { name: 'Path (folder/name)' })
        await user.type(path, 'topics/list')
        await user.click(within(dialog).getByRole('button', { name: 'Create' }))
        const alert = await within(dialog).findByRole('alert')
        expect(alert).toHaveTextContent('Something already exists there; nothing was replaced.')
        expect(screen.getByRole('dialog', { name: 'New request' })).toBeInTheDocument()
        expect(path).toHaveValue('topics/list')
        expect(dialog.contains(document_().activeElement)).toBe(true)
        // A folder: its own refusal replaces the text.
        await user.click(within(dialog).getByRole('radio', { name: 'New folder' }))
        await user.click(within(dialog).getByRole('button', { name: 'Create' }))
        await waitFor(() => expect(within(screen.getByRole('dialog', { name: 'New folder' })).getByRole('alert')).toHaveTextContent('Use a path inside the collection: folder/name, no leading dot, slash or "..".'))
        expect(within(screen.getByRole('dialog', { name: 'New folder' })).getByRole('textbox', { name: 'Folder path' })).toHaveValue('topics/list')
    })

    it('a cURL import\'s late reply never replaces the current selection: held past the draft\'s selection, the sheet closing and another selection, it is ignored (attempt-07 R1)', async () => {
        const docFor = (id: string, relative: string, url: string) => ({ kind: 'document' as const, document: { ...document().document, id, relative, new: id === 'imported', fields: { ...fields, url }, saved: { ...fields, url } }, notes: [] })
        const { user, requests, native, line } = await open(board(), (request) =>
            request.kind === 'openRequest' && request.relative === 'imported/request.request.toml' ? docFor('imported', 'imported/request.request.toml', 'http://127.0.0.1:8080/imported') : request.kind === 'openRequest' && request.relative === 'health.request.toml' ? docFor('health', 'health.request.toml', 'http://127.0.0.1:8080/health') : null
        )
        let release: ((r: WorkbenchResult) => void) | null = null
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'importCurl') return new Promise<{ kind: 'workbench'; result: WorkbenchResult }>((resolve) => (release = (result) => resolve({ kind: 'workbench', result })))
            return observe(operation)
        }
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Import…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Import' })
        await user.type(within(sheet).getByRole('textbox', { name: 'cURL command' }), 'curl http://127.0.0.1:8080/imported')
        await user.click(within(sheet).getByRole('button', { name: 'Create draft' }))
        await waitFor(() => expect(release).not.toBeNull())
        // The core selects the draft (its own tab), the sheet is closed, and health is selected and loaded.
        const base = board()
        const collection = base.workbench.collection!
        const tab = (id: string, relative: string, isNew = false) => ({ id, relative, method: 'GET', draft: isNew, new: isNew, recovered: false, conflict: null, interrupted: false })
        const snap = (selected: string) => ({ ...base, workbench: { ...base.workbench, collection: { ...collection, tabs: [...collection.tabs, tab('imported', 'imported/request.request.toml', true), tab('health', 'health.request.toml')], selected } } })
        await native.push(snap('imported'))
        // The page behind the open sheet is aria-hidden: the editor is queried as hidden until the sheet closes.
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL', hidden: true })).toHaveValue('http://127.0.0.1:8080/imported'))
        await user.click(within(sheet).getByRole('button', { name: 'Close Import' }))
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Import' })).toBeNull())
        await native.push(snap('health'))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/health'))
        // The import's reply, released now, belongs to a superseded load: health stays with its editor.
        release!(docFor('imported', 'imported/request.request.toml', 'http://127.0.0.1:8080/imported'))
        await new Promise((r) => setTimeout(r, 50))
        expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/health')
        expect(within(line()).getByRole('button', { name: 'Send' })).toBeInTheDocument()
        expect(screen.getByRole('region', { name: 'Response pane' })).toBeInTheDocument()
    })

    it('a cURL import\'s reply held across a collection close and re-open is ignored; the re-opened selection loads afresh (attempt-07 R1)', async () => {
        const { user, requests, native } = await open()
        let release: ((r: WorkbenchResult) => void) | null = null
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'importCurl') return new Promise<{ kind: 'workbench'; result: WorkbenchResult }>((resolve) => (release = (result) => resolve({ kind: 'workbench', result })))
            return observe(operation)
        }
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Import…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Import' })
        await user.type(within(sheet).getByRole('textbox', { name: 'cURL command' }), 'curl http://127.0.0.1:8080/x')
        await user.click(within(sheet).getByRole('button', { name: 'Create draft' }))
        await waitFor(() => expect(release).not.toBeNull())
        await user.click(within(sheet).getByRole('button', { name: 'Close Import' }))
        const closed = board()
        await native.push({ ...closed, workbench: { ...closed.workbench, collection: null } })
        expect(await screen.findByText('No collection is open.')).toBeInTheDocument()
        await native.push(board())
        await waitFor(() => expect(screen.getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics'))
        release!({ ...document(), document: { ...document().document, id: 'imported', relative: 'imported/request.request.toml', new: true, fields: { ...fields, url: 'http://127.0.0.1:8080/x' }, saved: { ...fields, url: 'http://127.0.0.1:8080/x' } } })
        await new Promise((r) => setTimeout(r, 50))
        expect(screen.getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics')
        expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toBeNull()
    })

    it('an unselected tree entry whose load is refused, then lost, says so above the current editor, which stays; Try again reopens it once each time (attempt-07 R2)', async () => {
        let mode: 'refused' | 'lost' = 'refused'
        const { user, requests, tree, line, native } = await open(board(), (request) => (request.kind === 'openRequest' && request.relative === 'health.request.toml' && mode === 'refused' ? { kind: 'refused', problem: { kind: 'project', detail: 'missing' } } : null))
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'openRequest' && req.relative === 'health.request.toml' && mode === 'lost') return Promise.reject(new Error('observation lost'))
            return observe(operation)
        }
        const opens = () => requests.filter((r) => r.kind === 'openRequest' && r.relative === 'health.request.toml').length
        await user.click(within(tree).getByRole('button', { name: 'Open GET health.request.toml' }))
        await waitFor(() => expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toHaveTextContent('health could not be opened: That file is missing.'))
        expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toHaveAttribute('role', 'alert')
        // The previous editor stays: the core left the selection unchanged.
        expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics')
        expect(opens()).toBe(1)
        await new Promise((r) => setTimeout(r, 100))
        expect(opens()).toBe(1)
        mode = 'lost'
        await user.click(screen.getByRole('button', { name: 'Try again' }))
        await waitFor(() => expect(opens()).toBe(2))
        await waitFor(() => expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toHaveTextContent('health could not be opened: The desktop did not report how the request ended.'))
        expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics')
    })

    it('a cURL import\'s conversion report shows with its draft whether the result or the selecting snapshot arrives first; later selections stay protected (attempt-08 R2)', async () => {
        const NOTES = [{ kind: 'headerReplaced' as const, variable: 'token', reference: 'imported.token' }]
        const imported = (notes: typeof NOTES) => ({ kind: 'document' as const, document: { ...document().document, id: 'imported', relative: 'imported/request.request.toml', new: true, fields: { ...fields, url: 'http://127.0.0.1:8080/imported' }, saved: { ...fields, url: 'http://127.0.0.1:8080/imported' } }, notes })
        const { user, requests, native, line } = await open(board(), (request) => (request.kind === 'openRequest' && request.relative === 'imported/request.request.toml' ? imported([]) : null))
        let release: ((r: WorkbenchResult) => void) | null = null
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'importCurl') return new Promise<{ kind: 'workbench'; result: WorkbenchResult }>((resolve) => (release = (result) => resolve({ kind: 'workbench', result })))
            return observe(operation)
        }
        const base = board()
        const collection = base.workbench.collection!
        const draftTab = { id: 'imported', relative: 'imported/request.request.toml', method: 'GET', draft: true, new: true, recovered: false, conflict: null, interrupted: false }
        const snap = (selected: string) => ({ ...base, workbench: { ...base.workbench, collection: { ...collection, tabs: [...collection.tabs, draftTab], selected } } })
        // Snapshot first: the core selects the draft (its own load, empty notes) before the import's result arrives.
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Import…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Import' })
        await user.type(within(sheet).getByRole('textbox', { name: 'cURL command' }), 'curl -H "Authorization: Bearer x" http://127.0.0.1:8080/imported')
        await user.click(within(sheet).getByRole('button', { name: 'Create draft' }))
        await waitFor(() => expect(release).not.toBeNull())
        await native.push(snap('imported'))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL', hidden: true })).toHaveValue('http://127.0.0.1:8080/imported'))
        await act(async () => release!(imported(NOTES)))
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Import' })).toBeNull())
        await waitFor(() => expect(document_().querySelector('[data-slot="workbench-notes"]')).toHaveTextContent('A credential header now uses {{token}}: store its value in Secrets as imported.token.'))
        // A later selection is still protected: list is selected and shown without the report.
        await native.push(snap('list'))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics'))
        expect(document_().querySelector('[data-slot="workbench-notes"]')).toBeNull()
    })

    it('a cURL import\'s conversion report shows when the result arrives before the selecting snapshot (attempt-08 R2)', async () => {
        const NOTES = [{ kind: 'authRemoved' as const, reference: 'imported.auth' }]
        const imported = { kind: 'document' as const, document: { ...document().document, id: 'imported', relative: 'imported/request.request.toml', new: true, fields: { ...fields, url: 'http://127.0.0.1:8080/imported' }, saved: { ...fields, url: 'http://127.0.0.1:8080/imported' } }, notes: NOTES }
        const { user, native, line } = await open(board(), (request) => (request.kind === 'importCurl' ? imported : null))
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Import…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Import' })
        await user.type(within(sheet).getByRole('textbox', { name: 'cURL command' }), 'curl -u a:b http://127.0.0.1:8080/imported')
        await user.click(within(sheet).getByRole('button', { name: 'Create draft' }))
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Import' })).toBeNull())
        const base = board()
        const collection = base.workbench.collection!
        const draftTab = { id: 'imported', relative: 'imported/request.request.toml', method: 'GET', draft: true, new: true, recovered: false, conflict: null, interrupted: false }
        await native.push({ ...base, workbench: { ...base.workbench, collection: { ...collection, tabs: [...collection.tabs, draftTab], selected: 'imported' } } })
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/imported'))
        expect(document_().querySelector('[data-slot="workbench-notes"]')).toHaveTextContent('The authentication credential was removed: store its value in Secrets as imported.auth.')
    })

    it('typing while an environment or schema save is pending is kept: the reply brings the revision, the next Save sends the newer text (attempt-08 R1)', async () => {
        // Revisions are digests to the client's parser.
        let envRevision = '1'.repeat(64)
        let schemaRevision = 'a'.repeat(64)
        const { user, requests, native } = await open(board(), (request) =>
            request.kind === 'openEnvironment'
                ? { kind: 'environment', name: request.name, private: request.private, text: 'v = 1', revision: envRevision }
                : request.kind === 'saveEnvironment'
                  ? { kind: 'environment', name: request.name, private: request.private, text: request.text, revision: (envRevision = '2'.repeat(64)) }
                  : request.kind === 'openSchema'
                    ? { kind: 'schema', schema: { relative: request.relative, text: '{"version":1}', revision: schemaRevision } }
                    : request.kind === 'saveSchema'
                      ? { kind: 'schema', schema: { relative: request.relative, text: request.text, revision: (schemaRevision = 'b'.repeat(64)) } }
                      : null
        )
        const held: ((r: WorkbenchResult) => void)[] = []
        const observe = native.observeWorkbench
        native.observeWorkbench = async (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'saveEnvironment' || req?.kind === 'saveSchema') {
                const r = await observe(operation)
                return new Promise((resolve) => held.push(() => resolve(r)))
            }
            return observe(operation)
        }
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Environments…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Environments' })
        await user.click(within(sheet).getByRole('button', { name: 'Edit private local' }))
        const editor = within(await within(sheet).findByRole('form', { name: 'Private environment local' })).getByRole('textbox')
        await user.clear(editor)
        await user.type(editor, 'v = 2')
        await user.click(within(sheet).getByRole('button', { name: 'Save environment' }))
        await waitFor(() => expect(held).toHaveLength(1))
        await user.type(editor, '3')
        expect(editor).toHaveValue('v = 23')
        await act(async () => held[0]!({ kind: 'done' }))
        await new Promise((r) => setTimeout(r, 20))
        expect(editor).toHaveValue('v = 23')
        await user.click(within(sheet).getByRole('button', { name: 'Save environment' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'saveEnvironment')).toHaveLength(2))
        expect(requests.filter((r) => r.kind === 'saveEnvironment').at(-1)).toMatchObject({ kind: 'saveEnvironment', name: 'local', private: true, text: 'v = 23', revision: '2'.repeat(64) })
        await act(async () => held[1]!({ kind: 'done' }))
        await user.click(within(sheet).getByRole('button', { name: 'Close Environments' }))
        // The schema editor likewise.
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Schemas…' }))
        const schemas = await screen.findByRole('dialog', { name: 'Schemas' })
        await user.type(within(schemas).getByRole('textbox', { name: 'New schema path (.json, .yaml or .yml)' }), 'topics.json')
        await user.click(within(schemas).getByRole('button', { name: 'New schema' }))
        const schemaEditor = within(await within(schemas).findByRole('form', { name: 'Schema topics.json' })).getByRole('textbox')
        await user.type(schemaEditor, '{{"version":2}')
        await user.click(within(schemas).getByRole('button', { name: 'Save schema' }))
        await waitFor(() => expect(held).toHaveLength(3))
        await user.type(schemaEditor, '3')
        expect(schemaEditor).toHaveValue('{"version":2}3')
        await act(async () => held[2]!({ kind: 'done' }))
        await new Promise((r) => setTimeout(r, 20))
        expect(schemaEditor).toHaveValue('{"version":2}3')
        await user.click(within(schemas).getByRole('button', { name: 'Save schema' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'saveSchema')).toHaveLength(2))
        expect(requests.filter((r) => r.kind === 'saveSchema').at(-1)).toMatchObject({ kind: 'saveSchema', relative: 'topics.json', text: '{"version":2}3', revision: 'b'.repeat(64) })
    })

    it('returning to an edited request after its editor unmounted reloads it from the core: the protected fields come back, never the old snapshot (attempt-09 R1)', async () => {
        let opensOfList = 0
        const { user, requests, native, line } = await open(board(), (request) => {
            if (request.kind === 'openRequest' && request.relative === 'topics/list.request.toml') {
                opensOfList += 1
                // The core returns the protected draft once there is one.
                return opensOfList >= 2 ? document({ url: 'http://127.0.0.1:8080/edited' }) : null
            }
            if (request.kind === 'openRequest' && request.relative === 'health.request.toml') return { kind: 'refused', problem: { kind: 'project', detail: 'missing' } }
            return null
        })
        const held: ((r: WorkbenchResult) => void)[] = []
        let holdHealth = true
        const observe = native.observeWorkbench
        native.observeWorkbench = (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            if (req?.kind === 'openRequest' && req.relative === 'health.request.toml' && holdHealth) return new Promise<{ kind: 'workbench'; result: WorkbenchResult }>((resolve) => held.push((result) => resolve({ kind: 'workbench', result })))
            return observe(operation)
        }
        const base = board()
        const collection = base.workbench.collection!
        const healthTab = { id: 'health', relative: 'health.request.toml', method: 'GET', draft: false, new: false, recovered: false, conflict: null, interrupted: false }
        const snap = (selected: string) => ({ ...base, workbench: { ...base.workbench, collection: { ...collection, tabs: [...collection.tabs, healthTab], selected } } })
        // A is edited and the edit protected.
        await user.clear(within(line()).getByRole('textbox', { name: 'URL' }))
        await user.type(within(line()).getByRole('textbox', { name: 'URL' }), 'http://127.0.0.1:8080/edited')
        await waitFor(() => expect(requests.filter((r) => r.kind === 'protect').at(-1)).toMatchObject({ fields: { url: 'http://127.0.0.1:8080/edited' } }), { timeout: 2000 })
        // B's load stays pending; back to A: A is loaded again from the core, with the protected fields.
        await native.push(snap('health'))
        await waitFor(() => expect(held).toHaveLength(1))
        await native.push(snap('list'))
        await waitFor(() => expect(opensOfList).toBe(2))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/edited'))
        // B's load refused; back to A again: loaded once more, the fields kept.
        holdHealth = false
        await native.push(snap('health'))
        await waitFor(() => expect(document_().querySelector('[data-slot="workbench-load-failed"]')).toHaveTextContent('health could not be opened'))
        await native.push(snap('list'))
        await waitFor(() => expect(opensOfList).toBe(3))
        await waitFor(() => expect(within(line()).getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/edited'))
        // Save sends the current fields; nothing was sent by itself.
        await user.click(within(line()).getByRole('button', { name: 'Save' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'save').at(-1)).toMatchObject({ fields: { url: 'http://127.0.0.1:8080/edited' } }))
        expect(requests.filter((r) => r.kind === 'send')).toHaveLength(0)
    })

    it('a superseded environment or schema open cannot replace the current editor or its typing: a newer selection, a new resource or Close retires it (attempt-09 R2)', async () => {
        const base = board()
        const collection = base.workbench.collection!
        const { user, requests, native } = await open({ ...base, workbench: { ...base.workbench, collection: { ...collection, schemas: ['existing.json'] } } }, (request) =>
            request.kind === 'openEnvironment'
                ? { kind: 'environment', name: request.name, private: request.private, text: `${request.name} = 1`, revision: '1'.repeat(64) }
                : request.kind === 'openSchema'
                  ? { kind: 'schema', schema: { relative: request.relative, text: '{"existing":1}', revision: 'a'.repeat(64) } }
                  : null
        )
        const held: ((r: WorkbenchResult) => void)[] = []
        const holdFor = new Set<string>()
        const observe = native.observeWorkbench
        native.observeWorkbench = async (operation) => {
            const req = requests.find((r, i) => `op-${i + 1}` === operation)
            const key = req?.kind === 'openEnvironment' ? `env:${req.name}` : req?.kind === 'openSchema' ? `schema:${req.relative}` : ''
            if (key && holdFor.has(key)) {
                const r = await observe(operation)
                return new Promise((resolve) => held.push(() => resolve(r)))
            }
            return observe(operation)
        }
        // Schemas: an open held, a new schema started and typed into, the old reply released.
        holdFor.add('schema:existing.json')
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Schemas…' }))
        const schemas = await screen.findByRole('dialog', { name: 'Schemas' })
        await user.click(within(schemas).getByRole('button', { name: 'Edit existing.json' }))
        await waitFor(() => expect(held).toHaveLength(1))
        await user.type(within(schemas).getByRole('textbox', { name: 'New schema path (.json, .yaml or .yml)' }), 'draft.json')
        await user.click(within(schemas).getByRole('button', { name: 'New schema' }))
        const draft = within(within(schemas).getByRole('form', { name: 'Schema draft.json' })).getByRole('textbox')
        await user.type(draft, '{{"newWork":true}')
        await act(async () => held[0]!({ kind: 'done' }))
        await new Promise((r) => setTimeout(r, 20))
        expect(within(schemas).getByRole('form', { name: 'Schema draft.json' })).toBeInTheDocument()
        expect(draft).toHaveValue('{"newWork":true}')
        expect(within(schemas).queryByRole('form', { name: 'Schema existing.json' })).toBeNull()
        // Close retires a pending open too.
        holdFor.add('schema:existing.json')
        await user.click(within(schemas).getByRole('button', { name: 'Edit existing.json' }))
        await waitFor(() => expect(held).toHaveLength(2))
        await user.click(within(within(schemas).getByRole('form', { name: 'Schema draft.json' })).getByRole('button', { name: 'Close' }))
        await act(async () => held[1]!({ kind: 'done' }))
        await new Promise((r) => setTimeout(r, 20))
        expect(within(schemas).queryByRole('form', { name: /^Schema / })).toBeNull()
        await user.click(within(schemas).getByRole('button', { name: 'Close Schemas' }))
        // Environments: an open held, a newer environment opened, the old reply released.
        holdFor.add('env:local')
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Environments…' }))
        const envs = await screen.findByRole('dialog', { name: 'Environments' })
        await user.click(within(envs).getByRole('button', { name: 'Edit private local' }))
        await waitFor(() => expect(held).toHaveLength(3))
        await user.type(within(envs).getByRole('textbox', { name: 'Environment name' }), 'staging')
        await user.click(within(envs).getByRole('button', { name: 'New private environment' }))
        const staging = within(await within(envs).findByRole('form', { name: 'Private environment staging' })).getByRole('textbox')
        await user.type(staging, ' # typed')
        await act(async () => held[2]!({ kind: 'done' }))
        await new Promise((r) => setTimeout(r, 20))
        expect(within(envs).getByRole('form', { name: 'Private environment staging' })).toBeInTheDocument()
        expect(staging).toHaveValue('staging = 1 # typed')
        expect(within(envs).queryByRole('form', { name: 'Private environment local' })).toBeNull()
    })

    it('reads the preview for Variables and Preview, and the Collection chip’s menu opens the tools sheet, refreshes and closes the collection', async () => {
        const { user, requests } = await open()
        await user.click(screen.getByRole('tab', { name: 'Preview' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'preview')).toHaveLength(1))
        expect(await screen.findByRole('region', { name: 'Preview' })).toHaveTextContent('GET http://127.0.0.1:8080/topics?page=1')
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        const menu = screen.getByRole('menu', { name: 'Collection actions' })
        expect(
            within(menu)
                .getAllByRole('menuitem')
                .map((m) => m.textContent)
        ).toEqual(['Refresh', 'Environments…', 'Secrets…', 'Schemas…', 'Import…', 'Git…', 'Close collection'])
        await user.click(within(menu).getByRole('menuitem', { name: 'Environments…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Environments' })
        expect(within(sheet).getByText('local')).toBeInTheDocument()
        expect(within(sheet).getByRole('button', { name: 'Edit private local' })).toBeInTheDocument()
        await user.click(within(sheet).getByRole('button', { name: 'Close Environments' }))
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Environments' })).toBeNull())
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Refresh' }))
        expect(requests.filter((r) => r.kind === 'refresh')).toHaveLength(1)
        await user.selectOptions(screen.getByRole('combobox', { name: 'Environment' }), '')
        expect(requests.filter((r) => r.kind === 'selectEnvironment').at(-1)).toMatchObject({ kind: 'selectEnvironment', name: null })
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Close collection' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'closeCollection')).toHaveLength(1))
        expect(requests.filter((r) => r.kind === 'closeCollection')[0]).toMatchObject({ discard: false })
    })

    it('creates a request draft from + and shows it new; a tab with edits the core could not protect closes only after the operator chooses to discard them', async () => {
        const { user, requests, tree, native } = await open(board(), (request) =>
            request.kind === 'newRequest' || (request.kind === 'openRequest' && request.relative === 'topics/create.request.toml')
                ? {
                      kind: 'document',
                      document: {
                          ...document().document,
                          id: 'create',
                          relative: 'topics/create.request.toml',
                          new: true,
                          fields: { ...fields, url: '' },
                          saved: { ...fields, url: '' },
                      },
                      notes: [],
                  }
                : request.kind === 'closeTab' && !request.discard
                  ? { kind: 'refused', problem: { kind: 'unprotected' } }
                  : request.kind === 'closeTab'
                    ? { kind: 'closed', id: request.id }
                    : null
        )
        await user.click(within(tree).getByRole('button', { name: 'New request' }))
        const dialog = await screen.findByRole('dialog', { name: 'New request' })
        await user.type(within(dialog).getByRole('textbox', { name: 'Path (folder/name)' }), 'topics/create')
        await user.click(within(dialog).getByRole('button', { name: 'Create' }))
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'New request' })).toBeNull())
        expect(requests.filter((r) => r.kind === 'newRequest').at(-1)).toMatchObject({ kind: 'newRequest', relative: 'topics/create' })
        const next = board()
        const collection = next.workbench.collection!
        const draftTab = { id: 'create', relative: 'topics/create.request.toml', method: 'GET', draft: true, new: true, recovered: false, conflict: null, interrupted: false }
        const withDraft = (selected: string) => ({ ...next, workbench: { ...next.workbench, collection: { ...collection, tabs: [...collection.tabs, draftTab], selected } } })
        // A snapshot that lists the draft but still names the list request as selected arrives first: the draft
        // stays shown and the list request is not re-opened (which made the core select it again in a native run).
        const opens = requests.filter((r) => r.kind === 'openRequest').length
        await native.push(withDraft(collection.selected!))
        // The list request's editor (the form with the URL field) is not re-opened over the draft.
        expect(screen.queryAllByRole('form', { name: 'Request list' }).filter((f) => within(f).queryByRole('textbox', { name: 'URL' }))).toHaveLength(0)
        expect(requests.filter((r) => r.kind === 'openRequest')).toHaveLength(opens)
        // The core's next snapshot lists the draft as the selected tab; the editor shows it new.
        await native.push(withDraft('create'))
        expect(await screen.findByRole('form', { name: 'Request create' })).toBeInTheDocument()
        expect(within(screen.getByRole('tablist', { name: 'Open requests' })).getByRole('tab', { name: 'GET create (new)' })).toHaveAttribute('aria-selected', 'true')
        expect(document_().querySelector('[data-slot="workbench-save-status"]')).toHaveTextContent('New · not saved yet')
        // Closing the selected tab: the core still sees unprotected edits; the operator decides.
        await user.click(screen.getByRole('button', { name: 'Close tab list' }))
        const closing = await screen.findByRole('alertdialog', {
            name: 'Close list without your latest edits?',
        })
        await user.click(within(closing).getByRole('button', { name: 'Keep editing' }))
        await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull())
        expect(requests.filter((r) => r.kind === 'closeTab')).toHaveLength(1)
        await user.click(screen.getByRole('button', { name: 'Close tab list' }))
        await user.click(
            within(await screen.findByRole('alertdialog')).getByRole('button', {
                name: 'Close and discard edits',
            })
        )
        // Three closeTab requests: the two refused ones (edits kept) and the discard the operator chose.
        await waitFor(() => expect(requests.filter((r) => r.kind === 'closeTab')).toHaveLength(3))
        expect(requests.filter((r) => r.kind === 'closeTab').map((r) => (r.kind === 'closeTab' ? r.discard : null))).toEqual([false, false, true])
        expect(requests.filter((r) => r.kind === 'closeTab')[2]).toMatchObject({ kind: 'closeTab', id: 'list', discard: true })
    })

    it('re-opens the selected request when the same collection is closed and opened again: following its selection is scoped to the open collection', async () => {
        const { user, requests, native } = await open()
        const opens = () => requests.filter((r) => r.kind === 'openRequest').length
        expect(opens()).toBe(1)
        await user.click(screen.getByRole('button', { name: 'Collection AIRS' }))
        await user.click(screen.getByRole('menuitem', { name: 'Close collection' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'closeCollection')).toHaveLength(1))
        const closed = board()
        await native.push({ ...closed, workbench: { ...closed.workbench, collection: null } })
        expect(await screen.findByText('No collection is open.')).toBeInTheDocument()
        expect(screen.queryByRole('textbox', { name: 'URL' })).toBeNull()
        // The same collection, with the same persisted selection: its document is opened and shown again.
        await native.push(board())
        await waitFor(() => expect(screen.getByRole('textbox', { name: 'URL' })).toHaveValue('http://127.0.0.1:8080/topics'))
        expect(within(screen.getByRole('tablist', { name: 'Open requests' })).getByRole('tab', { name: 'GET list' })).toHaveAttribute('aria-selected', 'true')
        expect(opens()).toBe(2)
    })

    it('draws nested folders as a hierarchy: a collapsed folder hides its whole subtree, a folder counts every request under it, and search still finds them', async () => {
        const snapshot = board()
        const collection = snapshot.workbench.collection!
        const { tree, user } = await open({
            ...snapshot,
            workbench: {
                ...snapshot.workbench,
                collection: {
                    ...collection,
                    folders: [...collection.folders, 'products', 'products/runtime'],
                    requests: [
                        ...collection.requests,
                        { relative: 'products/list.request.toml', id: 'plist', method: 'GET', url: 'http://127.0.0.1:8080/products' },
                        { relative: 'products/runtime/scan.request.toml', id: 'scan', method: 'POST', url: 'http://127.0.0.1:8080/scan' },
                    ],
                },
            },
        })
        expect(within(tree).getByRole('button', { name: 'Collapse folder products' })).toHaveTextContent('products2')
        expect(within(tree).getByRole('button', { name: 'Collapse folder products/runtime' })).toHaveTextContent('runtime1')
        expect(within(tree).getByRole('button', { name: 'Open POST products/runtime/scan.request.toml' })).toBeInTheDocument()
        await user.click(within(tree).getByRole('button', { name: 'Collapse folder products' }))
        expect(within(tree).queryByRole('button', { name: /folder products\/runtime/ })).toBeNull()
        expect(within(tree).queryByRole('button', { name: 'Open POST products/runtime/scan.request.toml' })).toBeNull()
        expect(within(tree).queryByRole('button', { name: 'Open GET products/list.request.toml' })).toBeNull()
        expect(within(tree).getByRole('button', { name: 'Expand folder products' })).toHaveTextContent('products2')
        // The other folder and the root request are untouched.
        expect(within(tree).getByRole('button', { name: 'Open GET topics/list.request.toml' })).toBeInTheDocument()
        expect(within(tree).getByRole('button', { name: 'Open GET health.request.toml' })).toBeInTheDocument()
        await user.type(within(tree).getByRole('searchbox', { name: 'Find request' }), 'scan')
        expect(within(tree).getByRole('button', { name: 'Open POST products/runtime/scan.request.toml' })).toBeInTheDocument()
        await user.clear(within(tree).getByRole('searchbox', { name: 'Find request' }))
        await user.click(within(tree).getByRole('button', { name: 'Expand folder products' }))
        expect(within(tree).getByRole('button', { name: 'Collapse folder products/runtime' })).toBeInTheDocument()
        expect(within(tree).getByRole('button', { name: 'Open POST products/runtime/scan.request.toml' })).toBeInTheDocument()
        // Collapsing the inner folder alone hides only its request.
        await user.click(within(tree).getByRole('button', { name: 'Collapse folder products/runtime' }))
        expect(within(tree).queryByRole('button', { name: 'Open POST products/runtime/scan.request.toml' })).toBeNull()
        expect(within(tree).getByRole('button', { name: 'Open GET products/list.request.toml' })).toBeInTheDocument()
    })

    it('opens a collection tool from the keyboard; closing the sheet by Escape or its close control returns focus to the Collection chip, as the menu does on Escape (I8, T09)', async () => {
        const { user } = await open()
        const chip = screen.getByRole('button', { name: 'Collection AIRS' })
        const openEnvironments = async () => {
            chip.focus()
            await user.keyboard('{Enter}')
            const menu = screen.getByRole('menu', { name: 'Collection actions' })
            await user.tab()
            expect(within(menu).getByRole('menuitem', { name: 'Refresh' })).toHaveFocus()
            await user.tab()
            expect(within(menu).getByRole('menuitem', { name: 'Environments…' })).toHaveFocus()
            await user.keyboard('{Enter}')
            const sheet = await screen.findByRole('dialog', { name: 'Environments' })
            expect(screen.queryByRole('menu')).toBeNull()
            expect(sheet.contains(document_().activeElement)).toBe(true)
            return sheet
        }
        await openEnvironments()
        await user.keyboard('{Escape}')
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Environments' })).toBeNull())
        await waitFor(() => expect(chip).toHaveFocus())
        const sheet = await openEnvironments()
        await user.click(within(sheet).getByRole('button', { name: 'Close Environments' }))
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Environments' })).toBeNull())
        await waitFor(() => expect(chip).toHaveFocus())
        // The menu alone: Escape closes it and focus is back on the chip.
        await user.keyboard('{Enter}')
        await user.tab()
        expect(screen.getByRole('menuitem', { name: 'Refresh' })).toHaveFocus()
        await user.keyboard('{Escape}')
        expect(screen.queryByRole('menu')).toBeNull()
        expect(chip).toHaveFocus()
    })

    it('a send from an already dirty request announces its completion once; the next edit says the state again', async () => {
        const { user, line } = await open()
        const status = () => document_().querySelector('[data-slot="workbench-save-status"]') as HTMLElement
        await user.type(within(line()).getByRole('textbox', { name: 'URL' }), '?x=1')
        expect(status()).toHaveTextContent('Unsaved changes')
        await user.click(within(line()).getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(status()).toHaveTextContent('list · 200 · 42 ms'))
        // Dirty before and after: the completion still gives way to the state on the next edit.
        await user.type(within(line()).getByRole('textbox', { name: 'URL' }), '&y=2')
        expect(status()).toHaveTextContent('Unsaved changes')
        expect(status()).toHaveAttribute('data-state', 'unsaved')
    })

    it('a send from a new request announces its completion once; the next edit says "New · not saved yet" again', async () => {
        const next = board()
        const collection = next.workbench.collection!
        const draftTab = { id: 'create', relative: 'topics/create.request.toml', method: 'GET', draft: true, new: true, recovered: false, conflict: null, interrupted: false }
        const draft = {
            kind: 'document' as const,
            document: {
                ...document().document,
                id: 'create',
                relative: 'topics/create.request.toml',
                new: true,
                fields: { ...fields, url: 'http://127.0.0.1:8080/created' },
                saved: { ...fields, url: 'http://127.0.0.1:8080/created' },
            },
            notes: [],
        }
        const { user, line } = await open({ ...next, workbench: { ...next.workbench, collection: { ...collection, tabs: [draftTab], selected: 'create' } } }, (request) =>
            request.kind === 'openRequest' ? draft : request.kind === 'send' ? { kind: 'ran', run: { ...ran, record: { ...ran.record, request: 'create', relative: 'topics/create.request.toml' } } } : null
        )
        const status = () => document_().querySelector('[data-slot="workbench-save-status"]') as HTMLElement
        expect(status()).toHaveTextContent('New · not saved yet')
        await user.click(within(line()).getByRole('button', { name: 'Send' }))
        await waitFor(() => expect(status()).toHaveTextContent('create · 200 · 42 ms'))
        await user.type(within(line()).getByRole('textbox', { name: 'URL' }), '?x=1')
        expect(status()).toHaveTextContent('New · not saved yet')
        expect(status()).toHaveAttribute('data-state', 'new')
    })

    it('shows the start panel with the recents and the v1 import when no collection is open, and the built-in collection opens from it', async () => {
        const snapshot = fxApi()
        const { user, requests } = await (async () => {
            const native = scriptedNative({
                ...snapshot,
                workbench: {
                    ...snapshot.workbench,
                    collection: null,
                    recent: [{ index: 0, name: 'Topics', folder: 'api' }],
                    adoptionSettled: false,
                },
            })
            const requests = workbench(native, answers())
            const user = userEvent.setup()
            render(<App client={new DesktopClient(native.transport)} />)
            await screen.findByRole('heading', { level: 1, name: 'API Client' })
            return { user, requests }
        })()
        const start = await screen.findByText('No collection is open.')
        expect(start).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Topics (api)' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Import v1 API Client profile…' })).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Open Strata Cloud Manager' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'openBuiltIn')).toHaveLength(1))
        await user.click(screen.getByRole('button', { name: 'Topics (api)' }))
        await waitFor(() => expect(requests.filter((r) => r.kind === 'openRecent')).toHaveLength(1))
    })
})
