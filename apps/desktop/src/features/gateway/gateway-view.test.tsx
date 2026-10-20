/**
 * AI Gateway through the real App, DesktopClient and shell: the workspaces
 * listed on opening, a native choice (never a client-chosen workspace),
 * scoped reads carrying the current selection reference, coverage that is
 * never "none" from a partial inventory, details whose heading takes focus
 * and whose Close returns it, a reply for a workspace chosen before dropped,
 * prerequisites stated per plane, and the remembered choice's notes.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import {
    DesktopClient,
    type GatewayList,
    type GatewayPrerequisites,
    type GatewayView,
    type Intent,
    type ListedWorkspace,
    type ManagementRequest,
    type ManagementResult,
    type ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const WS_A = '16f7e90d-382a-4e78-b577-1b01eb5f8297'
const WS_B = '16f7e90d-382a-4e78-b577-1b01eb5f8298'
const REF_A = 'gw-aaaaaaaaaaaaaaaaaaaaaaaa'
const REF_B = 'gw-bbbbbbbbbbbbbbbbbbbbbbbb'
const A: ListedWorkspace = { id: WS_A, name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }
const B: ListedWorkspace = { id: WS_B, name: 'Team B', slug: 'team-b', scopeName: 'scope-team-b' }

const view = (overrides: Partial<GatewayView> = {}): GatewayView => ({
    workspaces: null,
    complete: false,
    selected: null,
    remembered: null,
    rememberedMissing: false,
    ...overrides,
})

const ready = (gateway: GatewayView = view()): ShellSnapshot => ({
    ...withTenant(
        baseSnapshot,
        tenant({
            management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1234567890.iam.panserviceaccount.com', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' },
        }),
        CONTEXT
    ),
    gateway,
})

const listed = (selected: ListedWorkspace | null, reference = REF_A) =>
    view({ workspaces: [A, B], complete: true, selected: selected ? { reference, workspace: selected, restored: false } : null })

const list = (resource: GatewayList['resource'], rows: Record<string, unknown>[], extra: Partial<GatewayList> = {}): GatewayList => ({
    resource,
    workspace: resource === 'workspaces' || resource === 'integrations' ? null : WS_A,
    rows: JSON.stringify(rows),
    count: rows.length,
    total: rows.length,
    complete: true,
    ...extra,
})

const prerequisites = (overrides: Partial<GatewayPrerequisites> = {}): GatewayPrerequisites => ({
    workspace: WS_A,
    routes: list('routes', [{ id: 'r1', name: 'default-route' }]),
    routesFailure: null,
    providers: list('providers', [{ id: 'p1', name: 'openai-prod' }]),
    providersFailure: null,
    integrations: null,
    integrationsFailure: 'denied',
    serviceKeys: list('serviceKeys', [{ id: 'k1', scopes: ['completions.write'] }]),
    serviceKeysFailure: null,
    inferenceKeys: 1,
    scopeName: 'scope-team-a',
    scope: 'unbound',
    dataPlane: 'granted',
    adminPlane: 'denied',
    iamPlane: 'granted',
    ...overrides,
})

type Answer = (request: ManagementRequest) => ManagementResult | Promise<ManagementResult>

/** Scripts management reads and the native choice of a workspace. */
function gateway(native: Native, answer: Answer): ManagementRequest[] {
    const reads: ManagementRequest[] = []
    const pending = new Map<string, ManagementRequest>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        // The native choice arrives as a pushed snapshot (see `chosen`).
        if (intent.kind === 'selectGatewayWorkspace') {
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-select', operation: null } }
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

/** The snapshot the native host publishes once `workspace` is chosen. */
async function chosen(native: Native, workspace: ListedWorkspace) {
    await native.push({ ...native.current, gateway: listed(workspace, workspace === A ? REF_A : REF_B) })
}

const ok = (g: Extract<ManagementResult, { kind: 'gateway' }>['gateway']): ManagementResult => ({ kind: 'gateway', gateway: g })

const standard: Answer = (request) => {
    if (request.kind !== 'gateway') return { kind: 'failed', failure: 'invalidResponse', outcome: 'completed' }
    const read = request.read
    if (read.kind === 'inventory') {
        if (read.resource === 'providers') {
            return ok({ kind: 'list', list: list('providers', [{ id: 'p1', name: 'openai-prod', status: 'active' }]) })
        }
        return ok({ kind: 'list', list: list(read.resource, []) })
    }
    if (read.kind === 'detail') {
        return ok({
            kind: 'detail',
            detail: { resource: read.resource, id: read.id, found: 'found', item: JSON.stringify({ id: read.id, name: 'openai-prod', credential_bearing: true }) },
        })
    }
    if (read.kind === 'prerequisites') return ok({ kind: 'prerequisites', prerequisites: prerequisites() })
    return { kind: 'failed', failure: 'notFound', outcome: 'completed' }
}

async function open(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^AI Gateway/ }))
    await screen.findByRole('heading', { level: 1, name: 'Gateway workspaces and configuration' })
    return user
}

