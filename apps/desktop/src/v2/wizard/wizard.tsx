import { useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import type { ContextRef, ManagementResult, ProductId, Region, SetupView, ShellSnapshot, SwitchResolution, TenantEnvironment, VerifyState, VerifyStep } from '@prisma-airs/desktop-contracts'
import type { Dispatch, DispatchOutcome } from '../../features/tenants/dispatch'
import { outcomeMessage } from '../../features/tenants/dispatch'
import { readManagement } from '../../features/administration/management-read'
import { AcademyAccount } from '../../features/academy/academy-account'
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
    onNavigate: (feature: 'import' | 'home') => void
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

    return (
        <div data-slot='wizard' data-mode={mode} className='flex' style={{ minHeight: mode === 'add' ? 'min(780px, 84vh)' : '100vh', background: 'var(--v2-bg)', color: 'var(--v2-ink)' }}>
            <nav aria-label='Setup steps' className='flex shrink-0 flex-col gap-1 p-6' style={{ width: mode === 'add' ? 244 : 320, background: 'var(--v2-side)', borderRight: '1px solid var(--v2-rule)' }}>
                <p className='text-[15px] font-bold'>{mode === 'add' ? C.railTitleAdd : C.railTitle}</p>
                <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                    {mode === 'add' ? C.railSubAdd : C.railSub}
                </p>
                {railCtx && step !== 'welcome' && (
                    <p data-slot='rail-context' className='mt-1 text-[12px] font-semibold'>
                        {railCtx}
                    </p>
                )}
                <ol className='mt-4 flex flex-col gap-1'>
                    {steps.map((s, i) => (
                        <li key={s}>
                            <button
                                type='button'
                                aria-current={s === step ? 'step' : undefined}
                                disabled={!reachable(i)}
                                onClick={() => reachable(i) && go(i)}
                                className='flex min-h-9 w-full items-center gap-2 rounded-[7px] px-2.5 text-left text-[13px] disabled:opacity-60'
                                style={{ background: s === step ? 'var(--v2-sel)' : 'transparent', color: s === step ? 'var(--v2-sel-ink)' : 'var(--v2-ink)' }}
                            >
                                <span className='grid size-5 place-items-center rounded-full text-[11px] font-bold' style={{ border: '1px solid currentColor' }}>
                                    {i + 1}
                                </span>
                                {C.steps[s]}
                            </button>
                        </li>
                    ))}
                </ol>
                {mode === 'add' && (
                    <div className='mt-auto flex flex-col gap-2 pt-4'>
                        <p className='text-[11px]' style={{ color: 'var(--v2-faint)' }}>
                            {C.cancelNote}
                        </p>
                        <button type='button' onClick={() => void cancel()} disabled={cancelling} className='h-8 rounded-[7px] border text-[12px] disabled:opacity-60' style={{ borderColor: 'var(--v2-rule)' }}>
                            {cancelling ? C.cancelling : C.cancel}
                        </button>
                    </div>
                )}
            </nav>
            <main className='min-w-0 flex-grow' style={{ padding: mode === 'add' ? '34px 44px 24px' : '32px 44px 28px', maxWidth: 'var(--v2-reading)' }}>
                <section aria-labelledby={headingId} className='flex flex-col gap-4'>
                    {step === 'welcome' && (
                        <Step heading={heading} id={headingId} title={C.welcome.title} lead={C.welcome.lead}>
                            <ul className='grid gap-3' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))' }}>
                                {C.welcome.cards.map((c) => (
                                    <li key={c.title} className='rounded-[12px] border p-5' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)' }}>
                                        <p className='text-[14px] font-semibold'>{c.title}</p>
                                        <p className='mt-2 text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                                            {c.body}
                                        </p>
                                    </li>
                                ))}
                            </ul>
                            <Actions primary={{ label: C.welcome.continue, onClick: () => void leaveWelcome(), disabled: busy }} />
                        </Step>
                    )}
                    {step === 'tenant' && (
                        <Step heading={heading} id={headingId} title={C.tenant.title} lead={C.tenant.lead}>
                            <form onSubmit={(e) => void submitProfile(e)} aria-busy={busy} className='flex flex-col gap-3' data-slot='tenant-form'>
                                <Field label={C.tenant.name} name='name' value={form.name} onChange={(v) => setForm((f) => ({ ...f, name: v }))} hint={C.tenant.nameHint} error={touched.name ? errors.name : ''} maxLength={64} onBlur={(v) => { setTouched((t) => ({ ...t, name: true })); setErrors((e) => ({ ...e, name: fieldError('name', v) })) }} />
                                <div className='flex flex-wrap gap-3'>
                                    <label className='flex flex-col gap-1 text-[13px] font-medium'>
                                        {C.tenant.environment}
                                        <select name='environment' value={form.environment} onChange={(e) => setForm((f) => ({ ...f, environment: e.target.value as TenantEnvironment }))} className='h-8 px-2'>
                                            {ENVIRONMENTS.map((env) => (
                                                <option key={env} value={env}>
                                                    {ENVIRONMENT_LABELS[env]}
                                                </option>
                                            ))}
                                        </select>
                                    </label>
                                    <label className='flex flex-col gap-1 text-[13px] font-medium'>
                                        {C.tenant.region}
                                        <select name='region' value={form.region} onChange={(e) => setForm((f) => ({ ...f, region: e.target.value as Region }))} className='h-8 px-2'>
                                            {REGIONS.map((r) => (
                                                <option key={r.value} value={r.value}>
                                                    {r.label}
                                                </option>
                                            ))}
                                        </select>
                                    </label>
                                </div>
                                <Field label={C.tenant.tsg} name='tsg' value={form.tsg} onChange={(v) => setForm((f) => ({ ...f, tsg: v }))} hint={C.tenant.tsgHint} inputMode='numeric' error={touched.tsg ? errors.tsg : ''} onBlur={(v) => { setTouched((t) => ({ ...t, tsg: true })); setErrors((e) => ({ ...e, tsg: fieldError('tsg', v) })) }} />
                                {setup && (
                                    <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                        {C.tenant.updateHint}
                                    </p>
                                )}
                                <div className='flex items-center gap-3'>
                                    <Primary label={setup ? C.tenant.save : C.tenant.create} disabled={busy} />
                                    {profileDirty && <Secondary label={C.tenant.revert} onClick={revertProfile} />}
                                    <button type='button' onClick={() => props.onNavigate('import')} className='text-[13px] underline' style={{ color: 'var(--v2-acc)' }} title={C.tenant.importHint}>
                                        {C.tenant.importV1}
                                    </button>
                                </div>
                            </form>
                            {setup && (
                                <div className='flex flex-col gap-5'>
                                    <form onSubmit={(e) => void saveScanKey(e)} aria-busy={busy} className='flex flex-col gap-2' data-slot='scan-key-form'>
                                        <p className='text-[14px] font-semibold'>{C.tenant.scanKey}</p>
                                        <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                            {setup.scanRequired ? C.tenant.scanRequired : C.tenant.scanOptional} {C.tenant.scanHint}
                                        </p>
                                        <p data-slot='scan-state'>
                                            <Verdict tone={scanReady ? 'allowed' : setup.tenant.scan.state === 'failed' ? 'blocked' : 'info'} word={C.tenant.state[setup.tenant.scan.state]} />
                                        </p>
                                        <SecretField label={C.tenant.scanKey} inputRef={scanRef} error={errors.scanKey} />
                                        <div className='flex items-center gap-3'>
                                            <Primary label={scanReady ? C.tenant.replaceKey : C.tenant.saveKey} disabled={busy} />
                                            {scanReady && <Secondary label={C.tenant.removeKey} disabled={busy} onClick={() => void remove('removeScanCredential', C.tenant.keyRemoved)} />}
                                        </div>
                                    </form>
                                    <form onSubmit={(e) => void saveClient(e)} aria-busy={busy} className='flex flex-col gap-2' data-slot='management-form'>
                                        <p className='text-[14px] font-semibold'>{C.tenant.management}</p>
                                        <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                            {C.tenant.managementHint}
                                        </p>
                                        <p data-slot='management-state'>
                                            <Verdict tone={clientReady ? 'allowed' : setup.tenant.management.state === 'failed' ? 'blocked' : 'info'} word={C.tenant.state[setup.tenant.management.state]} reason={clientReady && setup.tenant.management.clientId ? `Client ID: ${setup.tenant.management.clientId}` : undefined} />
                                        </p>
                                        <Field label={C.tenant.clientId} name='clientId' error={errors.clientId} />
                                        <SecretField label={C.tenant.clientSecret} inputRef={secretRef} error={errors.clientSecret} />
                                        <div className='flex items-center gap-3'>
                                            <Primary label={clientReady ? C.tenant.replaceClient : C.tenant.saveClient} disabled={busy} />
                                            {clientReady && <Secondary label={C.tenant.removeClient} disabled={busy} onClick={() => void remove('removeManagementCredential', C.tenant.clientRemoved)} />}
                                        </div>
                                    </form>
                                    <section className='flex flex-col gap-2' data-slot='gateway-section' aria-label={C.tenant.gateway}>
                                        <p className='text-[14px] font-semibold'>{C.tenant.gateway}</p>
                                        <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                            {C.tenant.gatewayHint}
                                        </p>
                                        <label className='flex items-center gap-2 text-[13px]'>
                                            <input type='checkbox' checked={gatewayOn} onChange={(e) => setGatewayOn(e.target.checked)} />
                                            {C.tenant.gatewayOn}
                                        </label>
                                        {gatewayOn && !clientReady && (
                                            <p className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                                {C.tenant.gatewayNeedsClient}
                                            </p>
                                        )}
                                        {gatewayOn && clientReady && (
                                            <div className='flex flex-col gap-2'>
                                                <div className='flex items-center gap-3'>
                                                    <Secondary label={C.tenant.readWorkspaces} disabled={busy} onClick={() => void listWorkspaces()} />
                                                    {workspacesNote && <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>{workspacesNote}</span>}
                                                </div>
                                                {workspaces && workspaces.length > 0 && (
                                                    <div className='flex items-end gap-3'>
                                                        <label className='flex flex-col gap-1 text-[13px] font-medium'>
                                                            {C.tenant.workspace}
                                                            <select value={workspace} onChange={(e) => setWorkspace(e.target.value)} className='h-8 px-2'>
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
                                                    <form onSubmit={(e) => void saveWorkspaceKey(e)} aria-busy={busy} className='flex flex-col gap-2' data-slot='workspace-key-form'>
                                                        <p className='text-[13px]'>{C.tenant.workspaceChosen(chosen.workspace.name)}</p>
                                                        <p data-slot='inference-state'>
                                                            <Verdict tone={setup.tenant.inference.state === 'ready' ? 'allowed' : 'info'} word={C.tenant.state[setup.tenant.inference.state]} />
                                                        </p>
                                                        <Field label={C.tenant.endpoint} name='endpoint' value={endpoint} onChange={setEndpoint} hint={C.tenant.endpointHint} error={errors.endpoint} />
                                                        <SecretField label={C.tenant.workspaceKey} inputRef={workspaceKeyRef} error={errors.workspaceKey} />
                                                        <div>
                                                            <Primary label={C.tenant.saveWorkspaceKey} disabled={busy} />
                                                        </div>
                                                    </form>
                                                )}
                                            </div>
                                        )}
                                    </section>
                                </div>
                            )}
                            <Actions back={index > 0 ? () => go(index - 1) : undefined} primary={{ label: tenantContinue.label, onClick: () => go(index + 1), disabled: tenantContinue.disabled }} />
                        </Step>
                    )}
                    {step === 'products' && setup && (
                        <Step heading={heading} id={headingId} title={C.products.title} lead={C.products.lead}>
                            <p className='text-[12px]' style={{ color: 'var(--v2-faint)' }}>
                                {C.products.licence}
                            </p>
                            <ul className='grid gap-3' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))' }}>
                                {PRODUCT_CARDS.map((card) => {
                                    const unavailable = card.id === null
                                    const always = card.id === 'runtimeSecurity'
                                    const on = card.id !== null && setup.products.includes(card.id)
                                    const reads = card.id === null ? null : card.id === 'runtimeSecurity' ? (scanReady ? C.products.readsWith.scanReady : C.products.readsWith.scanMissing) : clientReady ? C.products.readsWith.clientReady : C.products.readsWith.clientMissing
                                    const lastStep = run && !run.stale && card.id ? run.steps.find((s) => s.product === card.id && s.line !== 'managementClient') : undefined
                                    return (
                                        <li key={card.label} className='rounded-[12px] border p-4' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', opacity: unavailable ? 0.7 : 1 }}>
                                            <label className='flex items-start gap-3'>
                                                <input type='checkbox' disabled={unavailable || always} checked={on} onChange={(e) => card.id && void toggleProduct(card.id, e.target.checked)} aria-describedby={`${card.label}-body`} className='mt-1' />
                                                <span className='flex flex-col gap-1'>
                                                    <span className='text-[14px] font-semibold'>{card.label}</span>
                                                    <span id={`${card.label}-body`} className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                                        {card.body}
                                                    </span>
                                                    <span className='flex flex-wrap gap-1.5'>
                                                        {unavailable && <Verdict tone='info' word={C.products.unavailable} />}
                                                        {always && <Verdict tone='info' word={C.products.always} />}
                                                        {card.readOnly && <Verdict tone='info' word={C.products.readOnly} />}
                                                        {reads && !unavailable && <Verdict tone={reads === C.products.readsWith.clientMissing || reads === C.products.readsWith.scanMissing ? 'flagged' : 'allowed'} word={reads} />}
                                                        {lastStep && lastStep.state !== 'pending' && <Verdict tone={toneOf(lastStep.state)} word={C.verify.state[lastStep.state]} />}
                                                    </span>
                                                </span>
                                            </label>
                                        </li>
                                    )
                                })}
                            </ul>
                            <Actions back={index > 0 ? () => go(index - 1) : undefined} primary={{ label: C.products.continue, onClick: () => go(index + 1) }} />
                        </Step>
                    )}
                    {step === 'verify' && setup && (
                        <Step heading={heading} id={headingId} title={C.verify.title} lead={C.verify.lead}>
                            <ul className='flex flex-col gap-2' aria-label='Checks' aria-busy={running}>
                                {plannedRows.map((r) => (
                                    <li key={r.line} className='flex items-center justify-between gap-3 rounded-[12px] border px-4 py-3' style={{ borderColor: 'var(--v2-rule)', background: 'var(--v2-paper)', opacity: run?.stale ? 0.6 : 1 }} data-slot={`verify-${r.line}`}>
                                        <span className='flex flex-col'>
                                            <span className='text-[14px] font-medium'>{C.verify.rows[r.line]}</span>
                                            {r.reason && (
                                                <span className='text-[12px]' style={{ color: 'var(--v2-soft)' }}>
                                                    {r.reason}
                                                </span>
                                            )}
                                        </span>
                                        <Verdict tone={run || r.state === 'unavailable' ? toneOf(r.state) : 'info'} word={run || r.state === 'unavailable' ? C.verify.state[r.state] : C.verify.notRun} />
                                    </li>
                                ))}
                            </ul>
                            <p role='status' aria-live='polite' className='text-[13px]' style={{ color: run?.stale || run?.failed ? 'var(--v2-block)' : 'var(--v2-soft)' }}>
                                {verifyStatus}
                            </p>
                            <Actions
                                back={index > 0 ? () => go(index - 1) : undefined}
                                // A failed run offers the way back to the inputs; a rerun stays available
                                // too, because the failure may have been outside this desktop.
                                extra={run && run.done && run.failed && !run.stale ? { label: C.verify.fix, onClick: () => go(steps.indexOf('tenant')) } : undefined}
                                secondary={{ label: run ? C.verify.again : C.verify.run, onClick: () => void verify(), disabled: busy || running }}
                                primary={{ label: run && run.done && !run.stale && run.warnings.length > 0 && !run.blocking ? C.verify.continueWarnings : C.verify.continue, onClick: () => go(index + 1), disabled: !setup.completable }}
                            />
                        </Step>
                    )}
                    {step === 'packs' && setup && (
                        <Step heading={heading} id={headingId} title={C.packs.title} lead={C.packs.lead}>
                            {setup.packs.some((p) => p.count > 0) ? (
                                <ul className='flex flex-col gap-2' aria-label={C.packs.title}>
                                    {setup.packs.map((p) => (
                                        <li key={p.product}>
                                            <label className='flex items-center gap-2 text-[13px]'>
                                                <input type='checkbox' disabled={p.count === 0} checked={selectedPacks.includes(p.product)} onChange={(e) => setPacks(e.target.checked ? [...selectedPacks, p.product] : selectedPacks.filter((x) => x !== p.product))} />
                                                {C.packs.pack(productLabel(p.product), p.count)}
                                            </label>
                                        </li>
                                    ))}
                                </ul>
                            ) : (
                                <p className='text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                                    {C.packs.none}
                                </p>
                            )}
                            <Actions back={index > 0 ? () => go(index - 1) : undefined} secondary={{ label: C.packs.skip, onClick: () => { setPacks([]); go(index + 1) } }} primary={{ label: C.packs.continue, onClick: () => go(index + 1) }} />
                        </Step>
                    )}
                    {step === 'academy' && (
                        <Step heading={heading} id={headingId} title={C.academy.title} lead={C.academy.lead}>
                            <AcademyAccount view={snapshot.academy} dispatch={dispatch} />
                            <Actions back={index > 0 ? () => go(index - 1) : undefined} primary={{ label: snapshot.academy.state === 'signedIn' ? C.academy.continue : C.academy.skip, onClick: () => go(index + 1) }} />
                        </Step>
                    )}
                    {step === 'done' && setup && (
                        <Step heading={heading} id={headingId} title={C.done.title} lead={C.done.lead(setup.tenant.label)}>
                            <ul className='text-[13px]' style={{ color: 'var(--v2-soft)' }} data-slot='summary'>
                                <li>
                                    {C.done.summary.scan}: {scanReady ? C.done.summary.saved : C.done.summary.notSaved}
                                </li>
                                <li>
                                    {C.done.summary.client}: {clientReady ? C.done.summary.saved : C.done.summary.notSaved}
                                </li>
                                <li>
                                    {C.done.summary.products}: {setup.products.map(productLabel).join(', ')}
                                </li>
                                <li>
                                    {C.done.summary.packs}: {selectedPacks.length ? selectedPacks.map(productLabel).join(', ') : C.done.summary.none}
                                </li>
                            </ul>
                            <Actions back={index > 0 ? () => go(index - 1) : undefined} primary={{ label: finishing ? (mode === 'add' ? C.done.finishing : C.done.opening) : mode === 'add' ? C.done.finish : C.done.open, onClick: () => void finish(), disabled: finishing || !setup.completable || profileDirty }} />
                        </Step>
                    )}
                    <p role='status' aria-live='polite' data-slot='wizard-status' className='flex items-center gap-3 text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                        {status?.text}
                        {status?.retry && <Secondary label={C.outcome.retry} onClick={status.retry} />}
                    </p>
                </section>
            </main>
            {/* The guard is a dialog above the sheet (I8): rendered at the body, outside the sheet's inert subtree. */}
            {completionGuard && setup && createPortal(<SwitchGuard target={setup.tenant.label} completion onStay={() => void dispatch({ kind: 'dismissSwitchGuard' })} onResolve={(resolution) => void finish(resolution)} />, document.body)}
        </div>
    )
}

