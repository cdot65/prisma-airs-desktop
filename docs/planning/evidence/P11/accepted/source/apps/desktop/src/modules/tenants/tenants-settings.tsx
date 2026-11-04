import { type FormEvent, useId, useState } from 'react'
import { Button, Callout, TextField } from '@ai-security-academy/presentation'
import { AlertTriangle } from 'lucide-react'
import type {
    Destination,
    Region,
    ResetFailure,
    ShellSnapshot,
    TenantEnvironment,
    TenantView,
    VaultError,
} from '@prisma-airs/desktop-contracts'
import { ScanKeyForm } from './credential-entry'
import { ManagementCredentialForm } from './management-credential'
import type { ObserveManagement } from '../administration/management-read'
import { type Dispatch, outcomeMessage } from './dispatch'
import { Wizard } from '../../v2/wizard/wizard'
import { Modal } from '../../v2/modal'

const REGIONS: { value: Region; label: string }[] = [
    { value: 'unitedStates', label: 'United States' },
    { value: 'europe', label: 'Europe (Germany)' },
    { value: 'india', label: 'India' },
    { value: 'singapore', label: 'Singapore' },
]

const REASON: Record<VaultError, string> = {
    locked: 'secure storage is locked',
    denied: 'the system denied access to secure storage',
    unavailable: 'local storage is unavailable',
    notFound: 'a stored item was missing',
    corrupt: 'a stored item was unreadable',
}

/** What a stopped reset did and did not do, stated without overclaiming. */
export function resetFailureText(failure: ResetFailure): string {
    const reason = REASON[failure.reason]
    return failure.stage === 'credentials'
        ? `Reset stopped while deleting scan keys because ${reason}. All scan keys were removed from use and are deleted as soon as possible; enter them again to scan. Tenant profiles and preferences were kept. Try the reset again.`
        : `Reset deleted the scan keys but could not remove the local desktop data because ${reason}. Tenant profiles were kept without keys. Try the reset again.`
}

function regionLabel(region: Region): string {
    return REGIONS.find((r) => r.value === region)?.label ?? region
}

function destinationLabel(destination: Destination): string {
    return destination.kind === 'region' ? `${regionLabel(destination.region)} region` : destination.origin
}

/** Destination fields shared by tenant creation and endpoint review. */
function DestinationFields({ name, initial }: { name: string; initial?: Destination }) {
    const [kind, setKind] = useState<'region' | 'custom'>(initial?.kind ?? 'region')
    const selectId = useId()
    return (
        <fieldset className='flex flex-col gap-2'>
            <legend className='text-academy-ink text-[13px] font-medium'>Scan destination</legend>
            {(['region', 'custom'] as const).map((value) => (
                <label key={value} className='flex min-h-8 cursor-pointer items-center gap-3'>
                    <input
                        type='radio'
                        name={`${name}-kind`}
                        value={value}
                        checked={kind === value}
                        onChange={() => setKind(value)}
                        className='size-4 accent-[var(--academy-accent)]'
                    />
                    <span className='text-academy-ink text-[13px]'>
                        {value === 'region' ? 'AIRS region' : 'Custom HTTPS endpoint'}
                    </span>
                </label>
            ))}
            {kind === 'region' ? (
                <div className='flex flex-col gap-1'>
                    <label htmlFor={selectId} className='text-academy-ink text-[13px] font-medium'>
                        Region
                    </label>
                    <select
                        id={selectId}
                        name={`${name}-region`}
                        defaultValue={initial?.kind === 'region' ? initial.region : 'unitedStates'}
                        className='border-academy-rule bg-academy-bg text-academy-ink min-h-8 rounded-[8px] border px-3 text-[13px]'
                    >
                        {REGIONS.map((r) => (
                            <option key={r.value} value={r.value}>
                                {r.label}
                            </option>
                        ))}
                    </select>
                </div>
            ) : (
                <TextField
                    label='Endpoint origin'
                    name={`${name}-origin`}
                    placeholder='https://scan.example.com'
                    hint='HTTPS only, with no path, query or credentials. The scan key is sent only here.'
                    defaultValue={initial?.kind === 'custom' ? initial.origin : ''}
                    autoComplete='off'
                    spellCheck={false}
                />
            )}
        </fieldset>
    )
}

