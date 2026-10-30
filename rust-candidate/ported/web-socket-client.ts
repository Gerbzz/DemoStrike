// Original CS Demo Manager listener semantics over one Tauri event subscription.
type Listener = (payload: unknown) => void;
type PushMessage = { name: string; payload?: unknown };
type Subscription = { native: any; handler: number; id?: number; closed: boolean; failed: boolean; established: boolean; ready: Promise<void> };
const clients = new Set<WeakRef<WebSocketClient>>();
let current: Subscription | undefined;
let lifecycleRegistered = false;
let pageActive = true;
let pageWaiters: Array<() => void> = [];
let retryTimer: ReturnType<typeof setTimeout> | undefined;
let retryAttempt = 0;
let generation = 0;
function clearRetry() { if (retryTimer !== undefined) clearTimeout(retryTimer); retryTimer = undefined; }
function scheduleRetry() {
  if (!pageActive || current || retryTimer !== undefined || clients.size === 0) return;
  const delay = Math.min(30000, 250 * 2 ** Math.min(retryAttempt++, 7));
  retryTimer = setTimeout(() => { retryTimer = undefined; if (pageActive) void ensureReady().catch(() => {}); }, delay);
}
function reportTransportFailure(error: unknown) { notifyClients(false, error); scheduleRetry(); }
function reportSynchronousFailure(error: unknown, attempt: number) {
  queueMicrotask(() => { if (pageActive && !current && generation === attempt) reportTransportFailure(error); });
}

