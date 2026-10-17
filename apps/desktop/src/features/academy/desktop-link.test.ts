/**
 * The production desktop tRPC link against literal SuperJSON/envelope
 * fixtures shared with the Rust learner transport tests
 * (packages/desktop-contracts/fixtures/academy).
 */
import { createTRPCClient, TRPCClientError } from '@trpc/client'
import { initTRPC } from '@trpc/server'
import SuperJSON from 'superjson'
import { describe, expect, it } from 'vitest'
import { DesktopClient, type Intent } from '@prisma-airs/desktop-contracts'
import unitsGet from '@prisma-airs/desktop-contracts/fixtures/academy/units-get.result.json'
import unitsGetInput from '@prisma-airs/desktop-contracts/fixtures/academy/units-get.input.json'
import shopBuyError from '@prisma-airs/desktop-contracts/fixtures/academy/shop-buy.error.json'
import { desktopLink } from './desktop-link'

// A router type with the two procedures the fixtures cover.
const t = initTRPC.create({ transformer: SuperJSON })
const router = t.router({
    units: t.router({
        get: t.procedure.input((v) => v as { unitId: string; since: Date }).query(() => ({})),
    }),
    shop: t.router({ buy: t.procedure.input((v) => v as { itemId: string }).mutation(() => ({})) }),
    live: t.procedure.subscription(async function* () {}),
})
type Router = typeof router

const LEARNER = 'lx-0123456789abcdef01234567'

function native(results: Record<string, unknown>, refuse?: string) {
    const dispatched: Intent[] = []
    const observed: string[] = []
    let n = 0
    const transport = {
        async observe(bytes: Uint8Array) {
            const body = JSON.parse(new TextDecoder().decode(bytes)) as { scope: { kind: string; operation?: string } }
            if (body.scope.kind === 'deliverySession') {
                return { kind: 'deliverySession', session: `ds-${'b'.repeat(32)}`, nextSequence: '1' }
            }
            observed.push(body.scope.operation ?? '')
            return { kind: 'academyCall', result: results[body.scope.operation ?? ''] }
        },
        async dispatch(bytes: Uint8Array) {
            const body = JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }
            dispatched.push(body.intent)
            if (refuse) return { outcome: 'refusal', refusal: refuse }
            n += 1
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: `op-${n}` } }
        },
        openChannel: () => ({ token: '__CHANNEL__:1', close: () => undefined }),
    }
    const client = new DesktopClient(transport)
    const trpc = createTRPCClient<Router>({
        links: [desktopLink<Router>({ client, learner: () => LEARNER, transformer: SuperJSON })],
    })
    return { trpc, dispatched, observed }
}

