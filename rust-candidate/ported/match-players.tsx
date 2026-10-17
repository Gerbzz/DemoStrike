import React from 'react';
import { Link, useParams } from 'react-router';
import { Content } from 'csdm/ui/components/content';
import { useSourceMatch } from './match-loader';

export function MatchPlayers() {
  const { players, fullCombatCoverage } = useSourceMatch();
  const { steamId } = useParams();
  const selected = steamId === undefined ? null : players.find((player) => player.steamid === steamId);
  if (steamId !== undefined && !selected) return <Content><p role="alert">This player is not recorded in the selected match.</p></Content>;
  return <Content>
    <h1 className="text-title">{selected ? selected.name : 'Recorded players'}</h1>
    <p className="mt-4 text-gray-700">{fullCombatCoverage
      ? 'Combat rows show observed CT/T sides. Damage sums raw dmg_health events and can include overkill; it is not effective HP removed.'
      : 'Player identities are recorded. Individual combat statistics are unavailable in this library.'}</p>
    {players.length === 0 ? <p className="mt-16">No players were recorded for this match.</p> :
      <div className="mt-16 overflow-auto rounded-8 border border-gray-300">
        <table className="w-full text-left"><thead className="bg-gray-100"><tr>
          <th className="p-8">Player</th><th className="p-8">Steam ID</th>
          <th className="p-8 text-right">Enemy kills</th><th className="p-8 text-right">Assists</th>
          <th className="p-8 text-right">Deaths</th><th className="p-8 text-right">Raw enemy damage</th>
        </tr></thead><tbody>{(selected ? [selected] : players).map((player) => <tr key={player.steamid} className="border-t border-gray-300">
          <td className="p-8">{selected ? player.name : <Link className="text-blue-600 underline" to={encodeURIComponent(player.steamid)}>{player.name}</Link>}</td>
          <td className="p-8 select-text">{player.steamid}</td>
          <td className="p-8 text-right tabular-nums">{player.combat?.enemyKills ?? '—'}</td>
          <td className="p-8 text-right tabular-nums">{player.combat?.enemyAssists ?? '—'}</td>
          <td className="p-8 text-right tabular-nums">{player.combat?.deaths ?? '—'}</td>
          <td className="p-8 text-right tabular-nums">{player.combat?.enemyHealthDamage ?? '—'}</td>
        </tr>)}</tbody></table>
      </div>}
    {selected?.combat && <section className="mt-24" aria-label="Player rounds">
      <h2 className="text-subtitle">Round by round</h2>
      <div className="mt-12 overflow-auto rounded-8 border border-gray-300">
        <table className="w-full text-left"><thead className="bg-gray-100"><tr>
          <th className="p-8">Round</th><th className="p-8">Observed side</th>
          <th className="p-8 text-right">Enemy kills</th><th className="p-8 text-right">Assists</th>
          <th className="p-8 text-right">Deaths</th><th className="p-8 text-right">Raw enemy damage</th>
        </tr></thead><tbody>{selected.combat.rounds.map((round) => <tr key={round.roundNumber} className="border-t border-gray-300">
          <td className="p-8">{round.roundNumber}</td><td className="p-8">{round.side ?? 'Unavailable'}</td>
          <td className="p-8 text-right tabular-nums">{round.enemyKills ?? '—'}</td>
          <td className="p-8 text-right tabular-nums">{round.enemyAssists ?? '—'}</td>
          <td className="p-8 text-right tabular-nums">{round.deaths}</td>
          <td className="p-8 text-right tabular-nums">{round.enemyHealthDamage ?? '—'}</td>
        </tr>)}</tbody></table>
      </div>
    </section>}
    {selected && <Link to=".." relative="path" className="mt-16 text-blue-600 underline">All players</Link>}
  </Content>;
}

export const MatchPlayersLoader = MatchPlayers;
