type DeviceState = Record<string, any>;

interface PlantState {
  devices: Record<string, DeviceState>;
  subscribers: Map<string, Set<(state: DeviceState) => void>>;
}

const plantState: PlantState = {
  devices: {},
  subscribers: new Map(),
};

export function subscribe(
  deviceId: string,
  callback: (state: DeviceState) => void
): () => void {
  if (!plantState.subscribers.has(deviceId)) {
    plantState.subscribers.set(deviceId, new Set());
  }

  plantState.subscribers.get(deviceId)!.add(callback);

  if (plantState.devices[deviceId]) {
    callback(plantState.devices[deviceId]);
  }

  return () => {
    plantState.subscribers.get(deviceId)?.delete(callback);
  };
}

export function updatePlantState(devices: Record<string, DeviceState>) {
  for (const [deviceId, deviceState] of Object.entries(devices)) {
    const previousState = plantState.devices[deviceId];
    plantState.devices[deviceId] = deviceState;

    if (hasChanged(previousState, deviceState)) {
      notifySubscribers(deviceId, deviceState);
    }
  }
}

function hasChanged(
  previous: DeviceState | undefined,
  current: DeviceState
): boolean {
  if (!previous) return true;

  return JSON.stringify(previous) !== JSON.stringify(current);
}

function notifySubscribers(deviceId: string, state: DeviceState) {
  const subscribers = plantState.subscribers.get(deviceId);
  if (subscribers) {
    subscribers.forEach((callback) => callback(state));
  }
}

export function getDeviceState(deviceId: string): DeviceState | undefined {
  return plantState.devices[deviceId];
}

export function getAllDevices(): Record<string, DeviceState> {
  return { ...plantState.devices };
}
