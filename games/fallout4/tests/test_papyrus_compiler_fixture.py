import copy
import sys
import unittest
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import verify_papyrus_compiler_fixture as verifier


def id_value(index):
    return {"Identifier": index}


def function(name_index, instructions):
    return {"name": name_index, "instructions": instructions}


def instruction(opcode, arguments, varargs=None):
    return {"opcode": opcode, "arguments": arguments, "varargs": varargs or []}


def valid_document():
    strings = [
        "CodexOperandChild",
        "CodexOperandParent",
        "",
        "CallStatic",
        "None",
        "value",
        "Int",
        "::nonevar",
        "self",
        "static",
        "codexOperandParent",
        "StaticTarget",
        "CallMethod",
        "receiver",
        "MethodTarget",
        "ParentTarget",
        "CallBranch",
        "Bool",
        "::temp0",
        "true",
        "false",
    ]
    child = {
        "name": 0,
        "parent": 1,
        "functions": [
            function(3, [instruction(25, [id_value(10), id_value(11), id_value(7)], [id_value(5), {"String": 9}])]),
            function(12, [instruction(23, [id_value(14), id_value(13), id_value(7)], [{"Integer": 17}, {"FloatBits": 0x40200000}])]),
            function(15, [instruction(24, [id_value(15), id_value(7)], [id_value(5)])]),
            function(
                16,
                [
                    instruction(18, [id_value(18), id_value(5), {"Integer": 0}]),
                    instruction(22, [id_value(18), {"Integer": 3}]),
                    instruction(25, [id_value(10), id_value(11), id_value(7)], [id_value(5), {"String": 19}]),
                    instruction(20, [{"Integer": 2}]),
                    instruction(25, [id_value(10), id_value(11), id_value(7)], [{"Integer": 0}, {"String": 20}]),
                ],
            ),
        ],
    }
    return {
        "header": {"major": 3, "minor": 9, "game": 2},
        "strings": [list(value.encode("utf-8")) for value in strings],
        "objects": [child],
    }


def valid_struct_array_document():
    strings = [
        "CodexStructFixture",
        "ScriptObject",
        "",
        "Payload",
        "identifier",
        "Int",
        "label",
        "String",
        "ReadPayload",
        "None",
        "value",
        "codexStructFixture#Payload",
        "copiedIdentifier",
        "self",
        "::temp0",
        "updated",
        "ReadArray",
        "values",
        "arrayCount",
        "first",
    ]
    read_payload = function(
        8,
        [
            instruction(38, [id_value(14), id_value(10), id_value(4)]),
            instruction(13, [id_value(12), id_value(14)]),
            instruction(39, [id_value(10), id_value(6), {"String": 15}]),
        ],
    )
    read_payload["parameters"] = [[10, 11]]
    read_array = function(
        16,
        [
            instruction(31, [id_value(14), id_value(17)]),
            instruction(13, [id_value(18), id_value(14)]),
            instruction(32, [id_value(14), id_value(17), {"Integer": 0}]),
            instruction(13, [id_value(19), id_value(14)]),
            instruction(33, [id_value(17), {"Integer": 0}, id_value(18)]),
        ],
    )
    return {
        "header": {"major": 3, "minor": 9, "game": 2},
        "strings": [list(value.encode("utf-8")) for value in strings],
        "objects": [
            {
                "name": 0,
                "parent": 1,
                "struct_definitions": [
                    {
                        "name": 3,
                        "members": [
                            {"name": 4, "type_name": 5},
                            {"name": 6, "type_name": 7},
                        ],
                    }
                ],
                "functions": [read_payload, read_array],
            }
        ],
    }


def valid_call_signature_documents():
    parent_strings = [
        "CodexOperandParent",
        "ScriptObject",
        "StaticTarget",
        "None",
        "integerValue",
        "Int",
        "stringValue",
        "String",
        "MethodTarget",
        "floatValue",
        "Float",
        "ParentTarget",
    ]
    parent = {
        "strings": [list(value.encode("utf-8")) for value in parent_strings],
        "objects": [
            {
                "name": 0,
                "parent": 1,
                "functions": [
                    {"name": 2, "return_type": 3, "parameters": [[4, 5], [6, 7]]},
                    {"name": 8, "return_type": 3, "parameters": [[4, 5], [9, 10]]},
                    {"name": 11, "return_type": 3, "parameters": [[4, 5]]},
                ],
            }
        ],
    }
    child_strings = [
        "CodexOperandChild",
        "CodexOperandParent",
        "CallStatic",
        "CallMethod",
        "ParentTarget",
        "CallBranch",
        "value",
        "Int",
        "receiver",
        "CodexOperandParent",
        "::nonevar",
        "StaticTarget",
        "static",
        "MethodTarget",
        "true",
        "false",
    ]
    call = lambda opcode, arguments, varargs: instruction(opcode, arguments, varargs)
    child = {
        "strings": [list(value.encode("utf-8")) for value in child_strings],
        "objects": [
            {
                "name": 0,
                "parent": 1,
                "functions": [
                    {
                        "name": 2,
                        "parameters": [[6, 7]],
                        "instructions": [call(25, [id_value(1), id_value(11), id_value(10)], [id_value(6), {"String": 12}])],
                    },
                    {
                        "name": 3,
                        "parameters": [[8, 9]],
                        "instructions": [call(23, [id_value(13), id_value(8), id_value(10)], [{"Integer": 17}, {"FloatBits": 0x40200000}])],
                    },
                    {
                        "name": 4,
                        "parameters": [[6, 7]],
                        "instructions": [call(24, [id_value(4), id_value(10)], [id_value(6)])],
                    },
                    {
                        "name": 5,
                        "parameters": [[6, 7]],
                        "instructions": [
                            call(25, [id_value(1), id_value(11), id_value(10)], [id_value(6), {"String": 14}]),
                            call(25, [id_value(1), id_value(11), id_value(10)], [{"Integer": 0}, {"String": 15}]),
                        ],
                    },
                ],
            }
        ],
    }
    return child, parent


