// Diagnostic observer only. The canonical native driver is executed unchanged.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn } = require('node:child_process');
const { StringDecoder } = require('node:string_decoder');

const BRANCH = 'refs/heads/diagnostic/dem-native-context-20261022';
const DRIVER_BLOB = '04b7c48938e8d904c4e1410de2f0b46f26ed7823';
const METHODS = new Set(['Browser.getVersion', 'Page.navigate', 'Page.navigateToHistoryEntry',
  'Page.getNavigationHistory', 'Page.createIsolatedWorld', 'Page.captureScreenshot',
  'Page.getLayoutMetrics', 'Runtime.evaluate', 'Runtime.callFunctionOn',
  'Runtime.getProperties', 'Runtime.releaseObject', 'DOM.describeNode', 'DOM.resolveNode']);
const EVENTS = new Set(['Runtime.executionContextCreated', 'Runtime.executionContextDestroyed',
  'Runtime.executionContextsCleared', 'Page.frameNavigated', 'Page.navigatedWithinDocument',
  'Page.lifecycleEvent', 'Inspector.targetCrashed', 'Target.detachedFromTarget']);
const hash = (data) => crypto.createHash('sha256').update(data).digest('hex');
const integer = (value) => Number.isSafeInteger(value) ? value : undefined;
const opaque = (value) => typeof value === 'string' ? hash(value).slice(0, 24) : undefined;
const enumValue = (value, allowed) => allowed.includes(value) ? value : undefined;
const versionValue = (value) => typeof value === 'string' && /^(?:(?:Chrome|Chromium|HeadlessChrome|Microsoft Edge|Edg|WebKit)\/)?[0-9]+(?:\.[0-9]+){0,4}(?:\s+\(@?[0-9a-f]{7,64}\))?$/.test(value) ? value : undefined;
const errorCode = (value) => enumValue(value, ['ENOENT', 'EACCES', 'EPERM', 'EIO', 'EBADF', 'EINVAL', 'ENOMEM', 'EPIPE', 'ENOSPC', 'EMFILE', 'ENFILE', 'ECONNREFUSED', 'ETIMEDOUT']);
const API_METHODS = new Set(['locator.evaluate', 'locator.waitFor', 'locator.click', 'locator.press', 'locator.focus', 'locator.getAttribute', 'locator.textContent', 'locator.count',
  'page.evaluate', 'page.screenshot', 'page.waitForFunction', 'page.waitForTimeout', 'page.goto', 'page.waitForLoadState', 'browser.close']);
let canonicalExitCode;

function safeRoute(value) {
  try {
    const url = new URL(value);
    return { origin: url.hostname === 'tauri.localhost' ? 'tauri.localhost' : 'other',
      viewerRound: Number(url.hash.match(/viewer-2d\/(\d+)/)?.[1]) || null };
  } catch { return { origin: 'unavailable' }; }
}

function safeError(error) {
  if (!error) return undefined;
  const message = String(error.message || '');
  const kind = /context.*destroyed/i.test(message) ? 'context-destroyed'
    : /context.*(find|found|specified)|find.*context/i.test(message) ? 'context-missing'
      : /object.*(find|found)|find.*object/i.test(message) ? 'object-missing'
        : /closed|crashed/i.test(message) ? 'target-closed-or-crashed' : 'other';
  return { code: Number.isFinite(error.code) ? error.code : undefined,
    kind, messageSha256: hash(message), messageLength: message.length };
}