/** Reads the destination fields at the moment of submission. */
function readDestination(form: HTMLFormElement, name: string): Destination {
    const data = new FormData(form)
    if (data.get(`${name}-kind`) === 'custom') {
        return { kind: 'custom', origin: String(data.get(`${name}-origin`) ?? '') }
    }
    return { kind: 'region', region: String(data.get(`${name}-region`) ?? 'unitedStates') as Region }
}

function useAction(dispatch: Dispatch) {
    const [message, setMessage] = useState<string | null>(null)
    const [busy, setBusy] = useState(false)
    async function run(intent: Parameters<Dispatch>[0]): Promise<boolean> {
        setBusy(true)
        setMessage(null)
        const outcome = await dispatch(intent)
        setBusy(false)
        setMessage(outcomeMessage(outcome))
        return outcome.kind === 'admitted'
    }
    return { message, busy, run }
}

function Message({ text }: { text: string | null }) {
    return text ? (
        <p role='alert' className='text-academy-accent-warn text-[13px]'>
            {text}
        </p>
    ) : null
}

function EndpointChange({ tenant, snapshot, dispatch }: { tenant: TenantView; snapshot: ShellSnapshot; dispatch: Dispatch }) {
    const { message, busy, run } = useAction(dispatch)
    const [editing, setEditing] = useState(false)
    const headingId = useId()
    const context = snapshot.active?.context
    const review = snapshot.endpointReview?.tenant === tenant.id ? snapshot.endpointReview : null
    if (!context) return null
    return (
        <section aria-labelledby={headingId} data-slot='endpoint' className='settings-card'>
            <h3 id={headingId} className='text-academy-ink text-[14px] font-semibold'>
                Scan destination
            </h3>
            <p className='text-academy-ink-soft text-[13px]'>
                Scan requests and the scan key go only to <span className='font-mono'>{tenant.origin}</span>.
            </p>
            {review ? (
                <div data-slot='endpoint-review' className='flex flex-col gap-3'>
                    <Callout tone='warning' icon={AlertTriangle}>
                        <p>
                            Review: scan requests will go to <span className='font-mono'>{review.toOrigin}</span> instead
                            of <span className='font-mono'>{review.fromOrigin}</span>.
                        </p>
                        {review.removesScanCredential ? (
                            <p>The current scan key will be removed. Enter a key for the new destination afterwards.</p>
                        ) : null}
                    </Callout>
                    <div className='flex flex-wrap gap-2'>
                        <Button
                            type='button'
                            tier='primary'
                            disabled={busy}
                            onClick={() => void run({ kind: 'confirmEndpointChange', context, review: review.review })}
                        >
                            Change destination
                        </Button>
                        <Button
                            type='button'
                            tier='tertiary'
                            disabled={busy}
                            onClick={() => void run({ kind: 'discardReview', review: review.review })}
                        >
                            Keep current destination
                        </Button>
                    </div>
                </div>
            ) : editing ? (
                <form
                    className='flex flex-col gap-3'
                    aria-busy={busy}
                    onSubmit={(event: FormEvent<HTMLFormElement>) => {
                        event.preventDefault()
                        const destination = readDestination(event.currentTarget, 'endpoint')
                        void run({ kind: 'reviewEndpointChange', context, destination }).then((ok) => {
                            if (ok) setEditing(false)
                        })
                    }}
                >
                    <DestinationFields name='endpoint' initial={tenant.destination} />
                    <div className='flex flex-wrap gap-2'>
                        <Button type='submit' tier='primary' disabled={busy}>
                            Review change
                        </Button>
                        <Button type='button' tier='tertiary' onClick={() => setEditing(false)}>
                            Cancel
                        </Button>
                    </div>
                </form>
            ) : (
                <div>
                    <Button type='button' tier='secondary' onClick={() => setEditing(true)}>
                        Change destination…
                    </Button>
                </div>
            )}
            <Message text={message} />
        </section>
    )
}

