/**
 * The production Tauri binding and typed facade, exercised through Tauri's
 * IPC mocks against the literal wire fixtures the Rust host tests also use.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { ContractViolation, DeliveryOutcomeUnknown, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import dispatchRequest from '@prisma-airs/desktop-contracts/fixtures/wire/dispatch-request.json'
import dispatchReceipt from '@prisma-airs/desktop-contracts/fixtures/wire/dispatch-receipt.json'
import refusalGap from '@prisma-airs/desktop-contracts/fixtures/wire/dispatch-refusal-sequence-gap.json'
import sessionRequest from '@prisma-airs/desktop-contracts/fixtures/wire/observe-session-request.json'
import sessionResponse from '@prisma-airs/desktop-contracts/fixtures/wire/observe-session-response.json'
import shellResponse from '@prisma-airs/desktop-contracts/fixtures/wire/observe-shell-response.json'
import { createDesktopClient } from './invoke'

const decode = (payload: unknown): Record<string, unknown> => {
    // Raw bytes, never a JSON args object: Tauri sends ArrayBuffer views as
    // application/octet-stream so the host's size ceiling applies first.
    expect(ArrayBuffer.isView(payload)).toBe(true)
    expect(Object.prototype.toString.call(payload)).toBe('[object Uint8Array]')
    return JSON.parse(new TextDecoder().decode(payload as Uint8Array)) as Record<string, unknown>
}

type Handler = (cmd: string, body: Record<string, unknown>, raw: Uint8Array) => unknown

function host(handler: Handler) {
    const calls: { cmd: string; body: Record<string, unknown>; raw: Uint8Array }[] = []
    mockIPC((cmd, payload) => {
        const body = decode(payload)
        calls.push({ cmd, body, raw: payload as Uint8Array })
        return handler(cmd, body, payload as Uint8Array)
    })
    return calls
}

const isSessionRequest = (cmd: string, body: Record<string, unknown>) =>
    cmd === 'desktop_observe' && JSON.stringify(body) === JSON.stringify(sessionRequest)

afterEach(() => clearMocks())

describe('Tauri desktop transport', () => {
    it('registers a session and sends the literal raw envelope', async () => {
        const calls = host((cmd, body) => {
            if (isSessionRequest(cmd, body)) return sessionResponse
            expect(cmd).toBe('desktop_dispatch')
            expect(body).toEqual(dispatchRequest)
            return dispatchReceipt
        })
        const client = createDesktopClient()
        await expect(client.dispatch({ kind: 'setAppearance', theme: 'dark' })).resolves.toEqual(dispatchReceipt)
        expect(calls.map((c) => c.cmd)).toEqual(['desktop_observe', 'desktop_dispatch'])
    })

    it('serializes admissions and advances the sequence only after a result', async () => {
        let release: (value: unknown) => void = () => undefined
        const sequences: string[] = []
        host((cmd, body) => {
            if (isSessionRequest(cmd, body)) return sessionResponse
            sequences.push(body.sequence as string)
            if (body.sequence === '1') return new Promise((resolve) => (release = resolve))
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-2', operation: null } }
        })
        const client = createDesktopClient()
        const first = client.dispatch({ kind: 'setAppearance', theme: 'dark' })
        const second = client.dispatch({ kind: 'setAppearance', theme: 'light' })
        await vi.waitFor(() => expect(sequences).toEqual(['1']))
        await new Promise((r) => setTimeout(r, 10))
        expect(sequences).toEqual(['1'])
        release(dispatchReceipt)
        await expect(first).resolves.toEqual(dispatchReceipt)
        await expect(second).resolves.toMatchObject({ outcome: 'receipt' })
        expect(sequences).toEqual(['1', '2'])
    })

    it('recovers a lost admission result by resending identical bytes', async () => {
        const payloads: Uint8Array[] = []
        host((cmd, body, raw) => {
            if (isSessionRequest(cmd, body)) return sessionResponse
            payloads.push(raw)
            if (payloads.length === 1) throw new Error('connection reset')
            return dispatchReceipt
        })
        const client = createDesktopClient()
        await expect(client.dispatch({ kind: 'setAppearance', theme: 'dark' })).resolves.toEqual(dispatchReceipt)
        expect(payloads).toHaveLength(2)
        expect(Array.from(payloads[1]!)).toEqual(Array.from(payloads[0]!))
    })

    it('never reuses a sequence for different input after an unknown outcome', async () => {
        let sessions = 0
        const deliveries: Record<string, unknown>[] = []
        host((cmd, body) => {
            if (isSessionRequest(cmd, body)) {
                sessions += 1
                return { ...sessionResponse, session: `ds-${String(sessions).padStart(32, '0')}` }
            }
            deliveries.push(body)
            if (sessions === 1) throw new Error('lost')
            return dispatchReceipt
        })
        const client = createDesktopClient()
        await expect(client.dispatch({ kind: 'setAppearance', theme: 'dark' })).rejects.toBeInstanceOf(DeliveryOutcomeUnknown)
        await expect(client.dispatch({ kind: 'setAppearance', theme: 'light' })).resolves.toEqual(dispatchReceipt)
        const first = deliveries.filter((d) => d.session === deliveries[0]!.session)
        expect(first.every((d) => (d.intent as { theme: string }).theme === 'dark')).toBe(true)
        expect(deliveries.at(-1)).toMatchObject({ sequence: '1', intent: { theme: 'light' } })
        expect(deliveries.at(-1)!.session).not.toBe(deliveries[0]!.session)
    })

    it('returns refusals and rejects responses outside the contract', async () => {
        const responses: unknown[] = [refusalGap, { outcome: 'receipt' }, { outcome: 'refusal', refusal: 'approved' }]
        host((cmd, body) => (isSessionRequest(cmd, body) ? sessionResponse : responses.shift()))
        const client = createDesktopClient()
        await expect(client.dispatch({ kind: 'setAppearance', theme: 'dark' })).resolves.toEqual(refusalGap)
        await expect(client.dispatch({ kind: 'setAppearance', theme: 'dark' })).rejects.toBeInstanceOf(ContractViolation)
        await expect(client.dispatch({ kind: 'setAppearance', theme: 'dark' })).rejects.toBeInstanceOf(ContractViolation)
    })

    it('keeps sequences beyond 2^53 exact', async () => {
        const seen: string[] = []
        host((cmd, body) => {
            if (isSessionRequest(cmd, body)) return { ...sessionResponse, nextSequence: '9007199254740993' }
            seen.push(body.sequence as string)
            return dispatchReceipt
        })
        const client = createDesktopClient()
        await client.dispatch({ kind: 'setAppearance', theme: 'dark' })
        await client.dispatch({ kind: 'setAppearance', theme: 'dark' })
        expect(seen).toEqual(['9007199254740993', '9007199254740994'])
    })

    it('rotates the delivery session at u64 exhaustion instead of overflowing', async () => {
        const seen: string[] = []
        let sessions = 0
        host((cmd, body) => {
            if (isSessionRequest(cmd, body)) {
                sessions += 1
                return {
                    ...sessionResponse,
                    session: `ds-${String(sessions).padStart(32, '0')}`,
                    nextSequence: sessions === 1 ? '18446744073709551615' : '1',
                }
            }
            seen.push(`${body.session as string}:${body.sequence as string}`)
            return dispatchReceipt
        })
        const client = createDesktopClient()
        await client.dispatch({ kind: 'setAppearance', theme: 'dark' })
        await client.dispatch({ kind: 'setAppearance', theme: 'light' })
        expect(seen).toEqual([
            `ds-${'1'.padStart(32, '0')}:18446744073709551615`,
            `ds-${'2'.padStart(32, '0')}:1`,
        ])
    })

    it('observes the shell over a channel, ignores stale revisions and resyncs', async () => {
        const tokens: string[] = []
        host((cmd, body) => {
            expect(cmd).toBe('desktop_observe')
            const scope = body.scope as { kind: string; updates: string }
            expect(scope.kind).toBe('shell')
            expect(scope.updates).toMatch(/^__CHANNEL__:\d+$/)
            tokens.push(scope.updates)
            return shellResponse
        })
        const snapshots: ShellSnapshot[] = []
        const menus: unknown[] = []
        const errors: unknown[] = []
        const client = createDesktopClient()
        const stop = await client.observeShell({
            onSnapshot: (s) => snapshots.push(s),
            onMenu: (m) => menus.push(m),
            onError: (e) => errors.push(e),
        })
        const send = (token: string, index: number, message: unknown) => {
            const id = Number(token.split(':')[1])
            window.__TAURI_INTERNALS__!.runCallback!(id, { message, index })
        }
        const base = shellResponse.snapshot
        send(tokens[0]!, 0, { kind: 'shell', snapshot: { ...base, revision: '2', appearance: 'dark' } })
        send(tokens[0]!, 1, { kind: 'shell', snapshot: { ...base, revision: '1' } })
        send(tokens[0]!, 2, { kind: 'menu', command: { kind: 'navigate', feature: 'settings' } })
        send(tokens[0]!, 3, { kind: 'shell', snapshot: { ...base, revision: 'not-a-number' } })
        expect(snapshots.map((s) => s.revision)).toEqual(['1', '2'])
        expect(menus).toEqual([{ kind: 'navigate', feature: 'settings' }])
        expect(errors[0]).toBeInstanceOf(ContractViolation)
        send(tokens[0]!, 4, { kind: 'resync' })
        await vi.waitFor(() => expect(tokens).toHaveLength(2))
        expect(tokens[1]).not.toBe(tokens[0])
        stop()
    })

    it('closes replaced, failed and stopped observations on both sides', async () => {
        const opened: string[] = []
        const closed: string[] = []
        let fail = false
        host((cmd, body) => {
            const scope = body.scope as { kind: string; updates: string }
            if (scope.kind === 'closeShell') {
                closed.push(scope.updates)
                return { kind: 'closed' }
            }
            opened.push(scope.updates)
            if (fail) throw new Error('observe failed')
            return shellResponse
        })
        const callbacks = () => (window.__TAURI_INTERNALS__ as unknown as { callbacks: Map<number, unknown> }).callbacks
        const client = createDesktopClient()
        const stop = await client.observeShell({ onSnapshot: () => undefined, onMenu: () => undefined })
        const first = opened[0]!
        const id = (token: string) => Number(token.split(':')[1])
        expect(callbacks().has(id(first))).toBe(true)

        window.__TAURI_INTERNALS__!.runCallback!(id(first), { message: { kind: 'resync' }, index: 0 })
        await vi.waitFor(() => expect(closed).toEqual([first]))
        expect(callbacks().has(id(first))).toBe(false)
        await vi.waitFor(() => expect(opened).toHaveLength(2))

        stop()
        await vi.waitFor(() => expect(closed).toEqual([first, opened[1]]))
        expect(callbacks().has(id(opened[1]!))).toBe(false)

        fail = true
        await expect(client.observeShell({ onSnapshot: () => undefined, onMenu: () => undefined })).rejects.toThrow('observe failed')
        await vi.waitFor(() => expect(closed).toContain(opened[2]))
        expect(callbacks().has(id(opened[2]!))).toBe(false)
    })

    it('rejects snapshots and menu commands outside the contract', async () => {
        const base = shellResponse.snapshot
        const bad = [
            { ...base, revision: '18446744073709551616' },
            { ...base, features: [{ id: 'billing', label: 'Billing', availability: { state: 'available' } }] },
            { ...base, features: [{ id: 'runtime', label: 'Runtime', availability: { state: 'unavailable' } }] },
            { ...base, features: [base.features[0], base.features[0]] },
            { ...base, appearance: 'vegas' },
        ]
        for (const snapshot of bad) {
            clearMocks()
            const opened: string[] = []
            const closed: string[] = []
            host((_cmd, body) => {
                const scope = body.scope as { kind: string; updates: string }
                if (scope.kind === 'closeShell') {
                    closed.push(scope.updates)
                    return { kind: 'closed' }
                }
                opened.push(scope.updates)
                return { kind: 'shell', snapshot }
            })
            await expect(
                createDesktopClient().observeShell({ onSnapshot: () => undefined, onMenu: () => undefined })
            ).rejects.toBeInstanceOf(ContractViolation)
            // An invalid initial snapshot still releases both sides.
            await vi.waitFor(() => expect(closed).toEqual(opened))
            const callbacks = (window.__TAURI_INTERNALS__ as unknown as { callbacks: Map<number, unknown> }).callbacks
            expect(callbacks.has(Number(opened[0]!.split(':')[1]))).toBe(false)
        }
        clearMocks()
        const tokens: string[] = []
        host((_cmd, body) => {
            const scope = body.scope as { kind: string; updates: string }
            if (scope.kind === 'shell') tokens.push(scope.updates)
            return shellResponse
        })
        const errors: unknown[] = []
        const menus: unknown[] = []
        await createDesktopClient().observeShell({ onSnapshot: () => undefined, onMenu: (m) => menus.push(m), onError: (e) => errors.push(e) })
        window.__TAURI_INTERNALS__!.runCallback!(Number(tokens[0]!.split(':')[1]), {
            message: { kind: 'menu', command: { kind: 'navigate', feature: 'shop' } },
            index: 0,
        })
        expect(menus).toEqual([])
        expect(errors[0]).toBeInstanceOf(ContractViolation)
    })
})