class ProtocolObserver {
  constructor(emit, now = Date.now) { this.emit = emit; this.now = now; this.pending = new Map(); this.counts = {}; }
  accept(line) {
    const protocol = line.includes('pw:protocol');
    if (!protocol) {
      if (!line.includes('pw:api')) return false;
      const api = line.match(/\b((?:page|locator|frame|browser|browserContext|elementHandle)\.[A-Za-z0-9_]+) (started|succeeded|failed)\b/);
      if (api) this.emit({ kind: 'api', api: API_METHODS.has(api[1]) ? api[1] : 'other',
        apiSha256: API_METHODS.has(api[1]) ? undefined : hash(api[1]), phase: api[2] });
      return true; // Debug lines are never forwarded as raw payloads.
    }
    try {
      const begin = line.indexOf('{');
      const end = line.lastIndexOf('}');
      if (begin < 0 || end < begin) throw new Error('No protocol object');
      const message = JSON.parse(line.slice(begin, end + 1));
      const sending = /\bSEND\b/.test(line.slice(0, begin));
      this.consume(message, sending);
    } catch { this.counts.parseErrors = (this.counts.parseErrors || 0) + 1; }
    return true;
  }
  consume(message, sending) {
    if (sending && METHODS.has(message.method)) {
      const params = message.params || {};
      if (integer(message.id) === undefined) { this.counts.invalidIds = (this.counts.invalidIds || 0) + 1; return; }
      const event = { kind: 'send', id: message.id, method: message.method, session: opaque(message.sessionId),
        context: integer(params.executionContextId ?? params.contextId), object: opaque(params.objectId),
        backendNode: integer(params.backendNodeId), frame: opaque(params.frameId),
        functionSha256: typeof params.functionDeclaration === 'string' ? hash(params.functionDeclaration) : undefined,
        expressionSha256: typeof params.expression === 'string' ? hash(params.expression) : undefined };
      if (this.pending.size >= 10000) { this.pending.delete(this.pending.keys().next().value); this.counts.pendingEvictions = (this.counts.pendingEvictions || 0) + 1; }
      this.pending.set(message.id, { ...event, sentAt: this.now() });
      this.emit(event);
    } else if (!sending && message.id && this.pending.has(message.id)) {
      const pending = this.pending.get(message.id);
      this.pending.delete(message.id);
      const result = message.result || {};
      this.emit({ kind: 'receive', id: message.id, method: pending.method, session: pending.session,
        elapsedMs: this.now() - pending.sentAt, error: safeError(message.error),
        object: opaque(result.object?.objectId ?? result.result?.objectId), executionContext: integer(result.executionContextId),
        browser: pending.method === 'Browser.getVersion' ? {
          product: versionValue(result.product), protocol: versionValue(result.protocolVersion),
          javascript: versionValue(result.jsVersion), revisionSha256: typeof result.revision === 'string' ? hash(result.revision) : undefined } : undefined });
    } else if (!sending && EVENTS.has(message.method)) {
      const params = message.params || {};
      const context = params.context || {};
      this.emit({ kind: 'event', method: message.method, session: opaque(message.sessionId),
        context: integer(context.id ?? params.executionContextId), frame: opaque(context.auxData?.frameId ?? params.frameId ?? params.frame?.id),
        world: context.auxData?.isDefault ? 'main' : /utility/i.test(context.name || '') ? 'utility' : 'other',
        contextType: enumValue(context.auxData?.type, ['default', 'isolated', 'worker']),
        lifecycle: enumValue(params.name, ['init', 'commit', 'DOMContentLoaded', 'load', 'networkAlmostIdle', 'networkIdle', 'firstPaint', 'firstContentfulPaint']),
        route: params.url || params.frame?.url ? safeRoute(params.url || params.frame.url) : undefined });
    }
  }
  oversized(prefix, characters) {
    const isProtocol = prefix.includes('pw:protocol');
    if (!isProtocol) { this.emit({ kind: 'oversize-nonprotocol-line-omitted', characters });
      this.counts.oversizedNonprotocol = (this.counts.oversizedNonprotocol || 0) + 1; return; }
    const id = Number(prefix.match(/"id"\s*:\s*(\d+)/)?.[1]);
    if (/\bRECV\b/.test(prefix) && this.pending.has(id)) {
      const pending = this.pending.get(id); this.pending.delete(id);
      this.emit({ kind: 'receive-oversize-payload-omitted', id, method: pending.method,
        elapsedMs: this.now() - pending.sentAt, characters });
    } else this.emit({ kind: 'oversize-protocol-line-omitted', id: id || undefined, characters });
    this.counts.oversized = (this.counts.oversized || 0) + 1;
  }
}

