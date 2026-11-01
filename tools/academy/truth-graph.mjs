#!/usr/bin/env node
/**
 * C05 artifact/import-graph and seeded truth-canary check for the desktop
 * frontend build.
 *
 *   node tools/academy/truth-graph.mjs --out <dir>
 *
 * Builds apps/desktop with its production Vite config into a temporary
 * directory (removed afterwards),
 * with one added plugin that (a) records every module in the graph and
 * (b) seeds a non-removable canary statement into each Academy truth or
 * privileged module (server, database, authored content and question
 * banks, content pipeline, scripts, admin/manager features, Redis, lab
 * evaluation) as it is transformed. The check fails if any truth module is
 * in the graph, any forbidden package is bundled, or a canary appears in
 * the emitted assets. A positive control seeds the same kind of canary into
 * the learner host (always shipped) and requires it to be found, proving the
 * detector sees shipped code.
 *
 * It also lists every tRPC procedure the shipped Academy modules call and
 * classifies it against the pinned desktop sets
 * (packages/desktop-contracts/fixtures/academy/learner-procedures.json):
 * account allowlist, public catalog, or refused by design (with the reason
 * the presentation stays truthful). A refused procedure missing from that
 * list fails the check. The CLI writes <dir>/truth-graph.json and
 * <dir>/academy-modules.txt; tests call truthGraph().
 */
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const app = join(repo, 'apps/desktop')
function realpathOr(path) {
    try {
        return realpathSync(path)
    } catch {
        return path
    }
}
// Its real path: Vite reports module ids with symbolic links resolved, so a
// checkout that links its .work (a worktree) would otherwise match no module.
const academy = realpathOr(join(repo, '.work/academy'))

/** Academy-relative prefixes that must never ship in the desktop build. */
const TRUTH = [
    'src/server/',
    'src/db/',
    'src/content/',
    'src/content-pipeline/',
    'scripts/',
    'src/features/admin/',
    'src/lib/redis.ts',
    'src/routes/_authenticated/admin',
    'packages/shared/src/server',
]
/** Lab evaluation and grading engines (server-side truth). */
const TRUTH_NAMES = [
    /(^|\/)evaluat(e|or|ion)s?(\/|\.tsx?$)/i,
    /(^|\/)grader/i,
    /(^|\/)grading(\/|\.tsx?$)/i,
    /answer-?keys?/i,
    /(^|\/)goldens?(\/|\.(ts|json)$)/i,
    /(^|\/)rubrics?(\/|\.(ts|json)$)/i,
]
/** Packages a learner client never needs (server, database, providers). */
const FORBIDDEN_PACKAGES = [
    'drizzle-orm',
    'postgres',
    'pg',
    'ioredis',
    '@upstash/redis',
    '@clerk/',
    '@sentry/',
    'openai',
    '@anthropic-ai/',
    'stripe',
    'resend',
]

/**
 * Procedures shipped modules call that the desktop transport refuses, and
 * why the learner presentation stays truthful without them.
 */
const UNMOUNTED =
    'reached only through the gamification barrel; no shipped learner page mounts the component, so the call is never made'
const PRESENTERS =
    "Home's away-commendation and event-celebration presenters claim on mount only with LearnerHost awardPresenters; the desktop passes false, so the call is never made (Academy web announces them and the dossier lists every commendation)"
const REFUSED_BY_DESIGN = {
    'labs.': 'the web lab runtime; the desktop runs hands-on labs through desktop.attempt/desktop.draft (LearnerHost labs="desktop"), so no web lab runner mounts and no labs.* start, step or submit call is made',
    'gamification.acknowledgeCommendations': PRESENTERS,
    'gamification.claimCommendations': PRESENTERS,
    'gamification.acknowledgeSko27Celebration': PRESENTERS,
    'gamification.claimSko27Celebration': PRESENTERS,
    'gamification.sko27LoginAward': UNMOUNTED,
    'gamification.firstRotation':
        "Home's first-rotation checklist is not in the desktop token allowlist; the card renders only when Academy returns it, so the refusal leaves it hidden",
    'paths.recommended':
        "Home's recommended paths are not in the desktop token allowlist; the refusal leaves the briefing without recommendations (the catalog stays reachable from Learn)",
    'identityPractical.': 'assessor authority stays on Academy web: the identity practical runs with LearnerHost assessorTools=false, so the assessor queue and review are never fetched or offered',
    'telemetry.track': 'view, click and award beacons fire it and forget it with meta.silent; the desktop sends no Academy product telemetry and the refusal shows nothing',
}

