import { forwardRef, lazy, Suspense, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Link, Outlet, redirect, RouterProvider } from '@tanstack/react-router'
import {
    AssessmentIntroPage,
    AssessmentPage,
    CertificatePage,
    CompletionMoments,
    CoursePage,
    currentCourseSlug,
    DossierPage,
    HomePage,
    IdentityPracticalPage,
    LEARNER_ACCOUNT_NAV,
    LEARNER_NAV,
    LearnerAccountPage,
    LearnPage,
    MyCertificatePage,
    NotificationsPage,
    OnboardingPage,
    OrientationPage,
    PathLandingPage,
    ProgramHubPage,
    ShiftEntryPage,
    ShopPage,
    UnitPage,
    useLearnerPageHeader,
    WeeklyBoardPage,
} from '@ai-security-academy/learner'
import type { DesktopClient, FeatureId, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { LearnerProvider, useLearnerTrpc } from '../../features/academy/learner-provider'
import { DownloadsPage, OfflineNotice, OfflineWorkProvider } from '../../features/academy/offline-work'
import { Btn } from '../controls'
import { Glyph, type GlyphName } from '../icons'
import { ACADEMY_COPY as C } from './copy'

export interface AcademyProps {
    snapshot: ShellSnapshot
    client: DesktopClient
    onNavigate: (id: FeatureId) => void
    /** The learner path to open (default `/learn`). */
    initialPath?: string
}

// Fixture builds only; never loaded otherwise.
const LearnerCallProbe = lazy(() => import('../../features/academy/learner-call-probe'))

const faint = { color: 'var(--v2-faint)' }
const soft = { color: 'var(--v2-soft)' }
const CARD = { background: 'var(--v2-paper)', border: '1px solid var(--v2-rule)', borderRadius: 12 }

/** The Rust-owned appearance as the learner host's theme. */
export function themeFor(appearance: ShellSnapshot['appearance']): 'gruvbox' | 'paper' {
    if (appearance === 'dark') return 'gruvbox'
    if (appearance === 'light') return 'paper'
    return typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'gruvbox' : 'paper'
}

/** A learning path as Academy's catalog lists it (the fields the row draws). */
interface PathRow {
    id: string
    slug: string
    title: string
    description: string
    familyKey: string | null
    unitCount: number
    unitsDone: number
    estMinutes: number | null
    locked: boolean
}

function pathRows(rows: unknown): PathRow[] {
    if (!Array.isArray(rows)) return []
    return rows.flatMap((r) => {
        if (!r || typeof r !== 'object') return []
        const o = r as Record<string, unknown>
        if (typeof o.id !== 'string' || typeof o.slug !== 'string' || typeof o.title !== 'string') return []
        return [
            {
                id: o.id,
                slug: o.slug,
                title: o.title,
                description: typeof o.description === 'string' ? o.description : '',
                familyKey: typeof o.familyKey === 'string' ? o.familyKey : null,
                unitCount: typeof o.unitCount === 'number' ? o.unitCount : 0,
                unitsDone: typeof o.unitsDone === 'number' ? o.unitsDone : 0,
                estMinutes: typeof o.estMinutes === 'number' ? o.estMinutes : null,
                locked: o.locked === true,
            },
        ]
    })
}

const FAMILY_GLYPH: Record<string, GlyphName> = { 'prisma-airs': 'shield', runtime: 'shield', gateway: 'route', 'red-teaming': 'cross', models: 'box', skills: 'code' }

/**
 * Builds the in-memory learner router (one per mounted generation), the
 * legacy destination's routes: reading routes serve guests too; account
 * routes ask a guest to sign in. The root draws the prototype's frame
 * around the page: the page on the left, the learner's place on the right,
 * the Learning paths row beneath.
 */
function academyRouter(onNavigate: (id: FeatureId) => void, initialPath: string, signedIn: boolean, place: () => React.ReactNode, learner: string) {
    const root = createRootRoute({
        component: () => (
            <>
                <div className='grid min-w-0' style={{ gridTemplateColumns: 'minmax(0, 1.4fr) minmax(0, 1fr)', gap: 16 }} data-slot='academy-frame'>
                    <section aria-label={C.pages} className='flex min-w-0 flex-col' style={{ ...CARD, padding: '20px 22px', gap: 10 }} data-slot='academy-pages'>
                        <LearnerTrail />
                        <LearnerStatus />
                        <Outlet />
                    </section>
                    <section aria-label={C.place} className='flex min-w-0 flex-col self-start' style={{ ...CARD, padding: '20px 22px', gap: 10 }} data-slot='academy-place'>
                        <span className='text-[11px] font-bold tracking-[0.05em] uppercase' style={faint}>
                            {C.place}
                        </span>
                        {place()}
                        <AcademyNav signedIn={signedIn} />
                        <OfflineNotice />
                    </section>
                </div>
                <PathsRow learner={learner} />
            </>
        ),
        notFoundComponent: () => <NotInThisBuild />,
    })
    const signInFirst = () => <SignInHere onNavigate={onNavigate} />
    const account = (path: string, Page: () => React.ReactNode) => createRoute({ getParentRoute: () => root, path, component: signedIn ? () => <>{Page()}</> : signInFirst })
    const index = account('/', () => <HomePage />)
    const program = account('/program', () => <ProgramHubPage />)
    const downloads = account('/downloads', () => <DownloadsPage />)
    const assessment = account('/assessment', () => <AssessmentPage />)
    const assessmentIntro = account('/assessment/intro', () => <AssessmentIntroPage />)
    const identityPractical = account('/assessment/identity-practical', () => <IdentityPracticalPage />)
    const exam = createRoute({
        getParentRoute: () => root,
        path: '/exam',
        beforeLoad: () => {
            throw redirect({ to: '/assessment', replace: true })
        },
    })
    const certificates = account('/certificate', () => <MyCertificatePage />)
    const certificate = createRoute({
        getParentRoute: () => root,
        path: '/certificate/$certId',
        component: signedIn
            ? function CertificateRoute() {
                  return <CertificatePage certId={certificate.useParams().certId} />
              }
            : signInFirst,
    })
    const shift = account('/shift', () => <ShiftEntryPage />)
    const dossier = account('/dossier', () => <DossierPage />)
    const leaderboard = account('/leaderboard', () => <WeeklyBoardPage />)
    const settings = account('/settings', () => <LearnerAccountPage />)
    const notifications = account('/notifications', () => <NotificationsPage />)
    const shop = account('/shop', () => <ShopPage />)
    const orientation = account('/orientation', () => <OrientationPage />)
    const onboarding = account('/onboarding', () => <OnboardingPage />)
    const learn = createRoute({ getParentRoute: () => root, path: '/learn', component: LearnPage })
    const paths = createRoute({
        getParentRoute: () => root,
        path: '/paths',
        beforeLoad: () => {
            throw redirect({ to: '/learn', replace: true })
        },
    })
    const path = createRoute({
        getParentRoute: () => root,
        path: '/paths/$pathSlug',
        component: function PathRoute() {
            return <PathLandingPage pathSlug={path.useParams().pathSlug} />
        },
    })
    const course = createRoute({
        getParentRoute: () => root,
        path: '/courses/$courseSlug',
        beforeLoad: ({ params }) => {
            const current = currentCourseSlug(params.courseSlug)
            if (current) throw redirect({ to: '/courses/$courseSlug', params: { courseSlug: current }, replace: true })
        },
        component: function CourseRoute() {
            return <CoursePage courseSlug={course.useParams().courseSlug} />
        },
    })
    const unit = createRoute({
        getParentRoute: () => root,
        path: '/units/$unitId',
        component: function UnitRoute() {
            return <UnitPage unitId={unit.useParams().unitId} />
        },
    })
    const signIn = createRoute({ getParentRoute: () => root, path: '/sign-in', component: () => <SignInHere onNavigate={onNavigate} /> })
    return createRouter({
        routeTree: root.addChildren([
            index,
            learn,
            paths,
            path,
            course,
            unit,
            signIn,
            program,
            assessment,
            assessmentIntro,
            identityPractical,
            exam,
            certificates,
            certificate,
            shift,
            dossier,
            leaderboard,
            settings,
            notifications,
            shop,
            orientation,
            onboarding,
            downloads,
        ]),
        history: createMemoryHistory({ initialEntries: [initialPath] }),
        defaultPreload: false,
        defaultErrorComponent: PageFailed,
    })
}

/** Academy's own sections (guests see Learn only) and, signed in, Downloads, Notifications and Settings. */
function AcademyNav({ signedIn }: { signedIn: boolean }) {
    const items = LEARNER_NAV.filter((item) => signedIn || item.to === '/learn')
    return (
        <nav aria-label={C.sections} data-slot='academy-nav' className='flex flex-wrap items-center' style={{ gap: 4 }}>
            {items.map((item) => (
                <NavLink key={item.to} to={item.to} label={item.label} />
            ))}
            {signedIn ? (
                <span data-slot='academy-account-nav' className='flex flex-wrap' style={{ gap: 4 }}>
                    <NavLink to='/downloads' label={C.downloadsLink} />
                    {LEARNER_ACCOUNT_NAV.map((item) => (
                        <NavLink key={item.to} to={item.to} label={item.label} />
                    ))}
                </span>
            ) : null}
        </nav>
    )
}

function NavLink({ to, label }: { to: string; label: string }) {
    return (
        <Link
            to={to}
            activeOptions={{ exact: to === '/' }}
            activeProps={{ 'aria-current': 'page', style: { background: 'var(--v2-acc-soft)', color: 'var(--v2-ink)' } }}
            className='inline-flex h-8 items-center rounded-[7px] px-3 text-[13px] font-medium'
            style={{ color: 'var(--v2-soft)' }}
        >
            {label}
        </Link>
    )
}

/** Session-critical status a learner page publishes (the assessment's timer), kept in view. */
function LearnerStatus() {
    const { status } = useLearnerPageHeader()
    if (!status) return null
    return (
        <div data-slot='academy-status' className='sticky top-0 z-10 flex min-h-11 items-center justify-end gap-3 py-2' style={{ background: 'var(--v2-paper)' }}>
            {status}
        </div>
    )
}

/** The trail each learner page publishes (Learn › Course › Unit), as links within the destination. */
function LearnerTrail() {
    const { breadcrumbs } = useLearnerPageHeader()
    if (breadcrumbs.length === 0) return null
    return (
        <nav aria-label='Academy location' data-slot='academy-trail'>
            <ol className='flex flex-wrap items-center text-[13px]' style={{ gap: 4, ...soft }}>
                {breadcrumbs.map((crumb, index) => {
                    const last = index === breadcrumbs.length - 1
                    return (
                        <li key={`${index}-${crumb.label}`} className='flex items-center' style={{ gap: 4 }}>
                            {crumb.to && !last ? (
                                <Link to={crumb.to} className='inline-flex min-h-8 items-center underline'>
                                    {crumb.label}
                                </Link>
                            ) : (
                                <span aria-current={last ? 'page' : undefined} style={last ? { color: 'var(--v2-ink)' } : undefined}>
                                    {crumb.label}
                                </span>
                            )}
                            {last ? null : <span aria-hidden='true'>›</span>}
                        </li>
                    )
                })}
            </ol>
        </nav>
    )
}

/** The Learning paths row: Academy's paths as its catalog lists them for this generation, read through the learner client. */
function PathsRow({ learner }: { learner: string }) {
    const trpc = useLearnerTrpc() as unknown as { paths: { list: { query(): Promise<unknown> } } }
    const query = useQuery({
        queryKey: ['v2', 'academy', 'paths', learner],
        queryFn: async () => pathRows(await trpc.paths.list.query()),
    })
    return (
        <section aria-labelledby='academy-paths' className='flex flex-col' style={{ gap: 12 }} data-slot='academy-paths'>
            <h2 id='academy-paths' className='text-[15px] font-bold' style={{ marginTop: 6 }}>
                {C.paths}
            </h2>
            {query.isPending ? (
                <p role='status' className='text-[13px]' style={faint}>
                    {C.pathsReading}
                </p>
            ) : query.isError ? (
                <p role='alert' className='text-[13px]' style={faint}>
                    {C.pathsFailed}
                </p>
            ) : query.data.length === 0 ? (
                <p className='text-[13px]' style={faint}>
                    {C.pathsEmpty}
                </p>
            ) : (
                <div className='grid' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))', gap: 16 }} data-slot='academy-path-cards'>
                    {query.data.map((p) => {
                        const pct = p.unitCount > 0 ? Math.round((100 * p.unitsDone) / p.unitCount) : 0
                        return (
                            <Link
                                key={p.id}
                                to='/paths/$pathSlug'
                                params={{ pathSlug: p.slug }}
                                aria-label={C.openPath(p.title)}
                                className='flex min-w-0 flex-col'
                                style={{ ...CARD, padding: '18px 20px', gap: 10, color: 'var(--v2-ink)' }}
                                data-slot='academy-path'
                            >
                                <span className='flex items-center' style={{ gap: 9 }}>
                                    <span className='grid shrink-0 place-items-center rounded-[8px]' style={{ width: 28, height: 28, background: 'var(--v2-inset)', color: 'var(--v2-soft)' }}>
                                        <Glyph name={FAMILY_GLYPH[p.familyKey ?? ''] ?? 'cap'} size={14} color='var(--v2-soft)' />
                                    </span>
                                    <b className='min-w-0 truncate'>{p.title}</b>
                                    {p.locked ? (
                                        <span className='ml-auto text-[12px]' style={faint}>
                                            {C.pathLocked}
                                        </span>
                                    ) : null}
                                </span>
                                <span className='text-[12px]' style={{ minHeight: 32, ...soft }}>
                                    {p.description}
                                </span>
                                <span
                                    className='rounded-full'
                                    style={{ height: 5, background: 'var(--v2-inset)' }}
                                    role='progressbar'
                                    aria-valuemin={0}
                                    aria-valuemax={100}
                                    aria-valuenow={pct}
                                    aria-label={`${p.title} progress`}
                                >
                                    <span className='block h-full rounded-full' style={{ width: `${pct}%`, background: 'var(--v2-acc)' }} />
                                </span>
                                <span className='text-[12px]' style={faint}>
                                    {C.pathMeta(p.unitCount, p.unitsDone, p.estMinutes)}
                                </span>
                            </Link>
                        )
                    })}
                </div>
            )}
        </section>
    )
}

