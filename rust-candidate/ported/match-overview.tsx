import React from 'react';
import { Content } from 'csdm/ui/components/content';
import { durationSeconds } from './match-data';
import { useSourceMatch } from './match-loader';

export function MatchOverview() {
  const { summary } = useSourceMatch();
  const duration = durationSeconds(summary);
  const imported = new Date(summary.importedAt);
  return <Content>
    <div className="flex flex-wrap items-start justify-between gap-16 border-b border-gray-300 pb-16">
      <div>
        <h1 className="text-title">{summary.mapName}</h1>
        <p className="mt-4 text-gray-700 select-text">{summary.fileName}</p>
      </div>
      <div className="flex gap-x-16 text-subtitle">
        <div><span className="text-gray-700">CT</span> {summary.scoreCt}</div>
        <div><span className="text-gray-700">T</span> {summary.scoreT}</div>
      </div>
    </div>
    <div className="mt-16 grid grid-cols-1 gap-16 sm:grid-cols-2 lg:grid-cols-3">
      <div><h2 className="text-body-strong">Recorded rounds</h2><p>{summary.roundCount}</p></div>
      <div><h2 className="text-body-strong">Recorded players</h2><p>{summary.playerCount}</p></div>
      <div><h2 className="text-body-strong">Utility events</h2><p>{summary.utilityCount}</p></div>
      <div><h2 className="text-body-strong">Tick rate</h2><p>{summary.tickRate ?? 'Unavailable'}</p></div>
      <div><h2 className="text-body-strong">Duration</h2><p>{duration === null ? 'Unavailable' : `${Math.floor(duration / 60)}m ${duration % 60}s`}</p></div>
      <div><h2 className="text-body-strong">Imported</h2><p>{Number.isNaN(imported.getTime()) ? summary.importedAt : imported.toLocaleString()}</p></div>
    </div>
    <div className="mt-24 rounded-8 border border-gray-300 p-16">
      <h2 className="text-subtitle">Source and coverage</h2>
      <p className="mt-8 text-gray-700">This overview shows fields recorded by the DemoStrike import. Team identities, kills, assists, ratings, and economy are unavailable in this library.</p>
      <dl className="mt-12 grid grid-cols-1 gap-8 sm:grid-cols-2">
        <div><dt className="text-body-strong">Parser</dt><dd>{summary.parserCapability}</dd></div>
        <div><dt className="text-body-strong">Source checksum</dt><dd className="break-all select-text">{summary.sourceSha256 ?? 'Unavailable'}</dd></div>
        <div className="sm:col-span-2"><dt className="text-body-strong">Demo path</dt><dd className="break-all select-text">{summary.demoFilePath ?? 'Unavailable'}</dd></div>
      </dl>
    </div>
  </Content>;
}