function Step({ heading, id, title, lead, children }: { heading: React.RefObject<HTMLHeadingElement | null>; id: string; title: string; lead: string; children: ReactNode }) {
    return (
        <>
            <h1 ref={heading} id={id} tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)' }}>
                {title}
            </h1>
            <p className='text-[14px]' style={{ color: 'var(--v2-soft)', maxWidth: 640 }}>
                {lead}
            </p>
            {children}
        </>
    )
}

function Field({ label, name, hint, error, onBlur, value, onChange, ...rest }: { label: string; name: string; hint?: string; error?: string; onBlur?: (value: string) => void; value?: string; onChange?: (value: string) => void; maxLength?: number; inputMode?: 'numeric' }) {
    const id = useId()
    return (
        <label className='flex flex-col gap-1 text-[13px] font-medium'>
            {label}
            <input id={id} name={name} autoComplete='off' value={value} onChange={onChange ? (e) => onChange(e.target.value) : undefined} aria-invalid={error ? true : undefined} aria-describedby={error ? `${id}-error` : hint ? `${id}-hint` : undefined} onBlur={(e) => onBlur?.(e.target.value)} className='h-8 px-2' {...rest} />
            {error ? (
                <span id={`${id}-error`} role='alert' className='text-[12px] font-normal' style={{ color: 'var(--v2-block)' }}>
                    {error}
                </span>
            ) : hint ? (
                <span id={`${id}-hint`} className='text-[12px] font-normal' style={{ color: 'var(--v2-faint)' }}>
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
        <label className='flex flex-col gap-1 text-[13px] font-medium'>
            {label}
            <input id={id} ref={inputRef} type='password' autoComplete='off' aria-invalid={error ? true : undefined} aria-describedby={error ? `${id}-error` : undefined} className='h-8 px-2' />
            {error && (
                <span id={`${id}-error`} role='alert' className='text-[12px] font-normal' style={{ color: 'var(--v2-block)' }}>
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
        <button type='button' onClick={onClick} disabled={disabled} className='h-8 rounded-[7px] border px-3 text-[13px] disabled:opacity-60' style={{ borderColor: 'var(--v2-rule)' }}>
            {label}
        </button>
    )
}

type Action = { label: string; onClick: () => void; disabled?: boolean }
function Actions({ back, extra, secondary, primary }: { back?: () => void; extra?: Action; secondary?: Action; primary: Action }) {
    return (
        <div className='mt-2 flex items-center gap-2'>
            {back && (
                <button type='button' onClick={back} className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                    {C.back}
                </button>
            )}
            <span className='flex-grow' />
            {extra && <Secondary label={extra.label} onClick={extra.onClick} disabled={extra.disabled} />}
            {secondary && <Secondary label={secondary.label} onClick={secondary.onClick} disabled={secondary.disabled} />}
            <button type='button' onClick={primary.onClick} disabled={primary.disabled} className='h-8 rounded-[7px] px-4 text-[13px] font-semibold disabled:opacity-60' style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}>
                {primary.label}
            </button>
        </div>
    )
}
