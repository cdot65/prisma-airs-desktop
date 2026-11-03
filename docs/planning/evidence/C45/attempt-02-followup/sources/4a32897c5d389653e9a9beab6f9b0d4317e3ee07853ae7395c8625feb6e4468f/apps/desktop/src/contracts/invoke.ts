/**
 * Binds the typed desktop facade to Tauri's IPC. Requests are sent as raw
 * bytes so the native host applies its size ceiling before parsing.
 */
import { Channel, invoke } from '@tauri-apps/api/core'
import { DesktopClient, type DesktopTransport } from '@prisma-airs/desktop-contracts'

export const tauriTransport: DesktopTransport = {
    dispatch: (bytes) => invoke('desktop_dispatch', bytes),
    observe: (bytes) => invoke('desktop_observe', bytes),
    openChannel(onMessage) {
        const channel = new Channel<unknown>()
        channel.onmessage = onMessage
        return {
            token: channel.toJSON(),
            close() {
                // Stop delivering and drop the registered callback; the facade
                // separately asks the host to end its native task.
                channel.onmessage = () => undefined
                window.__TAURI_INTERNALS__?.unregisterCallback?.(channel.id)
            },
        }
    },
}

export function createDesktopClient(): DesktopClient {
    return new DesktopClient(tauriTransport)
}
