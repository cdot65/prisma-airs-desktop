// Tauri injects these IPC internals into the packaged WebView; the transport
// uses unregisterCallback to release a closed channel's callback.
export {}
declare global {
    interface Window {
        __TAURI_INTERNALS__?: {
            unregisterCallback?: (id: number) => void
            runCallback?: (id: number, data: unknown) => void
        }
    }
}
