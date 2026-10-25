"""打包与 stable.json 合并回归；不访问公网、不需要发布密钥。"""
import hashlib
import json
import tempfile
import unittest
import zipfile
import subprocess
import io
from unittest.mock import patch
from pathlib import Path

from build_components import archive, merge as merge_components, deploy_files
from merge_stable_manifest import merge, fetch_manifest


class ComponentPackagingTest(unittest.TestCase):
    def test_manifest_requests_use_the_same_ci_headers_for_merge_and_verification(self):
        with patch('urllib.request.urlopen', return_value=io.BytesIO(b'{"version":"3.0.2"}')) as opener:
            self.assertEqual(fetch_manifest('https://example.com/stable.json', timeout=15), {'version': '3.0.2'})
            request = opener.call_args.args[0]
            self.assertEqual(request.get_header('User-agent'), 'AzurPilot-CI/1')
            self.assertEqual(request.get_header('Cache-control'), 'no-cache')
            self.assertEqual(opener.call_args.kwargs['timeout'], 15)

    def test_archive_is_deterministic_and_content_addressed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            file = root / 'file.txt'
            file.write_bytes(b'payload')
            first = archive(root / 'one', 'source', [(file, 'module/test.py')])
            second = archive(root / 'two', 'source', [(file, 'module/test.py')])
            self.assertEqual(first, second)
            path = next((root / 'one').glob('*.zip'))
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), first['sha256'])
            with zipfile.ZipFile(path) as package:
                self.assertEqual(package.read('module/test.py'), b'payload')

    def test_runtime_publish_preserves_launcher_github_url_and_unknown_fields(self):
        current = {'version': '3.0.1', 'platforms': {'windows-x86_64': {'url': 'primary', 'github_url': 'github', 'sha256': 'hash'}}, 'notes': 'keep', 'future': True}
        runtime = {'schema': 2, 'platforms': {'windows-x86_64': []}}
        updated = merge(current, runtime=runtime)
        self.assertEqual(updated['platforms'], current['platforms'])
        self.assertEqual(updated['runtime'], runtime)
        self.assertTrue(updated['future'])
        launcher = {'version': '3.0.2', 'platforms': {'linux-x86_64': {'url': 'a', 'github_url': 'b', 'sha256': 'c'}}}
        result = merge(updated, launcher=launcher)
        self.assertEqual(result['runtime'], runtime)
        self.assertEqual(result['version'], '3.0.2')
        self.assertEqual(result['platforms']['linux-x86_64']['github_url'], 'b')
        with self.assertRaises(ValueError):
            merge({}, runtime=runtime)
        with self.assertRaises(ValueError):
            merge(current, runtime={'schema': 1, 'platforms': {'windows-x86_64': []}})

    def test_deploy_file_listing_handles_unicode_paths_in_git_index(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(['git', 'init', '--quiet', str(root)], check=True)
            for name in ['deploy/installer.py', 'config/deploy.template.yaml', 'README/开发文档.md']:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('content', encoding='utf-8')
            subprocess.run(['git', '-C', str(root), 'add', '.'], check=True)
            self.assertEqual(deploy_files(root), ['config/deploy.template.yaml', 'deploy/installer.py'])

    def test_worker_validator_accepts_schema_two_and_rejects_old_components(self):
        script = (Path(__file__).resolve().parents[1] / 'cf-worker/deploy.sh').read_text(encoding='utf-8')
        validator = script.split("<<'JS'\n", 1)[1].split('\nJS', 1)[0]
        runtime = {
            'schema': 2, 'revision': 'a' * 40,
            'platforms': {'windows-x86_64': [
                {'id': name, 'sha256': 'b' * 64, 'size': 1, 'urls': ['https://example.com/component.zip']}
                for name in ('git', 'adb')]},
            'deploy': {'bases': ['https://example.com/raw'], 'files': ['deploy/installer.py']},
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'stable.json'
            def validate():
                path.write_text(json.dumps({'version': '3.0.2', 'platforms': {'windows-x86_64': {}}, 'runtime': runtime}), encoding='utf-8')
                return subprocess.run(['node', '-', str(path)], input=validator, encoding='utf-8', capture_output=True).returncode
            self.assertEqual(validate(), 0)
            runtime['schema'] = 1
            self.assertNotEqual(validate(), 0)
            runtime['schema'] = 2
            runtime['platforms']['windows-x86_64'][0]['id'] = 'dependencies'
            self.assertNotEqual(validate(), 0)

    def test_mismatched_revisions_cannot_be_published(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, revision in [('a', 'a' * 40), ('b', 'b' * 40)]:
                # 不同 revision 在校验第二份清单前即失败，无需真实依赖下载。
                fragment = {'schema': 2, 'revision': revision, 'platforms': {},
                            'deploy': {'bases': [], 'files': []}}
                (root / f'{name}.json').write_text(json.dumps(fragment), encoding='utf-8')
            with self.assertRaisesRegex(RuntimeError, '不一致'):
                merge_components(root, root / 'merged.json')


if __name__ == '__main__':
    unittest.main()
