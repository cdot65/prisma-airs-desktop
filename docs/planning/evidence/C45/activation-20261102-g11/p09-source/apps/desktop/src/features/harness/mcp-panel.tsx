import { useEffect, useId, useState } from 'react'
import { Button } from '@ai-security-academy/presentation'
import type {
    ContextRef,
    HarnessMcpView,
    McpServerView,
    McpSignInFailure,
    SelectedWorkspace,
    SignInStep,
} from '@prisma-airs/desktop-contracts'
import { MANAGEMENT_FAILURE_TEXT } from '../administration/management-read'
import type { Dispatch } from '../tenants/dispatch'

/** Why signing in to a workspace MCP server, or renewing it, failed. */
export const MCP_SIGN_IN_FAILURE_TEXT: Record<McpSignInFailure, string> = {
    notOffered: 'The server did not ask for a sign-in, so this app cannot use it.',
    denied: 'The server refused access.',
    notFound: 'The server was not found at its catalog address.',
    unavailable: 'The server is unavailable.',
    invalidMetadata: 'The server’s sign-in details point elsewhere or lack what this app needs, so they are not used.',
    registrationNotOffered: 'The server does not let this app register for a sign-in.',
    network: 'The server could not be reached. Try again.',
    invalidResponse: 'The server returned something this app does not accept.',
    timedOut: 'The browser did not come back within five minutes.',
    cancelled: 'Sign-in was cancelled.',
    browserUnavailable: 'The browser could not be opened.',
    callbackUnavailable: 'The sign-in callback could not be opened on this Mac.',
    sessionExpired: 'The sign-in has expired. Sign in again to use its tools.',
    storeFailed: 'The sign-in could not be kept in the Keychain.',
}

const STEP_TEXT: Record<SignInStep, string> = {
    contacting: 'Contacting the server…',
    browser: 'Continue in your browser. This app is waiting for it to come back.',
    completing: 'Completing the sign-in…',
}

/** At most eight servers per conversation (the engine's bound). */
const MAX_ON = 8

function standing(s: McpServerView): string {
    if (!s.supported) return 'This server’s identifier is not one the Harness engine accepts; it cannot be used.'
    if (s.expired) return MCP_SIGN_IN_FAILURE_TEXT[s.expired]
    if (s.stale) return 'Signed in for an earlier version of this server. Sign in again to use it.'
    if (s.signedIn) return 'Signed in.'
    return 'Not signed in: its tools will be unavailable until you sign in.'
}

interface Props {
    context: ContextRef
    selected: SelectedWorkspace
    mcp: HarnessMcpView | null
    /** A conversation is running (changing its servers ends it). */
    running: boolean
    dispatch: Dispatch
    onMessage: (message: string | null) => void
}

/**
 * The chosen workspace's MCP servers (C22, D049): read from its Gateway
 * catalog, turned on for new conversations (at most eight), each signed in
 * to in the browser by this app. The engine only ever gets each server's
 * token in memory, and asks before every tool call.
 */