function UpdateTenant({ snapshot, dispatch }: { snapshot: ShellSnapshot; dispatch: Dispatch }) {
    const { message, busy, run } = useAction(dispatch)
    const [selected, setSelected] = useState(snapshot.active?.tenant ?? '')
    const [editing, setEditing] = useState<string | null>(null)
    const [confirmDelete, setConfirmDelete] = useState<string | null>(null)
    const headingId = useId()
    const tenant = snapshot.tenants.find((t) => t.id === selected) ?? snapshot.tenants[0]
    return (
        <section aria-labelledby={headingId} data-slot='update-tenant' className='settings-card'>
            <h3 id={headingId}>Update Tenant</h3>
            {!tenant ? <p className='settings-description'>No tenants yet. Add a tenant to connect.</p> : <>
                <label className='settings-field'>Tenant
                    <select aria-label='Tenant to update' value={tenant.id} disabled={busy} onChange={(e) => { setSelected(e.target.value); setEditing(null); setConfirmDelete(null) }}>
                        {snapshot.tenants.map((t) => <option key={t.id} value={t.id}>{t.label}{t.id === snapshot.active?.tenant ? ' (active)' : ''}</option>)}
                    </select>
                </label>
                <p className='settings-description'>{tenant.tsgId ? `TSG ${tenant.tsgId} · ` : ''}{destinationLabel(tenant.destination)}</p>
                {editing === tenant.id ? <form key={tenant.id} className='flex flex-col gap-3' onSubmit={(event) => {
                    event.preventDefault()
                    const data = new FormData(event.currentTarget)
                    void run({ kind: 'editTenant', tenant: tenant.id, label: String(data.get('label') ?? '').trim(), environment: String(data.get('environment')) as TenantEnvironment }).then((ok) => { if (ok) setEditing(null) })
                }}>
                    <div className='settings-fields'>
                        <label className='settings-field'>Display name<input name='label' required maxLength={64} defaultValue={tenant.label} autoComplete='off' /></label>
                        <label className='settings-field'>Environment<select name='environment' defaultValue={tenant.environment}>
                            <option value='production'>Production</option><option value='staging'>Staging</option><option value='other'>Other</option>
                        </select></label>
                    </div>
                    <p className='settings-description'>Environment labels this profile; it does not change service endpoints. To change the TSG identity, add a replacement tenant.</p>
                    <div className='flex flex-wrap gap-2'><Button type='submit' tier='primary' disabled={busy}>Save changes</Button><Button type='button' tier='tertiary' disabled={busy} onClick={() => setEditing(null)}>Cancel</Button></div>
                </form> : <div className='flex flex-wrap gap-2'>
                    <Button type='button' tier='secondary' disabled={busy} onClick={() => setEditing(tenant.id)}>Edit</Button>
                    {tenant.id !== snapshot.active?.tenant && <Button type='button' tier='secondary' disabled={busy} onClick={() => void run({ kind: 'selectTenant', tenant: tenant.id, resolution: null })}>Make active</Button>}
                    {confirmDelete === tenant.id ? <>
                        <Button type='button' tier='primary' disabled={busy} aria-label={`Confirm deleting ${tenant.label} and its credentials`} onClick={() => void run({ kind: 'deleteTenant', tenant: tenant.id }).then((ok) => { if (ok) setConfirmDelete(null) })}>Confirm delete</Button>
                        <Button type='button' tier='tertiary' disabled={busy} onClick={() => setConfirmDelete(null)}>Cancel</Button>
                    </> : <Button type='button' tier='tertiary' disabled={busy} onClick={() => setConfirmDelete(tenant.id)}>Delete</Button>}
                </div>}
            </>}
            <Message text={message} />
        </section>
    )
}

function AddTenant({ snapshot, dispatch, observeManagement }: Props) {
    const headingId = useId()
    const [open, setOpen] = useState(false)
    const [closeRequests, setCloseRequests] = useState(0)
    const [added, setAdded] = useState<string | null>(null)
    return <section aria-labelledby={headingId} className='settings-card' data-slot='add-tenant'>
        <h3 id={headingId}>Add Tenant</h3>
        <p className='settings-description'>Connect a tenant with its TSG ID, management client and scan region. OAuth authentication is required before it is added; a Scan Key can be configured later.</p>
        <div><Button type='button' tier='primary' onClick={() => { setCloseRequests(0); setOpen(true) }}>Add tenant…</Button></div>
        {added && <p role='status'>Added {added}.</p>}
        {open && <Modal label='Add a tenant' onClose={() => setCloseRequests((n) => n + 1)} width={1160} padding={0} data-slot='settings-add-tenant-sheet'>
            <Wizard mode='add' snapshot={snapshot} dispatch={dispatch} observeManagement={observeManagement} onNavigate={() => undefined} closeRequests={closeRequests} onClose={(label) => { setOpen(false); setCloseRequests(0); if (label) setAdded(label) }} />
        </Modal>}
    </section>
}