function SignInHere({ onNavigate }: { onNavigate: (id: FeatureId) => void }) {
    return (
        <section aria-labelledby='academy-sign-in' className='flex flex-col' style={{ gap: 12, padding: '20px 0' }}>
            <h1 id='academy-sign-in' className='text-[22px] font-bold'>
                {C.signInTitle}
            </h1>
            <p className='text-[14px]' style={soft}>
                {C.signInText}
            </p>
            <div className='flex items-center' style={{ gap: 10 }}>
                <Btn tier='primary' onClick={() => onNavigate('settings')}>
                    {C.openSettings}
                </Btn>
                <Link to='/learn' className='text-[13px] underline' style={soft}>
                    {C.backToCatalog}
                </Link>
            </div>
        </section>
    )
}

function PageFailed() {
    return (
        <section aria-labelledby='academy-failed' className='flex flex-col' style={{ gap: 12, padding: '20px 0' }}>
            <h1 id='academy-failed' className='text-[22px] font-bold'>
                {C.failedTitle}
            </h1>
            <p className='text-[14px]' style={soft}>
                {C.failedText}
            </p>
            <Link to='/learn' className='text-[13px] underline'>
                {C.backToCatalog}
            </Link>
        </section>
    )
}

function NotInThisBuild() {
    return (
        <section aria-labelledby='academy-later' className='flex flex-col' style={{ gap: 12, padding: '20px 0' }}>
            <h1 id='academy-later' className='text-[22px] font-bold'>
                {C.notHereTitle}
            </h1>
            <p className='rounded-[10px] text-[13px]' style={{ padding: '12px 14px', background: 'var(--v2-acc-soft)' }}>
                {C.notHereText}
            </p>
            <Link to='/learn' className='text-[13px] underline'>
                {C.backToCatalog}
            </Link>
        </section>
    )
}

