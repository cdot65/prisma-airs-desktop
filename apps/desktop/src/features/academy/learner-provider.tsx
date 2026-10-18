/**
 * Account-scoped React Query cache and Academy learner host. Every learner
 * reference (an account generation, or the guest generation while signed
 * out) gets its own QueryClient and a remounted subtree: signing in or out,
 * or changing account, retires the previous cache, so no reply or cached
 * value for one account is shown to another. Tenant switches do not change
 * the learner reference and keep the cache.
 */
import { createContext, type ReactNode, useContext, useEffect, useMemo } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { createTRPCClient, TRPCClientError, type TRPCClient } from '@trpc/client'
import type { AnyRouter } from '@trpc/server'
import SuperJSON from 'superjson'
import { LearnerHost } from '@ai-security-academy/learner'
import type { DesktopClient, LearnerRef } from '@prisma-airs/desktop-contracts'
import { desktopLink } from './desktop-link'

const NO_RETRY = new Set([
    'UNAUTHORIZED',
    'FORBIDDEN',
    'BAD_REQUEST',
    'NOT_FOUND',
    'PRECONDITION_FAILED',
    'METHOD_NOT_SUPPORTED',
])

/** Queries may be asked once more after a transient failure; mutations never. */
export function learnerQueryClient(): QueryClient {
    return new QueryClient({
        defaultOptions: {
            queries: {
                staleTime: 60_000,
                retry: (count, error) =>
                    count < 1 &&
                    !(error instanceof TRPCClientError && NO_RETRY.has(String((error.data as { code?: unknown })?.code))),
            },
            mutations: { retry: false },
        },
    })
}

const TrpcContext = createContext<TRPCClient<AnyRouter> | null>(null)

/** A plain learner tRPC client for the current generation (inside a provider). */
export function useLearnerTrpc<TRouter extends AnyRouter>(): TRPCClient<TRouter> {
    const client = useContext(TrpcContext)
    if (!client) throw new Error('useLearnerTrpc needs a LearnerProvider')
    return client as unknown as TRPCClient<TRouter>
}

interface Props {
    client: DesktopClient
    learner: LearnerRef
    /** Academy's account id when signed in; null for the guest generation. */
    accountId?: string | null
    theme?: 'gruvbox' | 'paper'
    children: ReactNode
}

function Generation({ client, learner, accountId = null, theme = 'gruvbox', children }: Props) {
    const queryClient = useMemo(() => learnerQueryClient(), [])
    const links = useMemo(
        () => [
            desktopLink<AnyRouter>({
                client,
                learner: () => learner,
                signedIn: () => accountId !== null,
                transformer: SuperJSON,
            }),
        ],
        [client, learner, accountId]
    )
    const trpc = useMemo(() => createTRPCClient<AnyRouter>({ links }), [links])
    // The generation is Academy's learner session: a new one (sign-in,
    // sign-out, reset, even for the same account) retires Academy's
    // learner-scoped state.
    const identity = useMemo(() => ({ userId: accountId, session: learner }), [accountId, learner])
    useEffect(() => () => queryClient.clear(), [queryClient])
    return (
        <QueryClientProvider client={queryClient}>
            <TrpcContext.Provider value={trpc}>
                <LearnerHost
                    links={links}
                    queryClient={queryClient}
                    identity={identity}
                    theme={theme}
                    labs='preview'
                    diagrams='prerendered'
                >
                    {children}
                </LearnerHost>
            </TrpcContext.Provider>
        </QueryClientProvider>
    )
}

export function LearnerProvider(props: Props) {
    // Keyed by generation: query observers bind to the client they first
    // saw, so the learner subtree remounts and keeps nothing from the
    // previous account.
    return <Generation key={props.learner} {...props} />
}
