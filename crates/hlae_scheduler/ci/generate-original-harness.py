#!/usr/bin/env python3
"""Build a test translation unit from pinned, unmodified scheduler source slices.

The input files must be the two official advancedfx files at commit
7567c24ec2501a7b07cda6213ece4540e57b5039. This script copies selected
method bodies verbatim; it never translates the scheduler algorithm.
"""

import hashlib
import json
import sys
from pathlib import Path


def slice_once(source: str, start: str, end: str, label: str) -> str:
    if source.count(start) != 1 or source.count(end) != 1:
        raise ValueError(f"{label}: expected unique source anchors")
    first = source.index(start)
    last = source.index(end, first)
    if last <= first:
        raise ValueError(f"{label}: reversed source anchors")
    return source[first:last].rstrip() + "\n"


PRELUDE = r'''
// Generated from pinned advancedfx source by ci/generate-original-harness.py.
// Copyright (c) 2021 advancedfx.org. MIT: see LICENSE-ADVANCEDFX.
// Scheduling method bodies and tree code below are copied verbatim.
#include <cstddef>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iterator>
#include <map>
#include <queue>
#include <string>
#include <vector>

namespace advancedfx { inline void Warning(char const *) {} }

constexpr std::size_t _TRUNCATE = static_cast<std::size_t>(-1);
using std::atoi;
template <std::size_t N>
int _snprintf_s(char (&buffer)[N], std::size_t, char const *format, double value) {
    return std::snprintf(buffer, N, format, value);
}
'''


CLASS_BEGIN = r'''
class CommandSystem {
public:
    bool Enabled;
    CommandSystem(IExecuteClientCmdForCommandSystem *,
                  IGetTickForCommandSystem *, IGetTimeForCommandSystem *);
    ~CommandSystem() { Clear(); }
    void OnExecuteCommands(void);
    void OnLevelInitPreEntity(void);
    void Add(char const *);
    void AddTick(char const *);
    void AddAtTime(char const *, double);
    void AddAtTick(char const *, double);
    void EditStart(double);
    void EditStartTick(double);
    bool Remove(int);
    void Clear(void);
    bool IsSupportedByTime(void);
    bool IsSupportedByTick(void);

    // The original CCommand includes curve types from AfxMath. Plain commands
    // take the original DoCommand early-return path. Curves are inert stubs.
    struct CurveStub {
        bool CanEval() { return false; }
        double Eval(double) { return 0; }
    };
    class CCommand {
    public:
        void SetCommand(char const *value) { m_Command = value; }
        bool DoCommand(double, std::queue<std::string> &);
    private:
        bool m_Formated = false;
        std::string m_Command;
        std::vector<CurveStub> m_Interp;
    };
'''


CLASS_END = r'''
    // Test-only insertion seam for ranges normally loaded by XML or AddCurves.
    // It changes no scheduling decisions, but is not retained source code.
    void AddRange(bool tick, double low, double high, bool epsilon,
                  char const *command) {
        CCommand *cmd = new CCommand();
        cmd->SetCommand(command);
        Interval interval(low, high, epsilon);
        if (tick) {
            m_TickMap.insert({ interval, cmd });
            m_TickTree = Insert(m_TickTree, interval, cmd);
        } else {
            m_TimeMap.insert({ interval, cmd });
            m_TimeTree = Insert(m_TimeTree, interval, cmd);
        }
    }

    double m_LastTime;
    double m_LastTick;
    IExecuteClientCmdForCommandSystem *m_pExecuteClientCmd = nullptr;
    IGetTickForCommandSystem *m_pGetTick = nullptr;
    IGetTimeForCommandSystem *m_pGetTime = nullptr;
};
'''


