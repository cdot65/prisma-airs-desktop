/**
 * A scripted native host for production presentation tests: it answers the
 * real DesktopClient's observe/dispatch calls, records every byte sent
 * across the boundary and pushes native snapshots on the observation
 * channel, as the Rust host would.
 */
import { act } from '@testing-library/react'
import type { DesktopTransport, Intent, ShellSnapshot, TenantView } from '@prisma-airs/desktop-contracts'
import shellResponse from '@prisma-airs/desktop-contracts/fixtures/wire/observe-shell-response.json'

export const baseSnapshot = shellResponse.snapshot as ShellSnapshot

export function tenant(overrides: Partial<TenantView> = {}): TenantView {
    return {
        id: 'tn-0123456789abcdef',
        label: 'Prod EU',
        tsgId: '1234567890',
        destination: { kind: 'region', region: 'europe' },
        origin: 'https://service-de.api.aisecurity.paloaltonetworks.com',
        scan: { state: 'missing', mode: null, failure: null },
        management: { state: 'missing', mode: null, failure: null, clientId: null, tokenOrigin: null },
        ...overrides,
    }
}

export function withTenant(snapshot: ShellSnapshot, view: TenantView, context = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'): ShellSnapshot {
    return { ...snapshot, tenants: [view], active: { tenant: view.id, context } }
}

type Respond = (intent: Intent) => unknown
/** Answers one management observation (the native `management` scope). */
type ObserveManagement = (operation: string) => unknown | Promise<unknown>
/** Answers one API Client observation (the native `workbench` scope). */
type ObserveWorkbench = (operation: string) => unknown | Promise<unknown>

export interface Native {
    transport: DesktopTransport
    dispatched: Intent[]
    /** Every request body sent to the native host, decoded as text. */
    sent: string[]
    /** Every update delivered to the presentation. */
    delivered: string[]
    respond: Respond
    observeManagement: ObserveManagement
    observeWorkbench: ObserveWorkbench
    push(snapshot: ShellSnapshot): Promise<void>
    current: ShellSnapshot
}

const decoder = new TextDecoder()

export function scriptedNative(initial: ShellSnapshot): Native {
    const listeners: ((update: unknown) => void)[] = []
    let revision = BigInt(initial.revision)
    const native: Native = {
        dispatched: [],
        sent: [],
        delivered: [],
        current: initial,
        respond: () => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } }),
        observeManagement: () => ({ kind: 'refusal', refusal: 'invalidInput' }),
        observeWorkbench: () => ({ kind: 'refusal', refusal: 'invalidInput' }),
        push: (next) =>
            act(async () => {
                // Native revisions only increase; the client ignores others.
                revision += 1n
                const snapshot = { ...next, revision: revision.toString() }
                native.current = snapshot
                const update = { kind: 'shell', snapshot }
                native.delivered.push(JSON.stringify(update))
                listeners.at(-1)?.(update)
                await new Promise((resolve) => setTimeout(resolve, 0))
            }),
        transport: {
            async observe(bytes) {
                const text = decoder.decode(bytes)
                native.sent.push(text)
                const body = JSON.parse(text) as { scope: { kind: string; operation?: string } }
                if (body.scope.kind === 'deliverySession') {
                    return { kind: 'deliverySession', session: `ds-${'a'.repeat(32)}`, nextSequence: '1' }
                }
                if (body.scope.kind === 'closeShell') return { kind: 'closed' }
                if (body.scope.kind === 'management') {
                    const response = await native.observeManagement(body.scope.operation ?? '')
                    native.delivered.push(JSON.stringify(response))
                    return response
                }
                if (body.scope.kind === 'workbench') {
                    const response = await native.observeWorkbench(body.scope.operation ?? '')
                    native.delivered.push(JSON.stringify(response))
                    return response
                }
                const response = { kind: 'shell', snapshot: native.current }
                native.delivered.push(JSON.stringify(response))
                return response
            },
            async dispatch(bytes) {
                const text = decoder.decode(bytes)
                native.sent.push(text)
                const body = JSON.parse(text) as { intent: Intent }
                native.dispatched.push(body.intent)
                return native.respond(body.intent)
            },
            openChannel(onMessage) {
                listeners.push(onMessage)
                return { token: `__CHANNEL__:${listeners.length}`, close: () => undefined }
            },
        },
    }
    return native
}
