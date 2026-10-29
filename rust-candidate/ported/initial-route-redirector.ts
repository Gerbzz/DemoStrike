// Adapted from CS Demo Manager initial-route-redirector.tsx; MIT license retained in LICENSE.upstream.
import { useEffect, useRef } from 'react';
import { useNavigate } from 'react-router';
import { useUiSettings } from 'csdm/ui/settings/ui/use-ui-settings';
import { useArgument } from 'csdm/ui/bootstrap/use-argument';
import { ArgumentName } from 'csdm/common/argument/argument-name';
import { RoutePath, buildPendingDownloadPath } from 'csdm/ui/routes-paths';
import { StartPath } from 'csdm/common/argument/start-path';
import { Page } from 'csdm/node/settings/page';
import { useSettingsOverlay } from 'csdm/ui/settings/use-settings-overlay';

export function InitialRouteRedirector() {
  const { initialPage: defaultPage } = useUiSettings();
  const { openSettings } = useSettingsOverlay();
  const startPathArgument = useArgument(ArgumentName.StartPath);
  const navigate = useNavigate();
  const hasRedirected = useRef(false);

  useEffect(() => {
    if (hasRedirected.current) {
      return;
    }

    if (startPathArgument === StartPath.Settings) {
      openSettings();
    }

    let to: string = RoutePath.Matches;
    if (startPathArgument && startPathArgument !== StartPath.Settings) {
      switch (startPathArgument) {
        case StartPath.Bans:
          to = RoutePath.Ban;
          break;
        case StartPath.Demos:
          to = RoutePath.Demos;
          break;
        case StartPath.Downloads:
          to = buildPendingDownloadPath();
          break;
        case StartPath.Matches:
          to = RoutePath.Matches;
          break;
        case StartPath.Players:
          to = RoutePath.Players;
          break;
        case StartPath.Search:
          to = RoutePath.Search;
          break;
        case StartPath.Teams:
          to = RoutePath.Teams;
          break;
      }
    } else {
      switch (defaultPage) {
        case Page.Demos:
          to = RoutePath.Demos;
          break;
        case Page.Download:
          to = RoutePath.Downloads;
          break;
        case Page.Players:
          to = RoutePath.Players;
          break;
        case Page.Teams:
          to = RoutePath.Teams;
          break;
      }
    }

    // Lock the initial decision before navigation yields. Clearing startup
    // arguments causes a rerender while a data-router navigation is pending.
    hasRedirected.current = true;
    void (async () => {
      try {
        await navigate(to, { replace: true });
      } catch (error) {
        hasRedirected.current = false;
        throw error;
      }
    })();
  }, [defaultPage, navigate, openSettings, startPathArgument]);

  return null;
}
