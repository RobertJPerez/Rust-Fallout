"""The focused build slot must survive a brief Windows status-file open race."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).with_name("team-v2-focused.py")
SPEC = importlib.util.spec_from_file_location("team_v2_focused", SOURCE)
assert SPEC is not None and SPEC.loader is not None
focused = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(focused)


class CoordinationReadTests(unittest.TestCase):
    def test_transient_status_open_race_recovers(self) -> None:
        with (
            patch.object(
                Path,
                "read_text",
                side_effect=[PermissionError("sharing violation"), '{"lane":"scripts"}'],
            ) as read,
            patch.object(focused.time, "sleep") as sleep,
        ):
            self.assertEqual(focused.read_json(Path("status.json")), {"lane": "scripts"})
        self.assertEqual(read.call_count, 2)
        sleep.assert_called_once_with(focused.READ_RETRY_SECONDS)

    def test_persistent_missing_file_remains_a_failure(self) -> None:
        with (
            patch.object(Path, "read_text", side_effect=FileNotFoundError("missing")) as read,
            patch.object(focused.time, "sleep") as sleep,
        ):
            with self.assertRaises(FileNotFoundError):
                focused.read_json(Path("missing.json"))
        self.assertEqual(read.call_count, focused.READ_ATTEMPTS)
        self.assertEqual(sleep.call_count, focused.READ_ATTEMPTS - 1)

    def test_malformed_file_is_not_retried(self) -> None:
        with (
            patch.object(Path, "read_text", return_value="{") as read,
            patch.object(focused.time, "sleep") as sleep,
        ):
            with self.assertRaises(json.JSONDecodeError):
                focused.read_json(Path("malformed.json"))
        read.assert_called_once()
        sleep.assert_not_called()


if __name__ == "__main__":
    unittest.main()
