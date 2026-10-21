"""Verify the narrow editor update and test the export crate Cargo actually resolves."""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import tomllib

OLD = "4be130f016fe7a0a3b7ae5d59a037c299594aecb"
FIX = "a539163b9d86546ba61130c56a0395e05d16d39d"
CAPTURE = "fdf2f6ac8828374b803577ae1f6b310b73860075"
ROOT = Path(__file__).resolve().parents[1]
MANIFESTS = {
    "Cargo.toml": 5,
    "third_party/filmcraft_source/crates/ui-egui/Cargo.toml": 18,
}
LOCKS = {"Cargo.lock": 39, "rust-candidate/src-tauri/Cargo.lock": 7}


def read(path):
    return (ROOT / path).read_text(encoding="utf-8")


def baseline(ref, path):
    return subprocess.check_output(
        ["git", "show", f"{ref}:{path}"], cwd=ROOT, text=True, encoding="utf-8"
    )


def check_pins(base):
    for path, count in MANIFESTS.items():
        text = read(path)
        assert text.count(FIX) == count and OLD not in text, f"{path}: editor pins do not consume {FIX}"
        if base:
            assert text == baseline(base, path).replace(OLD, FIX), f"{path}: change exceeds editor pins"
    for path, count in LOCKS.items():
        text = read(path)
        packages = tomllib.loads(text)["package"]
        editor = [p for p in packages if p["name"].startswith("filmcraft-") and "git+" in p.get("source", "")]
        assert len(editor) == count, f"{path}: unexpected editor graph"
        assert all(p["source"].endswith(f"?rev={FIX}#{FIX}") for p in editor), f"{path}: split editor source"
        capture = [p for p in packages if p["name"].startswith("clipstrike-")]
        expected = {"clipstrike-core"} if path.startswith("rust-candidate/") else {
            "clipstrike-audio", "clipstrike-core", "clipstrike-media", "clipstrike-render", "clipstrike-ui"
        }
        assert {p["name"] for p in capture} == expected, f"{path}: changed capture package set"
        assert all(p["source"].endswith(f"?rev={CAPTURE}#{CAPTURE}") for p in capture), f"{path}: capture pin moved"
        if base:
            # This also preserves every registry checksum, dependency edge, and capture entry.
            assert text == baseline(base, path).replace(OLD, FIX), f"{path}: lock drift beyond source identity"
        print(f"{path}: {len(editor)} coherent editor packages; {len(capture)} capture packages unchanged", flush=True)
    if base:
        paths = subprocess.check_output(["git", "diff", "--name-only", base, "--"], cwd=ROOT, text=True).splitlines()
        allowed = set(MANIFESTS) | set(LOCKS) | {"tests/clipstrike_dependency.py", ".github/workflows/clipstrike-dependency.yml"}
        assert set(paths) <= allowed, f"out-of-scope edits: {set(paths) - allowed}"


def resolved_export():
    result = subprocess.check_output(
        ["cargo", "+stable", "metadata", "--locked", "--features", "native-ui", "--format-version", "1"], cwd=ROOT
    )
    metadata = json.loads(result)
    packages = metadata["packages"]
    exports = [p for p in packages if p["name"] == "filmcraft-export"]
    assert len(exports) == 1, "multiple export package identities"
    export = exports[0]
    assert export["source"].endswith(f"?rev={FIX}#{FIX}"), "resolved export is not the merged fix"
    assert all(p["source"].endswith(f"?rev={FIX}#{FIX}") for p in packages
               if p["name"].startswith("filmcraft-") and (p.get("source") or "").startswith("git+")), "mixed resolved editor graph"
    manifest = Path(export["manifest_path"])
    checkout = manifest.parents[2]
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
    assert head == FIX, f"resolved checkout HEAD mismatch: {head}"
    print(f"DemoStrike resolves {export['id']} at {manifest}", flush=True)
    return manifest, export["source"]


def test_consumed_export(manifest, source):
    # Cargo cannot test a dependency's dev targets from the consuming workspace.
    # A focused package runs its public integration tests against the same Git
    # library, retaining Demo's registry versions rather than the donor's lock.
    git_url = source.removeprefix("git+").split("?rev=")[0]
    locked = tomllib.loads(read("Cargo.lock"))["package"]
    rayon = next(p["version"] for p in locked if p["name"] == "rayon")
    identity = lambda p: (p["name"], p["version"], p.get("source"), p.get("checksum"))
    expected = {identity(p) for p in locked}
    with tempfile.TemporaryDirectory(prefix="demostrike-export-") as folder:
        harness = Path(folder)
        text = '[workspace]\n[package]\nname = "demostrike-export-compatibility"\nversion = "0.0.0"\nedition = "2024"\n[dependencies]\n'
        for name in ["export", "frame", "media", "project", "render", "time"]:
            text += f'filmcraft-{name} = {{ git = {json.dumps(git_url)}, rev = "{FIX}" }}\n'
        text += f'rayon = "={rayon}"\n'
        for name in ["renderer_reentrancy", "pool_trim"]:
            path = manifest.parent / "tests" / f"{name}.rs"
            text += f'\n[[test]]\nname = "{name}"\npath = {json.dumps(str(path))}\n'
        (harness / "Cargo.toml").write_text(text, encoding="utf-8")
        (harness / "Cargo.lock").write_text(read("Cargo.lock"), encoding="utf-8")
        # Permit only package pruning/the test host entry, before any compilation.
        subprocess.run(["cargo", "+stable", "metadata", "--offline", "--format-version", "1"],
                       cwd=harness, check=True, stdout=subprocess.DEVNULL)
        actual = tomllib.loads((harness / "Cargo.lock").read_text(encoding="utf-8"))["package"]
        drift = [identity(p) for p in actual if p["name"] != "demostrike-export-compatibility" and identity(p) not in expected]
        assert not drift, f"test harness dependencies differ from Demo lock: {drift}"
        print(f"Consumed export harness: {len(actual) - 1} dependency versions/sources/checksums match Demo lock", flush=True)
        subprocess.run(["cargo", "+stable", "test", "--locked", "--test", "renderer_reentrancy", "--test", "pool_trim",
                        "--", "--test-threads=1", "--nocapture"], cwd=harness, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-ref")
    parser.add_argument("--test-export", action="store_true")
    args = parser.parse_args()
    check_pins(args.base_ref)
    if args.test_export:
        manifest, source = resolved_export()
        test_consumed_export(manifest, source)
    print("Dependency contract verified", flush=True)


if __name__ == "__main__":
    main()
