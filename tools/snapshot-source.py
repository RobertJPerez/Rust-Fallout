"""Hash the reviewable source/configuration files without including local retail data."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
names = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=ROOT).decode("utf-8").split("\0")
extensions = {".rs", ".wgsl", ".py", ".ps1", ".toml", ".cpp", ".hpp"}
rows = []
for name in sorted(set(names)):
    path = ROOT / name
    if not name or not path.is_file():
        continue
    if path.suffix not in extensions and path.name not in {"Cargo.lock", "CMakeLists.txt", "sources.lock.json"}:
        continue
    rows.append({"path": name.replace("\\", "/"), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})
identity = hashlib.sha256(json.dumps(rows, sort_keys=True, separators=(",", ":")).encode("utf-8")).hexdigest()
revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT))
report = {"schema_version": 1, "revision": revision, "working_tree_dirty": dirty,
          "scope": "Rust/C++ source, WGSL shaders, headers, tooling, manifests, source pins and Cargo locks; files are bound by their individual hashes",
          "digest_recipe": "SHA256 of UTF-8 JSON files array, sort_keys=True, separators=(',', ':')", "sha256": identity, "files": rows}
(ROOT / "reports/source-snapshot.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(f"{len(rows)} source/configuration files: {identity}")
