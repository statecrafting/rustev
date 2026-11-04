"""Negative controls for omitted acceptance and failure propagation."""
from pathlib import Path
import shutil
import subprocess
import tempfile
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
    @patch.object(verify, "ensure_disk", return_value="none")
    @patch.object(verify.subprocess, "run")
    def test_failed_spec_fails_group_and_stops(self, run, _disk):
        run.return_value = subprocess.CompletedProcess([], 2)
        self.assertEqual(verify.run_group(["001", "002"]), 1)
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args.args[0], ["make", "verify", "VERIFIED_SPECS=001"])



class DiskFloorTests(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir, True)
        self.target = self.dir / "target"
        (self.target / "debug" / "incremental").mkdir(parents=True)
        env = patch.dict(verify.os.environ, {"CARGO_TARGET_DIR": str(self.target)})
        env.start()
        self.addCleanup(env.stop)

    @patch.object(verify.subprocess, "run")
    def test_above_the_floor_touches_nothing(self, run):
        self.assertEqual(verify.ensure_disk(floor=0), "none")
        self.assertTrue((self.target / "debug" / "incremental").exists())
        run.assert_not_called()

    @patch.object(verify.subprocess, "run")
    def test_incremental_artifacts_go_first(self, run):
        frees = iter([1, 10])
        with patch.object(verify, "free_bytes", side_effect=lambda _p: next(frees)):
            self.assertEqual(verify.ensure_disk(floor=5), "incremental")
        self.assertFalse((self.target / "debug" / "incremental").exists())
        run.assert_not_called()

    @patch.object(verify.subprocess, "run")
    def test_still_below_the_floor_cleans_the_target(self, run):
        with patch.object(verify, "free_bytes", return_value=1):
            self.assertEqual(verify.ensure_disk(floor=5), "clean")
        self.assertEqual(run.call_args.args[0], ["cargo", "clean"])

    @patch.dict(verify.os.environ, {}, clear=True)
    @patch.object(verify.subprocess, "run")
    def test_every_spec_checks_the_floor_first(self, run):
        run.return_value = subprocess.CompletedProcess([], 0)
        with patch.object(verify, "ensure_disk", return_value="none") as disk:
            self.assertEqual(verify.run_group(["001", "002"]), 0)
        self.assertEqual(disk.call_count, 2)


if __name__ == "__main__":
    unittest.main()
