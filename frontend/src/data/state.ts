import type { FieldValues, PlantSnapshot } from './types';
import { WsClient } from './ws-client';
import { wsUrl } from '../config';

type StateCallback = (state: FieldValues) => void;

const devices:     PlantSnapshot                    = {};
const subscribers: Map<string, Set<StateCallback>>  = new Map();

// ============================================================================
// Public API
// ============================================================================

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

// ============================================================================
// Internal
// ============================================================================

function ingest(snapshot: PlantSnapshot): void {
  for (const [deviceId, fields] of Object.entries(snapshot)) {
    const prev = devices[deviceId];
    devices[deviceId] = fields;
    if (!prev || JSON.stringify(prev) !== JSON.stringify(fields)) {
      subscribers.get(deviceId)?.forEach(cb => cb(fields));
    }
  }
}