function focusHeading(heading: HTMLElement) {
    if (!heading.hasAttribute('tabindex')) heading.setAttribute('tabindex', '-1')
    heading.focus()
}

/** Moves focus to the page heading after each navigation (keyboard and VoiceOver users). */
function useHeadingFocus(container: React.RefObject<HTMLElement | null>, router: ReturnType<typeof academyRouter>) {
    useEffect(() => {
        let tries = 0
        let frame = 0
        const tryFocus = () => {
            const active = document.activeElement
            if (active && active !== document.body && !container.current?.contains(active)) return
            const heading = container.current?.querySelector('[data-slot="academy-pages"] h1')
            if (heading instanceof HTMLElement) {
                focusHeading(heading)
            } else if (tries++ < 300) {
                frame = requestAnimationFrame(tryFocus)
            }
        }
        const unsubscribe = router.subscribe('onResolved', ({ pathChanged }) => {
            if (!pathChanged) return
            tries = 0
            cancelAnimationFrame(frame)
            frame = requestAnimationFrame(tryFocus)
        })
        return () => {
            unsubscribe()
            cancelAnimationFrame(frame)
        }
    }, [container, router])
}

/**
 * Lesson bodies are server-compiled HTML whose links are plain anchors. A
 * learner path routes within the destination; an in-page anchor scrolls;
 * any other address stays out of the window and the learner is told.
 */
