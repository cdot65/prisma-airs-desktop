/**
 * The first-run wizard and add-tenant sheet (C29) over the scripted native
 * transport: the draft, its credentials, the chosen products, the verify run
 * and the completion gate all live in the core's `setup`; this presentation
 * only walks the steps, sends intents and shows what the snapshot says.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { DeliveryOutcomeUnknown, DesktopClient, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { scriptedNative, tenant, withTenant, type Native } from '../../test/native'
import { fxHome, fxSwitcher } from '../fixtures'
import { resetShellStore } from '../store'
import { DRAFT_CONTEXT, draft, setupView, verifyView } from './fixtures'

/** fx.on.welcome: v2 with onboarding still needed. */
function fxOnboarding(): ShellSnapshot {
    return { ...fxHome(), onboarding: 'needed', tenants: [], active: null }
}

async function launch(snapshot: ShellSnapshot): Promise<Native> {
    const native = scriptedNative(snapshot)
    render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('heading', { level: 1 })
    return native
}
const admitted = (operation: string | null = null) => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation } })

beforeEach(() => resetShellStore())

describe('first-run wizard', () => {
    it('opens on Welcome, marks the wizard in progress only once the core admits it, and keeps a refusal visible', async () => {
        const user = userEvent.setup()
        const native = await launch(fxOnboarding())
        expect(screen.getByRole('heading', { level: 1, name: 'Welcome to Prisma AIRS Desktop' })).toBeInTheDocument()
        expect(screen.getByText(/built entirely on Palo Alto Networks’ own public APIs/)).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Back' })).toBeNull()
        native.respond = () => ({ outcome: 'refusal', refusal: 'deniedStore' })
        await user.click(screen.getByRole('button', { name: 'Set up a tenant' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'setOnboarding', state: 'inProgress' })
        expect(screen.getByRole('heading', { level: 1, name: 'Welcome to Prisma AIRS Desktop' })).toBeInTheDocument()
        expect(document.querySelector('[data-slot="wizard-status"]')).toHaveTextContent(/Keychain|denied|store/i)
        native.respond = () => admitted()
        await user.click(screen.getByRole('button', { name: 'Set up a tenant' }))
        expect(screen.getByRole('heading', { level: 1, name: 'Connect your tenant' })).toHaveFocus()
    })

    it('validates per field, drafts the tenant in the core, and attaches the scan key to the draft context once without echoing it', async () => {
        const user = userEvent.setup()
        const native = await launch({ ...fxOnboarding(), onboarding: 'inProgress' })
        expect(screen.getByRole('heading', { level: 1, name: 'Connect your tenant' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Create the tenant to continue' })).toBeDisabled()
        await user.click(screen.getByRole('button', { name: 'Create tenant' }))
        const name = screen.getByLabelText(/Display name/)
        expect(name).toHaveAttribute('aria-invalid', 'true')
        expect(name).toHaveAccessibleDescription('Enter a display name.')
        expect(native.dispatched.filter((i) => i.kind === 'beginTenantSetup')).toHaveLength(0)
        await user.type(name, 'Northwind Health')
        await user.click(within(screen.getByRole('radiogroup', { name: 'Environment' })).getByRole('radio', { name: 'Staging' }))
        await user.selectOptions(screen.getByLabelText('Region'), 'europe')
        await user.type(screen.getByLabelText(/TSG ID/), '1762204418')
        await user.click(screen.getByRole('button', { name: 'Create tenant' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'beginTenantSetup', mode: 'firstRun', profile: { label: 'Northwind Health', tsgId: '1762204418', destination: { kind: 'region', region: 'europe' }, environment: 'staging' } })
        // The core holds the draft; nothing is a tenant yet.
        await native.push({ ...native.current, setup: setupView({ tenant: draft() }) })
        expect(await screen.findByText('Northwind Health is drafted. Add its scan key below.')).toBeInTheDocument()
        expect(native.current.tenants).toEqual([])
        expect(document.querySelector('[data-slot="rail-context"]')).toHaveTextContent('Northwind Health · Staging · TSG 1762204418')
        expect(screen.getByRole('button', { name: 'Save the scan key to continue' })).toBeDisabled()
        const key = screen.getByLabelText('Scan API key', { selector: 'input' })
        expect(key).toHaveAttribute('type', 'password')
        await user.type(key, 'synthetic-scan-key-aaaa')
        await user.click(screen.getByRole('button', { name: 'Save scan key' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'configureScanCredential', context: DRAFT_CONTEXT, mode: 'keychain', key: 'synthetic-scan-key-aaaa' })
        expect((key as HTMLInputElement).value).toBe('')
        expect(document.body.innerHTML).not.toContain('synthetic-scan-key-aaaa')
        await native.push({ ...native.current, setup: setupView() })
        expect(within(document.querySelector('[data-slot="scan-state"]') as HTMLElement).getByText('Saved')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Replace scan key' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Remove scan key' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Continue' })).toBeEnabled()
        // Editing the profile goes through updateTenantSetup on the draft.
        await user.type(screen.getByLabelText(/Display name/), ' EU')
        await user.click(screen.getByRole('button', { name: 'Save changes' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'updateTenantSetup', context: DRAFT_CONTEXT, profile: { label: 'Northwind Health EU', tsgId: '1762204418', destination: { kind: 'region', region: 'europe' }, environment: 'staging' } })
    })

    it('sets up a Gateway workspace on the draft: lists through the management client, chooses one, saves the endpoint and key once', async () => {
        const user = userEvent.setup()
        const clientReady = draft({ scan: { state: 'ready', mode: 'keychain', failure: null }, management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1762204418.iam.example', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' } })
        const native = await launch({ ...fxOnboarding(), onboarding: 'inProgress', setup: setupView({ tenant: clientReady }) })
        await user.click(screen.getByRole('switch', { name: 'Set up an AI Gateway workspace now' }))
        native.respond = () => admitted('op-7')
        native.observeManagement = () => ({ kind: 'management', result: { kind: 'gateway', gateway: { kind: 'list', list: { resource: 'workspaces', workspace: null, rows: JSON.stringify([{ id: 'ws-1', name: 'Harness' }, { id: 'ws-2', name: 'Lab' }]), count: 2, total: 2, complete: true } } } })
        await user.click(screen.getByRole('button', { name: 'Read workspaces' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'management', context: DRAFT_CONTEXT, request: { kind: 'gateway', workspace: null, read: { kind: 'inventory', resource: 'workspaces' } } })
        const chooser = await screen.findByLabelText('Workspace')
        await user.selectOptions(chooser, 'ws-2')
        native.respond = () => admitted()
        await user.click(screen.getByRole('button', { name: 'Use this workspace' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'selectGatewayWorkspace', context: DRAFT_CONTEXT, workspace: 'ws-2' })
        expect(await screen.findByText('Workspace Lab chosen.')).toBeInTheDocument()
        // The core records the choice on the draft; the key is configured under its reference.
        await native.push({ ...native.current, setup: setupView({ tenant: clientReady, workspace: { reference: 'gw-ref-0001', workspace: { id: 'ws-2', name: 'Lab', slug: 'lab', scopeName: 'Lab' }, restored: false } }) })
        await user.type(await screen.findByLabelText(/Inference endpoint/), 'https://gateway.example.test/v1')
        const key = screen.getByLabelText('Workspace inference key', { selector: 'input' })
        await user.type(key, 'synthetic-workspace-key-zz')
        await user.click(screen.getByRole('button', { name: 'Save workspace key' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'configureInferenceCredential', context: DRAFT_CONTEXT, mode: 'keychain', workspace: 'gw-ref-0001', endpoint: 'https://gateway.example.test/v1', secret: 'synthetic-workspace-key-zz' })
        expect((key as HTMLInputElement).value).toBe('')
        expect(document.body.innerHTML).not.toContain('synthetic-workspace-key-zz')
    })

    it('records products in the core, verifies through one core-owned run, gates Continue on the snapshot, and completes with one intent', async () => {
        const user = userEvent.setup()
        const native = await launch({ ...fxOnboarding(), onboarding: 'inProgress', setup: setupView() })
        await user.click(screen.getByRole('button', { name: 'Continue' }))
        expect(screen.getByRole('heading', { level: 1, name: 'Which products will you manage?' })).toBeInTheDocument()
        const boxes = screen.getAllByRole('checkbox')
        expect(boxes.map((b) => b.getAttribute('aria-disabled') === 'true')).toEqual([true, false, false, false, true, true])
        expect(boxes[0]).toBeChecked()
        expect(screen.getAllByText('Not available in this release')).toHaveLength(2)
        await user.click(screen.getByRole('checkbox', { name: /AI Gateway/ }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'chooseSetupProducts', context: DRAFT_CONTEXT, products: ['runtimeSecurity', 'gateway'] })
        await native.push({ ...native.current, setup: setupView({ products: ['runtimeSecurity', 'gateway'], packs: [{ product: 'runtimeSecurity', count: 6 }, { product: 'gateway', count: 2 }] }) })
        await user.click(screen.getByRole('button', { name: 'Continue' }))
        // Verify: rows are planned until a run exists (AIS/AES listed as not available); Continue waits for the core's gate.
        expect(screen.getAllByText('Not run yet')).toHaveLength(3)
        expect(screen.getAllByText('Not available')).toHaveLength(2)
        expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled()
        await user.click(screen.getByRole('button', { name: 'Run the checks' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'verifyTenantSetup', context: DRAFT_CONTEXT })
        const stored = { line: 'scanKey' as const, product: 'runtimeSecurity' as const, state: 'stored' as const, reason: 'Stored in the Keychain. Not checked here: the first scan proves it against the service.' }
        const warning = { line: 'managementClient' as const, product: 'runtimeSecurity' as const, state: 'warning' as const, reason: 'No management client: AI Gateway stay read-only until one is saved in Administration.' }
        const gatewayPending = { line: 'gateway' as const, product: 'gateway' as const, state: 'pending' as const, reason: 'Reading…' }
        await native.push({ ...native.current, setup: setupView({ products: ['runtimeSecurity', 'gateway'], verify: verifyView([stored, warning, gatewayPending]) }) })
        expect(await screen.findByRole('list', { name: 'Checks' })).toHaveAttribute('aria-busy', 'true')
        expect(within(document.querySelector('[data-slot="verify-scanKey"]') as HTMLElement).getByText('Stored')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled()
        const gatewayWarn = { ...gatewayPending, state: 'warning' as const, reason: 'Needs the management client; not checked.' }
        await native.push({ ...native.current, setup: setupView({ products: ['runtimeSecurity', 'gateway'], verify: verifyView([stored, warning, gatewayWarn]) }) })
        expect(await screen.findByText('Checks finished with warnings.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Continue with warnings' })).toBeEnabled()
        // An edit makes the run stale: the core says so and the gate closes.
        await native.push({ ...native.current, setup: setupView({ products: ['runtimeSecurity', 'gateway'], verify: verifyView([stored, warning, gatewayWarn], { stale: true }) }) })
        expect(await screen.findByText('The inputs changed since this run. Verify again before continuing.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled()
        await native.push({ ...native.current, setup: setupView({ products: ['runtimeSecurity', 'gateway'], packs: [{ product: 'runtimeSecurity', count: 6 }, { product: 'gateway', count: 2 }], verify: verifyView([stored, warning, gatewayWarn]) }) })
        await user.click(await screen.findByRole('button', { name: 'Continue with warnings' }))
        // Packs per chosen product with counts; Academy through the reviewed account component.
        expect(screen.getByRole('checkbox', { name: 'AI Runtime Security · 6 cases' })).toBeChecked()
        await user.click(screen.getByRole('checkbox', { name: 'AI Gateway · 2 cases' }))
        // A product dropped later takes its pack with it (the core would refuse a stray pack).
        await native.push({ ...native.current, setup: setupView({ products: ['runtimeSecurity'], packs: [{ product: 'runtimeSecurity', count: 6 }], verify: verifyView([stored, warning]) }) })
        expect(screen.queryByRole('checkbox', { name: /AI Gateway/ })).toBeNull()
        await native.push({ ...native.current, setup: setupView({ products: ['runtimeSecurity', 'gateway'], packs: [{ product: 'runtimeSecurity', count: 6 }, { product: 'gateway', count: 2 }], verify: verifyView([stored, warning, gatewayWarn]) }) })
        expect(await screen.findByRole('checkbox', { name: 'AI Gateway · 2 cases' })).not.toBeChecked()
        await user.click(screen.getByRole('button', { name: 'Continue' }))
        expect(screen.getByRole('heading', { level: 1, name: 'Join the AI Security Academy' })).toBeInTheDocument()
        expect(screen.getByText(/syncs your Academy progress only/)).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Sign in to Academy' })).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Skip for now' }))
        expect(screen.getByRole('heading', { level: 1, name: /is ready$/ })).toBeInTheDocument()
        expect(document.querySelector('[data-slot="summary"]')).toHaveTextContent('Products: AI Runtime Security, AI Gateway')
        expect(document.querySelector('[data-slot="summary"]')).toHaveTextContent('Starter cases: AI Runtime Security')
        // Completion is one intent; the shell takes over when the core confirms.
        await user.click(screen.getByRole('button', { name: 'Open Prisma AIRS' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'completeTenantSetup', context: DRAFT_CONTEXT, packs: ['runtimeSecurity'], resolution: null })
        expect(screen.getByRole('button', { name: 'Opening…' })).toBeDisabled()
        const nw = tenant({ id: 'tn-0123456789abcdef', label: 'Northwind Health', tsgId: '1762204418', environment: 'staging', products: ['runtimeSecurity', 'gateway'] })
        await native.push(withTenant({ ...native.current, onboarding: 'done', setup: null }, nw))
        expect(await screen.findByRole('navigation', { name: 'Destinations' })).toBeInTheDocument()
    })

    it('keeps a refused or unconfirmed completion visible with Retry instead of closing', async () => {
        const user = userEvent.setup()
        const verified = verifyView([{ line: 'scanKey', product: 'runtimeSecurity', state: 'stored', reason: 'Stored.' }, { line: 'managementClient', product: 'runtimeSecurity', state: 'skipped', reason: 'No client.' }])
        const native = await launch({ ...fxOnboarding(), onboarding: 'inProgress', setup: setupView({ verify: verified }) })
        for (const step of ['Continue', 'Continue', 'Continue', 'Continue', 'Skip for now']) await user.click(screen.getByRole('button', { name: step }))
        expect(screen.getByRole('heading', { level: 1, name: /is ready$/ })).toBeInTheDocument()
        native.respond = () => ({ outcome: 'refusal', refusal: 'invalidInput' })
        await user.click(screen.getByRole('button', { name: 'Open Prisma AIRS' }))
        expect(await screen.findByText('Check the entered values and try again.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Open Prisma AIRS' })).toBeEnabled()
        native.respond = () => {
            throw new DeliveryOutcomeUnknown('lost')
        }
        await user.click(screen.getByRole('button', { name: 'Open Prisma AIRS' }))
        expect(await screen.findByText('Sent but not confirmed. The current state is shown.')).toBeInTheDocument()
        native.respond = () => admitted()
        // (An unconfirmed delivery is re-sent once by the client before it gives up.)
        const sent = native.dispatched.length
        await user.click(screen.getByRole('button', { name: 'Retry' }))
        expect(native.dispatched.length).toBe(sent + 1)
        expect(native.dispatched.at(-1)).toEqual({ kind: 'completeTenantSetup', context: DRAFT_CONTEXT, packs: ['runtimeSecurity'], resolution: null })
    })

    it('reaches the reviewed v1 import from the tenant step and comes back to the setup', async () => {
        const user = userEvent.setup()
        const native = await launch({ ...fxOnboarding(), onboarding: 'inProgress' })
        await user.click(screen.getByRole('button', { name: 'Import from Prisma AIRS Desktop v1' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'import' })
        await native.push({ ...native.current, activeFeature: 'import' })
        expect(await screen.findByRole('heading', { level: 1, name: 'Import from v1' })).toBeInTheDocument()
        expect(screen.queryByRole('heading', { level: 1, name: 'Connect your tenant' })).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Back to setup' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'navigate', feature: 'home' })
        await native.push({ ...native.current, activeFeature: 'home' })
        expect(await screen.findByRole('heading', { level: 1, name: 'Connect your tenant' })).toBeInTheDocument()
    })

    it('a committed import ends the first run: the shell opens on the imported tenant', async () => {
        const user = userEvent.setup()
        const native = await launch({ ...fxOnboarding(), onboarding: 'inProgress' })
        await user.click(screen.getByRole('button', { name: 'Import from Prisma AIRS Desktop v1' }))
        await native.push({ ...native.current, activeFeature: 'import' })
        await screen.findByRole('heading', { level: 1, name: 'Import from v1' })
        // The reviewed import commits its tenant; the core reports onboarding done and no draft.
        const imported = tenant({ id: 'tn-00000000c0a57a10', label: 'Imported', tsgId: '2222222222' })
        await native.push(withTenant({ ...native.current, onboarding: 'done', setup: null, activeFeature: 'home' }, imported))
        expect(await screen.findByRole('navigation', { name: 'Destinations' })).toBeInTheDocument()
        expect(screen.queryByRole('heading', { level: 1, name: 'Connect your tenant' })).toBeNull()
        expect(screen.getByRole('button', { name: /^Switch tenant: Imported/ })).toBeInTheDocument()
    })
})

describe('add-tenant sheet', () => {
    it('opens from the switcher as a modal with the 244px rail and no Welcome or Academy step; Cancel before a draft sends nothing', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        expect(sheet).toHaveAttribute('aria-modal', 'true')
        expect(sheet.style.maxHeight).toBe('92vh')
        const steps = within(sheet).getByRole('navigation', { name: 'Setup steps' })
        expect(within(steps).getAllByRole('button').map((b) => b.textContent)).toEqual(['1Tenant', '2Products', '3Verify', '4Starter casesOptional', '5Done', 'Light appearance'])
        expect(within(sheet).getByRole('button', { name: 'Cancel' })).toBeInTheDocument()
        expect(steps).toHaveStyle({ width: '244px' })
        expect(within(sheet).queryByRole('button', { name: 'Back' })).toBeNull()
        await user.click(within(sheet).getByRole('button', { name: 'Cancel' }))
        expect(screen.queryByRole('dialog')).toBeNull()
        expect(native.dispatched.filter((i) => i.kind !== 'navigate')).toEqual([])
    })

    it('drafts in add mode with an optional key, discards the draft on Cancel, and leaves the shell as it was', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        await user.type(within(sheet).getByLabelText(/Display name/), 'Coastal Bank')
        await user.type(within(sheet).getByLabelText(/TSG ID/), '2000000001')
        await user.click(within(sheet).getByRole('button', { name: 'Create tenant' }))
        expect(native.dispatched.at(-1)).toMatchObject({ kind: 'beginTenantSetup', mode: 'add', profile: { label: 'Coastal Bank', tsgId: '2000000001' } })
        const before = { tenants: native.current.tenants, active: native.current.active }
        await native.push({ ...native.current, setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001', environment: 'production', destination: { kind: 'region', region: 'unitedStates' } }) }) })
        await within(sheet).findByText('Coastal Bank is drafted. Add its scan key below.')
        expect(within(sheet).getByText(/Optional here; without it scans stay unavailable/)).toBeInTheDocument()
        expect(within(sheet).getByRole('button', { name: 'Continue' })).toBeEnabled()
        expect(native.current.tenants).toEqual(before.tenants)
        expect(native.current.active).toEqual(before.active)
        await user.click(within(sheet).getByRole('button', { name: 'Cancel' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'discardTenantSetup', context: DRAFT_CONTEXT })
        expect(screen.queryByRole('dialog')).toBeNull()
    })

    it('keeps the sheet open with Retry when the discard is refused, and closes only once the core admits it', async () => {
        const user = userEvent.setup()
        const native = await launch({ ...fxSwitcher(), setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001' }) }) })
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        native.respond = () => ({ outcome: 'refusal', refusal: 'deniedStore' })
        await user.keyboard('{Escape}')
        expect(native.dispatched.at(-1)).toEqual({ kind: 'discardTenantSetup', context: DRAFT_CONTEXT })
        expect(screen.getByRole('dialog', { name: 'Add a tenant' })).toBeInTheDocument()
        expect(await within(sheet).findByText(/could not be discarded/)).toBeInTheDocument()
        native.respond = () => admitted()
        await user.click(within(sheet).getByRole('button', { name: 'Retry' }))
        expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup')).toHaveLength(2)
        expect(screen.queryByRole('dialog')).toBeNull()
    })

    it('does not let unsaved profile edits reach later steps or completion', async () => {
        const user = userEvent.setup()
        const verified = verifyView([{ line: 'scanKey', product: 'runtimeSecurity', state: 'warning', reason: 'No scan key.' }, { line: 'managementClient', product: 'runtimeSecurity', state: 'skipped', reason: 'No client.' }])
        await launch({ ...fxSwitcher(), setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001' }), verify: verified }) })
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        await user.clear(within(sheet).getByLabelText(/TSG ID/))
        expect(within(sheet).getByRole('button', { name: 'Save or revert the changes to continue' })).toBeDisabled()
        expect(within(within(sheet).getByRole('navigation', { name: 'Setup steps' })).getByRole('button', { name: /Done$/ })).toBeDisabled()
        await user.click(within(sheet).getByRole('button', { name: 'Revert changes' }))
        expect(within(sheet).getByLabelText(/TSG ID/)).toHaveValue('2000000001')
        expect(within(sheet).getByRole('button', { name: 'Continue' })).toBeEnabled()
    })

    it('waits for a pending creation on Cancel and discards what it produced before closing', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        await user.type(within(sheet).getByLabelText(/Display name/), 'Coastal Bank')
        await user.type(within(sheet).getByLabelText(/TSG ID/), '2000000001')
        let answer: (value: unknown) => void = () => undefined
        native.respond = () => new Promise((resolve) => (answer = resolve))
        await user.click(within(sheet).getByRole('button', { name: 'Create tenant' }))
        await user.click(within(sheet).getByRole('button', { name: 'Cancel' }))
        // Nothing closes and nothing is discarded while the creation is undecided.
        expect(screen.getByRole('dialog', { name: 'Add a tenant' })).toBeInTheDocument()
        expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup')).toHaveLength(0)
        native.respond = () => admitted()
        // The receipt settles first, on its own render: the draft is coming, so the
        // sheet must neither close nor discard yet.
        answer(admitted())
        await waitFor(() => expect(within(sheet).getByRole('button', { name: 'Discarding…' })).toBeDisabled())
        await new Promise((r) => setTimeout(r, 30))
        expect(screen.getByRole('dialog', { name: 'Add a tenant' })).toBeInTheDocument()
        expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup')).toHaveLength(0)
        await native.push({ ...native.current, setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001', environment: 'production', destination: { kind: 'region', region: 'unitedStates' } }) }) })
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup')).toHaveLength(1))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'discardTenantSetup', context: DRAFT_CONTEXT })
        await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    })

    it('discards the draft once when its snapshot arrives before the creation receipt', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        await user.type(within(sheet).getByLabelText(/Display name/), 'Coastal Bank')
        await user.type(within(sheet).getByLabelText(/TSG ID/), '2000000001')
        let answer: (value: unknown) => void = () => undefined
        native.respond = () => new Promise((resolve) => (answer = resolve))
        await user.click(within(sheet).getByRole('button', { name: 'Create tenant' }))
        await user.click(within(sheet).getByRole('button', { name: 'Cancel' }))
        // The draft shows up first; the receipt is still outstanding.
        await native.push({ ...native.current, setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001', environment: 'production', destination: { kind: 'region', region: 'unitedStates' } }) }) })
        expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup')).toHaveLength(0)
        native.respond = () => admitted()
        answer(admitted())
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup')).toHaveLength(1))
        await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    })

    it('an unconfirmed discard with the draft still present offers Retry, which discards and closes', async () => {
        const user = userEvent.setup()
        const native = await launch({ ...fxSwitcher(), setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001' }) }) })
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        native.respond = () => {
            throw new DeliveryOutcomeUnknown('lost')
        }
        await user.click(within(sheet).getByRole('button', { name: 'Cancel' }))
        await within(sheet).findByText('Sent but not confirmed. The current state is shown.')
        // The draft is still there: Retry is offered; Escape is the same retry.
        await native.push({ ...native.current })
        expect(screen.getByRole('dialog', { name: 'Add a tenant' })).toBeInTheDocument()
        const sent = native.dispatched.filter((i) => i.kind === 'discardTenantSetup').length
        native.respond = () => admitted()
        await user.click(within(sheet).getByRole('button', { name: 'Retry' }))
        expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup')).toHaveLength(sent + 1)
        expect(native.dispatched.at(-1)).toEqual({ kind: 'discardTenantSetup', context: DRAFT_CONTEXT })
        await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    })

    it('after an unconfirmed creation, Cancel discards the draft the next snapshot shows, and an unconfirmed discard closes once the draft is gone', async () => {
        const user = userEvent.setup()
        const native = await launch(fxSwitcher())
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        await user.type(within(sheet).getByLabelText(/Display name/), 'Coastal Bank')
        await user.type(within(sheet).getByLabelText(/TSG ID/), '2000000001')
        native.respond = () => {
            throw new DeliveryOutcomeUnknown('lost')
        }
        await user.click(within(sheet).getByRole('button', { name: 'Create tenant' }))
        await within(sheet).findByText('Sent but not confirmed. The current state is shown.')
        await user.click(within(sheet).getByRole('button', { name: 'Cancel' }))
        expect(screen.getByRole('dialog', { name: 'Add a tenant' })).toBeInTheDocument()
        expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup')).toHaveLength(0)
        // The creation did go through: the draft appears, and the discard is sent — unconfirmed too.
        await native.push({ ...native.current, setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001', environment: 'production', destination: { kind: 'region', region: 'unitedStates' } }) }) })
        await waitFor(() => expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup').length).toBeGreaterThan(0))
        expect(screen.getByRole('dialog', { name: 'Add a tenant' })).toBeInTheDocument()
        native.respond = () => admitted()
        await native.push({ ...native.current, setup: null })
        await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    })

    it('the Done checks line is derived from the verification states: a warning and a failure are counted, a passed line is counted, and "All passed" needs every line to pass (attempt-01 R1)', async () => {
        const warned = verifyView([
            { line: 'scanKey', product: 'runtimeSecurity', state: 'warning', reason: 'No scan key saved; scans stay unavailable until one is added in Administration.' },
            { line: 'managementClient', product: 'runtimeSecurity', state: 'fail', reason: 'The client was refused.' },
            { line: 'gateway', product: 'gateway', state: 'pass', reason: 'Read the workspace.' },
        ])
        const native = await launch({ ...fxSwitcher(), setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001' }), products: ['runtimeSecurity', 'gateway'], verify: warned }) })
        const user = userEvent.setup()
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        for (const step of ['Continue', 'Continue', 'Continue with warnings', 'Skip for now']) await user.click(within(sheet).getByRole('button', { name: step }))
        expect(within(sheet).getByRole('heading', { level: 1, name: 'Coastal Bank is ready' })).toBeInTheDocument()
        expect(sheet.querySelector('[data-slot="summary"]')).toHaveTextContent('Checks: 1 passed · 1 warning · 1 failed · 2 not available')
        expect(sheet.querySelector('[data-slot="summary"]')).not.toHaveTextContent('All passed')
        void native
    })

    it('the Environment segmented control is a native radio group: one tab stop, arrow keys move the selection, Tab leaves to Region (attempt-01 R2)', async () => {
        const user = userEvent.setup()
        await launch({ ...fxOnboarding(), onboarding: 'inProgress' })
        const group = screen.getByRole('radiogroup', { name: 'Environment' })
        const production = within(group).getByRole('radio', { name: 'Production' })
        const staging = within(group).getByRole('radio', { name: 'Staging' })
        expect(production).toBeChecked()
        screen.getByLabelText(/TSG ID/).focus()
        await user.tab()
        expect(production).toHaveFocus()
        await user.keyboard('{ArrowRight}')
        expect(staging).toBeChecked()
        expect(staging).toHaveFocus()
        expect(document.querySelector('[data-slot="rail-context"]') ?? document.body).toBeInTheDocument()
        await user.tab()
        expect(screen.getByLabelText('Region')).toHaveFocus()
        await user.tab({ shift: true })
        expect(staging).toHaveFocus()
        await user.keyboard('{ArrowLeft}')
        expect(production).toBeChecked()
    })

    it('finishes with one completion intent and a toast naming the new tenant', async () => {
        const user = userEvent.setup()
        const verified = verifyView([{ line: 'scanKey', product: 'runtimeSecurity', state: 'warning', reason: 'No scan key saved; scans stay unavailable until one is added in Administration.' }, { line: 'managementClient', product: 'runtimeSecurity', state: 'skipped', reason: 'No client.' }])
        const native = await launch({ ...fxSwitcher(), setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001' }), verify: verified }) })
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        expect(document.querySelector('[data-slot="rail-context"]')).toHaveTextContent('Coastal Bank · Staging · TSG 2000000001')
        await user.click(within(sheet).getByRole('button', { name: 'Continue' }))
        await user.click(within(sheet).getByRole('button', { name: 'Continue' }))
        await user.click(within(sheet).getByRole('button', { name: 'Continue with warnings' }))
        await user.click(within(sheet).getByRole('button', { name: 'Skip for now' }))
        await user.click(within(sheet).getByRole('button', { name: 'Finish' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'completeTenantSetup', context: DRAFT_CONTEXT, packs: [], resolution: null })
        expect(screen.queryByRole('dialog')).toBeNull()
        expect(native.dispatched.filter((i) => i.kind === 'discardTenantSetup')).toHaveLength(0)
        expect(await screen.findByText(/Coastal Bank is set up and active\./)).toBeInTheDocument()
    })

    it('completion is the guarded switch path (I1): a pending Harness decision shows the guard in the sheet and the resolution re-sends the completion', async () => {
        const user = userEvent.setup()
        const verified = verifyView([{ line: 'scanKey', product: 'runtimeSecurity', state: 'warning', reason: 'No scan key saved; scans stay unavailable until one is added in Administration.' }, { line: 'managementClient', product: 'runtimeSecurity', state: 'skipped', reason: 'No client.' }])
        const native = await launch({ ...fxSwitcher(), setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001' }), verify: verified }) })
        await user.click(screen.getByRole('button', { name: /^Switch tenant/ }))
        await user.click(screen.getByRole('button', { name: 'Add a tenant…' }))
        const sheet = screen.getByRole('dialog', { name: 'Add a tenant' })
        for (const step of ['Continue', 'Continue', 'Continue with warnings', 'Skip for now']) await user.click(within(sheet).getByRole('button', { name: step }))
        native.respond = () => ({ outcome: 'refusal', refusal: 'decisionPending' })
        await user.click(within(sheet).getByRole('button', { name: 'Finish' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'completeTenantSetup', context: DRAFT_CONTEXT, packs: [], resolution: null })
        // Not a status line: the sheet stays, the draft stays, nothing was decided.
        expect(screen.getByRole('dialog', { name: 'Add a tenant' })).toBeInTheDocument()
        expect(document.querySelector('[data-slot="wizard-status"]')?.textContent).toBe('')
        const draftId = native.current.setup?.tenant.id as string
        await native.push({ ...native.current, switchBlocked: { to: draftId, pending: { conversation: 'op-7', decision: 'ap-1' }, completion: true } })
        const guard = await screen.findByRole('dialog', { name: 'A Harness decision is waiting' })
        expect(within(guard).getByText(/Finishing the setup of Coastal Bank makes it active/)).toBeInTheDocument()
        native.respond = () => admitted()
        await user.click(within(guard).getByRole('button', { name: 'Stay' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'dismissSwitchGuard' })
        await native.push({ ...native.current, switchBlocked: null })
        await waitFor(() => expect(screen.queryByRole('dialog', { name: 'A Harness decision is waiting' })).toBeNull())
        expect(screen.getByRole('dialog', { name: 'Add a tenant' })).toBeInTheDocument()
        await native.push({ ...native.current, switchBlocked: { to: draftId, pending: { conversation: 'op-7', decision: 'ap-1' }, completion: true } })
        await user.click(within(await screen.findByRole('dialog', { name: 'A Harness decision is waiting' })).getByRole('button', { name: 'Archive and switch' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'completeTenantSetup', context: DRAFT_CONTEXT, packs: [], resolution: 'archive' })
        expect(await screen.findByText(/Coastal Bank is set up and active\./)).toBeInTheDocument()
    })
})
