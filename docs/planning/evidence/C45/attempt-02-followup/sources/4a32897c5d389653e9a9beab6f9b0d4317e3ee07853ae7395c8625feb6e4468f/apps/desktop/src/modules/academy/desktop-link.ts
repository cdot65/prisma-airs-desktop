/**
 * The desktop's terminating tRPC link for Academy learner procedures.
 *
 * Instead of HTTP, each operation is admitted by the native core as an
 * `academyCall` for the current learner reference; Rust checks the pinned
 * learner allowlist, injects the account credential and returns the reply in
 * tRPC wire form. The link rebuilds the same envelope tRPC's HTTP link would
 * read and applies tRPC's own `transformResult`, so Academy's hooks see
 * identical data, errors and SuperJSON-revived values. Credentials never
 * reach this code.
 */
import { TRPCClientError, type TRPCLink } from '@trpc/client'
import type { AnyRouter } from '@trpc/server'
import { observable } from '@trpc/server/observable'
import { transformResult } from '@trpc/server/unstable-core-do-not-import'
import type { CallFailure, DesktopClient, LearnerRef, Outcome, Refusal } from '@prisma-airs/desktop-contracts'

/** The transformer pair tRPC links take (SuperJSON for Academy). */
export interface Transformer {
    serialize(value: unknown): unknown
    deserialize(value: unknown): unknown
}

export interface DesktopLinkOptions {
    client: DesktopClient
    /** The current learner reference, read at each operation. */
    learner: () => LearnerRef | null
    /**
     * Whether that reference is a signed-in account (false for the guest
     * generation). A refused procedure then reads as Academy's own sign-in
     * gate for a guest, and as not available on the desktop when signed in.
     */
    signedIn?: () => boolean
    transformer: Transformer
}

const REFUSAL_TEXT: Partial<Record<Refusal, string>> = {
    staleContext: 'The Academy account changed. Reload this page.',
    capabilityMissing: 'This Academy action is not available on the desktop.',
    invalidInput: 'The request could not be sent.',
    busy: 'The desktop is busy. Try again in a moment.',
    shuttingDown: 'The desktop is shutting down.',
}

/**
 * A client error the hooks can inspect like any other tRPC failure. `data`
 * also carries the native remote outcome (`notSent`, `rejected`, `unknown`)
 * so a possibly applied mutation is never mistaken for one that failed.
 */
function desktopError(
    message: string,
    code: string,
    outcome: Outcome,
    extra: { httpStatus?: number; failure?: CallFailure | Refusal } = {}
) {
    return TRPCClientError.from({
        error: {
            message,
            code: -32000,
            data: { code, httpStatus: extra.httpStatus ?? 0, desktop: true, outcome, failure: extra.failure ?? null },
        },
    })
}

const MAY_HAVE_APPLIED = 'Academy may have applied this change. Check its current state before trying again.'

function failureMessage(failure: CallFailure, outcome: Outcome, mutation: boolean): string {
    if (failure === 'sessionExpired' || failure === 'credentialUnavailable') {
        return 'Your Academy session expired. Sign in again.'
    }
    if (outcome === 'unknown' && mutation) return MAY_HAVE_APPLIED
    if (failure === 'cancelled') return 'The request was cancelled.'
    if (failure === 'invalidResponse') return 'Academy returned a reply the desktop does not accept.'
    return 'Academy could not be reached. Check your connection and try again.'
}