DRIVER = r'''
// Everything below is the inert test driver, not an advancedfx method.
struct TickClock : IGetTickForCommandSystem {
    float value = 0;
    float GetTick() override { return value; }
};
struct TimeClock : IGetTimeForCommandSystem {
    float value = 0;
    float GetTime() override { return value; }
};
struct Sink : IExecuteClientCmdForCommandSystem {
    std::vector<std::string> seen;
    CommandSystem *scheduler = nullptr;
    bool mutate_once = false;
    void ExecuteClientCmd(char const *value) override {
        seen.emplace_back(value);
        if (mutate_once) {
            mutate_once = false;
            scheduler->Clear();
            scheduler->Enabled = false;
        }
    }
};
struct Fixture {
    Sink sink;
    TickClock tick;
    TimeClock time;
    CommandSystem scheduler;
    Fixture() : scheduler(&sink, &tick, &time) { sink.scheduler = &scheduler; }
    std::vector<std::string> frame(float tick_value, float time_value) {
        tick.value = tick_value;
        time.value = time_value;
        scheduler.OnExecuteCommands();
        auto result = sink.seen;
        sink.seen.clear();
        return result;
    }
};
static void emit(char const *label, std::vector<std::string> const &commands) {
    std::printf("%s|", label);
    for (std::size_t i = 0; i < commands.size(); ++i)
        std::printf("%s%s", i ? "," : "", commands[i].c_str());
    std::printf("\n");
}
int main() {
    {
        Fixture f;
        f.scheduler.AddAtTick("one", 1); f.scheduler.AddAtTime("time", 0);
        emit("initial", f.frame(1, 0));
        emit("tick_zero", f.frame(0, 0));
        emit("forward", f.frame(2, 1));
        emit("repeat", f.frame(2, 1));
        f.scheduler.OnLevelInitPreEntity();
        emit("reset", f.frame(2, 1));
    }
    {
        Fixture f;
        f.scheduler.AddAtTick("first", 3); f.scheduler.AddAtTick("second", 3);
        f.frame(3, 0);
        emit("same_tick", f.frame(4, 0));
        emit("backward", f.frame(2, 0));
        emit("seek_forward", f.frame(4, 0));
    }
    {
        Fixture f;
        f.scheduler.AddRange(true, 2, 4, true, "closed");
        f.scheduler.AddRange(true, 2, 4, false, "open");
        f.frame(2, 0);
        emit("range_inside", f.frame(4, 0));
        emit("range_end", f.frame(5, 0));
    }
    {
        Fixture f;
        f.scheduler.AddAtTick("root", 5); f.scheduler.AddAtTick("left", 3);
        f.scheduler.AddAtTick("right", 6);
        f.frame(2, 0);
        emit("one_branch", f.frame(7, 0));
    }
    {
        Fixture f;
        f.scheduler.AddAtTick("tick", 1); f.scheduler.AddAtTime("time", 1);
        f.frame(1, 1);
        emit("tick_before_time", f.frame(2, 2));
        f.scheduler.Enabled = false;
        emit("disabled", f.frame(3, 3));
    }
    {
        Fixture f;
        f.scheduler.AddAtTick("older", 1); f.scheduler.AddAtTick("removed", 2);
        f.scheduler.Remove(1); f.scheduler.AddAtTick("new", 3);
        f.frame(0.5f, 0);
        emit("partial_tree", f.frame(4, 0));
    }
    {
        Fixture f;
        f.scheduler.AddAtTick("a", 10); f.scheduler.AddAtTick("b", 12);
        f.scheduler.EditStartTick(20);
        f.scheduler.OnLevelInitPreEntity();
        f.frame(20, 0);
        emit("edit_start", f.frame(21, 0));
        f.frame(22, 0);
        emit("edit_second", f.frame(23, 0));
    }
    {
        Fixture f;
        f.scheduler.AddAtTick("B", 1.0000000000000002);
        f.scheduler.AddAtTick("A", 1.0);
        f.scheduler.EditStartTick(1000.0);
        f.frame(1000, 0);
        emit("edit_collapsed_order", f.frame(1001, 0));
    }
    {
        Fixture f;
        f.scheduler.AddAtTick("first", 3); f.scheduler.AddAtTick("second", 3);
        f.frame(3, 0);
        f.sink.mutate_once = true;
        emit("callback_mutation", f.frame(4, 0));
    }
    return 0;
}
'''


def main() -> None:
    if len(sys.argv) != 5:
        raise SystemExit(
            "usage: generate-original-harness.py CommandSystem.cpp CommandSystem.h output.cpp manifest.json"
        )
    cpp_path, header_path, output_path, manifest_path = map(Path, sys.argv[1:])
    cpp_bytes, header_bytes = cpp_path.read_bytes(), header_path.read_bytes()
    cpp = cpp_bytes.decode("utf-8").replace("\r\n", "\n")
    header = header_bytes.decode("utf-8").replace("\r\n", "\n")

    parts = [
        ("interfaces", "CommandSystem.h", slice_once(
            header, "class IExecuteClientCmdForCommandSystem {",
            "class CommandSystem\n{", "interfaces")),
        ("tree_and_maps", "CommandSystem.h", slice_once(
            header, "struct Interval {", "double m_LastTime;", "tree_and_maps")),
        ("add_edit_remove_clear", "CommandSystem.cpp", slice_once(
            cpp, "CommandSystem::CommandSystem(", "namespace CommandSystemXML {",
            "add_edit_remove_clear")),
        ("frame_reset_plain_command", "CommandSystem.cpp", slice_once(
            cpp, "void CommandSystem::OnExecuteCommands(void)",
            "void CommandSystem::AddCurves(", "frame_reset_plain_command")),
    ]
    if "root->Left->Max >= i.Low" not in parts[1][2]:
        raise ValueError("pinned tree traversal marker missing")
    if "if (0 != tick)" not in parts[3][2]:
        raise ValueError("pinned tick-zero marker missing")
    if "while(!m_CommandsToExecute.empty())" not in parts[3][2]:
        raise ValueError("pinned queue-drain marker missing")

    generated = (PRELUDE + parts[0][2] + CLASS_BEGIN + parts[1][2]
                 + CLASS_END + parts[2][2] + parts[3][2] + DRIVER)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(generated, encoding="utf-8")
    manifest = {
        "commit": "7567c24ec2501a7b07cda6213ece4540e57b5039",
        "inputs": {
            "CommandSystem.cpp": hashlib.sha256(cpp_bytes).hexdigest(),
            "CommandSystem.h": hashlib.sha256(header_bytes).hexdigest(),
        },
        "verbatim_slices": [
            {"name": name, "file": file, "sha256": hashlib.sha256(
                body.encode("utf-8")).hexdigest()}
            for name, file, body in parts
        ],
        "shim": (
            "concrete clock and sink implementations, CCommand curve fields, "
            "_snprintf_s, Warning, range insertion seam, fixtures and main"
        ),
    }
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"generated original-source harness: {output_path}")
    print(f"source-slice manifest: {manifest_path}")


if __name__ == "__main__":
    main()
