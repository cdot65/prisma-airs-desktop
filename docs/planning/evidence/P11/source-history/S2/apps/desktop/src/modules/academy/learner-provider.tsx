/**
 * Account-scoped React Query cache and Academy learner host. Every learner
 * reference (an account generation, or the guest generation while signed
 * out) gets its own QueryClient and a remounted subtree: signing in or out,
 * or changing account, retires the previous cache, so no reply or cached
 * value for one account is shown to another. Tenant switches do not change
 * the learner reference and keep the cache.
 */
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { createTRPCClient, TRPCClientError, type TRPCClient } from '@trpc/client'
import type { AnyRouter } from '@trpc/server'
import SuperJSON from 'superjson'
import { LearnerHost } from '@ai-security-academy/learner'
import type { DesktopClient, LearnerRef } from '@prisma-airs/desktop-contracts'
import { desktopLink } from './desktop-link'
import { hostAction, loadAcademyImage, nativeLocalDrafts } from './host-actions'

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
    /** The signed-in account's display name, for Academy's account pages. */
    displayName?: string | null
    theme?: 'gruvbox' | 'paper'
    /** Academy web's origin (public links such as certificate verify URLs). */
    webOrigin?: string | null
    children: ReactNode
}

function Generation({
    client,
    learner,
    accountId = null,
    displayName = null,
    theme = 'gruvbox',
    webOrigin = null,
    children,
}: Props) {
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
    const identity = useMemo(
        () => ({
            userId: accountId,
            session: learner,
            // The desktop knows the account's display name only; the email
            // and uploaded photo come from Academy's settings read.
            profile: accountId ? { fullName: displayName, firstName: null, email: null, imageUrl: null } : null,
        }),
        [accountId, learner, displayName]
    )
    // Native actions Academy's pages ask the host for: sign-out ends the
    // learner session in the core; print opens the native print sheet; the
    // avatar is chosen, checked and uploaded natively and read back as a
    // data URL (this WebView loads no remote images).
    const onSignOut = useCallback(() => void client.dispatch({ kind: 'signOutAcademy' }).catch(() => undefined), [client])
    const onPrint = useCallback(() => hostAction(client, { kind: 'printPage', learner }), [client, learner])
    const onUploadAvatar = useCallback(() => hostAction(client, { kind: 'uploadAvatar', learner }), [client, learner])
    const loadImage = useCallback(
        (url: string) => loadAcademyImage(client, { kind: 'loadAcademyImage', learner, url }),
        [client, learner]
    )
    // Lab work Academy has not confirmed is sealed in the core for this
    // account (signed in only); the core never sends it by itself.
    const localDrafts = useMemo(
        () => (accountId === null ? undefined : nativeLocalDrafts(client, learner)),
        [client, learner, accountId]
    )
    useEffect(() => () => queryClient.clear(), [queryClient])
    return (
        <QueryClientProvider client={queryClient}>
            <TrpcContext.Provider value={trpc}>
                <LearnerHost
                    links={links}
                    queryClient={queryClient}
                    identity={identity}
                    theme={theme}
                    labs='desktop'
                    diagrams='prerendered'
                    webOrigin={webOrigin}
                    onSignOut={onSignOut}
                    onPrint={onPrint}
                    remoteImages={false}
                    loadImage={loadImage}
                    avatarUpload={accountId !== null}
                    onUploadAvatar={onUploadAvatar}
                    appearance='host'
                    assessorTools={false}
                    awardPresenters={false}
                    courseReset
                    lessonFeedback
                    localDrafts={localDrafts}
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
