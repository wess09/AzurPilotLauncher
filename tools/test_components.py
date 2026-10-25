"""打包与 stable.json 合并回归；不访问公网、不需要发布密钥。"""
import hashlib
import json
import tempfile
import unittest
import zipfile
from pathlib import Path

from build_components import archive, merge as merge_components
from merge_stable_manifest import merge


class ComponentPackagingTest(unittest.TestCase):
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
        runtime = {'schema': 1, 'platforms': {'windows-x86_64': []}}
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

    def test_mismatched_revisions_cannot_be_published(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, revision in [('a', 'a' * 40), ('b', 'b' * 40)]:
                # 不同 revision 在校验第二份清单前即失败，无需真实依赖下载。
                fragment = {'schema': 1, 'revision': revision, 'python': '3.14.6', 'platforms': {}}
                (root / f'{name}.json').write_text(json.dumps(fragment), encoding='utf-8')
            with self.assertRaisesRegex(RuntimeError, '不一致'):
                merge_components(root, root / 'merged.json')


if __name__ == '__main__':
    unittest.main()
