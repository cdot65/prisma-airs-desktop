import { useEffect, useRef, useState } from 'react'
import type { RotationView } from '@prisma-airs/desktop-contracts'
import { clockOf } from '../copy'
import { Verdict } from '../verdict'
import { ADMIN_COPY as C } from './copy'

export interface RotationPanelProps {
    rotation: RotationView
    /** The tenant the panel was opened for: an announcement never outlives it. */
    tenant: string | null
    /** Hands the held one-time key to the host sheet (I6). */
    onReveal: (operation: string) => void
    onFinishLater: () => void
    onUpdated: () => void
    onIssueAnother: () => void
    onRemoveOld: () => void
    onReconcile: () => void
    busy: boolean
}

/**
 * The staged Runtime key rotation (C31, I7; screens/reviews-administration.md
 * stages 1–4): every stage is the core's (`RotationView.stage`). The secret
 * is offered only while the core still holds it for this context; once it is
 * not, the panel says so and offers another issuance. Two timestamps are
 * shown with distinct labels: the operator's own statement and the remote
 * readback.
 */
export function RotationPanel({ rotation: r, tenant, onReveal, onFinishLater, onUpdated, onIssueAnother, onRemoveOld, onReconcile, busy }: RotationPanelProps) {
    const btn = 'h-8 rounded-[7px] border px-3 text-[13px] disabled:opacity-60'
    const primary = 'h-8 rounded-[7px] px-3 text-[13px] font-semibold disabled:opacity-60'
    const primaryStyle = {
        background: 'var(--v2-acc)',
        color: 'var(--v2-acc-ink)',
    }
    const rule = { borderColor: 'var(--v2-rule)' }
    // One persistent live region (DESIGN-V2 §8): each observed stage change
    // is announced once, without moving focus; nothing is announced for a
    // tenant other than the one the panel opened for.
    const [opened] = useState(tenant)
    const announcement = tenant === opened ? C.rotation.announce[r.stage](r.application) : ''
    // A stage change replaces its controls; if the focused one went with it,
    // focus stays in the panel instead of falling out of the sheet.
    const panel = useRef<HTMLElement>(null)
    useEffect(() => {
        const active = document.activeElement
        if (panel.current && (!active || active === document.body)) panel.current.focus()
    }, [r.stage])
    return (
        <section
            ref={panel}
            tabIndex={-1}
            aria-label={C.rotation.title(r.application)}
            data-slot='rotation-panel'
            data-stage={r.stage}
            className='flex flex-col gap-3 rounded-[12px] border p-4'
            style={rule}
        >
            <p role='status' aria-live='polite' aria-atomic='true' className='sr-only' data-slot='rotation-status'>
                {announcement}
            </p>
            {r.unsaved ? (
                <p role='alert' data-slot='rotation-unsaved' className='text-[13px]' style={{ color: 'var(--v2-flag)' }}>
                    {C.rotation.unsaved}
                </p>
            ) : null}
            {r.stage === 'issued' ? (
                <>
                    <Verdict tone='flagged' word={C.rotation.issued} />
                    <p className='text-[13px]'>{C.rotation.issuedNote}</p>
                    <div className='flex flex-wrap gap-2'>
                        <button type='button' className={primary} style={primaryStyle} disabled={busy || !r.issuedBy} onClick={() => r.issuedBy && onReveal(r.issuedBy)}>
                            {C.rotation.reveal}
                        </button>
                        <button type='button' className={btn} style={rule} onClick={onFinishLater}>
                            {C.rotation.finishLater}
                        </button>
                        <button type='button' className={btn} style={rule} disabled={busy} onClick={onUpdated}>
                            {C.rotation.updated}
                        </button>
                    </div>
                </>
            ) : r.stage === 'resumed' ? (
                <>
                    <Verdict tone='flagged' word={C.rotation.resumed} />
                    <p className='text-[13px]'>{C.rotation.resumedNote(r.application)}</p>
                    <div className='flex flex-wrap gap-2'>
                        <button type='button' className={btn} style={rule} disabled={busy} onClick={onIssueAnother}>
                            {C.rotation.issueAnother}
                        </button>
                        <button type='button' className={primary} style={primaryStyle} disabled={busy} onClick={onUpdated}>
                            {C.rotation.updated}
                        </button>
                    </div>
                </>
            ) : r.stage === 'acknowledged' ? (
                <>
                    <Verdict tone='info' word={C.rotation.acknowledged(clockOf(r.deploymentAcknowledgedAt ?? 0))} />
                    <p className='text-[13px]'>{C.rotation.acknowledgedNote}</p>
                    {r.removalRejected ? (
                        <p role='alert' className='text-[13px]' style={{ color: 'var(--v2-block)' }}>
                            {C.rotation.rejected(r.removalRejected)}
                        </p>
                    ) : null}
                    <div>
                        <button type='button' className={primary} style={primaryStyle} disabled={busy} onClick={onRemoveOld}>
                            {C.rotation.removeOld(r.oldKey.name)}
                        </button>
                    </div>
                </>
            ) : r.stage === 'issuanceUnconfirmed' || r.stage === 'removalUnconfirmed' ? (
                <>
                    <Verdict tone='flagged' word={r.stage === 'issuanceUnconfirmed' ? C.rotation.issuanceUnconfirmed : C.rotation.removalUnconfirmed} />
                    <p className='text-[13px]'>{r.stage === 'issuanceUnconfirmed' ? C.rotation.issuanceUnconfirmedNote : C.rotation.removalUnconfirmedNote}</p>
                    <div className='flex flex-wrap gap-2'>
                        {r.stage === 'issuanceUnconfirmed' && r.issuedBy ? (
                            // The service answered with a key the readback has not confirmed yet: it is still shown once.
                            <button type='button' className={primary} style={primaryStyle} disabled={busy} onClick={() => r.issuedBy && onReveal(r.issuedBy)}>
                                {C.rotation.reveal}
                            </button>
                        ) : null}
                        <button type='button' className={btn} style={rule} disabled={busy} onClick={onReconcile}>
                            {C.applied.reconcile}
                        </button>
                    </div>
                </>
            ) : (
                <>
                    <Verdict tone='allowed' word={C.rotation.complete(clockOf(r.oldKeyRemovedAt ?? 0))} />
                    <p className='text-[13px]'>{C.rotation.completeNote}</p>
                </>
            )}
            {r.deploymentAcknowledgedAt !== null || r.oldKeyRemovedAt !== null ? (
                <table className='text-[12px]' data-slot='rotation-timestamps'>
                    <caption className='sr-only'>{C.rotation.timestamps}</caption>
                    <tbody>
                        <tr>
                            <th scope='row' className='pr-3 text-left font-medium'>
                                {C.rotation.acknowledgedAt}
                            </th>
                            <td>{r.deploymentAcknowledgedAt !== null ? clockOf(r.deploymentAcknowledgedAt) : '—'}</td>
                        </tr>
                        <tr>
                            <th scope='row' className='pr-3 text-left font-medium'>
                                {C.rotation.removedAt}
                            </th>
                            <td>{r.oldKeyRemovedAt !== null ? clockOf(r.oldKeyRemovedAt) : '—'}</td>
                        </tr>
                    </tbody>
                </table>
            ) : null}
        </section>
    )
}