export function desktopLink<TRouter extends AnyRouter>(options: DesktopLinkOptions): TRPCLink<TRouter> {
    return () =>
        ({ op }) =>
            observable((observer) => {
                let stopped = false
                const fail = (error: unknown) => {
                    if (!stopped) observer.error(error as never)
                }
                const run = async () => {
                    if (op.type === 'subscription') {
                        throw desktopError('Live Academy updates are not available on the desktop.', 'METHOD_NOT_SUPPORTED', 'notSent')
                    }
                    const mutation = op.type === 'mutation'
                    const learner = options.learner()
                    if (!learner) throw desktopError('Sign in to Academy first.', 'UNAUTHORIZED', 'notSent', { httpStatus: 401 })
                    const input =
                        op.input === undefined ? null : JSON.stringify(options.transformer.serialize(op.input))
                    let response
                    try {
                        response = await options.client.dispatch({
                            kind: 'academyCall',
                            learner,
                            procedure: op.path,
                            procedureKind: op.type,
                            input,
                        })
                    } catch {
                        // Admission unknown: nothing is resent; the hook decides.
                        throw desktopError(
                            mutation ? MAY_HAVE_APPLIED : 'The desktop did not confirm the request.',
                            'CLIENT_CLOSED_REQUEST',
                            'unknown'
                        )
                    }
                    if (response.outcome === 'refusal') {
                        if (response.refusal === 'capabilityMissing') {
                            // Not Academy's FORBIDDEN (a grant or progression
                            // gate): a guest is asked to sign in, a signed-in
                            // learner is told the desktop does not carry it.
                            const guest = options.signedIn ? !options.signedIn() : false
                            throw guest
                                ? desktopError('Sign in to Academy to do this.', 'UNAUTHORIZED', 'notSent', {
                                      httpStatus: 401,
                                      failure: response.refusal,
                                  })
                                : desktopError(REFUSAL_TEXT.capabilityMissing!, 'METHOD_NOT_SUPPORTED', 'notSent', {
                                      httpStatus: 405,
                                      failure: response.refusal,
                                  })
                        }
                        throw desktopError(
                            REFUSAL_TEXT[response.refusal] ?? 'The request was refused.',
                            'PRECONDITION_FAILED',
                            'notSent',
                            { failure: response.refusal }
                        )
                    }
                    const operation = response.receipt.operation
                    if (!operation) throw desktopError('The desktop returned no operation.', 'INTERNAL_SERVER_ERROR', 'unknown')
                    let result
                    try {
                        result = await options.client.observeAcademyCall(operation)
                    } catch {
                        // Admitted but its result could not be read.
                        throw desktopError(
                            mutation ? MAY_HAVE_APPLIED : 'The result is no longer available.',
                            'TIMEOUT',
                            'unknown'
                        )
                    }
                    if (result.kind === 'failed') {
                        const expired = result.failure === 'sessionExpired' || result.failure === 'credentialUnavailable'
                        throw desktopError(
                            failureMessage(result.failure, result.outcome, mutation),
                            expired ? 'UNAUTHORIZED' : 'CLIENT_CLOSED_REQUEST',
                            result.outcome,
                            { httpStatus: result.failure === 'sessionExpired' ? 401 : 0, failure: result.failure }
                        )
                    }
                    const envelope =
                        result.kind === 'data'
                            ? { result: { data: JSON.parse(result.data) as unknown } }
                            : { error: JSON.parse(result.error) as unknown }
                    const transformed = transformResult(envelope as never, options.transformer)
                    if (!transformed.ok) {
                        // The server's own error, plus what is known of its effect
                        // (a 5xx may have applied a mutation).
                        const shape = transformed.error as {
                            error: { message?: string; data?: Record<string, unknown> }
                        }
                        if (result.kind === 'error') {
                            shape.error.data = { ...(shape.error.data ?? {}), outcome: result.outcome }
                            // A server error on a mutation may still have applied
                            // it: lead with that guidance and keep the server's
                            // own message and codes in `data`.
                            if (result.outcome === 'unknown' && mutation) {
                                shape.error.data.serverMessage = shape.error.message ?? null
                                shape.error.message = MAY_HAVE_APPLIED
                            }
                        }
                        throw TRPCClientError.from(transformed.error)
                    }
                    if (stopped) return
                    observer.next({ result: transformed.result })
                    observer.complete()
                }
                run().catch((error: unknown) =>
                    fail(error instanceof TRPCClientError ? error : TRPCClientError.from(error as Error))
                )
                return () => {
                    // Unsubscribing ignores the result; it does not cancel
                    // native work (a mutation's effect may already exist).
                    stopped = true
                }
            })
}