const CANARY = 'AIRS-C05-TRUTH-CANARY'
const CONTROL = 'AIRS-C05-SHIPPED-CONTROL'

/** Academy-relative path of a module: its prepared source (`@/` alias) or
 * an installed copy of an Academy package (`file:` dependency). */
const academyRelative = (id) => {
    const clean = id.split('?')[0]
    const installed = clean.match(/node_modules\/@ai-security-academy\/([^/]+)\/(.*)$/)
    if (installed) return `packages/${installed[1]}/${installed[2]}`
    const rel = relative(academy, clean)
    return rel.startsWith('..') ? null : rel
}
const isTruth = (rel) =>
    TRUTH.some((p) => rel.startsWith(p)) ||
    (rel.startsWith('src/') && !rel.includes('__tests__') && TRUTH_NAMES.some((re) => re.test(rel)))
const packageOf = (id) => {
    const m = id.split('?')[0].match(/node_modules\/((?:@[^/]+\/)?[^/]+)/g)
    return m ? m.at(-1).replace('node_modules/', '') : null
}

/**
 * Builds the desktop app with the graph plugin into a temporary directory
 * (removed afterwards) and returns the report and the shipped Academy module
 * list.
 */
export async function truthGraph() {
    const buildDir = mkdtempSync(join(tmpdir(), 'airs-c05-truth-graph-'))
    const dist = join(buildDir, 'dist')
    try {
        const modules = new Set()
        const seeded = []
        const graphPlugin = {
            name: 'airs-c05-truth-graph',
            enforce: 'post',
            transform(code, id) {
                const rel = academyRelative(id)
                if (!rel || !/\.(m?[jt]sx?)$/.test(id.split('?')[0])) return null
                if (isTruth(rel)) {
                    seeded.push(rel)
                    return { code: `${code}\n;globalThis.__airsCanary=${JSON.stringify(`${CANARY}:${rel}`)};\n`, map: null }
                }
                if (rel === 'packages/learner/src/host.tsx') {
                    return { code: `${code}\n;globalThis.__airsControl=${JSON.stringify(CONTROL)};\n`, map: null }
                }
                return null
            },
            generateBundle() {
                for (const id of this.getModuleIds()) modules.add(id)
            },
        }

        const require = createRequire(join(app, 'package.json'))
        const { build } = await import(require.resolve('vite'))
        await build({
            root: app,
            configFile: join(app, 'vite.config.ts'),
            logLevel: 'warn',
            plugins: [graphPlugin],
            build: { outDir: dist, emptyOutDir: true, sourcemap: false },
        })

        function files(dir) {
            return readdirSync(dir).flatMap((name) => {
                const path = join(dir, name)
                return statSync(path).isDirectory() ? files(path) : [path]
            })
        }
        const emitted = files(dist)
        const canaryHits = []
        let controlFound = false
        for (const file of emitted) {
            const text = readFileSync(file, 'latin1')
            if (text.includes(CONTROL)) controlFound = true
            let at = text.indexOf(CANARY)
            while (at >= 0) {
                canaryHits.push({ file: relative(dist, file), canary: text.slice(at, text.indexOf('"', at)) })
                at = text.indexOf(CANARY, at + 1)
            }
        }

        const ids = [...modules].map((id) => id.split('?')[0])
        const academyModules = ids.map(academyRelative).filter(Boolean).sort()
        const truthModules = academyModules.filter(isTruth)
        const packages = [...new Set(ids.map(packageOf).filter(Boolean))].sort()
        const forbiddenPackages = packages.filter((p) => FORBIDDEN_PACKAGES.some((f) => (f.endsWith('/') ? p.startsWith(f) : p === f)))
        const procedureSets = JSON.parse(
            readFileSync(join(repo, 'packages/desktop-contracts/fixtures/academy/learner-procedures.json'), 'utf8')
        )
        const allowed = new Map(procedureSets.learner.map((e) => [e.procedure, e.kind]))
        const publicSet = new Set(procedureSets.public)
        const calls = new Map()
        const sourceOf = (rel) =>
            rel.startsWith('src/') || rel.startsWith('packages/') ? join(academy, rel) : null
        for (const rel of academyModules) {
            const path = sourceOf(rel)
            let text
            try {
                text = path ? readFileSync(path, 'utf8') : ''
            } catch {
                continue
            }
            const patterns = [
                [/trpc\s*\.\s*([a-zA-Z][\w.]*?)\s*\.\s*(useQuery|useSuspenseQuery|useInfiniteQuery|useSuspenseInfiniteQuery|useMutation)\b/g, (how) => (how === 'useMutation' ? 'mutation' : 'query')],
                [/utils\s*\.\s*([a-zA-Z][\w.]*?)\s*\.\s*(fetch|ensureData|prefetch|fetchInfinite)\b/g, () => 'query'],
            ]
            for (const [re, kindOf] of patterns) {
                for (const m of text.matchAll(re)) {
                    const key = `${m[1]} ${kindOf(m[2])}`
                    if (!calls.has(key)) calls.set(key, new Set())
                    calls.get(key).add(rel)
                }
            }
        }
        const procedures = [...calls.entries()]
            .map(([key, from]) => {
                const [procedure, kind] = key.split(' ')
                const reason = Object.entries(REFUSED_BY_DESIGN).find(([p]) => (p.endsWith('.') ? procedure.startsWith(p) : procedure === p))?.[1]
                const access =
                    allowed.get(procedure) === kind
                        ? 'account'
                        : publicSet.has(procedure) && kind === 'query'
                          ? 'public'
                          : reason
                            ? 'refusedByDesign'
                            : 'refusedUnexplained'
                return { procedure, kind, access, ...(access === 'refusedByDesign' ? { reason } : {}), from: [...from].sort() }
            })
            .sort((a, b) => a.procedure.localeCompare(b.procedure) || a.kind.localeCompare(b.kind))
        const unexplained = procedures.filter((p) => p.access === 'refusedUnexplained')
        const nodeBuiltins = ids.filter((id) => id.startsWith('node:') || id.startsWith('\0node:'))
        const report = {
            schema: 'airs-v2/academy-truth-graph/1',
            academyCommit: JSON.parse(readFileSync(join(repo, 'docs/architecture/inputs.lock.json'), 'utf8')).academy.commit,
            moduleCount: ids.length,
            academyModuleCount: academyModules.length,
            academyAreas: [...new Set(academyModules.map((m) => m.split('/').slice(0, m.startsWith('src/features/') ? 3 : 2).join('/')))].sort(),
            packages,
            truthPrefixes: TRUTH,
            truthNamePatterns: TRUTH_NAMES.map(String),
            forbiddenPackageRules: FORBIDDEN_PACKAGES,
            truthModulesSeededButNotShipped: [...new Set(seeded)].filter((m) => !truthModules.includes(m)).length,
            truthModules,
            forbiddenPackages,
            nodeBuiltins,
            canaryHits,
            controlFound,
            procedures,
            unexplainedRefusals: unexplained.map((p) => `${p.procedure} (${p.kind})`),
            emittedFiles: emitted.map((f) => relative(dist, f)).sort(),
        }
        report.passed =
            truthModules.length === 0 &&
            forbiddenPackages.length === 0 &&
            nodeBuiltins.length === 0 &&
            canaryHits.length === 0 &&
            controlFound &&
            unexplained.length === 0

        return { report, academyModules }
    } finally {
        rmSync(buildDir, { recursive: true, force: true })
    }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const outArg = process.argv.indexOf('--out')
    if (outArg < 0 || !process.argv[outArg + 1]) {
        console.error('usage: truth-graph.mjs --out <dir>')
        process.exit(2)
    }
    const out = resolve(process.argv[outArg + 1])
    mkdirSync(out, { recursive: true })
    const { report, academyModules } = await truthGraph()
    writeFileSync(join(out, 'truth-graph.json'), `${JSON.stringify(report, null, 2)}\n`)
    writeFileSync(join(out, 'academy-modules.txt'), `${academyModules.join('\n')}\n`)
    console.log(
        `${report.passed ? 'PASS' : 'FAIL'} modules=${report.moduleCount} academy=${report.academyModuleCount} truth=${report.truthModules.length} forbiddenPackages=${report.forbiddenPackages.length} canaries=${report.canaryHits.length} control=${report.controlFound} procedures=${report.procedures.length} unexplainedRefusals=${report.unexplainedRefusals.length}`
    )
    process.exit(report.passed ? 0 : 1)
}
