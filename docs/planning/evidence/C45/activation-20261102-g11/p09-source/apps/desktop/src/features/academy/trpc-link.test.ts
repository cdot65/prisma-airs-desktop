/**
 * C05 production learner transport: the desktop tRPC link Academy's learner
 * pages are hosted on, against the literal SuperJSON/envelope fixtures the
 * Rust `academy_learner` suite replays (packages/desktop-contracts/fixtures/
 * academy) and the pinned procedure sets Rust enforces
 * (learner-procedures.json). Covers the guest catalog, knowledge-check
 * lifecycle, account-generation changes, refusals and truth screening.
 */
import { createTRPCClient, TRPCClientError } from '@trpc/client'
import type { AnyRouter } from '@trpc/server'
import SuperJSON from 'superjson'
import { afterEach, beforeEach, describe, expect, it, type MockInstance, vi } from 'vitest'
import { DesktopClient, type Intent, type LearnerRef } from '@prisma-airs/desktop-contracts'
import procedures from '@prisma-airs/desktop-contracts/fixtures/academy/learner-procedures.json'
import coursesList from '@prisma-airs/desktop-contracts/fixtures/academy/courses-list.result.json'
import startInput from '@prisma-airs/desktop-contracts/fixtures/academy/units-startKnowledgeCheck.input.json'
import startResult from '@prisma-airs/desktop-contracts/fixtures/academy/units-startKnowledgeCheck.result.json'
import submitInput from '@prisma-airs/desktop-contracts/fixtures/academy/units-submitKnowledgeCheck.input.json'
import submitResult from '@prisma-airs/desktop-contracts/fixtures/academy/units-submitKnowledgeCheck.result.json'
import canary from '@prisma-airs/desktop-contracts/fixtures/academy/units-get.truth-canary.result.json'
import { desktopLink } from './desktop-link'
import { learnerQueryClient } from './learner-provider'

const GUEST: LearnerRef = 'lx-aaaaaaaaaaaaaaaaaaaaaaaa'
const ACCOUNT: LearnerRef = 'lx-bbbbbbbbbbbbbbbbbbbbbbbb'

type Reply = { outcome: 'refusal'; refusal: string } | { result: unknown }

/**
 * A scripted native core: `answer` decides each dispatched intent's reply
 * (a refusal, or the AcademyCallResult observed for its operation).
 */
function native(answer: (intent: Intent & { kind: 'academyCall' }) => Reply) {
    const dispatched: (Intent & { kind: 'academyCall' })[] = []
    const results = new Map<string, unknown>()
    let n = 0
    const client = new DesktopClient({
        async observe(bytes: Uint8Array) {
            const body = JSON.parse(new TextDecoder().decode(bytes)) as { scope: { kind: string; operation?: string } }
            if (body.scope.kind === 'deliverySession') {
                return { kind: 'deliverySession', session: `ds-${'c'.repeat(32)}`, nextSequence: '1' }
            }
            return { kind: 'academyCall', result: results.get(body.scope.operation ?? '') }
        },
        async dispatch(bytes: Uint8Array) {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent & { kind: 'academyCall' } })
                .intent
            dispatched.push(intent)
            const reply = answer(intent)
            if ('outcome' in reply) return reply
            n += 1
            results.set(`op-${n}`, reply.result)
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: `op-${n}` } }
        },
        openChannel: () => ({ token: '__CHANNEL__:1', close: () => undefined }),
    })
    return { client, dispatched }
}

/** A path-proxied learner client for the current reference (any procedure). */
function learnerClient(client: DesktopClient, learner: () => LearnerRef | null) {
    return createTRPCClient<AnyRouter>({
        links: [desktopLink<AnyRouter>({ client, learner, signedIn: () => learner() === ACCOUNT, transformer: SuperJSON })],
    }) as unknown as Record<string, Record<string, Record<string, (input?: unknown) => Promise<unknown>>>>
}

function call(trpc: ReturnType<typeof learnerClient>, procedure: string, kind: string, input?: unknown) {
    let node: unknown = trpc
    for (const part of procedure.split('.')) node = (node as Record<string, unknown>)[part]
    return (node as Record<string, (input?: unknown) => Promise<unknown>>)[kind === 'query' ? 'query' : 'mutate']!(input)
}

/** Rust hands the renderer `result.data` of the server envelope as text. */
const data = (envelope: { result: { data: unknown } }) => ({
    result: { kind: 'data', data: JSON.stringify(envelope.result.data) },
})