class BoundedLines {
  constructor(onLine, onOversize, maxChars = 262144) {
    this.onLine = onLine; this.onOversize = onOversize; this.maxChars = maxChars;
    this.decoder = new StringDecoder('utf8'); this.buffer = ''; this.dropped = 0; this.prefix = '';
  }
  write(chunk) { this.consume(this.decoder.write(chunk)); }
  consume(value) {
    const pieces = value.split('\n');
    for (let index = 0; index < pieces.length; index++) {
      const piece = pieces[index];
      if (this.dropped) this.dropped += piece.length;
      else if (this.buffer.length + piece.length > this.maxChars) {
        this.prefix = (this.buffer + piece.slice(0, 512)).slice(0, 512);
        this.dropped = this.buffer.length + piece.length; this.buffer = '';
      } else this.buffer += piece;
      if (index < pieces.length - 1) this.flush();
    }
  }
  flush() {
    if (this.dropped) this.onOversize(this.prefix, this.dropped);
    else this.onLine(this.buffer.replace(/\r$/, ''));
    this.buffer = ''; this.dropped = 0; this.prefix = '';
  }
  end() { this.consume(this.decoder.end()); if (this.buffer || this.dropped) this.flush(); }
}

class StderrSplitter {
  constructor(onCanonical, onDebug, onDebugEnd) {
    this.onCanonical = onCanonical; this.onDebug = onDebug; this.onDebugEnd = onDebugEnd;
    this.prefix = Buffer.alloc(0); this.mode = undefined;
  }
  route(bytes) { (this.mode === 'debug' ? this.onDebug : this.onCanonical)(bytes); }
  write(chunk) {
    for (let offset = 0; offset < chunk.length;) {
      const newline = chunk.indexOf(10, offset);
      const end = newline < 0 ? chunk.length : newline + 1;
      const piece = chunk.subarray(offset, end);
      if (this.mode) this.route(piece);
      else {
        const lookahead = Buffer.concat([this.prefix, piece.subarray(0, Math.max(0, 512 - this.prefix.length))]);
        if (newline >= 0 || lookahead.length >= 512) {
          this.mode = /^(?:\d{4}-\d{2}-\d{2}T[0-9:.]+Z\s+)?pw:(?:api|protocol)\s/.test(lookahead.toString('utf8')) ? 'debug' : 'canonical';
          if (this.prefix.length) this.route(this.prefix);
          this.prefix = Buffer.alloc(0); this.route(piece);
        } else this.prefix = lookahead;
      }
      if (newline >= 0) this.mode = undefined;
      offset = end;
    }
  }
  end() {
    if (this.prefix.length) {
      this.mode = /^(?:\d{4}-\d{2}-\d{2}T[0-9:.]+Z\s+)?pw:(?:api|protocol)\s/.test(this.prefix.toString('utf8')) ? 'debug' : 'canonical';
      this.route(this.prefix); this.prefix = Buffer.alloc(0);
    }
    this.onDebugEnd();
  }
}

function boundedWriter(file, maxRecords = 5000, maxBytes = 2 * 1024 * 1024) {
  const start = Date.now(); let records = 0; let bytes = 0; let omitted = 0; let writeErrors = 0; let enabled = true;
  try { fs.writeFileSync(file, ''); } catch { writeErrors++; enabled = false; }
  return { emit(data) {
    const line = JSON.stringify({ at: new Date().toISOString(), elapsedMs: Date.now() - start, ...data }) + '\n';
    const size = Buffer.byteLength(line);
    if (!enabled || records >= maxRecords || bytes + size > maxBytes) { omitted++; return; }
    try { fs.appendFileSync(file, line); records++; bytes += size; } catch { writeErrors++; omitted++; enabled = false; }
  }, stats() { return { records, bytes, omitted, writeErrors, maxRecords, maxBytes }; } };
}

