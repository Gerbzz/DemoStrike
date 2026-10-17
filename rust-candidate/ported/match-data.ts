export type MatchSummary = {
  [key: string]: unknown;
  id: number;
  fileName: string;
  demoFilePath: string | null;
  mapName: string;
  scoreCt: number;
  scoreT: number;
  importedAt: string;
  parserCapability: string;
  sourceSha256: string | null;
  tickRate: number | null;
  durationTicks: number | null;
  playerCount: number;
  roundCount: number;
  utilityCount: number;
};

export type LibrarySnapshot = { path: string; matches: MatchSummary[] };
export type MatchDetail = {
  summary: MatchSummary;
  rounds: { roundNumber: number; winner: string | null; startTick: number | null; endTick: number | null }[];
  players: { steamid: string; name: string }[];
  utilityEvents: {
    id: number; roundNumber: number; utilityType: string; throwerName: string;
    throwerTeam: string; throwTick: number; enemiesBlinded: number | null;
    damageEnemy: number | null;
  }[];
};

export const libraryPathKey = 'csdm-port-selected-library';
export const matchRouteKey = (id: number) => `local-${id}`;
export function matchIdFromRoute(key: string | undefined) {
  if (!key || !/^local-[1-9]\d*$/.test(key)) return null;
  const id = Number(key.slice(6));
  return Number.isSafeInteger(id) ? id : null;
}

export function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const native = (window as any).__TAURI_INTERNALS__ as
    { invoke: <R>(name: string, input?: Record<string, unknown>) => Promise<R> } | undefined;
  return native ? native.invoke<T>(command, args) : Promise.reject(new Error('The Tauri host is unavailable.'));
}

export function describe(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function durationSeconds(summary: MatchSummary) {
  const rate = summary.tickRate;
  const ticks = summary.durationTicks;
  return rate !== null && rate > 0 && ticks !== null && ticks >= 0
    ? Math.round(ticks / rate) : null;
}
