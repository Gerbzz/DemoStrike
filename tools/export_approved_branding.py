"""Export approved vector branding without cropping, recoloring or changing padding.
Requires Inkscape and Pillow only when regenerating; builds consume committed files.
Run --check for read-only hash, dimensions, ICO payload and ICNS decode checks.
"""
from pathlib import Path
import argparse, hashlib, io, json, struct, subprocess, tempfile
from PIL import Image
ROOT = Path(__file__).resolve().parents[1]
CONFIG = json.loads((ROOT / "tools/approved-branding.json").read_text())
def sha(b): return hashlib.sha256(b).hexdigest()
def main():
    parser = argparse.ArgumentParser(); parser.add_argument("--check", action="store_true"); args = parser.parse_args()
    source = ROOT / CONFIG["source"]
    assert sha(source.read_bytes()) == CONFIG["approved_sha256"], "approved vector changed"
    manifest_path = ROOT / "assets/branding/platform/manifest.json"
    if not args.check:
        with tempfile.TemporaryDirectory() as tmp:
            pngs = {}
            for size in CONFIG["sizes"]:
                p = Path(tmp) / f"{size}.png"
                subprocess.run(["inkscape", str(source), "--export-area-page", f"--export-width={size}", f"--export-height={size}", "--export-background-opacity=0", f"--export-filename={p}"], check=True, stdout=subprocess.DEVNULL)
                # Normalize to RGBA8, preserving all rendered pixels and canvas geometry.
                im = Image.open(p).convert("RGBA"); out = io.BytesIO(); im.save(out, format="PNG"); pngs[size] = out.getvalue()
            sizes = CONFIG["ico_sizes"]; offset = 6 + 16*len(sizes)
            ico = struct.pack("<HHH", 0, 1, len(sizes)); payload = b""
            for s in sizes:
                data = pngs[s]; ico += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
                payload += data; offset += len(data)
            # Standard PNG-backed ICNS elements. Native macOS acceptance remains separate.
            icns = b""
            for kind, size in [(b"icp4",16),(b"icp5",32),(b"icp6",64),(b"ic07",128),(b"ic08",256),(b"ic09",512),(b"ic10",1024)]:
                data = pngs[size]; icns += kind + struct.pack(">I",len(data)+8) + data
            blobs = {"ico": ico+payload, "icns": b"icns"+struct.pack(">I",len(icns)+8)+icns, "svg": source.read_bytes()}
            manifest = {"source": CONFIG["source"], "approved_sha256": CONFIG["approved_sha256"], "transform": "Full original SVG page; unchanged proportions, centering and padding", "native_acceptance": "unknown", "files": {}}
            for path, kind in CONFIG["outputs"].items():
                data = pngs[kind] if isinstance(kind,int) else blobs[kind]
                p = ROOT/path; p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(data)
                manifest["files"][path] = {"sha256":sha(data),"kind":kind}
            manifest_path.parent.mkdir(parents=True,exist_ok=True); manifest_path.write_text(json.dumps(manifest,indent=2)+"\n")
    manifest=json.loads(manifest_path.read_text())
    assert manifest["approved_sha256"] == CONFIG["approved_sha256"]
    assert set(manifest["files"]) == set(CONFIG["outputs"])
    for path, item in manifest["files"].items():
        data=(ROOT/path).read_bytes(); assert sha(data)==item["sha256"],path
        kind=item["kind"]
        if isinstance(kind,int):
            im=Image.open(io.BytesIO(data)).convert("RGBA"); assert im.size==(kind,kind)
            assert im.getchannel("A").getbbox(); assert im.getpixel((0,0))[3]==0
        elif kind=="svg": assert data==source.read_bytes()
        elif kind=="ico":
            assert struct.unpack_from("<HHH",data)==(0,1,len(CONFIG["ico_sizes"]))
            for i,s in enumerate(CONFIG["ico_sizes"]):
                w,h,_,_,planes,bits,n,off=struct.unpack_from("<BBBBHHII",data,6+i*16)
                assert (w or 256,h or 256,planes,bits)==(s,s,1,32)
                im=Image.open(io.BytesIO(data[off:off+n])); assert im.size==(s,s)
        elif kind=="icns":
            im=Image.open(io.BytesIO(data)); assert im.format=="ICNS"; im.load(); assert im.size==(1024,1024)
    for path, tokens in CONFIG["references"].items():
        content=(ROOT/path).read_text()
        for token in tokens: assert token in content,(path,token)
    print("Approved vector, export hashes, PNG/ICO/ICNS structure and runtime references PASS")
if __name__=="__main__": main()
