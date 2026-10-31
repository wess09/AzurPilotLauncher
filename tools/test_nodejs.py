"""Regression checks for portable Node.js archive extraction."""

import io
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

from extract_nodejs import extract


class NodeArchiveTests(unittest.TestCase):
    def test_windows_zip_keeps_node_and_npm_in_one_runtime(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "node.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("node-test/node.exe", b"node")
                bundle.writestr("node-test/npm.cmd", b"npm")
                bundle.writestr("node-test/node_modules/npm/bin/npm-cli.js", b"cli")
            extract(archive, root / "staging", "node-test")
            self.assertEqual((root / "staging/node-test/node.exe").read_bytes(), b"node")
            self.assertTrue((root / "staging/node-test/npm.cmd").is_file())

    def test_zip_rejects_escaping_paths_and_windows_aliases(self):
        for name in ["../escape", "/escape", "node-test/../../escape", "node-test/CON.txt", "node-test/file:stream", "other/node.exe"]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                archive = root / "node.zip"
                with zipfile.ZipFile(archive, "w") as bundle:
                    bundle.writestr(name, b"untrusted")
                with self.assertRaises(ValueError):
                    extract(archive, root / "staging", "node-test")
                self.assertFalse((root / "escape").exists())

    def test_tar_preserves_executable_bits_and_internal_npm_link(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "node.tar.gz"
            with tarfile.open(archive, "w:gz") as bundle:
                node = tarfile.TarInfo("node-test/bin/node")
                node.size, node.mode = 4, 0o755
                bundle.addfile(node, io.BytesIO(b"node"))
                cli = tarfile.TarInfo("node-test/lib/node_modules/npm/bin/npm-cli.js")
                cli.size = 3
                bundle.addfile(cli, io.BytesIO(b"npm"))
                link = tarfile.TarInfo("node-test/bin/npm")
                link.type, link.linkname = tarfile.SYMTYPE, "../lib/node_modules/npm/bin/npm-cli.js"
                bundle.addfile(link)
            try:
                extract(archive, root / "staging", "node-test")
            except OSError as error:
                if getattr(error, "winerror", None) == 1314:
                    self.skipTest("Windows symlink privilege is unavailable")
                raise
            self.assertEqual((root / "staging/node-test/bin/npm").read_bytes(), b"npm")
            if __import__("os").name != "nt":
                self.assertTrue((root / "staging/node-test/bin/node").stat().st_mode & 0o111)

    def test_tar_rejects_link_outside_staging(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "node.tar.gz"
            with tarfile.open(archive, "w:gz") as bundle:
                link = tarfile.TarInfo("node-test/bin/npm")
                link.type, link.linkname = tarfile.SYMTYPE, "../../../escape"
                bundle.addfile(link)
            with self.assertRaises(tarfile.FilterError):
                extract(archive, root / "staging", "node-test")


if __name__ == "__main__":
    unittest.main()
