/**
 * C41: the API Client as R2App draws it (start=api), through the real App,
 * DesktopClient and parser over scripted workbench results. The board (the
 * chips, the tree, the tabs, the request line, the sub-tabs, the response
 * pane), the Params table writing the file's lines, the response tabs and
 * History, the collection menu and its sheet, creating a request, and
 * closing with unprotected edits. Nothing here sends by itself: every
 * request the view makes is listed and asserted.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
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
    const tree = await screen.findByRole('form', { name: 'Request list' })
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
        // The Headers rows read the file's "Name: value" lines.
        await user.click(screen.getByRole('tab', { name: 'Headers 1' }))
        expect(within(screen.getByRole('table', { name: 'Headers' })).getByRole('textbox', { name: 'Name 1' })).toHaveValue('Accept')
    })

    it('shows the response under Body (pretty, then Raw), Headers, Details and History, where an older run is shown as recorded (I5)', async () => {
        const { user, requests, line } = await open()
        await user.click(within(line()).getByRole('button', { name: 'Send' }))
        const status = document_().querySelector('[data-slot="workbench-save-status"]') as HTMLElement
        await waitFor(() => expect(status).toHaveTextContent('list · 200 · 42 ms'))
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
        // The core's next snapshot lists the draft as the selected tab; the editor shows it new.
        const next = board()
        const collection = next.workbench.collection!
        await native.push({
            ...next,
            workbench: {
                ...next.workbench,
                collection: {
                    ...collection,
                    tabs: [
                        ...collection.tabs,
                        {
                            id: 'create',
                            relative: 'topics/create.request.toml',
                            method: 'GET',
                            draft: true,
                            new: true,
                            recovered: false,
                            conflict: null,
                            interrupted: false,
                        },
                    ],
                    selected: 'create',
                },
            },
        })
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
