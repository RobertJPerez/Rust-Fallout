#!/usr/bin/env python3
"""Compile small Fallout 4 Papyrus sources and inspect the resulting PEX."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
LOCAL = ROOT / "local"
FIXTURES = ROOT / "tests" / "fixtures" / "papyrus-compiler"
CAPRICA_REVISION = "e4dee0860914d75e770d3f9ab374f7aba474b701"
FIXTURE_NAMES = ("CodexOperandParent", "CodexOperandChild", "CodexStructFixture")
CAPRICA_SOURCE_PATHS = (
    "CMakeLists.txt",
    "Caprica/CMakeLists.txt",
    "vcpkg.json",
    "Caprica/main.cpp",
    "Caprica/main_options.cpp",
    "Caprica/pex/PexFile.cpp",
    "Caprica/pex/PexFunction.cpp",
    "Caprica/pex/PexFunctionBuilder.h",
    "Caprica/pex/PexInstruction.cpp",
    "Caprica/pex/PexInstruction.h",
    "Caprica/pex/PexWriter.h",
    "Caprica/papyrus/PapyrusCFG.cpp",
    "Caprica/papyrus/PapyrusCompilationContext.cpp",
    "Caprica/papyrus/PapyrusIdentifier.cpp",
    "Caprica/papyrus/PapyrusResolutionContext.cpp",
    "Caprica/papyrus/expressions/PapyrusArrayIndexExpression.h",
    "Caprica/papyrus/expressions/PapyrusArrayLengthExpression.h",
    "Caprica/papyrus/expressions/PapyrusFunctionCallExpression.cpp",
    "Caprica/papyrus/expressions/PapyrusMemberAccessExpression.h",
    "Caprica/papyrus/parser/PapyrusParser.cpp",
)
MAX_PEX_BYTES = 16 * 1024 * 1024
MAX_JSON_BYTES = 64 * 1024 * 1024


class FixtureError(RuntimeError):
    pass


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def resolve_path(value: str) -> Path:
    candidate = Path(value)
    return (candidate if candidate.is_absolute() else ROOT / candidate).resolve()


def check_fixture_sources() -> dict[str, str]:
    hashes: dict[str, str] = {}
    for name in (*FIXTURE_NAMES, "ScriptObject"):
        path = FIXTURES / f"{name}.psc"
        if not path.is_file() or path.stat().st_size > 64 * 1024:
            raise FixtureError(f"missing or oversized fixture source: {path}")
        hashes[path.name] = digest_file(path)
    return hashes


def hash_caprica_sources(source_checkout: Path) -> dict[str, str]:
    hashes: dict[str, str] = {}
    for relative in CAPRICA_SOURCE_PATHS:
        path = source_checkout / relative
        if not path.is_file() or path.stat().st_size > 8 * 1024 * 1024:
            raise FixtureError(f"missing or oversized pinned Caprica source: {relative}")
        hashes[relative] = digest_file(path)
    return hashes


def identifier(value: Any, strings: list[str]) -> str:
    if not isinstance(value, dict) or set(value) != {"Identifier"}:
        raise FixtureError(f"expected an identifier operand, received {value!r}")
    index = value["Identifier"]
    if not isinstance(index, int) or isinstance(index, bool) or not 0 <= index < len(strings):
        raise FixtureError(f"identifier string index is out of range: {index!r}")
    return strings[index]


def string_value(value: Any, strings: list[str]) -> str:
    if not isinstance(value, dict) or set(value) != {"String"}:
        raise FixtureError(f"expected a string operand, received {value!r}")
    index = value["String"]
    if not isinstance(index, int) or isinstance(index, bool) or not 0 <= index < len(strings):
        raise FixtureError(f"string table index is out of range: {index!r}")
    return strings[index]


def read_text_table(document: dict[str, Any]) -> list[str]:
    raw_strings = document.get("strings")
    if not isinstance(raw_strings, list):
        raise FixtureError("PEX parser output has no string table")
    strings: list[str] = []
    try:
        for raw in raw_strings:
            if not isinstance(raw, list) or any(
                not isinstance(byte, int) or isinstance(byte, bool) or not 0 <= byte <= 255
                for byte in raw
            ):
                raise FixtureError("PEX parser returned malformed string bytes")
            strings.append(bytes(raw).decode("utf-8", errors="strict"))
    except UnicodeDecodeError as error:
        raise FixtureError("fixture identifier is not valid UTF-8") from error
    return strings


def function_named(document: dict[str, Any], strings: list[str], name: str) -> dict[str, Any]:
    matches = [
        function
        for obj in document.get("objects", [])
        for function in obj.get("functions", [])
        if identifier({"Identifier": function.get("name")}, strings) == name
    ]
    if len(matches) != 1:
        raise FixtureError(f"expected one {name} function, found {len(matches)}")
    return matches[0]


def one_instruction(function: dict[str, Any], opcode: int, context: str) -> dict[str, Any]:
    instructions = [
        instruction for instruction in function.get("instructions", []) if instruction.get("opcode") == opcode
    ]
    if len(instructions) != 1:
        raise FixtureError(f"expected one opcode {opcode} in {context}, found {len(instructions)}")
    return instructions[0]


def object_named(document: dict[str, Any], strings: list[str], name: str) -> dict[str, Any]:
    matches = [
        obj
        for obj in document.get("objects", [])
        if identifier({"Identifier": obj.get("name")}, strings).casefold() == name.casefold()
    ]
    if len(matches) != 1:
        raise FixtureError(f"expected one {name} object, found {len(matches)}")
    return matches[0]


def function_in_object(obj: dict[str, Any], strings: list[str], name: str) -> dict[str, Any]:
    matches = [
        function
        for function in obj.get("functions", [])
        if identifier({"Identifier": function.get("name")}, strings).casefold() == name.casefold()
    ]
    if len(matches) != 1:
        raise FixtureError(f"expected one {name} declaration, found {len(matches)}")
    return matches[0]


def declaration_types(declarations: Any, strings: list[str], context: str) -> list[tuple[str, str]]:
    if not isinstance(declarations, list):
        raise FixtureError(f"{context} declarations are malformed")
    result: list[tuple[str, str]] = []
    for declaration in declarations:
        if not isinstance(declaration, list) or len(declaration) != 2:
            raise FixtureError(f"{context} declaration is malformed")
        name, type_name = declaration
        result.append((identifier({"Identifier": name}, strings), identifier({"Identifier": type_name}, strings)))
    return result


def variable_types(function: dict[str, Any], strings: list[str]) -> dict[str, str]:
    variables: dict[str, str] = {}
    for field in ("parameters", "locals"):
        for name, type_name in declaration_types(function.get(field, []), strings, field):
            key = name.casefold()
            previous = variables.setdefault(key, type_name)
            if previous.casefold() != type_name.casefold():
                raise FixtureError(f"caller variable {name} has conflicting PEX types")
    return variables


def operand_type(value: Any, strings: list[str], variables: dict[str, str]) -> str:
    if isinstance(value, dict) and len(value) == 1:
        tag, payload = next(iter(value.items()))
        if tag == "Identifier":
            name = identifier(value, strings)
            type_name = variables.get(name.casefold())
            if type_name is None:
                raise FixtureError(f"cannot infer PEX type for call argument identifier {name}")
            return type_name
        if tag == "String":
            string_value(value, strings)
            return "String"
        if tag == "Integer" and isinstance(payload, int) and not isinstance(payload, bool):
            return "Int"
        if tag == "FloatBits" and isinstance(payload, int) and not isinstance(payload, bool) and 0 <= payload <= 0xFFFFFFFF:
            return "Float"
        if tag == "BoolByte" and isinstance(payload, int) and not isinstance(payload, bool) and 0 <= payload <= 0xFF:
            return "Bool"
    raise FixtureError(f"unsupported PEX value in compiler call signature check: {value!r}")


def check_call_argument_types(
    values: Any,
    expected_types: list[str],
    strings: list[str],
    variables: dict[str, str],
    context: str,
) -> list[str]:
    if not isinstance(values, list) or len(values) != len(expected_types):
        actual_count = len(values) if isinstance(values, list) else "malformed"
        raise FixtureError(f"{context} has {actual_count} arguments for {len(expected_types)} declared parameters")
    actual_types = [operand_type(value, strings, variables) for value in values]
    if [type_name.casefold() for type_name in actual_types] != [
        type_name.casefold() for type_name in expected_types
    ]:
        raise FixtureError(f"{context} argument types {actual_types!r} differ from declaration {expected_types!r}")
    return actual_types


def validate_call_signatures(child_document: dict[str, Any], parent_document: dict[str, Any]) -> dict[str, Any]:
    child_strings = read_text_table(child_document)
    parent_strings = read_text_table(parent_document)
    child = object_named(child_document, child_strings, "CodexOperandChild")
    parent = object_named(parent_document, parent_strings, "CodexOperandParent")
    declared_parent = identifier({"Identifier": child.get("parent")}, child_strings)
    parent_name = identifier({"Identifier": parent.get("name")}, parent_strings)
    if declared_parent.casefold() != parent_name.casefold():
        raise FixtureError("CALLPARENT fixture base name differs from the compiled parent declaration")

    checked_calls: list[dict[str, Any]] = []

    def check_call(
        caller_name: str,
        instruction: dict[str, Any],
        target_name: str,
        target: dict[str, Any],
        context: str,
        receiver: Any | None = None,
    ) -> None:
        caller = function_in_object(child, child_strings, caller_name)
        arguments = instruction.get("arguments")
        values = instruction.get("varargs")
        if not isinstance(arguments, list):
            raise FixtureError(f"{context} fixed operands are malformed")
        parameters = declaration_types(target.get("parameters", []), parent_strings, f"{context} target parameters")
        expected_types = [type_name for _, type_name in parameters]
        caller_variables = variable_types(caller, child_strings)
        actual_types = check_call_argument_types(values, expected_types, child_strings, caller_variables, context)

        if receiver is not None:
            receiver_name = identifier(receiver, child_strings)
            receiver_type = caller_variables.get(receiver_name.casefold())
            if receiver_type is None or receiver_type.casefold() != parent_name.casefold():
                raise FixtureError(f"{context} receiver type does not match {parent_name}")

        result_position = 2 if instruction.get("opcode") in (23, 25) else 1
        if len(arguments) <= result_position:
            raise FixtureError(f"{context} result operand is missing")
        return_type_index = target.get("return_type")
        return_type = identifier({"Identifier": return_type_index}, parent_strings)
        result_name = identifier(arguments[result_position], child_strings)
        if return_type.casefold() == "none":
            if result_name.casefold() != "::nonevar":
                raise FixtureError(f"{context} void result does not use ::nonevar")
        elif caller_variables.get(result_name.casefold(), "").casefold() != return_type.casefold():
            raise FixtureError(f"{context} result variable does not match return type {return_type}")

        checked_calls.append(
            {
                "kind": {23: "CALLMETHOD", 24: "CALLPARENT", 25: "CALLSTATIC"}.get(
                    instruction.get("opcode"), "UNKNOWN"
                ),
                "caller": caller_name,
                "target": f"{parent_name}.{target_name}",
                "argument_types": actual_types,
                "declared_parameter_types": expected_types,
            }
        )

    for caller_name, target_name, opcode in (
        ("CallStatic", "StaticTarget", 25),
        ("CallMethod", "MethodTarget", 23),
        ("ParentTarget", "ParentTarget", 24),
    ):
        caller = function_in_object(child, child_strings, caller_name)
        instruction = one_instruction(caller, opcode, f"{caller_name} signature check")
        arguments = instruction.get("arguments")
        expected_fixed_count = 3 if opcode in (23, 25) else 2
        if not isinstance(arguments, list) or len(arguments) != expected_fixed_count:
            raise FixtureError(f"{caller_name} call has malformed fixed operands")
        if opcode == 25:
            called_class = identifier(arguments[0], child_strings)
            if called_class.casefold() != parent_name.casefold():
                raise FixtureError("CALLSTATIC class operand differs from the compiled parent class")
        if opcode in (23, 25):
            called_name = identifier(arguments[0 if opcode == 23 else 1], child_strings)
        else:
            called_name = identifier(arguments[0], child_strings)
        if called_name.casefold() != target_name.casefold():
            raise FixtureError(f"{caller_name} function operand differs from the expected target")
        target = function_in_object(parent, parent_strings, target_name)
        check_call(
            caller_name,
            instruction,
            target_name,
            target,
            f"opcode {opcode} {caller_name}",
            receiver=arguments[1] if opcode == 23 else None,
        )

    branch_caller = function_in_object(child, child_strings, "CallBranch")
    branch_static_calls = [row for row in branch_caller.get("instructions", []) if row.get("opcode") == 25]
    if len(branch_static_calls) != 2:
        raise FixtureError(f"CallBranch must contain two CALLSTATIC instructions, found {len(branch_static_calls)}")
    target = function_in_object(parent, parent_strings, "StaticTarget")
    for index, instruction in enumerate(branch_static_calls):
        arguments = instruction.get("arguments")
        if not isinstance(arguments, list) or len(arguments) != 3:
            raise FixtureError(f"CallBranch CALLSTATIC {index} has malformed fixed operands")
        if identifier(arguments[0], child_strings).casefold() != parent_name.casefold():
            raise FixtureError(f"CallBranch CALLSTATIC {index} has an unexpected class operand")
        if identifier(arguments[1], child_strings).casefold() != "statictarget":
            raise FixtureError(f"CallBranch CALLSTATIC {index} has an unexpected function operand")
        check_call("CallBranch", instruction, "StaticTarget", target, f"CALLSTATIC CallBranch[{index}]")

    return {
        "calls_checked": checked_calls,
        "scope": "Compiler-emitted argument value/type tags and caller variable declarations agree with target PEX parameter types for these fixtures; no rejection policy or VM dispatch is asserted",
    }


def validate_child_pex(document: dict[str, Any]) -> dict[str, Any]:
    header = document.get("header", {})
    if (header.get("major"), header.get("minor"), header.get("game")) != (3, 9, 2):
        raise FixtureError("compiler output is not Fallout 4 PEX 3.9/game 2")
    strings = read_text_table(document)
    child_objects = [obj for obj in document.get("objects", []) if identifier({"Identifier": obj.get("name")}, strings) == "CodexOperandChild"]
    if len(child_objects) != 1:
        raise FixtureError(f"expected one CodexOperandChild object, found {len(child_objects)}")
    child = child_objects[0]
    if identifier({"Identifier": child.get("parent")}, strings).casefold() != "codexoperandparent":
        raise FixtureError("child PEX does not retain the fixture parent class")

    static = one_instruction(function_named(document, strings, "CallStatic"), 25, "CallStatic")
    static_args = static.get("arguments")
    static_varargs = static.get("varargs")
    if not isinstance(static_args, list) or len(static_args) != 3:
        raise FixtureError("CALLSTATIC must carry class, function and result operands")
    if [identifier(value, strings) for value in static_args] != ["codexOperandParent", "StaticTarget", "::nonevar"]:
        raise FixtureError("CALLSTATIC operand order or values differ from the compiler fixture")
    if not isinstance(static_varargs, list) or len(static_varargs) != 2:
        raise FixtureError("CALLSTATIC must retain both source arguments")
    if [identifier(static_varargs[0], strings), string_value(static_varargs[1], strings)] != ["value", "static"]:
        raise FixtureError("CALLSTATIC varargs do not match the source call")

    method = one_instruction(function_named(document, strings, "CallMethod"), 23, "CallMethod")
    method_args = method.get("arguments")
    method_varargs = method.get("varargs")
    if not isinstance(method_args, list) or len(method_args) != 3:
        raise FixtureError("CALLMETHOD must carry function, receiver and result operands")
    if [identifier(value, strings) for value in method_args] != ["MethodTarget", "receiver", "::nonevar"]:
        raise FixtureError("CALLMETHOD operand order or values differ from the compiler fixture")
    if not isinstance(method_varargs, list) or len(method_varargs) != 2:
        raise FixtureError("CALLMETHOD must retain both source arguments")
    if method_varargs != [{"Integer": 17}, {"FloatBits": 0x40200000}]:
        raise FixtureError("CALLMETHOD varargs differ from the integer and 2.5 source arguments")

    parent = one_instruction(function_named(document, strings, "ParentTarget"), 24, "ParentTarget")
    parent_args = parent.get("arguments")
    parent_varargs = parent.get("varargs")
    if not isinstance(parent_args, list) or len(parent_args) != 2:
        raise FixtureError("CALLPARENT must carry function and result operands")
    if [identifier(value, strings) for value in parent_args] != ["ParentTarget", "::nonevar"]:
        raise FixtureError("CALLPARENT operand order or values differ from the compiler fixture")
    if not isinstance(parent_varargs, list) or len(parent_varargs) != 1:
        raise FixtureError("CALLPARENT must retain its source argument")
    if identifier(parent_varargs[0], strings) != "value":
        raise FixtureError("CALLPARENT varargs differ from the source call")

    branch_function = function_named(document, strings, "CallBranch")
    branch_instructions = branch_function.get("instructions", [])
    branch_targets: list[dict[str, int]] = []
    for pc, instruction in enumerate(branch_instructions):
        opcode = instruction.get("opcode")
        if opcode not in (20, 21, 22):
            continue
        args = instruction.get("arguments")
        displacement_index = 0 if opcode == 20 else 1
        if not isinstance(args, list) or len(args) <= displacement_index:
            raise FixtureError(f"malformed branch operands at instruction {pc}")
        displacement = args[displacement_index]
        if not isinstance(displacement, dict) or set(displacement) != {"Integer"}:
            raise FixtureError(f"branch displacement is not an integer at instruction {pc}")
        delta = displacement["Integer"]
        if not isinstance(delta, int) or isinstance(delta, bool):
            raise FixtureError(f"branch displacement is malformed at instruction {pc}")
        target = pc + delta
        if not 0 <= target <= len(branch_instructions):
            raise FixtureError(f"branch target {target} lies outside the function")
        branch_targets.append({"pc": pc, "opcode": opcode, "target_instruction_or_end": target})
    branch_opcodes = {row["opcode"] for row in branch_targets}
    if not {20, 22}.issubset(branch_opcodes):
        raise FixtureError("conditional fixture did not emit JUMPF and JUMP instructions")

    return {
        "object": "CodexOperandChild",
        "calls": {
            "CALLMETHOD": {"opcode": 23, "arguments": ["MethodTarget", "receiver", "::nonevar"], "varargs": [17, "2.5(float bits 0x40200000)"]},
            "CALLPARENT": {"opcode": 24, "arguments": ["ParentTarget", "::nonevar"], "varargs": ["value"]},
            "CALLSTATIC": {"opcode": 25, "arguments": ["codexOperandParent", "StaticTarget", "::nonevar"], "varargs": ["value", "static"]},
        },
        "branch_targets": branch_targets,
        "scope": "Compiler-authored FO4 PEX operand positions and relative branch targets; no VM dispatch or continuation behavior asserted",
    }


def validate_struct_array_pex(document: dict[str, Any]) -> dict[str, Any]:
    header = document.get("header", {})
    if (header.get("major"), header.get("minor"), header.get("game")) != (3, 9, 2):
        raise FixtureError("struct/array output is not Fallout 4 PEX 3.9/game 2")
    strings = read_text_table(document)
    objects = [
        obj for obj in document.get("objects", [])
        if identifier({"Identifier": obj.get("name")}, strings) == "CodexStructFixture"
    ]
    if len(objects) != 1:
        raise FixtureError(f"expected one CodexStructFixture object, found {len(objects)}")
    obj = objects[0]
    definitions = obj.get("struct_definitions")
    if not isinstance(definitions, list) or len(definitions) != 1:
        raise FixtureError("expected one Payload struct definition")
    struct = definitions[0]
    if identifier({"Identifier": struct.get("name")}, strings) != "Payload":
        raise FixtureError("compiler output lost the Payload struct name")
    members = struct.get("members")
    if not isinstance(members, list):
        raise FixtureError("Payload struct members are malformed")
    member_rows = [(identifier({"Identifier": row.get("name")}, strings), identifier({"Identifier": row.get("type_name")}, strings)) for row in members]
    if member_rows != [("identifier", "Int"), ("label", "String")]:
        raise FixtureError(f"Payload member order/types differ from source: {member_rows!r}")

    read_payload = function_named(document, strings, "ReadPayload")
    parameters = read_payload.get("parameters")
    if not isinstance(parameters, list) or len(parameters) != 1 or len(parameters[0]) != 2:
        raise FixtureError("ReadPayload must retain one typed struct parameter")
    parameter_name, parameter_type = (identifier({"Identifier": value}, strings) for value in parameters[0])
    if parameter_name != "value" or parameter_type.casefold() != "codexstructfixture#payload":
        raise FixtureError("ReadPayload struct parameter type differs from the compiler fixture")
    struct_get = one_instruction(read_payload, 38, "ReadPayload STRUCTGET")
    get_args = struct_get.get("arguments")
    if not isinstance(get_args, list) or len(get_args) != 3:
        raise FixtureError("STRUCTGET must carry destination, struct value and member operands")
    if identifier(get_args[1], strings) != "value" or identifier(get_args[2], strings) != "identifier":
        raise FixtureError("STRUCTGET operand order differs from the compiler fixture")
    struct_set = one_instruction(read_payload, 39, "ReadPayload STRUCTSET")
    set_args = struct_set.get("arguments")
    if not isinstance(set_args, list) or len(set_args) != 3:
        raise FixtureError("STRUCTSET must carry struct value, member and assigned value operands")
    if [identifier(set_args[0], strings), identifier(set_args[1], strings), string_value(set_args[2], strings)] != [
        "value", "label", "updated"
    ]:
        raise FixtureError("STRUCTSET operand order or values differ from the compiler fixture")

    read_array = function_named(document, strings, "ReadArray")
    array_length = one_instruction(read_array, 31, "ReadArray ARRAYLENGTH")
    length_args = array_length.get("arguments")
    if not isinstance(length_args, list) or len(length_args) != 2 or identifier(length_args[1], strings) != "values":
        raise FixtureError("ARRAYLENGTH operand order differs from the compiler fixture")
    array_get = one_instruction(read_array, 32, "ReadArray ARRAYGETELEMENT")
    get_element_args = array_get.get("arguments")
    if not isinstance(get_element_args, list) or len(get_element_args) != 3:
        raise FixtureError("ARRAYGETELEMENT must carry destination, array and index operands")
    if identifier(get_element_args[1], strings) != "values" or get_element_args[2] != {"Integer": 0}:
        raise FixtureError("ARRAYGETELEMENT operand order or index differs from the compiler fixture")
    array_set = one_instruction(read_array, 33, "ReadArray ARRAYSETELEMENT")
    set_element_args = array_set.get("arguments")
    if not isinstance(set_element_args, list) or len(set_element_args) != 3:
        raise FixtureError("ARRAYSETELEMENT must carry array, index and value operands")
    if identifier(set_element_args[0], strings) != "values" or set_element_args[1] != {"Integer": 0} or identifier(set_element_args[2], strings) != "arrayCount":
        raise FixtureError("ARRAYSETELEMENT operand order or index differs from the compiler fixture")

    return {
        "object": "CodexStructFixture",
        "struct": {"name": "Payload", "members": member_rows},
        "struct_opcodes": {
            "STRUCTGET": {"opcode": 38, "operands": ["destination", "struct value", "member"]},
            "STRUCTSET": {"opcode": 39, "operands": ["struct value", "member", "assigned value"]},
        },
        "array_opcodes": {
            "ARRAYLENGTH": {"opcode": 31, "operands": ["destination", "array"]},
            "ARRAYGETELEMENT": {"opcode": 32, "operands": ["destination", "array", "index"]},
            "ARRAYSETELEMENT": {"opcode": 33, "operands": ["array", "index", "value"]},
        },
        "scope": "Compiler-authored FO4 PEX struct/array wire operands only; no mutation, aliasing or VM execution semantics asserted",
    }


def run_command(command: list[str], cwd: Path, env: dict[str, str], timeout: int) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout, check=False)
    except subprocess.TimeoutExpired as error:
        raise FixtureError(f"timed out after {timeout} seconds: {command[0]}") from error


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", default="local/caprica-build-001/Caprica/Caprica.exe")
    parser.add_argument("--source-checkout", default="local/research/Caprica")
    parser.add_argument("--rust-parser", default="local/target/debug/fallout4-prep.exe")
    parser.add_argument("--output-dir", required=True, help="new directory under local/ for generated PEX and reports")
    args = parser.parse_args()

    compiler = resolve_path(args.compiler)
    source_checkout = resolve_path(args.source_checkout)
    rust_parser = resolve_path(args.rust_parser)
    output = resolve_path(args.output_dir)
    if not compiler.is_file() or not rust_parser.is_file():
        raise FixtureError("compiler and Rust parser binaries must already be built")
    for label, path in (
        ("compiler", compiler),
        ("Caprica source checkout", source_checkout),
        ("Rust parser", rust_parser),
    ):
        if not path.is_relative_to(LOCAL.resolve()):
            raise FixtureError(f"{label} must remain under ignored local/: {path}")
    if not output.is_relative_to(LOCAL.resolve()):
        raise FixtureError("output directory must remain under ignored local/")
    if output.exists():
        raise FixtureError(f"refusing to overwrite existing output directory: {output}")
    output.mkdir(parents=True)

    revision = run_command(["git", "rev-parse", "HEAD"], source_checkout, os.environ.copy(), 30)
    if revision.returncode != 0 or revision.stdout.strip() != CAPRICA_REVISION:
        raise FixtureError("Caprica checkout revision differs from the pinned compiler source")
    dirty = run_command(["git", "status", "--porcelain"], source_checkout, os.environ.copy(), 30)
    if dirty.returncode != 0 or dirty.stdout.strip():
        raise FixtureError("Caprica research checkout must remain clean")

    fixture_hashes = check_fixture_sources()
    caprica_source_hashes = hash_caprica_sources(source_checkout)
    compiler_hash = digest_file(compiler)
    parser_hash = digest_file(rust_parser)
    empty_flags = output / "empty-user-flags.flg"
    empty_flags.write_bytes(b"")
    compiled_dir = output / "compiled"
    compiled_dir.mkdir()

    env = os.environ.copy()
    dependency_bin = ROOT / "local" / "caprica-deps-001" / "installed" / "x64-windows" / "bin"
    env["PATH"] = str(dependency_bin) + os.pathsep + env.get("PATH", "")
    compiler_command = [
        str(compiler),
        "--game=fallout4",
        f"--import={FIXTURES}",
        f"--flags={empty_flags}",
        f"--output={compiled_dir}",
        "--quiet",
        "--strict=true",
        *(str(FIXTURES / f"{name}.psc") for name in FIXTURE_NAMES),
    ]
    compiler_run = run_command(compiler_command, ROOT, env, 180)
    (output / "compiler.stdout.txt").write_text(compiler_run.stdout, encoding="utf-8")
    (output / "compiler.stderr.txt").write_text(compiler_run.stderr, encoding="utf-8")
    if compiler_run.returncode != 0:
        raise FixtureError(f"Caprica returned {compiler_run.returncode}; inspect local compiler logs")

    parsed: dict[str, dict[str, Any]] = {}
    for name in FIXTURE_NAMES:
        pex_path = compiled_dir / f"{name}.pex"
        if not pex_path.is_file() or pex_path.stat().st_size > MAX_PEX_BYTES:
            raise FixtureError(f"missing or oversized compiler output: {pex_path.name}")
        parser_run = run_command([str(rust_parser), "pex", str(pex_path)], ROOT, env, 60)
        if parser_run.returncode != 0 or len(parser_run.stdout.encode("utf-8")) > MAX_JSON_BYTES:
            raise FixtureError(f"Rust parser rejected {pex_path.name}: {parser_run.stderr.strip()}")
        try:
            document = json.loads(parser_run.stdout)
        except json.JSONDecodeError as error:
            raise FixtureError(f"Rust parser emitted invalid JSON for {pex_path.name}") from error
        parsed[name] = document
        (output / f"{name}.parsed.json").write_text(
            json.dumps(document, indent=2, ensure_ascii=True) + "\n", encoding="utf-8"
        )

    child_result = validate_child_pex(parsed["CodexOperandChild"])
    call_signature_result = validate_call_signatures(
        parsed["CodexOperandChild"], parsed["CodexOperandParent"]
    )
    struct_array_result = validate_struct_array_pex(parsed["CodexStructFixture"])
    final_fixture_hashes = check_fixture_sources()
    if fixture_hashes != final_fixture_hashes:
        raise FixtureError("fixture sources changed while compiling")
    if caprica_source_hashes != hash_caprica_sources(source_checkout):
        raise FixtureError("pinned Caprica sources changed while compiling")
    if digest_file(compiler) != compiler_hash or digest_file(rust_parser) != parser_hash:
        raise FixtureError("compiler or Rust parser binary changed while verifying")

    report = {
        "schema": 1,
        "status": "compiler-authored-fo4-pex-calls-branches-structs-and-arrays-verified",
        "compiler_source": {
            "revision": CAPRICA_REVISION,
            "working_tree": "clean",
            "binary": str(compiler.relative_to(ROOT)),
            "binary_sha256": compiler_hash,
            "source_sha256": caprica_source_hashes,
            "cmake_compatibility_hook": {
                "path": "tools/cmake/caprica-pugixml-compat.cmake",
                "sha256": digest_file(ROOT / "tools" / "cmake" / "caprica-pugixml-compat.cmake"),
            },
        },
        "rust_parser": {"binary": str(rust_parser.relative_to(ROOT)), "binary_sha256": parser_hash},
        "fixture_sources": fixture_hashes,
        "generated_pex": {
            name: {
                "sha256": digest_file(compiled_dir / f"{name}.pex"),
                "bytes": (compiled_dir / f"{name}.pex").stat().st_size,
            }
            for name in FIXTURE_NAMES
        },
        "compile_command": [Path(compiler_command[0]).relative_to(ROOT).as_posix(), *compiler_command[1:]],
        "verification": {
            "calls_and_branches": child_result,
            "call_signature_consistency": call_signature_result,
            "structs_and_arrays": struct_array_result,
        },
        "runtime_ready": False,
    }
    report_bytes = (json.dumps(report, indent=2, ensure_ascii=True) + "\n").encode("utf-8")
    (output / "verification.json").write_bytes(report_bytes)
    completion = {
        "schema": 1,
        "status": "complete",
        "verification_sha256": hashlib.sha256(report_bytes).hexdigest(),
        "runtime_ready": False,
    }
    (output / "complete.json").write_text(json.dumps(completion, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({
        "status": report["status"],
        "output": str(output.relative_to(ROOT)),
        "branch_targets": child_result["branch_targets"],
        "call_signature_checks": len(call_signature_result["calls_checked"]),
        "struct_opcodes": struct_array_result["struct_opcodes"],
        "array_opcodes": struct_array_result["array_opcodes"],
    }, indent=2))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except FixtureError as error:
        print(f"fixture verification failed: {error}", file=sys.stderr)
        raise SystemExit(1)
