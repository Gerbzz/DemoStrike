import React from 'react';
import { Link, useParams } from 'react-router';
import { Content } from 'csdm/ui/components/content';
import { useSourceMatch } from './match-loader';

export function MatchPlayers() {
  const { players } = useSourceMatch();
  const { steamId } = useParams();
  const selected = steamId === undefined ? null : players.find((player) => player.steamid === steamId);
  if (steamId !== undefined && !selected) return <Content><p role="alert">This player is not recorded in the selected match.</p></Content>;
  return <Content>
    <h1 className="text-title">{selected ? selected.name : 'Recorded players'}</h1>
    <p className="mt-4 text-gray-700">Player identities are recorded. Individual combat statistics are unavailable in this library.</p>
    {players.length === 0 ? <p className="mt-16">No players were recorded for this match.</p> :
      <div className="mt-16 overflow-auto rounded-8 border border-gray-300">
        <table className="w-full text-left"><thead className="bg-gray-100"><tr>
          <th className="p-8">Player</th><th className="p-8">Steam ID</th>
        </tr></thead><tbody>{(selected ? [selected] : players).map((player) => <tr key={player.steamid} className="border-t border-gray-300">
          <td className="p-8">{selected ? player.name : <Link className="text-blue-600 underline" to={encodeURIComponent(player.steamid)}>{player.name}</Link>}</td>
          <td className="p-8 select-text">{player.steamid}</td>
        </tr>)}</tbody></table>
      </div>}
    {selected && <Link to=".." relative="path" className="mt-16 text-blue-600 underline">All players</Link>}
  </Content>;
}

export const MatchPlayersLoader = MatchPlayers;
