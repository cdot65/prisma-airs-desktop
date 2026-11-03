/**
 * The desktop production build ships Academy's learner presentation and
 * nothing of Academy's truth: no server, database, authored content or
 * question banks, content pipeline, scripts, admin features or grading and
 * lab-evaluation modules, no server or provider packages, and no seeded
 * canary from any of them; every procedure the shipped modules call is
 * allowed for the desktop or refused by design (tools/academy/truth-graph.mjs).
 */
import { expect, it } from 'vitest'
import { truthGraph } from '../../../../../tools/academy/truth-graph.mjs'

it('ships no Academy truth, privileged module or unexplained procedure', async () => {
    const { report } = await truthGraph()
    expect(report.controlFound).toBe(true)
    expect(report.truthModules).toEqual([])
    expect(report.forbiddenPackages).toEqual([])
    expect(report.nodeBuiltins).toEqual([])
    expect(report.canaryHits).toEqual([])
    expect(report.unexplainedRefusals).toEqual([])
    expect(report.academyModuleCount).toBeGreaterThan(0)
    // The reading and knowledge-check path is carried for an account.
    const access = new Map(report.procedures.map((p) => [p.procedure, p.access]))
    for (const procedure of ['units.get', 'courses.list', 'units.startKnowledgeCheck', 'units.submitKnowledgeCheck']) {
        expect(access.get(procedure), procedure).toBe('account')
    }
    expect(report.passed).toBe(true)
})