function notifyClients(connected: boolean, error?: unknown) {
  const noticeGeneration = generation;
  for (const ref of clients) {
    if (!pageActive || generation !== noticeGeneration) break;
    const client = ref.deref();
    if (!client) { clients.delete(ref); continue; }
    try { if (connected) client.transportConnected(); else client.transportFailed(error); }
    catch (callbackError) { console.error('CS Demo Manager connection callback failed', callbackError); }
  }
}
function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const native = (window as any).__TAURI_INTERNALS__;
  if (!native?.invoke) return Promise.reject(new Error('This renderer requires the DemoStrike Tauri host.'));
  return native.invoke(command, args);
}
function dispose(entry: Subscription) {
  if (!entry.closed) { entry.closed = true; entry.native.unregisterCallback?.(entry.handler); }
  if (entry.id !== undefined) {
    const id = entry.id; entry.id = undefined;
    try { (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__?.unregisterListener('csdm-push', id); } catch (_) {}
    try { void Promise.resolve(entry.native.invoke('plugin:event|unlisten', { event: 'csdm-push', eventId: id })).catch(() => {}); } catch (_) {}
  }
  if (current === entry) current = undefined;
}
function subscribe(): Promise<void> {
  if (!lifecycleRegistered) {
    window.addEventListener('pagehide', () => { pageActive = false; generation++; clearRetry(); if (current) dispose(current); });
    window.addEventListener('pageshow', () => { pageActive = true; const waiting = pageWaiters; pageWaiters = []; for (const wake of waiting) wake(); if (clients.size) void ensureReady().catch(() => {}); });
    lifecycleRegistered = true;
  }
  if (current) return current.ready;
  const attempt = ++generation;
  const native = (window as any).__TAURI_INTERNALS__;
  if (!native?.invoke || !native?.transformCallback) { const error = new Error('Tauri event transport is unavailable.'); reportSynchronousFailure(error, attempt); return Promise.reject(error); }
  let handler: number;
  try {
    handler = native.transformCallback((event: { payload?: PushMessage }) => {
      const message = event?.payload;
      if (!message || typeof message.name !== 'string') return;
      for (const ref of clients) { const client = ref.deref(); if (client) client.dispatch(message); else clients.delete(ref); }
    });
  } catch (error) { reportSynchronousFailure(error, attempt); return Promise.reject(error); }
  const entry: Subscription = { native, handler, closed: false, failed: false, established: false, ready: Promise.resolve() };
  current = entry;
  try {
    entry.ready = Promise.resolve(native.invoke('plugin:event|listen', { event: 'csdm-push', target: { kind: 'Any' }, handler }))
      .then((id: number) => { entry.id = id; if (entry.closed) dispose(entry); else if (current === entry && pageActive) { entry.established = true; clearRetry(); retryAttempt = 0; notifyClients(true); } }, (error: unknown) => { const active = !entry.closed && current === entry && pageActive; if (active) entry.failed = true; dispose(entry); if (active) reportTransportFailure(error); throw error; });
  } catch (error) { entry.failed = true; dispose(entry); reportSynchronousFailure(error, attempt); return Promise.reject(error); }
  return entry.ready;
}
async function ensureReady(): Promise<void> {
  for (;;) {
    if (!pageActive) await new Promise<void>((resolve) => pageWaiters.push(resolve));
    const ready = subscribe();
    const entry = current;
    try { await ready; } catch (error) { if (!entry || entry.failed) throw error; continue; }
    if (pageActive && entry && current === entry && !entry.closed) return;
  }
}
export class WebSocketClient {
  private listeners = new Map<string, Listener[]>();
  private ready: Promise<void>;
  private connectionState: 'pending' | 'connected' | 'error' = 'pending';
  private onConnectionSuccess: () => void;
  private onConnectionError: (event: CloseEvent) => void;
  public constructor(onConnectionSuccess: () => void, onConnectionError: (event: CloseEvent) => void) {
    this.onConnectionSuccess = onConnectionSuccess;
    this.onConnectionError = onConnectionError;
    clients.add(new WeakRef(this));
    this.ready = ensureReady().then(() => { if (pageActive && current?.established && !current.closed) this.transportConnected(); });
    void this.ready.catch(() => {});
  }
  public transportConnected() {
    if (this.connectionState === 'connected') return;
    this.connectionState = 'connected';
    this.onConnectionSuccess();
  }
  public transportFailed(error: unknown) {
    if (this.connectionState === 'error') return;
    this.connectionState = 'error';
    // Code 0 denotes no WebSocket close code; preserve the actual Tauri failure separately.
    this.onConnectionError({ code: 0, reason: 'Tauri push subscription failed', cause: error } as unknown as CloseEvent);
  }
  public dispatch(message: PushMessage) {
    try { for (const listener of this.listeners.get(message.name) ?? []) listener(message.payload); }
    catch (error) { console.error('CS Demo Manager push dispatch failed', error); }
  }
  public on(name: string, listener: Listener) { const listeners = this.listeners.get(name); if (listeners === undefined) this.listeners.set(name, [listener]); else listeners.push(listener); }
  public off(name: string, listener: Listener) { const listeners = this.listeners.get(name); if (listeners !== undefined) this.listeners.set(name, listeners.filter((item) => item !== listener)); }
  public removeAllEventListeners(name: string) { this.listeners.set(name, []); }
  public send(message: { name: string; payload?: unknown }) {
    const request = ensureReady().then(() => invoke('csdm_request', { name: message.name, payload: message.payload }));
    if (message.name === 'connect-database') return request.catch((error) => ({ code: 1, message: String(error) }));
    if (message.name === 'insert-tag' || message.name === 'update-tag' || message.name === 'delete-tag' || message.name === 'update-checksums-tags' || message.name === 'update-players-tags') {
      return request.catch((error) => { console.error('Tag request failed; database outcome may be unknown', error); throw 1; }).then((value: any) => {
        if (value && typeof value === 'object' && Object.keys(value).length === 1 && Object.hasOwn(value, '__demostrikeTagError')) {
          const code = value.__demostrikeTagError;
          throw [1, 3000, 3001, 3002, 3003].includes(code) ? code : 1;
        }
        return message.name === 'update-tag' || message.name === 'delete-tag' || message.name === 'update-checksums-tags' || message.name === 'update-players-tags' ? undefined : value;
      });
    }
    return request;
  }
}
