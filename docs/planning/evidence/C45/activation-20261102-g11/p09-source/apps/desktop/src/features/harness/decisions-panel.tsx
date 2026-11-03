import { useId, useState } from 'react'
import { Button, Callout } from '@ai-security-academy/presentation'
import { AlertTriangle } from 'lucide-react'
import type { ConversationView, FeatureId, HarnessApproval, ProposalView } from '@prisma-airs/desktop-contracts'
import { CHANGE_NOUN, verbOf } from '../administration/runtime-changes'
import type { Dispatch } from '../tenants/dispatch'

const APPROVAL_TITLE: Record<HarnessApproval['kind'], string> = {
    command: 'Run a command',
    fileChange: 'Change files',
    mcpTool: 'Run a workspace MCP tool',
    desktopRead: 'Read the tenant through the desktop',
}

interface Props {
    conversation: ConversationView
    dispatch: Dispatch
    onNavigate: (id: FeatureId) => void
    onMessage: (message: string | null) => void
}

/**
 * What the Harness waits on the operator for (C21, D048): approvals for
 * commands and file changes in the project, proposed management changes
 * (opened only into this app's own review, never applied here), and the
 * renewed permission to change the project after a sign-in restart.
 */
export function DecisionsPanel({ conversation, dispatch, onNavigate, onMessage }: Props) {
    const ids = useId()
    const [busy, setBusy] = useState(false)
    const id = conversation.id

    async function send(intent: Parameters<Dispatch>[0], refused: string): Promise<boolean> {
        onMessage(null)
        setBusy(true)
        try {
            const outcome = await dispatch(intent)
            if (outcome.kind !== 'admitted') onMessage(refused)
            return outcome.kind === 'admitted'
        } finally {
            setBusy(false)
        }
    }

    const decide = (a: HarnessApproval, allow: boolean) =>
        send({ kind: 'decideHarnessApproval', conversation: id, approval: a.id, allow }, 'That request is no longer waiting.')

    async function review(p: ProposalView) {
        const opened = await send(
            { kind: 'reviewHarnessProposal', conversation: id, proposal: p.id },
            'The proposal could not be opened. Finish or discard the change already in review in Administration first.',
        )
        if (opened) onNavigate('administration')
    }

    const proposals = conversation.proposals.filter((p) => p.state !== 'dismissed')
    return (
        <>
            {conversation.phase === 'confirmWrite' && conversation.project ? (
                <Callout tone='default' icon={AlertTriangle} data-slot='harness-confirm-write'>
                    <p>{`Your sign-in needs renewing, so the engine will restart with this conversation. Allow it to change files in ${conversation.project} again?`}</p>
                    <div className='mt-2 flex flex-wrap gap-2'>
                        <Button
                            type='button'
                            tier='primary'
                            size='sm'
                            disabled={busy}
                            onClick={() => void send({ kind: 'confirmHarnessWrite', conversation: id, allow: true }, 'That was not done.')}
                        >
                            Allow changes and continue
                        </Button>
                        <Button
                            type='button'
                            tier='secondary'
                            size='sm'
                            disabled={busy}
                            onClick={() => void send({ kind: 'confirmHarnessWrite', conversation: id, allow: false }, 'That was not done.')}
                        >
                            Not now
                        </Button>
                    </div>
                    <p className='text-academy-ink-faint mt-2 text-[12px]'>
                        Not now keeps your message in the box. To continue without changes, turn off changes for the folder; that ends this conversation.
                    </p>
                </Callout>
            ) : null}
            {conversation.approvals.length > 0 ? (
                <section aria-labelledby={`${ids}-approvals`} className='flex flex-col gap-2' data-slot='harness-approvals'>
                    <h3 id={`${ids}-approvals`} className='text-[14px] font-semibold'>
                        Waiting for your approval
                    </h3>
                    <ul role='list' className='flex flex-col gap-2'>
                        {conversation.approvals.map((a) => (
                            <li key={a.id} className='border-academy-accent-warn rounded-[12px] border p-3' data-slot='harness-approval'>
                                <p className='text-[13px] font-semibold'>{APPROVAL_TITLE[a.kind]}</p>
                                {a.kind === 'mcpTool' ? (
                                    <p className='font-mono text-[12px] break-all'>{`${a.server} · ${a.tool}`}</p>
                                ) : null}
                                {a.arguments ? (
                                    <pre aria-label='Arguments' className='font-mono text-[12px] break-all whitespace-pre-wrap'>
                                        {a.arguments}
                                    </pre>
                                ) : null}
                                {a.command ? <pre className='font-mono text-[12px] break-all whitespace-pre-wrap'>{a.command}</pre> : null}
                                {a.files.length > 0 ? (
                                    <ul role='list' aria-label='Files' className='font-mono text-[12px] break-all'>
                                        {a.files.map((f) => (
                                            <li key={f}>{f}</li>
                                        ))}
                                    </ul>
                                ) : null}
                                {a.cwd ? <p className='text-academy-ink-faint text-[12px] break-all'>{`In ${a.cwd}`}</p> : null}
                                {a.reason ? <p className='text-[12px]'>{`Reason given: ${a.reason}`}</p> : null}
                                <div className='mt-2 flex gap-2'>
                                    <Button type='button' tier='primary' size='sm' disabled={busy} onClick={() => void decide(a, true)}>
                                        {a.kind === 'mcpTool' ? 'Approve once' : 'Allow'}
                                    </Button>
                                    <Button type='button' tier='secondary' size='sm' disabled={busy} onClick={() => void decide(a, false)}>
                                        Decline
                                    </Button>
                                </div>
                            </li>
                        ))}
                    </ul>
                </section>
            ) : null}
            {proposals.length > 0 ? (
                <section aria-labelledby={`${ids}-proposals`} className='flex flex-col gap-2' data-slot='harness-proposals'>
                    <h3 id={`${ids}-proposals`} className='text-[14px] font-semibold'>
                        Proposed changes
                    </h3>
                    <p className='text-academy-ink-faint text-[12px]'>
                        The Harness cannot change your tenant. A proposal opens in Administration’s review, where you check the difference and apply it
                        yourself.
                    </p>
                    <ul role='list' className='flex flex-col gap-2'>
                        {proposals.map((p) => (
                            <li key={p.id} className='border-academy-rule rounded-[12px] border p-3' data-slot='harness-proposal'>
                                <p className='text-[13px] font-semibold'>
                                    {`${verbOf(p.draft.kind, p.draft.action)} ${CHANGE_NOUN[p.draft.kind]}${p.draft.target ? ` ${p.draft.target}` : ''}`}
                                </p>
                                <p className='text-[13px] whitespace-pre-wrap'>{p.summary}</p>
                                {p.state === 'closed' ? (
                                    <p className='text-academy-ink-faint mt-2 text-[12px]'>
                                        Its review has ended. Administration lists what was applied.
                                    </p>
                                ) : p.state === 'inReview' ? (
                                    <div className='mt-2 flex items-center gap-2'>
                                        <p className='text-academy-ink-faint text-[12px]'>Opened for review.</p>
                                        <Button type='button' tier='secondary' size='sm' onClick={() => onNavigate('administration')}>
                                            Go to Administration
                                        </Button>
                                    </div>
                                ) : (
                                    <div className='mt-2 flex gap-2'>
                                        <Button type='button' tier='primary' size='sm' disabled={busy} onClick={() => void review(p)}>
                                            Review
                                        </Button>
                                        <Button
                                            type='button'
                                            tier='tertiary'
                                            size='sm'
                                            disabled={busy}
                                            onClick={() =>
                                                void send({ kind: 'dismissHarnessProposal', conversation: id, proposal: p.id }, 'That proposal is no longer here.')
                                            }
                                        >
                                            Dismiss
                                        </Button>
                                    </div>
                                )}
                            </li>
                        ))}
                    </ul>
                </section>
            ) : null}
        </>
    )
}
