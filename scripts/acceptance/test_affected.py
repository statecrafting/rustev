import unittest

import affected

META = {
    "workspace_root": "/w",
    "packages": [
        {"name": "a", "manifest_path": "/w/crates/a/Cargo.toml",
         "dependencies": [{"name": "b", "path": "/w/crates/b"}, {"name": "serde"}]},
        {"name": "b", "manifest_path": "/w/crates/b/Cargo.toml", "dependencies": []},
        {"name": "c", "manifest_path": "/w/tools/c/Cargo.toml",
         "dependencies": [{"name": "a", "path": "/w/crates/a"}]},
    ],
}


class Closure(unittest.TestCase):
    def test_follows_path_dependencies_only(self):
        self.assertEqual(affected.closure(META, ["a"], False), ["crates/a/", "crates/b/"])

    def test_dependents_are_not_inputs(self):
        self.assertNotIn("tools/c/", affected.closure(META, ["b"], False))

    def test_every_package(self):
        self.assertEqual(affected.closure(META, [], True), ["crates/a/", "crates/b/", "tools/c/"])

    def test_an_unknown_package_is_refused(self):
        with self.assertRaises(KeyError):
            affected.closure(META, ["nope"], False)


class Affected(unittest.TestCase):
    def test_prefix_match_is_by_directory(self):
        self.assertTrue(affected.affected(["crates/a/src/lib.rs"], ["crates/a/"]))
        self.assertFalse(affected.affected(["crates/ab/src/lib.rs"], ["crates/a/"]))

    def test_exact_file(self):
        self.assertTrue(affected.affected(["Cargo.lock"], ["Cargo.lock"]))
        self.assertFalse(affected.affected(["specs/001/spec.md"], ["Cargo.lock", "crates/a/"]))

    def test_doubt_answers_affected(self):
        import contextlib, io
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            affected.main(["--package", "a"])  # no base
        self.assertEqual(out.getvalue().strip(), "true")


if __name__ == "__main__":
    unittest.main()
