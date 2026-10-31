import { useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import type { ContextRef, ManagementResult, ProductId, Region, SetupView, ShellSnapshot, SwitchResolution, TenantEnvironment, VerifyState, VerifyStep } from '@prisma-airs/desktop-contracts'
import type { Dispatch, DispatchOutcome } from '../../features/tenants/dispatch'
import { outcomeMessage } from '../../features/tenants/dispatch'
import { readManagement } from '../../features/administration/management-read'
import { AcademyAccount } from '../../features/academy/academy-account'
import { Glyph, type GlyphName } from '../icons'
import { Verdict, type VerdictTone } from '../verdict'
import { ENVIRONMENT_LABELS } from '../copy'
import { SwitchGuard } from '../switch-guard'
import { PRODUCT_CARDS, WIZARD_COPY as C } from './copy'

export type WizardStep = 'welcome' | 'tenant' | 'products' | 'verify' | 'packs' | 'academy' | 'done'
const FIRST_RUN: WizardStep[] = ['welcome', 'tenant', 'products', 'verify', 'packs', 'academy', 'done']
const ADD: WizardStep[] = ['tenant', 'products', 'verify', 'packs', 'done']

const REGIONS: { value: Region; label: string }[] = [
    { value: 'unitedStates', label: 'United States' },
    { value: 'europe', label: 'Europe' },
    { value: 'india', label: 'India' },
    { value: 'singapore', label: 'Singapore' },
]
const ENVIRONMENTS: TenantEnvironment[] = ['production', 'staging', 'other']

export interface WizardProps {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observeManagement: (operation: string) => Promise<ManagementResult>
    mode: 'firstRun' | 'add'
    /**
     * add mode: the sheet closes, with the added tenant's label once the
     * core committed it (a plain close means Cancel: the shell discards the
     * draft); firstRun: the shell takes over once onboarding is done.
     */
    onClose: (added?: string) => void
    onNavigate: (feature: 'home') => void
    /** Board fixtures and tests only: open on a later step. */
    initial?: WizardInitial
    /**
     * add mode: every close request from outside (Escape, the scrim) arrives
     * here as a count; the wizard runs the same Cancel as its button, so a
     * refused or unconfirmed discard stays visible instead of closing.
     */
    closeRequests?: number
}

/** The step a board fixture opens the wizard on (boards.md `fx.on.*`); the state itself is the snapshot's `setup`. */
export interface WizardInitial {
    step?: WizardStep
    packs?: ProductId[]
}

const toneOf = (state: VerifyState): VerdictTone => (state === 'pass' ? 'allowed' : state === 'warning' ? 'flagged' : state === 'fail' ? 'blocked' : 'info')
const UNAVAILABLE_LINES: VerifyStep[] = [
    { line: 'ais', product: null, state: 'unavailable', reason: 'Not available in this release: no admitted service (D058).' },
    { line: 'aes', product: null, state: 'unavailable', reason: 'Not available in this release: no admitted service (D058).' },
]
const fieldError = (name: 'name' | 'tsg', value: string): string => (name === 'name' ? (value.trim() ? '' : C.tenant.errors.name) : /^\d{10}$/.test(value.trim()) ? '' : C.tenant.errors.tsg)
type Status = { text: string; retry?: () => void } | null

/**
 * The first-run wizard and the add-tenant sheet (C29, screens/wizard.md).
 * Everything durable lives in the core's setup draft (`snapshot.setup`):
 * the profile, the credentials attached to the draft's own context, the
 * chosen products, the verification run and its staleness, and the
 * completion gate. This component only walks the steps and sends intents;
 * secrets are read from their fields once, cleared, and sent inside the
 * intent (provenance P25); nothing here holds them.
 */
export function Wizard(props: WizardProps) {
    const { snapshot, dispatch, mode } = props
    const steps = mode === 'add' ? ADD : FIRST_RUN
    const setup: SetupView | null = snapshot.setup && snapshot.setup.mode === mode ? snapshot.setup : null
    const context: ContextRef | null = setup?.context ?? null
    const startStep: WizardStep = props.initial?.step ?? (mode === 'firstRun' && snapshot.onboarding === 'inProgress' ? 'tenant' : (steps[0] as WizardStep))
    const startIndex = Math.max(0, steps.indexOf(startStep))
    const [index, setIndex] = useState(startIndex)
    const step = steps[index] as WizardStep
    const [visited, setVisited] = useState<Set<WizardStep>>(() => new Set<WizardStep>(steps.slice(0, startIndex + 1)))
    const [status, setStatus] = useState<Status>(null)
    const [busy, setBusy] = useState(false)
    const [finishing, setFinishing] = useState(false)
    const [packs, setPacks] = useState<ProductId[] | null>(props.initial?.packs ?? null)
    // Steps the operator skipped (the rail tags them Skipped once passed).
    const [skipped, setSkipped] = useState<Set<WizardStep>>(() => new Set())
    const [helpOpen, setHelpOpen] = useState(false)
    const [scanOpen, setScanOpen] = useState(false)
    const heading = useRef<HTMLHeadingElement>(null)
    const headingId = useId()

    useEffect(() => {
        heading.current?.focus()
    }, [step])

    // Steps after Verify stay out of reach until the current inputs verified
    // without a blocking failure (R4): an edit anywhere sends the walk back
    // through Verify; unsaved profile edits keep every later step out of reach.
    const verifyIndex = steps.indexOf('verify')
    const tenantIndex = steps.indexOf('tenant')
    const reachable = (i: number) => visited.has(steps[i] as WizardStep) && (i <= tenantIndex || !profileDirty) && (i <= verifyIndex || Boolean(setup?.completable))
    const go = (next: number) => {
        if (next < 0 || next >= steps.length) return
        setStatus(null)
        setIndex(next)
        setVisited((v) => new Set<WizardStep>([...v, steps[next] as WizardStep]))
    }
    const report = (outcome: DispatchOutcome, ok: string | null, retry?: () => void): boolean => {
        if (outcome.kind === 'admitted') {
            setStatus(ok ? { text: ok } : null)
            return true
        }
        if (outcome.kind === 'refused') setStatus({ text: outcomeMessage(outcome) ?? C.outcome.failed })
        else if (outcome.kind === 'unknown') setStatus({ text: C.outcome.unknown, retry })
        else setStatus({ text: C.outcome.failed, retry })
        return false
    }

    // ---- welcome: leaving it marks onboarding in progress (Rust-owned) ----
    async function leaveWelcome() {
        if (snapshot.onboarding !== 'needed') return go(index + 1)
        setBusy(true)
        const outcome = await dispatch({ kind: 'setOnboarding', state: 'inProgress' })
        setBusy(false)
        if (report(outcome, null, () => void leaveWelcome())) go(index + 1)
    }

    // ---- tenant: the profile form drafts or updates the core's setup ----
    const [form, setForm] = useState<{ name: string; environment: TenantEnvironment; region: Region; tsg: string }>(() => ({
        name: setup?.tenant.label ?? '',
        environment: setup?.tenant.environment ?? 'production',
        region: setup && setup.tenant.destination.kind === 'region' ? setup.tenant.destination.region : 'unitedStates',
        tsg: setup?.tenant.tsgId ?? '',
    }))
    const [errors, setErrors] = useState<Record<string, string>>({})
    const [touched, setTouched] = useState<Record<string, boolean>>({})
    const savedProfile = setup ? { name: setup.tenant.label, environment: setup.tenant.environment, region: setup.tenant.destination.kind === 'region' ? setup.tenant.destination.region : 'unitedStates', tsg: setup.tenant.tsgId ?? '' } : null
    // Displayed details that differ from the draft's, or are invalid, must be
    // saved or reverted before anything downstream (R7): the core only ever
    // verifies and commits what it holds.
    const profileDirty = Boolean(savedProfile && (form.name.trim() !== savedProfile.name || form.environment !== savedProfile.environment || form.region !== savedProfile.region || form.tsg.trim() !== savedProfile.tsg))
    const revertProfile = () => {
        if (savedProfile) setForm(savedProfile)
        setErrors({})
        setTouched({})
    }
    const scanRef = useRef<HTMLInputElement>(null)
    const secretRef = useRef<HTMLInputElement>(null)
    const workspaceKeyRef = useRef<HTMLInputElement>(null)
    const profileOf = () => ({ label: form.name.trim(), tsgId: form.tsg.trim(), destination: { kind: 'region' as const, region: form.region }, environment: form.environment })

    // A draft creation that is in flight, or whose delivery was not confirmed,
    // may still produce a draft: Cancel waits for it and discards what appears.
    const createInFlight = useRef<Promise<DispatchOutcome> | null>(null)
    // Mirrored in state so a settlement re-runs the cancel decision whatever
    // order the receipt and the draft snapshot arrive in.
    const [createPending, setCreatePending] = useState(false)
    // An admitted creation whose draft has not been observed yet: the draft is
    // coming, so a cancellation waits for it however long the snapshot takes.
    const [createAdmitted, setCreateAdmitted] = useState(false)
    const [createUnknown, setCreateUnknown] = useState<string | null>(null)
    async function submitProfile(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const next = { name: fieldError('name', form.name), tsg: fieldError('tsg', form.tsg) }
        setTouched({ name: true, tsg: true })
        setErrors(next)
        if (next.name || next.tsg) return
        setBusy(true)
        let outcome: DispatchOutcome
        if (setup) outcome = await dispatch({ kind: 'updateTenantSetup', context: setup.context, profile: profileOf() })
        else {
            const promise = dispatch({ kind: 'beginTenantSetup', mode, profile: profileOf() })
            createInFlight.current = promise
            setCreatePending(true)
            outcome = await promise
            createInFlight.current = null
            if (outcome.kind === 'unknown') setCreateUnknown(snapshot.revision)
            if (outcome.kind === 'admitted') setCreateAdmitted(true)
            setCreatePending(false)
        }
        setBusy(false)
        report(outcome, setup ? C.tenant.updated : C.tenant.created(form.name.trim()), () => void submitProfile(event))
    }
    useEffect(() => {
        // A later snapshot settles an unconfirmed creation either way; an admitted
        // one is settled by the draft it produced.
        if (createUnknown !== null && createUnknown !== snapshot.revision) setCreateUnknown(null)
        if (createAdmitted && setup) setCreateAdmitted(false)
    }, [createUnknown, createAdmitted, setup, snapshot.revision])

    async function saveScanKey(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const field = scanRef.current
        if (!field || !context) return
        const key = field.value
        field.value = ''
        if (!key) return setErrors((e) => ({ ...e, scanKey: C.tenant.errors.scanKey }))
        setErrors((e) => ({ ...e, scanKey: '' }))
        setBusy(true)
        report(await dispatch({ kind: 'configureScanCredential', context, mode: 'keychain', key }), C.tenant.keySaved)
        setBusy(false)
    }

    async function saveClient(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const secretField = secretRef.current
        if (!secretField || !context) return
        const secret = secretField.value
        secretField.value = ''
        const clientId = String(new FormData(event.currentTarget).get('clientId') ?? '').trim()
        if (!clientId) return setErrors((e) => ({ ...e, clientId: C.tenant.errors.clientId }))
        if (!secret) return setErrors((e) => ({ ...e, clientSecret: C.tenant.errors.clientSecret }))
        setErrors((e) => ({ ...e, clientId: '', clientSecret: '' }))
        setBusy(true)
        report(await dispatch({ kind: 'configureManagementCredential', context, mode: 'keychain', clientId, secret }), C.tenant.clientSaved)
        setBusy(false)
    }

    async function remove(kind: 'removeScanCredential' | 'removeManagementCredential', ok: string) {
        if (!context) return
        setBusy(true)
        report(await dispatch({ kind, context }), ok)
        setBusy(false)
    }

    // ---- Gateway workspace (optional): list through the management client, choose, key ----
    const [gatewayOn, setGatewayOn] = useState(false)
    const [workspaces, setWorkspaces] = useState<{ id: string; name: string }[] | null>(null)
    const [workspacesNote, setWorkspacesNote] = useState<string | null>(null)
    const [workspace, setWorkspace] = useState<string>('')
    // The chosen workspace is core state on the draft (its reference is what the key is configured under).
    const chosen = setup?.workspace ?? null
    const [endpoint, setEndpoint] = useState('')

    async function listWorkspaces() {
        if (!context) return
        setWorkspacesNote(C.tenant.readingWorkspaces)
        setWorkspaces(null)
        const outcome = await readManagement(dispatch, props.observeManagement, context, { kind: 'gateway', workspace: null, read: { kind: 'inventory', resource: 'workspaces' } })
        if (outcome.kind !== 'result') return setWorkspacesNote(outcome.kind === 'notAdmitted' ? outcome.message : C.outcome.failed)
        const result = outcome.result
        if (result.kind !== 'gateway' || result.gateway.kind !== 'list') return setWorkspacesNote(result.kind === 'failed' ? C.outcome.failed : C.tenant.noWorkspaces)
        const rows = (JSON.parse(result.gateway.list.rows) as Record<string, unknown>[])
            .filter((r) => typeof r.id === 'string')
            .map((r) => ({ id: String(r.id), name: typeof r.name === 'string' && r.name ? r.name : String(r.id) }))
        setWorkspaces(rows)
        setWorkspace(rows[0]?.id ?? '')
        setWorkspacesNote(rows.length ? null : C.tenant.noWorkspaces)
    }

    async function chooseWorkspace() {
        if (!context || !workspace) return
        setBusy(true)
        const outcome = await dispatch({ kind: 'selectGatewayWorkspace', context, workspace })
        setBusy(false)
        report(outcome, C.tenant.workspaceChosen(workspaces?.find((w) => w.id === workspace)?.name ?? workspace))
    }

    async function saveWorkspaceKey(event: FormEvent<HTMLFormElement>) {
        event.preventDefault()
        const field = workspaceKeyRef.current
        if (!field || !context || !chosen) return
        const secret = field.value
        field.value = ''
        if (!/^https:\/\/\S+$/.test(endpoint.trim())) return setErrors((e) => ({ ...e, endpoint: C.tenant.errors.endpoint }))
        if (!secret) return setErrors((e) => ({ ...e, workspaceKey: C.tenant.errors.workspaceKey }))
        setErrors((e) => ({ ...e, endpoint: '', workspaceKey: '' }))
        setBusy(true)
        report(await dispatch({ kind: 'configureInferenceCredential', context, mode: 'keychain', workspace: chosen.reference, endpoint: endpoint.trim(), secret }), C.tenant.workspaceKeySaved)
        setBusy(false)
    }

    const scanReady = setup?.tenant.scan.state === 'ready'
    const clientReady = setup?.tenant.management.state === 'ready'
    const tenantContinue = !setup ? { label: C.tenant.continueNeedsDraft, disabled: true } : profileDirty ? { label: C.tenant.continueDirty, disabled: true } : setup.scanRequired && !scanReady ? { label: C.tenant.continueNeedsKey, disabled: true } : { label: C.tenant.continue, disabled: false }

    // ---- products: the choice is recorded in the core as it changes ----
    async function toggleProduct(id: ProductId, on: boolean) {
        if (!setup) return
        const next = on ? [...setup.products, id] : setup.products.filter((p) => p !== id)
        report(await dispatch({ kind: 'chooseSetupProducts', context: setup.context, products: next }), null)
    }

    // ---- verify: one core-owned run; lines stream through the snapshot ----
    async function verify() {
        if (!context) return
        setBusy(true)
        report(await dispatch({ kind: 'verifyTenantSetup', context }), null, () => void verify())
        setBusy(false)
    }
    const run = setup?.verify ?? null
    const running = Boolean(run && !run.done)
    const plannedRows: VerifyStep[] = run
        ? run.steps
        : [
              { line: 'scanKey', product: 'runtimeSecurity', state: 'pending', reason: '' },
              { line: 'managementClient', product: 'runtimeSecurity', state: 'pending', reason: '' },
              ...(setup?.products ?? []).filter((p) => p !== 'runtimeSecurity').map((p): VerifyStep => ({ line: p === 'gateway' ? 'gateway' : p === 'redTeam' ? 'redTeam' : 'modelSkill', product: p, state: 'pending', reason: '' })),
              ...UNAVAILABLE_LINES,
          ]
    const verifyStatus = !run ? '' : running ? C.verify.running : run.stale ? C.verify.stale : run.blocking ? C.verify.failed : run.failed ? C.verify.failedOptional : run.warnings.length ? C.verify.finishedWarnings : C.verify.finishedClean

    // ---- packs: the chosen packs are imported by the completion intent ----
    // Only packs of currently chosen products with cases count; a product removed
    // after its pack was ticked takes the pack with it (R8).
    const offered = setup?.packs.filter((p) => p.count > 0 && setup.products.includes(p.product)).map((p) => p.product) ?? []
    const selectedPacks: ProductId[] = (packs ?? offered).filter((p) => offered.includes(p))

    // ---- done: completion is one core intent; the shell confirms it ----
    // Completion makes the new tenant active: it is the one guarded switch
    // path (I1). A pending Harness decision refuses it; the core's guard is
    // rendered here and the completion re-sent with the chosen resolution.
    async function finish(resolution: SwitchResolution | null = null) {
        if (!setup) return
        setFinishing(true)
        const outcome = await dispatch({ kind: 'completeTenantSetup', context: setup.context, packs: selectedPacks, resolution })
        if (outcome.kind === 'admitted') {
            // First run: the shell takes over once the snapshot shows onboarding
            // done (the same intent set it). Add: the sheet closes with the label.
            if (mode === 'add') props.onClose(setup.tenant.label)
            return
        }
        setFinishing(false)
        // The guard is the snapshot's to show (switchBlocked), not a status line.
        if (outcome.kind === 'refused' && outcome.refusal === 'decisionPending') return
        report(outcome, null, () => void finish())
    }
    const completionGuard = snapshot.switchBlocked?.completion && setup && snapshot.switchBlocked.to === setup.tenant.id ? snapshot.switchBlocked : null

    // ---- cancel (add mode): the draft is discarded in the core before the sheet closes ----
    // idle → awaitingCreate (a creation is pending or unconfirmed: wait for what it
    // produces) → discarding (sent; closes on admission, or when the draft is seen
    // gone after an unconfirmed delivery) → retry (refused or failed, with Retry).
    type CancelState = 'idle' | 'awaitingCreate' | 'discarding' | 'retry'
    const [cancelState, setCancelState] = useState<CancelState>('idle')
    const latestSetup = useRef(setup)
    latestSetup.current = setup
    const cancelling = cancelState === 'awaitingCreate' || cancelState === 'discarding'
    async function discardNow(context: ContextRef) {
        setCancelState('discarding')
        const outcome = await dispatch({ kind: 'discardTenantSetup', context })
        if (outcome.kind === 'admitted') return props.onClose()
        // Unconfirmed: the draft may be gone (the next snapshot closes the sheet) or
        // still here; the operator can send the discard again for the current draft.
        if (outcome.kind === 'unknown') return setStatus({ text: C.outcome.unknown, retry: () => void retryDiscard() })
        setCancelState('retry')
        setStatus({ text: `${C.cancelFailed} ${outcome.kind === 'refused' ? (outcomeMessage(outcome) ?? '') : C.outcome.failed}`.trim(), retry: () => void retryDiscard() })
    }
    async function retryDiscard() {
        const current = latestSetup.current
        if (!current) return props.onClose()
        await discardNow(current.context)
    }
    async function cancel() {
        // A close request while a discard is unconfirmed is the operator's retry.
        if (cancelState === 'discarding') return retryDiscard()
        if (cancelling) return
        if (createInFlight.current || createUnknown !== null || createAdmitted) {
            setCancelState('awaitingCreate')
            if (createInFlight.current) {
                const outcome = await createInFlight.current
                // Nothing was created: close. Admitted or unconfirmed: the next snapshot decides.
                if (outcome.kind === 'refused' || outcome.kind === 'failed') return props.onClose()
            }
            return
        }
        const current = latestSetup.current
        if (!current) return props.onClose()
        await discardNow(current.context)
    }
    useEffect(() => {
        if (cancelState !== 'awaitingCreate' || createPending) return
        if (setup) void discardNow(setup.context)
        // An admitted creation's draft is still on its way: keep waiting for it.
        else if (createAdmitted) return
        else if (createUnknown === null) props.onClose()
        // eslint-disable-next-line react-hooks/exhaustive-deps -- decided per settlement and snapshot
    }, [cancelState, setup, createUnknown, createAdmitted, createPending, snapshot.revision])
    const seenCloseRequests = useRef(props.closeRequests ?? 0)
    useEffect(() => {
        if ((props.closeRequests ?? 0) > seenCloseRequests.current) {
            seenCloseRequests.current = props.closeRequests ?? 0
            void cancel()
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps -- a request is handled once
    }, [props.closeRequests])
    useEffect(() => {
        // An unconfirmed discard that did go through shows up as the draft being gone.
        if (cancelState === 'discarding' && !setup) props.onClose()
    }, [cancelState, setup, props])
    useEffect(() => {
        // The draft's context changed (a credential or profile change): the core
        // cleared its listing and choice, so the local chooser goes with them.
        setWorkspaces(null)
        setWorkspacesNote(null)
        setWorkspace('')
    }, [context])

    const railCtx = setup ? `${setup.tenant.label} · ${ENVIRONMENT_LABELS[setup.tenant.environment]}${setup.tenant.tsgId ? ` · TSG ${setup.tenant.tsgId}` : ''}` : ''
    const productLabel = (id: ProductId) => PRODUCT_CARDS.find((c) => c.id === id)?.label ?? id
    const isAdd = mode === 'add'
    const dark = snapshot.appearance === 'dark' || (snapshot.appearance === 'system' && typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches)
    const skip = (s: WizardStep) => {
        setSkipped((x) => new Set([...x, s]))
        go(index + 1)
    }
    const unskip = (s: WizardStep) =>
        setSkipped((x) => {
            const n = new Set(x)
            n.delete(s)
            return n
        })

    // ---- the pinned footer, per step ----
    type Btn = { label: string; onClick: () => void; disabled?: boolean }
    let primary: Btn
    let secondary: Btn | undefined
    let extra: Btn | undefined
    let skipBtn: Btn | undefined
    let hint = ''
    switch (step) {
        case 'welcome':
            primary = { label: C.welcome.continue, onClick: () => void leaveWelcome(), disabled: busy }
            break
        case 'tenant':
            primary = { label: tenantContinue.label, onClick: () => go(index + 1), disabled: tenantContinue.disabled }
            hint = isAdd && !setup ? C.cancelNote : ''
            break
        case 'products':
            primary = { label: C.products.continue, onClick: () => go(index + 1) }
            break
        case 'verify':
            primary = { label: run && run.done && !run.stale && run.warnings.length > 0 && !run.blocking ? C.verify.continueWarnings : C.verify.continue, onClick: () => go(index + 1), disabled: !setup?.completable }
            secondary = { label: run ? C.verify.again : C.verify.run, onClick: () => void verify(), disabled: busy || running }
            extra = run && run.done && run.failed && !run.stale ? { label: C.verify.fix, onClick: () => go(steps.indexOf('tenant')) } : undefined
            break
        case 'packs':
            primary = { label: C.packs.continue, onClick: () => { unskip('packs'); go(index + 1) } }
            skipBtn = { label: C.packs.skip, onClick: () => { setPacks([]); skip('packs') } }
            break
        case 'academy':
            primary = snapshot.academy.state === 'signedIn' ? { label: C.academy.continue, onClick: () => { unskip('academy'); go(index + 1) } } : { label: C.academy.skip, onClick: () => skip('academy') }
            break
        default:
            primary = { label: finishing ? (isAdd ? C.done.finishing : C.done.opening) : isAdd ? C.done.finish : C.done.open, onClick: () => void finish(), disabled: finishing || !setup?.completable || profileDirty }
    }
    const showBack = index > 0

    const card = (children: ReactNode, extraStyle: React.CSSProperties = {}, rest: Record<string, unknown> = {}) => (
        <section className='flex flex-col rounded-[12px] border' style={{ gap: 12, padding: '18px 20px', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', ...extraStyle }} {...rest}>
            {children}
        </section>
    )
    const pill = (text: string, tone: 'required' | 'optional' = 'optional') => (
        <span className='rounded-full text-[12px] font-semibold' style={{ padding: '1px 7px', background: 'var(--v2-inset)', color: tone === 'required' ? 'var(--v2-soft)' : 'var(--v2-faint)' }}>
            {text}
        </span>
    )
    const tile = (name: GlyphName, size = 30) => (
        <span className='grid shrink-0 place-items-center rounded-[8px]' style={{ width: size, height: size, background: 'var(--v2-inset)', color: 'var(--v2-soft)' }}>
            <Glyph name={name} size={Math.round(size / 2)} color='var(--v2-soft)' />
        </span>
    )
    const stateTone = (state: 'missing' | 'storing' | 'ready' | 'failed'): VerdictTone => (state === 'ready' ? 'allowed' : state === 'failed' ? 'blocked' : 'info')
    const scanCard = setup && (
        <OptionalCard
            icon='shield'
            title={C.tenant.scanKey}
            text={`${setup.scanRequired ? C.tenant.scanRequired : C.tenant.scanOptional} ${C.tenant.scanHint}`}
            tag={setup.scanRequired ? C.required : C.optional}
            open={scanOpen || setup.scanRequired || scanReady}
            onToggle={setup.scanRequired ? undefined : (on) => setScanOpen(on)}
            state={
                <span data-slot='scan-state'>
                    <Verdict tone={stateTone(setup.tenant.scan.state)} word={C.tenant.state[setup.tenant.scan.state]} />
                </span>
            }
            slot='scan-key-form'
            onSubmit={(e) => void saveScanKey(e)}
            busy={busy}
        >
            <SecretField label={C.tenant.scanKey} inputRef={scanRef} error={errors.scanKey} />
            <div className='flex items-end' style={{ gap: 10 }}>
                <Primary label={scanReady ? C.tenant.replaceKey : C.tenant.saveKey} disabled={busy} />
                {scanReady && <Secondary label={C.tenant.removeKey} disabled={busy} onClick={() => void remove('removeScanCredential', C.tenant.keyRemoved)} />}
            </div>
        </OptionalCard>
    )

    return (
        <div data-slot='wizard' data-mode={mode} className='flex' style={{ height: isAdd ? 'min(780px, 84vh)' : '100vh', minHeight: 0, background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}>
            <nav aria-label='Setup steps' className='flex shrink-0 flex-col' style={{ width: isAdd ? 244 : 320, padding: isAdd ? '22px 16px 16px' : '22px 20px 20px', background: 'var(--v2-side)', borderRight: '1px solid var(--v2-rule)' }}>
                <div className='flex items-center' style={{ gap: 12, marginBottom: 28 }}>
                    <span className='grid shrink-0 place-items-center rounded-[10px]' style={{ width: 40, height: 40, background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }} data-slot='brand-tile'>
                        <Glyph name='shield' size={22} color='var(--v2-acc-ink)' />
                    </span>
                    <span className='min-w-0'>
                        <b className='text-[15px]'>{isAdd ? C.railTitleAdd : C.railTitle}</b>
                        <br />
                        <span style={{ color: 'var(--v2-faint)' }}>{isAdd ? C.railSubAdd : C.railSub}</span>
                        {railCtx && step !== 'welcome' && (
                            <>
                                <br />
                                <span data-slot='rail-context' className='inline-block text-[12px] font-semibold leading-[1.35] break-words' style={{ marginTop: 4 }}>
                                    {railCtx}
                                </span>
                            </>
                        )}
                    </span>
                </div>
                <ol className='flex flex-col' style={{ gap: 2 }}>
                    {steps.map((s, i) => {
                        const cur = s === step
                        const done = i < index && visited.has(s) && !skipped.has(s)
                        const optional = s === 'packs' || s === 'academy'
                        return (
                            <li key={s}>
                                <button
                                    type='button'
                                    aria-current={cur ? 'step' : undefined}
                                    disabled={!reachable(i)}
                                    onClick={() => reachable(i) && go(i)}
                                    className='flex w-full items-center rounded-[8px] text-left text-[13px] disabled:opacity-60'
                                    style={{ gap: 12, height: 40, padding: '0 10px', background: cur ? 'var(--v2-paper)' : 'transparent', color: 'var(--v2-ink)', fontWeight: cur ? 600 : 400 }}
                                >
                                    <span className='grid shrink-0 place-items-center rounded-full text-[11px] font-bold' style={{ width: 24, height: 24, border: `1.5px solid ${cur || done ? 'var(--v2-acc)' : 'var(--v2-ctl)'}`, background: done ? 'var(--v2-acc)' : cur ? 'var(--v2-acc-soft)' : 'transparent', color: done ? 'var(--v2-acc-ink)' : cur ? 'var(--v2-acc)' : 'var(--v2-soft)' }} data-slot='step-mark' data-done={done ? 'true' : undefined}>
                                        {done ? <Glyph name='check' size={12} color='var(--v2-acc-ink)' /> : i + 1}
                                    </span>
                                    <span className='flex-grow'>{C.steps[s]}</span>
                                    {skipped.has(s) ? (
                                        <span className='text-[12px] font-normal' style={{ color: 'var(--v2-faint)' }}>
                                            {C.skipped}
                                        </span>
                                    ) : optional ? (
                                        <span className='text-[12px] font-normal' style={{ color: 'var(--v2-faint)' }}>
                                            {C.optional}
                                        </span>
                                    ) : null}
                                </button>
                            </li>
                        )
                    })}
                </ol>
                <div className='mt-auto flex rounded-[10px] text-[12px] leading-[1.5]' style={{ gap: 8, padding: 12, background: 'var(--v2-inset)', color: 'var(--v2-soft)' }} data-slot='privacy-note'>
                    <Glyph name='lock' size={14} color='var(--v2-soft)' className='mt-[2px] shrink-0' />
                    <span>{C.privacy}</span>
                </div>
                <button type='button' onClick={() => void dispatch({ kind: 'setAppearance', theme: dark ? 'light' : 'dark' })} className='h-8 self-start rounded-[7px] border px-3 text-[12px]' style={{ marginTop: 10, borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-soft)' }} data-slot='theme-button'>
                    {dark ? C.themeDark : C.themeLight}
                </button>
            </nav>
            <div className='flex min-w-0 flex-grow flex-col'>
                <main className='min-h-0 flex-grow overflow-auto' style={{ padding: isAdd ? '26px 32px 22px' : '32px 44px 28px' }}>
                    <section aria-labelledby={headingId} className='flex flex-col' style={{ gap: 18, maxWidth: 860 }}>
                        {step === 'welcome' && (
                            <div className='flex flex-col' style={{ gap: 18, paddingTop: 30 }}>
                                <span className='grid place-items-center rounded-[18px]' style={{ width: 72, height: 72, background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}>
                                    <Glyph name='shield' size={38} color='var(--v2-acc-ink)' />
                                </span>
                                <h1 ref={heading} id={headingId} tabIndex={-1} className='font-bold' style={{ fontSize: 32, letterSpacing: '-0.02em' }}>
                                    {C.welcome.title}
                                </h1>
                                <p className='text-[15px] leading-[1.55]' style={{ maxWidth: 680, color: 'var(--v2-soft)' }}>
                                    {C.welcome.lead}
                                </p>
                                <p className='text-[14px] leading-[1.55]' style={{ maxWidth: 680, color: 'var(--v2-soft)' }}>
                                    {C.welcome.lead2}
                                </p>
                                <ul className='grid' style={{ gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 12, marginTop: 6 }}>
                                    {C.welcome.cards.map((c) => (
                                        <li key={c.title} className='flex flex-col rounded-[12px] border' style={{ gap: 10, padding: '18px 20px', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                                            <span className='grid place-items-center rounded-[8px]' style={{ width: 30, height: 30, background: 'var(--v2-acc-soft)', color: 'var(--v2-acc)' }}>
                                                <Glyph name={c.icon} size={15} color='var(--v2-acc)' />
                                            </span>
                                            <b>{c.title}</b>
                                            <span className='leading-[1.45]' style={{ color: 'var(--v2-soft)' }}>
                                                {c.body}
                                            </span>
                                        </li>
                                    ))}
                                </ul>
                            </div>
                        )}
                        {step === 'tenant' && (
                            <>
                                <div className='flex flex-wrap items-end' style={{ gap: 12 }}>
                                    <div>
                                        <h1 ref={heading} id={headingId} tabIndex={-1} className='font-bold' style={{ fontSize: 26, letterSpacing: '-0.01em' }}>
                                            {C.tenant.title}
                                        </h1>
                                        <p className='text-[14px]' style={{ marginTop: 4, color: 'var(--v2-soft)' }}>
                                            {C.tenant.lead}
                                        </p>
                                    </div>
                                </div>
                                <form onSubmit={(e) => void submitProfile(e)} aria-busy={busy} className='flex flex-col rounded-[12px] border' style={{ gap: 12, padding: '18px 20px', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }} data-slot='tenant-form'>
                                    <div className='flex items-center' style={{ gap: 8 }}>
                                        <b className='text-[14px]'>{C.tenant.card}</b>
                                        {pill(C.required, 'required')}
                                    </div>
                                    <div className='grid' style={{ gridTemplateColumns: '1fr 1fr', gap: 12 }} data-slot='field-grid'>
                                        <Field label={C.tenant.name} name='name' value={form.name} onChange={(v) => setForm((f) => ({ ...f, name: v }))} hint={C.tenant.nameHint} error={touched.name ? errors.name : ''} maxLength={64} onBlur={(v) => { setTouched((t) => ({ ...t, name: true })); setErrors((e) => ({ ...e, name: fieldError('name', v) })) }} />
                                        <Field label={C.tenant.tsg} name='tsg' value={form.tsg} onChange={(v) => setForm((f) => ({ ...f, tsg: v }))} hint={C.tenant.tsgHint} inputMode='numeric' error={touched.tsg ? errors.tsg : ''} onBlur={(v) => { setTouched((t) => ({ ...t, tsg: true })); setErrors((e) => ({ ...e, tsg: fieldError('tsg', v) })) }} />
                                        <div className='flex flex-col text-[12px] font-semibold' style={{ gap: 5, color: 'var(--v2-soft)' }}>
                                            {C.tenant.environment}
                                            <div role='radiogroup' aria-label={C.tenant.environment} className='flex w-fit rounded-[8px]' style={{ padding: 2, background: 'var(--v2-inset)' }} data-slot='environment-control'>
                                                {ENVIRONMENTS.map((env) => {
                                                    const on = form.environment === env
                                                    return (
                                                        <button key={env} type='button' role='radio' aria-checked={on} onClick={() => setForm((f) => ({ ...f, environment: env }))} className='h-8 rounded-[6px] text-[12.5px] font-semibold' style={{ padding: '0 14px', border: 0, background: on ? 'var(--v2-paper)' : 'transparent', boxShadow: on ? '0 1px 2px rgba(0,0,0,0.12)' : 'none', color: 'var(--v2-ink)' }}>
                                                            {ENVIRONMENT_LABELS[env]}
                                                        </button>
                                                    )
                                                })}
                                            </div>
                                            <span className='font-normal' style={{ color: 'var(--v2-faint)' }}>
                                                {C.tenant.environmentHint}
                                            </span>
                                        </div>
                                        <label className='flex flex-col text-[12px] font-semibold' style={{ gap: 5, color: 'var(--v2-soft)' }}>
                                            {C.tenant.region}
                                            <select name='region' aria-label={C.tenant.region} value={form.region} onChange={(e) => setForm((f) => ({ ...f, region: e.target.value as Region }))} className='h-8 min-h-0 rounded-[7px] border px-2.5 text-[13px] font-normal' style={{ borderColor: 'var(--v2-ctl)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}>
                                                {REGIONS.map((r) => (
                                                    <option key={r.value} value={r.value}>
                                                        {r.label}
                                                    </option>
                                                ))}
                                            </select>
                                            <span className='font-normal' style={{ color: 'var(--v2-faint)' }}>
                                                {C.tenant.regionHint}
                                            </span>
                                        </label>
                                    </div>
                                    {setup && (
                                        <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                            {C.tenant.updateHint}
                                        </p>
                                    )}
                                    <div className='flex items-center' style={{ gap: 10 }}>
                                        <Primary label={setup ? C.tenant.save : C.tenant.create} disabled={busy} />
                                        {profileDirty && <Secondary label={C.tenant.revert} onClick={revertProfile} />}
                                    </div>
                                </form>
                                {setup && (
                                    <>
                                        {scanCard}
                                        <form onSubmit={(e) => void saveClient(e)} aria-busy={busy} className='flex flex-col rounded-[12px] border' style={{ gap: 12, padding: '18px 20px', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }} data-slot='management-form'>
                                            <div className='flex items-center' style={{ gap: 8 }}>
                                                <b className='text-[14px]'>{C.tenant.management}</b>
                                                {pill(C.optional)}
                                                <button type='button' aria-expanded={helpOpen} onClick={() => setHelpOpen((h) => !h)} className='ml-auto text-[12px] font-semibold' style={{ color: 'var(--v2-acc)' }}>
                                                    {C.tenant.whereFind}
                                                </button>
                                            </div>
                                            {helpOpen && (
                                                <ol className='flex list-decimal flex-col rounded-[8px] leading-[1.5]' style={{ gap: 4, padding: '12px 14px 12px 32px', background: 'var(--v2-inset)', color: 'var(--v2-soft)' }}>
                                                    {C.tenant.whereSteps.map((t) => (
                                                        <li key={t}>{t}</li>
                                                    ))}
                                                </ol>
                                            )}
                                            <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                                {C.tenant.managementHint}
                                            </p>
                                            <p data-slot='management-state'>
                                                <Verdict tone={stateTone(setup.tenant.management.state)} word={C.tenant.state[setup.tenant.management.state]} reason={clientReady && setup.tenant.management.clientId ? `Client ID: ${setup.tenant.management.clientId}` : undefined} />
                                            </p>
                                            <div className='grid' style={{ gridTemplateColumns: '1fr 1fr', gap: 12 }}>
                                                <Field label={C.tenant.clientId} name='clientId' error={errors.clientId} />
                                                <SecretField label={C.tenant.clientSecret} inputRef={secretRef} error={errors.clientSecret} />
                                            </div>
                                            <div className='flex items-center' style={{ gap: 10 }}>
                                                <Primary label={clientReady ? C.tenant.replaceClient : C.tenant.saveClient} disabled={busy} />
                                                {clientReady && <Secondary label={C.tenant.removeClient} disabled={busy} onClick={() => void remove('removeManagementCredential', C.tenant.clientRemoved)} />}
                                            </div>
                                        </form>
                                        <OptionalCard icon='route' title={C.tenant.gateway} text={C.tenant.gatewayHint} tag={C.optional} open={gatewayOn} onToggle={(on) => setGatewayOn(on)} switchLabel={C.tenant.gatewayOn} slot='gateway-section'>
                                            {!clientReady && (
                                                <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                                    {C.tenant.gatewayNeedsClient}
                                                </p>
                                            )}
                                            {clientReady && (
                                                <div className='flex flex-col' style={{ gap: 10 }}>
                                                    <div className='flex items-center' style={{ gap: 10 }}>
                                                        <Secondary label={C.tenant.readWorkspaces} disabled={busy} onClick={() => void listWorkspaces()} />
                                                        {workspacesNote && (
                                                            <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                                                {workspacesNote}
                                                            </span>
                                                        )}
                                                    </div>
                                                    {workspaces && workspaces.length > 0 && (
                                                        <div className='flex items-end' style={{ gap: 10 }}>
                                                            <label className='flex flex-col text-[12px] font-semibold' style={{ gap: 5, color: 'var(--v2-soft)' }}>
                                                                {C.tenant.workspace}
                                                                <select value={workspace} onChange={(e) => setWorkspace(e.target.value)} className='h-8 min-h-0 rounded-[7px] border px-2.5 text-[13px] font-normal' style={{ borderColor: 'var(--v2-ctl)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}>
                                                                    {workspaces.map((w) => (
                                                                        <option key={w.id} value={w.id}>
                                                                            {w.name}
                                                                        </option>
                                                                    ))}
                                                                </select>
                                                            </label>
                                                            <Secondary label={C.tenant.chooseWorkspace} disabled={busy || !workspace} onClick={() => void chooseWorkspace()} />
                                                        </div>
                                                    )}
                                                    {chosen && (
                                                        <form onSubmit={(e) => void saveWorkspaceKey(e)} aria-busy={busy} className='flex flex-col' style={{ gap: 10 }} data-slot='workspace-key-form'>
                                                            <p className='text-[13px]'>{C.tenant.workspaceChosen(chosen.workspace.name)}</p>
                                                            <p data-slot='inference-state'>
                                                                <Verdict tone={setup.tenant.inference.state === 'ready' ? 'allowed' : 'info'} word={C.tenant.state[setup.tenant.inference.state]} />
                                                            </p>
                                                            <div className='grid' style={{ gridTemplateColumns: '1fr 1fr', gap: 12 }}>
                                                                <Field label={C.tenant.endpoint} name='endpoint' value={endpoint} onChange={setEndpoint} hint={C.tenant.endpointHint} error={errors.endpoint} />
                                                                <SecretField label={C.tenant.workspaceKey} inputRef={workspaceKeyRef} error={errors.workspaceKey} />
                                                            </div>
                                                            <div>
                                                                <Primary label={C.tenant.saveWorkspaceKey} disabled={busy} />
                                                            </div>
                                                        </form>
                                                    )}
                                                </div>
                                            )}
                                        </OptionalCard>
                                    </>
                                )}
                            </>
                        )}
                        {step === 'products' && setup && (
                            <>
                                <div>
                                    <h1 ref={heading} id={headingId} tabIndex={-1} className='font-bold' style={{ fontSize: 26, letterSpacing: '-0.01em' }}>
                                        {C.products.title}
                                    </h1>
                                    <p className='text-[14px]' style={{ marginTop: 4, color: 'var(--v2-soft)' }}>
                                        {C.products.lead(setup.tenant.label)}
                                    </p>
                                </div>
                                <div className='grid' style={{ gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 12 }} data-slot='product-cards'>
                                    {PRODUCT_CARDS.map((card) => {
                                        const unavailable = card.id === null
                                        const always = card.id === 'runtimeSecurity'
                                        const on = card.id !== null && setup.products.includes(card.id)
                                        const reads = card.id === null ? null : card.id === 'runtimeSecurity' ? (scanReady ? C.products.readsWith.scanReady : C.products.readsWith.scanMissing) : clientReady ? C.products.readsWith.clientReady : C.products.readsWith.clientMissing
                                        const lastStep = run && !run.stale && card.id ? run.steps.find((s) => s.product === card.id && s.line !== 'managementClient') : undefined
                                        const missing = reads === C.products.readsWith.clientMissing || reads === C.products.readsWith.scanMissing
                                        return (
                                            <button
                                                key={card.label}
                                                type='button'
                                                role='checkbox'
                                                aria-checked={on}
                                                aria-disabled={unavailable || always ? true : undefined}
                                                aria-label={card.label}
                                                aria-describedby={`${card.label}-body`}
                                                onClick={() => card.id && !always && void toggleProduct(card.id, !on)}
                                                className='relative flex flex-col text-left'
                                                style={{ gap: 10, minHeight: 176, padding: '18px 20px', border: `1.5px solid ${on ? 'var(--v2-acc)' : 'var(--v2-rule)'}`, borderRadius: 12, background: on ? 'var(--v2-acc-soft)' : 'var(--v2-paper)', color: 'var(--v2-ink)', opacity: unavailable ? 0.7 : 1, cursor: unavailable || always ? 'default' : 'pointer' }}
                                                data-slot='product-card'
                                            >
                                                <span className='absolute grid place-items-center rounded-full' style={{ top: 14, right: 14, width: 20, height: 20, border: `1.5px solid ${on ? 'var(--v2-acc)' : 'var(--v2-ctl)'}`, background: on ? 'var(--v2-acc)' : 'transparent', color: 'var(--v2-acc-ink)' }}>
                                                    {on && <Glyph name='check' size={11} color='var(--v2-acc-ink)' />}
                                                </span>
                                                <span className='grid place-items-center rounded-[10px]' style={{ width: 36, height: 36, background: on ? 'var(--v2-acc)' : 'var(--v2-inset)', color: on ? 'var(--v2-acc-ink)' : 'var(--v2-soft)' }}>
                                                    <Glyph name={card.icon} size={18} color={on ? 'var(--v2-acc-ink)' : 'var(--v2-soft)'} />
                                                </span>
                                                <span className='block leading-[1.35]' style={{ paddingRight: 26 }}>
                                                    <b className='text-[14px]'>{card.label}</b>
                                                </span>
                                                <span id={`${card.label}-body`} className='leading-[1.45]' style={{ color: 'var(--v2-soft)' }}>
                                                    {card.body}
                                                </span>
                                                <span className='mt-auto flex flex-wrap' style={{ gap: 6 }}>
                                                    {unavailable && <Verdict tone='info' word={C.products.unavailable} />}
                                                    {always && <Verdict tone='info' word={C.products.always} />}
                                                    {card.readOnly && <Verdict tone='info' word={C.products.readOnly} />}
                                                    {reads && !unavailable && <Verdict tone={missing ? 'flagged' : 'allowed'} word={reads} />}
                                                    {lastStep && lastStep.state !== 'pending' && <Verdict tone={toneOf(lastStep.state)} word={C.verify.state[lastStep.state]} />}
                                                </span>
                                            </button>
                                        )
                                    })}
                                </div>
                                {!scanReady && (
                                    <div className='flex items-center rounded-[8px] leading-[1.45]' style={{ gap: 8, padding: '10px 12px', background: 'var(--v2-flag-bg)' }} data-slot='no-scan-banner'>
                                        <Glyph name='alert' size={15} color='var(--v2-flag)' className='shrink-0' />
                                        <span className='flex-grow'>{C.products.noScan}</span>
                                        <Secondary label={C.products.addScanKey} onClick={() => go(steps.indexOf('tenant'))} />
                                    </div>
                                )}
                                <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                    {C.products.licence}
                                </p>
                            </>
                        )}
                        {step === 'verify' && setup && (
                            <>
                                <div>
                                    <h1 ref={heading} id={headingId} tabIndex={-1} className='font-bold' style={{ fontSize: 26, letterSpacing: '-0.01em' }}>
                                        {C.verify.title}
                                    </h1>
                                    <p className='text-[14px]' style={{ marginTop: 4, color: 'var(--v2-soft)' }}>
                                        {C.verify.lead(setup.tenant.label)}
                                    </p>
                                </div>
                                <div role='status' aria-live='polite' className='flex items-center rounded-[12px]' style={{ gap: 12, padding: '14px 16px', background: !run ? 'var(--v2-inset)' : run.stale || run.blocking ? 'var(--v2-block-bg)' : running ? 'var(--v2-acc-soft)' : run.failed || run.warnings.length ? 'var(--v2-flag-bg)' : 'var(--v2-ok-bg)' }} data-slot='verify-banner'>
                                    <Glyph name={!run || running ? 'clock' : run.stale || run.blocking ? 'ban' : run.failed || run.warnings.length ? 'alert' : 'check'} size={22} color={!run ? 'var(--v2-soft)' : run.stale || run.blocking ? 'var(--v2-block)' : running ? 'var(--v2-acc)' : run.failed || run.warnings.length ? 'var(--v2-flag)' : 'var(--v2-ok)'} className='shrink-0' />
                                    <span className='flex-grow'>
                                        <b className='text-[14px]'>{!run ? C.verify.notRunTitle : verifyStatus}</b>
                                        <br />
                                        <span style={{ color: 'var(--v2-soft)' }}>{!run ? C.verify.notRunText : running ? C.verify.runningText : run.stale ? C.verify.staleText : run.blocking ? C.verify.failedText : run.failed ? C.verify.failedOptionalText : C.verify.doneText}</span>
                                    </span>
                                    {run && run.done && run.failed && !run.stale && <Secondary label={C.verify.editCredentials} onClick={() => go(steps.indexOf('tenant'))} />}
                                </div>
                                <ul className='flex flex-col overflow-hidden rounded-[12px] border' aria-label='Checks' aria-busy={running} style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', opacity: run?.stale ? 0.6 : 1 }}>
                                    {plannedRows.map((r, i) => {
                                        const tone = run || r.state === 'unavailable' ? toneOf(r.state) : 'info'
                                        const colors: [string, string, GlyphName] = tone === 'allowed' ? ['var(--v2-ok-bg)', 'var(--v2-ok)', 'check'] : tone === 'flagged' ? ['var(--v2-flag-bg)', 'var(--v2-flag)', 'alert'] : tone === 'blocked' ? ['var(--v2-block-bg)', 'var(--v2-block)', 'x'] : ['var(--v2-inset)', 'var(--v2-soft)', running && r.state === 'pending' ? 'clock' : 'dot']
                                        return (
                                            <li key={r.line} className='flex items-center' style={{ gap: 12, padding: '12px 18px', borderTop: i ? '1px solid var(--v2-rule)' : 0 }} data-slot={`verify-${r.line}`}>
                                                <span className='grid shrink-0 place-items-center rounded-full' style={{ width: 26, height: 26, background: colors[0], color: colors[1] }}>
                                                    <Glyph name={colors[2]} size={13} color={colors[1]} />
                                                </span>
                                                <span className='min-w-0 flex-grow'>
                                                    <b className='font-semibold'>{C.verify.rows[r.line]}</b>
                                                    <br />
                                                    <span className='leading-[1.45]' style={{ color: tone === 'blocked' ? 'var(--v2-block)' : tone === 'flagged' ? 'var(--v2-flag)' : 'var(--v2-soft)' }}>
                                                        {r.reason}
                                                    </span>
                                                </span>
                                                <Verdict tone={tone} word={run || r.state === 'unavailable' ? C.verify.state[r.state] : C.verify.notRun} />
                                            </li>
                                        )
                                    })}
                                </ul>
                            </>
                        )}
                        {step === 'packs' && setup && (
                            <>
                                <div>
                                    <h1 ref={heading} id={headingId} tabIndex={-1} className='font-bold' style={{ fontSize: 26, letterSpacing: '-0.01em' }}>
                                        {C.packs.title}
                                    </h1>
                                    <p className='text-[14px]' style={{ marginTop: 4, color: 'var(--v2-soft)' }}>
                                        {C.packs.lead(setup.tenant.label)}
                                    </p>
                                </div>
                                {setup.packs.some((p) => p.count > 0) ? (
                                    <div role='group' aria-label={C.packs.title} className='flex flex-col overflow-hidden rounded-[12px] border' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }} data-slot='pack-list'>
                                        {setup.packs.map((p, i) => {
                                            const on = selectedPacks.includes(p.product)
                                            const off = p.count === 0 || !setup.products.includes(p.product)
                                            const cardInfo = PRODUCT_CARDS.find((c) => c.id === p.product)
                                            return (
                                                <button
                                                    key={p.product}
                                                    type='button'
                                                    role='checkbox'
                                                    aria-checked={on}
                                                    aria-disabled={off ? true : undefined}
                                                    aria-label={C.packs.pack(productLabel(p.product), p.count)}
                                                    onClick={() => !off && setPacks(on ? selectedPacks.filter((x) => x !== p.product) : [...selectedPacks, p.product])}
                                                    className='flex w-full items-center text-left'
                                                    style={{ gap: 14, padding: '12px 18px', border: 0, borderTop: i ? '1px solid var(--v2-rule)' : 0, background: 'transparent', color: 'var(--v2-ink)', opacity: off ? 0.55 : 1 }}
                                                    data-slot='pack-row'
                                                >
                                                    <span className='grid shrink-0 place-items-center rounded-[5px]' style={{ width: 18, height: 18, border: `1.5px solid ${on ? 'var(--v2-acc)' : 'var(--v2-ctl)'}`, background: on ? 'var(--v2-acc)' : 'transparent', color: 'var(--v2-acc-ink)' }}>
                                                        {on && <Glyph name='check' size={11} color='var(--v2-acc-ink)' />}
                                                    </span>
                                                    {tile(cardInfo?.icon ?? 'cases')}
                                                    <span className='min-w-0 flex-grow'>
                                                        <b className='font-semibold'>{C.packs.name(productLabel(p.product))}</b> <span style={{ color: 'var(--v2-faint)' }}>· {productLabel(p.product)}</span>
                                                        <br />
                                                        <span style={{ color: 'var(--v2-soft)' }}>{cardInfo?.body ?? ''}</span>
                                                    </span>
                                                    <span className='text-[12px] whitespace-nowrap' style={{ color: 'var(--v2-faint)' }}>
                                                        {C.packs.count(p.count)}
                                                    </span>
                                                </button>
                                            )
                                        })}
                                    </div>
                                ) : (
                                    <p className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                                        {C.packs.none}
                                    </p>
                                )}
                                <p style={{ color: 'var(--v2-soft)' }} data-slot='pack-summary'>
                                    {C.packs.summary(selectedPacks.length, selectedPacks.reduce((a, id) => a + (setup.packs.find((p) => p.product === id)?.count ?? 0), 0))}
                                </p>
                            </>
                        )}
                        {step === 'academy' && (
                            <>
                                <div>
                                    <h1 ref={heading} id={headingId} tabIndex={-1} className='font-bold' style={{ fontSize: 26, letterSpacing: '-0.01em' }}>
                                        {C.academy.title}
                                    </h1>
                                    <p className='text-[14px]' style={{ marginTop: 4, color: 'var(--v2-soft)' }}>
                                        {C.academy.lead}
                                    </p>
                                </div>
                                <div className='grid' style={{ gridTemplateColumns: 'minmax(0, 1fr) minmax(0, 1fr)', gap: 14 }}>
                                    {card(
                                        <>
                                            <b className='text-[14px]'>{C.academy.account}</b>
                                            <AcademyAccount view={snapshot.academy} dispatch={dispatch} />
                                        </>,
                                        {},
                                        { 'data-slot': 'academy-card' }
                                    )}
                                    {card(
                                        <>
                                            <b className='text-[14px]'>{C.academy.whatSyncs}</b>
                                            <ul className='flex list-disc flex-col pl-5 text-[13px]' style={{ gap: 6, color: 'var(--v2-soft)' }}>
                                                {C.academy.syncs.map((t) => (
                                                    <li key={t}>{t}</li>
                                                ))}
                                            </ul>
                                        </>,
                                        { gap: 10 },
                                        { 'data-slot': 'academy-syncs' }
                                    )}
                                </div>
                            </>
                        )}
                        {step === 'done' && setup && (
                            <div className='flex flex-col' style={{ gap: 16, paddingTop: 10 }}>
                                <span className='grid place-items-center rounded-[18px]' style={{ width: 64, height: 64, background: 'var(--v2-ok-bg)', color: 'var(--v2-ok)' }}>
                                    <Glyph name='check' size={34} color='var(--v2-ok)' />
                                </span>
                                <h1 ref={heading} id={headingId} tabIndex={-1} className='font-bold' style={{ fontSize: 30, letterSpacing: '-0.02em' }}>
                                    {C.done.title(setup.tenant.label)}
                                </h1>
                                <p className='text-[15px]' style={{ color: 'var(--v2-soft)' }}>
                                    {C.done.lead}
                                </p>
                                <div className='flex flex-col overflow-hidden rounded-[12px] border' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }} data-slot='summary'>
                                    {(
                                        [
                                            [C.done.summary.tenant, railCtx, 'ok', 'tenant'],
                                            [C.done.summary.scan, scanReady ? C.done.summary.saved : C.done.summary.notSaved, scanReady ? 'ok' : 'skip', 'tenant'],
                                            [C.done.summary.client, clientReady ? C.done.summary.saved : C.done.summary.notSaved, clientReady ? 'ok' : 'skip', 'tenant'],
                                            [C.done.summary.products, setup.products.map(productLabel).join(', '), 'ok', 'products'],
                                            [C.done.summary.checks, run && run.done ? (run.warnings.length ? C.done.summary.checksWarnings(run.warnings.length) : C.done.summary.checksPassed) : C.done.summary.none, run?.warnings.length ? 'warn' : 'ok', 'verify'],
                                            [C.done.summary.packs, selectedPacks.length ? selectedPacks.map(productLabel).join(', ') : C.done.summary.none, selectedPacks.length ? 'ok' : 'skip', 'packs'],
                                            ...(isAdd ? [] : [[C.done.summary.academy, snapshot.academy.state === 'signedIn' ? snapshot.academy.displayName : C.done.summary.academySkipped, snapshot.academy.state === 'signedIn' ? 'ok' : 'skip', 'academy']]),
                                        ] as [string, string, 'ok' | 'warn' | 'skip', WizardStep][]
                                    ).map(([k, v, tone, target], i) => (
                                        <div key={k} className='flex items-center' style={{ gap: 12, padding: '12px 18px', borderTop: i ? '1px solid var(--v2-rule)' : 0 }}>
                                            <span className='grid shrink-0 place-items-center rounded-full' style={{ width: 26, height: 26, background: tone === 'ok' ? 'var(--v2-ok-bg)' : tone === 'warn' ? 'var(--v2-flag-bg)' : 'var(--v2-inset)', color: tone === 'ok' ? 'var(--v2-ok)' : tone === 'warn' ? 'var(--v2-flag)' : 'var(--v2-soft)' }}>
                                                <Glyph name={tone === 'ok' ? 'check' : tone === 'warn' ? 'alert' : 'dot'} size={13} color={tone === 'ok' ? 'var(--v2-ok)' : tone === 'warn' ? 'var(--v2-flag)' : 'var(--v2-soft)'} />
                                            </span>
                                            <span className='font-semibold' style={{ width: 130 }}>
                                                {k}
                                            </span>
                                            <span className='sr-only'>: </span>
                                            <span className='min-w-0 flex-grow' style={{ color: 'var(--v2-soft)' }}>
                                                {v}
                                            </span>
                                            <button type='button' onClick={() => reachable(steps.indexOf(target)) && go(steps.indexOf(target))} className='text-[12px] font-semibold' style={{ color: 'var(--v2-acc)' }} aria-label={`${C.done.change} ${k}`}>
                                                {C.done.change}
                                            </button>
                                        </div>
                                    ))}
                                </div>
                                <div className='grid' style={{ gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 12 }} data-slot='next-up'>
                                    {C.done.nextUp.map((n) => (
                                        <div key={n.title} className='flex flex-col rounded-[12px] border' style={{ gap: 8, padding: '18px 20px', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                                            <span className='grid place-items-center rounded-[8px]' style={{ width: 28, height: 28, background: 'var(--v2-acc-soft)', color: 'var(--v2-acc)' }}>
                                                <Glyph name={n.icon} size={14} color='var(--v2-acc)' />
                                            </span>
                                            <b>{n.title}</b>
                                            <span className='leading-[1.45]' style={{ color: 'var(--v2-soft)' }}>
                                                {n.text}
                                            </span>
                                        </div>
                                    ))}
                                </div>
                            </div>
                        )}
                    </section>
                </main>
                <footer className='flex shrink-0 flex-wrap items-center' style={{ gap: 10, padding: isAdd ? '14px 32px' : '14px 44px', borderTop: '1px solid var(--v2-rule)', background: 'var(--v2-bg)' }} data-slot='wizard-footer'>
                    {isAdd && (
                        <button type='button' onClick={() => void cancel()} disabled={cancelling} className='h-8 rounded-[7px] border px-3.5 text-[13px] disabled:opacity-60' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                            {cancelling ? C.cancelling : C.cancel}
                        </button>
                    )}
                    {showBack && (
                        <button type='button' onClick={() => go(index - 1)} className='h-8 rounded-[7px] border px-3.5 text-[13px]' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                            {C.back}
                        </button>
                    )}
                    <p role='status' aria-live='polite' data-slot='wizard-status' className='flex min-w-0 items-center text-[12.5px]' style={{ gap: 10, color: status ? 'var(--v2-ink)' : 'var(--v2-soft)' }}>
                        <span className='min-w-0'>{status?.text ?? hint}</span>
                        {status?.retry && <Secondary label={C.outcome.retry} onClick={status.retry} />}
                    </p>
                    <span className='ml-auto text-[12px]' style={{ color: 'var(--v2-faint)' }} data-slot='wizard-progress'>
                        {C.progress(index + 1, steps.length)}
                    </span>
                    {extra && <Secondary label={extra.label} onClick={extra.onClick} disabled={extra.disabled} />}
                    {secondary && <Secondary label={secondary.label} onClick={secondary.onClick} disabled={secondary.disabled} />}
                    {skipBtn && <Secondary label={skipBtn.label} onClick={skipBtn.onClick} disabled={skipBtn.disabled} />}
                    <button type='button' onClick={primary.onClick} disabled={primary.disabled} className='h-8 rounded-[7px] px-[18px] text-[13px] font-semibold disabled:opacity-60' style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }} data-slot='wizard-primary'>
                        {primary.label}
                    </button>
                </footer>
            </div>
            {/* The guard is a dialog above the sheet (I8): rendered at the body, outside the sheet's inert subtree. */}
            {completionGuard && setup && createPortal(<SwitchGuard target={setup.tenant.label} completion onStay={() => void dispatch({ kind: 'dismissSwitchGuard' })} onResolve={(resolution) => void finish(resolution)} />, document.body)}
        </div>
    )
}

/** A section card with a switch in its header (R2Onboard's optional cards); a required card has no switch and stays open. */
function OptionalCard({ icon, title, text, tag, open, onToggle, switchLabel, state, slot, onSubmit, busy, children }: { icon: GlyphName; title: string; text: string; tag: string; open: boolean; onToggle?: (on: boolean) => void; switchLabel?: string; state?: ReactNode; slot: string; onSubmit?: (e: FormEvent<HTMLFormElement>) => void; busy?: boolean; children: ReactNode }) {
    const Tag = onSubmit ? 'form' : 'section'
    return (
        <Tag onSubmit={onSubmit} aria-busy={onSubmit ? busy : undefined} aria-label={onSubmit ? undefined : title} className='flex flex-col rounded-[12px] border' style={{ gap: 12, padding: '18px 20px', borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }} data-slot={slot}>
            <div className='flex items-center' style={{ gap: 12 }}>
                <span className='grid shrink-0 place-items-center rounded-[8px]' style={{ width: 30, height: 30, background: 'var(--v2-inset)', color: 'var(--v2-soft)' }}>
                    <Glyph name={icon} size={15} color='var(--v2-soft)' />
                </span>
                <span className='min-w-0 flex-grow'>
                    <span className='flex items-center' style={{ gap: 8 }}>
                        <b className='text-[14px]'>{title}</b>
                        <span className='rounded-full text-[12px] font-semibold' style={{ padding: '1px 7px', background: 'var(--v2-inset)', color: 'var(--v2-faint)' }}>
                            {tag}
                        </span>
                        {state}
                    </span>
                    <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                        {text}
                    </span>
                </span>
                {onToggle && (
                    <button type='button' role='switch' aria-checked={open} aria-label={switchLabel ?? title} onClick={() => onToggle(!open)} className='relative shrink-0 rounded-full' style={{ width: 38, height: 22, border: 0, background: open ? 'var(--v2-acc)' : 'var(--v2-inset)', boxShadow: `inset 0 0 0 1px ${open ? 'var(--v2-acc)' : 'var(--v2-ctl)'}` }} data-slot='switch'>
                        <span className='absolute rounded-full' style={{ top: 3, left: open ? 19 : 3, width: 16, height: 16, background: open ? 'var(--v2-acc-ink)' : 'var(--v2-paper)' }} />
                    </button>
                )}
            </div>
            {open && (
                <div className='flex flex-col' style={{ gap: 10, paddingLeft: 42 }}>
                    {children}
                </div>
            )}
        </Tag>
    )
}

function Field({ label, name, hint, error, onBlur, value, onChange, ...rest }: { label: string; name: string; hint?: string; error?: string; onBlur?: (value: string) => void; value?: string; onChange?: (value: string) => void; maxLength?: number; inputMode?: 'numeric' }) {
    const id = useId()
    return (
        <label className='flex flex-col text-[12px] font-semibold' style={{ gap: 5, color: 'var(--v2-soft)' }}>
            {label}
            <input id={id} name={name} autoComplete='off' value={value} onChange={onChange ? (e) => onChange(e.target.value) : undefined} aria-invalid={error ? true : undefined} aria-describedby={error ? `${id}-error` : hint ? `${id}-hint` : undefined} onBlur={(e) => onBlur?.(e.target.value)} className='h-8 min-h-0 rounded-[7px] border px-2.5 text-[13px] font-normal' style={{ borderColor: error ? 'var(--v2-block)' : 'var(--v2-ctl)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }} {...rest} />
            {error ? (
                <span id={`${id}-error`} role='alert' className='font-normal' style={{ color: 'var(--v2-block)' }}>
                    {error}
                </span>
            ) : hint ? (
                <span id={`${id}-hint`} className='font-normal' style={{ color: 'var(--v2-faint)' }}>
                    {hint}
                </span>
            ) : null}
        </label>
    )
}

/** A password field read once through its ref and cleared by the caller (P25). */
function SecretField({ label, inputRef, error }: { label: string; inputRef: React.RefObject<HTMLInputElement | null>; error?: string }) {
    const id = useId()
    return (
        <label className='flex flex-col text-[12px] font-semibold' style={{ gap: 5, color: 'var(--v2-soft)' }}>
            {label}
            <input id={id} ref={inputRef} type='password' autoComplete='off' aria-invalid={error ? true : undefined} aria-describedby={error ? `${id}-error` : undefined} className='h-8 min-h-0 rounded-[7px] border px-2.5 text-[13px] font-normal' style={{ borderColor: error ? 'var(--v2-block)' : 'var(--v2-ctl)', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }} />
            {error && (
                <span id={`${id}-error`} role='alert' className='font-normal' style={{ color: 'var(--v2-block)' }}>
                    {error}
                </span>
            )}
        </label>
    )
}

function Primary({ label, disabled }: { label: string; disabled?: boolean }) {
    return (
        <button type='submit' disabled={disabled} className='h-8 rounded-[7px] px-4 text-[13px] font-semibold disabled:opacity-60' style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}>
            {label}
        </button>
    )
}

function Secondary({ label, onClick, disabled }: { label: string; onClick: () => void; disabled?: boolean }) {
    return (
        <button type='button' onClick={onClick} disabled={disabled} className='h-8 rounded-[7px] border px-3 text-[13px] disabled:opacity-60' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', color: 'var(--v2-ink)' }}>
            {label}
        </button>
    )
}
