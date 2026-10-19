/**
 * API Client document Save through the real App, DesktopClient and shell
 * (C14): Save sends the fields the editor holds at the click (not a
 * debounced copy), invalid input is refused without touching the editor,
 * typing during a save stays and is protected against the new revision, a
 * stale revision reloads, conflicts offer Use disk / Keep mine with the
 * fresh snapshot, typing pauses and leaving a document protect drafts, and
 * a recovered draft can be discarded.
 */
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import {
    type CollectionView,
    DesktopClient,
    type DocumentView,
    type Fields,
    type Intent,
    type ShellSnapshot,
    type TabView,
    type WorkbenchRequest,
    type WorkbenchResult,
} from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative } from '../../test/native'
import { PROTECT_DELAY_MS, SAVE_DOCUMENT_EVENT } from './document-editor'

const R1 = '1'.repeat(32)
const R2 = '2'.repeat(32)

const fields = (overrides: Partial<Fields> = {}): Fields => ({
    method: 'GET',
    url: 'https://a.example/items',
    headers: 'Accept: application/json',
    query: '',
    body: '',
    bodyKind: 'text',
    settings: '',
    ...overrides,
})

const doc = (overrides: Partial<DocumentView> = {}): DocumentView => ({
    id: 'list',
    relative: 'items/list.request.toml',
    revision: R1,
    fields: fields(),
    saved: fields(),
    new: false,
    recovered: false,
    bodyEditable: true,
    schema: null,
    conflict: null,
    interrupted: false,
    metadataDirty: false,
    ...overrides,
})

const tab = (overrides: Partial<TabView> = {}): TabView => ({
    id: 'list',
    relative: 'items/list.request.toml',
    method: 'GET',
    draft: false,
    new: false,
    recovered: false,
    conflict: null,
    interrupted: false,
    ...overrides,
})

const collection = (overrides: Partial<CollectionView> = {}): CollectionView => ({
    id: 'c14',
    name: 'Items API',
    folder: 'api',
    builtIn: false,
    complete: true,
    requests: [
        { relative: 'items/list.request.toml', id: 'list', method: 'GET', url: 'https://a.example/items' },
        { relative: 'items/create.request.toml', id: 'create', method: 'POST', url: 'https://a.example/items' },
    ],
    folders: ['items'],
    schemas: [],
    issues: [],
    environments: [],
    environment: null,
    tabs: [tab()],
    selected: 'list',
    secrets: [],
    unsaved: null,
    archives: 0,
    ...overrides,
})

const opened = (c: Partial<CollectionView> = {}): ShellSnapshot => ({
    ...baseSnapshot,
    workbench: { ...baseSnapshot.workbench, collection: collection(c) },
})

type Answer = (request: WorkbenchRequest) => WorkbenchResult | Promise<WorkbenchResult>

