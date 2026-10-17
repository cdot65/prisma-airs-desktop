/**
 * Account-scoped React Query and tRPC clients for Academy learner screens.
 * Every learner reference gets its own QueryClient: signing out or changing
 * account retires the previous cache, so no reply or cached value for one
 * account is shown to another. Tenant switches do not change the learner
 * reference and keep the cache.
 */
import { createContext, type ReactNode, useContext, useEffect, useMemo } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { createTRPCClient, TRPCClientError, type TRPCClient } from '@trpc/client'
import type { AnyRouter } from '@trpc/server'
import SuperJSON from 'superjson'
import type { DesktopClient, LearnerRef } from '@prisma-airs/desktop-contracts'
import { desktopLink } from './desktop-link'

const NO_RETRY = new Set(['UNAUTHORIZED', 'FORBIDDEN', 'BAD_REQUEST', 'NOT_FOUND', 'PRECONDITION_FAILED'])

/** Queries may be asked once more after a transient failure; mutations never. */
export function learnerQueryClient(): QueryClient {
    return new QueryClient({
        defaultOptions: {
            queries: {
                retry: (count, error) =>
                    count < 1 &&
                    !(error instanceof TRPCClientError && NO_RETRY.has(String((error.data as { code?: unknown })?.code))),
            },
            mutations: { retry: false },
        },
    })
}

const TrpcContext = createContext<TRPCClient<AnyRouter> | null>(null)

/** The learner tRPC client for the current account (inside a provider). */
export function useLearnerTrpc<TRouter extends AnyRouter>(): TRPCClient<TRouter> {
    const client = useContext(TrpcContext)
    if (!client) throw new Error('useLearnerTrpc needs a signed-in LearnerProvider')
    return client as unknown as TRPCClient<TRouter>
}

export function LearnerProvider({
    client,
    learner,
    children,
}: {
    client: DesktopClient
    learner: LearnerRef
    children: ReactNode
}) {
    // A new account generation always gets a fresh cache.
    const queryClient = useMemo(() => learnerQueryClient(), [learner])
    const trpc = useMemo(
        () =>
            createTRPCClient<AnyRouter>({
                links: [desktopLink<AnyRouter>({ client, learner: () => learner, transformer: SuperJSON })],
            }),
        [client, learner]
    )
    useEffect(() => () => queryClient.clear(), [queryClient])
    return (
        // Keyed by account: query observers bind to the client they first
        // saw, so the learner subtree remounts and keeps nothing from the
        // previous account.
        <QueryClientProvider key={learner} client={queryClient}>
            <TrpcContext.Provider value={trpc}>{children}</TrpcContext.Provider>
        </QueryClientProvider>
    )
}
