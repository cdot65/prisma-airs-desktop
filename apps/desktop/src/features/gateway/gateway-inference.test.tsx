/**
 * The AI Gateway Playground and observation tabs (C18) through the
 * production App and DesktopClient against a scripted native host: the
 * inference key is entered for the chosen workspace and never shown, a run
 * is one explicit press and stopping it never claims nothing was charged,
 * and usage, logs and limits keep each source's data or failure.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import {
    DesktopClient,
    type GatewayView,
    type Intent,
    type ListedWorkspace,
    type ManagementRequest,
    type ManagementResult,
    type PlaygroundView,
    type ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { createdText } from './gateway-files'
import { bars } from './gateway-observe'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const WS_A = '16f7e90d-382a-4e78-b577-1b01eb5f8297'
const REF_A = 'gw-aaaaaaaaaaaaaaaaaaaaaaaa'
const A: ListedWorkspace = { id: WS_A, name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }
const B: ListedWorkspace = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8298', name: 'Team B', slug: 'team-b', scopeName: 'scope-team-b' }
const ENDPOINT = 'https://gateway.example/v1'

const view = (overrides: Partial<GatewayView> = {}): GatewayView => ({
    workspaces: [A, B],
    complete: true,
    selected: { reference: REF_A, workspace: A, restored: false },
    remembered: null,
    rememberedMissing: false,
    local: { state: 'ready', failure: null, drafts: [], records: [], playground: [], runs: [], files: [], problem: null, unsaved: false },
    playground: null,
    files: null,
    ...overrides,
})

type Key = ShellSnapshot['tenants'][number]['inference']
const noKey: Key = { state: 'missing', mode: null, failure: null, target: null }
const keyFor = (workspace: ListedWorkspace): Key => ({ state: 'ready', mode: 'keychain', failure: null, target: { workspace, endpoint: ENDPOINT } })

const ready = (gateway: GatewayView = view(), inference: Key = noKey): ShellSnapshot => ({
    ...withTenant(
        baseSnapshot,
        tenant({
            management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1234567890.iam.panserviceaccount.com', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' },
            inference,
        }),
        CONTEXT
    ),
    gateway,
})

type Answer = (request: ManagementRequest) => ManagementResult | Promise<ManagementResult>

function scripted(native: Native, answer: Answer): ManagementRequest[] {
    const reads: ManagementRequest[] = []
    const pending = new Map<string, ManagementRequest>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind === 'runInference') {
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-run', operation: 'op-900' } }
        }
        if (intent.kind !== 'management') return respond(intent)
        reads.push(intent.request)
        const operation = `op-${reads.length}`
        pending.set(operation, intent.request)
        return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${reads.length}`, operation } }
    }
    native.observeManagement = async (operation) => {
        const request = pending.get(operation)
        if (!request) return { kind: 'refusal', refusal: 'invalidInput' }
        return { kind: 'management', result: await answer(request) }
    }
    return reads
}

const routes: ManagementResult = {
    kind: 'gateway',
    gateway: {
        kind: 'list',
        list: { resource: 'routes', workspace: WS_A, rows: JSON.stringify([{ id: 'cfg-1', name: 'default-route' }]), count: 1, total: 1, complete: true },
    },
}

const series = (total: number, errors: number | null = null) => ({
    points: [
        { x: '2026-10-20T00:00:00Z', y: total / 2 },
        { x: '2026-10-20T01:00:00Z', y: total / 2 },
    ],
    total,
    errors,
    partial: false,
})

const observed = (o: unknown): ManagementResult => ({ kind: 'gateway', gateway: { kind: 'observed', observed: o } }) as ManagementResult

const answer: Answer = (request) => {
    if (request.kind !== 'gateway') return { kind: 'failed', failure: 'invalidResponse', outcome: 'completed' }
    const read = request.read
    if (read.kind === 'inventory') return read.resource === 'routes' ? routes : { kind: 'gateway', gateway: { kind: 'list', list: { resource: read.resource, workspace: WS_A, rows: '[]', count: 0, total: 0, complete: true } } }
    if (read.kind !== 'observe') return { kind: 'failed', failure: 'notFound', outcome: 'completed' }
    const o = read.read
    if (o.kind === 'usage') {
        return observed({
            kind: 'usage',
            usage: {
                workspace: WS_A, days: o.days, requests: series(40, 4), requestsFailure: null, cost: null, costFailure: 'unavailable',
                tokens: series(900), tokensFailure: null, latency: series(350), latencyFailure: null,
                models: { rows: [{ model: 'gpt-4o', provider: 'openai', requests: 30, cost: 100, totalTokens: null, successRate: 0.98 }], total: 1, complete: true, partial: false },
                modelsFailure: null,
            },
        })
    }
    if (o.kind === 'logs') {
        return observed({
            kind: 'logs',
            logs: {
                workspace: 'team-a', days: o.days, page: o.page, trace: o.trace ?? null, status: o.status ?? null,
                rows: [{ id: 'log-1', createdAt: '2026-10-20T00:00:00Z', model: 'gpt-4o', provider: 'openai', status: 446, units: 0, cost: 0, traceId: o.trace ?? 'airs-1', cacheStatus: null, apiKeyId: null }],
                total: 1, more: false, capped: false, partial: false,
            },
        })
    }
    return observed({
        kind: 'limits',
        limits: {
            workspace: WS_A,
            usage: { policies: [{ id: 'ul-1', name: 'Monthly', status: 'active', limitType: 'cost', groupBy: ['api_key'], creditLimit: 5000, alertThreshold: 80, periodicReset: 'monthly', nextResetAt: '2026-11-01T00:00:00Z', value: null, unit: null, target: null }], total: 1, complete: true },
            usageFailure: null,
            rate: null,
            rateFailure: 'denied',
        },
    })
}

async function open(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^AI Gateway/ }))
    await screen.findByRole('heading', { level: 1, name: 'Gateway workspaces and configuration' })
    return user
}

const run = (overrides: Partial<PlaygroundView> = {}): PlaygroundView => ({
    operation: 'op-900',
    workspace: A,
    api: 'chat',
    model: '@openai-prod/gpt-4o',
    route: 'cfg-1',
    file: null,
    trace: 'airs-0123456789abcdef',
    state: 'running',
    output: 'Hel',
    outcome: null,
    failure: null,
    sent: true,
    answered: true,
    servedModel: 'gpt-4o',
    responseId: 'c1',
    finish: null,
    usage: null,
    ...overrides,
})

describe('AI Gateway Playground and observation (C18)', () => {
    it('stores the inference key for the chosen workspace and never shows it', async () => {
        const native = scriptedNative(ready())
        scripted(native, answer)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Playground' }))
        expect(await screen.findByText(/No inference key is stored for this tenant\. The management client is never used for inference\./)).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Run prompt' })).toBeDisabled()
        await user.type(screen.getByLabelText('Runtime endpoint'), ENDPOINT)
        const secret = screen.getByLabelText('Workspace API key with completions access')
        expect(secret).toHaveAttribute('type', 'password')
        await user.type(secret, 'gw-CANARY-key')
        await user.click(screen.getByRole('button', { name: 'Store key for Team A' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'configureInferenceCredential',
                context: CONTEXT,
                mode: 'keychain',
                workspace: REF_A,
                endpoint: ENDPOINT,
                secret: 'gw-CANARY-key',
            })
        )
        await waitFor(() => expect(secret).toHaveValue(''))
        await native.push({ ...native.current, tenants: ready(view(), keyFor(A)).tenants })
        expect(await screen.findByText(`A key for Team A is stored (Keychain). It is sent only to ${ENDPOINT}.`)).toBeInTheDocument()
        expect(document.body.textContent).not.toContain('gw-CANARY-key')
        // A key for another workspace is not offered here.
        await user.type(screen.getByLabelText('Model'), '@openai-prod/gpt-4o')
        await user.type(screen.getByLabelText('Prompt'), 'Say hello')
        expect(screen.getByRole('button', { name: 'Run prompt' })).toBeEnabled()
        await native.push({ ...native.current, tenants: ready(view(), keyFor(B)).tenants })
        expect(await screen.findByText(/The stored key is for workspace Team B\./)).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Run prompt' })).toBeDisabled()
    })

    it('runs once when asked, streams the output, and stopping never claims nothing was charged', async () => {
        const native = scriptedNative(ready(view(), keyFor(A)))
        const reads = scripted(native, answer)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Playground' }))
        await waitFor(() => expect(reads).toContainEqual({ kind: 'gateway', workspace: REF_A, read: { kind: 'inventory', resource: 'routes' } }))
        await user.type(screen.getByLabelText('Model'), '@openai-prod/gpt-4o')
        await user.selectOptions(await screen.findByLabelText('Route'), 'cfg-1')
        await user.type(screen.getByLabelText('Prompt'), 'Say hello')
        expect(screen.getByText(/may incur provider usage and cost\. It is sent once and never retried\./)).toBeInTheDocument()
        expect(native.dispatched.some((i) => i.kind === 'runInference')).toBe(false)
        await user.click(screen.getByRole('button', { name: 'Run prompt' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'runInference',
                context: CONTEXT,
                workspace: REF_A,
                run: { api: 'chat', model: '@openai-prod/gpt-4o', prompt: 'Say hello', route: 'cfg-1' },
            })
        )
        // The draft is kept on this Mac as typed.
        await waitFor(() => expect(native.dispatched.some((i) => i.kind === 'savePlaygroundDraft' && i.draft.prompt === 'Say hello')).toBe(true), { timeout: 3000 })
        await native.push({ ...native.current, gateway: view({ playground: run() }) })
        expect(await screen.findByText('Receiving…')).toBeInTheDocument()
        await native.push({ ...native.current, gateway: view({ playground: run({ output: 'Hello, wor' }) }) })
        expect(await screen.findByText('Hello, wor')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Run prompt' })).toBeDisabled()
        await user.click(screen.getByRole('button', { name: 'Stop run' }))
        expect(native.dispatched).toContainEqual({ kind: 'stopInference', operation: 'op-900' })
        await native.push({ ...native.current, gateway: view({ playground: run({ state: 'stopped', outcome: 'unknown', output: 'Hello, wor' }) }) })
        expect(await screen.findByText(/Stopped\. The Gateway may have run it and charged for it: find it in the logs by its trace\./)).toBeInTheDocument()
        expect(native.dispatched.filter((i) => i.kind === 'runInference')).toHaveLength(1)
        // Its trace finds it in the logs.
        await user.click(screen.getByRole('button', { name: 'Find in logs' }))
        expect(screen.getByRole('tab', { name: 'Logs' })).toHaveAttribute('aria-selected', 'true')
        await waitFor(() =>
            expect(reads).toContainEqual({ kind: 'gateway', workspace: REF_A, read: { kind: 'observe', read: { kind: 'logs', days: 7, page: 0, trace: 'airs-0123456789abcdef' } } })
        )
        expect(await screen.findByText('446 (blocked by a guardrail)')).toBeInTheDocument()
    })

    it('names a failed run and keeps uncertain runs on this Mac without their output', async () => {
        const kept = {
            id: `gi-${'c'.repeat(24)}`, workspace: A, api: 'chat' as const, model: 'gpt-4o', route: null, trace: 'airs-1', end: 'failed' as const, outcome: 'unknown' as const,
            failure: 'network' as const, answered: true, outputBytes: 12, usage: null, settledAt: '2026-10-20T00:00:00Z',
        }
        const native = scriptedNative(
            ready(view({ playground: run({ state: 'failed', outcome: 'rejected', failure: 'blocked', output: '' }), local: { ...view().local, runs: [kept] } }), keyFor(A))
        )
        scripted(native, answer)
        const user = await open(native)
        const local = await screen.findByRole('list', { name: 'Playground runs without a known result' })
        expect(within(local).getByText(/failed after it was sent and answered: the Gateway may have run it and charged for it\./)).toBeInTheDocument()
        expect(within(local).getByText(/Its output is not kept\./)).toBeInTheDocument()
        await user.click(within(local).getByRole('button', { name: 'Dismiss run: airs-1' }))
        expect(native.dispatched).toContainEqual({ kind: 'discardGatewayLocal', context: CONTEXT, id: kept.id })
        await user.click(screen.getByRole('tab', { name: 'Playground' }))
        expect(await screen.findByText(/Failed\. A Gateway guardrail blocked the request or its response\./)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Clear response' }))
        expect(native.dispatched).toContainEqual({ kind: 'clearInference', operation: 'op-900' })
    })

    it('shows usage, logs and limits with each source’s own state', async () => {
        const native = scriptedNative(ready())
        const reads = scripted(native, answer)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Usage' }))
        await waitFor(() => expect(reads).toContainEqual({ kind: 'gateway', workspace: REF_A, read: { kind: 'observe', read: { kind: 'usage', days: 7 } } }))
        const requests = await screen.findByRole('img', { name: /^Requests: Total 40; 2 intervals; highest 20/ })
        expect(requests).toBeInTheDocument()
        expect(screen.getByText(/Not read: The management service is unavailable/)).toBeInTheDocument()
        expect(screen.getByText('Errors in the window: 4')).toBeInTheDocument()
        expect(screen.getByRole('rowheader', { name: 'gpt-4o' })).toBeInTheDocument()
        // The chart's values are reachable as a table.
        await user.click(screen.getByText('Show requests values'))
        expect(screen.getByRole('table', { name: 'Requests by interval' })).toBeInTheDocument()
        await user.click(screen.getByRole('radio', { name: 'Last 30 days' }))
        await waitFor(() => expect(reads).toContainEqual({ kind: 'gateway', workspace: REF_A, read: { kind: 'observe', read: { kind: 'usage', days: 30 } } }))
        await user.click(screen.getByRole('tab', { name: 'Limits' }))
        expect(await screen.findByRole('rowheader', { name: 'Monthly' })).toBeInTheDocument()
        expect(screen.getByText('$50.00 · alert at 80%')).toBeInTheDocument()
        expect(screen.getByText(/Rate limits could not be read: This management client is not allowed/)).toBeInTheDocument()
        await user.click(screen.getByRole('tab', { name: 'Logs' }))
        await user.type(screen.getByLabelText('Trace ID'), 'bad trace')
        await user.click(screen.getByRole('button', { name: 'Read logs' }))
        expect(await screen.findByText(/A trace ID is 1-128 letters/)).toBeInTheDocument()
        // Nothing here sends inference.
        expect(native.dispatched.some((i) => i.kind === 'runInference')).toBe(false)
    })

    it('edits a kept Playground draft offline by its ID and keeps edits made just before leaving', async () => {
        const draft = {
            id: `gp-${'d'.repeat(24)}`, workspace: A, api: 'chat' as const, model: '@openai-prod/gpt-4o', route: null, system: null,
            prompt: 'kept prompt', file: null, savedAt: '2026-10-20T00:00:00Z',
        }
        // Offline: no listing, no workspace chosen.
        const offline = view({ workspaces: null, complete: false, selected: null, local: { ...view().local, playground: [draft] } })
        const native = scriptedNative(ready(offline, keyFor(A)))
        scripted(native, () => ({ kind: 'failed', failure: 'network', outcome: 'notSent' }))
        const user = await open(native)
        const list = await screen.findByRole('list', { name: 'Playground drafts' })
        await user.click(within(list).getByRole('button', { name: 'Edit Playground draft for Team A' }))
        const prompt = within(list).getByLabelText('Draft prompt')
        expect(prompt).toHaveValue('kept prompt')
        await user.clear(prompt)
        await user.type(prompt, 'edited offline')
        await user.click(within(list).getByRole('radio', { name: 'Responses' }))
        // Done keeps the pending edit at once, by the draft's ID, with no workspace.
        await user.click(within(list).getByRole('button', { name: 'Done editing' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'savePlaygroundDraft',
                context: CONTEXT,
                draft: { id: draft.id, api: 'responses', model: '@openai-prod/gpt-4o', prompt: 'edited offline' },
            })
        )
        expect(native.dispatched.some((i) => i.kind === 'savePlaygroundDraft' && 'workspace' in i)).toBe(false)
        await waitFor(() => expect(within(list).getByRole('button', { name: 'Edit Playground draft for Team A' })).toHaveFocus())
        expect(native.dispatched.some((i) => i.kind === 'runInference')).toBe(false)
    })

    it('keeps a Playground edit made just before leaving the tab', async () => {
        const native = scriptedNative(ready(view(), keyFor(A)))
        scripted(native, answer)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Playground' }))
        await user.type(screen.getByLabelText('Model'), 'm')
        await user.type(screen.getByLabelText('Prompt'), 'left at once')
        // Within the pause: leaving the form keeps it immediately.
        await user.click(screen.getByRole('tab', { name: 'Usage' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'savePlaygroundDraft',
                context: CONTEXT,
                workspace: REF_A,
                draft: { api: 'chat', model: 'm', prompt: 'left at once' },
            })
        )
    })

    it('runs with the Responses API when chosen', async () => {
        const native = scriptedNative(ready(view(), keyFor(A)))
        scripted(native, answer)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Playground' }))
        const apis = screen.getByRole('radiogroup', { name: 'Inference API' })
        expect(within(apis).getByRole('radio', { name: 'Chat completions' })).toBeChecked()
        await user.click(within(apis).getByRole('radio', { name: 'Responses' }))
        await user.type(screen.getByLabelText('Model'), '@openai-prod/gpt-4o')
        await user.type(screen.getByLabelText('Prompt'), 'hi')
        await user.click(screen.getByRole('button', { name: 'Run prompt' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'runInference',
                context: CONTEXT,
                workspace: REF_A,
                run: { api: 'responses', model: '@openai-prod/gpt-4o', prompt: 'hi' },
            })
        )
        await native.push({
            ...native.current,
            gateway: view({ playground: run({ api: 'responses', state: 'failed', outcome: 'completed', failure: 'modelFailed', output: 'part' }) }),
        })
        expect(await screen.findByText(/Failed\. The model failed while answering\./)).toBeInTheDocument()
        expect(screen.getByText('Responses', { selector: 'dd' })).toBeInTheDocument()
    })

    it('continues the logs of the query shown, never mixes queries and never calls a capped list complete', async () => {
        type LogsRead = Extract<ManagementRequest, { kind: 'gateway' }>['read']
        const row = (id: string, trace: string) => ({
            id, createdAt: '2026-10-20T00:00:00Z', model: 'gpt-4o', provider: null, status: 200, units: null, cost: null, traceId: trace, cacheStatus: null, apiKeyId: null,
        })
        let release: (() => void) | null = null
        const logs: Answer = async (request) => {
            if (request.kind !== 'gateway' || request.read.kind !== 'observe' || request.read.read.kind !== 'logs') return answer(request)
            const o = request.read.read
            const trace = o.trace ?? null
            if (o.page === 1 && trace === null && release === null) await new Promise<void>((r) => (release = r))
            return observed({
                kind: 'logs',
                logs: {
                    workspace: 'team-a', days: o.days, page: o.page, trace, status: o.status ?? null,
                    rows: [row(`log-${trace ?? 'all'}-${o.page}`, trace ?? `t-${o.page}`)],
                    total: trace === null ? 5000 : 1, more: trace === null && o.page < 39, capped: trace === null && o.page === 39, partial: false,
                },
            })
        }
        const native = scriptedNative(ready())
        const reads = scripted(native, logs)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Logs' }))
        expect(await screen.findByText('t-0')).toBeInTheDocument()
        // The trace is edited but not applied: more continues the list shown.
        await user.type(screen.getByLabelText('Trace ID'), 'trace-B')
        await user.click(screen.getByRole('button', { name: 'Read more logs' }))
        await waitFor(() => expect(reads.filter((r) => r.kind === 'gateway' && (r.read as LogsRead).kind === 'observe')).toHaveLength(2))
        const continuation = reads.at(-1) as Extract<ManagementRequest, { kind: 'gateway' }>
        expect(continuation.read).toEqual({ kind: 'observe', read: { kind: 'logs', days: 7, page: 1 } })
        // Applying the filter while that page is out: the late page never joins.
        await user.click(screen.getByRole('button', { name: 'Read logs' }))
        expect(await screen.findByText('trace-B')).toBeInTheDocument()
        ;(release as (() => void) | null)?.()
        await new Promise((r) => setTimeout(r, 50))
        expect(screen.queryByText('t-0')).not.toBeInTheDocument()
        expect(screen.queryByText('t-1')).not.toBeInTheDocument()
        expect(screen.getByText(/1 request shown of 1 for last 7 days, trace trace-B · all read/)).toBeInTheDocument()
        // A capped list says more exist; nothing more is offered.
        await user.clear(screen.getByLabelText('Trace ID'))
        native.observeManagement = async () => ({
            kind: 'management',
            result: observed({
                kind: 'logs',
                logs: { workspace: 'team-a', days: 7, page: 0, trace: null, status: null, rows: [row('log-x', 't-x')], total: 5000, more: false, capped: true, partial: false },
            }),
        })
        await user.click(screen.getByRole('button', { name: 'Read logs' }))
        expect(await screen.findByText(/only the first 1,000 are read here: more exist/)).toBeInTheDocument()
        expect(screen.queryByText(/all read/)).not.toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Read more logs' })).not.toBeInTheDocument()
    })

    it('tells a complete answer from one the Gateway cut short and from a failed one', async () => {
        const native = scriptedNative(ready(view(), keyFor(A)))
        scripted(native, answer)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Playground' }))
        const usage = { prompt: 2, completion: 256, total: 258 }
        const ended = { api: 'responses' as const, outcome: 'completed' as const, sent: true, answered: true }
        await native.push({ ...native.current, gateway: view({ playground: run({ ...ended, state: 'completed', finish: 'completed', output: 'Whole answer', usage }) }) })
        const status = () => document.querySelector('[data-slot="gateway-playground-state"]')
        expect(await screen.findByText('Whole answer')).toBeInTheDocument()
        expect(status()).toHaveTextContent(/^Completed\.$/)
        expect(document.querySelector('[data-slot="gateway-playground-finish"]')).toHaveTextContent(/^completed$/)
        await native.push({
            ...native.current,
            gateway: view({ playground: run({ ...ended, state: 'incomplete', finish: 'max_output_tokens', output: 'Partial answ', usage }) }),
        })
        expect(await screen.findByText('Partial answ')).toBeInTheDocument()
        expect(status()).toHaveTextContent(
            'Incomplete: the Gateway ended the answer before it was finished. The output below is partial. It reached the output token limit (max_output_tokens).'
        )
        expect(status()).toHaveAttribute('role', 'status')
        expect(status()).not.toHaveTextContent(/charged|Nothing was sent/)
        expect(document.querySelector('[data-slot="gateway-playground-finish"]')).toHaveTextContent('max_output_tokens (incomplete)')
        expect(screen.getByText('258 (2 in, 256 out)')).toBeInTheDocument()
        expect(screen.getByText('airs-0123456789abcdef')).toBeInTheDocument()
        await native.push({ ...native.current, gateway: view({ playground: run({ api: 'chat', state: 'incomplete', outcome: 'completed', finish: 'content_filter', output: 'x' }) }) })
        await waitFor(() => expect(status()).toHaveTextContent(/A content filter stopped it \(content_filter\)\.$/))
        await native.push({ ...native.current, gateway: view({ playground: run({ ...ended, state: 'failed', failure: 'modelFailed', output: 'part' }) }) })
        await waitFor(() => expect(status()).toHaveTextContent(/^Failed\. The model failed while answering\./))
        expect(status()).not.toHaveTextContent(/Incomplete/)
    })

    it('keeps the Files view usable with a full list after uploads and times no date can render', async () => {
        expect(createdText(1760000000)).toBe('2025-10-09 08:53 UTC')
        expect(createdText(8_640_000_000_000)).toBe('+275760-09-13 00:00 UTC')
        expect(createdText(9_000_000_000_000)).toBe('Unknown time')
        expect(createdText(null)).toBe('Unknown time')
        const native = scriptedNative(ready(view(), keyFor(A)))
        scripted(native, answer)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Files' }))
        // The snapshot after uploads into a full list, through the client's
        // own validation: the bound kept, more reported, odd times unknown.
        const full = Array.from({ length: 200 }, (_, i) => ({
            id: `file-${i}`, filename: `f${i}.txt`, bytes: 1, createdAt: i === 0 ? null : 1760000000, purpose: 'assistants', status: 'processed',
        }))
        await native.push({
            ...native.current,
            gateway: view({
                files: {
                    workspace: A, files: full, more: true, reading: false, readFailure: null, chosen: null, choosing: false, chooseFailure: null,
                    action: { action: 'upload', filename: 'f0.txt', file: 'file-0', bytes: 1, trace: 'airs-00000000000000cc', outcome: 'completed', failure: null, sent: true },
                },
            }),
        })
        const table = await screen.findByRole('table', { name: 'Files in Team A' })
        expect(within(table).getAllByRole('row')).toHaveLength(201)
        expect(within(table).getByText('Unknown time')).toBeInTheDocument()
        expect(screen.getByText(/200 files shown: the Gateway holds more, and only the first 200 are shown here\./)).toBeInTheDocument()
        expect(document.querySelector('[data-slot="gateway-files-action-state"]')).toHaveTextContent('Uploaded f0.txt as file-0.')
        // The file with an unknown time is still usable.
        await user.click(screen.getByRole('button', { name: 'Delete f0.txt (file-0)' }))
        await user.click(screen.getByRole('button', { name: 'Delete file' }))
        expect(native.dispatched).toContainEqual({ kind: 'deleteGatewayFile', context: CONTEXT, workspace: REF_A, file: 'file-0' })
    })

    it('reads, uploads and deletes Gateway files once each, and the view never holds a path or content', async () => {
        const file = { id: 'file-abc123', filename: 'notes.txt', bytes: 2048, createdAt: 1760000000, purpose: 'assistants', status: 'processed' }
        const files = (overrides: Partial<NonNullable<GatewayView['files']>> = {}): GatewayView['files'] => ({
            workspace: A, files: null, more: false, reading: false, readFailure: null, chosen: null, choosing: false, chooseFailure: null, action: null, ...overrides,
        })
        const native = scriptedNative(ready(view(), keyFor(A)))
        scripted(native, answer)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Files' }))
        expect(screen.getByRole('heading', { name: 'Files in Team A' })).toBeInTheDocument()
        expect(screen.getByText(/stays in the Gateway until it is deleted; this Mac keeps no copy/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Read files' }))
        expect(native.dispatched).toContainEqual({ kind: 'readGatewayFiles', context: CONTEXT, workspace: REF_A })
        await native.push({ ...native.current, gateway: view({ files: files({ files: [file], more: true }) }) })
        const table = await screen.findByRole('table', { name: 'Files in Team A' })
        expect(within(table).getByText('notes.txt')).toBeInTheDocument()
        expect(within(table).getByText('2.0 KiB')).toBeInTheDocument()
        expect(screen.getByText(/1 file shown: the Gateway holds more, and only the first 200 are shown here\./)).toBeInTheDocument()

        await user.click(screen.getByRole('button', { name: 'Choose file…' }))
        expect(native.dispatched).toContainEqual({ kind: 'chooseGatewayFile', context: CONTEXT, workspace: REF_A })
        const token = `gf-${'e'.repeat(24)}`
        await native.push({ ...native.current, gateway: view({ files: files({ files: [file], chosen: { token, name: 'report.csv', bytes: 8 } }) }) })
        expect(await screen.findByText('report.csv · 8 bytes')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Upload report.csv to Team A' }))
        expect(native.dispatched).toContainEqual({ kind: 'uploadGatewayFile', context: CONTEXT, workspace: REF_A, file: token })
        expect(native.sent.join('\n')).not.toMatch(/\/Users\/|report\.csv"[^}]*content/)
        const action = { action: 'upload' as const, filename: 'report.csv', file: null, bytes: 8, trace: 'airs-00000000000000aa', failure: null }
        await native.push({ ...native.current, gateway: view({ files: files({ files: [file], action: { ...action, outcome: null, sent: true } }) }) })
        const actionState = () => document.querySelector('[data-slot="gateway-files-action-state"]')
        await waitFor(() => expect(actionState()).toHaveTextContent('Uploading report.csv…'))
        await waitFor(() => expect(actionState()).toHaveFocus())
        expect(screen.getByRole('button', { name: 'Read files' })).toBeDisabled()
        await native.push({
            ...native.current,
            gateway: view({ files: files({ files: [file], action: { ...action, outcome: 'unknown', failure: 'unavailable', sent: true } }) }),
        })
        await waitFor(() =>
            expect(actionState()).toHaveTextContent(
                'The upload of report.csv may have been stored: its result is not known. The Gateway or its provider failed. It is kept on this Mac with trace airs-00000000000000aa and is never sent again. Read the files to see what exists.'
            )
        )
        expect(screen.queryByRole('button', { name: /^Upload / })).not.toBeInTheDocument()

        // Delete asks first; Cancel returns to the row and sends nothing.
        await user.click(screen.getByRole('button', { name: 'Delete notes.txt (file-abc123)' }))
        const confirm = screen.getByRole('group', { name: /Delete notes\.txt \(file-abc123\) from the Gateway\?/ })
        expect(within(confirm).getByRole('button', { name: 'Delete file' })).toHaveFocus()
        await user.click(within(confirm).getByRole('button', { name: 'Cancel' }))
        await waitFor(() => expect(screen.getByRole('button', { name: 'Delete notes.txt (file-abc123)' })).toHaveFocus())
        expect(native.dispatched.some((i) => i.kind === 'deleteGatewayFile')).toBe(false)
        await user.click(screen.getByRole('button', { name: 'Delete notes.txt (file-abc123)' }))
        await user.click(screen.getByRole('button', { name: 'Delete file' }))
        expect(native.dispatched.filter((i) => i.kind === 'deleteGatewayFile')).toEqual([
            { kind: 'deleteGatewayFile', context: CONTEXT, workspace: REF_A, file: 'file-abc123' },
        ])
        await native.push({
            ...native.current,
            gateway: view({ files: files({ files: [], action: { action: 'delete', filename: 'notes.txt', file: 'file-abc123', bytes: null, trace: 'airs-00000000000000bb', failure: null, outcome: 'completed', sent: true } }) }),
        })
        await waitFor(() => expect(actionState()).toHaveTextContent('Deleted notes.txt from the Gateway.'))
        expect(screen.getByText('No files.')).toBeInTheDocument()
        // A file the sheet could not take is named, and nothing is held.
        await native.push({ ...native.current, gateway: view({ files: files({ files: [], chooseFailure: 'tooLarge' }) }) })
        expect(await screen.findByRole('alert')).toHaveTextContent('That file is larger than 4 MiB. Nothing was read for upload.')
    })

    it('attaches a file the workspace listed to a Responses run, and keeps it in the draft', async () => {
        const listed = { id: 'file-abc123', filename: 'notes.txt', bytes: 12, createdAt: 1760000000, purpose: 'assistants', status: 'processed' }
        const files: GatewayView['files'] = {
            workspace: A, files: [listed], more: false, reading: false, readFailure: null, chosen: null, choosing: false, chooseFailure: null, action: null,
        }
        const native = scriptedNative(ready(view({ files: null }), keyFor(A)))
        scripted(native, answer)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Playground' }))
        expect(screen.queryByLabelText('Attached file (optional)')).not.toBeInTheDocument()
        await user.click(within(screen.getByRole('radiogroup', { name: 'Inference API' })).getByRole('radio', { name: 'Responses' }))
        expect(screen.getByText("Read this workspace's files in the Files tab to attach one.")).toBeInTheDocument()
        await native.push({ ...native.current, gateway: view({ files }) })
        await user.selectOptions(await screen.findByLabelText('Attached file (optional)'), 'file-abc123')
        await user.type(screen.getByLabelText('Model'), '@openai-prod/gpt-4o')
        await user.type(screen.getByLabelText('Prompt'), 'summarize')
        await user.click(screen.getByRole('button', { name: 'Run prompt' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'runInference',
                context: CONTEXT,
                workspace: REF_A,
                run: { api: 'responses', model: '@openai-prod/gpt-4o', prompt: 'summarize', file: 'file-abc123' },
            })
        )
        expect(native.dispatched).toContainEqual(
            expect.objectContaining({ kind: 'savePlaygroundDraft', draft: expect.objectContaining({ api: 'responses', file: 'file-abc123' }) })
        )
        await native.push({
            ...native.current,
            gateway: view({ files, playground: run({ api: 'responses', file: 'file-abc123', state: 'completed', outcome: 'completed', finish: 'completed' }) }),
        })
        expect(await screen.findByText('file-abc123', { selector: 'dd' })).toBeInTheDocument()
        // Back to chat: the file is not sent.
        await user.click(within(screen.getByRole('radiogroup', { name: 'Inference API' })).getByRole('radio', { name: 'Chat completions' }))
        expect(screen.queryByLabelText('Attached file (optional)')).not.toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Clear response' }))
        await native.push({ ...native.current, gateway: view({ files }) })
        await user.click(screen.getByRole('button', { name: 'Run prompt' }))
        await waitFor(() =>
            expect(native.dispatched.filter((i) => i.kind === 'runInference').at(-1)).toEqual({
                kind: 'runInference',
                context: CONTEXT,
                workspace: REF_A,
                run: { api: 'chat', model: '@openai-prod/gpt-4o', prompt: 'summarize' },
            })
        )
    })

    it('shows file changes without a known result offline, and dismisses one locally', async () => {
        const record = {
            id: `gf-${'f'.repeat(24)}`, workspace: A, action: 'upload' as const, filename: 'report.csv', file: null, bytes: 8, trace: 'airs-00000000000000cc',
            outcome: 'unknown' as const, failure: 'network' as const, settledAt: '2026-10-20T00:00:00Z',
        }
        const offline = view({ workspaces: null, complete: false, selected: null, local: { ...view().local, files: [record] } })
        const native = scriptedNative(ready(offline, keyFor(A)))
        scripted(native, () => ({ kind: 'failed', failure: 'network', outcome: 'notSent' }))
        await open(native)
        const list = await screen.findByRole('list', { name: 'Gateway file changes without a known result' })
        expect(within(list).getByText(/^Upload of report\.csv \(8 bytes\) in Team A was sent without a known result: the file may have been stored\./)).toBeInTheDocument()
        expect(within(list).getByText(/Trace airs-00000000000000cc .* It is never sent again/)).toBeInTheDocument()
        await userEvent.setup().click(within(list).getByRole('button', { name: 'Dismiss file record: airs-00000000000000cc' }))
        expect(native.dispatched).toContainEqual({ kind: 'discardGatewayLocal', context: CONTEXT, id: record.id })
        expect(native.dispatched.some((i) => i.kind === 'uploadGatewayFile' || i.kind === 'readGatewayFiles')).toBe(false)
    })

    it('draws a long series with at most 160 bars and loses nothing it sums', () => {
        const values = Array.from({ length: 720 }, (_, i) => i % 7)
        const drawn = bars(values, false)
        expect(drawn.length).toBeLessThanOrEqual(160)
        expect(drawn.reduce((a, b) => a + b, 0)).toBe(values.reduce((a, b) => a + b, 0))
        expect(bars([300, 500], true)).toEqual([300, 500])
        expect(Math.max(...bars(Array(720).fill(350), true))).toBe(350)
    })
})
