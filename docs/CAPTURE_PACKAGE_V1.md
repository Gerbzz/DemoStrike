# Capture package v1 (local prototype)

DemoStrike's `demostrike-cli package <draft.json> <new-folder>` requires each
MP4 to have matching video and audio stream headers, then creates a folder
with `manifest.json` and one file under `clips/` per take. The separate
`package-structural` command is for metadata/file-handoff experiments only;
its output is labeled `structural` and does not establish valid media.
ClipStrike's `clipstrike-import <package-folder> <new-project.cstrike>` creates
an editable project with separate assets and timeline clips. Both commands
reject existing output paths. The `.cstrike` file links to the package media;
keep the package folder available after import. It stores the package location
relative to the project where possible, so moving both together works. If
the package moves separately, ClipStrike marks it offline until relinked.

Each draft input supplies an existing MP4 `source_path` and a `clip` record.
The package writer computes its relative path and SHA-256. The other clip
fields are:

| Provenance | Media/QC |
| --- | --- |
| `clip_id`, `demo_sha256`, optional `demo_path_hint`, `event_id`, `player_steamid64`, `map`, `round`, `utility_type`, `start_tick`, `end_tick`, `camera_role`, `group_order` | `duration_ms`, `width`, `height`, `fps_num`, `fps_den`, `audio`, `qc_status` |

`audio` has `source`, `channels`, `sample_rate`, and `verified`. Version 1
accepts `source: "hlae_wav"`, `verified: true`, a positive channel count and
sample rate, and `qc_status: "accepted"`. Those are assertions supplied by
the capture process. Current code checks the metadata and file hash but does
**not** decode MP4, inspect WAV samples, listen for sound, or establish that
the video really comes from a given demo. Do not mark test fixtures as
production-accepted takes.

The manifest's `media_validation` is `structural` or `stream_probed`. The
normal DemoStrike package command probes source and copied MP4 streams;
ClipStrike records that reported level but does not independently probe
content. Stream headers do not prove audible sound, HLAE origin, a complete
decode, or editorial acceptance. The existing HLAE capture route provides
separate video MP4 and WAV files; the MP4 alone fails the strict package
command. A temporary mux is needed until capture/audio assembly is integrated.

The package writer uses a sibling `.partial` stage. On cancellation, rerun
the same draft to resume; a partially copied clip is discarded and recopied
without altering its source. A complete stage with changed draft metadata is
refused during automatic resume. If a complete stage survived interruption, use
`demostrike-cli recover <new-folder>`. It never deletes or overwrites an
unrecognized destination. The importer rejects unknown schema versions,
path escape, missing or changed files, reused clip IDs/paths/order, mixed
resolution or frame rate, and fractional FPS that its millisecond timeline
cannot yet represent exactly. The exact source rational FPS stays in the
capture manifest. Accepting an integer rate such as 60/1 is only a temporary
compatibility gate: a millisecond timeline cannot place every 60 fps frame
boundary exactly (one frame is 16.666... ms). No frame-exact editing claim is
made for 60/1. Projects save with a temporary file and keep the previous
version as `.cstrike.bak`; `Project::load_backup` can reopen it. This is a
recovery foundation, not a passed power-loss or installer gate.
