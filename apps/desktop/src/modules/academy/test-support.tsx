/**
 * Shared harness for Academy destination suites: a scripted native core that
 * answers learner calls per procedure (and per learner generation), records
 * every intent, and scripts refusals or raw call results; plus shell
 * snapshots for the guest and signed-in generations.
 */
import { DesktopClient, type Intent, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import shellResponse from '@prisma-airs/desktop-contracts/fixtures/wire/observe-shell-response.json'

export const GUEST = 'lx-aaaaaaaaaaaaaaaaaaaaaaaa'
export const ACCOUNT = 'lx-bbbbbbbbbbbbbbbbbbbbbbbb'
export const ACADEMY_ORIGIN = 'https://academy.example'

const base = shellResponse.snapshot as ShellSnapshot

/** A guest or signed-in shell snapshot for the Academy destination. */
export function academySnapshot(
    signedIn: boolean,
    learner = signedIn ? ACCOUNT : GUEST,
    offline: Partial<ShellSnapshot['academyOffline']> = {}
): ShellSnapshot {
    return {
        ...base,
        learner,
        academyOrigin: ACADEMY_ORIGIN,
        academy: signedIn
            ? { state: 'signedIn', displayName: 'Ada Lovelace', accountId: 'user_2fixturelearner', learner }
            : { state: 'signedOut' },
        academyOffline: {
            state: signedIn ? 'empty' : 'signedOut',
            reachable: true,
            downloads: [],
            localDrafts: [],
            ...offline,
        },
    } as ShellSnapshot
}

/** A reply that makes the native core refuse the call before sending it. */
export const refuse = (refusal = 'capabilityMissing') => ({ __refusal__: refusal })
/** A raw AcademyCallResult (server errors, unknown outcomes). */
export const result = (value: unknown) => ({ __result__: value })
/** A learner call whose remote outcome is unknown (the reply was lost). */
export const lostReply = () => result({ kind: 'failed', failure: 'network', outcome: 'unknown' })
const HELD = Symbol('held')
/** A learner call Academy has not answered yet (a slow optional read). */
export const held = () => ({ __held__: true })

export type Replies = Record<string, unknown> | ((procedure: string, learner: string, input: unknown) => unknown)

/** Native host and offline actions observed like learner calls, scripted as `@<kind>`. */
const HOST_ACTIONS = new Set([
    'printPage',
    'uploadAvatar',
    'loadAcademyImage',
    'downloadCourse',
    'removeDownload',
    'saveLocalDraft',
    'loadLocalDraft',
    'clearLocalDraft',
])

/**
 * The native core: `replies` maps a procedure (`a.b`) to its data, a scripted
 * refusal or a raw result; a function may vary them by learner and input.
 * Host actions are scripted as `@printPage`, `@uploadAvatar` and
 * `@loadAcademyImage` (input: the image URL); offline actions as
 * `@downloadCourse` and the like (input: the course or attempt id, and for
 * `@saveLocalDraft` the intent); unscripted ones open nothing.
 */
export function scriptedNative(replies: Replies) {
    const intents: Intent[] = []
    const calls: (Intent & { kind: 'academyCall' })[] = []
    const results = new Map<string, unknown>()
    let n = 0
    const lookup = (procedure: string, learner: string, input: unknown) =>
        typeof replies === 'function' ? replies(procedure, learner, input) : replies[procedure]
    const client = new DesktopClient({
        async observe(bytes: Uint8Array) {
            const body = JSON.parse(new TextDecoder().decode(bytes)) as { scope: { kind: string; operation?: string } }
            if (body.scope.kind === 'deliverySession') {
                return { kind: 'deliverySession', session: `ds-${'f'.repeat(32)}`, nextSequence: '1' }
            }
            const value = results.get(body.scope.operation ?? '')
            if (value === HELD) return new Promise(() => undefined)
            return { kind: 'academyCall', result: value }
        },
        async dispatch(bytes: Uint8Array) {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
            intents.push(intent)
            n += 1
            const host = HOST_ACTIONS.has(intent.kind)
            if (intent.kind !== 'academyCall' && !host) {
                return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: null } }
            }
            let value: unknown
            if (intent.kind === 'academyCall') {
                calls.push(intent)
                const input = intent.input ? (JSON.parse(intent.input) as { json?: unknown }).json : undefined
                value = lookup(intent.procedure, intent.learner, input)
            } else {
                const action = intent as Extract<Intent, { learner: string }> & { url?: string; courseId?: string; attemptId?: string }
                const input = intent.kind === 'saveLocalDraft' ? intent : (action.url ?? action.courseId ?? action.attemptId)
                value = lookup(`@${intent.kind}`, action.learner, input)
                if (value === undefined) value = result({ kind: 'failed', failure: 'hostUnavailable', outcome: 'notSent' })
            }
            if (value && typeof value === 'object' && '__refusal__' in value) {
                return { outcome: 'refusal', refusal: (value as { __refusal__: string }).__refusal__ }
            }
            results.set(
                `op-${n}`,
                value && typeof value === 'object' && '__held__' in value
                    ? HELD
                    : value && typeof value === 'object' && '__result__' in value
                    ? (value as { __result__: unknown }).__result__
                    : value === undefined
                      ? { kind: 'failed', failure: 'network', outcome: 'notSent' }
                      : { kind: 'data', data: JSON.stringify({ json: value }) }
            )
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: `op-${n}` } }
        },
        openChannel: () => ({ token: '__CHANNEL__:1', close: () => undefined }),
    })
    return { client, intents, calls }
}
