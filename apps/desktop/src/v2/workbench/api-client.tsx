import { forwardRef } from 'react'
import type { ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { WorkbenchView } from '../../features/workbench/workbench-view'
import { V2Layout } from '../../shell/presentation'
import type { Dispatch } from '../../features/tenants/dispatch'
import type { ObserveWorkbench } from '../../features/workbench/workbench-call'

export interface ApiClientProps {
    snapshot: ShellSnapshot
    dispatch: Dispatch
    observe: ObserveWorkbench
}

/**
 * The API Client in the v2 shell (C33): the C14/C15 workbench (collections,
 * requests, Send with its completion status region, immutable history) in
 * the v2 page rhythm. Its title row is the workbench's own heading. Generic
 * requests keep the core's mutation authority: a known tenant change needs
 * its exact review; an unknown operation is refused.
 */
export const ApiClient = forwardRef<HTMLHeadingElement, ApiClientProps>(function ApiClient({ snapshot, dispatch, observe }, ref) {
    return (
        <div data-slot='api-client-v2' className='flex min-w-0 flex-col gap-4'>
            {/* The workbench's own components take the v2 layout (title row, request line, response pane). */}
            <V2Layout.Provider value={true}>
                <WorkbenchView ref={ref} snapshot={snapshot} dispatch={dispatch} observe={observe} />
            </V2Layout.Provider>
        </div>
    )
})