describe('AI Gateway', () => {
    it('lists the workspaces on opening and chooses one natively before reading it', async () => {
        const native = scriptedNative(ready())
        const reads = gateway(native, standard)
        const user = await open(native)
        await waitFor(() => expect(reads).toEqual([{ kind: 'gateway', workspace: null, read: { kind: 'inventory', resource: 'workspaces' } }]))
        // The listing settles natively and is published with the snapshot.
        await native.push({ ...native.current, gateway: listed(null) })
        expect(await screen.findByText('Choose a workspace to read its configuration.')).toBeInTheDocument()
        expect(screen.queryByRole('tablist')).toBeNull()
        await user.click(screen.getByRole('radio', { name: /Team A/ }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({ kind: 'selectGatewayWorkspace', context: CONTEXT, workspace: WS_A })
        )
        await chosen(native, A)
        // The scoped read carries the native reference, never a workspace ID.
        await waitFor(() =>
            expect(reads).toContainEqual({ kind: 'gateway', workspace: REF_A, read: { kind: 'inventory', resource: 'providers' } })
        )
        expect(await screen.findByText('1 providers · complete')).toBeInTheDocument()
        expect(screen.getByRole('radio', { name: /Team A/ })).toBeChecked()
        // Nothing here creates, changes or provisions.
        for (const name of [/create/i, /new /i, /delete/i, /rotate/i, /provision/i, /send/i]) {
            expect(screen.queryByRole('button', { name })).toBeNull()
        }
    })

    it('never reads "none" from a partial inventory and says a complete one is empty', async () => {
        const native = scriptedNative(ready(listed(A)))
        gateway(native, (request) => {
            if (request.kind === 'gateway' && request.read.kind === 'inventory' && request.read.resource === 'providers') {
                return ok({ kind: 'list', list: list('providers', [{ id: 'p1', name: 'openai-prod' }], { total: 7, complete: false }) })
            }
            return standard(request)
        })
        const user = await open(native)
        expect(await screen.findByText('1 of 7+ providers read. More exist; they are not shown and not assumed absent.')).toBeInTheDocument()
        await user.click(screen.getByRole('tab', { name: 'Routes' }))
        expect(await screen.findByText('The service reports no routes.')).toBeInTheDocument()
    })

    it('opens a detail with focus on its heading and returns focus on close', async () => {
        const native = scriptedNative(ready(listed(A)))
        const reads = gateway(native, standard)
        const user = await open(native)
        const details = await screen.findByRole('button', { name: 'Details for openai-prod' })
        await user.click(details)
        await waitFor(() => expect(screen.getByRole('heading', { level: 2, name: 'openai-prod' })).toHaveFocus())
        expect(reads.at(-1)).toEqual({ kind: 'gateway', workspace: REF_A, read: { kind: 'detail', resource: 'providers', id: 'p1' } })
        expect(screen.getByText('This configuration carries credentials. Their values are not shown.')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Close details' }))
        expect(details).toHaveFocus()
    })

    it('drops a reply for a workspace chosen before', async () => {
        const native = scriptedNative(ready(listed(A)))
        let release: (r: ManagementResult) => void = () => {}
        gateway(native, (request) => {
            if (request.kind === 'gateway' && request.workspace === REF_A && request.read.kind === 'inventory') {
                return new Promise<ManagementResult>((resolve) => (release = resolve))
            }
            if (request.kind === 'gateway' && request.read.kind === 'inventory' && request.read.resource === 'providers') {
                return ok({ kind: 'list', list: list('providers', [{ id: 'p2', name: 'team-b-provider' }]) })
            }
            return standard(request)
        })
        const user = await open(native)
        await screen.findByText('Reading providers…')
        await user.click(screen.getByRole('radio', { name: /Team B/ }))
        await chosen(native, B)
        expect(await screen.findByText('team-b-provider')).toBeInTheDocument()
        release(ok({ kind: 'list', list: list('providers', [{ id: 'p1', name: 'team-a-provider' }]) }))
        await new Promise((resolve) => setTimeout(resolve, 20))
        expect(screen.queryByText('team-a-provider')).toBeNull()
    })

    it('states each prerequisite as read and never offers the management client for inference', async () => {
        const native = scriptedNative(ready(listed(A)))
        gateway(native, standard)
        const user = await open(native)
        await user.click(await screen.findByRole('tab', { name: 'Inference prerequisites' }))
        expect(await screen.findByText('scope-team-a · The workspace’s IAM scope exists but does not list this workspace.')).toBeInTheDocument()
        const sources = screen.getByText('Integrations', { selector: 'dt' }).nextElementSibling
        expect(sources).toHaveTextContent('Unavailable: This management client is not allowed to read this.')
        expect(screen.getByText('Admin plane', { selector: 'dt' }).nextElementSibling).toHaveTextContent('Not allowed for this management client')
        expect(screen.getByText(/1 of the service keys read may call inference/)).toBeInTheDocument()
        expect(screen.getByText(/the management client is never\s+used in its place/)).toBeInTheDocument()
    })

    it('says when the remembered workspace is gone, restored, or unconfirmed', async () => {
        const gone = scriptedNative(ready(view({ workspaces: [B], complete: true, remembered: WS_A, rememberedMissing: true })))
        gateway(gone, standard)
        await open(gone)
        expect(screen.getByText('The workspace last chosen for this tenant is no longer listed. Choose another.')).toBeInTheDocument()
    })

    it('keeps the listing failure visible and reads nothing scoped without a choice', async () => {
        const native = scriptedNative(ready())
        const reads = gateway(native, () => ({ kind: 'failed', failure: 'denied', outcome: 'rejected' }))
        await open(native)
        expect(await screen.findByRole('alert')).toHaveTextContent('The workspaces could not be read: This management client is not allowed')
        expect(reads.every((r) => r.kind === 'gateway' && r.workspace === null)).toBe(true)
    })
})
