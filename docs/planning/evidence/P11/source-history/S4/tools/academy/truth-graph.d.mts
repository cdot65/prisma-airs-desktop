/** Types for truth-graph.mjs (called by the desktop's C05 truth check). */
export interface TruthGraphReport {
    schema: string
    academyCommit: string
    moduleCount: number
    academyModuleCount: number
    academyAreas: string[]
    packages: string[]
    truthModules: string[]
    forbiddenPackages: string[]
    nodeBuiltins: string[]
    canaryHits: { file: string; canary: string }[]
    controlFound: boolean
    procedures: { procedure: string; kind: 'query' | 'mutation'; access: string; reason?: string; from: string[] }[]
    unexplainedRefusals: string[]
    emittedFiles: string[]
    passed: boolean
}

export declare function truthGraph(): Promise<{ report: TruthGraphReport; academyModules: string[] }>
