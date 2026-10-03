"""Check candidate proof boundaries without executing or accepting game behavior."""
import unittest
from unittest.mock import patch

import integration_operand_audit as operand_audit


class IntegrationGuards(unittest.TestCase):
    def test_catch_all_negative_cannot_accept_an_unrelated_failure(self):
        def altered(inputs):
            try:
                operand_audit.operands.compare(*inputs)
            except RuntimeError:
                return ["saved_local_bits"]

        with patch.object(operand_audit.operands, "compare", side_effect=RuntimeError("Missing input")) as original:
            with patch.object(operand_audit.operands, "negative_checks", side_effect=altered):
                with self.assertRaisesRegex(AssertionError, "unrelated reason"):
                    operand_audit.checked_negatives([])
            self.assertIs(operand_audit.operands.compare, original)

    def test_matching_failure_does_not_certify_omitted_negative_cases(self):
        def altered(inputs):
            try:
                operand_audit.operands.compare(*inputs)
            except RuntimeError:
                return ["saved_local_bits"]

        with patch.object(operand_audit.operands, "compare", side_effect=RuntimeError("Saved storage association differs at event 1")) as original:
            with patch.object(operand_audit.operands, "negative_checks", side_effect=altered):
                with self.assertRaisesRegex(RuntimeError, "coverage/order differs"):
                    operand_audit.checked_negatives([])
            self.assertIs(operand_audit.operands.compare, original)


if __name__ == "__main__":
    unittest.main()