/** Records API Client requests and answers each from `answer`. */
function workbench(native: Native, answer: Answer): WorkbenchRequest[] {
    const requests: WorkbenchRequest[] = []
    const pending = new Map<string, WorkbenchRequest>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind !== 'workbench') return respond(intent)
        requests.push(intent.request)
        const operation = `op-${requests.length}`
        pending.set(operation, intent.request)
        return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${requests.length}`, operation } }
    }
    native.observeWorkbench = async (operation) => {
        const request = pending.get(operation)
        if (!request) return { kind: 'refusal', refusal: 'invalidInput' }
        return { kind: 'workbench', result: await answer(request) }
    }
    return requests
}

/** Answers like Rust for one document at R1 (saves move it to R2). */
const standard =
    (held: DocumentView = doc()): Answer =>
    (request) => {
        switch (request.kind) {
            case 'openRequest':
                return { kind: 'document', document: held, notes: [] }
            case 'save':
                return { kind: 'saved', document: doc({ revision: R2, fields: request.fields, saved: request.fields }) }
            case 'protect':
                return { kind: 'protected', id: request.id }
            default:
                return { kind: 'done' }
        }
    }

const of = <K extends WorkbenchRequest['kind']>(requests: WorkbenchRequest[], kind: K) =>
    requests.filter((r): r is Extract<WorkbenchRequest, { kind: K }> => r.kind === kind)

async function open(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^API Client/ }))
    await screen.findByRole('heading', { level: 1, name: 'API Client' })
    await screen.findByRole('form', { name: 'Request list' })
    return user
}

const url = () => screen.getByRole('textbox', { name: 'URL' }) as HTMLInputElement

describe('API Client document Save', () => {
    it('sends the fields typed before the protection debounce fires', async () => {
        const native = scriptedNative(opened())
        const requests = workbench(native, standard())
        const user = await open(native)
        await user.clear(url())
        await user.type(url(), 'https://b.example/new')
        // Saved at once: well inside the debounce.
        await user.click(screen.getByRole('button', { name: 'Save' }))
        await waitFor(() => expect(document.querySelector('[data-slot="workbench-save-status"]')).toHaveTextContent(/^Saved$/))
        const saves = of(requests, 'save')
        expect(saves).toHaveLength(1)
        expect(saves[0]).toEqual({ kind: 'save', id: 'list', revision: R1, fields: fields({ url: 'https://b.example/new' }), edit: expect.any(Number) })
        // The pending protection was replaced by the save; none followed.
        await new Promise((r) => setTimeout(r, PROTECT_DELAY_MS + 50))
        expect(of(requests, 'protect')).toHaveLength(0)
        // ⌘S saves the current snapshot against the new revision.
        await user.type(url(), '?x=1')
        await user.keyboard('{Meta>}s{/Meta}')
        await waitFor(() => expect(of(requests, 'save')).toHaveLength(2))
        expect(of(requests, 'save')[1]).toMatchObject({ revision: R2, fields: { url: 'https://b.example/new?x=1' } })
        // File > Save from the native menu (how ⌘S arrives in the app).
        await user.type(url(), '&y=2')
        await act(async () => void window.dispatchEvent(new Event(SAVE_DOCUMENT_EVENT)))
        await waitFor(() => expect(of(requests, 'save')).toHaveLength(3))
        expect(of(requests, 'save')[2]).toMatchObject({ fields: { url: 'https://b.example/new?x=1&y=2' } })
    })

    it('refuses invalid input without changing what the editor holds', async () => {
        const native = scriptedNative(opened())
        const requests = workbench(native, (request) =>
            request.kind === 'save'
                ? { kind: 'saveRefused', refusal: { kind: 'invalid', problem: { field: 'headers', line: 2 } } }
                : standard()(request)
        )
        const user = await open(native)
        const headers = screen.getByRole('textbox', { name: 'Headers' })
        await user.type(headers, '{Enter}not a header')
        await user.click(screen.getByRole('button', { name: 'Save' }))
        expect(await screen.findByText('Fix to save. Headers: use Name: value (line 2).')).toBeInTheDocument()
        expect(headers).toHaveValue('Accept: application/json\nnot a header')
        expect(of(requests, 'save')[0]).toMatchObject({ fields: { headers: 'Accept: application/json\nnot a header' } })
        expect(screen.getByText('Unsaved changes')).toBeInTheDocument()
    })

    it('keeps text typed while a save is in flight and protects it against the new revision', async () => {
        const native = scriptedNative(opened())
        let finish: (() => void) | null = null
        const requests = workbench(native, async (request) => {
            if (request.kind === 'save') {
                await new Promise<void>((r) => (finish = r))
                return { kind: 'saved', document: doc({ revision: R2, fields: request.fields, saved: request.fields }) }
            }
            return standard()(request)
        })
        const user = await open(native)
        await user.type(url(), '/one')
        await user.click(screen.getByRole('button', { name: 'Save' }))
        await waitFor(() => expect(finish).not.toBeNull())
        await user.type(url(), '/two')
        await act(async () => finish?.())
        expect(await screen.findByText('Saved. Edits made while saving are kept unsaved.')).toBeInTheDocument()
        expect(url()).toHaveValue('https://a.example/items/one/two')
        await waitFor(() => expect(of(requests, 'protect').at(-1)).toMatchObject({ revision: R2, fields: { url: 'https://a.example/items/one/two' } }))
        expect(of(requests, 'save')[0]).toMatchObject({ revision: R1, fields: { url: 'https://a.example/items/one' } })
        expect(screen.getByText('Unsaved changes')).toBeInTheDocument()
    })

    it('reloads a document whose revision is no longer current', async () => {
        const native = scriptedNative(opened())
        let reloads = 0
        const requests = workbench(native, (request) => {
            if (request.kind === 'save') return { kind: 'refused', problem: { kind: 'stale' } }
            if (request.kind === 'openRequest' && ++reloads > 1) {
                return { kind: 'document', document: doc({ revision: R2, fields: fields({ url: 'https://disk.example/' }), saved: fields({ url: 'https://disk.example/' }) }), notes: [] }
            }
            return standard()(request)
        })
        const user = await open(native)
        await user.type(url(), '/mine')
        await user.click(screen.getByRole('button', { name: 'Save' }))
        expect(await screen.findByText(/This request changed since it was loaded/)).toBeInTheDocument()
        await waitFor(() => expect(url()).toHaveValue('https://disk.example/'))
        expect(of(requests, 'openRequest')).toHaveLength(2)
    })

    it('offers Use disk and Keep mine on a conflict and continues from the chosen version', async () => {
        const native = scriptedNative(opened())
        const requests = workbench(native, (request) => {
            if (request.kind === 'save' && request.revision === R1) return { kind: 'saveRefused', refusal: { kind: 'conflict' } }
            if (request.kind === 'resolve') {
                return {
                    kind: 'document',
                    document: doc({ revision: R2, fields: request.fields, saved: fields({ method: 'PUT', url: 'https://theirs.example/' }) }),
                    notes: [],
                }
            }
            return standard()(request)
        })
        const user = await open(native)
        await user.type(url(), '/mine')
        await user.click(screen.getByRole('button', { name: 'Save' }))
        const banner = (await screen.findByText('Changed on disk')).closest('[data-slot="workbench-conflict"]') as HTMLElement
        expect(banner).toHaveAttribute('role', 'alert')
        await user.click(within(banner).getByRole('button', { name: 'Keep mine' }))
        expect(of(requests, 'resolve')[0]).toEqual({
            kind: 'resolve',
            id: 'list',
            revision: R1,
            fields: fields({ url: 'https://a.example/items/mine' }),
            choice: 'keepMine',
        })
        expect(await screen.findByText('Your edits now apply to the files on disk. Save to write them.')).toBeInTheDocument()
        expect(url()).toHaveValue('https://a.example/items/mine')
        await user.click(screen.getByRole('button', { name: 'Save' }))
        await waitFor(() => expect(of(requests, 'save').at(-1)).toMatchObject({ revision: R2, fields: { url: 'https://a.example/items/mine' } }))
    })

    it('protects the snapshot when typing pauses, marking unprotected edits once', async () => {
        const native = scriptedNative(opened())
        const requests = workbench(native, standard())
        const user = await open(native)
        await user.type(url(), '/a')
        await user.type(url(), 'b')
        await waitFor(() => expect(of(requests, 'protect')).toHaveLength(1), { timeout: 2000 })
        expect(of(requests, 'editing')).toEqual([{ kind: 'editing', id: 'list', edit: 1 }])
        expect(of(requests, 'protect')[0]).toEqual({ kind: 'protect', id: 'list', revision: R1, fields: fields({ url: 'https://a.example/items/ab' }), edit: 3 })
    })

    it('re-arms the close guard after a save and while an older protection is outstanding', async () => {
        const native = scriptedNative(opened())
        let release: (() => void) | null = null
        const requests = workbench(native, async (request) => {
            if (request.kind === 'protect') {
                await new Promise<void>((r) => (release = r))
                return { kind: 'protected', id: request.id }
            }
            return standard()(request)
        })
        const user = await open(native)
        await user.type(url(), '/a')
        await user.click(screen.getByRole('button', { name: 'Save' }))
        await waitFor(() => expect(document.querySelector('[data-slot="workbench-save-status"]')).toHaveTextContent(/^Saved$/))
        expect(of(requests, 'save')[0]).toMatchObject({ edit: 2 })
        // Edit after Save: guarded again.
        await user.type(url(), 'b')
        expect(of(requests, 'editing').map((r) => r.edit)).toEqual([1, 3])
        // Typing while that protection is outstanding is reported again, so
        // its (older) acknowledgement cannot clear the newer edit in Rust.
        await waitFor(() => expect(release).not.toBeNull(), { timeout: 2000 })
        expect(of(requests, 'protect')[0]).toMatchObject({ edit: 3 })
        await user.type(url(), 'c')
        expect(of(requests, 'editing').map((r) => r.edit)).toEqual([1, 3, 4])
        await act(async () => release?.())
    })

    it('protects the latest typing before a tab closes', async () => {
        const native = scriptedNative(opened())
        const requests = workbench(native, (request) =>
            request.kind === 'closeTab' ? { kind: 'closed', id: request.id } : standard()(request)
        )
        const user = await open(native)
        await user.type(url(), '/x')
        // Inside the debounce.
        await user.click(screen.getByRole('button', { name: 'Close tab list' }))
        await waitFor(() => expect(of(requests, 'closeTab')).toHaveLength(1))
        const order = requests.map((r) => r.kind).filter((k) => k === 'protect' || k === 'closeTab')
        expect(order).toEqual(['protect', 'closeTab'])
        expect(of(requests, 'protect')[0]).toMatchObject({ fields: { url: 'https://a.example/items/x' }, edit: 2 })
        expect(of(requests, 'closeTab')[0]).toEqual({ kind: 'closeTab', id: 'list', discard: false })
    })

    it('asks before closing the collection when the latest typing cannot be kept', async () => {
        const native = scriptedNative(opened())
        const requests = workbench(native, (request) =>
            request.kind === 'protect' ? { kind: 'refused', problem: { kind: 'private', detail: 'unavailable' } } : standard()(request)
        )
        const user = await open(native)
        await user.type(url(), '/kept')
        await user.click(screen.getByRole('button', { name: 'Close collection' }))
        const ask = (await screen.findByText('Close the collection without your latest edits?')).closest('[data-slot="workbench-closing"]') as HTMLElement
        expect(of(requests, 'closeCollection')).toHaveLength(0)
        await user.click(within(ask).getByRole('button', { name: 'Keep editing' }))
        expect(document.querySelector('[data-slot="workbench-closing"]')).toBeNull()
        expect(url()).toHaveValue('https://a.example/items/kept')
        await user.click(screen.getByRole('button', { name: 'Close collection' }))
        const again = (await screen.findByText('Close the collection without your latest edits?')).closest('[data-slot="workbench-closing"]') as HTMLElement
        const protects = of(requests, 'protect').length
        await user.click(within(again).getByRole('button', { name: 'Close and discard edits' }))
        await waitFor(() => expect(of(requests, 'closeCollection')).toEqual([{ kind: 'closeCollection', discard: true }]))
        await new Promise((r) => setTimeout(r, PROTECT_DELAY_MS + 50))
        expect(of(requests, 'protect')).toHaveLength(protects)
    })

    it('protects pending edits when leaving the document', async () => {
        const second = doc({ id: 'create', relative: 'items/create.request.toml', fields: fields({ method: 'POST' }), saved: fields({ method: 'POST' }) })
        const native = scriptedNative(opened({ tabs: [tab(), tab({ id: 'create', relative: 'items/create.request.toml', method: 'POST' })] }))
        const requests = workbench(native, (request) =>
            request.kind === 'openRequest' && request.relative === second.relative
                ? { kind: 'document', document: second, notes: [] }
                : standard()(request)
        )
        const user = await open(native)
        await user.type(url(), '/left')
        await user.click(screen.getByRole('tab', { name: 'POST create' }))
        await native.push(opened({ tabs: [tab(), tab({ id: 'create', relative: 'items/create.request.toml', method: 'POST' })], selected: 'create' }))
        await waitFor(() => expect(of(requests, 'protect')).toHaveLength(1))
        expect(of(requests, 'protect')[0]).toMatchObject({ id: 'list', fields: { url: 'https://a.example/items/left' } })
        expect(of(requests, 'selectTab')).toEqual([{ kind: 'selectTab', id: 'create' }])
    })

    it('shows a recovered draft and discards it to the saved version', async () => {
        const recovered = doc({ recovered: true, fields: fields({ url: 'https://draft.example/' }) })
        const native = scriptedNative(opened({ tabs: [tab({ draft: true, recovered: true })] }))
        const requests = workbench(native, (request) =>
            request.kind === 'discardDraft' ? { kind: 'document', document: doc(), notes: [] } : standard(recovered)(request)
        )
        const user = await open(native)
        expect(screen.getByText('Recovered draft')).toBeInTheDocument()
        expect(url()).toHaveValue('https://draft.example/')
        await user.click(screen.getByRole('button', { name: 'Discard draft' }))
        await waitFor(() => expect(url()).toHaveValue('https://a.example/items'))
        expect(of(requests, 'discardDraft')).toEqual([{ kind: 'discardDraft', id: 'list' }])
    })
})

describe('API Client collection', () => {
    it('opens collections only through the native sheets and lists recent ones', async () => {
        const native = scriptedNative({
            ...baseSnapshot,
            workbench: { ...baseSnapshot.workbench, loaded: true, recent: [{ index: 0, name: 'Items API', folder: 'api' }] },
        })
        const requests = workbench(native, () => ({ kind: 'cancelled' }))
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^API Client/ }))
        expect(await screen.findByText('No collection is open.')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Open collection…' }))
        await user.click(screen.getByRole('button', { name: 'Items API (api)' }))
        await user.click(screen.getByRole('button', { name: 'Open Strata Cloud Manager' }))
        await waitFor(() => expect(requests).toEqual([{ kind: 'openFolder' }, { kind: 'openRecent', index: 0 }, { kind: 'openBuiltIn' }]))
        // No request carries a path chosen by the presentation.
        expect(JSON.stringify(native.sent)).not.toMatch(/\/Users\//)
    })

    it('imports a cURL command as a draft and shows the conversion report', async () => {
        const native = scriptedNative(opened({ tabs: [], selected: null }))
        const requests = workbench(native, (request) =>
            request.kind === 'importCurl'
                ? {
                      kind: 'document',
                      document: doc({ id: 'abc', relative: 'imported/request.request.toml', new: true }),
                      notes: [{ kind: 'authRemoved', reference: 'import-0123456789abcdef' }],
                  }
                : { kind: 'done' }
        )
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^API Client/ }))
        await user.click(await screen.findByRole('tab', { name: 'Import' }))
        await user.type(screen.getByRole('textbox', { name: 'cURL command' }), 'curl -u a:b https://a.example/')
        await user.click(screen.getByRole('button', { name: 'Create draft' }))
        await waitFor(() => expect(of(requests, 'importCurl')).toEqual([{ kind: 'importCurl', text: 'curl -u a:b https://a.example/', relative: 'imported/request' }]))
        expect(await screen.findByText(/store its value in Secrets as import-0123456789abcdef/)).toBeInTheDocument()
    })

    it('never shows a secret value after it is stored', async () => {
        const native = scriptedNative(opened({ secrets: [{ reference: 'dev-token', stored: false, managed: false }, { reference: 'tenant.select', stored: false, managed: true }] }))
        const requests = workbench(native, standard())
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^API Client/ }))
        await user.click(await screen.findByRole('tab', { name: 'Secrets' }))
        const field = screen.getByLabelText('dev-token (no value)')
        expect(field).toHaveAttribute('type', 'password')
        await user.type(field, 'SYNTHETIC-VALUE')
        await user.click(screen.getByRole('button', { name: 'Store value for dev-token' }))
        await waitFor(() => expect(of(requests, 'setSecret')).toEqual([{ kind: 'setSecret', reference: 'dev-token', value: 'SYNTHETIC-VALUE' }]))
        expect(field).toHaveValue('')
        expect(screen.getByText(/managed by the tenant in Settings/)).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Store value for tenant.select' })).toBeNull()
    })
})
