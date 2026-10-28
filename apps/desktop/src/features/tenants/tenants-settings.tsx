import { type FormEvent, useId, useState } from 'react'
import { Button, Callout, TextField } from '@ai-security-academy/presentation'
import { AlertTriangle } from 'lucide-react'
import type {
    Destination,
    Region,
    ResetFailure,
    ShellSnapshot,
    TenantView,
    VaultError,
} from '@prisma-airs/desktop-contracts'
import { ScanKeyForm } from './credential-entry'
import { ManagementCredentialForm } from './management-credential'
import type { ObserveManagement } from '../administration/management-read'
import { type Dispatch, outcomeMessage } from './dispatch'
import { ProfileImport } from './profile-import'

const REGIONS: { value: Region; label: string }[] = [
    { value: 'unitedStates', label: 'United States' },
    { value: 'europe', label: 'Europe (Germany)' },
    { value: 'india', label: 'India' },
    { value: 'singapore', label: 'Singapore' },
]

const REASON: Record<VaultError, string> = {
    locked: 'the Keychain is locked',
    denied: 'macOS denied access to the Keychain',
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
                <label key={value} className='flex min-h-11 cursor-pointer items-center gap-3'>
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
                        className='border-academy-rule bg-academy-bg text-academy-ink min-h-11 rounded-[8px] border px-3 text-[13px]'
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
        <section aria-labelledby={headingId} data-slot='endpoint' className='flex flex-col gap-3'>
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

function TenantList({ snapshot, dispatch }: { snapshot: ShellSnapshot; dispatch: Dispatch }) {
    const { message, busy, run } = useAction(dispatch)
    const [confirmDelete, setConfirmDelete] = useState<string | null>(null)
    const headingId = useId()
    if (snapshot.tenants.length === 0) {
        return (
            <p className='text-academy-ink-soft text-[13px]' data-slot='no-tenants'>
                No tenants yet. Add one below or import tenant profiles.
            </p>
        )
    }
    return (
        <section aria-labelledby={headingId} className='flex flex-col gap-2'>
            <h3 id={headingId} className='text-academy-ink text-[14px] font-semibold'>
                Tenants
            </h3>
            <ul className='flex flex-col gap-2' data-slot='tenant-list'>
                {snapshot.tenants.map((tenant) => {
                    const active = snapshot.active?.tenant === tenant.id
                    return (
                        <li
                            key={tenant.id}
                            className='border-academy-rule flex flex-wrap items-center justify-between gap-3 rounded-[10px] border px-4 py-3'
                            aria-current={active ? 'true' : undefined}
                        >
                            <span className='flex flex-col'>
                                <span className='text-academy-ink text-[14px] font-medium'>
                                    {tenant.label}
                                    {active ? (
                                        <span className='text-academy-accent ml-2 text-[12px]'> (active)</span>
                                    ) : null}
                                </span>
                                <span className='text-academy-ink-faint text-[12px]'>
                                    {tenant.tsgId ? `TSG ${tenant.tsgId} · ` : ''}
                                    {destinationLabel(tenant.destination)} · scan key{' '}
                                    {tenant.scan.state === 'ready' ? 'saved' : tenant.scan.state === 'failed' ? 'failed' : tenant.scan.state === 'storing' ? 'saving' : 'missing'}
                                </span>
                            </span>
                            <span className='flex flex-wrap gap-2'>
                                {!active ? (
                                    <Button
                                        type='button'
                                        tier='secondary'
                                        size='sm'
                                        disabled={busy}
                                        aria-label={`Make ${tenant.label} active`}
                                        onClick={() => void run({ kind: 'selectTenant', tenant: tenant.id, resolution: null })}
                                    >
                                        Make active
                                    </Button>
                                ) : null}
                                {confirmDelete === tenant.id ? (
                                    <Button
                                        type='button'
                                        tier='primary'
                                        size='sm'
                                        disabled={busy}
                                        aria-label={`Confirm deleting ${tenant.label} and its scan key`}
                                        onClick={() => {
                                            setConfirmDelete(null)
                                            void run({ kind: 'deleteTenant', tenant: tenant.id })
                                        }}
                                    >
                                        Confirm delete
                                    </Button>
                                ) : (
                                    <Button
                                        type='button'
                                        tier='tertiary'
                                        size='sm'
                                        disabled={busy}
                                        aria-label={`Delete ${tenant.label}`}
                                        onClick={() => setConfirmDelete(tenant.id)}
                                    >
                                        Delete
                                    </Button>
                                )}
                            </span>
                        </li>
                    )
                })}
            </ul>
            <Message text={message} />
        </section>
    )
}

function AddTenant({ dispatch }: { dispatch: Dispatch }) {
    const { message, busy, run } = useAction(dispatch)
    const headingId = useId()
    return (
        <section aria-labelledby={headingId} className='flex flex-col gap-3'>
            <h3 id={headingId} className='text-academy-ink text-[14px] font-semibold'>
                Add a tenant
            </h3>
            <form
                className='flex flex-col gap-3'
                aria-busy={busy}
                data-slot='add-tenant'
                onSubmit={(event: FormEvent<HTMLFormElement>) => {
                    event.preventDefault()
                    const form = event.currentTarget
                    const data = new FormData(form)
                    const tsg = String(data.get('tenant-tsg') ?? '')
                    void run({
                        kind: 'createTenant',
                        profile: {
                            label: String(data.get('tenant-label') ?? ''),
                            tsgId: tsg.trim() === '' ? null : tsg,
                            destination: readDestination(form, 'tenant'),
                            environment: 'production',
                        },
                    }).then((ok) => {
                        if (ok) form.reset()
                    })
                }}
            >
                <TextField label='Label' name='tenant-label' required maxLength={64} autoComplete='off' />
                <TextField label='TSG ID (optional)' name='tenant-tsg' inputMode='numeric' autoComplete='off' />
                <DestinationFields name='tenant' />
                <div>
                    <Button type='submit' tier='primary' disabled={busy}>
                        Add tenant
                    </Button>
                </div>
            </form>
            <Message text={message} />
        </section>
    )
}

function ResetLocalData({ snapshot, dispatch }: { snapshot: ShellSnapshot; dispatch: Dispatch }) {
    const { message, busy, run } = useAction(dispatch)
    const [confirming, setConfirming] = useState(false)
    const headingId = useId()
    return (
        <section aria-labelledby={headingId} className='flex flex-col gap-3' data-slot='reset'>
            <h3 id={headingId} className='text-academy-ink text-[14px] font-semibold'>
                Reset local data
            </h3>
            <p className='text-academy-ink-soft text-[13px]'>
                Removes this desktop’s tenants, preferences and v2 scan keys. Prisma AIRS Desktop v1 data is not
                touched.
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
                    The last change was saved, but macOS did not confirm it was written to disk. It may be lost if
                    the Mac stops unexpectedly; the next change retries.
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

/** Tenant profiles, the active tenant's scan key and destination, import and reset. */
export function TenantsSettings({ snapshot, dispatch, observeManagement }: Props) {
    const legend = useId()
    const active = snapshot.active ? snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) : undefined
    return (
        <fieldset
            aria-labelledby={legend}
            data-slot='tenants'
            className='border-academy-rule flex flex-col gap-6 rounded-[12px] border p-5'
        >
            <legend id={legend} className='text-academy-ink px-1 text-[15px] font-semibold'>
                Tenants
            </legend>
            <StorageNotice snapshot={snapshot} />
            {active && snapshot.active ? (
                <div className='flex flex-col gap-6' data-slot='active-tenant'>
                    <p className='text-academy-ink text-[14px]'>
                        Active tenant: <span className='font-semibold'>{active.label}</span>
                    </p>
                    <ScanKeyForm
                        key={snapshot.active.context}
                        tenant={active}
                        context={snapshot.active.context}
                        dispatch={dispatch}
                    />
                    <ManagementCredentialForm
                        key={`management-${snapshot.active.context}`}
                        tenant={active}
                        context={snapshot.active.context}
                        dispatch={dispatch}
                        observe={observeManagement}
                    />
                    <EndpointChange tenant={active} snapshot={snapshot} dispatch={dispatch} />
                </div>
            ) : null}
            <TenantList snapshot={snapshot} dispatch={dispatch} />
            <AddTenant dispatch={dispatch} />
            <ProfileImport view={snapshot.import} dispatch={dispatch} />
            <ResetLocalData snapshot={snapshot} dispatch={dispatch} />
        </fieldset>
    )
}
