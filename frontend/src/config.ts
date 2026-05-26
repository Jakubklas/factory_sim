// Backend URL resolution.
// 1. window.__env.BE_URL is injected at container start via /env.js.
// 2. If absent (Vite dev / direct file open), fall back to same-host + port 3001.
declare global {
  interface Window { __env?: { BE_URL?: string } }
}

const fallback = () => `http://${window.location.hostname}:3001`;
const beUrl    = () => window.__env?.BE_URL || fallback();

export const apiBase = () => beUrl();
export const wsUrl   = () => `${beUrl().replace(/^http/, 'ws')}/ws`;
