/**
 * API Client Send through the real App, DesktopClient and shell (C15):
 * Send carries the fields the editor holds at the click (typed before the
 * protection debounce, never saved) with the active tenant context; a
 * known tenant change shows its review, and Apply sends the fresh snapshot
 * with the review ID (the presentation cannot mark anything approved);
 * refusals and uncertain outcomes are explained; a running request can be
 * cancelled; history shows runs and saving a response goes through the
 * native sheet.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import {
    type CollectionView,
    DesktopClient,
    type DocumentView,
    type Fields,
    type Intent,
    type RunRecord,
    type RunView,
    type SendReviewView,
    type ShellSnapshot,
    type WorkbenchRequest,
    type WorkbenchResult,
} from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant, framed } from '../../test/native'

const R1 = '1'.repeat(32)
const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'

const fields = (overrides: Partial<Fields> = {}): Fields => ({
    method: 'GET',
    url: 'http://127.0.0.1:8080/items',
    headers: '',
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

const collection = (overrides: Partial<CollectionView> = {}): CollectionView => ({
    id: 'c15',
    name: 'Exec',
    folder: 'api',
    scope: '1'.repeat(32),
    builtIn: false,
    complete: true,
    requests: [{ relative: 'items/list.request.toml', id: 'list', method: 'GET', url: 'http://127.0.0.1:8080/items' }],
    folders: ['items'],
    schemas: [],
    issues: [],
    environments: [],
    environment: null,
    tabs: [{ id: 'list', relative: 'items/list.request.toml', method: 'GET', draft: false, new: false, recovered: false, conflict: null, interrupted: false }],
    selected: 'list',
    secrets: [],
    unsaved: null,
    archives: 0,
    ...overrides,
})

const opened = (workbench: Partial<ShellSnapshot['workbench']> = {}): ShellSnapshot =>
    withTenant({ ...baseSnapshot, workbench: { ...baseSnapshot.workbench, loaded: true, collection: collection(), ...workbench } }, tenant(), CONTEXT)

const record = (overrides: Partial<RunRecord> = {}): RunRecord => ({
    id: 'run-0123456789abcdef',
    request: 'list',
    relative: 'items/list.request.toml',
    at: 1_760_000_000,
    method: 'GET',
    url: 'http://127.0.0.1:8080/items',
    class: 'anonymous',
    operation: null,
    state: 'completed',
    failure: null,
    uncertain: false,
    status: 200,
    headers: [['content-type', 'application/json']],
    redirect: null,
    elapsedMs: 42,
    bytes: 11,
    stored: true,
    ...overrides,
})

const ran = (overrides: Partial<RunRecord> = {}, preview: string | null = '{"ok":true}'): RunView => ({
    record: record(overrides),
    preview,
    previewBytes: preview?.length ?? 0,
    tokenDropped: false,
})

const review = (): SendReviewView => ({
    review: 'rv-00000000000000aa',
    request: 'list',
    operation: 'IAM/Delete access policy',
    method: 'DELETE',
    url: 'https://127.0.0.1:9443/iam/v1/access_policies/abc',
    headers: [],
    body: '',
    bodyBytes: 0,
    files: [],
    destination: 'https://127.0.0.1:9443',
    expires: 1_760_000_900,
})

type Answer = (request: WorkbenchRequest) => WorkbenchResult | Promise<WorkbenchResult>

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

const standard =
    (sent: Answer): Answer =>
    (request) => {
        switch (request.kind) {
            case 'openRequest':
                return { kind: 'document', document: doc(), notes: [] }
            case 'protect':
                return { kind: 'protected', id: request.id }
            case 'send':
            case 'apply':
            case 'cancel':
            case 'history':
            case 'showRun':
            case 'saveResponse':
                return sent(request)
            default:
                return { kind: 'done' }
        }
    }

const of = <K extends WorkbenchRequest['kind']>(requests: WorkbenchRequest[], kind: K) =>
    requests.filter((r): r is Extract<WorkbenchRequest, { kind: K }> => r.kind === kind)

async function open(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await native.push({ ...native.current, onboarding: 'done', activeFeature: 'apiClient' })
    await screen.findByRole('form', { name: 'Request list' })
    return user
}

const url = () => screen.getByRole('textbox', { name: 'URL' }) as HTMLInputElement

describe('API Client Send', () => {
    it('sends the fields typed before the protection debounce, with the active context', async () => {
        const native = framed(scriptedNative(opened()), 'apiClient')
        const requests = workbench(native, standard(() => ({ kind: 'ran', run: ran() })))
        const user = await open(native)
        await user.clear(url())
        await user.type(url(), 'http://127.0.0.1:8080/fresh')
        await user.click(screen.getByRole('button', { name: 'Send' }))
        const response = await screen.findByRole('region', { name: 'Response' })
        expect(within(response).getByText('HTTP 200 · 11 B · 42 ms')).toBeInTheDocument()
        // C44: the v2 pane pretty-prints JSON first; Raw shows exactly what arrived.
        await user.click(within(response).getByRole('button', { name: 'Raw' }))
        expect(within(response).getByLabelText('Response body')).toHaveTextContent('{"ok":true}')
        expect(of(requests, 'send')).toEqual([
            { kind: 'send', id: 'list', revision: R1, fields: fields({ url: 'http://127.0.0.1:8080/fresh' }), context: CONTEXT },
        ])
        // Nothing was saved to send it.
        expect(of(requests, 'save')).toHaveLength(0)
    })

    it('shows a tenant change for review and applies the fresh snapshot with its review ID', async () => {
        const native = framed(scriptedNative(opened()), 'apiClient')
        const requests = workbench(
            native,
            standard((request) =>
                request.kind === 'send'
                    ? { kind: 'reviewRequired', review: review() }
                    : { kind: 'ran', run: ran({ method: 'DELETE', class: 'managed', operation: 'IAM/Delete access policy' }) }
            )
        )
        const user = await open(native)
        await user.click(screen.getByRole('button', { name: 'Send' }))
        expect(await screen.findByText('This change to the tenant needs your review below.')).toBeInTheDocument()
        // The review arrives with the native snapshot.
        await native.push(opened({ review: review() }))
        const panel = (await screen.findByText('Review this change to the tenant: IAM/Delete access policy')).closest('[data-slot="workbench-review"]') as HTMLElement
        expect(panel).toHaveAttribute('role', 'alert')
        expect(within(panel).getByText('DELETE https://127.0.0.1:9443/iam/v1/access_policies/abc')).toBeInTheDocument()
        // Typed after the review: Apply sends what the editor holds now, so
        // Rust can see it differs from what was reviewed.
        await user.type(url(), '/x')
        await user.click(within(panel).getByRole('button', { name: 'Send to tenant…' }))
        await waitFor(() => expect(of(requests, 'apply')).toHaveLength(1))
        expect(of(requests, 'apply')[0]).toEqual({
            kind: 'apply',
            review: 'rv-00000000000000aa',
            id: 'list',
            revision: R1,
            fields: fields({ url: 'http://127.0.0.1:8080/items/x' }),
            context: CONTEXT,
        })
        expect(await screen.findByRole('region', { name: 'Response' })).toBeInTheDocument()
    })

    it('explains refusals and uncertain outcomes, and never offers to resend', async () => {
        let attempt = 0
        const native = framed(scriptedNative(opened()), 'apiClient')
        workbench(
            native,
            standard(() =>
                ++attempt === 1
                    ? { kind: 'sendRefused', refusal: { kind: 'unknownEndpoint' } }
                    : { kind: 'ran', run: ran({ method: 'POST', state: 'failed', failure: 'request', uncertain: true, status: null, bytes: 0, stored: false }, null) }
            )
        )
        const user = await open(native)
        await user.click(screen.getByRole('button', { name: 'Send' }))
        expect(await screen.findByText(/known Strata Cloud Manager and AIRS operations/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Send' }))
        const response = await screen.findByRole('region', { name: 'Response' })
        expect(within(response).getByText(/may have reached the server/)).toBeInTheDocument()
        expect(within(response).queryByRole('button', { name: /retry|resend/i })).toBeNull()
    })

    it('cancels a running request', async () => {
        const native = framed(scriptedNative(opened({ running: [{ run: 'run-00000000000000bb', request: 'list', method: 'POST', bytes: 2048 }] })), 'apiClient')
        const requests = workbench(native, standard(() => ({ kind: 'done' })))
        const user = await open(native)
        const running = await screen.findByText('Sending POST… 2.0 KB transferred')
        await user.click(within(running).getByRole('button', { name: 'Cancel request' }))
        await waitFor(() => expect(of(requests, 'cancel')).toEqual([{ kind: 'cancel', run: 'run-00000000000000bb' }]))
    })

    it('lists history, shows a run and saves its response through the native sheet', async () => {
        const native = framed(scriptedNative(opened()), 'apiClient')
        const requests = workbench(
            native,
            standard((request) => {
                if (request.kind === 'history') return { kind: 'history', runs: [record()], more: false }
                if (request.kind === 'showRun') return { kind: 'ran', run: ran() }
                return { kind: 'responseSaved', bytes: 11, name: 'list-response.json' }
            })
        )
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'History' }))
        await user.click(await screen.findByRole('button', { name: /^GET items\/list · HTTP 200/ }))
        const response = await screen.findByRole('region', { name: 'Response' })
        await user.click(within(response).getByRole('button', { name: 'Save response…' }))
        // C44: the pane's status line (outside the response region) announces the save.
        expect(await screen.findByText('Saved 11 B as list-response.json.')).toBeInTheDocument()
        expect(of(requests, 'showRun')).toEqual([{ kind: 'showRun', run: 'run-0123456789abcdef' }])
        expect(of(requests, 'saveResponse')).toEqual([{ kind: 'saveResponse', run: 'run-0123456789abcdef' }])
        // Showing history sent nothing.
        expect(of(requests, 'send')).toHaveLength(0)
    })
})
