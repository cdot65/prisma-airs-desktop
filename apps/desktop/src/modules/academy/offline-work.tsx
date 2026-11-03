/**
 * Offline Academy work for the signed-in account: the Downloads page (keep a
 * course on this Mac, update or remove it), the notice shown while Academy
 * cannot be reached. What is kept, its status and reachability come from
 * the core's snapshot; downloaded course content stays sealed in the core
 * and reaches pages only as the learner calls it answers offline.
 */
import { createContext, useContext, useState, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Button, Callout } from '@ai-security-academy/presentation'
import { CloudOff, Info } from 'lucide-react'
import type { AcademyCallResult, DesktopClient, DownloadView, LearnerRef, ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { offlineCall, type OfflineIntent } from './host-actions'
import { useLearnerTrpc } from './learner-provider'

interface OfflineWork {
    client: DesktopClient
    learner: LearnerRef
    offline: ShellSnapshot['academyOffline']
}

const OfflineContext = createContext<OfflineWork | null>(null)

export function OfflineWorkProvider({ value, children }: { value: OfflineWork; children: ReactNode }) {
    return <OfflineContext.Provider value={value}>{children}</OfflineContext.Provider>
}

/** Shown on every Academy page while the last learner call found Academy unreachable. */
export function OfflineNotice() {
    const work = useContext(OfflineContext)
    if (!work || work.offline.reachable || work.offline.state === 'signedOut') return null
    return (
        <div data-slot='academy-offline' className='mb-4'>
            <Callout tone='default' icon={CloudOff}>
                <p>
                    Academy can&apos;t be reached. Downloaded courses and lab work saved on this Mac still open.
                    Nothing is sent until you choose to send it again.
                </p>
            </Callout>
        </div>
    )
}

const STATUS: Record<DownloadView['status'], string> = {
    current: 'Ready offline',
    outdated: 'A newer version is on Academy',
    expired: 'Offline access ended. Connect to renew it.',
    withdrawn: 'No longer part of your account',
}

const day = (instant: string) =>
    new Date(instant).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })

interface CourseRow {
    id: string
    title: string
}

/** While Academy cannot be reached, the course list is asked again now and then (a reply is what tells the core Academy is back). */
const RECHECK_MS = 15_000

function useCatalog(reachable: boolean) {
    const trpc = useLearnerTrpc() as unknown as { courses: { list: { query(): Promise<unknown> } } }
    return useQuery({
        queryKey: ['desktop', 'offline', 'courses'],
        refetchInterval: reachable ? false : RECHECK_MS,
        retry: reachable ? 1 : false,
        queryFn: async (): Promise<CourseRow[]> => {
            const rows = await trpc.courses.list.query()
            if (!Array.isArray(rows)) return []
            return rows.flatMap((row) =>
                row && typeof row === 'object' && typeof row.id === 'string' && typeof row.title === 'string'
                    ? [{ id: row.id, title: row.title }]
                    : []
            )
        },
    })
}

const FAILED: Partial<Record<string, string>> = {
    notDownloadable: 'Academy does not offer this course to your account for offline use.',
    offlineFull: 'There is not enough room for this course in offline storage. Remove a download and try again.',
    offlineUnavailable: 'Offline storage could not be opened. Check that Keychain access was allowed.',
    network: 'Academy could not be reached. Try again when you are online.',
    timeout: 'Academy did not answer in time. Try again.',
}

function failureText(result: AcademyCallResult | null): string {
    if (result?.kind === 'failed') return FAILED[result.failure] ?? 'The download did not finish. Nothing was kept.'
    return 'The download did not finish. Nothing was kept.'
}

