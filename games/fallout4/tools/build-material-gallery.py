#!/usr/bin/env python3
"""Build a local-only visual preview of bounded Fallout 4 BA2 texture candidates.

This is an inspection gallery, not a game renderer or a pixel-parity test. DDS
decode failures are retained as explicit per-image diagnostics.
"""

from __future__ import annotations

import argparse
import hashlib
import html
import json
from pathlib import Path
import shutil
import subprocess
import sys


MAX_REPORT_BYTES = 8 * 1024 * 1024
MAX_DDS_BYTES = 64 * 1024 * 1024
MAX_PNG_BYTES = 32 * 1024 * 1024
MAX_TEXTURES = 128


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_stable(path: Path, limit: int) -> bytes:
    before = path.stat()
    if not path.is_file() or before.st_size > limit:
        raise ValueError(f"{path}: missing, non-file, or exceeds {limit} bytes")
    data = path.read_bytes()
    after = path.stat()
    if len(data) != before.st_size or before.st_size != after.st_size:
        raise ValueError(f"{path}: changed while read")
    if before.st_mtime_ns != after.st_mtime_ns:
        raise ValueError(f"{path}: modification time changed while read")
    return data


def local_output(root: Path, source: Path, requested: Path) -> Path:
    local = (root / "local").resolve(strict=True)
    source = source.resolve(strict=True)
    if local not in source.parents:
        raise ValueError("candidate evidence must be under ignored local/")
    parent = requested.parent if requested.parent != Path("") else Path(".")
    output = parent.resolve(strict=True) / requested.name
    if output == source or source in output.parents:
        raise ValueError("gallery output cannot be the candidate evidence directory or inside it")
    if local not in output.parents:
        raise ValueError("gallery output must be a new directory under ignored local/")
    if output.exists():
        raise ValueError("gallery output already exists; choose a new directory")
    return output


def safe_dds_path(root: Path, relative: str) -> Path:
    path = Path(relative)
    if path.is_absolute() or path.as_posix() != relative or len(path.parts) != 2:
        raise ValueError(f"unsafe candidate member path {relative!r}")
    if path.parts[0] != "raw" or path.suffix.lower() != ".dds":
        raise ValueError(f"unsupported candidate member path {relative!r}")
    result = (root / path).resolve(strict=True)
    if root.resolve(strict=True) not in result.parents:
        raise ValueError("candidate member resolves outside evidence directory")
    return result


def load_evidence(evidence_dir: Path) -> tuple[dict, str, list[tuple[dict, Path, bytes]]]:
    completion_bytes = read_stable(evidence_dir / "complete.json", 1024 * 1024)
    completion = json.loads(completion_bytes)
    report_bytes = read_stable(evidence_dir / "report.json", MAX_REPORT_BYTES)
    report_hash = sha256(report_bytes)
    report = json.loads(report_bytes)
    if (
        completion.get("schema_version") != 1
        or completion.get("report_sha256") != report_hash
        or completion.get("complete") is not True
        or completion.get("runtime_ready") is not False
        or report.get("schema_version") != 1
        or report.get("selected_count") != len(report.get("textures", []))
        or completion.get("selected_count") != report.get("selected_count")
        or completion.get("extracted_bytes") != report.get("extracted_bytes")
        or not 0 < len(report.get("textures", [])) <= MAX_TEXTURES
    ):
        raise ValueError("candidate extraction completion/report mismatch or unsupported scope")

    rows = []
    total_bytes = 0
    seen_ids: set[int] = set()
    for texture in report["textures"]:
        texture_id = texture.get("id")
        if not isinstance(texture_id, int) or texture_id < 1 or texture_id in seen_ids:
            raise ValueError("candidate report has invalid or repeated texture ids")
        seen_ids.add(texture_id)
        member = safe_dds_path(evidence_dir, texture.get("dds_file", ""))
        dds = read_stable(member, MAX_DDS_BYTES)
        if not dds.startswith(b"DDS ") or sha256(dds) != texture.get("dds_sha256"):
            raise ValueError(f"texture {texture_id}: DDS signature or fingerprint mismatch")
        if len(dds) != texture.get("dds_bytes"):
            raise ValueError(f"texture {texture_id}: byte count does not match report")
        total_bytes += len(dds)
        rows.append((texture, member, dds))
    if total_bytes != report.get("extracted_bytes"):
        raise ValueError("candidate DDS total differs from report")
    return report, report_hash, rows


