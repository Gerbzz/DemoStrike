# Pinned source inventory

Scope: standalone, pure Rust port of the `CommandSystem` scheduling core at
`advancedfx/advancedfx` commit `7567c24ec2501a7b07cda6213ece4540e57b5039`.

| Upstream file | Relevant behavior | Dependency boundary |
| --- | --- | --- |
| [`shared/CommandSystem.cpp`](https://github.com/advancedfx/advancedfx/blob/7567c24ec2501a7b07cda6213ece4540e57b5039/shared/CommandSystem.cpp) | `AddAtTick`, `AddAtTime`, `EditStart`, `Remove`, `Clear`, `OnExecuteCommands`, `OnLevelInitPreEntity` | Also contains console parsing, RapidXML persistence, interpolation formatting. Those are outside this scheduler crate. |
| [`shared/CommandSystem.h`](https://github.com/advancedfx/advancedfx/blob/7567c24ec2501a7b07cda6213ece4540e57b5039/shared/CommandSystem.h) | interval comparator, insertion, overlap predicate, tree traversal, cursor state | Includes `AfxConsole.h` and `AfxMath.h`; this crate does not import either. |
| [`LICENSE`](https://github.com/advancedfx/advancedfx/blob/7567c24ec2501a7b07cda6213ece4540e57b5039/LICENSE) | MIT, copyright (c) 2021 advancedfx.org | Attribution retained in `LICENSE-ADVANCEDFX`. |
| [`.gitmodules`](https://github.com/advancedfx/advancedfx/blob/7567c24ec2501a7b07cda6213ece4540e57b5039/.gitmodules) | Lists `deps/release/prop` (`advancedfx-prop`), RapidXML, and others as submodules | The repo README says MIT does not cover submodules. No submodule content is copied here. |

The source's `OnExecuteCommands` collects tick matches first, then time matches,
and only then drains its command queue. `GetTick` and `GetTime` return `float`,
which is promoted to `double` for comparisons. A tick sample of zero leaves
`m_LastTick` unchanged; time zero is processed normally. Both cursors initialize
to `-1` and reset to `-1` on level init. Disabled frames still update cursors.

The interval tree follows just **one** child per visited node: left if its
`Max >= query.Low`, otherwise right. This can omit qualifying commands in the
other subtree. The Rust port preserves that traversal for differential fidelity;
consumers should not interpret the scheduler as an exhaustive interval search.
After removal or start shifting invalidates a tree, a subsequent add inserts
only the new node until another invalidation; older mapped commands may also
be omitted. The port preserves this pinned behavior.

No game process, hook, injection, command runner, network service, or executable
download is included.
