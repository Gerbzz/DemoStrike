# Design direction

The work surface is a dense match analysis application used for scanning demos and moving into detailed evidence. It follows the upstream CS Demo Manager Matches → Match Overview hierarchy while introducing a softer VS Code Dark Modern-like palette. The left rail handles destinations; the match list is a compact table; selected match information occupies the main canvas. No workspace brand logo is shown. Branding can appear on loading and About surfaces after those flows are ported.

Tokens: canvas `#1f1f1f`, rail `#252526`, raised controls `#2d2d30`, border `#3c3c3c`, foreground `#d4d4d4`, secondary `#aeb4bb`, accent `#64a9f5`, CT `#74aef6`, T `#efb46a`. Spacing uses 4, 8, 12, 16, 24, 32px. Controls have visible focus, hover, disabled, empty, loading, and error treatments. Numerals use tabular figures. At narrower widths the list and details stack; the destination rail collapses to a compact top bar.

Visual acceptance requires a fresh capture of the actual Tauri application with a real import at the current source revision. This session cannot make that claim because local builds and executable downloads are disallowed.
