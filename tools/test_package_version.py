"""执行 CI 的实际版本准备脚本，验证金丝雀版本与各平台元数据一致。"""
import json
import os
import plistlib
import re
import subprocess
import sys
import tempfile
import textwrap
import tomllib
import unittest
from pathlib import Path


WORKFLOW = Path(__file__).resolve().parents[1] / '.github/workflows/package.yml'
VERSION_STEP = WORKFLOW.read_text(encoding='utf-8').split(
    '      - name: Prepare version from tag or input\n', 1
)[1].split('      - name:', 1)[0]
VERSION_SCRIPT = textwrap.dedent(VERSION_STEP.split('        run: |\n', 1)[1])


class PackageVersionTest(unittest.TestCase):
    def prepare(self, base, event='push', ref_type='branch', ref_name='main', version=''):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.toml').write_text(
                f'[package]\nname = "azurpilot"\nversion = "{base}"\n', encoding='utf-8')
            (root / 'Cargo.lock').write_text(
                f'[[package]]\nname = "azurpilot"\nversion = "{base}"\n\n'
                '[[package]]\nname = "dependency"\nversion = "1.2.3"\n', encoding='utf-8')
            (root / 'tauri.conf.json').write_text(
                json.dumps({'version': base, 'productName': 'AzurPilot'}), encoding='utf-8')
            (root / 'Info.plist').write_bytes(plistlib.dumps({
                'CFBundleVersion': base, 'CFBundleShortVersionString': base,
                'CFBundleName': 'AzurPilot',
            }))
            result = subprocess.run(
                [sys.executable, '-c', VERSION_SCRIPT], cwd=root, capture_output=True,
                encoding='utf-8', env={**os.environ, 'GITHUB_EVENT_NAME': event,
                    'REF_TYPE': ref_type, 'REF_NAME': ref_name, 'INPUT_VERSION': version,
                    'GITHUB_SHA': 'abcdef0123456789' * 2 + 'abcdef01',
                    'GITHUB_ENV': str(root / 'env'), 'GITHUB_OUTPUT': str(root / 'output')})
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            actual = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
            lock = tomllib.loads((root / 'Cargo.lock').read_text(encoding='utf-8'))['package']
            self.assertEqual(lock[0]['version'], actual)
            self.assertEqual(lock[1]['version'], '1.2.3')
            tauri = json.loads((root / 'tauri.conf.json').read_text(encoding='utf-8'))
            self.assertEqual(tauri, {'version': actual, 'productName': 'AzurPilot'})
            plist = plistlib.loads((root / 'Info.plist').read_bytes())
            self.assertEqual(plist['CFBundleVersion'], actual)
            self.assertEqual(plist['CFBundleShortVersionString'], actual)
            self.assertEqual(plist['CFBundleName'], 'AzurPilot')
            self.assertEqual((root / 'env').read_text(encoding='utf-8'), f'PACKAGE_VERSION={actual}\n')
            self.assertEqual((root / 'output').read_text(encoding='utf-8'), f'version={actual}\n')
            return actual

    def test_push_increments_patch_and_synchronizes_all_version_files(self):
        for base, expected_core in [('3.0.3', '3.0.4'), ('3.0.9', '3.0.10'), ('3.9.99', '3.9.100')]:
            with self.subTest(base=base):
                actual = self.prepare(base)
                self.assertRegex(actual, rf'^{re.escape(expected_core)}-canary\.abcdef01\.\d{{14}}$')

    def test_push_replaces_existing_prerelease_and_build_suffix(self):
        actual = self.prepare('3.0.3-beta.1+build.2')
        self.assertRegex(actual, r'^3\.0\.4-canary\.abcdef01\.\d{14}$')

    def test_formal_tag_uses_requested_version_without_incrementing(self):
        self.assertEqual(self.prepare('3.0.3', ref_type='tag', ref_name='3.0.5'), '3.0.5')

    def test_manual_build_keeps_explicit_or_default_version(self):
        self.assertEqual(self.prepare('3.0.3', event='workflow_dispatch', version='3.1.0'), '3.1.0')
        self.assertEqual(self.prepare('3.0.3', event='workflow_dispatch'), '3.0.3')


if __name__ == '__main__':
    unittest.main()
