// Pure WebSocket transport. No knowledge of plant state or data format.
// Handles connection, reconnection with exponential backoff, and clean shutdown.

export interface WsClientOptions {
  url:           string;
  onFrame:       (data: string) => void;
  onConnect?:    () => void;
  onDisconnect?: () => void;
}

export class WsClient {
  private ws:             WebSocket | null = null;
  private reconnectDelay: number = 1_000;
  private stopped:        boolean = false;

  constructor(private readonly opts: WsClientOptions) {}

  connect(): void {
    this.stopped = false;
    this.open();
  }

  disconnect(): void {
    this.stopped = true;
    this.ws?.close();
  }

  private open(): void {
    this.ws = new WebSocket(this.opts.url);

    this.ws.onopen = () => {
      console.log('[ws] connected to', this.opts.url);
      this.reconnectDelay = 1_000;
      this.opts.onConnect?.();
    };

    this.ws.onmessage = (e: MessageEvent) => {
      this.opts.onFrame(e.data as string);
    };

    this.ws.onerror = (e: Event) => {
      console.error('[ws] error', e);
    };

    this.ws.onclose = () => {
      console.log('[ws] disconnected');
      this.opts.onDisconnect?.();
      if (!this.stopped) this.scheduleReconnect();
    };
  }

  private scheduleReconnect(): void {
    setTimeout(() => {
      console.log(`[ws] reconnecting in ${this.reconnectDelay}ms`);
      this.reconnectDelay = Math.min(this.reconnectDelay * 2, 30_000);
      this.open();
    }, this.reconnectDelay);
  }
}
