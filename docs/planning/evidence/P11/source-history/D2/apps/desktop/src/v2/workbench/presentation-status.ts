import type { RunRecord } from '@prisma-airs/desktop-contracts'

/** The inspected response can be a collection-history entry from another request. */
export interface DisplayedResponse {
    record: RunRecord
    history: boolean
}

/** Presentation state only. No request/credential authority moves into the footer. */
export interface EditorStatus {
    save: { text: string; state: 'new' | 'unsaved' | 'saved' }
    response: DisplayedResponse | null
}
