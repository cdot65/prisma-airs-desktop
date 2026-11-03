/**
 * Fixture builds only (`snapshot.fixtureBuild`, a Rust compile-time fact):
 * measures the C04-registered learner-call-overhead budget through the
 * production learner transport. It issues sequential `courses.list` queries
 * (a public catalog read) through the same desktop tRPC link the learner
 * pages use and exposes each dispatch-to-result time for the measurement
 * driver (tools/perf/measure_c05.py), which subtracts the synthetic
 * endpoint's own handling time from its log.
 */
import { useState } from 'react'
import { useLearnerTrpc } from './learner-provider'

export const PROBE_CALLS = 100

export default function LearnerCallProbe() {
    const trpc = useLearnerTrpc() as unknown as { courses: { list: { query(): Promise<unknown> } } }
    const [samples, setSamples] = useState<number[] | null>(null)
    const [running, setRunning] = useState(false)
    const run = async () => {
        setRunning(true)
        const taken: number[] = []
        for (let i = 0; i < PROBE_CALLS; i += 1) {
            const start = performance.now()
            await trpc.courses.list.query()
            taken.push(Math.round((performance.now() - start) * 1000) / 1000)
        }
        setSamples(taken)
        setRunning(false)
    }
    return (
        <section aria-label='Fixture learner call probe' data-slot='learner-call-probe' className='mt-8 text-[11px]'>
            <button type='button' disabled={running} onClick={() => void run()} className='underline'>
                Run learner call probe
            </button>
            <output aria-label='Learner call probe samples'>{samples ? JSON.stringify(samples) : ''}</output>
        </section>
    )
}
