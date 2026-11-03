/**
 * Wizard board fixtures (docs/architecture/ui-v2/screens/boards.md, `fx.on.*`
 * and `fx.addTenant`): every reviewed R2Onboard state as the native snapshot
 * the real DesktopClient parses, with the setup draft the core would hold at
 * that point and the step the wizard opens on.
 */
import type { SetupView, ShellSnapshot, TenantView, VerifyStep, VerifyView } from '@prisma-airs/desktop-contracts'
import { tenant } from '../../test/native'
import { fxHome, fxSwitcher } from '../fixtures'
import type { WizardInitial } from './wizard'

export interface WizardBoard {
    snapshot: ShellSnapshot
    mode: 'firstRun' | 'add'
    initial?: WizardInitial
}

export const DRAFT_CONTEXT = 'cx-draft00000000000000000000'
const DRAFT_ID = 'tn-00000000d4af7001'

/** The draft Northwind Health as the core shows it, with the credentials given. */
export function draft(over: Partial<TenantView> = {}): TenantView {
    return tenant({ id: DRAFT_ID, label: 'Northwind Health', tsgId: '1762204418', environment: 'staging', destination: { kind: 'region', region: 'europe' }, ...over })
}
const keyed = (): TenantView => draft({ scan: { state: 'ready', mode: 'keychain', failure: null } })

const STORED: VerifyStep = { line: 'scanKey', product: 'runtimeSecurity', state: 'stored', reason: 'Stored in the Keychain. Not checked here: the first scan proves it against the service.' }
const CLIENT_SKIPPED: VerifyStep = { line: 'managementClient', product: 'runtimeSecurity', state: 'skipped', reason: 'No management client; Administration stays read-only.' }
export const UNAVAILABLE: VerifyStep[] = [
    { line: 'ais', product: null, state: 'unavailable', reason: 'Not available in this release: no admitted service (D058).' },
    { line: 'aes', product: null, state: 'unavailable', reason: 'Not available in this release: no admitted service (D058).' },
]

/** A verify view as the core derives it: warnings carry warning lines and failed optional lines; only the scan key blocks. */
export function verifyView(lines: VerifyStep[], over: Partial<VerifyView> = {}): VerifyView {
    const steps = [...lines, ...UNAVAILABLE]
    const done = steps.every((s) => s.state !== 'pending')
    const warnings = steps.flatMap((s) => (s.product === null ? [] : s.state === 'warning' ? [{ product: s.product, reason: s.reason }] : s.state === 'fail' && s.line !== 'scanKey' ? [{ product: s.product, reason: `Failed: ${s.reason}` }] : []))
    return { generation: 1, steps, done, failed: steps.some((s) => s.state === 'fail'), blocking: steps.some((s) => s.state === 'fail' && s.line === 'scanKey'), stale: false, warnings, ...over }
}

export function setupView(over: Partial<SetupView> = {}): SetupView {
    const verify = over.verify ?? null
    return {
        mode: 'firstRun',
        context: DRAFT_CONTEXT,
        tenant: keyed(),
        products: ['runtimeSecurity'],
        scanRequired: true,
        packs: [{ product: 'runtimeSecurity', count: 6 }],
        workspace: null,
        verify,
        completable: Boolean(verify && verify.done && !verify.blocking && !verify.stale),
        ...over,
    }
}

const needed = (): ShellSnapshot => ({ ...fxHome(), onboarding: 'needed', tenants: [], active: null })
const withSetup = (setup: SetupView): ShellSnapshot => ({ ...needed(), onboarding: 'inProgress', setup })
const VERIFIED = verifyView([STORED, CLIENT_SKIPPED])

