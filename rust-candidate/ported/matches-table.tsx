import React, { useState } from 'react';
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
export function MatchesTable({ onOpenLibrary }: { onOpenLibrary: (path: string) => void }) {
  const [path, setPath] = useState('');
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
          {!hasActiveFilter && !search && (
            <div className="flex w-full flex-col gap-y-8 border-t border-gray-300 pt-16 text-left">
              <label htmlFor="local-library-path" className="text-body-strong">Open an existing DemoStrike SQLite library</label>
              <p className="text-gray-700">Paste the path to a library file to inspect its matches, rounds, players, and utility in read-only mode.</p>
              <input id="local-library-path" className="w-full rounded-4 border border-gray-400 bg-gray-75 p-8 select-text"
                value={path} onChange={(event) => setPath(event.target.value)} placeholder="C:\\path\\to\\matches.sqlite" />
              <div><Button isDisabled={!path.trim()} onClick={() => onOpenLibrary(path.trim())}>Open local library</Button></div>
            </div>
          )}
        </div>
      </CenteredContent>
    );
  }

  return <Table table={table} />;
}
