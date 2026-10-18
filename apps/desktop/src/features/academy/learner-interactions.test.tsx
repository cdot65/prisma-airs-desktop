/**
 * C06 learner interactions through the production desktop transport:
 * Academy's own assessment, account, certificate and Program pages in the
 * Academy destination, with hosted authority (deadlines, grading, awards and
 * submission state come from Academy), truthful unknown outcomes and the
 * desktop's native host actions (sign-out, print), Shift timing that slow
 * optional reads never block, and course removal behind Academy's
 * affected-certification confirmation. Guests are asked to sign in for
 * account sections and nothing is sent for them.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AcademyView } from './academy-view'
import { ACADEMY_ORIGIN, ACCOUNT, academySnapshot, held, lostReply, result, scriptedNative, type Replies } from './test-support'

function academy(path: string, replies: Replies, signedIn = true) {
    const native = scriptedNative(replies)
    render(<AcademyView snapshot={academySnapshot(signedIn)} client={native.client} onNavigate={vi.fn()} initialPath={path} />)
    return native
}

const SETTINGS = {
    fullName: 'Ada Lovelace',
    displayName: 'Ada Lovelace',
    role: 'Security Engineer',
    region: 'EMEA',
    preferences: {
        emailDigests: true,
        streakReminders: true,
        proctorAlerts: false,
        labSounds: false,
        dailyIncidentAlerts: true,
        weeklyRecap: true,
        autoplayVideos: false,
    },
    accent: 'orange',
    theme: 'gruvbox',
    slaAccommodation: 'standard',
    onboardingCompletedAt: '2026-10-01T00:00:00.000Z',
    activeCertificationId: 'ai-app-security',
    account: { email: 'ada@example.com', firstName: 'Ada', lastName: 'Lovelace', imageUrl: null, avatarUrl: null },
}

let fetchSpy: ReturnType<typeof vi.fn>
beforeEach(() => {
    fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
    window.scrollTo = vi.fn() as never
    window.matchMedia = vi.fn().mockReturnValue({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }) as never
})
afterEach(() => {
    // The learner transport is the native core; the WebView never fetches.
    expect(fetchSpy).not.toHaveBeenCalled()
    vi.unstubAllGlobals()
})

describe('account sections', () => {
    it.each(['/', '/program', '/assessment', '/certificate', '/shift', '/dossier', '/settings'])(
        'ask a guest to sign in at %s and send nothing for it',
        async (path) => {
            const { calls } = academy(path, {}, false)
            expect(await screen.findByRole('heading', { level: 1, name: 'Sign in to continue' })).toBeInTheDocument()
            expect(calls).toEqual([])
            const nav = screen.getByRole('navigation', { name: 'Academy sections' })
            expect(within(nav).getAllByRole('link').map((l) => l.textContent)).toEqual(['Learn'])
        }
    )

    it("show Academy's own navigation to a signed-in learner", async () => {
        academy('/learn', { 'paths.list': [], 'library.list': [] })
        const nav = await screen.findByRole('navigation', { name: 'Academy sections' })
        expect(within(nav).getAllByRole('link').map((l) => l.textContent)).toEqual(['Home', 'Learn', 'Program', 'Notifications', 'Settings'])
        expect(within(nav).getByRole('link', { name: 'Learn' })).toHaveAttribute('aria-current', 'page')
    })
})

describe('learner account', () => {
    const replies = {
        'settings.get': SETTINGS,
        'shop.list': { items: [], credits: 0 },
        'gamification.summary': null,
    }

    it("is reached from Academy's Settings entry", async () => {
        const user = userEvent.setup()
        academy('/learn', { ...replies, 'paths.list': [], 'library.list': [] })
        const nav = await screen.findByRole('navigation', { name: 'Academy sections' })
        await user.click(within(nav).getByRole('link', { name: 'Settings' }))
        expect(await screen.findByRole('heading', { level: 1, name: 'Ada Lovelace' })).toBeInTheDocument()
        expect(within(nav).getByRole('link', { name: 'Settings' })).toHaveAttribute('aria-current', 'page')
    })

    it('signs out through the desktop and leaves appearance to the app', async () => {
        const user = userEvent.setup()
        const { intents } = academy('/settings', replies)
        expect(await screen.findByRole('heading', { level: 1, name: 'Ada Lovelace' })).toBeInTheDocument()
        expect(screen.getByText(/This app sets the theme and accent/)).toBeInTheDocument()
        expect(screen.queryByRole('radiogroup', { name: 'Interface theme' })).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Sign out' }))
        expect(intents.at(-1)).toEqual({ kind: 'signOutAcademy' })
    })

    const AVATAR = `${ACADEMY_ORIGIN}/api/avatars/user_2fixturelearner.png?v=7`
    const PNG = 'data:image/png;base64,iVBORw0KGgo='
    const JPEG = 'data:image/jpeg;base64,/9j/4AAQ'
    const MAY_HAVE_CHANGED =
        'Your photo may have changed, but Academy did not confirm it. The photo shown is what Academy serves now; upload again if it is not the one you chose.'
    const withAvatar = { ...SETTINGS, account: { ...SETTINGS.account, avatarUrl: AVATAR } }

    it('uploads a new photo natively, re-reads Academy and shows the photo the core reads back', async () => {
        const user = userEvent.setup()
        let uploaded = false
        const { intents, calls } = academy('/settings', (procedure, _learner, input) => {
            if (procedure === 'settings.get') return uploaded ? withAvatar : SETTINGS
            if (procedure === '@uploadAvatar') {
                uploaded = true
                return withAvatar
            }
            if (procedure === '@loadAcademyImage') return input === AVATAR ? { src: PNG } : undefined
            return replies[procedure as keyof typeof replies]
        })
        await user.click(await screen.findByRole('button', { name: 'Change photo' }))
        expect(intents).toContainEqual({ kind: 'uploadAvatar', learner: ACCOUNT })
        // The renderer never presigns or uploads; the core does.
        expect(calls.some((c) => c.procedure.startsWith('settings.avatar') || c.procedure === 'settings.confirmAvatar')).toBe(false)
        await waitFor(() => expect(document.querySelector('[data-slot="avatar-upload"] img')).toHaveAttribute('src', PNG))
        expect(intents).toContainEqual({ kind: 'loadAcademyImage', learner: ACCOUNT, url: AVATAR })
        expect(screen.queryByRole('alert')).toBeNull()
    })

    it.each([
        ['cancelled in the picker', result({ kind: 'failed', failure: 'cancelled', outcome: 'notSent' }), null],
        ['an unsupported image', result({ kind: 'failed', failure: 'unsupportedImage', outcome: 'notSent' }), 'Upload failed. Your photo is unchanged; try again.'],
        ['a refused destination', result({ kind: 'failed', failure: 'destinationRefused', outcome: 'notSent' }), 'Upload failed. Your photo is unchanged; try again.'],
        ['a denied upload', result({ kind: 'error', error: JSON.stringify({ json: { message: 'x', code: -32003, data: { code: 'FORBIDDEN', httpStatus: 403 } } }), httpStatus: 403, outcome: 'rejected' }), 'Upload failed. Your photo is unchanged; try again.'],
        ['an unconfirmed confirmation', result({ kind: 'failed', failure: 'network', outcome: 'unknown' }), MAY_HAVE_CHANGED],
        ['a stored photo whose reply was lost', result({ kind: 'failed', failure: 'uploadFailed', outcome: 'unknown' }), MAY_HAVE_CHANGED],
        ['a stored photo whose confirmation was denied', result({ kind: 'error', error: JSON.stringify({ json: { message: 'x', code: -32004, data: { code: 'NOT_FOUND', httpStatus: 404 } } }), httpStatus: 404, outcome: 'unknown' }), MAY_HAVE_CHANGED],
        ['a store that refused the photo', result({ kind: 'failed', failure: 'uploadFailed', outcome: 'rejected' }), 'Upload failed. Your photo is unchanged; try again.'],
    ])('reports %s truthfully', async (_case, reply, message) => {
        const user = userEvent.setup()
        let reads = 0
        academy('/settings', (procedure) => {
            if (procedure === 'settings.get') {
                reads += 1
                return SETTINGS
            }
            if (procedure === '@uploadAvatar') return reply
            return replies[procedure as keyof typeof replies]
        })
        await user.click(await screen.findByRole('button', { name: 'Change photo' }))
        if (message) expect(await screen.findByRole('alert')).toHaveTextContent(message)
        else {
            await new Promise((r) => setTimeout(r, 50))
            expect(screen.queryByRole('alert')).toBeNull()
        }
        // Academy is re-read only when the photo may have changed.
        await new Promise((r) => setTimeout(r, 50))
        expect(reads > 1).toBe(message === MAY_HAVE_CHANGED)
    })

    it('reads the photo Academy serves again after an upload that may have replaced it at the same address', async () => {
        const user = userEvent.setup()
        let served = PNG
        const { intents } = academy('/settings', (procedure, _learner, input) => {
            if (procedure === 'settings.get') return withAvatar
            if (procedure === '@loadAcademyImage') return input === AVATAR ? { src: served } : undefined
            if (procedure === '@uploadAvatar') {
                served = JPEG
                return result({ kind: 'failed', failure: 'uploadFailed', outcome: 'unknown' })
            }
            return replies[procedure as keyof typeof replies]
        })
        await waitFor(() => expect(document.querySelector('[data-slot="avatar-upload"] img')).toHaveAttribute('src', PNG))
        await user.click(screen.getByRole('button', { name: 'Change photo' }))
        expect(await screen.findByRole('alert')).toHaveTextContent(MAY_HAVE_CHANGED)
        await waitFor(() => expect(document.querySelector('[data-slot="avatar-upload"] img')).toHaveAttribute('src', JPEG))
        expect(intents.filter((i) => i.kind === 'loadAcademyImage' && i.url === AVATAR).length).toBeGreaterThanOrEqual(2)
    })

    it('removes the photo with Academy and returns to initials', async () => {
        const user = userEvent.setup()
        let removed = false
        const { calls } = academy('/settings', (procedure) => {
            if (procedure === 'settings.get') return removed ? SETTINGS : withAvatar
            if (procedure === 'settings.removeAvatar') {
                removed = true
                return SETTINGS
            }
            if (procedure === '@loadAcademyImage') return { src: PNG }
            return replies[procedure as keyof typeof replies]
        })
        await waitFor(() => expect(document.querySelector('[data-slot="avatar-upload"] img')).toHaveAttribute('src', PNG))
        await user.click(screen.getByRole('button', { name: 'Remove photo' }))
        await waitFor(() => expect(document.querySelector('[data-slot="avatar-upload"] img')).toBeNull())
        expect(calls.filter((c) => c.procedure === 'settings.removeAvatar')).toHaveLength(1)
    })
})

describe('certificates', () => {
    const certificate = {
        certId: 'cert-certification',
        kind: 'certification',
        certificationId: 'ai-app-security',
        certTitle: 'Certified AI Application Security Professional',
        certificationTitle: 'AI Application Security',
        score: 92,
        breakdown: { courses: [{ courseId: 'ai-gateway', weight: 1, earnedWeight: 1, scorePercent: 100 }] },
        issuedAt: '2026-06-30T00:00:00.000Z',
        revokedAt: null,
        recipientName: 'Ada Lovelace',
    }

    it("link verification to Academy web and download through the desktop's print sheet", async () => {
        const user = userEvent.setup()
        const { intents } = academy('/certificate', { 'certificate.mine': [certificate], '@printPage': { opened: true } })
        await screen.findByTestId('certificate-credential')
        // The public verify URL (shared to LinkedIn, copied, in the QR code)
        // is on Academy web's origin, not the desktop's.
        const share = screen.getByRole('link', { name: /Add to LinkedIn/ }).getAttribute('href') ?? ''
        expect(new URL(share).searchParams.get('certUrl')).toBe(`${ACADEMY_ORIGIN}/verify/cert-certification`)
        await user.click(screen.getByRole('button', { name: /Download PDF/ }))
        expect(intents.at(-1)).toEqual({ kind: 'printPage', learner: ACCOUNT })
        expect(screen.queryByRole('alert')).toBeNull()
    })

    it('says so when the print sheet cannot open and changes nothing', async () => {
        const user = userEvent.setup()
        const { calls } = academy('/certificate', {
            'certificate.mine': [certificate],
            '@printPage': result({ kind: 'failed', failure: 'hostUnavailable', outcome: 'notSent' }),
        })
        await screen.findByTestId('certificate-credential')
        const before = calls.length
        await user.click(screen.getByRole('button', { name: /Download PDF/ }))
        expect(await screen.findByRole('alert')).toHaveTextContent('The print sheet could not open. Your certificate is unchanged')
        expect(calls.slice(before).filter((c) => c.procedureKind === 'mutation')).toEqual([])
        expect(screen.getByTestId('certificate-credential')).toBeInTheDocument()
    })
})

describe('final assessment', () => {
    const ATTEMPT = '00000000-0000-4000-8000-0000000000e1'
    const active = {
        attemptId: ATTEMPT,
        // Academy's deadline: the desktop never computes or sends one.
        deadline: new Date(Date.now() + 60 * 60 * 1000).toISOString(),
        questions: [
            {
                id: 'q1',
                moduleId: 'm',
                objectiveId: 'o',
                type: 'mcq',
                prompt: 'Where does AIRS evaluate a request?',
                options: ['Before the model', 'After shipping'],
            },
        ],
        answers: {},
        terminalItems: [],
    }

    it('checks an unconfirmed submission with Academy before allowing a retry, never resending it', async () => {
        const user = userEvent.setup()
        const { calls } = academy('/assessment', (procedure) => {
            switch (procedure) {
                case 'exam.getActive':
                    return active
                case 'exam.answer':
                    return { saved: true }
                case 'exam.submit':
                    return lostReply()
                case 'exam.submissionState':
                    return { attemptId: ATTEMPT, status: 'submitted' }
                case 'exam.result':
                    return { status: 'grading', gradingCount: 1 }
                default:
                    return undefined
            }
        })
        await user.click(await screen.findByRole('button', { name: /Before the model/ }))
        await user.click(screen.getByRole('button', { name: /See results/ }))
        expect(await screen.findByText(/We could not confirm submission/)).toBeInTheDocument()
        await new Promise((r) => setTimeout(r, 50))
        expect(calls.filter((c) => c.procedure === 'exam.submit')).toHaveLength(1)

        await user.click(screen.getByRole('button', { name: 'Check submission status' }))
        // Academy recorded it: the result comes from Academy, not a resend.
        expect(await screen.findByRole('heading', { level: 1, name: 'Grading your written answers' })).toBeInTheDocument()
        expect(calls.filter((c) => c.procedure === 'exam.submit')).toHaveLength(1)
        expect(calls.filter((c) => c.procedure === 'exam.submissionState')).toHaveLength(1)
        // Only learner answers travel: no clock, verdict or award fields.
        for (const call of calls.filter((c) => c.procedure.startsWith('exam.') && c.input)) {
            expect(call.input).not.toMatch(/deadline|score|passed|award|correct|startedAt/)
        }
    })

    it('keeps the attempt open for a retry only when Academy says it is still in progress', async () => {
        const user = userEvent.setup()
        let submits = 0
        const { calls } = academy('/assessment', (procedure) => {
            if (procedure === 'exam.getActive') return active
            if (procedure === 'exam.answer') return { saved: true }
            if (procedure === 'exam.submissionState') return { attemptId: ATTEMPT, status: 'in-progress' }
            if (procedure === 'exam.submit') {
                submits += 1
                return submits === 1
                    ? lostReply()
                    : {
                          status: 'graded',
                          attemptId: ATTEMPT,
                          scorePercent: 40,
                          passed: false,
                          passThreshold: 70,
                          certTitle: 'Certified AI Application Security Professional',
                          perCourse: [],
                          results: [{ questionId: 'q1', correct: false }],
                          award: null,
                      }
            }
            return undefined
        })
        await user.click(await screen.findByRole('button', { name: /Before the model/ }))
        await user.click(screen.getByRole('button', { name: /See results/ }))
        await user.click(await screen.findByRole('button', { name: 'Check submission status' }))
        await user.click(await screen.findByRole('button', { name: 'Retry submission' }))
        await waitFor(() => expect(calls.filter((c) => c.procedure === 'exam.submit')).toHaveLength(2))
        // Academy's grade is what the learner sees.
        expect(await screen.findByTestId('exam-score')).toHaveTextContent('40%')
        // The retry is the learner's, for the same attempt.
        const attempts = calls.filter((c) => c.procedure === 'exam.submit').map((c) => JSON.parse(c.input ?? '{}').json.attemptId)
        expect(attempts).toEqual([ATTEMPT, ATTEMPT])
    })
})

describe('assessment timer', () => {
    const ATTEMPT = '00000000-0000-4000-8000-0000000000e2'
    const session = (deadlineInMs: number) => ({
        attemptId: ATTEMPT,
        // Academy's deadline; the desktop only counts down to it.
        deadline: new Date(Date.now() + deadlineInMs).toISOString(),
        questions: [{ id: 'q1', moduleId: 'm', objectiveId: 'o', type: 'mcq', prompt: 'Where does AIRS evaluate a request?', options: ['Before the model', 'After shipping'] }],
        answers: {},
        terminalItems: [],
    })
    const graded = { status: 'graded', attemptId: ATTEMPT, scorePercent: 0, passed: false, passThreshold: 70, certTitle: 'Cert', perCourse: [], results: [{ questionId: 'q1', correct: false }], award: null }

    it("shows Academy's countdown in view and submits once at the deadline", async () => {
        const active = session(125_000)
        academy('/assessment', { 'exam.getActive': active })
        const timer = await screen.findByRole('timer', { name: 'Time remaining' })
        expect(timer.textContent).toMatch(/^02:0[0-5]$/)
        expect(timer.closest('[data-slot="academy-status"]')).not.toBeNull()
        expect(screen.getByText('Proctored')).toBeInTheDocument()
    })

    it('submits exactly once when the deadline passes and shows Academy’s grade', async () => {
        const { calls } = academy('/assessment', { 'exam.getActive': session(1_200), 'exam.submit': graded })
        await screen.findByRole('timer', { name: 'Time remaining' })
        expect(await screen.findByTestId('exam-score', {}, { timeout: 4000 })).toHaveTextContent('0%')
        await new Promise((r) => setTimeout(r, 1200))
        expect(calls.filter((c) => c.procedure === 'exam.submit')).toHaveLength(1)
        expect(screen.queryByRole('timer')).toBeNull()
    })

    it('reports an unknown outcome at the deadline once and never resends it', async () => {
        const { calls } = academy('/assessment', {
            'exam.getActive': session(1_000),
            'exam.submit': lostReply(),
            'exam.submissionState': { attemptId: ATTEMPT, status: 'submitted' },
        })
        expect(await screen.findByText(/We could not confirm submission/, {}, { timeout: 4000 })).toBeInTheDocument()
        await new Promise((r) => setTimeout(r, 2200))
        expect(calls.filter((c) => c.procedure === 'exam.submit')).toHaveLength(1)
    })

    it('stops with the attempt: leaving it or another learner generation submits nothing', async () => {
        const user = userEvent.setup()
        const native = scriptedNative({ 'exam.getActive': session(1_500), 'exam.submit': graded })
        const view = render(<AcademyView snapshot={academySnapshot(true)} client={native.client} onNavigate={vi.fn()} initialPath='/assessment' />)
        await screen.findByRole('timer', { name: 'Time remaining' })
        await user.click(within(screen.getByRole('navigation', { name: 'Academy sections' })).getByRole('link', { name: 'Program' }))
        await waitFor(() => expect(screen.queryByRole('timer')).toBeNull())
        await new Promise((r) => setTimeout(r, 2000))
        expect(native.calls.filter((c) => c.procedure === 'exam.submit')).toHaveLength(0)
        view.unmount()

        const second = scriptedNative({ 'exam.getActive': session(1_500), 'exam.submit': graded })
        const again = render(<AcademyView snapshot={academySnapshot(true)} client={second.client} onNavigate={vi.fn()} initialPath='/assessment' />)
        await screen.findByRole('timer', { name: 'Time remaining' })
        again.rerender(<AcademyView snapshot={academySnapshot(true, 'lx-cccccccccccccccccccccccc')} client={second.client} onNavigate={vi.fn()} initialPath='/assessment' />)
        await new Promise((r) => setTimeout(r, 2000))
        expect(second.calls.filter((c) => c.procedure === 'exam.submit' && c.learner === ACCOUNT)).toHaveLength(0)
    })
})

describe('lesson feedback', () => {
    const lesson = (status: 'done' | 'not_started') => ({
        id: 'm-rt-1-lesson-01',
        moduleId: 'm-rt-1',
        order: 1,
        slug: '1',
        title: 'Scanning at runtime: part 1',
        xp: 100,
        estMinutes: 5,
        status,
        kind: 'content',
        bodyHtml: '<p>Lesson body.</p>',
        objectives: [],
        inlineChecks: [],
        sourceLessonId: 'lesson-scan-1',
        siblings: [],
        pathContext: null,
    })

    it('asks for a rating on a completed lesson and sends it once', async () => {
        const user = userEvent.setup()
        const { calls } = academy('/units/m-rt-1-lesson-01', {
            'units.get': lesson('done'),
            'admin.feedback.submit': { id: 'fb-1' },
        })
        await screen.findByTestId('feedback-widget')
        await user.click(within(screen.getByRole('group', { name: 'Rating' })).getAllByRole('button')[3]!)
        await user.click(screen.getByRole('button', { name: /Submit/ }))
        expect(await screen.findByTestId('feedback-thanks')).toBeInTheDocument()
        const sent = calls.filter((c) => c.procedure === 'admin.feedback.submit')
        expect(sent).toHaveLength(1)
        expect(JSON.parse(sent[0]!.input ?? '{}').json).toEqual({ moduleId: 'm-rt-1', lessonId: 'lesson-scan-1', rating: 4 })
    })

    it('does not ask before the lesson is complete', async () => {
        academy('/units/m-rt-1-lesson-01', { 'units.get': lesson('not_started') })
        expect(await screen.findByRole('heading', { level: 1, name: 'Scanning at runtime: part 1' })).toBeInTheDocument()
        expect(screen.queryByTestId('feedback-widget')).toBeNull()
    })
})

describe('shift', () => {
    const SHIFT = '00000000-0000-4000-8000-0000000000d1'
    const run = (stamped: boolean) => ({
        shiftId: SHIFT,
        stage: 1,
        stagesCleared: 0,
        alertsAnswered: 1,
        integrityLeft: 3,
        chain: 2,
        xpAwarded: 12,
        alert: { id: 'q2', type: 'mcq', prompt: 'A scan flags a prompt injection. What do you do first?', options: ['Block the request', 'Ignore it'] },
        alertOrder: 2,
        atBoundary: false,
        countermeasures: [],
        draft: [],
        rating: 100,
        // Academy's clock: the desktop renders this, it never computes one.
        serverNow: '2026-10-17T12:00:00.000Z',
        deadlineAt: stamped ? '2026-10-17T12:01:00.000Z' : null,
        slaTotalSeconds: stamped ? 60 : null,
        ruledOutOption: null,
        actor: null,
        suspectOption: null,
        optionRevealDelaySeconds: 0,
        alertsPerStage: 5,
        isBossProbe: false,
    })

    it('serves and answers a timed alert while the optional briefing has not arrived', async () => {
        const user = userEvent.setup()
        let served = false
        const { calls } = academy('/shift', (procedure) => {
            switch (procedure) {
                case 'orientation.get':
                    return {
                        completedAt: '2026-10-01T00:00:00.000Z',
                        skippedAt: null,
                        hintsSeen: { 'watch-floor-explainer': '2026-10-01', 'watch-floor-unlocked': '2026-10-01' },
                    }
                case 'shift.getActive':
                    return run(served)
                case 'shift.serve':
                    served = true
                    return { breachResolved: false, settled: null }
                case 'shift.briefing':
                case 'shift.daily':
                    return held()
                case 'shift.answer':
                    return held()
                case 'shift.eligibility':
                    return { unlocked: true, poolCount: 80, minQuestions: 50 }
                default:
                    return undefined
            }
        })
        expect(await screen.findByText('A scan flags a prompt injection. What do you do first?')).toBeInTheDocument()
        // The unstamped alert is served at once although the briefing is pending.
        await waitFor(() => expect(calls.filter((c) => c.procedure === 'shift.serve')).toHaveLength(1))
        await user.click(await screen.findByRole('button', { name: 'Block the request' }))
        await waitFor(() => expect(screen.getByRole('button', { name: 'Submit' })).toBeEnabled())
        await user.click(screen.getByRole('button', { name: 'Submit' }))
        await waitFor(() => expect(calls.filter((c) => c.procedure === 'shift.answer')).toHaveLength(1))
        const answer = JSON.parse(calls.find((c) => c.procedure === 'shift.answer')!.input ?? '{}').json
        // Only the learner's choice travels: no clock, grade or award.
        expect(answer).toEqual({ shiftId: SHIFT, order: 2, values: ['0'] })
        expect(calls.some((c) => c.procedure === 'shift.briefing')).toBe(true)
    })
})

describe('course removal', () => {
    const COURSE = {
        id: 'runtime-security',
        slug: 'runtime-security',
        title: 'Runtime Security Foundations',
        description: 'Scan prompts and responses at runtime.',
        iconKey: null,
        estMinutes: 60,
        status: 'published',
    }
    const replies = {
        'paths.list': [],
        'courses.list': [COURSE],
        'paths.forCourse': [],
        'progress.courseSummary': { hasProgress: true, units: 3, quizzes: 1, labs: 0, objectives: 2 },
        'units.listByPath': [
            {
                moduleId: 'm-rt-1',
                title: 'Scanning at runtime',
                description: 'Scanning at runtime.',
                concepts: [],
                estMinutes: 30,
                order: 1,
                unitsDone: 3,
                unitCount: 6,
                percent: 50,
                xpEarned: 300,
                xpTotal: 600,
                complete: false,
                reuseCount: 1,
                resumeUnitId: 'm-rt-1-lesson-04',
            },
        ],
        'modules.intro': null,
        'enrollment.affectedByCourseRemoval': {
            certifications: [
                {
                    certificationId: 'ai-app-security',
                    title: 'AI Application Security',
                    courses: [
                        { courseId: 'runtime-security', title: 'Runtime Security Foundations' },
                        { courseId: 'model-security', title: 'Model Security' },
                    ],
                },
            ],
        },
        'enrollment.removeByCourse': { removedCertifications: ['ai-app-security'] },
    }

    it('names every affected certification and course and keeps earned certificates before removing', async () => {
        const user = userEvent.setup()
        const { calls } = academy('/courses/runtime-security', replies)
        await user.click(await screen.findByRole('button', { name: 'Remove enrollment' }))
        const dialog = await screen.findByRole('dialog', { name: 'Remove this course?' })
        expect(dialog).toHaveTextContent('AI Application Security')
        expect(dialog).toHaveTextContent('Runtime Security Foundations, Model Security')
        expect(dialog).toHaveTextContent('Certificates you’ve already earned stay valid.')
        expect(calls.some((c) => c.procedure === 'enrollment.removeByCourse')).toBe(false)
        await user.click(within(dialog).getByRole('button', { name: 'Remove enrollment' }))
        await waitFor(() => expect(calls.filter((c) => c.procedure === 'enrollment.removeByCourse')).toHaveLength(1))
        expect(JSON.parse(calls.find((c) => c.procedure === 'enrollment.removeByCourse')!.input ?? '{}').json).toEqual({
            courseId: 'runtime-security',
        })
        expect(calls.some((c) => c.procedure === 'progress.resetCourse')).toBe(false)
    })

    it('resets course progress only after a confirmation with the counts and certificates kept', async () => {
        const user = userEvent.setup()
        const { calls } = academy('/courses/runtime-security', {
            ...replies,
            'progress.resetCourse': { reset: true },
        })
        await user.click(await screen.findByRole('button', { name: 'Reset course progress' }))
        const dialog = await screen.findByRole('dialog', { name: 'Reset your progress?' })
        expect(dialog).toHaveTextContent('3 units, 1 quizzes, 0 labs')
        expect(dialog).toHaveTextContent('Certificates you’ve already earned stay valid.')
        await user.click(within(dialog).getByRole('button', { name: 'Cancel' }))
        expect(calls.some((c) => c.procedure === 'progress.resetCourse')).toBe(false)
        await user.click(screen.getByRole('button', { name: 'Reset course progress' }))
        const again = await screen.findByRole('dialog', { name: 'Reset your progress?' })
        await user.click(within(again).getByRole('button', { name: 'Reset course progress' }))
        await waitFor(() => expect(calls.filter((c) => c.procedure === 'progress.resetCourse')).toHaveLength(1))
        expect(JSON.parse(calls.find((c) => c.procedure === 'progress.resetCourse')!.input ?? '{}').json).toEqual({
            courseId: 'runtime-security',
        })
    })

    it('offers no removal when Academy reports no affected certification', async () => {
        const { calls } = academy('/courses/runtime-security', {
            ...replies,
            'enrollment.affectedByCourseRemoval': { certifications: [] },
        })
        expect(await screen.findByRole('heading', { level: 1, name: 'Runtime Security Foundations' })).toBeInTheDocument()
        await waitFor(() => expect(calls.some((c) => c.procedure === 'enrollment.affectedByCourseRemoval')).toBe(true))
        expect(screen.queryByRole('button', { name: 'Remove enrollment' })).toBeNull()
    })
})
