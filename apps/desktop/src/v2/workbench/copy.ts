/**
 * Copy for the v2 API Client, Test Cases and "Show the calls" (C33). Tests
 * assert these exact strings.
 */
import type { CallCredential, CallPurpose, CaseProduct } from '@prisma-airs/desktop-contracts'

export const CASES_COPY = {
    title: 'Test Cases',
    count: (n: number, tenant: string) => `${n} ${n === 1 ? 'case' : 'cases'} in ${tenant}`,
    noTenant: 'Choose a tenant to see its cases.',
    loading: 'Opening the case library…',
    failed: 'The case library could not be opened on this Mac.',
    addPacks: 'Add starter packs',
    search: 'Search cases',
    filter: 'Filter by product',
    all: 'All',
    columns: { name: 'Case', product: 'Product', input: 'Input', result: 'Last result', action: 'Action' },
    notRun: 'Not run',
    openInPlayground: 'Open in Playground',
    inspect: 'Inspect',
    empty: 'No cases yet. Add a starter pack, or save a scan from the Playground as a case.',
    noMatch: 'No case matches.',
    range: (from: number, to: number, of: number) => `${from}–${to} of ${of}`,
    previous: 'Previous page',
    next: 'Next page',
    // Inspect: what the case is and which product runs it; nothing is run from here.
    details: 'Case details',
    closeDetails: 'Close details',
    specification: 'Specification',
    expected: 'Goal',
    lastResult: 'Last result',
    runsFrom: 'Runs from',
    pack: 'Starter pack',
    neverRunHere: 'This screen never runs a case: only a Runtime Security case is run, by you, in the Playground.',
    runs: {
        'runtime-security': 'Runtime Security: open it in the Playground and run the scan there.',
        'ai-gateway': 'AI Gateway: send it through the workspace from AI Gateway.',
        'ai-red-teaming': 'AI Red Teaming: hand the spec to a scan.',
        'model-security': 'Model Security: scan the model it names.',
        'skill-scanning': 'Skill Scanning: scan the skill it names.',
    } as Record<CaseProduct, string>,
    resultAt: (time: string) => `at ${time}`,
    verdictWord: { allow: 'Allowed', block: 'Blocked', alert: 'Flagged', unknown: 'Not determined' } as Record<string, string>,
    playgroundHanded: (name: string) => `${name} is in the Playground. Run the scan there; its result is recorded for the case.`,
    // Authoring and transfer (C12's library workflows, attempt-01 R3).
    newCase: 'New case',
    importCases: 'Import…',
    exportCases: 'Export…',
    addPublic: 'Add public examples',
    remove: 'Remove',
    removeCase: (name: string) => `Remove case ${name}`,
    copyTo: 'Copy cases to',
    copyNowhere: 'No other tenant',
    copy: 'Copy',
    copyCase: (name: string, tenant: string) => `Copy ${name} to ${tenant}`,
    form: {
        title: 'New case',
        product: 'Product',
        name: 'Name',
        profile: 'Security profile',
        app: 'Application',
        prompt: 'Input or test specification',
        response: 'Response (optional)',
        expected: 'Expected behavior (optional)',
        expectedHint: 'A test goal, never a recorded verdict',
        payload: 'Product payload (JSON object, optional)',
        note: 'Saving records this case. It runs nothing: a Runtime Security case is run by you in the Playground.',
        save: 'Save case',
        cancel: 'Cancel',
    },
    // Storage and outcomes (attempt-01 R4).
    failure: {
        locked: 'The Keychain is locked. Unlock it, then open the library again.',
        denied: 'macOS did not allow access to the library key.',
        unavailable: 'The library could not be read or saved on this Mac.',
        unreadable: 'The saved library could not be opened under its key. It was kept aside, not deleted.',
    } as Record<string, string>,
    reopen: 'Open the library again',
    unsaved: (why: string) => `The last change could not be saved on this Mac (${why}). It is shown here but will be lost when the app quits.`,
    saveAgain: 'Save again',
    // Export (attempt-02 R3): what an export holds, and how it went.
    transferNote: 'Exports hold prompts and responses as plaintext. Imports must belong to this tenant.',
    export: {
        saving: 'Choose where to save the export…',
        saved: (name: string) => `Saved ${name}. Cases exported as plaintext JSON with private file permissions. Treat the file as sensitive content.`,
        cancelled: 'Export cancelled. Nothing was saved.',
        failed: 'The export could not be saved.',
    },
    notice: {
        saved: (name: string) => `Saved ${name} to this tenant's cases.`,
        imported: (n: number) => (n === 0 ? 'Every case in that file is already in the library.' : `Imported ${n} case${n === 1 ? '' : 's'}.`),
        examplesAdded: (n: number) => `Added ${n} example${n === 1 ? '' : 's'}, not run.`,
        copied: (tenant: string) => `Copied as a new case into ${tenant}. Review its profile there.`,
        catalogUnavailable: 'The public example catalog could not be read. Nothing was changed.',
        fileUnreadable: 'That file could not be read. Nothing was changed.',
    },
} as const

export const PRODUCT_LABEL: Record<CaseProduct, string> = {
    'runtime-security': 'Runtime Security',
    'ai-gateway': 'AI Gateway',
    'ai-red-teaming': 'AI Red Teaming',
    'model-security': 'Model Security',
    'skill-scanning': 'Skill Scanning',
}

export const CALLS_COPY = {
    title: 'The calls',
    note: 'The newest 50 calls this app made for this tenant, newest last. The core attaches the real credential; it is never shown, and neither are queries or bodies.',
    empty: 'No calls yet. Read, scan or apply something and its calls appear here.',
    hide: 'Hide the calls',
    purpose: { scan: 'Scan', management: 'Management', inference: 'AI Gateway' } as Record<CallPurpose, string>,
    credential: {
        scanKey: 'scan API key (attached by the core)',
        managementClient: 'management client token (attached by the core)',
        workspaceKey: 'workspace key (attached by the core)',
    } as Record<CallCredential, string>,
    failed: 'no response',
    query: 'query withheld',
    requestHeaders: 'Request headers',
    responseHeaders: 'Response headers',
    bytes: (n: number) => `${n} bytes sent`,
} as const

export const API_COPY = {
    title: 'API Client',
} as const