describe('desktop tRPC link', () => {
    it('serializes input with SuperJSON and revives the literal reply exactly as Rust returns it', async () => {
        // Rust returns `result.data` of the server envelope as text.
        const data = JSON.stringify(unitsGet.result.data)
        const { trpc, dispatched } = native({ 'op-1': { kind: 'data', data } })
        const value = await trpc.units.get.query({ unitId: 'unit-prompt-injection-1', since: new Date('2026-09-30T00:00:00.000Z') })
        expect(dispatched).toEqual([
            {
                kind: 'academyCall',
                learner: LEARNER,
                procedure: 'units.get',
                procedureKind: 'query',
                input: JSON.stringify(unitsGetInput),
            },
        ])
        expect(value).toEqual({
            id: 'unit-prompt-injection-1',
            title: 'Prompt injection basics',
            completedAt: new Date('2026-10-01T12:30:00.000Z'),
            steps: [{ kind: 'reading', minutes: 6 }],
        })
        expect((value as { completedAt: unknown }).completedAt).toBeInstanceOf(Date)
    })

    it('turns a server error envelope into the same TRPCClientError the HTTP link would', async () => {
        const { trpc, dispatched } = native({ 'op-1': { kind: 'error', error: JSON.stringify(shopBuyError.error), httpStatus: 400, outcome: 'rejected' } })
        const error = await trpc.shop.buy.mutate({ itemId: 'x' }).catch((e: unknown) => e)
        expect(error).toBeInstanceOf(TRPCClientError)
        const e = error as TRPCClientError<Router>
        expect(e.message).toBe('Not enough credits')
        expect(e.data).toMatchObject({ code: 'BAD_REQUEST', httpStatus: 400, path: 'shop.buy', outcome: 'rejected' })
        expect(dispatched[0]).toMatchObject({ procedureKind: 'mutation', procedure: 'shop.buy' })
    })

    it('reports native failures and refusals without retrying', async () => {
        const failed = native({ 'op-1': { kind: 'failed', failure: 'sessionExpired', outcome: 'notSent' } })
        const expired = await failed.trpc.units.get.query({ unitId: 'u', since: new Date(0) }).catch((e: unknown) => e)
        expect((expired as TRPCClientError<Router>).data).toMatchObject({ code: 'UNAUTHORIZED' })
        expect(failed.dispatched).toHaveLength(1)

        const refused = native({}, 'capabilityMissing')
        const forbidden = await refused.trpc.shop.buy.mutate({ itemId: 'x' }).catch((e: unknown) => e)
        expect((forbidden as TRPCClientError<Router>).data).toMatchObject({ code: 'FORBIDDEN' })
        expect(refused.observed).toEqual([])
    })

    it('never sends subscriptions or calls without a learner', async () => {
        const { dispatched } = native({})
        const client = new DesktopClient({
            observe: async () => ({ kind: 'deliverySession', session: `ds-${'c'.repeat(32)}`, nextSequence: '1' }),
            dispatch: async () => {
                throw new Error('must not dispatch')
            },
            openChannel: () => ({ token: 't', close: () => undefined }),
        })
        const anonymous = createTRPCClient<Router>({
            links: [desktopLink<Router>({ client, learner: () => null, transformer: SuperJSON })],
        })
        const error = await anonymous.units.get.query({ unitId: 'u', since: new Date(0) }).catch((e: unknown) => e)
        expect((error as TRPCClientError<Router>).data).toMatchObject({ code: 'UNAUTHORIZED' })
        expect(dispatched).toEqual([])
    })

    it.each([
        ['a lost reply', { kind: 'failed', failure: 'network', outcome: 'unknown' }],
        ['a timeout after dispatch', { kind: 'failed', failure: 'timeout', outcome: 'unknown' }],
        ['a cancellation after dispatch', { kind: 'failed', failure: 'cancelled', outcome: 'unknown' }],
        ['an HTTP 5xx', { kind: 'error', error: JSON.stringify({ json: { message: 'boom', code: -32603, data: { code: 'INTERNAL_SERVER_ERROR', httpStatus: 500 } } }), httpStatus: 500, outcome: 'unknown' }],
    ])('reports %s on a mutation as possibly applied and never sends it again', async (_case, result) => {
        const { trpc, dispatched } = native({ 'op-1': result })
        const error = (await trpc.shop.buy.mutate({ itemId: 'x' }).catch((e: unknown) => e)) as TRPCClientError<Router>
        expect(error).toBeInstanceOf(TRPCClientError)
        expect((error.data as { outcome?: string }).outcome).toBe('unknown')
        expect(error.message).toMatch(/may have applied this change/)
        if (result.kind === 'error') {
            // The server's own status, code and message are kept.
            expect(error.data).toMatchObject({ code: 'INTERNAL_SERVER_ERROR', httpStatus: 500, serverMessage: 'boom' })
        }
        expect(dispatched).toHaveLength(1)
    })

    it('keeps not-sent and rejected distinct from unknown', async () => {
        const notSent = native({ 'op-1': { kind: 'failed', failure: 'credentialUnavailable', outcome: 'notSent' } })
        const e1 = (await notSent.trpc.shop.buy.mutate({ itemId: 'x' }).catch((e: unknown) => e)) as TRPCClientError<Router>
        expect(e1.data).toMatchObject({ outcome: 'notSent', code: 'UNAUTHORIZED' })
        expect(e1.message).not.toMatch(/may have applied/)
        const rejected = native({ 'op-1': { kind: 'error', error: JSON.stringify(shopBuyError.error), httpStatus: 400, outcome: 'rejected' } })
        const e2 = (await rejected.trpc.shop.buy.mutate({ itemId: 'x' }).catch((e: unknown) => e)) as TRPCClientError<Router>
        expect(e2.data).toMatchObject({ outcome: 'rejected' })
    })

    it('treats an unconfirmed admission or an unreadable result as unknown', async () => {
        const lostAdmission = new DesktopClient({
            observe: async () => ({ kind: 'deliverySession', session: `ds-${'e'.repeat(32)}`, nextSequence: '1' }),
            dispatch: async () => {
                throw new Error('lost')
            },
            openChannel: () => ({ token: 't', close: () => undefined }),
        })
        const trpc = createTRPCClient<Router>({
            links: [desktopLink<Router>({ client: lostAdmission, learner: () => LEARNER, transformer: SuperJSON })],
        })
        const error = (await trpc.shop.buy.mutate({ itemId: 'x' }).catch((e: unknown) => e)) as TRPCClientError<Router>
        expect(error.data).toMatchObject({ outcome: 'unknown' })
        expect(error.message).toMatch(/may have applied this change/)
    })
})