async function run() {
  if (process.platform !== 'win32' || process.env.GITHUB_ACTIONS !== 'true'
    || process.env.GITHUB_EVENT_NAME !== 'workflow_dispatch' || process.env.GITHUB_REF !== BRANCH) {
    throw new Error('Observer runs only in the exact manual Windows diagnostic ref');
  }
  const driver = path.resolve(__dirname, 'public-native-smoke.cjs');
  const bytes = fs.readFileSync(driver);
  const blob = crypto.createHash('sha1').update(Buffer.from(`blob ${bytes.length}\0`)).update(bytes).digest('hex');
  if (blob !== DRIVER_BLOB) throw new Error('Canonical native assertions/actions changed');
  const output = path.join(process.env.RUNNER_TEMP, 'demostrike-native-context-observer');
  fs.mkdirSync(output, { recursive: true });
  const writer = boundedWriter(path.join(output, 'protocol-metadata.jsonl'));
  const observer = new ProtocolObserver((entry) => writer.emit(entry));
  let stderrTail = ''; let versionRecorded = false; let versionBusy = false;
  let samplerResult; let samplerClosed = false;
  const child = spawn(process.execPath, [driver], { stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, DEBUG: 'pw:api,pw:protocol', DEBUG_COLORS: '0' } });
  writer.emit({ kind: 'driver-start', driverPid: child.pid, canonicalBlob: blob, node: process.version });
  const sampler = spawn('pwsh.exe', ['-NoProfile', '-NonInteractive', '-File',
    path.resolve(__dirname, 'observe-native-processes.ps1'), '-RootPid', String(child.pid), '-OwnerPid', String(process.pid),
    '-OutputFile', path.join(output, 'owned-processes.jsonl'), '-StopFile', path.join(output, 'sampler.stop')],
  { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
  sampler.stderr.on('data', () => writer.emit({ kind: 'sampler-error-output-omitted' }));
  sampler.on('error', (error) => { samplerResult = { startError: errorCode(error.code) };
    writer.emit({ kind: 'sampler-start-error', ...samplerResult }); });
  sampler.on('close', (code, signal) => { samplerClosed = true; samplerResult = { ...samplerResult, code, signal };
    writer.emit({ kind: 'sampler-close', ...samplerResult }); });
  child.stdout.pipe(process.stdout); // Preserve canonical capture output and errors.
  const lines = new BoundedLines((line) => observer.accept(line), (prefix, characters) => observer.oversized(prefix, characters));
  const tailDecoder = new StringDecoder('utf8');
  const splitter = new StderrSplitter((bytes) => {
    process.stderr.write(bytes); stderrTail = (stderrTail + tailDecoder.write(bytes)).slice(-32768);
  }, (bytes) => lines.write(bytes), () => lines.end());
  child.stderr.on('data', (chunk) => splitter.write(chunk));
  const versionTimer = setInterval(async () => {
    if (versionRecorded || versionBusy) return;
    versionBusy = true;
    try {
      const response = await fetch('http://127.0.0.1:19375/json/version', { signal: AbortSignal.timeout(1000) });
      if (!response.ok) return;
      const version = await response.json();
      if (!versionValue(version.Browser)) return;
      writer.emit({ kind: 'native-debug-version', browser: versionValue(version.Browser),
        protocol: versionValue(version['Protocol-Version']), javascript: versionValue(version['V8-Version']), webkit: versionValue(version['WebKit-Version']) });
      versionRecorded = true;
    } catch { /* The unchanged driver owns startup readiness and its timeout. */ }
    finally { versionBusy = false; }
  }, 2000);
  let result;
  try {
    result = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', (code, signal) => {
      canonicalExitCode = Number.isInteger(code) ? code : 1; process.exitCode = canonicalExitCode; resolve({ code, signal });
    }); });
  } finally {
    clearInterval(versionTimer); splitter.end(); stderrTail = (stderrTail + tailDecoder.end()).slice(-32768);
    const save = (name, value) => { try { fs.writeFileSync(path.join(output, name), value); }
      catch (error) { writer.emit({ kind: 'observer-write-unavailable', fileRole: name, error: safeError(error) }); } };
    save('sampler.stop', 'stop\n');
    if (!samplerClosed) await new Promise((resolve) => { const timer = setTimeout(() => { sampler.kill(); resolve(); }, 2000); sampler.once('close', () => { clearTimeout(timer); resolve(); }); });
    writer.emit({ kind: 'driver-close', ...result });
    save('driver-stderr-tail.txt', stderrTail);
    save('observer-summary.json', JSON.stringify({
      canonicalBlob: blob, driverResult: result || { error: 'driver-start-or-observer-failure' },
      versionRecorded, samplerResult: samplerResult || { status: 'close-not-observed' },
      trace: writer.stats(), protocolCounters: observer.counts,
      unresolved: [...observer.pending.values()].slice(0, 128).map(({ sentAt, ...entry }) => ({ ...entry, pendingMs: Date.now() - sentAt })),
      scope: 'Observation only; canonical actions/assertions/timeouts and native app remain unchanged',
    }, null, 2) + '\n');
  }
  process.exitCode = Number.isInteger(result.code) ? result.code : 1;
}

module.exports = { ProtocolObserver, BoundedLines, StderrSplitter, boundedWriter, safeRoute, safeError, opaque, version: versionValue };
if (require.main === module) run().catch((error) => { console.error('Diagnostic observer failure', safeError(error)); process.exitCode = canonicalExitCode ?? 1; });
