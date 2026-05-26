// Placeholder for dev. In production, the nginx container's entrypoint overwrites this
// with: window.__env = { BE_URL: "<from BE_URL env var>" }. When this file is empty,
// frontend/src/config.ts falls back to http://<current-hostname>:3001.
window.__env = window.__env || {};
