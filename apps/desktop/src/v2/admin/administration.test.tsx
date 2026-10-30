/**
 * Administration, reviews and key rotation in the v2 shell (C31) over the
 * scripted native transport: the accordion renders the core's
 * `AdminInventory` (V2-T19 unavailable products and tabs, V2-T20 exactly the
 * admitted actions with per-tab labels, Gateway keys rotate in place), a
 * review applies only once the typed name matches (V2-T03), a rotation
 * follows the core's stages with a host-only reveal, no secret controls once
 * resumed, a forgotten key on every exit (V2-T06, I6), and two labelled
 * timestamps (V2-T08). Topic results are keyed apart from detectors (gap 2).
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import {
    DesktopClient,
    type ChangeResultView,
    type ChangeReviewView,
    type Intent,
    type ManagementRequest,
    type ManagementResult,
    type RedTeamScan,
    type RotationView,
    type ScanRecordView,
    type ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { scriptedNative, tenant, type Native } from '../../test/native'
import { fxSwitcher } from '../fixtures'
import { resetShellStore, useShellStore } from '../store'
import INVENTORY from './inventory.fixture.json'
import { appliedState as appliedStateOf } from './review-sheet'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const KEY = {
    api_key_id: 'key-1',
    api_key_name: 'Support key',
    cust_app: 'support-copilot',
    expiration: '2027-01-01T00:00:00Z',
    revoked: false,
}
const TOPIC = {
    topic_id: '11111111-1111-4111-8111-111111111111',
    topic_name: 'Finance',
    description: 'Money talk',
    revision: 1,
}

function fxAdmin(over: Partial<ShellSnapshot> = {}): ShellSnapshot {
    const base = fxSwitcher()
    const nw = tenant({
        id: 'tn-0123456789abcdef',
        label: 'Northwind Health',
        tsgId: '1762204418',
        scan: { state: 'ready', mode: 'keychain', failure: null },
        management: {
            state: 'ready',
            mode: 'keychain',
            failure: null,
            clientId: 'svc@1762204418.iam.panserviceaccount.com',
            tokenOrigin: 'https://auth.apps.paloaltonetworks.com',
        },
    })
    return {
        ...base,
        tenants: [nw, base.tenants[1]!],
        active: { tenant: nw.id, context: CONTEXT },
        activeFeature: 'administration',
        ...over,
    }
}

const PROFILE = {
    profile_id: '22222222-2222-4222-8222-222222222222',
    profile_name: 'Production guardrails',
    revision: 3,
    policy: {
        'ai-security-profiles': [
            {
                'model-configuration': {
                    'topic-guardrails': { 'topic-list': [{ action: 'block', topic: [{ topic_id: TOPIC.topic_id, topic_name: 'Finance', revision: 1 }] }] },
                },
            },
        ],
    },
}
const APP = { customer_appId: 'app-1', app_name: 'support-copilot', cloud_provider: 'aws', environment: 'production' }
const DEPLOYMENT = { dp_name: 'dp-1', status: 'active' }
const DLP_ITEM = { id: 'dlp-1', name: 'Card numbers' }
const WS = { id: '16f7e90d-382a-4e78-b577-1b01eb5f8297', name: 'Team A', slug: 'team-a', scopeName: 'scope-team-a' }
const GATEWAY: ShellSnapshot['gateway'] = {
    workspaces: [WS],
    complete: true,
    selected: { reference: 'gw-aaaaaaaaaaaaaaaaaaaaaaaa', workspace: WS, restored: false },
    remembered: null,
    rememberedMissing: false,
    local: { state: 'ready', failure: null, drafts: [], records: [], playground: [], runs: [], files: [], problem: null, unsaved: false },
    playground: null,
    files: null,
}
/** A Red Team scan row as a reply might carry it, with a target name the core never admits (V2-T21). */
const RT_SCAN = {
    id: '9f1c2a7e-0000-4000-8000-000000000001',
    status: 'COMPLETED',
    jobType: 'STATIC',
    targetId: '7a7a7a7a-0000-4000-8000-000000000002',
    targetType: 'APPLICATION',
    createdAt: '2026-10-01T10:00:00Z',
    targetName: 'Payments assistant (prod)',
}

/** The tenant's inventories as the scripted service holds them; tests may change them. */
interface Inventories {
    keys: Record<string, unknown>[]
    topics: Record<string, unknown>[]
    /** Detail reads answer absent for these IDs. */
    absent: string[]
}

const ID_FIELD: Record<string, string> = {
    profiles: 'profile_id',
    topics: 'topic_id',
    applications: 'customer_appId',
    apiKeys: 'api_key_id',
    deploymentProfiles: 'dp_name',
}

/** Admits management reads and answers pages, details, Red Team, model and Gateway reads from small inventories. */
function service(native: Native, data: Inventories = { keys: [KEY], topics: [TOPIC], absent: [] }) {
    const pending = new Map<string, ManagementRequest>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind !== 'management') return respond(intent)
        const operation = `op-${pending.size + 101}`
        pending.set(operation, intent.request)
        return {
            outcome: 'receipt',
            receipt: { kind: 'admitted', request: `rq-${pending.size}`, operation },
        }
    }
    const itemsOf = (resource: string): Record<string, unknown>[] =>
        resource === 'apiKeys'
            ? data.keys
            : resource === 'topics'
              ? data.topics
              : resource === 'profiles'
                ? [PROFILE]
                : resource === 'applications'
                  ? [APP]
                  : resource === 'deploymentProfiles'
                    ? [DEPLOYMENT]
                    : [DLP_ITEM]
    const answer = (result: ManagementResult) => ({ kind: 'management' as const, result })
    native.observeManagement = (operation) => {
        const request = pending.get(operation)
        if (request?.kind === 'page') {
            const items = itemsOf(request.resource)
            return answer({
                kind: 'page',
                page: { resource: request.resource, offset: 0, limit: request.limit, items: JSON.stringify(items), count: items.length, nextOffset: null, complete: true, total: items.length },
            })
        }
        if (request?.kind === 'detail') {
            const field = ID_FIELD[request.resource] ?? 'id'
            const item = data.absent.includes(request.id) ? undefined : itemsOf(request.resource).find((i) => i[field] === request.id)
            return answer({ kind: 'detail', detail: { resource: request.resource, id: request.id, found: item ? 'found' : 'absent', item: item ? JSON.stringify(item) : null } })
        }
        if (request?.kind === 'redTeam') {
            return answer({
                kind: 'redTeam',
                observation: {
                    readAt: 1_761_490_000,
                    window: request.window,
                    statistics: { scans: 1, targets: 1, status: [{ label: 'COMPLETED', count: 1 }], risk: null, targetTypes: [{ label: 'APPLICATION', count: 1 }], noScans: false },
                    statisticsFailure: null,
                    // The extra field stands for anything a reply carries beyond the contract.
                    scans: { rows: [RT_SCAN as unknown as RedTeamScan], total: 1, capped: false, complete: true },
                    scansFailure: null,
                },
            })
        }
        if (request?.kind === 'supplyChain') {
            return answer({
                kind: 'supplyChain',
                observation: {
                    readAt: 1_761_490_000,
                    models: { rows: [{ id: '5b5b5b5b-0000-4000-8000-000000000003', outcome: 'BLOCKED', createdAt: '2026-10-02' }], total: 1, capped: false, complete: true },
                    modelsFailure: null,
                    skillStatistics: { uniqueSkills: 1, vulnerabilities: 2 },
                    skillStatisticsFailure: null,
                    skills: { rows: [{ pseudonym: 'a1b2c3d4', status: 'COMPLETED', outcome: null, vulnerabilities: 2, createdAt: '2026-10-03' }], total: 1, capped: false, complete: true },
                    skillsFailure: null,
                },
            })
        }
        if (request?.kind === 'gateway') {
            const read = request.read
            if (read.kind === 'inventory') {
                const rows = [{ id: `${read.resource}-1`, name: `${read.resource}-one` }]
                return answer({
                    kind: 'gateway',
                    gateway: { kind: 'list', list: { resource: read.resource, workspace: read.resource === 'workspaces' || read.resource === 'integrations' ? null : WS.id, rows: JSON.stringify(rows), count: 1, total: 1, complete: true } },
                })
            }
            if (read.kind === 'detail') {
                const item = read.resource === 'workspaces' ? { id: WS.id, name: WS.name } : { id: read.id, name: `${read.resource}-one` }
                return answer({ kind: 'gateway', gateway: { kind: 'detail', detail: { resource: read.resource, id: read.id, found: 'found', item: JSON.stringify(item) } } })
            }
        }
        return answer({ kind: 'failed', failure: 'unavailable', outcome: 'completed' })
    }
}

