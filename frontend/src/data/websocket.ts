import { updatePlantState } from './state';

interface WebSocketMessage {
  type: 'snapshot' | 'delta';
  timestamp: string;
  devices: Record<string, any>;
}

let ws: WebSocket | null = null;
let reconnectTimeout: number | null = null;
let reconnectDelay = 1000;

export function connectWebSocket() {
  const wsUrl = `ws://${window.location.hostname}:3000/ws`;

  console.log(`Connecting to WebSocket: ${wsUrl}`);

  ws = new WebSocket(wsUrl);

  ws.onopen = () => {
    console.log('WebSocket connected');
    reconnectDelay = 1000;
  };

  ws.onmessage = (event) => {
    try {
      const message: WebSocketMessage = JSON.parse(event.data);
      handleMessage(message);
    } catch (error) {
      console.error('Failed to parse WebSocket message:', error);
    }
  };

  ws.onerror = (error) => {
    console.error('WebSocket error:', error);
  };

  ws.onclose = () => {
    console.log('WebSocket disconnected, attempting to reconnect...');
    scheduleReconnect();
  };
}

function handleMessage(message: WebSocketMessage) {
  console.log('Received message:', message.type, message);

  if (message.type === 'snapshot' || message.type === 'delta') {
    updatePlantState(message.devices);
  }
}

function scheduleReconnect() {
  if (reconnectTimeout) {
    clearTimeout(reconnectTimeout);
  }

  reconnectTimeout = window.setTimeout(() => {
    console.log(`Reconnecting... (delay: ${reconnectDelay}ms)`);
    connectWebSocket();
    reconnectDelay = Math.min(reconnectDelay * 2, 30000);
  }, reconnectDelay);
}

export function sendCommand(command: any) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(command));
  } else {
    console.warn('WebSocket not connected, cannot send command');
  }
}