function useLessonLinks(router: ReturnType<typeof academyRouter>) {
    const [notice, setNotice] = useState<string | null>(null)
    const onClick = useCallback(
        (event: React.MouseEvent<HTMLElement>) => {
            if (event.defaultPrevented) return
            const anchor = (event.target as Element | null)?.closest?.('a[href]')
            if (!anchor || !event.currentTarget.contains(anchor)) return
            event.preventDefault()
            const href = anchor.getAttribute('href') ?? ''
            if (href.startsWith('#')) {
                const target = href.length > 1 ? event.currentTarget.querySelector(`[id="${CSS.escape(href.slice(1))}"]`) : null
                target?.scrollIntoView({ block: 'start' })
                setNotice(null)
                return
            }
            let url: URL
            try {
                url = new URL(href, window.location.href)
            } catch {
                setNotice(C.linkUnopenable)
                return
            }
            if (url.origin === window.location.origin) {
                setNotice(null)
                void router.navigate({ to: `${url.pathname}${url.search}${url.hash}` })
            } else if (url.protocol === 'https:' || url.protocol === 'http:') {
                setNotice(C.linkOutside(url.host, url.href))
            } else {
                setNotice(C.linkUnopenable)
            }
        },
        [router]
    )
    return { notice, onClick }
}

