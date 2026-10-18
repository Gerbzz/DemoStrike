// Adapted from akiver/cs-demo-manager@7270bc95c8cab87b7f9ddd490fea13c6eda3ace7
// src/ui/components/match/team-score.tsx (Git blob 0946c16e78b66cfc9aeeccdee4e17c651650378d)
// MIT, Copyright (c) 2014-present AkiVer. Full notice: ../../LICENSE.upstream.
import type { ScoreRelation } from '../bridge';
import { getTeamScoreClassName } from './get-team-score-class-name';

type Props = {
  teamName: string;
  teamScore: number;
  scoreOppositeTeam: number;
  relation: ScoreRelation;
};

export function TeamScore({ teamName, teamScore, scoreOppositeTeam, relation }: Props) {
  return (
    <div className="team-score" aria-label={`${teamName}: ${teamScore}, ${relation}`}>
      <p className={`team-score-number ${getTeamScoreClassName(teamScore, scoreOppositeTeam)}`}>{teamScore}</p>
      <p className="team-score-name">{teamName}</p>
    </div>
  );
}