/** The Downloads page (signed-in accounts only). */
export function DownloadsPage() {
    const work = useContext(OfflineContext)
    const [busy, setBusy] = useState<string | null>(null)
    const [message, setMessage] = useState<string | null>(null)
    const offline = work?.offline
    const catalog = useCatalog(offline?.reachable ?? true)
    if (!work || !offline) return null
    const run = async (courseId: string, intent: OfflineIntent, done: string) => {
        setBusy(courseId)
        setMessage(null)
        const result = await offlineCall(work.client, intent)
        setBusy(null)
        setMessage(result?.kind === 'data' ? done : intent.kind === 'removeDownload' ? 'The download could not be removed.' : failureText(result))
    }
    const download = (course: CourseRow | DownloadView) => {
        const id = 'id' in course ? course.id : course.courseId
        void run(id, { kind: 'downloadCourse', learner: work.learner, courseId: id }, `${course.title} is ready offline.`)
    }
    const remove = (course: DownloadView) =>
        void run(course.courseId, { kind: 'removeDownload', learner: work.learner, courseId: course.courseId }, `${course.title} was removed from this Mac.`)
    const kept = new Set(offline.downloads.map((d) => d.courseId))
    const available = (catalog.data ?? []).filter((c) => !kept.has(c.id))
    const closed = offline.state === 'unavailable'
    return (
        <section aria-labelledby='academy-downloads' className='mx-auto flex max-w-3xl flex-col gap-6 py-6'>
            <h1 id='academy-downloads' className='font-display text-academy-ink text-[28px] font-semibold'>
                Downloads
            </h1>
            <Callout tone='default' icon={Info}>
                <p>
                    Downloaded lessons, knowledge checks and lab instructions open without a connection, sealed on this
                    Mac for your account only. Videos, walkthroughs and live labs still need Academy. Progress and
                    submissions are sent only when you are online and choose to send them.
                </p>
            </Callout>
            {closed ? (
                <p role='alert' className='text-academy-ink text-[14px]'>
                    Offline storage could not be opened. Check that Keychain access was allowed, then sign in again.
                </p>
            ) : null}
            <p role='status' aria-live='polite' className='text-academy-ink-soft text-[13px] empty:hidden'>
                {message}
            </p>
            <section aria-labelledby='academy-downloads-kept' className='flex flex-col gap-3'>
                <h2 id='academy-downloads-kept' className='text-academy-ink text-[18px] font-semibold'>
                    On this Mac
                </h2>
                {offline.downloads.length === 0 ? (
                    <p className='text-academy-ink-soft text-[14px]'>No courses are downloaded yet.</p>
                ) : (
                    <ul className='flex flex-col gap-2'>
                        {offline.downloads.map((d) => (
                            <li key={d.courseId} className='border-academy-line rounded-academy-sm flex flex-wrap items-center gap-3 border p-3'>
                                <div className='flex min-w-0 flex-1 flex-col'>
                                    <span className='text-academy-ink text-[14px] font-medium'>{d.title}</span>
                                    <span className='text-academy-ink-soft text-[13px]'>
                                        {STATUS[d.status]}
                                        {d.status === 'current' || d.status === 'outdated'
                                            ? ` · ${d.units} ${d.units === 1 ? 'unit' : 'units'} · until ${day(d.leaseExpiresAt)}`
                                            : ''}
                                    </span>
                                </div>
                                {d.firstUnit && d.status !== 'withdrawn' ? (
                                    <Link
                                        to='/units/$unitId'
                                        params={{ unitId: d.firstUnit }}
                                        aria-label={`Open ${d.title}`}
                                        className='text-academy-ink inline-flex min-h-11 items-center px-2 text-[13px] underline'
                                    >
                                        Open
                                    </Link>
                                ) : null}
                                {d.status !== 'withdrawn' && offline.reachable && d.status !== 'current' ? (
                                    <Button type='button' tier='secondary' disabled={busy !== null} onClick={() => download(d)} aria-label={`Update ${d.title}`}>
                                        Update
                                    </Button>
                                ) : null}
                                <Button type='button' tier='secondary' disabled={busy !== null} onClick={() => remove(d)} aria-label={`Remove ${d.title}`}>
                                    Remove
                                </Button>
                            </li>
                        ))}
                    </ul>
                )}
                {offline.localDrafts.length > 0 ? (
                    <p className='text-academy-ink-soft text-[13px]' data-slot='academy-local-drafts'>
                        {offline.localDrafts.length === 1
                            ? 'One lab draft is saved on this Mac only. Open its lab to send it.'
                            : `${offline.localDrafts.length} lab drafts are saved on this Mac only. Open each lab to send it.`}
                    </p>
                ) : null}
            </section>
            <section aria-labelledby='academy-downloads-available' className='flex flex-col gap-3'>
                <h2 id='academy-downloads-available' className='text-academy-ink text-[18px] font-semibold'>
                    Available to download
                </h2>
                {!offline.reachable ? (
                    <p className='text-academy-ink-soft text-[14px]'>Connect to Academy to download more courses.</p>
                ) : catalog.isPending ? (
                    <p className='text-academy-ink-soft text-[14px]'>Loading courses…</p>
                ) : catalog.isError ? (
                    <p className='text-academy-ink-soft text-[14px]'>The course list could not be loaded.</p>
                ) : available.length === 0 ? (
                    <p className='text-academy-ink-soft text-[14px]'>Every course is already on this Mac.</p>
                ) : (
                    <ul className='flex flex-col gap-2'>
                        {available.map((c) => (
                            <li key={c.id} className='border-academy-line rounded-academy-sm flex items-center gap-3 border p-3'>
                                <span className='text-academy-ink min-w-0 flex-1 text-[14px]'>{c.title}</span>
                                <Button
                                    type='button'
                                    tier='primary'
                                    disabled={busy !== null || closed}
                                    onClick={() => download(c)}
                                    aria-label={`Download ${c.title}`}
                                >
                                    {busy === c.id ? 'Downloading…' : 'Download'}
                                </Button>
                            </li>
                        ))}
                    </ul>
                )}
            </section>
        </section>
    )
}
