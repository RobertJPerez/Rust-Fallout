#!/usr/bin/env python3
"""Summarize physical FO4 string-table locale and key coverage without decoding text."""
from __future__ import annotations

import hashlib
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

KNOWN_LOCALES = frozenset(
    {"cn", "de", "en", "es", "esmx", "fr", "it", "ja", "pl", "ptbr", "ru", "zhhant"}
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def logical_table(member_name: bytes, archive: str, extension: str) -> tuple[tuple[str, str, str], str]:
    name = member_name.decode("ascii")
    normalized = name.replace("/", "\\").casefold()
    path, sep, actual_extension = normalized.rpartition(".")
    if not sep or actual_extension != extension.casefold():
        raise ValueError(f"member extension does not match table metadata: {name!r}")
    stem = path.rsplit("\\", 1)[-1]
    stem_prefix, separator, locale = stem.rpartition("_")
    if not separator or locale not in KNOWN_LOCALES:
        raise ValueError(f"unknown or missing locale suffix: {name!r}")
    group = (archive.casefold(), f"{path[:-len(locale)-1]}{{locale}}", actual_extension)
    return group, locale


def load_evidence(root: Path) -> dict[str, Any]:
    complete = json.loads((root / "complete.json").read_text(encoding="utf-8"))
    manifest_path = root / "manifest.json"
    if not complete.get("complete") or complete.get("runtime_ready"):
        raise ValueError("Rust localization evidence is incomplete or incorrectly marked runtime-ready")
    if sha256(manifest_path) != complete["manifest_sha256"]:
        raise ValueError("Rust localization manifest hash disagrees with its completion marker")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    for name, hash_field in (("tables.jsonl", "table_rows_sha256"), ("keys.jsonl", "key_rows_sha256")):
        if sha256(root / name) != manifest[hash_field]:
            raise ValueError(f"{name} hash disagrees with Rust manifest")

    tables: dict[int, dict[str, Any]] = {}
    grouped: dict[tuple[str, str, str], dict[str, int]] = defaultdict(dict)
    for line in (root / "tables.jsonl").open(encoding="utf-8"):
        row = json.loads(line)
        table_id = row["table_id"]
        if table_id in tables:
            raise ValueError(f"duplicate table id {table_id}")
        group, locale = logical_table(bytes.fromhex(row["member_name_bytes_hex"]), row["archive"], row["extension"])
        if locale in grouped[group]:
            raise ValueError(f"duplicate locale {locale} in logical table {group}")
        grouped[group][locale] = table_id
        tables[table_id] = row

    keys: dict[int, set[int]] = {table_id: set() for table_id in tables}
    row_counts: Counter[int] = Counter()
    for line in (root / "keys.jsonl").open(encoding="utf-8"):
        row = json.loads(line)
        table_id, key = row["table_id"], row["key"]
        if table_id not in tables:
            raise ValueError(f"key row references unknown table {table_id}")
        if key in keys[table_id]:
            raise ValueError(f"duplicate key {key} in table {table_id}")
        keys[table_id].add(key)
        row_counts[table_id] += 1
    for table_id, table in tables.items():
        if row_counts[table_id] != table["keys"]:
            raise ValueError(f"table {table_id} key row count disagrees with table manifest")

    summaries = []
    differing_groups = 0
    no_english_groups = 0
    locale_group_sizes: Counter[int] = Counter()
    for group, variants in sorted(grouped.items()):
        locale_group_sizes[len(variants)] += 1
        english_id = variants.get("en")
        if english_id is None:
            no_english_groups += 1
            english_keys: set[int] = set()
        else:
            english_keys = keys[english_id]
        locale_rows = []
        for locale, table_id in sorted(variants.items()):
            present = keys[table_id]
            absent_from_locale = len(english_keys - present) if english_id is not None else None
            extra_to_english = len(present - english_keys) if english_id is not None else None
            locale_rows.append(
                {
                    "locale_suffix": locale,
                    "table_id": table_id,
                    "keys": len(present),
                    "keys_absent_from_locale_vs_en": absent_from_locale,
                    "keys_extra_vs_en": extra_to_english,
                }
            )
        group_differs = any(
            row["keys_absent_from_locale_vs_en"] not in (None, 0)
            or row["keys_extra_vs_en"] not in (None, 0)
            for row in locale_rows
        )
        differing_groups += int(group_differs)
        archive, logical_name, extension = group
        summaries.append(
            {
                "archive": archive,
                "logical_name": logical_name,
                "extension": extension,
                "locale_count": len(variants),
                "locales_missing_from_physical_archive": sorted(KNOWN_LOCALES - variants.keys()),
                "english_present": english_id is not None,
                "key_sets_match_english": english_id is not None and not group_differs,
                "locales": locale_rows,
            }
        )
    return {
        "schema": 1,
        "status": "physical-locale-key-coverage-not-language-selection",
        "source_manifest_sha256": sha256(root / "manifest.json"),
        "source_tables_sha256": sha256(root / "tables.jsonl"),
        "source_keys_sha256": sha256(root / "keys.jsonl"),
        "tables": len(tables),
        "keys": sum(row_counts.values()),
        "logical_table_groups": len(grouped),
        "locale_group_sizes": {str(k): v for k, v in sorted(locale_group_sizes.items())},
        "locale_suffix_counts": dict(sorted(Counter(locale for row in summaries for locale in (item["locale_suffix"] for item in row["locales"])).items())),
        "groups_without_english": no_english_groups,
        "groups_with_key_set_differences_from_english": differing_groups,
        "locale_suffixes_are_path_labels_only": True,
        "runtime_ready": False,
        "groups": summaries,
    }


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: py -3.13 tools/summarize_localization.py local/strings-audit-NNN", file=sys.stderr)
        return 2
    try:
        root = Path(sys.argv[1])
        if root.is_absolute() or ".." in root.parts or not root.parts or root.parts[0].casefold() != "local":
            raise ValueError("analysis input must be a path under ignored local/")
        local_root = Path("local").resolve(strict=True)
        root = root.resolve(strict=True)
        root.relative_to(local_root)
        report = load_evidence(root)
        output = root / "locale-coverage.json"
        output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        summary = {key: value for key, value in report.items() if key != "groups"}
        summary["coverage_report"] = str(output)
        print(json.dumps(summary, indent=2))
        return 0
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"localization analysis failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
