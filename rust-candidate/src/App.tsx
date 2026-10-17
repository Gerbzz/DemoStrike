import React, { useCallback, useMemo, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { TeamScore } from './upstream/TeamScore';
import {
  libraryBridge,
  type LibrarySnapshot,
  type MatchDetail,
  type MatchSummary,
  type UtilityEvent,
} from './bridge';

type Page = 'matches' | 'about';
type DetailTab = 'overview' | 'rounds' | 'players' | 'utility';
type LoadState = 'idle' | 'loading' | 'ready' | 'error';

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function formatWinner(winner: string | null): string {
  return winner ?? 'Unknown';
}

function MatchScore({ match }: { match: MatchSummary }) {
  return <div className="score" role="group" aria-label={`CT ${match.scoreCt}, T ${match.scoreT}`}>
    <TeamScore teamName="CT" teamScore={match.scoreCt} scoreOppositeTeam={match.scoreT} relation={match.scoreCtRelation} />
    <span className="score-divider" aria-hidden="true">:</span>
    <TeamScore teamName="T" teamScore={match.scoreT} scoreOppositeTeam={match.scoreCt} relation={match.scoreTRelation} />
  </div>;
}

function MatchesTable({ matches, selectedId, onSelect }: {
  matches: MatchSummary[];
  selectedId: number | null;
  onSelect: (id: number) => void;
}) {
  return (
    <div className="table-scroll">
      <table className="data-table matches-table">
        <thead><tr><th scope="col">Demo</th><th scope="col">Map</th><th scope="col">Score</th><th scope="col" className="numeric">Rounds</th><th scope="col" className="numeric">Utility</th><th scope="col">Imported</th></tr></thead>
        <tbody>
          {matches.map((match) => (
            <tr key={match.id} className={selectedId === match.id ? 'selected' : ''}>
              <td><button type="button" className="row-link" onClick={() => onSelect(match.id)} aria-current={selectedId === match.id ? 'true' : undefined}>{match.fileName}</button></td>
              <td>{match.mapName}</td><td><MatchScore match={match} /></td>
              <td className="numeric">{match.roundCount}</td><td className="numeric">{match.utilityCount}</td>
              <td className="muted">{formatDate(match.importedAt)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function EmptyState({ title, description, action }: { title: string; description: string; action?: React.ReactNode }) {
  return <div className="empty-state"><h2>{title}</h2><p>{description}</p>{action}</div>;
}

function Overview({ detail }: { detail: MatchDetail }) {
  const unknownRounds = detail.rounds.filter((round) => round.winner === null).length;
  return (
    <div className="overview-grid">
      <section className="overview-main" aria-labelledby="match-summary-heading">
        <h3 id="match-summary-heading">Match summary</h3>
        <dl className="summary-list">
          <div><dt>Demo file</dt><dd>{detail.summary.fileName}</dd></div>
          <div><dt>Map</dt><dd>{detail.summary.mapName}</dd></div>
          <div><dt>Recorded score</dt><dd><MatchScore match={detail.summary} /></dd></div>
          <div><dt>Players</dt><dd>{detail.summary.playerCount}</dd></div>
          <div><dt>Round rows</dt><dd>{detail.summary.roundCount}{unknownRounds > 0 ? ` · ${unknownRounds} without a winner` : ''}</dd></div>
          <div><dt>Parser</dt><dd>{detail.summary.parserCapability}</dd></div>
        </dl>
      </section>
      <section aria-labelledby="utility-heading">
        <h3 id="utility-heading">Utility recorded</h3>
        <div className="metric-total">{detail.summary.utilityCount}<span> events</span></div>
        <ul className="breakdown-list">
          {detail.utilityBreakdown.map((item) => <li key={item.utilityType}><span>{item.utilityType}</span><strong>{item.count}</strong></li>)}
        </ul>
      </section>
    </div>
  );
}

function RoundsView({ detail }: { detail: MatchDetail }) {
  return <div className="table-scroll"><table className="data-table"><thead><tr><th>Round</th><th>Winner</th><th className="numeric">Start tick</th><th className="numeric">End tick</th></tr></thead><tbody>
    {detail.rounds.map((round) => <tr key={round.roundNumber}><td>{round.roundNumber}</td><td className={round.winner === null ? 'muted' : round.winner === 'CT' ? 'ct' : 't'}>{formatWinner(round.winner)}</td><td className="numeric">{round.startTick ?? '—'}</td><td className="numeric">{round.endTick ?? '—'}</td></tr>)}
  </tbody></table></div>;
}

function PlayersView({ detail }: { detail: MatchDetail }) {
  return <div className="table-scroll"><table className="data-table"><thead><tr><th>Player</th><th>Steam ID</th></tr></thead><tbody>
    {detail.players.map((player) => <tr key={player.steamid}><td>{player.name}</td><td className="muted identifier">{player.steamid}</td></tr>)}
  </tbody></table></div>;
}

function UtilityView({ detail }: { detail: MatchDetail }) {
  const [round, setRound] = useState('all');
  const filtered = useMemo(() => round === 'all' ? detail.utilityEvents : detail.utilityEvents.filter((event) => event.roundNumber === Number(round)), [detail, round]);
  return <>
    <div className="sub-toolbar"><label htmlFor="round-filter">Round</label><select id="round-filter" value={round} onChange={(event) => setRound(event.target.value)}><option value="all">All rounds</option>{detail.rounds.map((item) => <option key={item.roundNumber} value={item.roundNumber}>Round {item.roundNumber}</option>)}</select><span className="muted">{filtered.length} events</span></div>
    {filtered.length === 0 ? <EmptyState title="No utility in this round" description="Choose another round to view recorded throws." /> : <UtilityTable events={filtered} />}
  </>;
}

function UtilityTable({ events }: { events: UtilityEvent[] }) {
  return <div className="table-scroll"><table className="data-table"><thead><tr><th className="numeric">Round</th><th>Type</th><th>Thrower</th><th>Team</th><th className="numeric">Tick</th><th className="numeric">Enemies blinded</th><th className="numeric">Enemy damage</th></tr></thead><tbody>
    {events.map((event) => <tr key={event.id}><td className="numeric">{event.roundNumber}</td><td>{event.utilityType}</td><td>{event.throwerName}</td><td className={event.throwerTeam === 'CT' ? 'ct' : 't'}>{event.throwerTeam}</td><td className="numeric">{event.throwTick}</td><td className="numeric">{event.enemiesBlinded === null ? <span className="muted" title="Blind attribution is not verified for this event">Unavailable</span> : event.enemiesBlinded}</td><td className="numeric">{event.damageEnemy === null ? <span className="muted" title="Damage attribution is not verified for this event">Unavailable</span> : event.damageEnemy}</td></tr>)}
  </tbody></table></div>;
}

function MatchDetailPanel({ detail, state, error }: { detail: MatchDetail | null; state: LoadState; error: string }) {
  const [tab, setTab] = useState<DetailTab>('overview');
  if (state === 'loading') return <div className="detail-status" role="status">Loading match details…</div>;
  if (state === 'error') return <div className="detail-status error" role="alert"><h2>Could not open this match</h2><p>{error}</p></div>;
  if (!detail) return <EmptyState title="Select a match" description="Choose a demo from the library to inspect its rounds, players and utility events." />;
  return <div className="match-detail">
    <header className="detail-header"><div><span className="muted detail-overline">{detail.summary.fileName}</span><h2>{detail.summary.mapName}</h2></div><MatchScore match={detail.summary} /></header>
    <div className="tab-list" role="tablist" aria-label="Match details">
      {(['overview', 'rounds', 'players', 'utility'] as const).map((item) => <button key={item} type="button" role="tab" aria-selected={tab === item} className={tab === item ? 'tab active' : 'tab'} onClick={() => setTab(item)}>{item[0].toUpperCase() + item.slice(1)}</button>)}
    </div>
    <div className="tab-content" role="tabpanel">
      {tab === 'overview' && <Overview detail={detail} />}
      {tab === 'rounds' && <RoundsView detail={detail} />}
      {tab === 'players' && <PlayersView detail={detail} />}
      {tab === 'utility' && <UtilityView key={detail.summary.id} detail={detail} />}
    </div>
  </div>;
}

export function App() {
  const [page, setPage] = useState<Page>('matches');
  const [pathInput, setPathInput] = useState('');
  const [snapshot, setSnapshot] = useState<LibrarySnapshot | null>(null);
  const [libraryState, setLibraryState] = useState<LoadState>('idle');
  const [libraryError, setLibraryError] = useState('');
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detail, setDetail] = useState<MatchDetail | null>(null);
  const [detailState, setDetailState] = useState<LoadState>('idle');
  const [detailError, setDetailError] = useState('');
  const request = useRef(0);
  const libraryRequest = useRef(0);

  const selectMatch = useCallback(async (path: string, id: number) => {
    const serial = ++request.current;
    setSelectedId(id);
    setDetail(null);
    setDetailState('loading');
    try {
      const result = await libraryBridge.getMatchDetail(path, id);
      if (serial !== request.current) return;
      setDetail(result);
      setDetailState('ready');
    } catch (error) {
      if (serial !== request.current) return;
      setDetailError(errorText(error));
      setDetailState('error');
    }
  }, []);

  const openLibrary = useCallback(async (path: string) => {
    const trimmed = path.trim();
    if (!trimmed) { setLibraryError('Enter the path to an existing DemoStrike SQLite library.'); setLibraryState('error'); return; }
    const librarySerial = ++libraryRequest.current;
    ++request.current;
    setLibraryState('loading');
    setLibraryError('');
    setDetail(null);
    setDetailState('idle');
    try {
      const result = await libraryBridge.listMatches(trimmed);
      if (librarySerial !== libraryRequest.current) return;
      setSnapshot(result);
      setLibraryState('ready');
      const first = result.matches[0];
      if (first) void selectMatch(trimmed, first.id);
      else setSelectedId(null);
    } catch (error) {
      if (librarySerial !== libraryRequest.current) return;
      setSnapshot(null);
      setSelectedId(null);
      setLibraryError(errorText(error));
      setLibraryState('error');
    }
  }, [selectMatch]);

  const visibleMatches = useMemo(() => snapshot?.matches.filter((match) => `${match.fileName} ${match.mapName}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())) ?? [], [snapshot, query]);

  const browseLibrary = async () => {
    try {
      const selected = await open({ multiple: false, directory: false, filters: [{ name: 'SQLite library', extensions: ['sqlite', 'db'] }] });
      if (selected) {
        setPathInput(selected);
        await openLibrary(selected);
      }
    } catch (error) {
      setLibraryError(`Could not open the file picker: ${errorText(error)}`);
      setLibraryState('error');
    }
  };

  return <div className="app-shell">
    <nav className="destination-rail" aria-label="Workspace">
      <div className="rail-heading">Workspace</div>
      <button type="button" className={page === 'matches' ? 'destination active' : 'destination'} onClick={() => setPage('matches')}>Matches</button>
      <div className="rail-spacer" />
      <button type="button" className={page === 'about' ? 'destination active' : 'destination'} onClick={() => setPage('about')}>About</button>
    </nav>
    <main className="main-region">
      {page === 'about' ? <section className="about-page"><h1>DemoStrike Rust Candidate</h1><p>A separate migration study based on CS Demo Manager 3.20.1. This first slice reads existing DemoStrike imports and does not analyze demos, launch the game or render videos.</p><p>CS Demo Manager © 2014–present AkiVer, MIT licensed. Upstream source pinned at <code>7270bc95</code>.</p></section> : <>
        <header className="workspace-header"><div><h1>Matches</h1><p>Imported demos in a DemoStrike library</p></div><span className="read-only-tag">Read only</span></header>
        <form className="library-toolbar" onSubmit={(event) => { event.preventDefault(); void openLibrary(pathInput); }}>
          <label htmlFor="library-path">Library file</label>
          <input id="library-path" className="path-input" type="text" spellCheck={false} value={pathInput} onChange={(event) => setPathInput(event.target.value)} placeholder="C:\\path\\to\\library.sqlite" />
          <button type="button" disabled={libraryState === 'loading'} onClick={() => void browseLibrary()}>Browse…</button>
          <button type="submit" className="primary-button" disabled={libraryState === 'loading'}>Open library</button>
          <button type="button" disabled={!snapshot || libraryState === 'loading'} onClick={() => snapshot && void openLibrary(snapshot.path)}>Refresh</button>
        </form>
        {libraryState === 'error' && <div className="inline-error" role="alert"><strong>Could not open library</strong><span>{libraryError}</span></div>}
        {libraryState === 'loading' && <div className="loading-strip" role="status">Reading library…</div>}
        {libraryState === 'idle' && <EmptyState title="Open a match library" description="Enter the path to an existing DemoStrike SQLite import. The candidate reads it without changing the file." />}
        {libraryState === 'ready' && snapshot && (snapshot.matches.length === 0 ? <EmptyState title="No imported matches" description="This library is valid but contains no matches. Open another DemoStrike library or import a demo in the existing application." /> : <div className="workspace-grid">
          <section className="library-panel" aria-label="Match library"><div className="panel-heading"><h2>Library <span className="count">{snapshot.matches.length}</span></h2><input type="search" aria-label="Search matches" placeholder="Search demos or maps" value={query} onChange={(event) => setQuery(event.target.value)} /></div>
            {visibleMatches.length === 0 ? <EmptyState title="No matching demos" description="Try another file name or map." /> : <MatchesTable matches={visibleMatches} selectedId={selectedId} onSelect={(id) => void selectMatch(snapshot.path, id)} />}
          </section>
          <section className="detail-panel" aria-label="Selected match"><MatchDetailPanel key={selectedId ?? 'none'} detail={detail} state={detailState} error={detailError} /></section>
        </div>)}
      </>}
    </main>
  </div>;
}
