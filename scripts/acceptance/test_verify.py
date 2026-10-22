"""Negative controls for omitted acceptance and failure propagation."""
import subprocess
import unittest
from unittest.mock import patch
import verify


class PartitionTests(unittest.TestCase):
    def test_exact_partition(self):
        verify.validate(["001", "002"], {"a": ["002"], "b": ["001"]})

    def test_rejects_missing_duplicate_extra_and_empty_groups(self):
        for groups in ({"a": ["001"]}, {"a": ["001", "002", "002"]},
                       {"a": ["001", "002", "003"]},
                       {"a": ["001", "002"], "b": []}):
            with self.subTest(groups=groups), self.assertRaises(ValueError):
                verify.validate(["001", "002"], groups)

    @patch.dict(verify.os.environ, {}, clear=True)
    @patch.object(verify.subprocess, "run")
    def test_failed_spec_fails_group_and_stops(self, run):
        run.return_value = subprocess.CompletedProcess([], 2)
        self.assertEqual(verify.run_group(["001", "002"]), 1)
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args.args[0], ["make", "verify", "VERIFIED_SPECS=001"])


if __name__ == "__main__":
    unittest.main()