class PapyrusCompilerFixtureTests(unittest.TestCase):
    def test_valid_call_operands_and_end_boundary(self):
        result = verifier.validate_child_pex(valid_document())
        self.assertEqual(
            result["branch_targets"],
            [
                {"pc": 1, "opcode": 22, "target_instruction_or_end": 4},
                {"pc": 3, "opcode": 20, "target_instruction_or_end": 5},
            ],
        )

    def test_rejects_out_of_range_string_index(self):
        with self.assertRaises(verifier.FixtureError):
            verifier.identifier(id_value(999), ["only"])

    def test_rejects_changed_call_operand_order(self):
        document = copy.deepcopy(valid_document())
        document["objects"][0]["functions"][0]["instructions"][0]["arguments"].reverse()
        with self.assertRaisesRegex(verifier.FixtureError, "CALLSTATIC operand order"):
            verifier.validate_child_pex(document)

    def test_rejects_branch_past_one_past_end_boundary(self):
        document = copy.deepcopy(valid_document())
        jump = document["objects"][0]["functions"][3]["instructions"][3]
        jump["arguments"][0] = {"Integer": 3}
        with self.assertRaisesRegex(verifier.FixtureError, "lies outside the function"):
            verifier.validate_child_pex(document)

    def test_valid_struct_and_array_wire_operands(self):
        result = verifier.validate_struct_array_pex(valid_struct_array_document())
        self.assertEqual(result["struct"]["members"], [("identifier", "Int"), ("label", "String")])
        self.assertEqual(
            [row["opcode"] for row in result["array_opcodes"].values()],
            [31, 32, 33],
        )

    def test_rejects_struct_member_operand_order_change(self):
        document = copy.deepcopy(valid_struct_array_document())
        get_instruction = document["objects"][0]["functions"][0]["instructions"][0]
        get_instruction["arguments"][1] = id_value(4)
        with self.assertRaisesRegex(verifier.FixtureError, "STRUCTGET operand order"):
            verifier.validate_struct_array_pex(document)

    def test_rejects_array_index_encoded_as_identifier(self):
        document = copy.deepcopy(valid_struct_array_document())
        get_instruction = document["objects"][0]["functions"][1]["instructions"][2]
        get_instruction["arguments"][2] = id_value(19)
        with self.assertRaisesRegex(verifier.FixtureError, "ARRAYGETELEMENT operand order"):
            verifier.validate_struct_array_pex(document)

    def test_call_signature_accepts_matching_identifier_and_literal_types(self):
        actual = verifier.check_call_argument_types(
            [id_value(0), {"String": 1}],
            ["Int", "String"],
            ["value", "static"],
            {"value": "Int"},
            "CALLSTATIC fixture",
        )
        self.assertEqual(actual, ["Int", "String"])

    def test_call_signature_rejects_wrong_argument_type(self):
        with self.assertRaisesRegex(verifier.FixtureError, "argument types"):
            verifier.check_call_argument_types(
                [{"String": 0}], ["Int"], ["wrong"], {}, "CALLMETHOD fixture"
            )

    def test_call_signature_rejects_wrong_argument_count(self):
        with self.assertRaisesRegex(verifier.FixtureError, "has 1 arguments for 2"):
            verifier.check_call_argument_types(
                [{"Integer": 1}], ["Int", "String"], [], {}, "CALLSTATIC fixture"
            )

    def test_call_signature_rejects_unresolved_identifier_type(self):
        with self.assertRaisesRegex(verifier.FixtureError, "cannot infer PEX type"):
            verifier.check_call_argument_types(
                [id_value(0)], ["Int"], ["missing"], {}, "CALLPARENT fixture"
            )

    def test_compiler_call_signatures_match_target_declarations(self):
        child, parent = valid_call_signature_documents()
        result = verifier.validate_call_signatures(child, parent)
        self.assertEqual(len(result["calls_checked"]), 5)
        self.assertEqual(
            [row["kind"] for row in result["calls_checked"]],
            ["CALLSTATIC", "CALLMETHOD", "CALLPARENT", "CALLSTATIC", "CALLSTATIC"],
        )

    def test_compiler_call_signature_rejects_changed_target_type(self):
        child, parent = valid_call_signature_documents()
        parent["objects"][0]["functions"][0]["parameters"][0][1] = 10
        with self.assertRaisesRegex(verifier.FixtureError, "argument types"):
            verifier.validate_call_signatures(child, parent)

    def test_compiler_call_signature_rejects_wrong_call_arity(self):
        child, parent = valid_call_signature_documents()
        child["objects"][0]["functions"][0]["instructions"][0]["varargs"].pop()
        with self.assertRaisesRegex(verifier.FixtureError, "has 1 arguments for 2"):
            verifier.validate_call_signatures(child, parent)


if __name__ == "__main__":
    unittest.main()
