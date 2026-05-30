import type { FieldValues, PlantSnapshot } from './types';
import { WsClient } from './ws-client';
import { wsUrl } from '../config';

type StateCallback = (state: FieldValues) => void;

const devices:     PlantSnapshot                    = {};
const subscribers: Map<string, Set<StateCallback>>  = new Map();

export function subscribe(deviceId: string, cb: StateCallback): () => void {
  if (!subscribers.has(deviceId)) subscribers.set(deviceId, new Set());
  subscribers.get(deviceId)!.add(cb);

  // Replay current state immediately if already populated.
  if (devices[deviceId]) cb(devices[deviceId]);

  return () => subscribers.get(deviceId)?.delete(cb);
}

export function connectToBackend(): void {
  new WsClient({
    url:     wsUrl(),
    onFrame: (data) => ingest(JSON.parse(data) as PlantSnapshot),
  }).connect();
}

let logCounter = 0;

function ingest(snapshot: PlantSnapshot): void {
  logCounter++;
  
  for (const [deviceId, fields] of Object.entries(snapshot)) {
    const prev = devices[deviceId];
    devices[deviceId] = fields;
    if (!prev || JSON.stringify(prev) !== JSON.stringify(fields)) {
      subscribers.get(deviceId)?.forEach(cb => cb(fields));
    }
  }
  
  // Log every 100 updates (roughly 10 seconds at 100ms tick)
  if (logCounter >= 100) {
    logCounter = 0;
    console.log(`[Frontend] Telemetry update - ${Object.keys(snapshot).length} devices`);
    const deviceEntries = Object.entries(snapshot).slice(0, 3);
    for (const [deviceId, fields] of deviceEntries) {
      console.log(`[Frontend]   ${deviceId}:`, fields);
    }
  }
}