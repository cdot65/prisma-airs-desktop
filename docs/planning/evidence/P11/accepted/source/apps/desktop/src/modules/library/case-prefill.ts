/** What the scan form is filled with (a Runtime case or a starter). */
export interface CasePrefill {
    name: string
    profile: string
    prompt: string
    response: string
    appName: string
    /** Shown as the loaded case's goal; never a verdict. */
    expected: string
    /** A new value remounts the scan form with these fields. */
    nonce: number
}

