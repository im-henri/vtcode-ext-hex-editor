import json
from pathlib import Path
import tempfile
import tomllib
import unittest

import build


class PackageVersionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "extension.json").write_text(json.dumps({"id": "vtcode.hex-editor", "version": "0.1.0", "apiVersion": "2"}), encoding="utf-8")
        (self.root / "Cargo.toml").write_text('[package]\nname = "vtcode-hex-editor"\nversion = "0.1.0"\n\n[dependencies]\nserde = "1"\n', encoding="utf-8")
        (self.root / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "vtcode-extension-api"\nversion = "0.1.0"\n\n[[package]]\nname = "vtcode-hex-editor"\nversion = "0.1.0"\ndependencies = ["vtcode-extension-api"]\n', encoding="utf-8")

    def test_version_bump_updates_only_the_extension_and_keeps_host_api_independent(self):
        self.assertEqual(build.package_version(self.root, "0.2.0-beta.1"), "0.2.0-beta.1")
        metadata = json.loads((self.root / "extension.json").read_text())
        self.assertEqual(metadata["version"], "0.2.0-beta.1")
        self.assertEqual(metadata["apiVersion"], "2")
        cargo = tomllib.loads((self.root / "Cargo.toml").read_text())
        self.assertEqual(cargo["package"]["version"], "0.2.0-beta.1")
        lock = tomllib.loads((self.root / "Cargo.lock").read_text())
        self.assertEqual([item["version"] for item in lock["package"]], ["0.1.0", "0.2.0-beta.1"])
        self.assertEqual(build.package_version(self.root, git_ref="refs/tags/v0.2.0-beta.1"), "0.2.0-beta.1")

    def test_invalid_versions_and_tags_are_rejected_without_modifying_files(self):
        before = {file.name: file.read_bytes() for file in self.root.iterdir()}
        for value in ["1", "v1.0.0", "01.0.0", "1.0.0-01", "1.0.0-", "1.0.0+", "1.0.0\n"]:
            with self.assertRaises(ValueError):
                build.package_version(self.root, value)
        with self.assertRaises(ValueError):
            build.package_version(self.root, git_ref="refs/tags/v9.9.9")
        self.assertEqual(before, {file.name: file.read_bytes() for file in self.root.iterdir()})
        for value in ["0.0.0", "1.0.0", "1.0.0-alpha.1", "1.0.0+001", "1.0.0-rc.1+build.9"]:
            self.assertTrue(build.valid_version(value), value)
        for value in [None, 1, {}, []]:
            self.assertFalse(build.valid_version(value))

    def test_mismatched_package_metadata_is_rejected_and_can_be_synchronized(self):
        (self.root / "extension.json").write_text('{"version":"0.3.0"}', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "versions differ"):
            build.package_version(self.root)
        self.assertEqual(build.package_version(self.root, "0.3.0"), "0.3.0")
        self.assertEqual(build.package_version(self.root, git_ref="refs/heads/main"), "0.3.0")


if __name__ == "__main__":
    unittest.main()
