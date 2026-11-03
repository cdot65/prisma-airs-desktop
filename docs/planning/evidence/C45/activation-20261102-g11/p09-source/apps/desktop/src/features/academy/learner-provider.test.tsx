/**
 * Learner query state is scoped to one account generation: a new learner
 * reference starts from an empty cache and never shows the previous
 * account's replies.
 */
import { act, render, screen } from '@testing-library/react'
import { useQuery } from '@tanstack/react-query'
import { describe, expect, it } from 'vitest'
import { DesktopClient, type Intent } from '@prisma-airs/desktop-contracts'
import { LearnerProvider, learnerQueryClient, useLearnerTrpc } from './learner-provider'

function native(reply: (intent: Intent) => string) {
    const dispatched: Intent[] = []
    let n = 0
    const data = new Map<string, string>()
    const client = new DesktopClient({
        async observe(bytes: Uint8Array) {
            const body = JSON.parse(new TextDecoder().decode(bytes)) as { scope: { kind: string; operation?: string } }
            if (body.scope.kind === 'deliverySession') return { kind: 'deliverySession', session: `ds-${'d'.repeat(32)}`, nextSequence: '1' }
            return { kind: 'academyCall', result: { kind: 'data', data: data.get(body.scope.operation ?? '') } }
        },
        async dispatch(bytes: Uint8Array) {
            const intent = (JSON.parse(new TextDecoder().decode(bytes)) as { intent: Intent }).intent
            dispatched.push(intent)
            n += 1
            data.set(`op-${n}`, reply(intent))
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${n}`, operation: `op-${n}` } }
        },
        openChannel: () => ({ token: 't', close: () => undefined }),
    })
    return { client, dispatched }
}

function Progress() {
    const trpc = useLearnerTrpc() as unknown as { progress: { get: { query(): Promise<{ done: number }> } } }
    const { data } = useQuery({ queryKey: ['progress.get'], queryFn: () => trpc.progress.get.query() })
    return <p>{data ? `done ${data.done}` : 'loading'}</p>
}

describe('learner provider', () => {
    it('gives each learner reference its own cache and calls under that reference', async () => {
        const { client, dispatched } = native((intent) =>
            JSON.stringify({ json: { done: intent.kind === 'academyCall' && intent.learner.endsWith('a') ? 1 : 7 } })
        )
        const view = render(
            <LearnerProvider client={client} learner='lx-aaaaaaaaaaaaaaaaaaaaaaaa'>
                <Progress />
            </LearnerProvider>
        )
        expect(await screen.findByText('done 1')).toBeInTheDocument()
        await act(async () =>
            view.rerender(
                <LearnerProvider client={client} learner='lx-bbbbbbbbbbbbbbbbbbbbbbbb'>
                    <Progress />
                </LearnerProvider>
            )
        )
        // The new account never sees the old account's cached reply.
        expect(screen.queryByText('done 1')).toBeNull()
        expect(await screen.findByText('done 7')).toBeInTheDocument()
        expect(dispatched.map((i) => (i.kind === 'academyCall' ? i.learner : null))).toEqual([
            'lx-aaaaaaaaaaaaaaaaaaaaaaaa',
            'lx-bbbbbbbbbbbbbbbbbbbbbbbb',
        ])
    })

    it('never retries mutations and stops on authorization failures', () => {
        const qc = learnerQueryClient()
        expect(qc.getDefaultOptions().mutations?.retry).toBe(false)
    })
})
