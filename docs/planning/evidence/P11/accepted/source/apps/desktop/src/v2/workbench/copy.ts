/**
 * Copy for the v2 API Client, Test Cases and "Show the calls" (C33). Tests
 * assert these exact strings.
 */
import type { CallCredential, CallPurpose, CaseProduct } from '@prisma-airs/desktop-contracts'

export const CASES_COPY = {
    title: 'Test Cases',
    count: (n: number, tenant: string) => `${n} ${n === 1 ? 'case' : 'cases'} in ${tenant} · sealed on this Mac, per tenant`,
    explain: 'AI Runtime Security cases run in its Playground. Cases for the other products are specifications you inspect, copy to another tenant or hand to that product.',
    loadPacks: 'Load starter packs…',
    packs: {
        title: 'Load starter packs',
        lead: (tenant: string) => `Ready-made cases for the products ${tenant} manages. Loaded packs are kept on this Mac.`,
        bundled: 'Bundled starter packs',
        bundledNote: 'Shipped with this app',
        public: 'Public examples',
        publicNote: 'Fetched from the public catalog',
        load: 'Load selected',
        cancel: 'Cancel',
        transfer: 'Your own cases',
    },
    newCaseTitle: 'New test case',
    newCaseLead: (tenant: string) => `Saved to ${tenant} on this Mac. Runtime Security cases run in the Playground; other products keep a specification to inspect or hand over.`,
    more: (name: string) => `More actions for ${name}`,
    clearFilters: 'Clear search and filters',
    page: (n: number, of: number) => `Page ${n} of ${of}`,
    columns2: { input: 'Input or specification' },
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
    showResult: (name: string) => `Show the result of ${name}`,
    input: 'Input or specification',
    responseRow: 'Response',
    payloadRow: 'Product payload',
    scanId: 'Scan ID',
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
    close: 'Close',
    closeExplain: 'Close Explain',
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

/** The v2 API Client (C41): R2App start=api over the C14/C15 workbench behaviour. Tests assert these exact strings. */
export const API_COPY = {
    title: 'API Client',
    description: 'Edit requests and inspect responses. Tenant credentials go only to known operations.',
    leadNone: 'Open a folder collection or start with the built-in Strata Cloud Manager requests.',
    chips: {
        collection: 'Collection',
        environment: 'Environment',
        builtIn: 'built-in',
        noEnvironment: 'none',
        envOrigin: {
            shared: 'shared',
            local: 'private',
            both: 'shared + private',
        } as Record<string, string>,
    },
    network: {
        loopbackOnly: 'This build sends API Client requests to loopback addresses only.',
        none: 'This build does not send API Client requests.',
    },
    unavailable: "The API Client's private folder is not available in this build.",
    privateFailure: (why: string) => `Private API Client data is unavailable (${why}). Drafts cannot be protected until it opens.`,
    opening: 'Opening the API Client…',
    working: 'Working…',
    // The collection menu (the Collection chip) and the tools it opens.
    menu: 'Collection actions',
    refresh: 'Refresh',
    tools: {
        environments: 'Environments…',
        secrets: 'Secrets…',
        schemas: 'Schemas…',
        import: 'Import…',
        git: 'Git…',
    },
    closeCollection: 'Close collection',
    issues: (n: number, complete: boolean) =>
        complete
            ? `Some files need attention (${n} listed). Requests sharing an identity are not shown, and their files are not changed.`
            : `This collection has problems to fix before files can change (${n} listed). Requests with problems are not shown.`,
    unsaved: (why: string) => `Drafts could not be protected on this Mac (${why}). Save or keep the app open until this clears.`,
    // The request tree.
    // The tree's name must not collide with a request document's form ('Request <name>'): a request named list is 'Request list'.
    tree: 'Requests',
    find: 'Find request',
    clearSearch: 'Clear search',
    noMatch: (q: string, n: number) => `No requests match “${q}”. ${n} ${n === 1 ? 'request is' : 'requests are'} in this collection.`,
    noRequests: 'This collection has no requests yet.',
    newRequest: 'New request',
    newRequestTitle: 'New request',
    newRequestLead: 'The request stays a private draft until you Save it.',
    newRequestPath: 'Path (folder/name)',
    newFolder: 'New folder',
    newFolderPath: 'Folder path',
    create: 'Create',
    cancel: 'Cancel',
    openRequest: (method: string, relative: string) => `Open ${method} ${relative}`,
    folder: (name: string, open: boolean) => `${open ? 'Collapse' : 'Expand'} folder ${name}`,
    tooMany: (shown: number, of: number) => `Showing ${shown} of ${of}; refine the search.`,
    // Open requests and the request line.
    openRequests: 'Open requests',
    closeTab: (name: string) => `Close tab ${name}`,
    selectRequest: 'Select a request, or create one with New request.',
    loadFailed: (name: string, why: string) => `${name} could not be opened: ${why}`,
    unknownFailure: 'the desktop did not report how it ended.',
    tryAgain: 'Try again',
    method: 'Method',
    url: 'URL',
    send: 'Send',
    save: 'Save',
    state: {
        saved: 'Saved',
        unsaved: 'Unsaved changes',
        new: 'New · not saved yet',
    },
    conflict: {
        changed: 'Changed on disk',
        deleted: 'Deleted on disk',
        interrupted: 'Interrupted save',
        changedText: 'The files changed since this request was opened. Choose which version to continue from; both are kept privately first.',
        interruptedText: 'An earlier save of this request did not finish. Choose which version to continue from; both are kept privately first.',
        useDisk: 'Use disk',
        keepMine: 'Keep mine',
    },
    recovered: 'Recovered draft',
    recoveredText: 'Unsaved changes from an earlier session were restored. Save to keep them in the collection.',
    discardDraft: 'Discard draft',
    discardChanges: 'Discard changes',
    notes: 'Conversion report',
    // The request's sub-tabs (the prototype's six).
    request: 'Request',
    sub: {
        params: 'Params',
        headers: 'Headers',
        body: 'Body',
        auth: 'Auth',
        variables: 'Variables',
        preview: 'Preview',
    },
    subText: {
        params: 'Query parameters are appended to the URL in order. Disabled rows are kept in the file but not sent.',
        headers: 'Headers are sent as written. Secret values come from the Keychain through {{references}} and are never written here.',
        body: 'The body as it will be sent, edited as text. Secrets are never part of it.',
        bodyFixed: 'This body is not text or is larger than 4 MiB; it is kept exactly as it is.',
        auth: '[auth] names the kind (none, basic, bearer, api-key or oauth2) and secret references; [variables] holds strings or { secret = "reference" }. Values are entered in Secrets, never here.',
        variables: 'The variables the selected environment and this request resolve to, as Preview reads them. Secret values are masked.',
        preview: 'The request exactly as it will be sent, with secrets masked. Nothing is sent from here.',
    },
    table: 'Table',
    text: 'Text',
    textOnly: 'Some rows have no separator; edit them as text.',
    addRow: { params: '+ Add parameter', headers: '+ Add header' },
    columns: {
        enabled: 'Enabled',
        name: 'Name',
        value: 'Value',
        remove: 'Remove',
    },
    removeRow: (name: string) => `Remove row ${name || 'without a name'}`,
    rowEnabled: (name: string) => `Row ${name || 'without a name'} enabled`,
    rowName: 'Name',
    rowValue: 'Value',
    bodyKind: 'Body kind',
    body: 'Body',
    settings: 'Auth and variables (TOML)',
    schema: 'Schema',
    noSchema: 'No schema',
    readPreview: 'Read the preview',
    previewHint: 'Preview prepares the request from what you have typed; Send prepares it again.',
    // The response pane.
    pane: 'Response pane',
    noResponse: 'Send to see the response here.',
    noResponseYet: 'No response yet',
    sending: 'Sending…',
    waitingReview: 'Waiting for your confirmation…',
    reviewBelow: 'This change to the tenant needs your review below.',
    response: 'Response',
    responseTabs: {
        body: 'Body',
        headers: 'Headers',
        details: 'Details',
        history: 'History',
    },
    pretty: 'Pretty',
    raw: 'Raw',
    copy: 'Copy',
    copied: 'Copied the response body.',
    copyFailed: 'The response body could not be copied.',
    saveResponse: 'Save response…',
    saved: (size: string, name: string) => `Saved ${size} as ${name}.`,
    responseBody: 'Response body',
    responseHeaders: 'Response headers',
    notText: 'The body is not text; save it to open it elsewhere.',
    firstBytes: (size: string) => `Showing the first ${size}.`,
    noBody: 'The response has no body.',
    details: {
        request: 'Request',
        status: 'Status',
        duration: 'Duration',
        size: 'Size',
        when: 'Sent',
        credential: 'Credential',
        operation: 'Operation',
        stored: 'Stored body',
        yes: 'complete',
        no: 'not stored',
        anonymous: 'none (anonymous request)',
    },
    uncertain: 'The request may have reached the server; no response arrived. It was not sent again. Check the result before sending again.',
    redirect: (to: string) => `Redirect to ${to} was not followed. Send a request to it to follow it.`,
    tokenDropped: 'The server refused the token; it was dropped. Send again to get a new one (nothing was resent).',
    cancel_: 'Cancel request',
    sendingProgress: (method: string, size: string) => `Sending ${method}… ${size} transferred`,
    // History (the response pane's History tab).
    historyLead: 'Requests sent from this collection on this Mac (30 days, up to 200). History shows what happened; nothing here sends anything again.',
    historyEmpty: 'No requests sent yet.',
    older: 'Older requests',
    clearHistory: 'Clear history',
    clearCookies: 'Clear cookies',
    cookiesCleared: 'Cookies cleared.',
    showRun: (summary: string) => `Show ${summary}`,
    // Review of a tenant-credential change.
    review: {
        title: (operation: string) => `Review this change to the tenant: ${operation}`,
        goes: (destination: string) => `Goes to ${destination} with the active tenant's credential. It is sent once and never again by itself.`,
        files: (files: string) => `Files sent as they are now: ${files}`,
        expires: (at: string) =>
            `Secret values are masked. The review expires at ${at}; any change to the request, its files or environment needs a new review.`,
        apply: 'Send to tenant…',
        dismiss: 'Dismiss review',
    },
    // Closing with unprotected edits.
    closing: {
        tab: (name: string) => `Close ${name} without your latest edits?`,
        collection: 'Close the collection without your latest edits?',
        text: 'The latest edits could not be kept on this Mac. Keep editing to try again, or close and discard them.',
        keep: 'Keep editing',
        discard: 'Close and discard edits',
    },
    // The start panel (no collection open).
    start: {
        restoreFailed: 'The preferred collection could not be opened. Restore its folder and try again, choose another collection, or open Strata Cloud Manager.',
        none: 'No collection is open.',
        open: 'Open collection…',
        create: 'New collection…',
        builtIn: 'Open Strata Cloud Manager',
        builtInNote: "Strata Cloud Manager: the built-in collection of 193 SCM and Prisma AIRS request specifications, kept in this app's private folder.",
        recent: 'Recent collections',

    },
    footer: {
        ready: 'Ready',
        requests: (n: number) => `${n} ${n === 1 ? 'request' : 'requests'}`,
        archives: (n: number) => `${n} earlier ${n === 1 ? 'version' : 'versions'} kept privately`,
    },
} as const

/** The method's colour (R2App mColor). */
export function methodColor(method: string): string {
    switch (method.toUpperCase()) {
        case 'GET':
            return 'var(--v2-ok)'
        case 'POST':
            return 'var(--v2-acc)'
        case 'PUT':
        case 'PATCH':
            return 'var(--v2-flag)'
        case 'DELETE':
            return 'var(--v2-block)'
        default:
            return 'var(--v2-soft)'
    }
}

export const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
export const BODY_KINDS = ['text', 'json', 'form', 'multipart', 'file']
