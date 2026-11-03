import { createContext, useContext } from 'react'

/**
 * A feature view reused inside the v2 shell, presented in its layout (C33):
 * set by the v2 wrapper, read by the shared components it renders (the page
 * heading, the API Client's request editor). Absent: the legacy layout.
 */
export const V2Layout = createContext(false)

export const useV2Layout = () => useContext(V2Layout)
