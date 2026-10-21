// Keep CSDM's original credits, version information and controls visible.
import React from 'react';
import { About as OriginalAbout } from '../upstream/cs-demo-manager/src/ui/settings/about/about';
import mark from '../../assets/branding/DemoStrike-geometric.svg?url';

export function About() {
  return (
    <>
      <div className="flex items-center gap-x-8 p-16">
        <img src={mark} alt="DemoStrike" width={48} height={48} />
        <p>DemoStrike</p>
      </div>
      <OriginalAbout />
    </>
  );
}