function AcademyRoutes({
    onNavigate,
    container,
    initialPath,
    signedIn,
    place,
    learner,
}: {
    onNavigate: (id: FeatureId) => void
    container: React.RefObject<HTMLElement | null>
    initialPath: string
    signedIn: boolean
    place: () => React.ReactNode
    learner: string
}) {
    const navigate = useRef(onNavigate)
    navigate.current = onNavigate
    const placeRef = useRef(place)
    placeRef.current = place
    const router = useMemo(
        () =>
            academyRouter(
                (id) => navigate.current(id),
                initialPath,
                signedIn,
                () => placeRef.current(),
                learner
            ),
        [initialPath, signedIn, learner]
    )
    useHeadingFocus(container, router)
    const links = useLessonLinks(router)
    return (
        <div onClick={links.onClick} className='flex min-w-0 flex-col' style={{ gap: 20 }}>
            <p role='status' aria-live='polite' data-slot='academy-link-notice' className='text-[13px] empty:hidden' style={soft}>
                {links.notice}
            </p>
            <RouterProvider router={router} />
        </div>
    )
}

/**
 * The Academy as R2App draws it (C42): the title row with the learner's
 * pills, the 1.4fr/1fr frame (Academy's own learner pages on the left,
 * the learner's place on the right) and the Learning paths row, around
 * the hosted learner pages with the Rust-owned theme. Signed out, the
 * guest generation reads the public catalog; signed in, the account
 * generation. Learner effects mount only here. The embed's content is
 * Academy's own (P19); nothing here signs in or sends on its own.
 */
