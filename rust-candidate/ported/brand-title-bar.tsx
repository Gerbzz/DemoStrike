// Branding adapter for the preserved CSDM title bar. Keep its native controls,
// database state and history behavior; the pinned upstream source stays intact.
import React from 'react';
import { DatabaseStatus } from 'csdm/ui/bootstrap/database-status';
import { useDatabaseStatus } from 'csdm/ui/bootstrap/use-database-status';
import { HistoryNavigation } from 'csdm/ui/title-bar/history/history-navigation';
import { MenuButton } from 'csdm/ui/title-bar/menu-button';
import { WindowControls } from 'csdm/ui/title-bar/window-controls/window-controls';
import { UpdateAvailableButton } from 'csdm/ui/title-bar/update-available-button';
import mark from '../../assets/branding/DemoStrike-geometric.svg?url';

export function TitleBar() {
  const databaseStatus = useDatabaseStatus();
  const onDoubleClick = async () => {
    if (!window.csdm.isMac) return;
    const isWindowMaximized = await window.csdm.isWindowMaximized();
    if (isWindowMaximized) window.csdm.unMaximizeWindow();
    else window.csdm.maximizeWindow();
  };

  return (
    <div onDoubleClick={onDoubleClick}
      className="relative z-10 flex h-(--title-bar-height) items-center overflow-hidden border-b border-b-gray-300 bg-gray-50 text-gray-900 drag">
      {!window.csdm.isMac && <MenuButton />}
      <div className="mx-auto flex items-center gap-x-16">
        {databaseStatus === DatabaseStatus.Connected && <HistoryNavigation />}
        <div className="flex items-center gap-x-8">
          <img data-testid="demostrike-mark" src={mark} alt="DemoStrike" width={24} height={24} />
          <p>DemoStrike</p>
        </div>
        <UpdateAvailableButton />
      </div>
      {!window.csdm.isMac && <WindowControls />}
    </div>
  );
}