export function McpPanel({ context, selected, mcp, running, dispatch, onMessage }: Props) {
    const ids = useId()
    const [busy, setBusy] = useState(false)
    const workspace = selected.reference
    const here = mcp !== null && mcp.workspace === selected.workspace.id

    // Read the catalog whenever the chosen workspace changes.
    useEffect(() => {
        void dispatch({ kind: 'listMcpServers', context, workspace })
    }, [dispatch, context, workspace])

    async function send(intent: Parameters<Dispatch>[0], refused: string) {
        onMessage(null)
        setBusy(true)
        try {
            const outcome = await dispatch(intent)
            if (outcome.kind !== 'admitted') onMessage(refused)
        } finally {
            setBusy(false)
        }
    }

    const servers = here ? mcp.servers : []
    const on = servers.filter((s) => s.enabled).length
    const signingIn = servers.some((s) => s.step !== null)
    return (
        <section aria-labelledby={`${ids}-heading`} className='border-academy-rule flex flex-col gap-2 rounded-[12px] border p-3' data-slot='harness-mcp'>
            <h2 id={`${ids}-heading`} className='text-[14px] font-semibold'>
                Workspace MCP servers
            </h2>
            <p className='text-academy-ink-faint text-[12px]'>
                Servers published in this workspace’s AI Gateway. The Harness asks you before every tool call, and each server’s sign-in stays
                with this app.
            </p>
            {!here || mcp.state === 'loading' ? (
                <p role='status' className='text-[13px]'>
                    Reading the workspace’s MCP servers…
                </p>
            ) : null}
            {here && mcp.failure ? (
                <p role='alert' className='text-academy-accent-warn text-[13px]'>
                    {`The MCP servers could not be read: ${MANAGEMENT_FAILURE_TEXT[mcp.failure]}`}
                </p>
            ) : null}
            {here && mcp.state !== 'loading' && servers.length === 0 && !mcp.failure ? (
                <p className='text-[13px]'>No MCP servers are published in this workspace.</p>
            ) : null}
            {here && !mcp.complete && servers.length > 0 ? (
                <p className='text-academy-ink-faint text-[12px]'>Only the first 500 servers are shown.</p>
            ) : null}
            {servers.length > 0 ? (
                <ul role='list' aria-label='MCP servers' className='flex flex-col gap-2'>
                    {servers.map((s) => {
                        const holdsSignIn = s.signedIn || s.stale || s.expired !== null
                        return (
                            <li key={s.id} className='border-academy-rule rounded-[8px] border p-2' data-slot='harness-mcp-server'>
                                <label className='flex items-start gap-2 text-[13px] font-semibold'>
                                    <input
                                        type='checkbox'
                                        className='mt-[3px]'
                                        checked={s.enabled}
                                        disabled={busy || !s.supported || (!s.enabled && on >= MAX_ON)}
                                        aria-describedby={`${ids}-${s.id}-standing`}
                                        onChange={(event) =>
                                            void send(
                                                { kind: 'enableMcpServer', context, workspace, server: s.id, enabled: event.target.checked },
                                                'That was not done. At most eight servers can be on, and only ones in the catalog.',
                                            )
                                        }
                                    />
                                    {`Use ${s.name} in new conversations`}
                                </label>
                                <p className='font-mono text-[11px] break-all'>{`${s.namespace} · ${s.url}`}</p>
                                <p id={`${ids}-${s.id}-standing`} className='text-[12px]'>
                                    {standing(s)}
                                </p>
                                {s.step ? (
                                    <p role='status' className='text-[12px]'>
                                        {STEP_TEXT[s.step]}
                                    </p>
                                ) : null}
                                {s.failure ? (
                                    <p role='alert' className='text-academy-accent-warn text-[12px]'>
                                        {MCP_SIGN_IN_FAILURE_TEXT[s.failure]}
                                    </p>
                                ) : null}
                                <div className='mt-1 flex flex-wrap gap-2'>
                                    {s.step ? (
                                        <Button
                                            type='button'
                                            tier='tertiary'
                                            size='sm'
                                            onClick={() => void send({ kind: 'cancelMcpSignIn', context }, 'That was not done.')}
                                        >
                                            Cancel sign-in
                                        </Button>
                                    ) : (
                                        <Button
                                            type='button'
                                            tier='secondary'
                                            size='sm'
                                            disabled={busy || !s.supported || signingIn}
                                            onClick={() =>
                                                void send(
                                                    { kind: 'signInMcpServer', context, workspace, server: s.id },
                                                    'Sign-in could not start. Another may be in progress.',
                                                )
                                            }
                                        >
                                            {holdsSignIn ? `Sign in to ${s.name} again` : `Sign in to ${s.name}`}
                                        </Button>
                                    )}
                                    {holdsSignIn ? (
                                        <Button
                                            type='button'
                                            tier='tertiary'
                                            size='sm'
                                            disabled={busy}
                                            onClick={() =>
                                                void send({ kind: 'signOutMcpServer', context, workspace, server: s.id }, 'That was not done.')
                                            }
                                        >
                                            {`Sign out of ${s.name}`}
                                        </Button>
                                    ) : null}
                                </div>
                            </li>
                        )
                    })}
                </ul>
            ) : null}
            {running ? (
                <p className='text-academy-ink-faint text-[12px]'>
                    Turning a server off, signing in again or signing out ends the running conversation that uses it.
                </p>
            ) : null}
            <Button
                type='button'
                tier='tertiary'
                size='sm'
                className='self-start'
                disabled={here && mcp.state === 'loading'}
                onClick={() => void send({ kind: 'listMcpServers', context, workspace }, 'The servers could not be read.')}
            >
                Read the servers again
            </Button>
        </section>
    )
}