export const Academy = forwardRef<HTMLHeadingElement, AcademyProps>(function Academy({ snapshot, client, onNavigate, initialPath = '/learn' }, ref) {
    const container = useRef<HTMLElement>(null)
    useImperativeHandle(ref, () => {
        const target = container.current
        return {
            focus: () => {
                const heading = target?.querySelector('[data-slot="academy-pages"] h1')
                if (heading instanceof HTMLElement) focusHeading(heading)
                else target?.querySelector<HTMLElement>('h1')?.focus()
            },
        } as unknown as HTMLHeadingElement
    })
    const learner = snapshot.learner
    const view = snapshot.academy
    const offline = snapshot.academyOffline
    const accountId = view.state === 'signedIn' ? view.accountId : null
    const displayName = view.state === 'signedIn' ? view.displayName : null
    const unreachable = !offline.reachable && offline.state !== 'signedOut'

    const band = (
        <div data-slot='page-header-v2' className='flex flex-wrap items-end' style={{ gap: 12 }}>
            <div className='min-w-0'>
                <h1 id='academy-heading' tabIndex={-1} className='font-bold' style={{ fontSize: 'var(--v2-title)', letterSpacing: '-0.01em' }}>
                    {C.title}
                </h1>
                <p className='text-[14px]' style={{ marginTop: 4, ...soft }}>
                    {C.lead}
                </p>
            </div>
            <span className='ml-auto flex flex-wrap items-center' style={{ gap: 8 }} data-slot='academy-pills'>
                <span
                    className='flex items-center rounded-full text-[13px] font-semibold'
                    style={{ height: 26, padding: '0 12px', gap: 6, background: displayName ? 'var(--v2-acc-soft)' : 'var(--v2-inset)', color: displayName ? 'var(--v2-acc)' : 'var(--v2-soft)' }}
                    data-slot='academy-account'
                >
                    <Glyph name={displayName ? 'cap' : 'eye'} size={12} color='currentColor' />
                    {displayName ? C.signedIn(displayName) : C.guest}
                </span>
                {unreachable ? (
                    <span
                        className='flex items-center rounded-full text-[13px] font-semibold'
                        style={{ height: 26, padding: '0 12px', gap: 6, background: 'var(--v2-flag-bg)', color: 'var(--v2-flag)' }}
                        data-slot='academy-offline-pill'
                    >
                        {C.offline}
                    </span>
                ) : null}
                {accountId && offline.downloads.length ? (
                    <span
                        className='flex items-center rounded-full text-[13px] font-semibold'
                        style={{ height: 26, padding: '0 12px', background: 'var(--v2-inset)', color: 'var(--v2-soft)' }}
                        data-slot='academy-downloads-pill'
                    >
                        {C.downloads(offline.downloads.length)}
                    </span>
                ) : null}
            </span>
        </div>
    )

    const place = () => (
        <>
            {displayName ? (
                <p className='text-[13px]' style={soft}>
                    {C.signedInNote(displayName)}
                </p>
            ) : (
                <>
                    <p className='text-[13px]' style={soft} data-slot='academy-guest'>
                        {C.guestNote}
                    </p>
                    <Btn className='self-start' onClick={() => onNavigate('settings')}>
                        {C.openSettings}
                    </Btn>
                </>
            )}
            {accountId && offline.localDrafts.length ? (
                <p className='text-[12px]' style={faint} data-slot='academy-drafts'>
                    {C.drafts(offline.localDrafts.length)}
                </p>
            ) : null}
        </>
    )

    if (!learner) {
        return (
            <section ref={container} tabIndex={-1} aria-label={C.region} data-slot='academy-v2' className='flex min-w-0 flex-col' style={{ gap: 20 }}>
                {band}
                <p role='status' className='text-[13px]' style={soft}>
                    {C.starting}
                </p>
            </section>
        )
    }
    return (
        <section ref={container} tabIndex={-1} aria-label={C.region} data-slot='academy-v2' className='flex min-w-0 flex-col outline-none' style={{ gap: 20 }}>
            {band}
            <LearnerProvider client={client} learner={learner} accountId={accountId} displayName={displayName} theme={themeFor(snapshot.appearance)} webOrigin={snapshot.academyOrigin}>
                <OfflineWorkProvider value={{ client, learner, offline }}>
                    <AcademyRoutes onNavigate={onNavigate} container={container} initialPath={initialPath} signedIn={accountId !== null} place={place} learner={learner} />
                </OfflineWorkProvider>
                <CompletionMoments />
                {snapshot.fixtureBuild ? (
                    <Suspense fallback={null}>
                        <LearnerCallProbe />
                    </Suspense>
                ) : null}
            </LearnerProvider>
        </section>
    )
})
