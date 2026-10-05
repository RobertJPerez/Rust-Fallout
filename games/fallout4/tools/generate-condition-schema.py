#!/usr/bin/env python3
"""Regenerate the pinned Fallout 4 CTDA function metadata table offline."""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE_RELATIVE = Path(
    "Mutagen.Bethesda.Fallout4/Records/Common Subrecords/Condition.cs"
)
EXPECTED_REVISION = "4f533562ee0c70347d47c1979d5464d42b06ee6b"


def require_match(pattern: str, text: str, label: str) -> str:
    match = re.search(pattern, text, re.DOTALL)
    if match is None:
        raise SystemExit(f"could not parse pinned {label}")
    return match.group(1)


def main() -> None:
    lock = json.loads((ROOT / "sources.lock.json").read_text(encoding="utf-8"))
    source_entry = next(
        (entry for entry in lock["sources"] if entry["id"] == "mutagen-vmad-oracle"),
        None,
    )
    if source_entry is None or source_entry["revision"] != EXPECTED_REVISION:
        raise SystemExit("sources.lock.json does not contain the expected pinned schema revision")
    source_path_relative = SOURCE_RELATIVE.as_posix()
    if source_path_relative not in source_entry["paths"]:
        raise SystemExit("the pinned condition schema file is absent from sources.lock.json")

    checkout = ROOT / "local" / "research" / "Mutagen"
    revision = subprocess.check_output(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True
    ).strip()
    if revision != source_entry["revision"]:
        raise SystemExit("the local schema checkout is not at its locked revision")
    status = subprocess.check_output(
        ["git", "-C", str(checkout), "status", "--porcelain"], text=True
    )
    if status.strip():
        raise SystemExit("the local schema checkout is dirty")

    source_path = checkout / SOURCE_RELATIVE
    source_bytes = source_path.read_bytes()
    source_sha256 = hashlib.sha256(source_bytes).hexdigest()
    source = source_bytes.decode("utf-8")

    function_body = require_match(
        r"public enum Function\s*\{(.*?)\n    \}", source, "Function enum"
    )
    functions = [
        (int(value), name)
        for name, value in re.findall(
            r"^\s*(\w+)\s*=\s*(\d+)\s*,?", function_body, re.MULTILINE
        )
    ]
    parameter_type_body = require_match(
        r"public enum ParameterType\s*\{(.*?)\n    \}",
        source,
        "ParameterType enum",
    )
    parameter_types = set(
        re.findall(r"^\s*(\w+)\s*=\s*\d+\s*,?", parameter_type_body, re.MULTILINE)
    )
    if len(functions) != 479 or len({index for index, _ in functions}) != 479:
        raise SystemExit("unexpected function enum shape for the locked revision")
    if len(parameter_types) != 57:
        raise SystemExit("unexpected parameter type enum shape for the locked revision")

    mapping_body = require_match(
        r"GetParameterTypes\(Function function\).*?return \(\(ushort\)function\) switch\s*\{(.*?)\n        \};",
        source,
        "GetParameterTypes map",
    )
    mappings = [
        (int(index), (first, second, third))
        for index, first, second, third in re.findall(
            r"(\d+)\s*=>\s*\(ParameterType\.(\w+),\s*ParameterType\.(\w+),\s*ParameterType\.(\w+)\)",
            mapping_body,
        )
    ]
    if len(mappings) != 220 or len({index for index, _ in mappings}) != 220:
        raise SystemExit("unexpected parameter mapping shape for the locked revision")

    category_body = require_match(
        r"GetCategory\(this Condition\.ParameterType type\).*?switch \(type\)\s*\{(.*?)\n        \}",
        source,
        "parameter category map",
    )
    categories: dict[str, str] = {}
    pending: list[str] = []
    for line in category_body.splitlines():
        case = re.search(r"case ParameterType\.(\w+):", line)
        if case:
            pending.append(case.group(1))
            continue
        category = re.search(r"return ParameterCategory\.(\w+);", line)
        if category:
            for parameter_type in pending:
                categories[parameter_type] = category.group(1).lower()
            pending = []
    if pending or set(categories) != parameter_types:
        raise SystemExit("parameter category map does not cover the pinned enum exactly")
    if {index for index, _ in mappings} - {index for index, _ in functions}:
        raise SystemExit("parameter map contains an ID missing from the function enum")

    functions.sort()
    mappings.sort()
    lines = [
        "//! Generated from the pinned Fallout 4 CTDA function schema.",
        f"//! Source revision: {revision}; Condition.cs SHA-256: {source_sha256}.",
        "//! Regenerate with py -3.13 tools/generate-condition-schema.py, then format with tools/cargo.ps1.",
        "use crate::condition::FunctionParameterHint;",
        "",
        "const FUNCTION_NAMES: &[(u16, &str)] = &[",
    ]
    lines.extend(f'    ({index}, "{name}"),' for index, name in functions)
    lines.extend(
        [
            "];",
            "",
            "#[derive(Debug, Clone, Copy)]",
            "struct SchemaParameterTypes {",
            "    types: [&'static str; 3],",
            "    categories: [&'static str; 3],",
            "}",
            "",
            "const PARAMETER_TYPES: &[(u16, SchemaParameterTypes)] = &[",
        ]
    )
    for index, types in mappings:
        type_values = ", ".join(f'"{item}"' for item in types)
        category_values = ", ".join(f'"{categories[item]}"' for item in types)
        lines.append(
            f"    ({index}, SchemaParameterTypes {{ types: [{type_values}], categories: [{category_values}] }}),"
        )
    lines.extend(
        [
            "];",
            "",
            "pub(super) fn function_parameter_hint(function_index: u16) -> FunctionParameterHint {",
            "    let function_name = FUNCTION_NAMES",
            "        .binary_search_by_key(&function_index, |(index, _)| *index)",
            "        .ok()",
            "        .map(|position| FUNCTION_NAMES[position].1);",
            "    let parameter_types = PARAMETER_TYPES",
            "        .binary_search_by_key(&function_index, |(index, _)| *index)",
            "        .ok()",
            "        .map(|position| PARAMETER_TYPES[position].1);",
            "    let (mapping_status, types, categories) = match parameter_types {",
            '        Some(mapping) => ("explicit", mapping.types, mapping.categories),',
            "        None if function_name.is_some() => (",
            '            "function-enum-known-parameter-map-defaulted",',
            '            ["Unspecified"; 3],',
            '            ["unresolved"; 3],',
            "        ),",
            '        None => ("unknown-function-index", ["Unspecified"; 3], ["unresolved"; 3]),',
            "    };",
            "    FunctionParameterHint {",
            "        function_name,",
            "        mapping_status,",
            "        parameter_one_type: types[0],",
            "        parameter_one_category: categories[0],",
            "        parameter_two_type: types[1],",
            "        parameter_two_category: categories[1],",
            "        parameter_three_type: types[2],",
            "        parameter_three_category: categories[2],",
            "    }",
            "}",
            "",
            "pub(super) fn function_schema_rows() -> impl Iterator<Item = (u16, FunctionParameterHint)> {",
            "    FUNCTION_NAMES",
            "        .iter()",
            "        .map(|(function_index, _)| (*function_index, function_parameter_hint(*function_index)))",
            "}",
            "",
            "#[cfg(test)]",
            "mod tests {",
            "    use super::*;",
            "",
            "    #[test]",
            "    fn pinned_function_tables_are_complete_and_sorted() {",
            "        assert_eq!(FUNCTION_NAMES.len(), 479);",
            "        assert_eq!(PARAMETER_TYPES.len(), 220);",
            "        assert!(FUNCTION_NAMES.windows(2).all(|rows| rows[0].0 < rows[1].0));",
            "        assert!(PARAMETER_TYPES.windows(2).all(|rows| rows[0].0 < rows[1].0));",
            "    }",
            "",
            "    #[test]",
            "    fn pinned_function_names_types_and_defaulted_states_are_retained() {",
            "        let distance = function_parameter_hint(1);",
            '        assert_eq!(distance.function_name, Some("GetDistance"));',
            '        assert_eq!(distance.parameter_one_type, "ObjectReference");',
            '        assert_eq!(distance.parameter_one_category, "form");',
            '        assert_eq!(distance.parameter_two_category, "none");',
            '        assert_eq!(distance.parameter_three_category, "none");',
            "",
            "        let two_form_parameters = function_parameter_hint(60);",
            '        assert_eq!(two_form_parameters.function_name, Some("GetFactionRankDifference"));',
            '        assert_eq!(two_form_parameters.parameter_two_type, "Actor");',
            '        assert_eq!(two_form_parameters.parameter_two_category, "form");',
            "",
            "        let third_string_parameter = function_parameter_hint(576);",
            '        assert_eq!(third_string_parameter.function_name, Some("GetEventData"));',
            '        assert_eq!(third_string_parameter.parameter_three_type, "String");',
            '        assert_eq!(third_string_parameter.parameter_three_category, "string");',
            "",
            "        let defaulted = function_parameter_hint(300);",
            '        assert_eq!(defaulted.function_name, Some("IsInInterior"));',
            '        assert_eq!(',
            '            defaulted.mapping_status,',
            '            "function-enum-known-parameter-map-defaulted"',
            "        );",
            '        assert_eq!(defaulted.parameter_two_category, "unresolved");',
            '        assert_eq!(defaulted.parameter_three_category, "unresolved");',
            "",
            "        let unknown = function_parameter_hint(9000);",
            "        assert_eq!(unknown.function_name, None);",
            '        assert_eq!(unknown.mapping_status, "unknown-function-index");',
            '        assert_eq!(unknown.parameter_one_category, "unresolved");',
            "    }",
            "}",
            "",
        ]
    )
    output = ROOT / "src" / "condition" / "condition_schema.rs"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(lines), encoding="utf-8")
    print(
        f"generated {output.relative_to(ROOT)}: {len(functions)} function names, "
        f"{len(mappings)} explicit maps, {len(functions) - len(mappings)} defaulted names"
    )


if __name__ == "__main__":
    main()