function ResetLocalData({ snapshot, dispatch }: { snapshot: ShellSnapshot; dispatch: Dispatch }) {
    const { message, busy, run } = useAction(dispatch)
    const [confirming, setConfirming] = useState(false)
    const headingId = useId()
    return (
        <section aria-labelledby={headingId} className='settings-card' data-slot='reset'>
            <h3 id={headingId} className='text-academy-ink text-[14px] font-semibold'>
                Reset local data
            </h3>
            <p className='text-academy-ink-soft text-[13px]'>
                Removes this app’s tenant profiles, preferences and stored credentials. Other applications are not changed.
            </p>
            {snapshot.resetFailure ? (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='reset-failure'>
                    {resetFailureText(snapshot.resetFailure)}
                </Callout>
            ) : null}
            <div className='flex flex-wrap gap-2'>
                {confirming ? (
                    <>
                        <Button
                            type='button'
                            tier='primary'
                            disabled={busy}
                            onClick={() => {
                                setConfirming(false)
                                void run({ kind: 'resetLocalData' })
                            }}
                        >
                            Confirm reset
                        </Button>
                        <Button type='button' tier='tertiary' onClick={() => setConfirming(false)}>
                            Cancel
                        </Button>
                    </>
                ) : (
                    <Button
                        type='button'
                        tier='secondary'
                        disabled={busy || snapshot.storage === 'resetting'}
                        onClick={() => setConfirming(true)}
                    >
                        Reset local data…
                    </Button>
                )}
            </div>
            <Message text={message} />
        </section>
    )
}

function StorageNotice({ snapshot }: { snapshot: ShellSnapshot }) {
    switch (snapshot.storage) {
        case 'ready':
            return snapshot.durabilityUnconfirmed ? (
                <Callout tone='warning' icon={AlertTriangle} role='status' data-slot='durability-notice'>
                    The last change was saved, but the system did not confirm it was written to disk. It may be lost if
                    this device stops unexpectedly; the next change retries.
                </Callout>
            ) : null
        case 'resetting':
            return (
                <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px]'>
                    Resetting local data… Scan keys are out of use until it finishes.
                </p>
            )
        case 'corrupt':
            return (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='storage-notice'>
                    Local desktop data could not be read. Nothing was changed. Reset local data to continue.
                </Callout>
            )
        case 'unavailable':
            return (
                <Callout tone='danger' icon={AlertTriangle} role='alert' data-slot='storage-notice'>
                    Local desktop data is unavailable. Another copy of the desktop may be running.
                </Callout>
            )
    }
}

interface Props {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observeManagement: ObserveManagement
}

/** Tenant settings keep profile editing separate from credential and destination authority. */
export function TenantsSettings({ snapshot, dispatch, observeManagement }: Props) {
    const legend = useId()
    const active = snapshot.active ? snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) : undefined
    return (
        <section aria-labelledby={legend} data-slot='tenants' className='flex flex-col gap-3'>
            <div className='flex flex-wrap items-baseline gap-3'>
                <h2 id={legend} className='text-[16px] font-semibold'>Tenants</h2>
                {active && <p className='settings-description'>Active tenant: <strong>{active.label}</strong></p>}
            </div>
            <StorageNotice snapshot={snapshot} />
            <div className='settings-grid'>
                {active && snapshot.active && <>
                    <ScanKeyForm key={snapshot.active.context} tenant={active} context={snapshot.active.context} dispatch={dispatch} />
                    <ManagementCredentialForm key={`management-${snapshot.active.context}`} tenant={active} context={snapshot.active.context} dispatch={dispatch} observe={observeManagement} />
                    <EndpointChange key={`destination-${snapshot.active.context}`} tenant={active} snapshot={snapshot} dispatch={dispatch} />
                </>}
                <UpdateTenant snapshot={snapshot} dispatch={dispatch} />
                <AddTenant snapshot={snapshot} dispatch={dispatch} observeManagement={observeManagement} />
                <ResetLocalData snapshot={snapshot} dispatch={dispatch} />
            </div>
        </section>
    )
}
