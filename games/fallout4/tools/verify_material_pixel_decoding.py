#!/usr/bin/env python3
"""Compare bounded Fallout 4 DDS top mips through two isolated decoders.

ImageMagick decodes the archived DDS directly. Microsoft texconv independently
decodes the same DDS to PNG; ImageMagick reads that lossless PNG into the same
RGBA8 byte layout. This measures decoder output agreement, not game rendering.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time


MAX_REPORT_BYTES = 8 * 1024 * 1024
MAX_DDS_BYTES = 64 * 1024 * 1024
MAX_TEXTURES = 128
MAX_PIXELS_PER_TEXTURE = 16 * 1024 * 1024
MAX_TOTAL_RGBA_BYTES = 512 * 1024 * 1024
MAX_TOOL_BYTES = 64 * 1024 * 1024
PROCESS_TIMEOUT_SECONDS = 180
TEXCONV_SHA256 = "dcfdec10244e02cf5037fba089c55fb7e1326b1c8181742d77d15fa5cb5eef06"
TEXCONV_VERSION = "Microsoft (R) DirectX Texture Converter [DirectXTex] Version 2026.5.8.1"
PIXEL_TOLERANCE = 1


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


def read_json(path: Path, limit: int = MAX_REPORT_BYTES) -> tuple[dict, bytes]:
    data = read_stable(path, limit)
    parsed = json.loads(data)
    if not isinstance(parsed, dict):
        raise ValueError(f"{path}: expected a JSON object")
    return parsed, data


def resolve_local(root: Path, requested: Path) -> Path:
    local = (root / "local").resolve(strict=True)
    candidate = requested if requested.is_absolute() else root / requested
    resolved = candidate.resolve(strict=True)
    if resolved != local and local not in resolved.parents:
        raise ValueError(f"evidence path must stay under ignored local/: {resolved}")
    return resolved


def resolve_new_local_output(root: Path, requested: Path) -> Path:
    local = (root / "local").resolve(strict=True)
    candidate = requested if requested.is_absolute() else root / requested
    parent = candidate.parent.resolve(strict=True)
    output = parent / candidate.name
    if output.exists():
        raise ValueError(f"output already exists; choose a new local directory: {output}")
    if local not in output.parents:
        raise ValueError("new pixel-audit output must be below ignored local/")
    return output


def checked_report_pair(directory: Path, report_name: str, complete_name: str) -> tuple[dict, dict, str]:
    report_path = directory / report_name
    complete_path = directory / complete_name
    report, report_bytes = read_json(report_path)
    complete, _ = read_json(complete_path, 1024 * 1024)
    report_hash = sha256(report_bytes)
    if complete.get("complete") is not True or complete.get("runtime_ready") is not False:
        raise ValueError(f"{directory}: incomplete evidence or unsupported runtime scope")
    if complete.get("report_sha256") != report_hash:
        raise ValueError(f"{directory}: completion/report hash mismatch")
    return report, complete, report_hash


def safe_member(evidence_dir: Path, relative: str) -> Path:
    path = Path(relative)
    if path.is_absolute() or path.as_posix() != relative or len(path.parts) != 2:
        raise ValueError(f"unsafe texture member path: {relative!r}")
    if path.parts[0] != "raw" or path.suffix.lower() != ".dds":
        raise ValueError(f"unsupported texture member path: {relative!r}")
    result = (evidence_dir / path).resolve(strict=True)
    if evidence_dir not in result.parents:
        raise ValueError("texture member resolves outside the candidate evidence")
    return result


def run_checked(command: list[str], timeout: int = PROCESS_TIMEOUT_SECONDS) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "no diagnostic text")[:1200]
        raise subprocess.CalledProcessError(result.returncode, command, output=result.stdout, stderr=detail)
    return result


def identify_dimensions(magick: Path, image: Path) -> tuple[int, int]:
    result = run_checked([str(magick), "identify", "-limit", "thread", "1", "-ping", "-format", "%w %h", str(image)])
    fields = result.stdout.strip().split()
    if len(fields) != 2:
        raise ValueError(f"ImageMagick returned an invalid size for {image.name}: {result.stdout[:300]!r}")
    width, height = (int(value) for value in fields)
    if width <= 0 or height <= 0:
        raise ValueError(f"ImageMagick returned invalid dimensions for {image.name}")
    return width, height


def compare_rgba8(expected: bytes, actual: bytes, width: int, height: int) -> dict:
    if width <= 0 or height <= 0:
        raise ValueError("pixel dimensions must be positive")
    expected_length = width * height * 4
    if len(expected) != expected_length or len(actual) != expected_length:
        raise ValueError(
            f"RGBA plane size mismatch for {width}x{height}: "
            f"expected {expected_length} bytes, got {len(expected)} and {len(actual)}"
        )
    differing_components = [0, 0, 0, 0]
    differing_pixels = 0
    max_abs_difference = 0
    within_tolerance = True
    for offset in range(0, expected_length, 4):
        pixel_differs = False
        for channel in range(4):
            difference = abs(expected[offset + channel] - actual[offset + channel])
            if difference:
                differing_components[channel] += 1
                pixel_differs = True
                if difference > max_abs_difference:
                    max_abs_difference = difference
                if difference > PIXEL_TOLERANCE:
                    within_tolerance = False
        if pixel_differs:
            differing_pixels += 1
    pixel_count = width * height
    return {
        "pixel_count": pixel_count,
        "exact_match": differing_pixels == 0,
        "pixels_with_any_difference": differing_pixels,
        "differing_components_rgba": differing_components,
        "max_abs_channel_difference": max_abs_difference,
        "within_one_8bit_code_value": within_tolerance,
    }


def load_inputs(root: Path, candidates_dir: Path, gallery_dir: Path, magick_arg: Path, texconv_arg: Path):
    candidates_dir = resolve_local(root, candidates_dir)
    gallery_dir = resolve_local(root, gallery_dir)
    extract, extract_complete, extract_hash = checked_report_pair(candidates_dir, "report.json", "complete.json")
    gallery, gallery_complete, gallery_hash = checked_report_pair(gallery_dir, "report.json", "complete.json")
    if extract.get("schema_version") != 1 or gallery.get("schema_version") != 1:
        raise ValueError("unsupported candidate or gallery report schema")
    textures = extract.get("textures")
    previews = gallery.get("previews")
    if not isinstance(textures, list) or not 0 < len(textures) <= MAX_TEXTURES:
        raise ValueError("candidate report has an unsupported texture count")
    if not isinstance(previews, list) or len(previews) != len(textures):
        raise ValueError("gallery and candidate texture counts differ")
    if gallery.get("candidate_extraction_report_sha256") != extract_hash:
        raise ValueError("gallery was generated from a different candidate extraction")
    if gallery_complete.get("previews_created") != len(previews) or gallery_complete.get("decode_failures_or_refusals") != 0:
        raise ValueError("ImageMagick gallery is incomplete or has decode failures")

    gallery_by_id = {row.get("id"): row for row in previews if isinstance(row, dict)}
    if len(gallery_by_id) != len(previews):
        raise ValueError("gallery report has invalid or duplicate texture ids")
    loaded = []
    total_rgba_bytes = 0
    seen_ids: set[int] = set()
    for texture in textures:
        if not isinstance(texture, dict):
            raise ValueError("candidate report contains a non-object texture row")
        texture_id = texture.get("id")
        if not isinstance(texture_id, int) or texture_id < 1 or texture_id in seen_ids:
            raise ValueError("candidate report has invalid or repeated texture ids")
        seen_ids.add(texture_id)
        preview = gallery_by_id.get(texture_id)
        if preview is None or preview.get("dds_sha256") != texture.get("dds_sha256"):
            raise ValueError(f"texture {texture_id}: gallery/source DDS fingerprints differ")
        if preview.get("preview_status") != "previewed":
            raise ValueError(f"texture {texture_id}: ImageMagick preview was not successful")
        header = texture.get("texture_header")
        if not isinstance(header, dict):
            raise ValueError(f"texture {texture_id}: missing candidate DDS header")
        width, height = header.get("width"), header.get("height")
        if not isinstance(width, int) or not isinstance(height, int) or width <= 0 or height <= 0:
            raise ValueError(f"texture {texture_id}: invalid header dimensions")
        pixels = width * height
        if pixels > MAX_PIXELS_PER_TEXTURE:
            raise ValueError(f"texture {texture_id}: exceeds pixel-count budget")
        total_rgba_bytes += pixels * 4
        if total_rgba_bytes > MAX_TOTAL_RGBA_BYTES:
            raise ValueError("candidate set exceeds aggregate decoded-pixel budget")
        source = safe_member(candidates_dir, texture.get("dds_file", ""))
        dds = read_stable(source, MAX_DDS_BYTES)
        if not dds.startswith(b"DDS ") or len(dds) != texture.get("dds_bytes") or sha256(dds) != texture.get("dds_sha256"):
            raise ValueError(f"texture {texture_id}: DDS bytes do not match the candidate manifest")
        if width != preview.get("width") or height != preview.get("height"):
            raise ValueError(f"texture {texture_id}: gallery and DDS dimensions differ")
        if header.get("dxgi_format") != preview.get("dxgi_format_raw"):
            raise ValueError(f"texture {texture_id}: gallery and DDS formats differ")
        loaded.append((texture, preview, source, width, height, sha256(dds)))

    magick = magick_arg.resolve(strict=True)
    texconv = texconv_arg.resolve(strict=True)
    magick_hash = sha256(read_stable(magick, MAX_TOOL_BYTES))
    texconv_hash = sha256(read_stable(texconv, MAX_TOOL_BYTES))
    if magick_hash != gallery.get("image_magick_binary_sha256"):
        raise ValueError("ImageMagick executable differs from the one used for the gallery")
    if texconv_hash != TEXCONV_SHA256:
        raise ValueError(f"texconv executable hash differs from the pinned release: {texconv_hash}")
    magick_version_result = run_checked([str(magick), "-version"], timeout=30)
    magick_version = magick_version_result.stdout.splitlines()[0] if magick_version_result.stdout else ""
    if magick_version != gallery.get("image_magick_version"):
        raise ValueError("ImageMagick version differs from the decoder used for the gallery")
    texconv_version_result = run_checked([str(texconv), "--help"], timeout=30)
    texconv_version = texconv_version_result.stdout.splitlines()[0] if texconv_version_result.stdout else ""
    if texconv_version != TEXCONV_VERSION:
        raise ValueError(f"unsupported texconv version: {texconv_version!r}")
    return {
        "candidates_dir": candidates_dir,
        "gallery_dir": gallery_dir,
        "extract_report_sha256": extract_hash,
        "gallery_report_sha256": gallery_hash,
        "textures": loaded,
        "magick": magick,
        "magick_hash": magick_hash,
        "magick_version": magick_version,
        "texconv": texconv,
        "texconv_hash": texconv_hash,
        "texconv_version": texconv_version,
    }


def audit(root: Path, candidates_arg: Path, gallery_arg: Path, texconv_arg: Path, output_arg: Path, magick_arg: Path) -> dict:
    inputs = load_inputs(root, candidates_arg, gallery_arg, magick_arg, texconv_arg)
    output = resolve_new_local_output(root, output_arg)
    output.mkdir()
    directxtex_dir = output / "directxtex-png"
    magick_raw_dir = output / "imagemagick-rgba"
    texconv_raw_dir = output / "texconv-rgba"
    directxtex_dir.mkdir()
    magick_raw_dir.mkdir()
    texconv_raw_dir.mkdir()

    results = []
    started = time.monotonic()
    for texture, _preview, source, width, height, dds_hash in inputs["textures"]:
        texture_id = texture["id"]
        stem = f"{texture_id:04d}"
        dds_input = f"{source}[0]"
        image_raw = magick_raw_dir / f"{stem}.rgba"
        texconv_png = directxtex_dir / f"{stem}.png"
        texconv_raw = texconv_raw_dir / f"{stem}.rgba"
        row = {
            "id": texture_id,
            "canonical_path": texture["canonical_path"],
            "candidate_archive": texture["candidate_archive"],
            "candidate_entry_index": texture["candidate_entry_index"],
            "dds_sha256": dds_hash,
            "width": width,
            "height": height,
            "dxgi_format_raw": texture["texture_header"]["dxgi_format"],
            "status": "decode_failed",
            "diagnostic": None,
        }
        try:
            im_dims = identify_dimensions(inputs["magick"], source)
            if im_dims != (width, height):
                raise ValueError(f"ImageMagick DDS top-mip dimensions {im_dims} differ from BA2 header {(width, height)}")
            run_checked([
                str(inputs["magick"]),
                "-limit", "memory", "512MiB",
                "-limit", "map", "1GiB",
                "-limit", "disk", "2GiB",
                "-limit", "thread", "1",
                dds_input,
                "-depth", "8",
                f"RGBA:{image_raw}",
            ])
            run_checked([
                str(inputs["texconv"]),
                "-nologo", "-m", "1", "-f", "R8G8B8A8_UNORM", "-ft", "png", "-y", "-o", str(directxtex_dir), str(source),
            ])
            if not texconv_png.is_file():
                raise ValueError("texconv did not create the expected PNG output")
            dxt_dims = identify_dimensions(inputs["magick"], texconv_png)
            if dxt_dims != (width, height):
                raise ValueError(f"texconv top-mip dimensions {dxt_dims} differ from BA2 header {(width, height)}")
            run_checked([
                str(inputs["magick"]),
                "-limit", "memory", "512MiB",
                "-limit", "map", "1GiB",
                "-limit", "disk", "2GiB",
                "-limit", "thread", "1",
                str(texconv_png),
                "-depth", "8",
                f"RGBA:{texconv_raw}",
            ])
            raw_a = read_stable(image_raw, width * height * 4)
            raw_b = read_stable(texconv_raw, width * height * 4)
            comparison = compare_rgba8(raw_a, raw_b, width, height)
            row.update(comparison)
            row["image_magick_rgba_sha256"] = sha256(raw_a)
            row["texconv_png_sha256"] = sha256(read_stable(texconv_png, MAX_DDS_BYTES))
            row["texconv_rgba_sha256"] = sha256(raw_b)
            row["texconv_png"] = texconv_png.relative_to(output).as_posix()
            row["status"] = "within_one_8bit_code_value" if comparison["within_one_8bit_code_value"] else "pixel_difference_exceeds_tolerance"
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            detail = getattr(error, "stderr", None) or getattr(error, "output", None)
            row["diagnostic"] = (str(error) + (f": {detail}" if detail else ""))[:1600]
        results.append(row)

    successful = [row for row in results if "within_one_8bit_code_value" in row]
    within = sum(row.get("within_one_8bit_code_value") is True for row in successful)
    exact = sum(row.get("exact_match") is True for row in successful)
    beyond = sum(row.get("within_one_8bit_code_value") is False for row in successful)
    report = {
        "schema_version": 1,
        "candidate_extraction_report_sha256": inputs["extract_report_sha256"],
        "imagemagick_gallery_report_sha256": inputs["gallery_report_sha256"],
        "candidate_directory": inputs["candidates_dir"].relative_to(root).as_posix(),
        "gallery_directory": inputs["gallery_dir"].relative_to(root).as_posix(),
        "image_magick_version": inputs["magick_version"],
        "image_magick_binary_sha256": inputs["magick_hash"],
        "texconv_version": inputs["texconv_version"],
        "texconv_binary_sha256": inputs["texconv_hash"],
        "texconv_pinned_release_sha256": TEXCONV_SHA256,
        "textures_selected": len(results),
        "textures_decoded_and_compared": len(successful),
        "textures_exactly_equal": exact,
        "textures_within_one_8bit_code_value": within,
        "textures_outside_one_8bit_code_value": beyond,
        "texture_failures": len(results) - len(successful),
        "per_texture": results,
        "elapsed_seconds": round(time.monotonic() - started, 3),
        "complete": len(results) == len(inputs["textures"]),
        "runtime_ready": False,
        "scope": "Top-mip RGBA8 decoder comparison only. ImageMagick decodes DDS directly; DirectXTex texconv decodes DDS to PNG, which ImageMagick reads into RGBA8 for byte comparison. A one-code-value per-channel tolerance records known quantization-rounding differences. This does not compare a game renderer, shader/color-space behavior, mip selection, or runtime/VFS winner.",
    }
    report_bytes = (json.dumps(report, indent=2, ensure_ascii=True) + "\n").encode("utf-8")
    report_path = output / "report.json"
    report_path.write_bytes(report_bytes)
    completion = {
        "schema_version": 1,
        "report_sha256": sha256(report_bytes),
        "textures_selected": len(results),
        "textures_decoded_and_compared": len(successful),
        "textures_within_one_8bit_code_value": within,
        "textures_outside_one_8bit_code_value": beyond,
        "texture_failures": len(results) - len(successful),
        "complete": report["complete"],
        "runtime_ready": False,
    }
    (output / "complete.json").write_text(json.dumps(completion, indent=2) + "\n", encoding="utf-8")
    return completion


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate_evidence", type=Path)
    parser.add_argument("imagemagick_gallery", type=Path)
    parser.add_argument("texconv_executable", type=Path)
    parser.add_argument("new_local_audit", type=Path)
    parser.add_argument("--magick", type=Path, required=True, help="same ImageMagick executable used for the gallery")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    try:
        result = audit(root, args.candidate_evidence, args.imagemagick_gallery, args.texconv_executable, args.new_local_audit, args.magick)
    except Exception as error:
        print(f"material pixel audit incomplete: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2))
    return 0 if result["texture_failures"] == 0 and result["textures_outside_one_8bit_code_value"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
