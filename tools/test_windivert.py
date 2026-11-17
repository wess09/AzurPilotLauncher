import hashlib
import io
from pathlib import Path
import tempfile
import unittest
import zipfile

from prepare_windivert import prepare


class WinDivertPreparationTests(unittest.TestCase):
    def fixture(self):
        output = io.BytesIO()
        files = {'WinDivert.dll': b'dll', 'WinDivert64.sys': b'signed driver', 'LICENSE': b'license'}
        with zipfile.ZipFile(output, 'w') as archive:
            for name, content in files.items():
                archive.writestr('bundle/' + name, content)
        data = output.getvalue()
        manifest = {'sha256': hashlib.sha256(data).hexdigest(), 'files': {
            name: {'member': 'bundle/' + name, 'sha256': hashlib.sha256(content).hexdigest()}
            for name, content in files.items()
        }}
        return data, manifest, files

    def test_checks_all_digests_before_writing_or_replacing_any_files(self):
        data, manifest, files = self.fixture()
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / 'output'
            prepare(data, destination, manifest)
            for name, content in files.items():
                self.assertEqual((destination / name).read_bytes(), content)
            manifest['files']['LICENSE']['sha256'] = '0' * 64
            (destination / 'WinDivert.dll').write_bytes(b'existing')
            with self.assertRaises(ValueError):
                prepare(data, destination, manifest)
            self.assertEqual((destination / 'WinDivert.dll').read_bytes(), b'existing')

    def test_corrupted_archive_does_not_create_installation(self):
        data, manifest, _ = self.fixture()
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / 'output'
            with self.assertRaises(ValueError):
                prepare(data + b'corruption', destination, manifest)
            self.assertFalse(destination.exists())

    def test_output_traversal_is_rejected(self):
        data, manifest, _ = self.fixture()
        manifest['files']['../outside'] = manifest['files']['LICENSE']
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(ValueError):
                prepare(data, Path(temporary) / 'output', manifest)
            self.assertFalse((Path(temporary) / 'outside').exists())


if __name__ == '__main__':
    unittest.main()
