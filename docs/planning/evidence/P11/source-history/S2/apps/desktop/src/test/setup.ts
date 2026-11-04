import '@testing-library/jest-dom/vitest'
import { afterEach } from 'vitest'
import { cleanup } from '@testing-library/react'

// The presentation never reaches the network; any attempt fails the test.
const refuse = () => {
    throw new Error('network access is not permitted in presentation tests')
}
globalThis.fetch = refuse as unknown as typeof fetch
globalThis.XMLHttpRequest = function XMLHttpRequest() {
    refuse()
} as unknown as typeof XMLHttpRequest
globalThis.WebSocket = function WebSocket() {
    refuse()
} as unknown as typeof WebSocket

// jsdom has no layout, so it does not implement scrollIntoView.
Element.prototype.scrollIntoView = function scrollIntoView() {}

afterEach(() => cleanup())