async function launch(snapshot: ShellSnapshot, data?: Inventories) {
    const native = scriptedNative(snapshot)
    service(native, data)
    const user = userEvent.setup()
    const { unmount } = render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('heading', { level: 1, name: 'Administration' })
    return { native, user, unmount }
}

const product = (id: string) => document.querySelector(`[data-product="${id}"]`) as HTMLElement
/** The Administration tab panel of a product (embedded views keep their own inner panels). */
const panelOf = (el: HTMLElement) => el.querySelector('[data-slot="admin-tab"]') as HTMLElement

function review(over: Partial<ChangeReviewView> = {}): ChangeReviewView {
    return {
        review: 'rv-0011223344556677',
        kind: 'topics',
        action: 'update',
        target: TOPIC.topic_id,
        state: 'ready',
        failure: null,
        before: JSON.stringify(TOPIC),
        after: JSON.stringify({ ...TOPIC, description: 'Money and payments' }),
        sourceKey: null,
        impact: 'Applies only to this tenant.',
        confirmation: 'Finance',
        workspace: null,
        references: [],
        ...over,
    }
}

function rotation(over: Partial<RotationView> = {}): RotationView {
    return {
        application: 'support-copilot',
        oldKey: { id: 'key-1', name: 'Support key' },
        newKey: { id: 'key-2', name: 'Support key 2' },
        stage: 'resumed',
        issuedBy: null,
        issuedAt: 1_761_490_000,
        deploymentAcknowledgedAt: null,
        oldKeyRemovedAt: null,
        removalRejected: null,
        unsaved: false,
        ...over,
    }
}

beforeEach(() => resetShellStore())