let fetchSpy: MockInstance<typeof fetch>
beforeEach(() => {
    fetchSpy = vi.spyOn(globalThis, 'fetch')
})
afterEach(() => {
    // The learner transport is the native core; the WebView never fetches.
    expect(fetchSpy).not.toHaveBeenCalled()
    fetchSpy.mockRestore()
})

describe('C05 learner transport', () => {
    it('labels every pinned procedure with its literal path and kind', async () => {
        const all = [
            ...procedures.learner.map((p) => [p.procedure, p.kind] as const),
            ...procedures.public.map((p) => [p, 'query'] as const),
        ]
        const { client, dispatched } = native(() => ({ result: { kind: 'data', data: '{"json":null}' } }))
        const trpc = learnerClient(client, () => ACCOUNT)
        for (const [procedure, kind] of all) await call(trpc, procedure, kind)
        expect(dispatched.map((i) => [i.procedure, i.procedureKind])).toEqual(all)
        expect(new Set(dispatched.map((i) => i.learner))).toEqual(new Set([ACCOUNT]))
        expect(dispatched.every((i) => i.input === null)).toBe(true)
        // The sets Rust enforces are disjoint from manager and grading routes.
        for (const [procedure] of all) {
            expect(procedure).not.toMatch(/^(admin\.(?!feedback\.submit$)|manager\.|assessor\.)|\.grade$/)
        }
    })

    it('reads the public catalog as the guest generation and revives its literal reply', async () => {
        const { client, dispatched } = native(() => data(coursesList))
        const courses = (await call(learnerClient(client, () => GUEST), 'courses.list', 'query')) as {
            slug: string
            updatedAt: Date
        }[]
        expect(dispatched).toEqual([
            { kind: 'academyCall', learner: GUEST, procedure: 'courses.list', procedureKind: 'query', input: null },
        ])
        expect(courses).toEqual([
            {
                id: 'runtime-security',
                slug: 'runtime-security',
                title: 'Runtime Security',
                description: 'Scan prompts and responses before they reach users.',
                updatedAt: new Date('2026-10-01T09:00:00.000Z'),
            },
        ])
        expect(courses[0]!.updatedAt).toBeInstanceOf(Date)
    })

    it('runs a knowledge check with byte-identical inputs and literal grades', async () => {
        const replies: Record<string, unknown> = {
            'units.startKnowledgeCheck': startResult,
            'units.submitKnowledgeCheck': submitResult,
        }
        const { client, dispatched } = native((intent) => data(replies[intent.procedure] as { result: { data: unknown } }))
        const trpc = learnerClient(client, () => ACCOUNT)
        const attempt = (await call(trpc, 'units.startKnowledgeCheck', 'mutation', { unitId: 'unit-kc-prompt-injection' })) as {
            attemptId: string
            questions: { id: string; options: string[] }[]
        }
        expect(attempt.questions[0]).toEqual({
            id: 'q-pi-1',
            moduleId: 'm-prompt-injection',
            objectiveId: 'obj-untrusted-input',
            type: 'mcq',
            prompt: 'Which input should a runtime scan treat as untrusted?',
            options: ['Retrieved web content', "The application's own constant labels"],
            weight: 1,
            scope: 'module',
            lessonId: null,
        })
        const graded = (await call(trpc, 'units.submitKnowledgeCheck', 'mutation', {
            unitId: 'unit-kc-prompt-injection',
            attemptId: attempt.attemptId,
            answers: [{ questionId: 'q-pi-1', value: 0 }],
        })) as { passed: boolean; scorePercent: number; results: unknown[] }
        expect(dispatched.map((i) => [i.procedure, i.procedureKind, i.input])).toEqual([
            ['units.startKnowledgeCheck', 'mutation', JSON.stringify(startInput)],
            ['units.submitKnowledgeCheck', 'mutation', JSON.stringify(submitInput)],
        ])
        expect(graded).toEqual({
            award: null,
            scorePercent: 100,
            passed: true,
            perObjective: [],
            results: [{ questionId: 'q-pi-1', correct: true }],
        })
        // The grade is the server's: the desktop never saw an answer key.
        expect(JSON.stringify(dispatched)).not.toMatch(/answerKey|correctValue/)
    })

    it('says a knowledge check may have been recorded when Academy fails after receiving it', async () => {
        const failure = {
            message: 'Internal server error',
            code: -32603,
            data: { code: 'INTERNAL_SERVER_ERROR', httpStatus: 500, path: 'units.submitKnowledgeCheck' },
        }
        const { client } = native(() => ({
            result: { kind: 'error', error: JSON.stringify({ json: failure }), httpStatus: 500, outcome: 'unknown' },
        }))
        const error = (await call(learnerClient(client, () => ACCOUNT), 'units.submitKnowledgeCheck', 'mutation', submitInput.json).catch(
            (e: unknown) => e
        )) as TRPCClientError<AnyRouter>
        expect(error).toBeInstanceOf(TRPCClientError)
        expect(error.message).toBe('Academy may have applied this change. Check its current state before trying again.')
        expect(error.data).toMatchObject({ code: 'INTERNAL_SERVER_ERROR', outcome: 'unknown', serverMessage: 'Internal server error' })
    })

    it('asks a guest to sign in and tells a signed-in learner the desktop does not carry it, never retrying', async () => {
        const { client, dispatched } = native((intent) =>
            !procedures.public.includes(intent.procedure) &&
            !(intent.learner === ACCOUNT && procedures.learner.some((p) => p.procedure === intent.procedure))
                ? { outcome: 'refusal', refusal: 'capabilityMissing' }
                : { result: { kind: 'data', data: '{"json":{"done":0}}' } }
        )
        const retry = learnerQueryClient().getDefaultOptions().queries?.retry as (n: number, e: unknown) => boolean
        // A guest reaching an account procedure gets Academy's sign-in gate.
        const guest = (await call(learnerClient(client, () => GUEST), 'progress.get', 'query').catch(
            (e: unknown) => e
        )) as TRPCClientError<AnyRouter>
        expect(guest.message).toBe('Sign in to Academy to do this.')
        expect(guest.data).toMatchObject({ code: 'UNAUTHORIZED', httpStatus: 401, outcome: 'notSent', failure: 'capabilityMissing' })
        expect(retry(0, guest)).toBe(false)
        // Signed in, a procedure outside the desktop allowlist is not an
        // Academy grant gate (FORBIDDEN): it is not carried by the desktop.
        const signed = (await call(learnerClient(client, () => ACCOUNT), 'labs.submitLab', 'mutation').catch(
            (e: unknown) => e
        )) as TRPCClientError<AnyRouter>
        expect(signed.message).toBe('This Academy action is not available on the desktop.')
        expect(signed.data).toMatchObject({ code: 'METHOD_NOT_SUPPORTED', httpStatus: 405, outcome: 'notSent' })
        expect(retry(0, signed)).toBe(false)
        expect(dispatched.map((i) => [i.procedure, i.learner])).toEqual([
            ['progress.get', GUEST],
            ['labs.submitLab', ACCOUNT],
        ])
    })

    it('calls under the current generation and reports a superseded one as an account change', async () => {
        let current: LearnerRef | null = GUEST
        const { client, dispatched } = native((intent) =>
            intent.learner === current
                ? { result: { kind: 'data', data: '{"json":[]}' } }
                : { outcome: 'refusal', refusal: 'staleContext' }
        )
        const trpc = learnerClient(client, () => current)
        await call(trpc, 'paths.list', 'query')
        current = ACCOUNT
        await call(trpc, 'paths.list', 'query')
        expect(dispatched.map((i) => i.learner)).toEqual([GUEST, ACCOUNT])

        // A client still bound to the guest generation after sign-in.
        const stale = learnerClient(client, () => GUEST)
        const error = (await call(stale, 'paths.list', 'query').catch((e: unknown) => e)) as TRPCClientError<AnyRouter>
        expect(error.message).toBe('The Academy account changed. Reload this page.')
        expect(error.data).toMatchObject({ code: 'PRECONDITION_FAILED', outcome: 'notSent' })

        current = null
        const none = (await call(trpc, 'paths.list', 'query').catch((e: unknown) => e)) as TRPCClientError<AnyRouter>
        expect(none.data).toMatchObject({ code: 'UNAUTHORIZED', outcome: 'notSent' })
        expect(dispatched).toHaveLength(3)
    })

    it('shows a screened reply as unacceptable without any of its content', async () => {
        // Rust screens `units-get.truth-canary.result.json` (it carries an
        // answer key) and reports only the failure.
        expect(JSON.stringify(canary)).toContain('AIRS-C05-TRUTH-CANARY')
        const { client } = native(() => ({ result: { kind: 'failed', failure: 'invalidResponse', outcome: 'unknown' } }))
        const error = (await call(learnerClient(client, () => ACCOUNT), 'units.get', 'query', { unitId: 'unit-kc-prompt-injection' }).catch(
            (e: unknown) => e
        )) as TRPCClientError<AnyRouter>
        expect(error.message).toBe('Academy returned a reply the desktop does not accept.')
        expect(error.data).toMatchObject({ failure: 'invalidResponse', outcome: 'unknown' })
        expect(JSON.stringify({ message: error.message, data: error.data })).not.toContain('CANARY')
    })
})
