export const BE_PORT = 3001;
export const wsUrl    = () => `ws://${window.location.hostname}:${BE_PORT}/ws`;
export const apiBase  = () => `http://${window.location.hostname}:${BE_PORT}`;