describe('v2 Administration', () => {
    it('renders the inventory: unavailable products show their gate and nothing else, removed tabs do not exist, unavailable tabs have no rows or actions (V2-T19, V2-T20)', async () => {
        const { user } = await launch(fxAdmin())
        const products = Array.from(document.querySelectorAll('[data-slot="admin-product"]')).map((p) => p.getAttribute('data-product'))
        expect(products).toEqual(['runtimeSecurity', 'gateway', 'redTeam', 'modelSkill', 'agenticIdentity', 'agenticEndpoint'])
        for (const id of ['agenticIdentity', 'agenticEndpoint']) {
            const p = product(id)
            expect(within(p).getByText(/Not available in this release · needs an admitted/)).toBeInTheDocument()
            expect(within(p).queryByRole('tablist')).toBeNull()
            expect(within(p).getByRole('button', { name: /Agentic/ })).toHaveAttribute('aria-disabled', 'true')
        }
        // Gateway: no Limits tab (removed); keys rotate in place.
        await user.click(within(product('gateway')).getByRole('button', { name: /AI Gateway/ }))
        const gatewayTabs = within(product('gateway'))
            .getAllByRole('tab')
            .map((t) => t.textContent)
        expect(gatewayTabs).toEqual(['Overview', 'Workspaces', 'Routes', 'Providers', 'Guardrails', 'Keys', 'Integrations', 'Logs', 'MCP servers'])
        // Model and skill scans: Codebase scans is unavailable, with its gate and no rows or actions.
        await user.click(
            within(product('modelSkill')).getByRole('button', {
                name: /Model and skill scans/,
            })
        )
        await user.click(
            within(product('modelSkill')).getByRole('tab', {
                name: 'Codebase scans',
            })
        )
        const panel = panelOf(product('modelSkill'))
        expect(panel).toHaveAttribute('data-disposition', 'unavailable')
        expect(within(panel).getByText('needs an admitted codebase-scan read')).toBeInTheDocument()
        expect(within(panel).queryByRole('button')).toBeNull()
        expect(within(panel).queryByRole('table')).toBeNull()
        // Read-only products say so.
        expect(within(product('modelSkill')).getByText('Read-only')).toBeInTheDocument()
    })

    it('labels creation and row actions per tab from the inventory: no "New application…", no key "Edit…" (V2-T20, gap 3)', async () => {
        const { user } = await launch(fxAdmin())
        const runtime = product('runtimeSecurity')
        expect(within(runtime).getByRole('tab', { name: 'Custom topics' })).toHaveAttribute('aria-selected', 'true')
        expect(await within(runtime).findByRole('button', { name: 'New topic…' })).toBeInTheDocument()
        await user.click(within(runtime).getByRole('tab', { name: 'Applications' }))
        await waitFor(() => expect(within(runtime).getByRole('tabpanel')).toHaveAttribute('data-tab', 'applications'))
        expect(within(runtime).queryByRole('button', { name: /^New/ })).toBeNull()
        await user.click(within(runtime).getByRole('tab', { name: 'API keys' }))
        expect(await within(runtime).findByRole('button', { name: 'New API key…' })).toBeInTheDocument()
        await user.click(
            await within(runtime).findByRole('button', {
                name: 'Details for Support key',
            })
        )
        const detail = await screen.findByRole('heading', {
            level: 2,
            name: 'Support key',
        })
        const actions = within(detail.closest('section') as HTMLElement)
            .getAllByRole('button')
            .map((b) => b.textContent)
        expect(actions).toEqual(expect.arrayContaining(['Issue additional key…', 'Regenerate…', 'Remove…']))
        expect(actions).not.toContain('Edit…')
    })

    it('applies a review only when the typed name matches, in the review sheet (V2-T03)', async () => {
        const { native, user } = await launch(fxAdmin())
        const runtime = product('runtimeSecurity')
        await user.click(
            await within(runtime).findByRole('button', {
                name: 'Details for Finance',
            })
        )
        await user.click(await screen.findByRole('button', { name: 'Edit…' }))
        const sheet = await screen.findByRole('dialog', {
            name: 'Review a change',
        })
        await user.click(within(sheet).getByRole('button', { name: 'Review change' }))
        expect(native.dispatched.at(-1)).toMatchObject({
            kind: 'reviewChange',
            context: CONTEXT,
            draft: { kind: 'topics', action: 'update', target: TOPIC.topic_id },
        })
        await native.push({
            ...native.current,
            changes: { review: review(), staged: null, results: [] },
        })
        const apply = await within(sheet).findByRole('button', { name: 'Apply…' })
        expect(within(sheet).getByText(/Type Finance to confirm/)).toBeInTheDocument()
        expect(apply).toBeDisabled()
        await user.type(within(sheet).getByRole('textbox', { name: /Type Finance to confirm/ }), 'finance')
        expect(apply).toBeDisabled()
        await user.clear(within(sheet).getByRole('textbox', { name: /Type Finance to confirm/ }))
        await user.type(within(sheet).getByRole('textbox', { name: /Type Finance to confirm/ }), 'Finance')
        expect(apply).toBeEnabled()
        await user.click(apply)
        expect(native.dispatched.at(-1)).toMatchObject({
            kind: 'requestApply',
            review: 'rv-0011223344556677',
            confirmation: 'Finance',
        })
        // The applied panel says only what the readback established.
        const applied: ChangeResultView = {
            operation: 'op-9',
            kind: 'topics',
            action: 'update',
            target: TOPIC.topic_id,
            name: 'Finance',
            settled: true,
            outcome: 'completed',
            failure: null,
            response: null,
            readback: 'found',
            current: JSON.stringify(TOPIC),
            secret: 'none',
            workspace: null,
            provisioning: null,
        }
        await native.push({
            ...native.current,
            changes: { review: null, staged: null, results: [applied] },
        })
        expect(await within(sheet).findByText('Live. The item was read back.')).toBeInTheDocument()
        // An unknown outcome is never a success: it offers reconciliation.
        await native.push({
            ...native.current,
            changes: {
                review: null,
                staged: null,
                results: [
                    {
                        ...applied,
                        operation: 'op-10',
                        outcome: 'unknown',
                        readback: null,
                        current: null,
                    },
                    applied,
                ],
            },
        })
    })

    it('follows a rotation: Issued reveals only through the host and forgets the key on exit, Resumed has no secret controls, Complete shows both labelled timestamps (V2-T06, V2-T08, I6)', async () => {
        const held: ChangeResultView = {
            operation: 'op-7',
            kind: 'apiKeys',
            action: 'version',
            target: 'key-1',
            name: 'Support key 2',
            settled: true,
            outcome: 'completed',
            failure: null,
            response: null,
            readback: 'found',
            current: null,
            secret: 'held',
            workspace: null,
            provisioning: null,
        }
        const { native, user } = await launch(
            fxAdmin({
                rotations: [rotation({ stage: 'issued', issuedBy: 'op-7' })],
                changes: { review: null, staged: null, results: [held] },
            })
        )
        const runtime = product('runtimeSecurity')
        await user.click(within(runtime).getByRole('tab', { name: 'API keys' }))
        await user.click(
            await within(runtime).findByRole('button', {
                name: 'Details for Support key',
            })
        )
        await user.click(await screen.findByRole('button', { name: 'Finish rotation…' }))
        let sheet = await screen.findByRole('dialog', {
            name: 'Rotate the key of support-copilot',
        })
        expect(within(sheet).getByText('Additional key issued · old key still active')).toBeInTheDocument()
        expect(within(sheet).getByText(/Shown once\. Not stored by this app/)).toBeInTheDocument()
        await user.click(within(sheet).getByRole('button', { name: 'Reveal new key…' }))
        expect(native.dispatched.at(-1)).toEqual({
            kind: 'revealSecret',
            operation: 'op-7',
        })
        // Escape is a review exit: the held key is forgotten (I6).
        await user.keyboard('{Escape}')
        await waitFor(() => expect(screen.queryByRole('dialog', { name: /Rotate the key/ })).toBeNull())
        expect(native.dispatched).toContainEqual({
            kind: 'forgetSecret',
            operation: 'op-7',
        })
        // Resumed: no secret controls, issue another or say the application was updated.
        await native.push({
            ...native.current,
            rotations: [rotation()],
            changes: {
                review: null,
                staged: null,
                results: [{ ...held, secret: 'forgotten' }],
            },
        })
        await user.click(await screen.findByRole('button', { name: 'Finish rotation…' }))
        sheet = await screen.findByRole('dialog', {
            name: 'Rotate the key of support-copilot',
        })
        expect(within(sheet).getByText('Rotation in progress · secret no longer available')).toBeInTheDocument()
        expect(within(sheet).queryByRole('button', { name: /Reveal/ })).toBeNull()
        await user.click(within(sheet).getByRole('button', { name: 'Application updated' }))
        expect(native.dispatched).toContainEqual({
            kind: 'acknowledgeDeployment',
            context: CONTEXT,
            application: 'support-copilot',
        })
        // Once the core records the acknowledgement, the old key's removal opens in the same sheet.
        await native.push({ ...native.current, rotations: [rotation({ stage: 'acknowledged', deploymentAcknowledgedAt: 1_761_490_300 })] })
        expect(
            await within(sheet).findByRole('heading', {
                level: 2,
                name: 'Remove Support key',
            })
        ).toBeInTheDocument()
        // Complete: both timestamps, labelled distinctly.
        await native.push({
            ...native.current,
            rotations: [
                rotation({
                    stage: 'complete',
                    deploymentAcknowledgedAt: 1_761_490_300,
                    oldKeyRemovedAt: 1_761_490_420,
                }),
            ],
        })
        expect(await within(sheet).findByText(/Rotation complete · old key removed \d\d:\d\d/)).toBeInTheDocument()
        const stamps = within(sheet).getByRole('table')
        expect(
            within(stamps).getByRole('rowheader', {
                name: 'Deployment acknowledged (your statement)',
            })
        ).toBeInTheDocument()
        expect(
            within(stamps).getByRole('rowheader', {
                name: 'Old key removed (read back)',
            })
        ).toBeInTheDocument()
    })

    it('Gateway keys rotate in place: never "Issue additional key…" or "Finish rotation…" (V2-T20)', async () => {
        const { user } = await launch(fxAdmin())
        await user.click(within(product('gateway')).getByRole('button', { name: /AI Gateway/ }))
        await user.click(within(product('gateway')).getByRole('tab', { name: 'Keys' }))
        const gateway = product('gateway')
        expect(within(gateway).queryByRole('button', { name: /Issue additional key/ })).toBeNull()
        expect(within(gateway).queryByRole('button', { name: /Finish rotation/ })).toBeNull()
    })

    it('shows custom topic results apart from the detectors: an applied topic flags without suppressing an enabled detector (gap 2)', async () => {
        const scan: ScanRecordView = {
            operation: 'op-3',
            progress: 'settled',
            outcome: 'completed',
            stop: null,
            failure: null,
            discarded: false,
            result: {
                scanId: 's-1',
                reportId: 'r-1',
                trId: null,
                profileName: 'chatbot-strict',
                profileId: null,
                category: 'malicious',
                action: 'block',
                verdict: 'block',
                timeout: false,
                serviceError: false,
                contentErrors: [],
                promptDetected: [
                    { detector: 'injection', detected: true },
                    { detector: 'topic_violation', detected: true },
                ],
                responseDetected: [],
                topics: {
                    violation: true,
                    blocked: ['injection-override'],
                    allowed: [],
                },
                responseTopics: { violation: null, blocked: [], allowed: [] },
            },
        }
        const { user } = await launch(fxAdmin({ scans: [scan] }))
        await user.click(
            within(product('runtimeSecurity')).getByRole('tab', {
                name: 'Playground',
            })
        )
        const topics = await screen.findByText('injection-override: flagged (blocked)')
        expect(topics.closest('[data-slot="scan-topics"]')).toHaveAttribute('data-topic-verdict', 'flagged')
        expect(screen.getByText('injection: detected')).toBeInTheDocument()
        expect(screen.queryByText(/topic_violation/)).toBeNull()
    })

    it('V2-T20: every product and tab renders its disposition, its content, and exactly the admitted actions', async () => {
        const { user } = await launch(fxAdmin({ gateway: GATEWAY }))
        type Tab = (typeof INVENTORY.products)[number]['tabs'][number]
        // The kind a tab shows first (DLP opens on data patterns, Gateway keys on service keys).
        const kindOf = (productId: string, tab: Tab) =>
            productId === 'gateway' ? tab.actions[0]?.kind : tab.label === 'DLP' ? 'dataPatterns' : (tab.resource ?? tab.actions[0]?.kind)
        const creating = (b: HTMLElement) => /^(New |Provision )/.test(b.textContent ?? '')
        for (const p of INVENTORY.products) {
            const el = product(p.id)
            if (p.availability.state === 'unavailable') {
                expect(within(el).queryByRole('tablist')).toBeNull()
                expect(within(el).queryByRole('table')).toBeNull()
                continue
            }
            const header = within(el).getByRole('button', { name: new RegExp(p.label) })
            if (header.getAttribute('aria-expanded') !== 'true') await user.click(header)
            const shown = p.tabs.filter((t) => t.disposition.kind !== 'removed')
            expect(within(el).getAllByRole('tab').map((t) => t.textContent)).toEqual(shown.map((t) => t.label))
            for (const t of shown) {
                await user.click(within(el).getByRole('tab', { name: t.label }))
                await waitFor(() => expect(panelOf(el)).toHaveAttribute('data-tab', t.id))
                const panel = panelOf(el)
                if (t.disposition.kind === 'unavailable') {
                    expect(within(panel).getByText('Not available in this release')).toBeInTheDocument()
                    expect(within(panel).queryByRole('button')).toBeNull()
                    expect(within(panel).queryByRole('table')).toBeNull()
                    continue
                }
                const kind = kindOf(p.id, t)
                const creates = t.actions.filter((a) => !a.row && a.kind === kind).map((a) => a.label)
                const rowActions = t.actions.filter((a) => a.row && a.kind === kind).map((a) => a.label)
                for (const label of creates) expect(await within(panel).findByRole('button', { name: label })).toBeInTheDocument()
                expect(within(panel).queryAllByRole('button').filter(creating).map((b) => b.textContent), `${p.id}/${t.id} creation`).toEqual(creates)
                if (t.disposition.kind === 'read' && p.id !== 'runtimeSecurity' && p.id !== 'gateway') {
                    // Read-only products: rows, and no change of any kind.
                    await waitFor(() =>
                        expect(within(panel).queryAllByRole('table').length + within(panel).queryAllByRole('list').length + within(panel).queryAllByRole('term').length, `${p.id}/${t.id} rows`).toBeGreaterThan(0)
                    )
                    expect(within(panel).queryAllByRole('button', { name: /…$/ }).filter((b) => !/attack results|Results for/.test(b.textContent ?? ''))).toEqual([])
                }
                if (rowActions.length === 0) continue
                if (p.id === 'gateway' && t.id === 'workspaces') {
                    const detail = await within(panel).findByText(/Settings of Team A/)
                    const buttons = within(detail.closest('[data-slot="gateway-workspace-detail"]') as HTMLElement)
                        .getAllByRole('button')
                        .map((b) => b.textContent)
                        .filter((x) => x?.endsWith('…'))
                    expect(buttons, `${p.id}/${t.id} rows`).toEqual(rowActions)
                    continue
                }
                // Rows are shown; a row's detail offers exactly the admitted row actions.
                const details = await within(panel).findAllByRole('button', { name: /^Details for / })
                await user.click(details[0]!)
                if (p.id === 'gateway') {
                    const bar = await waitFor(() => {
                        const found = panel.querySelector('[data-slot="gateway-item-actions"]')
                        expect(found).not.toBeNull()
                        return found as HTMLElement
                    })
                    expect(within(bar).getAllByRole('button').map((b) => b.textContent), `${p.id}/${t.id} rows`).toEqual(rowActions)
                } else {
                    const section = await waitFor(() => {
                        const found = panel.querySelector('[data-slot="administration-detail"]')
                        expect(found?.querySelector('[data-slot="detail-fields"]')).not.toBeNull()
                        return found as HTMLElement
                    })
                    const buttons = within(section)
                        .getAllByRole('button')
                        .map((b) => b.textContent)
                        .filter((x) => x !== 'Close details')
                    expect(buttons, `${p.id}/${t.id} rows`).toEqual(rowActions)
                }
            }
        }
    })

    it('Gateway keys show their rows and rotate in place through the typed review; no staged rotation (V2-T20)', async () => {
        const { native, user } = await launch(fxAdmin({ gateway: GATEWAY }))
        await user.click(within(product('gateway')).getByRole('button', { name: /AI Gateway/ }))
        await user.click(within(product('gateway')).getByRole('tab', { name: 'Keys' }))
        const panel = panelOf(product('gateway'))
        expect(await within(panel).findByText('serviceKeys-one')).toBeInTheDocument()
        await user.click(within(panel).getByRole('button', { name: 'Details for serviceKeys-one' }))
        const rotate = await within(panel).findByRole('button', { name: 'Rotate serviceKeys-one' })
        expect(within(panel).queryByRole('button', { name: /Issue additional key|Finish rotation/ })).toBeNull()
        await user.click(rotate)
        const sheet = await screen.findByRole('dialog', { name: 'Review a change' })
        expect(within(sheet).queryByText(/Rotate the key of/)).toBeNull()
        await user.click(within(sheet).getByRole('button', { name: 'Review change' }))
        expect(native.dispatched.at(-1)).toMatchObject({ kind: 'reviewChange', draft: { kind: 'gatewayServiceKeys', action: 'rotate', target: 'serviceKeys-1' } })
        await native.push({
            ...native.current,
            changes: { review: review({ kind: 'gatewayServiceKeys', action: 'rotate', target: 'serviceKeys-1', confirmation: 'serviceKeys-one' }), staged: null, results: [] },
        })
        const apply = await within(sheet).findByRole('button', { name: 'Apply…' })
        expect(apply).toBeDisabled()
        await user.type(within(sheet).getByRole('textbox', { name: /Type serviceKeys-one to confirm/ }), 'serviceKeys-one')
        expect(apply).toBeEnabled()
        expect(native.current.rotations).toEqual([])
    })

    it('V2-T21: the Red Team Targets tab shows identifiers and types only; a target name in the reply never renders', async () => {
        const { user } = await launch(fxAdmin())
        await user.click(within(product('redTeam')).getByRole('button', { name: /AI Red Teaming/ }))
        await user.click(within(product('redTeam')).getByRole('tab', { name: 'Targets' }))
        const panel = panelOf(product('redTeam'))
        const table = await within(panel).findByRole('table')
        expect(within(table).getByText('7a7a7a7a')).toBeInTheDocument()
        expect(within(table).getByText('Application')).toBeInTheDocument()
        expect(document.body.textContent).not.toContain('Payments assistant')
        expect(within(panel).getByRole('button', { name: 'Results for target 7a7a7a7a' })).toBeInTheDocument()
        // Each read-only tab shows its own part: Scans has the scan list and not the statistics tiles.
        await user.click(within(product('redTeam')).getByRole('tab', { name: 'Scans' }))
        expect(await within(product('redTeam')).findByRole('heading', { name: 'Red Teaming scans' })).toBeInTheDocument()
        expect(within(product('redTeam')).queryByLabelText('Red Teaming summary')).toBeNull()
    })

    it('a review exit ends the review in the core, also before its change settles and on unmount (V2-R06, I6)', async () => {
        const { native, user, unmount } = await launch(fxAdmin())
        const runtime = product('runtimeSecurity')
        await user.click(await within(runtime).findByRole('button', { name: 'Details for Finance' }))
        await user.click(await screen.findByRole('button', { name: 'Edit…' }))
        let sheet = await screen.findByRole('dialog', { name: 'Review a change' })
        await user.click(within(sheet).getByRole('button', { name: 'Review change' }))
        await native.push({ ...native.current, changes: { review: review(), staged: null, results: [] } })
        await user.type(await within(sheet).findByRole('textbox', { name: /Type Finance to confirm/ }), 'Finance')
        await user.click(within(sheet).getByRole('button', { name: 'Apply…' }))
        // Applied but not settled: Escape still ends the (consumed) review, so a key it returns is dropped on arrival.
        await native.push({
            ...native.current,
            changes: { review: null, staged: null, results: [{ ...settledResult(), settled: false, outcome: null, readback: null }] },
        })
        await user.keyboard('{Escape}')
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Review a change' })).toBeNull())
        expect(native.dispatched).toContainEqual({ kind: 'discardChange', review: 'rv-0011223344556677' })
        // A second review left by unmounting the whole page (navigation, window close).
        await user.click(await screen.findByRole('button', { name: 'Edit…' }))
        sheet = await screen.findByRole('dialog', { name: 'Review a change' })
        await user.click(within(sheet).getByRole('button', { name: 'Review change' }))
        await native.push({ ...native.current, changes: { review: review({ review: 'rv-8899aabbccddeeff' }), staged: null, results: [] } })
        await within(sheet).findByRole('button', { name: 'Apply…' })
        unmount()
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'discardChange', review: 'rv-8899aabbccddeeff' }))
    })

    it('says only what each action established: an update read back absent is not "Removed"; reconciliation reads the reviewed item (R5)', async () => {
        const data: Inventories = { keys: [KEY], topics: [TOPIC], absent: [] }
        const { native, user } = await launch(fxAdmin(), data)
        const runtime = product('runtimeSecurity')
        await user.click(await within(runtime).findByRole('button', { name: 'Details for Finance' }))
        await user.click(await screen.findByRole('button', { name: 'Edit…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Review a change' })
        await native.push({ ...native.current, changes: { review: null, staged: null, results: [{ ...settledResult(), readback: 'absent' }] } })
        const applied = await within(sheet).findByText(/Not confirmed/)
        expect(applied.closest('[data-slot="applied"]')).toHaveAttribute('data-state', 'unconfirmed')
        expect(within(sheet).queryByText(/^Removed/)).toBeNull()
        await user.click(within(sheet).getByRole('button', { name: 'Read the item back' }))
        expect(native.dispatched).toContainEqual({ kind: 'management', context: CONTEXT, request: { kind: 'detail', resource: 'topics', id: TOPIC.topic_id } })
        expect(await within(sheet).findByText('Read back now: the item is present.')).toBeInTheDocument()
        // A rejected change is never a success, whatever its readback says.
        await native.push({ ...native.current, changes: { review: null, staged: null, results: [{ ...settledResult('op-12'), outcome: 'rejected', readback: 'found' }, { ...settledResult(), readback: 'absent' }] } })
        expect(appliedStateOf({ ...settledResult(), outcome: 'rejected', readback: 'found' })).toBe('failed')
        expect(appliedStateOf({ ...settledResult(), action: 'delete', readback: 'absent' })).toBe('removed')
        expect(appliedStateOf({ ...settledResult(), action: 'update', readback: 'absent' })).toBe('unconfirmed')
        expect(appliedStateOf({ ...settledResult(), outcome: 'completed', failure: { kind: 'readback' } as unknown as ChangeResultView['failure'], readback: 'found' })).toBe('unconfirmed')
    })

    it('shows a response-only topic violation in its own labelled section beside unrelated detectors (R8)', async () => {
        const scan: ScanRecordView = {
            operation: 'op-4',
            progress: 'settled',
            outcome: 'completed',
            stop: null,
            failure: null,
            discarded: false,
            result: {
                scanId: 's-2',
                reportId: 'r-2',
                trId: null,
                profileName: 'chatbot-strict',
                profileId: null,
                category: 'malicious',
                action: 'block',
                verdict: 'block',
                timeout: false,
                serviceError: false,
                contentErrors: [],
                promptDetected: [{ detector: 'injection', detected: false }],
                responseDetected: [
                    { detector: 'dlp', detected: true },
                    { detector: 'topic_violation', detected: true },
                ],
                topics: { violation: false, blocked: [], allowed: [] },
                responseTopics: { violation: true, blocked: ['Finance'], allowed: [] },
            },
        }
        const { user } = await launch(fxAdmin({ scans: [scan] }))
        await user.click(within(product('runtimeSecurity')).getByRole('tab', { name: 'Playground' }))
        const response = await screen.findByRole('heading', { name: 'Custom topics · response' })
        const section = response.closest('[data-slot="scan-topics"]') as HTMLElement
        expect(section).toHaveAttribute('data-direction', 'response')
        expect(section).toHaveAttribute('data-topic-verdict', 'flagged')
        expect(within(section).getByText('Finance: flagged (blocked)')).toBeInTheDocument()
        const prompt = screen.getByRole('heading', { name: 'Custom topics · prompt' }).closest('[data-slot="scan-topics"]')
        expect(prompt).toHaveAttribute('data-topic-verdict', 'clear')
        expect(screen.getByText('dlp: detected')).toBeInTheDocument()
    })

    it('announces each rotation stage in one persistent status region, without moving focus, and toasts completion for its tenant (R9, I10)', async () => {
        const { native, user } = await launch(fxAdmin({ rotations: [rotation({ stage: 'acknowledged', deploymentAcknowledgedAt: 1_761_490_300 })] }))
        const runtime = product('runtimeSecurity')
        await user.click(within(runtime).getByRole('tab', { name: 'API keys' }))
        await user.click(await within(runtime).findByRole('button', { name: 'Details for Support key' }))
        await user.click(await screen.findByRole('button', { name: 'Finish rotation…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Rotate the key of support-copilot' })
        const status = sheet.querySelector('[data-slot="rotation-status"]') as HTMLElement
        expect(status).toHaveAttribute('role', 'status')
        expect(status).toHaveTextContent('Deployment acknowledged for support-copilot')
        await native.push({ ...native.current, rotations: [rotation({ stage: 'complete', deploymentAcknowledgedAt: 1_761_490_300, oldKeyRemovedAt: 1_761_490_420 })] })
        await waitFor(() => expect(sheet.querySelector('[data-slot="rotation-status"]')).toHaveTextContent('Rotation of support-copilot complete'))
        expect(sheet.querySelector('[data-slot="rotation-status"]')).toBe(status)
        // Focus never leaves the sheet, though the focused control went with its stage.
        expect(sheet.contains(document.activeElement)).toBe(true)
        await waitFor(() => expect(useShellStore.getState().toasts.map((t) => [t.text, t.tenant])).toContainEqual(['Rotation of support-copilot complete. The old key was read back gone.', 'tn-0123456789abcdef']))
    })

    it('derives profile and topic relationships from one profile walk, and marks the keys of a rotation (R7)', async () => {
        const data: Inventories = {
            keys: [KEY, { ...KEY, api_key_id: 'key-2', api_key_name: 'Support key 2' }],
            topics: [TOPIC],
            absent: [],
        }
        const { user } = await launch(fxAdmin({ rotations: [rotation({ stage: 'resumed' })] }), data)
        const runtime = product('runtimeSecurity')
        const usedBy = await within(runtime).findByText('Production guardrails')
        expect(usedBy).toHaveAttribute('data-slot', 'administration-relation')
        await user.click(within(runtime).getByRole('tab', { name: 'Security profiles' }))
        expect(await within(runtime).findByText('Finance', { selector: '[data-slot="administration-relation"]' })).toBeInTheDocument()
        await user.click(within(runtime).getByRole('tab', { name: 'API keys' }))
        expect(await within(runtime).findByText('Replace after deployment')).toBeInTheDocument()
        expect(within(runtime).getByText('New key for support-copilot')).toBeInTheDocument()
    })

    it('opens the old key’s removal only once the acknowledgement is recorded: refused, invalid, stale or unconfirmed never advance (R3)', async () => {
        const { native, user } = await launch(fxAdmin({ rotations: [rotation({ stage: 'resumed' })] }))
        const runtime = product('runtimeSecurity')
        await user.click(within(runtime).getByRole('tab', { name: 'API keys' }))
        await user.click(await within(runtime).findByRole('button', { name: 'Details for Support key' }))
        await user.click(await screen.findByRole('button', { name: 'Finish rotation…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Rotate the key of support-copilot' })
        const respond = native.respond
        for (const refusal of ['deniedStore', 'invalidInput', 'staleContext'] as const) {
            native.respond = (intent) => (intent.kind === 'acknowledgeDeployment' ? { outcome: 'refusal', refusal } : respond(intent))
            await user.click(within(sheet).getByRole('button', { name: 'Application updated' }))
            expect(await within(sheet).findByRole('alert')).toBeInTheDocument()
            expect(within(sheet).queryByRole('heading', { name: 'Remove Support key' })).toBeNull()
        }
        // Not confirmed: the delivery was lost; nothing advances until the acknowledgement is observed.
        const dispatch = native.transport.dispatch
        native.transport.dispatch = async () => {
            throw new Error('lost')
        }
        await user.click(within(sheet).getByRole('button', { name: 'Application updated' }))
        expect(await within(sheet).findByText(/did not confirm your acknowledgement/)).toBeInTheDocument()
        expect(within(sheet).queryByRole('heading', { name: 'Remove Support key' })).toBeNull()
        native.transport.dispatch = dispatch
        native.respond = respond
        // The recorded acknowledgement opens the removal, once.
        await native.push({ ...native.current, rotations: [rotation({ stage: 'acknowledged', deploymentAcknowledgedAt: 1_761_490_300 })] })
        expect(await within(sheet).findByRole('heading', { name: 'Remove Support key' })).toBeInTheDocument()
        expect(within(sheet).queryByText(/did not confirm your acknowledgement/)).toBeNull()
    })

    it('opens a Harness proposal’s native review in the review sheet with its exact draft; leaving discards it (R4)', async () => {
        const conversation = {
            id: 'op-7',
            workspace: WS,
            endpoint: 'https://gateway.example/rt/v1',
            phase: 'ready',
            failure: null,
            stop: null,
            entries: [],
            lastTurn: null,
            usage: null,
            cleanup: null,
            runtimeVersion: null,
            source: 'workspaceKey',
            historyId: '8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c6d',
            savedAt: null,
            saveFailure: null,
            unsent: null,
            signInFailure: null,
            project: null,
            write: false,
            approvals: [],
            proposals: [
                {
                    id: 'P-1',
                    summary: 'Tighten Finance',
                    draft: { kind: 'topics', action: 'update', target: TOPIC.topic_id, body: '{"topic_name":"Finance","description":"Money and payments"}' },
                    state: 'inReview',
                    review: 'rv-h1',
                },
            ],
            mcp: [],
        } as unknown as NonNullable<ShellSnapshot['harness']['conversation']>
        const base = fxAdmin({ gateway: GATEWAY })
        const { native, user } = await launch({
            ...base,
            harness: { ...base.harness, conversation },
            changes: { review: review({ review: 'rv-h1' }), staged: null, results: [] },
        })
        const sheet = await screen.findByRole('dialog', { name: 'Review a change' })
        expect(within(sheet).getAllByRole('heading', { name: `Proposed by the Harness: update custom topic ${TOPIC.topic_id}` }).length).toBeGreaterThan(0)
        expect(within(sheet).getByLabelText('custom topic draft (JSON)')).toHaveValue('{"topic_name":"Finance","description":"Money and payments"}')
        // Typed confirmation still gates the native review.
        const apply = within(sheet).getByRole('button', { name: 'Apply…' })
        expect(apply).toBeDisabled()
        await user.type(within(sheet).getByRole('textbox', { name: /Type Finance to confirm/ }), 'Finance')
        expect(apply).toBeEnabled()
        await user.keyboard('{Escape}')
        await waitFor(() => expect(native.dispatched).toContainEqual({ kind: 'discardChange', review: 'rv-h1' }))
    })

    it('names what a confirmed change established: tenant, resource, returned name and revision, and the recorded readback time (R5)', async () => {
        const { native, user } = await launch(fxAdmin())
        const runtime = product('runtimeSecurity')
        await user.click(await within(runtime).findByRole('button', { name: 'Details for Finance' }))
        await user.click(await screen.findByRole('button', { name: 'Edit…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Review a change' })
        const applied: ChangeResultView = { ...settledResult(), current: JSON.stringify({ ...TOPIC, revision: 2 }), settledAt: 1_761_490_123 }
        await native.push({ ...native.current, changes: { review: null, staged: null, results: [applied] } })
        const receipt = await waitFor(() => {
            const found = sheet.querySelector('[data-slot="applied-receipt"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        const pairs = Array.from(receipt.querySelectorAll('dt')).map((dt) => [dt.textContent, dt.nextElementSibling?.textContent])
        expect(pairs.slice(0, 4)).toEqual([
            ['Tenant', 'Northwind Health'],
            ['Resource', 'custom topic'],
            ['Name', 'Finance'],
            ['Revision', '2'],
        ])
        const readAt = pairs[4]
        expect(readAt?.[0]).toBe('Read back at')
        expect(readAt?.[1]).toMatch(/\d\d:\d\d/)
        // A later observation does not recompute the time.
        await native.push({ ...native.current })
        const again = Array.from(sheet.querySelectorAll('[data-slot="applied-receipt"] dd')).map((dd) => dd.textContent)
        expect(again[4]).toBe(readAt?.[1])
        // An unconfirmed change never receives the receipt.
        await native.push({ ...native.current, changes: { review: null, staged: null, results: [{ ...applied, operation: 'op-13', readback: 'stopped' }] } })
        await waitFor(() => expect(sheet.querySelector('[data-slot="applied"]')).toHaveAttribute('data-state', 'unconfirmed'))
        expect(sheet.querySelector('[data-slot="applied-receipt"]')).toBeNull()
    })

    it('shows a failed second issuance apart from the rotation it leaves unchanged, with a manual retry and nothing resent (R2)', async () => {
        const { native, user } = await launch(fxAdmin({ rotations: [rotation({ stage: 'resumed' })] }))
        const runtime = product('runtimeSecurity')
        await user.click(within(runtime).getByRole('tab', { name: 'API keys' }))
        await user.click(await within(runtime).findByRole('button', { name: 'Details for Support key' }))
        await user.click(await screen.findByRole('button', { name: 'Finish rotation…' }))
        const sheet = await screen.findByRole('dialog', { name: 'Rotate the key of support-copilot' })
        await user.click(within(sheet).getByRole('button', { name: 'Issue another key' }))
        expect(await within(sheet).findByRole('button', { name: 'Review change' })).toBeInTheDocument()
        const rejected: ChangeResultView = {
            ...settledResult('op-21'),
            kind: 'apiKeys',
            action: 'version',
            target: 'key-1',
            name: 'Support key 3',
            outcome: 'rejected',
            failure: { kind: 'service', failure: 'denied' },
            readback: null,
            current: null,
        }
        const before = native.dispatched.length
        await native.push({ ...native.current, changes: { review: null, staged: null, results: [rejected] } })
        const failed = await waitFor(() => {
            const found = sheet.querySelector('[data-slot="rotation-step-failed"]')
            expect(found).not.toBeNull()
            return found as HTMLElement
        })
        expect(failed).toHaveAttribute('role', 'alert')
        expect(failed.textContent).toMatch(/Nothing was changed/)
        expect(within(failed).getByText(/No new key was issued\. The rotation in progress is unchanged/)).toBeInTheDocument()
        // The previous rotation is still shown as it was.
        expect(sheet.querySelector('[data-slot="rotation-panel"]')).toHaveAttribute('data-stage', 'resumed')
        expect(native.dispatched.slice(before).filter((i) => i.kind === 'requestApply' || i.kind === 'reviewChange')).toEqual([])
        // Retrying is explicit: a new review, nothing sent until the operator asks.
        await user.click(within(failed).getByRole('button', { name: 'Review again…' }))
        expect(await within(sheet).findByRole('button', { name: 'Review change' })).toBeInTheDocument()
        expect(sheet.querySelector('[data-slot="rotation-step-failed"]')).toBeNull()
        expect(native.dispatched.slice(before).filter((i) => i.kind === 'requestApply' || i.kind === 'reviewChange')).toEqual([])
    })

    it('gives every topic verdict an icon and words, for the prompt and the response, in the production Playground (R3, I9)', async () => {
        const result = (topics: ScanRecordView['result'] extends infer R ? (R extends { topics: infer T } ? T : never) : never, responseTopics: typeof topics) =>
            ({
                scanId: 's-9',
                reportId: 'r-9',
                trId: null,
                profileName: 'chatbot-strict',
                profileId: null,
                category: 'malicious',
                action: 'block',
                verdict: 'block',
                timeout: false,
                serviceError: false,
                contentErrors: [],
                promptDetected: [],
                responseDetected: [{ detector: 'dlp', detected: false }],
                topics,
                responseTopics,
            }) as NonNullable<ScanRecordView['result']>
        const record = (operation: string, r: NonNullable<ScanRecordView['result']>): ScanRecordView => ({
            operation,
            progress: 'settled',
            outcome: 'completed',
            stop: null,
            failure: null,
            discarded: false,
            result: r,
        })
        const flagged = record('op-5', result({ violation: true, blocked: [], allowed: [] }, { violation: true, blocked: ['Finance'], allowed: ['Payroll'] }))
        const { native, user } = await launch(fxAdmin({ scans: [flagged] }))
        await user.click(within(product('runtimeSecurity')).getByRole('tab', { name: 'Playground' }))
        const expectIcon = (el: Element | null, words: RegExp) => {
            expect(el).not.toBeNull()
            expect(el!.querySelector('svg')).not.toBeNull()
            expect(el!.textContent).toMatch(words)
        }
        await screen.findByRole('heading', { name: 'Custom topics · response' })
        expectIcon(document.querySelector('[data-direction="prompt"] [data-verdict="unnamed"]'), /A topic matched \(the service named none\)/)
        expectIcon(document.querySelector('[data-direction="response"] [data-verdict="blocked"]'), /Finance: flagged \(blocked\)/)
        expectIcon(document.querySelector('[data-direction="response"] [data-verdict="monitor"]'), /Payroll: flagged \(monitor only\)/)
        // Clear and unreported verdicts carry an icon too.
        await native.push({ ...native.current, scans: [record('op-6', result({ violation: false, blocked: [], allowed: [] }, { violation: null, blocked: [], allowed: [] }))] })
        await waitFor(() => expect(document.querySelector('[data-direction="prompt"] [data-verdict="clear"]')).not.toBeNull())
        expectIcon(document.querySelector('[data-direction="prompt"] [data-verdict="clear"]'), /Not detected: no custom topic assigned to chatbot-strict matched/)
        expectIcon(document.querySelector('[data-direction="response"] [data-verdict="unreported"]'), /Not reported by the service for this scan/)
    })

    // ---- C39a: the board (R2App start=admin) over the same behaviour ----

    it('draws the product cards: description, headline figure from the read, status pill and alerts; Expand all and Collapse all (C39a)', async () => {
        const base = fxAdmin()
        const read = { ...base.inbox.read, products: [{ product: 'runtimeSecurity' as const, state: 'ok' as const, sourcesRead: 2, sourcesTotal: 2, detail: null }, { product: 'gateway' as const, state: 'needsRole' as const, sourcesRead: 0, sourcesTotal: 2, detail: null }] }
        const overview = { ...base.overview, read: { readAt: 1_761_490_000, chart: { bucketSeconds: 3600, buckets: [{ time: '2026-10-26T10:00:00Z', total: 12, violating: 3, detections: null }, { time: '2026-10-26T11:00:00Z', total: 9, violating: 0, detections: null }] }, chartFailure: null, summary: { sessions: 21, violating: 3, benign: 18, detectionEvents: 38 }, keysComplete: true, keysFailure: null, items: [], gaps: [], gateway: null } }
        const { user } = await launch({ ...base, inbox: { ...base.inbox, read, items: [{ id: 'e-1', kind: 'runtime', needsAction: true, severity: 'warn', title: '3 violating sessions', detail: '', source: 'Runtime Security', product: 'runtimeSecurity', target: null, actionLabel: null, facts: [], when: 1_761_490_000, hold: null }] }, overview })
        expect(screen.getByText('Northwind Health · every change is reviewed before it is applied')).toBeInTheDocument()
        const runtime = product('runtimeSecurity')
        expect(within(runtime).getByText('LLM and MCP guardrails: prompts, responses and tool calls are checked as they happen.')).toBeInTheDocument()
        expect(runtime.querySelector('[data-slot="admin-product-stat"]')).toHaveTextContent('38detections · 24h')
        expect(runtime.querySelector('[data-slot="admin-product-pill"]')).toHaveTextContent('Connected')
        expect(runtime.querySelector('[data-slot="admin-product-alerts"]')).toHaveTextContent('1 alert')
        const gateway = product('gateway')
        expect(gateway.querySelector('[data-slot="admin-product-pill"]')).toHaveTextContent('Needs a role')
        expect(gateway.querySelector('[data-slot="admin-product-stat"]')).toHaveTextContent('—requests · today')
        expect(product('agenticIdentity').querySelector('[data-slot="admin-product-pill"]')).toHaveTextContent('Not available')
        // Only Runtime is open at first; Expand all opens every available product, Collapse all closes them.
        expect(within(runtime).getByRole('button', { name: /AI Runtime Security/ })).toHaveAttribute('aria-expanded', 'true')
        expect(within(gateway).getByRole('button', { name: /AI Gateway/ })).toHaveAttribute('aria-expanded', 'false')
        await user.click(screen.getByRole('button', { name: 'Expand all' }))
        for (const id of ['runtimeSecurity', 'gateway', 'redTeam', 'modelSkill']) expect(within(product(id)).getByRole('tablist')).toBeInTheDocument()
        expect(within(product('agenticEndpoint')).queryByRole('tablist')).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Collapse all' }))
        for (const id of ['runtimeSecurity', 'gateway', 'redTeam', 'modelSkill']) expect(within(product(id)).queryByRole('tablist')).toBeNull()
    })

    it('draws the Runtime Overview from the read: three tiles and the 24-hour bars; the table footer names the count and the read time (C39a)', async () => {
        const base = fxAdmin()
        const overview = { ...base.overview, read: { readAt: 1_761_490_000, chart: { bucketSeconds: 3600, buckets: Array.from({ length: 24 }, (_, i) => ({ time: `2026-10-26T${String(i).padStart(2, '0')}:00:00Z`, total: 10, violating: i % 6 === 0 ? 2 : 0, detections: null })) }, chartFailure: null, summary: { sessions: 240, violating: 8, benign: 232, detectionEvents: 8 }, keysComplete: false, keysFailure: null, items: [], gaps: [], gateway: null } }
        const { user } = await launch({ ...base, overview })
        const runtime = product('runtimeSecurity')
        expect(await within(runtime).findByRole('button', { name: 'Details for Finance' })).toBeInTheDocument()
        expect(runtime.querySelector('[data-slot="administration-foot"]')).toHaveTextContent(/1 item.*Read \d\d:\d\d/)
        expect(within(runtime).getByRole('textbox', { name: 'Search custom topics' })).toBeInTheDocument()
        await user.click(within(runtime).getByRole('tab', { name: 'Overview' }))
        const tiles = Array.from(runtime.querySelectorAll('[data-slot="admin-kpi"]')).map((t) => t.textContent)
        expect(tiles[0]).toContain('Detection events · 24h')
        expect(tiles[0]).toContain('8')
        expect(tiles[1]).toContain('Sessions · 24h')
        expect(tiles[1]).toContain('240')
        expect(tiles[2]).toContain('Violating · 24h')
        expect(tiles[2]).toContain('API keys: Partial')
        expect(runtime.querySelector('[data-slot="admin-sparkline"]')).toHaveAttribute('data-bars', '24')
        expect(within(runtime).getByRole('img', { name: 'Activity, last 24 hours' })).toBeInTheDocument()
    })

    it('says when the chart was not read: the events tile names it and the bars are none (C39a)', async () => {
        const base = fxAdmin()
        const overview = { ...base.overview, read: { readAt: 1_761_490_000, chart: null, chartFailure: 'denied' as const, summary: { sessions: null, violating: null, benign: null, detectionEvents: null }, keysComplete: true, keysFailure: null, items: [], gaps: ['sessions chart'], gateway: null } }
        const { user } = await launch({ ...base, overview })
        const runtime = product('runtimeSecurity')
        await user.click(within(runtime).getByRole('tab', { name: 'Overview' }))
        expect(runtime.querySelectorAll('[data-slot="admin-kpi"]')[0]).toHaveTextContent('—')
        expect(runtime.querySelectorAll('[data-slot="admin-kpi"]')[0]).toHaveTextContent('Sessions chart not read')
        expect(within(runtime).getByRole('img', { name: 'Sessions chart not read' })).toBeInTheDocument()
        expect(runtime.querySelector('[data-slot="admin-sparkline"]')).toHaveAttribute('data-bars', '0')
    })
})

function settledResult(operation = 'op-11'): ChangeResultView {
    return {
        operation,
        kind: 'topics',
        action: 'update',
        target: TOPIC.topic_id,
        name: 'Finance',
        settled: true,
        outcome: 'completed',
        failure: null,
        response: null,
        readback: 'found',
        current: JSON.stringify(TOPIC),
        secret: 'none',
        workspace: null,
        provisioning: null,
    }
}