def make_gallery(root: Path, evidence_dir: Path, requested_output: Path, magick_arg: str | None) -> dict:
    evidence_dir = evidence_dir.resolve(strict=True)
    output = local_output(root, evidence_dir, requested_output)
    report, report_hash, rows = load_evidence(evidence_dir)
    magick = magick_arg or shutil.which("magick.exe") or shutil.which("magick")
    if not magick:
        raise ValueError("ImageMagick 'magick' was not found; pass --magick with its executable path")
    magick_path = str(Path(magick).resolve(strict=True))
    version_run = subprocess.run(
        [magick_path, "-version"],
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    )
    magick_version = version_run.stdout.splitlines()[0] if version_run.stdout else "version-unreported"
    magick_binary_sha256 = sha256(read_stable(Path(magick_path), 256 * 1024 * 1024))

    output.mkdir()
    previews_dir = output / "previews"
    previews_dir.mkdir()
    previews = []
    for texture, member, _dds in rows:
        texture_id = texture["id"]
        relative_png = f"previews/{texture_id:04d}.png"
        final_png = output / relative_png
        temp_png = output / f"previews/{texture_id:04d}.partial.png"
        command = [
            magick_path,
            "-limit", "memory", "512MiB",
            "-limit", "map", "1GiB",
            "-limit", "disk", "2GiB",
            "-limit", "thread", "1",
            str(member),
            "-strip",
            "-resize", "512x512>",
            str(temp_png),
        ]
        state = "previewed"
        diagnostic = None
        try:
            result = subprocess.run(command, check=False, capture_output=True, text=True, timeout=90)
            if result.returncode != 0 or not temp_png.is_file():
                state = "decoder_refused_or_failed"
                diagnostic = (result.stderr or result.stdout or "decoder produced no preview")[:1200]
                temp_png.unlink(missing_ok=True)
            else:
                png_stat = temp_png.stat()
                if png_stat.st_size <= 0 or png_stat.st_size > MAX_PNG_BYTES:
                    state = "preview_output_outside_size_limit"
                    diagnostic = f"PNG output size {png_stat.st_size} is outside 1..{MAX_PNG_BYTES}"
                    temp_png.unlink(missing_ok=True)
                else:
                    temp_png.replace(final_png)
        except (OSError, subprocess.TimeoutExpired) as error:
            state = "decoder_refused_or_failed"
            diagnostic = str(error)[:1200]
            temp_png.unlink(missing_ok=True)

        row = {
            "id": texture_id,
            "canonical_path": texture["canonical_path"],
            "material_fields": texture["material_fields"],
            "material_reference_count": texture["material_reference_count"],
            "candidate_archive": texture["candidate_archive"],
            "candidate_entry_index": texture["candidate_entry_index"],
            "dxgi_format_raw": texture["texture_header"]["dxgi_format"],
            "width": texture["texture_header"]["width"],
            "height": texture["texture_header"]["height"],
            "mip_count": texture["texture_header"]["mip_count"],
            "dds_sha256": texture["dds_sha256"],
            "preview_status": state,
            "diagnostic": diagnostic,
            "png_file": relative_png if state == "previewed" else None,
            "png_sha256": sha256(final_png.read_bytes()) if state == "previewed" else None,
        }
        previews.append(row)

    preview_count = sum(row["preview_status"] == "previewed" for row in previews)
    failures = len(previews) - preview_count
    cards = []
    for row in previews:
        image = (
            f'<img loading="lazy" src="{html.escape(row["png_file"], quote=True)}" '
            f'alt="Raw texture preview for {html.escape(row["canonical_path"], quote=True)}">'
            if row["png_file"]
            else '<div class="unavailable">Preview unavailable; see diagnostic in report.json.</div>'
        )
        fields = ", ".join(html.escape(value) for value in row["material_fields"])
        cards.append(
            "<article>"
            f"<h2>{html.escape(row['canonical_path'])}</h2>"
            f"{image}"
            f"<p>BA2 candidate: {html.escape(row['candidate_archive'])} &middot; "
            f"entry {row['candidate_entry_index']}</p>"
            f"<p>Material fields: {fields} &middot; DXGI value {row['dxgi_format_raw']} &middot; "
            f"{row['width']}&times;{row['height']} &middot; {row['mip_count']} mips</p>"
            "</article>"
        )
    page = """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>Fallout 4 material texture candidates</title><style>
body{font:15px system-ui,sans-serif;background:#17191d;color:#eee;margin:2rem}
h1{font-size:1.7rem} .note{max-width:1000px;line-height:1.5;color:#bbb}
main{display:grid;grid-template-columns:repeat(auto-fill,minmax(310px,1fr));gap:1rem}
article{background:#24272d;border:1px solid #3b4048;border-radius:8px;padding:1rem;overflow:hidden}
article h2{font-size:1rem;overflow-wrap:anywhere} img{display:block;width:100%;height:260px;object-fit:contain;background:#111}
article p{font-size:.82rem;color:#c5cad1;overflow-wrap:anywhere}.unavailable{height:260px;display:grid;place-items:center;color:#aaa;background:#111}
</style></head><body>
<h1>Fallout 4 material texture candidates</h1>
<p class="note">Local previews decoded from uniquely matched DDS members in physical Data BA2 archives. This page preserves raw texture channel appearance for inspection. It does not apply BGSM/BGEM shaders, select loose/VFS or runtime archive winners, interpret color spaces, or establish pixel/rendering parity. See report.json for source hashes and decoder diagnostics.</p>
<main>
""" + "\n".join(cards) + "\n</main></body></html>\n"
    (output / "index.html").write_text(page, encoding="utf-8", newline="\n")
    report_out = {
        "schema_version": 1,
        "candidate_extraction_report_sha256": report_hash,
        "image_magick_executable": magick_path,
        "image_magick_version": magick_version,
        "image_magick_binary_sha256": magick_binary_sha256,
        "selected_textures": len(previews),
        "previews_created": preview_count,
        "decode_failures_or_refusals": failures,
        "previews": previews,
        "scope": "Inspection previews only. ImageMagick output is not compared with the game renderer or another pixel decoder; unsupported DDS encodings remain explicit.",
    }
    report_bytes = (json.dumps(report_out, indent=2, ensure_ascii=True) + "\n").encode("utf-8")
    (output / "report.json").write_bytes(report_bytes)
    completion = {
        "schema_version": 1,
        "report_sha256": sha256(report_bytes),
        "selected_textures": len(previews),
        "previews_created": preview_count,
        "decode_failures_or_refusals": failures,
        "complete": True,
        "runtime_ready": False,
    }
    (output / "complete.json").write_text(json.dumps(completion, indent=2) + "\n", encoding="utf-8")
    return completion


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate_evidence", type=Path)
    parser.add_argument("new_local_gallery", type=Path)
    parser.add_argument("--magick", help="ImageMagick executable; defaults to PATH lookup")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    try:
        result = make_gallery(root, args.candidate_evidence, args.new_local_gallery, args.magick)
    except Exception as error:
        print(f"material gallery incomplete: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
