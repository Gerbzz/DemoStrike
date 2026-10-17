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
export type RoundDeath = {
  sourceIndex: number;
  eventOrder: number | null;
  tick: number;
  weapon: string | null;
  attackerSteamid: string | null;
  attackerSide: string | null;
  victimSteamid: string;
  victimSide: string | null;
  assisterSteamid: string | null;
};

export type RoundHurt = {
  sourceIndex: number;
  eventOrder: number | null;
  tick: number;
  weapon: string | null;
  attackerSteamid: string | null;
  attackerSide: string | null;
  victimSteamid: string;
  victimSide: string | null;
  rawDamage: number;
};

export type RecordedRound = {
  roundNumber: number;
  winner: string | null;
  startTick: number | null;
  freezeEndTick: number | null;
  endTick: number | null;
  endReason: string | null;
  ctEquipValue: number | null;
  tEquipValue: number | null;
  combat: { ctPlayers: number; tPlayers: number; unassignedPlayers: number; enemyKills: number | null } | null;
  deaths: RoundDeath[] | null;
  hurts: RoundHurt[] | null;
};

export type MatchDetail = {
  summary: MatchSummary;
  fullCombatCoverage: boolean;
  rounds: RecordedRound[];
  players: { steamid: string; name: string; combat: {
    enemyKills: number | null; deaths: number; enemyAssists: number | null;
    enemyHealthDamage: number | null;
    rounds: { roundNumber: number; side: string | null; enemyKills: number | null;
      deaths: number; enemyAssists: number | null; enemyHealthDamage: number | null }[];
  } | null }[];
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
  return rate !== null && Number.isFinite(rate) && rate > 0 && ticks !== null && Number.isFinite(ticks) && ticks >= 0
    ? Math.round(ticks / rate) : null;
}
