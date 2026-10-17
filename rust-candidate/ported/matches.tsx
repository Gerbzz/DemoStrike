import React, { useEffect, useState } from 'react';
import { Status } from 'csdm/common/types/status';
import { MatchesActionBar } from 'csdm/ui/matches/actionbar/action-bar';
import { MatchesTableStatusBar } from 'csdm/ui/matches/table/matches-status-bar';
import { MatchesTableProvider } from 'csdm/ui/matches/matches-table-provider';
import { useFetchMatches } from 'csdm/ui/matches/use-fetch-matches';
import { useMatchesStatus } from 'csdm/ui/matches/use-matches-status';
import { MatchesTable } from './matches-table';
import { LocalLibrary } from './local-library';

// Preserve the original Matches route and controls for its own library. A
// separate read-only view opens existing DemoStrike SQLite data without
// fabricating the fields required by the upstream PostgreSQL MatchTable.
export function Matches() {
  const [localPath, setLocalPath] = useState<string | null>(null);
  const fetchMatches = useFetchMatches();
  const status = useMatchesStatus();

  useEffect(() => {
    if (status === Status.Idle) void fetchMatches();
  });

  if (localPath) return <LocalLibrary path={localPath} onBack={() => setLocalPath(null)} />;

  return (
    <MatchesTableProvider>
      <MatchesActionBar />
      <MatchesTable onOpenLibrary={setLocalPath} />
      <MatchesTableStatusBar />
    </MatchesTableProvider>
  );
}
