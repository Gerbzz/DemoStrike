import { invoke } from '@tauri-apps/api/core';

export type ScoreRelation = 'tied' | 'leading' | 'trailing';

export type MatchSummary = {
  id: number;
  fileName: string;
  mapName: string;
  scoreCt: number;
  scoreT: number;
  scoreCtRelation: ScoreRelation;
  scoreTRelation: ScoreRelation;
  importedAt: string;
  parserCapability: string;
  playerCount: number;
  roundCount: number;
  utilityCount: number;
};

export type LibrarySnapshot = { path: string; matches: MatchSummary[] };
export type RoundSummary = { roundNumber: number; winner: string | null; startTick: number | null; endTick: number | null };
export type PlayerSummary = { steamid: string; name: string };
export type UtilityEvent = {
  id: number;
  roundNumber: number;
  utilityType: string;
  throwerName: string;
  throwerTeam: string;
  throwTick: number;
  enemiesBlinded: number | null;
  damageEnemy: number | null;
};
export type UtilityCount = { utilityType: string; count: number };
export type MatchDetail = {
  summary: MatchSummary;
  rounds: RoundSummary[];
  players: PlayerSummary[];
  utilityEvents: UtilityEvent[];
  utilityBreakdown: UtilityCount[];
};

export interface LibraryBridge {
  listMatches(path: string): Promise<LibrarySnapshot>;
  getMatchDetail(path: string, matchId: number): Promise<MatchDetail>;
}

// This is the first typed Tauri boundary. It does not emulate upstream window.csdm
// or its UUID-correlated WebSocket protocol, which are separate migration tracks.
export const libraryBridge: LibraryBridge = {
  listMatches: (path) => invoke<LibrarySnapshot>('list_matches', { path }),
  getMatchDetail: (path, matchId) => invoke<MatchDetail>('get_match_detail', { path, matchId }),
};
