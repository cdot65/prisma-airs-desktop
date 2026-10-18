/**
 * The Academy destination: Academy's own learner pages (catalog, paths,
 * courses, units with their lessons and knowledge checks) from
 * @ai-security-academy/learner, routed in memory and fed by the native
 * learner transport. Signed out, the guest generation reads the public
 * catalog; signed in, the account generation. Learner effects (completion
 * moments, awards) mount only here.
 */
import { forwardRef, lazy, Suspense, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react'
import {
    createMemoryHistory,
    createRootRoute,
    createRoute,
    createRouter,
    Link,
    Outlet,
    redirect,
    RouterProvider,
} from '@tanstack/react-router'
import { Button, Callout } from '@ai-security-academy/presentation'
import { Info } from 'lucide-react'
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
import { LearnerProvider } from './learner-provider'

interface Props {
    snapshot: ShellSnapshot
    client: DesktopClient
    onNavigate: (id: FeatureId) => void
    /** The learner path to open (default `/learn`). */
    initialPath?: string
}

// Fixture builds only; never loaded otherwise.
const LearnerCallProbe = lazy(() => import('./learner-call-probe'))

function themeFor(appearance: ShellSnapshot['appearance']): 'gruvbox' | 'paper' {
    if (appearance === 'dark') return 'gruvbox'
    if (appearance === 'light') return 'paper'
    return typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches
        ? 'gruvbox'
        : 'paper'
}

/**
 * Builds the in-memory learner router (one per mounted generation). Reading
 * routes serve guests too; account routes (Home, Program, assessment,
 * certificates, Shift, progression, account) ask a guest to sign in.
 */
function academyRouter(onNavigate: (id: FeatureId) => void, initialPath: string, signedIn: boolean) {
    const root = createRootRoute({
        component: () => (
            <>
                <AcademyNav signedIn={signedIn} />
                <LearnerTrail />
                <LearnerStatus />
                <Outlet />
            </>
        ),
        notFoundComponent: () => <NotInThisBuild />,
    })
    const signInFirst = () => <SignInHere onNavigate={onNavigate} />
    const account = (path: string, Page: () => React.ReactNode) =>
        createRoute({ getParentRoute: () => root, path, component: signedIn ? () => <>{Page()}</> : signInFirst })
    const index = account('/', () => <HomePage />)
    const program = account('/program', () => <ProgramHubPage />)
    const assessment = account('/assessment', () => <AssessmentPage />)
    const assessmentIntro = account('/assessment/intro', () => <AssessmentIntroPage />)
    const identityPractical = account('/assessment/identity-practical', () => <IdentityPracticalPage />)
    // The retired exam route goes to the assessment, as on Academy web.
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
    // The retired paths list folded into /learn, as on Academy web.
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
        // Retired slugs resolve to the current course before anything loads.
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
    const signIn = createRoute({
        getParentRoute: () => root,
        path: '/sign-in',
        component: () => <SignInHere onNavigate={onNavigate} />,
    })
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
        ]),
        history: createMemoryHistory({ initialEntries: [initialPath] }),
        defaultPreload: false,
        // A page that fails to render says so plainly; its error text (which
        // may name internals) is never shown.
        defaultErrorComponent: PageFailed,
    })
}

/**
 * Academy's own navigation: its primary sections, and for a signed-in learner
 * the web shell's Notifications and Settings entries. Guests see Learn only.
 */