export const fxOn = {
    /** R2On01 */
    welcome: (): WizardBoard => ({ snapshot: needed(), mode: 'firstRun' }),
    /** R2On02: the tenant step before anything is drafted. */
    tenantEmpty: (): WizardBoard => ({ snapshot: { ...needed(), onboarding: 'inProgress' }, mode: 'firstRun', initial: { step: 'tenant' } }),
    /** R2On02b: drafted and keyed; the rail carries the context line. */
    tenantFilled: (): WizardBoard => ({ snapshot: withSetup(setupView()), mode: 'firstRun', initial: { step: 'tenant' } }),
    /** R2On03 */
    products: (): WizardBoard => ({ snapshot: withSetup(setupView({ products: ['runtimeSecurity', 'gateway'], packs: [{ product: 'runtimeSecurity', count: 6 }, { product: 'gateway', count: 2 }] })), mode: 'firstRun', initial: { step: 'products' } }),
    /** R2On04: results streaming in. */
    verifyRunning: (): WizardBoard => ({
        snapshot: withSetup(setupView({ products: ['runtimeSecurity', 'gateway'], verify: verifyView([STORED, { line: 'managementClient', product: 'runtimeSecurity', state: 'pending', reason: 'Reading…' }, { line: 'gateway', product: 'gateway', state: 'pending', reason: 'Reading…' }]) })),
        mode: 'firstRun',
        initial: { step: 'verify' },
    }),
    /** R2On04b: a warning row on an admitted product. */
    verifyWarn: (): WizardBoard => ({
        snapshot: withSetup(setupView({ products: ['runtimeSecurity', 'gateway'], verify: verifyView([STORED, { line: 'managementClient', product: 'runtimeSecurity', state: 'warning', reason: 'No management client: AI Gateway stay read-only until one is saved in Administration.' }, { line: 'gateway', product: 'gateway', state: 'warning', reason: 'Needs the management client; not checked.' }]) })),
        mode: 'firstRun',
        initial: { step: 'verify' },
    }),
    /** R2On04c: a required line failed (first run without a scan key): blocked. */
    verifyFail: (): WizardBoard => ({
        snapshot: withSetup(setupView({ tenant: draft({ management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1762204418.iam.example', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' } }), verify: verifyView([{ line: 'scanKey', product: 'runtimeSecurity', state: 'fail', reason: 'No scan key saved. Runtime Security needs one on first run.' }, { line: 'managementClient', product: 'runtimeSecurity', state: 'pass', reason: 'The management client can read this tenant.' }]) })),
        mode: 'firstRun',
        initial: { step: 'verify' },
    }),
    /** R2On04c-opt: an optional service failed: continue with it recorded as a warning, or fix it. */
    verifyOptionalFail: (): WizardBoard => ({
        snapshot: withSetup(setupView({ tenant: draft({ scan: { state: 'ready', mode: 'keychain', failure: null }, management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1762204418.iam.example', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' } }), verify: verifyView([STORED, { line: 'managementClient', product: 'runtimeSecurity', state: 'fail', reason: 'The management client was not accepted. Check the client ID and secret.' }]) })),
        mode: 'firstRun',
        initial: { step: 'verify' },
    }),
    /** R2On04d: results that no longer count after an edit. */
    verifyStale: (): WizardBoard => ({ snapshot: withSetup(setupView({ verify: verifyView([STORED, CLIENT_SKIPPED], { stale: true }) })), mode: 'firstRun', initial: { step: 'verify' } }),
    /** R2On05 */
    packs: (): WizardBoard => ({ snapshot: withSetup(setupView({ verify: VERIFIED })), mode: 'firstRun', initial: { step: 'packs' } }),
    /** R2On06 */
    academy: (): WizardBoard => ({ snapshot: withSetup(setupView({ verify: VERIFIED })), mode: 'firstRun', initial: { step: 'academy' } }),
    /** R2On07 */
    done: (): WizardBoard => ({ snapshot: withSetup(setupView({ verify: VERIFIED })), mode: 'firstRun', initial: { step: 'done', packs: ['runtimeSecurity'] } }),
}

/** R2AddTenant: the sheet over a shell that already has tenants, nothing drafted yet. */
export const fxAddTenant = (): WizardBoard => ({ snapshot: fxSwitcher(), mode: 'add' })
/** R2AddTenant drafted: the add sheet with an unkeyed draft (the key is optional there). */
export const fxAddTenantDrafted = (): WizardBoard => ({ snapshot: { ...fxSwitcher(), setup: setupView({ mode: 'add', scanRequired: false, tenant: draft({ label: 'Coastal Bank', tsgId: '2000000001' }) }) }, mode: 'add', initial: { step: 'tenant' } })
