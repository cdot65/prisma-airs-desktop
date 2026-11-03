/**
 * AI Gateway through the real App, DesktopClient and shell: the workspaces
 * listed on opening, a native choice (never a client-chosen workspace),
 * scoped reads carrying the current selection reference, coverage that is
 * never "none" from a partial inventory, details whose heading takes focus
 * and whose Close returns it, a reply for a workspace chosen before dropped,
 * prerequisites stated per plane, and the remembered choice's notes. C17:
 * the chosen workspace's own detail, integration models bound to the
 * integration shown, reviewed changes carrying the selection reference with
 * their exact scope and blocking references, and provisioning steps whose
 * summary claims only what reconciliation established. Drafts and uncertain
 * results kept on this Mac are read and edited offline and reviewed again
 * only when asked.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import {
    type ChangeResultView,
    type ChangeReviewView,
    DesktopClient,
    type GatewayList,
    type GatewayLocalView,
    type GatewayPrerequisites,
    type GatewayView,
    type Intent,
    type ListedWorkspace,
    type ManagementRequest,
    type ManagementResult,
    type Provisioning,
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
    local: { state: 'ready', failure: null, drafts: [], records: [], playground: [], runs: [], files: [], problem: null, unsaved: false },
    playground: null,
    files: null,
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
        // Reading changes nothing: no change is prepared until asked.
        expect(screen.getByRole('button', { name: 'New provider…' })).toBeInTheDocument()
        expect(native.dispatched.some((i) => i.kind === 'reviewChange' || i.kind === 'requestApply')).toBe(false)
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
        // No key yet: the Playground is where one is stored.
        const credential = () => document.querySelector('[data-slot="gateway-inference-credential"]')
        expect(credential()).toHaveTextContent(
            "No inference key is stored for Team A. To run inference from here, store the workspace's runtime endpoint and an API key with completions access in the Playground tab. The management client is never used in its place."
        )
        expect(credential()).not.toHaveTextContent(/is stored \(/)
        // A key stored for this workspace is said so, from the same state the Playground uses.
        const tenants = (inference: ShellSnapshot['tenants'][number]['inference']) =>
            withTenant(baseSnapshot, tenant({ management: native.current.tenants[0]!.management, inference }), CONTEXT).tenants
        const endpoint = 'https://gateway.example/v1'
        await native.push({ ...native.current, tenants: tenants({ state: 'ready', mode: 'keychain', failure: null, target: { workspace: A, endpoint } }) })
        await waitFor(() =>
            expect(credential()).toHaveTextContent(
                `An inference key for Team A is stored (Keychain) and is sent only to ${endpoint}. Run prompts from the Playground tab. The management client is never used in its place.`
            )
        )
        expect(credential()).not.toHaveTextContent(/No inference key/)
        // A key for another workspace does not serve this one.
        await native.push({ ...native.current, tenants: tenants({ state: 'ready', mode: 'keychain', failure: null, target: { workspace: B, endpoint } }) })
        await waitFor(() => expect(credential()).toHaveTextContent(/^The stored inference key is for workspace Team B\. To run inference from Team A, store a key for it/))
        await user.click(within(credential() as HTMLElement).getByRole('button', { name: 'Open Playground' }))
        expect(screen.getByRole('tab', { name: 'Playground' })).toHaveAttribute('aria-selected', 'true')
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

    it('shows the chosen workspace’s own settings and limits, and its failures', async () => {
        const native = scriptedNative(ready(listed(A)))
        let deny = false
        // Holds the next detail read of A until released.
        let holdA = false
        let releaseA = () => {}
        const reads = gateway(native, (request) => {
            if (request.kind === 'gateway' && request.read.kind === 'detail' && request.read.resource === 'workspaces') {
                if (deny) return { kind: 'failed', failure: 'denied', outcome: 'rejected' }
                const reply = ok({
                    kind: 'detail',
                    detail: {
                        resource: 'workspaces',
                        id: request.read.id,
                        found: 'found',
                        item: JSON.stringify({
                            id: request.read.id,
                            name: request.read.id === WS_A ? 'Team A' : 'Team B',
                            defaults: { config_id: 'r1', allow_config_override: true },
                            usage_limits: [{ credit_limit: 100, type: 'cost' }],
                            rate_limits: [{ type: 'requests', unit: 'rpm', value: 60 }],
                        }),
                    },
                })
                if (holdA && request.read.id === WS_A) {
                    holdA = false
                    return new Promise<ManagementResult>((resolve) => (releaseA = () => resolve(reply)))
                }
                return reply
            }
            return standard(request)
        })
        const user = await open(native)
        await user.click(await screen.findByRole('tab', { name: 'Workspace' }))
        // An unscoped read of the workspace the native choice names.
        await waitFor(() => expect(reads).toContainEqual({ kind: 'gateway', workspace: null, read: { kind: 'detail', resource: 'workspaces', id: WS_A } }))
        const fields = await screen.findByText('rate_limits', { selector: 'dt' })
        expect(fields.nextElementSibling).toHaveTextContent('rpm')
        expect(screen.getByText('defaults', { selector: 'dt' }).nextElementSibling).toHaveTextContent('allow_config_override')
        deny = true
        await user.click(screen.getByRole('button', { name: /Refresh workspace$/ }))
        expect(await screen.findByRole('alert')).toHaveTextContent('The workspace could not be read: This management client is not allowed')
        // A refresh of A is still out when B is chosen: B starts with nothing
        // held, shows only its own detail, and A's late reply is dropped.
        deny = false
        holdA = true
        await user.click(screen.getByRole('button', { name: /Refresh workspace$/ }))
        await screen.findByText('Reading the workspace…')
        await chosen(native, B)
        await waitFor(() => expect(reads.filter((r) => r.kind === 'gateway' && r.read.kind === 'detail' && r.read.id === WS_B)).toHaveLength(1))
        await waitFor(() => expect(screen.getByText('id', { selector: 'dt' }).nextElementSibling).toHaveTextContent(WS_B))
        releaseA()
        await new Promise((resolve) => setTimeout(resolve, 20))
        expect(screen.getByText('id', { selector: 'dt' }).nextElementSibling).toHaveTextContent(WS_B)
        expect(screen.queryByText(WS_A)).toBeNull()
        expect(screen.queryByText(/The workspace could not be read/)).toBeNull()
    })

    it('never shows one integration’s models under another', async () => {
        const native = scriptedNative(ready(listed(A)))
        const two = [
            { id: 'int-a', name: 'Integration A' },
            { id: 'int-b', name: 'Integration B' },
        ]
        let releaseA: (r: ManagementResult) => void = () => {}
        let holdA = false
        gateway(native, (request) => {
            if (request.kind !== 'gateway') return standard(request)
            const read = request.read
            if (read.kind === 'inventory' && read.resource === 'integrations') return ok({ kind: 'list', list: list('integrations', two) })
            if (read.kind === 'detail' && read.resource === 'integrations') {
                return ok({ kind: 'detail', detail: { resource: 'integrations', id: read.id, found: 'found', item: JSON.stringify({ id: read.id, name: read.id === 'int-a' ? 'Integration A' : 'Integration B' }) } })
            }
            if (read.kind === 'integrationModels') {
                const models = { integration: read.id, models: JSON.stringify([{ slug: `model-of-${read.id}`, enabled: true }]), count: 1, total: 1, complete: true, allowAll: false }
                if (read.id === 'int-a' && holdA) return new Promise<ManagementResult>((resolve) => (releaseA = resolve)).then(() => ok({ kind: 'models', models }))
                return ok({ kind: 'models', models })
            }
            return standard(request)
        })
        const user = await open(native)
        await user.click(await screen.findByRole('tab', { name: 'Integrations' }))
        // Models read for A, then B opened without closing A.
        await user.click(await screen.findByRole('button', { name: 'Details for Integration A' }))
        await user.click(await screen.findByRole('button', { name: 'Read models' }))
        expect(await screen.findByText('model-of-int-a · enabled')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Details for Integration B' }))
        await screen.findByRole('heading', { level: 2, name: 'Integration B' })
        expect(screen.queryByText('model-of-int-a · enabled')).toBeNull()
        expect(screen.queryByText('Reading models…')).toBeNull()
        // A's read pending when B is opened: its late reply is discarded.
        await user.click(screen.getByRole('button', { name: 'Details for Integration A' }))
        await screen.findByRole('heading', { level: 2, name: 'Integration A' })
        holdA = true
        await user.click(await screen.findByRole('button', { name: 'Read models' }))
        await screen.findByText('Reading models…')
        await user.click(screen.getByRole('button', { name: 'Details for Integration B' }))
        await screen.findByRole('heading', { level: 2, name: 'Integration B' })
        releaseA({ kind: 'failed', failure: 'invalidResponse', outcome: 'completed' })
        await new Promise((resolve) => setTimeout(resolve, 20))
        expect(screen.queryByText('model-of-int-a · enabled')).toBeNull()
        expect(screen.queryByText('Reading models…')).toBeNull()
        // Reading B shows only B's.
        await user.click(screen.getByRole('button', { name: 'Read models' }))
        expect(await screen.findByText('model-of-int-b · enabled')).toBeInTheDocument()
        expect(screen.queryByText('model-of-int-a · enabled')).toBeNull()
    })

    it('reviews a change with the selection reference and shows its scope and what blocks it', async () => {
        const native = scriptedNative(ready(listed(A)))
        gateway(native, standard)
        const user = await open(native)
        await user.click(await screen.findByRole('button', { name: 'Details for openai-prod' }))
        const remove = await screen.findByRole('button', { name: 'Delete openai-prod' })
        await user.click(remove)
        // The workbench takes focus; Review sends the native reference.
        await waitFor(() => expect(screen.getByRole('heading', { level: 2, name: 'Delete provider openai-prod' })).toHaveFocus())
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'reviewChange',
                context: CONTEXT,
                draft: { kind: 'gatewayProviders', action: 'delete', target: 'p1', body: '{}', workspace: REF_A },
            })
        )
        const review: ChangeReviewView = {
            review: 'rv-0011223344556677',
            kind: 'gatewayProviders',
            action: 'delete',
            target: 'p1',
            state: 'ready',
            failure: null,
            before: JSON.stringify({ id: 'p1', name: 'openai-prod' }),
            after: null,
            sourceKey: null,
            impact: 'Deletes this item from the workspace.',
            confirmation: 'openai-prod',
            workspace: A,
            references: ['Route: default-route'],
        }
        await native.push({ ...native.current, changes: { review, staged: null, results: [] } })
        expect(await screen.findByText(/Applies to workspace/)).toHaveTextContent('Applies to workspace Team A (team-a), IAM scope scope-team-a.')
        expect(screen.getByText('Route: default-route')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Apply…' })).toBeDisabled()
        // Without references it can be applied, with the same reference.
        await native.push({ ...native.current, changes: { review: { ...review, references: [] }, staged: null, results: [] } })
        await waitFor(() => expect(screen.getByRole('button', { name: 'Apply…' })).toBeEnabled())
        await user.type(screen.getByLabelText('Type openai-prod to confirm'), 'openai-prod')
        await user.click(screen.getByRole('button', { name: 'Apply…' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'requestApply',
                review: review.review,
                draft: { kind: 'gatewayProviders', action: 'delete', target: 'p1', body: '{}', workspace: REF_A },
                confirmation: 'openai-prod',
            })
        )
        await user.click(screen.getByRole('button', { name: 'Close' }))
        expect(remove).toHaveFocus()
    })

    it('reports provisioning step by step and offers to stop waiting without claiming a rollback', async () => {
        const native = scriptedNative(ready(listed(A)))
        gateway(native, standard)
        const user = await open(native)
        await user.click(screen.getByRole('button', { name: 'Provision workspace…' }))
        expect(screen.getByRole('heading', { level: 2, name: 'Provision a workspace' })).toHaveFocus()
        const pending: ChangeResultView = {
            operation: 'op-7',
            kind: 'gatewayWorkspaces',
            action: 'create',
            target: '',
            name: 'Team C',
            settled: false,
            outcome: null,
            failure: null,
            response: null,
            readback: null,
            current: null,
            secret: 'none',
            workspace: null,
            provisioning: null,
        }
        await native.push({ ...native.current, changes: { review: null, staged: null, results: [pending] } })
        await user.click(await screen.findByRole('button', { name: 'Stop waiting' }))
        expect(native.dispatched).toContainEqual({ kind: 'cancelChange', operation: 'op-7' })
        expect(screen.getByText(/Stopping does not undo anything already sent/)).toBeInTheDocument()
        await native.push({
            ...native.current,
            changes: {
                review: null,
                staged: null,
                results: [
                    {
                        ...pending,
                        settled: true,
                        outcome: 'unknown',
                        failure: { kind: 'service', failure: 'cancelled' },
                        readback: 'found',
                        provisioning: {
                            scopeName: 'scope-team-c',
                            scope: 'completed',
                            workspace: 'unknown',
                            binding: 'notStarted',
                            workspaceId: 'ws-c',
                            workspaceSlug: 'team-c',
                            cleanup: 'unknown',
                            scopeRead: 'found',
                            boundRead: false,
                            workspaceRead: 'found',
                        },
                    },
                ],
            },
        })
        const steps = await screen.findByText('IAM scope scope-team-c')
        expect(steps.nextElementSibling).toHaveTextContent('done')
        expect(screen.getByText('Workspace', { selector: 'dt' }).nextElementSibling).toHaveTextContent('outcome unknown')
        expect(screen.getAllByText(/Whether the IAM scope this attempt created was deleted is not known/).length).toBeGreaterThan(0)
        expect(screen.queryByText(/Nothing was undone/)).toBeNull()
        expect(screen.getByText(/workspace found \(team-c\); the scope does not list it/)).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Stop waiting' })).toBeNull()
    })

    it('calls a provisioning verified only when reading the tenant back established it', async () => {
        const base: Provisioning = {
            scopeName: 'scope-team-c',
            scope: 'completed',
            workspace: 'completed',
            binding: 'completed',
            cleanup: 'notNeeded',
            workspaceId: 'ws-c',
            workspaceSlug: 'team-c',
            scopeRead: 'found',
            boundRead: true,
            workspaceRead: 'found',
        }
        const cases: [Partial<Provisioning>, RegExp][] = [
            [{}, /^Provisioned\. The workspace, its IAM scope and the scope’s binding to it were read back\.$/],
            [{ workspaceRead: 'failed', boundRead: null }, /did not confirm the workspace\./],
            [{ scopeRead: 'failed', boundRead: null }, /did not confirm the IAM scope\./],
            [{ boundRead: false }, /did not confirm the scope’s binding to the workspace\./],
            [{ workspaceRead: 'ambiguous', workspaceId: null, workspaceSlug: null, boundRead: null }, /did not confirm which workspace is this attempt’s/],
            [{ workspaceRead: 'incomplete', boundRead: null }, /did not confirm the workspace\./],
            [{ scope: 'deleted', workspace: 'rejected', binding: 'notStarted', cleanup: 'deleted' }, /^Not provisioned\. The IAM scope this attempt created was deleted again\.$/],
            [{ scope: 'completed', workspace: 'rejected', binding: 'notStarted', cleanup: 'failed' }, /^Not provisioned\. Deleting the IAM scope this attempt created was refused, so it remains\.$/],
            [{ scope: 'unknown', workspace: 'rejected', binding: 'notStarted', cleanup: 'unknown' }, /^Not every step’s outcome is known\. Whether the IAM scope this attempt created was deleted is not known\./],
            [{ binding: 'rejected', cleanup: 'retained', boundRead: false }, /^Partially completed: the workspace exists but a later step failed\. The IAM scope was kept/],
        ]
        const native = scriptedNative(ready(listed(A)))
        gateway(native, standard)
        await open(native)
        for (const [i, [overrides, expected]] of cases.entries()) {
            const result: ChangeResultView = {
                operation: `op-${80 + i}`,
                kind: 'gatewayWorkspaces',
                action: 'create',
                target: '',
                name: 'Team C',
                settled: true,
                outcome: 'completed',
                failure: null,
                response: null,
                readback: 'found',
                current: null,
                secret: 'none',
                workspace: null,
                provisioning: { ...base, ...overrides },
            }
            await native.push({ ...native.current, changes: { review: null, staged: null, results: [result] } })
            const changes = (await screen.findByRole('heading', { level: 2, name: 'Gateway changes' })).closest('section') as HTMLElement
            const status = within(changes).getAllByRole('status')[0] as HTMLElement
            expect(status.textContent).toMatch(expected)
            if (overrides.workspaceRead || overrides.scopeRead || overrides.boundRead === false)
                expect(status.textContent).not.toMatch(/were read back/)
        }
    })

    it('keeps drafts and uncertain results on this Mac, readable offline and reviewed again only when asked', async () => {
        const local: GatewayLocalView = {
            state: 'ready',
            failure: null,
            drafts: [
                {
                    id: `gd-${'1'.repeat(24)}`,
                    workspace: A,
                    kind: 'gatewayProviders',
                    action: 'update',
                    target: 'p1',
                    body: '{"note": "kept"}',
                    savedAt: '2026-10-20T10:00:00Z',
                },
            ],
            records: [
                {
                    id: `gr-${'2'.repeat(24)}`,
                    workspace: null,
                    kind: 'gatewayWorkspaces',
                    action: 'create',
                    target: '',
                    name: null,
                    body: '{"name": "Team C", "scope_name": "scope-team-c"}',
                    outcome: 'unknown',
                    readback: 'found',
                    provisioning: {
                        scopeName: 'scope-team-c',
                        scope: 'completed',
                        workspace: 'unknown',
                        binding: 'notStarted',
                        cleanup: 'retained',
                        workspaceId: null,
                        workspaceSlug: null,
                        scopeRead: 'found',
                        boundRead: null,
                        workspaceRead: 'failed',
                    },
                    settledAt: '2026-10-20T09:00:00Z',
                },
            ],
            playground: [],
            runs: [],
            files: [],
            problem: null,
            unsaved: false,
        }
        // Offline: the workspaces cannot be read.
        const native = scriptedNative(ready(view({ local: { ...local, state: 'idle', drafts: [], records: [] } })))
        gateway(native, () => ({ kind: 'failed', failure: 'network', outcome: 'notSent' }))
        const user = await open(native)
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'openGatewayLocal', context: CONTEXT }))
        await native.push({ ...native.current, gateway: view({ local }) })
        const kept = await screen.findByRole('region', { name: 'Kept on this Mac' })
        expect(within(kept).getByText(/Its outcome was not known when it settled/)).toBeInTheDocument()
        expect(within(kept).getByText(/The service has not been read since/)).toBeInTheDocument()
        expect(within(kept).getByText('IAM scope scope-team-c').nextElementSibling).toHaveTextContent('The IAM scope was kept')
        // The draft opens for editing; it cannot be reviewed until its
        // workspace is chosen from a fresh listing.
        const openDraft = within(kept).getByRole('button', { name: 'Open draft: Update provider p1 in Team A' })
        await user.click(openDraft)
        expect(screen.getByRole('heading', { level: 2, name: 'Edit provider p1' })).toHaveFocus()
        const editor = screen.getByRole('textbox', { name: 'provider draft (JSON)' })
        expect(editor).toHaveValue('{"note": "kept"}')
        expect(screen.getByText('This draft is for workspace Team A. Choose that workspace above to review it.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Review change' })).toBeDisabled()
        // Edits are kept on this Mac against the same draft (offline too).
        await user.clear(editor)
        await user.type(editor, '{{"note": "edited"}')
        await waitFor(
            () =>
                expect(native.dispatched).toContainEqual({
                    kind: 'saveGatewayDraft',
                    context: CONTEXT,
                    draft: { id: `gd-${'1'.repeat(24)}`, kind: 'gatewayProviders', action: 'update', target: 'p1', body: '{"note": "edited"}' },
                }),
            { timeout: 3000 }
        )
        expect(native.dispatched.some((i) => i.kind === 'reviewChange' || i.kind === 'requestApply')).toBe(false)
        // Text that is not complete yet is not kept, and the note says so.
        await native.push({ ...native.current, gateway: view({ local: { ...local, problem: 'incomplete' } }) })
        expect(await screen.findByText(/Only complete JSON is kept on this Mac, so these edits are not kept yet/)).toBeInTheDocument()
        expect(screen.getByRole('textbox', { name: 'provider draft (JSON)' })).toHaveValue('{"note": "edited"}')
        // Back online with Team A chosen: the review carries the reference
        // chosen now.
        await native.push({ ...native.current, gateway: { ...listed(A), local } })
        await waitFor(() => expect(screen.getByRole('button', { name: 'Review change' })).toBeEnabled())
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        await waitFor(() =>
            expect(native.dispatched).toContainEqual({
                kind: 'reviewChange',
                context: CONTEXT,
                draft: { kind: 'gatewayProviders', action: 'update', target: 'p1', body: '{"note": "edited"}', workspace: REF_A },
            })
        )
        await user.click(screen.getByRole('button', { name: 'Close' }))
        expect(screen.getByRole('button', { name: 'Open draft: Update provider p1 in Team A' })).toHaveFocus()
        // An uncertain provisioning is reviewed again from its text, as a new change.
        await user.click(screen.getByRole('button', { name: 'Open to retry: Provision workspace' }))
        expect(screen.getByRole('textbox', { name: 'workspace draft (JSON)' })).toHaveValue('{"name": "Team C", "scope_name": "scope-team-c"}')
        await user.click(screen.getByRole('button', { name: 'Close' }))
        await user.click(screen.getByRole('button', { name: 'Dismiss record: Provision workspace' }))
        expect(native.dispatched).toContainEqual({ kind: 'discardGatewayLocal', context: CONTEXT, id: `gr-${'2'.repeat(24)}` })
        await user.click(screen.getByRole('button', { name: 'Discard draft: Update provider p1 in Team A' }))
        expect(native.dispatched).toContainEqual({ kind: 'discardGatewayLocal', context: CONTEXT, id: `gd-${'1'.repeat(24)}` })
    })

    it('says a change stopped before its write or by a workspace switch sent nothing, and one stopped while reading back was applied', async () => {
        const native = scriptedNative(ready(listed(A)))
        gateway(native, standard)
        await open(native)
        const result = (operation: string, extra: Partial<ChangeResultView>): ChangeResultView => ({
            operation,
            kind: 'gatewayServiceKeys',
            action: 'create',
            target: '',
            name: 'deploy-key',
            settled: true,
            outcome: 'completed',
            failure: null,
            response: null,
            readback: 'found',
            current: null,
            secret: 'none',
            workspace: A,
            provisioning: null,
            ...extra,
        })
        await native.push({
            ...native.current,
            changes: {
                review: null,
                staged: null,
                results: [
                    result('op-91', { readback: 'stopped', secret: 'held' }),
                    result('op-92', { outcome: 'notSent', failure: { kind: 'workspaceChanged' }, readback: null }),
                ],
            },
        })
        const region = (await screen.findByRole('heading', { level: 2, name: 'Gateway changes' })).closest('section') as HTMLElement
        expect(within(region).getByText(/Applied: the service accepted the change\. The desktop stopped waiting before reading it back/)).toBeInTheDocument()
        expect(within(region).getByRole('button', { name: 'Show new key…' })).toBeInTheDocument()
        expect(within(region).getByText(/Another workspace was chosen before this change was sent, so nothing was sent\..*Nothing was changed\./)).toBeInTheDocument()
    })
})
