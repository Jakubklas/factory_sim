import type { FieldValues, PlantSnapshot } from './types';
import { WsClient } from './ws-client';
import { wsUrl } from '../config';

type StateCallback = (state: FieldValues) => void;

const devices:     PlantSnapshot                    = {};
const subscribers: Map<string, Set<StateCallback>>  = new Map();

export function subscribe(deviceId: string, cb: StateCallback): () => void {
  if (!subscribers.has(deviceId)) subscribers.set(deviceId, new Set());
  subscribers.get(deviceId)!.add(cb);

  if (devices[deviceId]) cb(devices[deviceId]);

  return () => subscribers.get(deviceId)?.delete(cb);
}

export function connectToBackend(): void {
  const url = wsUrl();
  console.log(`[ws] connecting to ${url}`);
  new WsClient({
    url,
    onFrame:       (data) => ingest(JSON.parse(data) as PlantSnapshot),
    onConnect:     () => console.log('[ws] connected'),
    onDisconnect:  () => console.warn('[ws] disconnected — will retry'),
  }).connect();
}

let frameCount  = 0;
let changeCount = 0;
let lastSummary = Date.now();

function ingest(snapshot: PlantSnapshot): void {
  frameCount++;

  for (const [deviceId, fields] of Object.entries(snapshot)) {
    const prev = devices[deviceId];
    devices[deviceId] = fields;
    if (!prev || JSON.stringify(prev) !== JSON.stringify(fields)) {
      changeCount++;
      subscribers.get(deviceId)?.forEach(cb => cb(fields));
    }
  }

  // Summary every 30s
  const now = Date.now();
  if (now - lastSummary >= 30_000) {
    const devs = Object.keys(snapshot).length;
    console.log(`[state] 30s summary — frames: ${frameCount}, changes dispatched: ${changeCount}, devices in snapshot: ${devs}`);
    for (const [deviceId, fields] of Object.entries(snapshot)) {
      console.log(`[state]   ${deviceId}:`, fields);
    }
    frameCount  = 0;
    changeCount = 0;
    lastSummary = now;
  }
}
