import type {
    FieldProblem,
    SaveRefusal,
    WorkbenchProblem,
    WorkbenchRequest,
    WorkbenchResult,
} from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'

/** Waits for the single result of an admitted API Client request. */
export type ObserveWorkbench = (operation: string) => Promise<WorkbenchResult>

/** One API Client request: its result, or why it never ran. */
export type WorkbenchCall = (request: WorkbenchRequest) => Promise<WorkbenchResult | { kind: 'undelivered'; message: string }>

export function workbenchCall(dispatch: Dispatch, observe: ObserveWorkbench): WorkbenchCall {
    return async (request) => {
        const outcome = await dispatch({ kind: 'workbench', request })
        if (outcome.kind !== 'admitted' || outcome.operation === null) {
            return { kind: 'undelivered', message: outcomeMessage(outcome) ?? 'The request could not be delivered.' }
        }
        try {
            return await observe(outcome.operation)
        } catch {
            return { kind: 'undelivered', message: 'The desktop did not report how the request ended. Check the request before trying again.' }
        }
    }
}

const FIELD_TEXT: Record<FieldProblem['field'], string> = {
    method: 'Method: use letters and hyphens only',
    url: 'URL: enter one line',
    headers: 'Headers: use Name: value',
    query: 'Params: use name=value',
    body: 'Body: this body cannot be written as text here',
    bodyKind: 'Body kind',
    settings: 'Auth and variables: only [auth] with references and [variables] are written',
}

export function fieldText(problem: FieldProblem): string {
    return problem.line === null ? FIELD_TEXT[problem.field] : `${FIELD_TEXT[problem.field]} (line ${problem.line})`
}

const REFUSAL_TEXT: Record<Exclude<SaveRefusal['kind'], 'invalid'>, string> = {
    conflict: 'Changed on disk since you opened it. Choose Use disk or Keep mine; both versions are kept privately first.',
    incomplete: 'The collection changed or has problems to fix before saving.',
    moved: 'This request was moved to another file. Reopen the collection.',
    sharedBody: 'Another request now uses this body file; it was not changed.',
    interrupted: 'An earlier save of this request was interrupted. Choose Use disk or Keep mine before saving.',
    exists: 'A file already exists at this path; nothing was replaced.',
    io: 'The files could not be written. Your edits are kept on this Mac.',
}

export function saveRefusalText(refusal: SaveRefusal): string {
    return refusal.kind === 'invalid' ? `Fix to save. ${fieldText(refusal.problem)}.` : REFUSAL_TEXT[refusal.kind]
}

const DETAIL_TEXT: Record<string, string> = {
    notCollection: 'That folder holds no API Client collection (collection.toml, format 1).',
    badPath: 'Use a path inside the collection: folder/name, no leading dot, slash or "..".',
    exists: 'Something already exists there; nothing was replaced.',
    notInRepository: 'Create a collection inside an existing Git repository.',
    incomplete: 'Fix the collection problems listed first.',
    missing: 'That file is missing.',
    tooLarge: 'That file is larger than the limit.',
    io: 'The file could not be read or written.',
    locked: 'The Keychain is locked. Unlock it and try again.',
    denied: 'macOS denied Keychain access.',
    unavailable: 'Private API Client storage is unavailable.',
    unreadable: 'Saved private API Client data did not open; it was kept aside.',
    notText: 'Schemas are JSON or YAML text up to 4 MiB.',
    conflict: 'Changed on disk since it was opened; reopen it.',
    version: 'Only Insomnia v5 exports (schema 5.1) are supported.',
    structure: 'The export has fields this import does not take.',
    limit: 'The export has more resources or nesting than the import takes.',
}

const CURL_TEXT: Record<string, string> = {
    tooLarge: 'The command is larger than 1 MiB.',
    notCurl: 'Paste one command that begins with curl.',
    shell: 'Shell expansion, operators and comments are not supported; quote literal values.',
    unclosed: 'A quote or escape is not closed.',
    tooManyWords: 'The command has too many arguments.',
    unsupportedOption: 'The command uses an option this import does not support.',
    missingArgument: 'An option is missing its value.',
    url: 'Give exactly one absolute http(s) URL.',
    urlCredentials: 'Move credentials out of the URL before importing.',
    queryCredentials: 'Move credential query parameters to a secret before importing.',
    glob: 'URL glob expressions are not supported.',
    template: 'Template syntax is not imported; add variables after conversion.',
    bodyCredentials: 'The body has credential-named fields; remove them before importing.',
    json: '--json data must be valid JSON.',
    header: 'Headers must be single-line Name: value with a value.',
    external: 'Files, stdin and interactive prompts are not supported.',
    combination: 'These options cannot be combined.',
}

const PROBLEM_TEXT: Record<string, string> = {
    noCollection: 'Open a collection first.',
    missing: 'That request is not open any more.',
    stale: 'This request changed since it was loaded. It was reloaded; check it and try again.',
    unavailable: 'The API Client is not available in this build.',
    invalid: 'Check the entered values and try again.',
    managed: 'tenant.* references belong to the tenant and are not stored here.',
    limit: 'A limit was reached (drafts or private environments).',
    keychain: 'The Keychain did not store the value.',
    unprotected: 'Your latest edits are not kept on this Mac yet.',
}

export function problemText(problem: WorkbenchProblem): string {
    if (problem.kind === 'curl') return CURL_TEXT[problem.detail] ?? 'The command could not be imported.'
    if (problem.kind === 'adoption') {
        return problem.detail === 'missing' ? 'No v1 API Client profile was found on this Mac.' : 'The v1 profile could not be read.'
    }
    if ('detail' in problem) return DETAIL_TEXT[problem.detail] ?? 'That could not be done.'
    return PROBLEM_TEXT[problem.kind] ?? 'That could not be done.'
}

/** Operator text for a result that did not do what was asked (else null). */
export function resultText(result: Awaited<ReturnType<WorkbenchCall>>): string | null {
    switch (result.kind) {
        case 'undelivered':
            return result.message
        case 'refused':
            return problemText(result.problem)
        case 'saveRefused':
            return saveRefusalText(result.refusal)
        default:
            return null
    }
}