function AcademyNav({ signedIn }: { signedIn: boolean }) {
    const items = LEARNER_NAV.filter((item) => signedIn || item.to === '/learn')
    return (
        <nav aria-label='Academy sections' data-slot='academy-nav' className='mb-4 flex flex-wrap items-center gap-1'>
            {items.map((item) => (
                <NavLink key={item.to} to={item.to} label={item.label} />
            ))}
            {signedIn ? (
                <span data-slot='academy-account-nav' className='ml-auto flex gap-1'>
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
            activeProps={{ 'aria-current': 'page', className: 'bg-academy-accent-soft text-academy-ink' }}
            className='text-academy-ink-soft hover:text-academy-ink rounded-academy-sm inline-flex min-h-11 items-center px-3 text-[13px] font-medium'
        >
            {label}
        </Link>
    )
}

/**
 * Session-critical status a learner page publishes (the final assessment's
 * timer, which also submits at Academy's deadline, and its proctored pill),
 * kept in view while the page scrolls. It unmounts with the page.
 */
function LearnerStatus() {
    const { status } = useLearnerPageHeader()
    if (!status) return null
    return (
        <div
            data-slot='academy-status'
            className='bg-academy-bg sticky top-0 z-10 mb-4 flex min-h-11 items-center justify-end gap-3 py-2'
        >
            {status}
        </div>
    )
}

/**
 * The trail each learner page publishes (Learn › Course › Unit), as links
 * within the Academy destination; the current page is the last crumb.
 */
function LearnerTrail() {
    const { breadcrumbs } = useLearnerPageHeader()
    if (breadcrumbs.length === 0) return null
    return (
        <nav aria-label='Academy location' data-slot='academy-trail' className='mb-4'>
            <ol className='text-academy-ink-soft flex flex-wrap items-center gap-1 text-[13px]'>
                {breadcrumbs.map((crumb, index) => {
                    const last = index === breadcrumbs.length - 1
                    return (
                        <li key={`${index}-${crumb.label}`} className='flex items-center gap-1'>
                            {crumb.to && !last ? (
                                <Link to={crumb.to} className='hover:text-academy-ink inline-flex min-h-11 items-center underline'>
                                    {crumb.label}
                                </Link>
                            ) : (
                                <span aria-current={last ? 'page' : undefined} className={last ? 'text-academy-ink' : undefined}>
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

function SignInHere({ onNavigate }: { onNavigate: (id: FeatureId) => void }) {
    return (
        <section aria-labelledby='academy-sign-in' className='mx-auto flex max-w-xl flex-col gap-4 py-10'>
            <h1 id='academy-sign-in' className='font-display text-academy-ink text-[28px] font-semibold'>
                Sign in to continue
            </h1>
            <p className='text-academy-ink-soft text-[14px]'>
                Your Academy account is set up in Settings. Signing in opens Academy in your browser; the desktop
                never sees your password.
            </p>
            <div className='flex gap-2'>
                <Button type='button' tier='primary' onClick={() => onNavigate('settings')}>
                    Open Settings
                </Button>
                <Link to='/learn' className='text-academy-ink-soft min-h-11 self-center text-[13px] underline'>
                    Back to the catalog
                </Link>
            </div>
        </section>
    )
}

function PageFailed() {
    return (
        <section aria-labelledby='academy-failed' className='mx-auto flex max-w-xl flex-col gap-4 py-10'>
            <h1 id='academy-failed' className='font-display text-academy-ink text-[28px] font-semibold'>
                This page could not be shown
            </h1>
            <p className='text-academy-ink-soft text-[14px]'>
                Something in this Academy page did not load correctly. Your progress is kept by Academy.
            </p>
            <Link to='/learn' className='text-academy-ink min-h-11 text-[13px] underline'>
                Back to the catalog
            </Link>
        </section>
    )
}

function NotInThisBuild() {
    return (
        <section aria-labelledby='academy-later' className='mx-auto flex max-w-xl flex-col gap-4 py-10'>
            <h1 id='academy-later' className='font-display text-academy-ink text-[28px] font-semibold'>
                Not in the desktop app
            </h1>
            <Callout tone='default' icon={Info}>
                <p>
                    This part of Academy is not part of the desktop app. Learning, the Program, assessments,
                    certificates and your account are here.
                </p>
            </Callout>
            <Link to='/learn' className='text-academy-ink min-h-11 text-[13px] underline'>
                Back to the catalog
            </Link>
        </section>
    )
}

function focusHeading(heading: HTMLElement) {
    if (!heading.hasAttribute('tabindex')) heading.setAttribute('tabindex', '-1')
    heading.focus()
}

/**
 * Moves focus to the page heading after each navigation, including the first
 * page the destination opens (keyboard and VoiceOver users).
 */
function useHeadingFocus(container: React.RefObject<HTMLElement | null>, router: ReturnType<typeof academyRouter>) {
    useEffect(() => {
        let tries = 0
        let frame = 0
        const tryFocus = () => {
            // Never take focus back from a control the learner moved to
            // outside the destination while the page was loading.
            const active = document.activeElement
            if (active && active !== document.body && !container.current?.contains(active)) return
            const heading = container.current?.querySelector('h1')
            if (heading) {
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
 * Lesson bodies are server-compiled HTML whose links are plain anchors, not
 * router links. A learner path routes within the destination (a plain
 * anchor would reload the whole window); an in-page anchor scrolls; any
 * other address stays out of the window (the navigation policy refuses
 * remote pages) and the learner is told where it points.
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
                setNotice('This link cannot be opened from the desktop app.')
                return
            }
            if (url.origin === window.location.origin) {
                setNotice(null)
                void router.navigate({ to: `${url.pathname}${url.search}${url.hash}` })
            } else if (url.protocol === 'https:' || url.protocol === 'http:') {
                setNotice(`This link goes to ${url.host}, outside the desktop app, and was not opened: ${url.href}`)
            } else {
                setNotice('This link cannot be opened from the desktop app.')
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
}: {
    onNavigate: (id: FeatureId) => void
    container: React.RefObject<HTMLElement | null>
    initialPath: string
    signedIn: boolean
}) {
    // One router per mounted generation: a new callback identity must not
    // reset the learner's place.
    const navigate = useRef(onNavigate)
    navigate.current = onNavigate
    const router = useMemo(
        () => academyRouter((id) => navigate.current(id), initialPath, signedIn),
        [initialPath, signedIn]
    )
    useHeadingFocus(container, router)
    const links = useLessonLinks(router)
    return (
        // Clicks on lesson anchors are routed here; keyboard activation of a
        // link dispatches the same click event.
        <div onClick={links.onClick}>
            <p role='status' aria-live='polite' data-slot='academy-link-notice' className='text-academy-ink-soft text-[13px] empty:hidden'>
                {links.notice}
            </p>
            <RouterProvider router={router} />
        </div>
    )
}

export const AcademyView = forwardRef<HTMLHeadingElement, Props>(function AcademyView({ snapshot, client, onNavigate, initialPath = '/learn' }, ref) {
    const container = useRef<HTMLElement>(null)
    // The shell focuses the destination after navigation: hand it the
    // page's own heading once it renders, or the region until then.
    useImperativeHandle(ref, () => {
        const target = container.current
        return {
            focus: () => {
                // The heading once the page shows it (useHeadingFocus), the
                // region until then.
                const heading = target?.querySelector('h1')
                if (heading) focusHeading(heading)
                else target?.focus()
            },
        } as unknown as HTMLHeadingElement
    })
    const learner = snapshot.learner
    const view = snapshot.academy
    const accountId = view.state === 'signedIn' ? view.accountId : null
    if (!learner) {
        return (
            <section ref={container} tabIndex={-1} aria-label='Academy' data-slot='academy'>
                <p role='status' className='text-academy-ink-soft text-[13px]'>
                    Academy is starting…
                </p>
            </section>
        )
    }
    return (
        <section ref={container} tabIndex={-1} aria-label='Academy' data-slot='academy' className='outline-none'>
            {view.state !== 'signedIn' ? (
                <p className='text-academy-ink-soft mb-4 text-[13px]' data-slot='academy-guest'>
                    Browsing as a guest. Sign in from Settings to track progress and take knowledge checks.
                </p>
            ) : null}
            <LearnerProvider
                client={client}
                learner={learner}
                accountId={accountId}
                displayName={view.state === 'signedIn' ? view.displayName : null}
                theme={themeFor(snapshot.appearance)}
                webOrigin={snapshot.academyOrigin}
            >
                <AcademyRoutes
                    onNavigate={onNavigate}
                    container={container}
                    initialPath={initialPath}
                    signedIn={accountId !== null}
                />
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
