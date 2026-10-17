import React from 'react';
import { Status } from 'csdm/common/types/status';
import { Button } from 'csdm/ui/components/buttons/button';
import { CenteredContent } from 'csdm/ui/components/content';
import { Table } from 'csdm/ui/components/table/table';
import { useMatchesStatus } from 'csdm/ui/matches/use-matches-status';
import { useMatchesTable } from 'csdm/ui/matches/table/use-matches-table';
import { useFetchMatches } from 'csdm/ui/matches/use-fetch-matches';
import { useActiveMatchesFilters } from 'csdm/ui/matches/use-active-matches-filters';
import { useFuzzySearchText } from 'csdm/ui/matches/use-fuzzy-search-text';

// Keep the original action bar, table, status bar, and route. This replaces
// only the messages that would otherwise claim an unported demo import works.
export function MatchesTable() {
  const table = useMatchesTable();
  const status = useMatchesStatus();
  const fetchMatches = useFetchMatches();
  const { hasActiveFilter } = useActiveMatchesFilters();
  const search = useFuzzySearchText();

  if (status === Status.Idle || status === Status.Loading || !table.isReady()) {
    return <CenteredContent><p className="text-subtitle">Loading matches…</p></CenteredContent>;
  }

  if (status === Status.Error) {
    return (
      <CenteredContent>
        <div className="flex max-w-[480px] flex-col items-center gap-y-12 text-center">
          <h1 className="text-subtitle">Could not load matches</h1>
          <p>The local library could not supply the full match fields required by this screen.</p>
          <Button onClick={() => void fetchMatches()}>Try again</Button>
        </div>
      </CenteredContent>
    );
  }

  if (table.getRowCount() === 0) {
    return (
      <CenteredContent>
        <div className="flex max-w-[520px] flex-col items-center gap-y-12 text-center">
          <h1 className="text-subtitle">{hasActiveFilter || search ? 'No matches found' : 'Your local library is empty'}</h1>
          <p>
            {hasActiveFilter || search
              ? 'Try a different search or filter.'
              : 'The original CS Demo Manager workspace is running in Tauri. Demo import and populated match data are still being ported.'}
          </p>
        </div>
      </CenteredContent>
    );
  }

  return <Table table={table} />;
}
