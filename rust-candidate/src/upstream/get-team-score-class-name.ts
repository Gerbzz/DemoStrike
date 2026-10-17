// Source: akiver/cs-demo-manager@7270bc95c8cab87b7f9ddd490fea13c6eda3ace7
// src/ui/styles/get-team-score-class-name.ts (Git blob 5d646660a00517a4e1d71ce84be1839de4ad3fde)
// MIT, Copyright (c) 2014-present AkiVer. Full notice: ../../LICENSE.upstream.
export function getTeamScoreClassName(score: number, oppositeScore: number) {
  if (score === oppositeScore) {
    return 'text-blue-400';
  }

  return score > oppositeScore ? 'text-green-400' : 'text-red-400';
}
