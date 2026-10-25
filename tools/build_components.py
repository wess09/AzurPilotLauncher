"""在 CI 生成可独立下载、内容寻址的运行组件。用户机器不运行此脚本。"""
import argparse
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import tempfile
import urllib.request
import zipfile
from pathlib import Path

IDS = ('source', 'resources', 'frontend', 'python', 'dependencies', 'adb')
CNB_BASE = 'https://cnb.cool/AzurPilot/AzurPilotLauncher/-/releases/download/1.0.0'
GITHUB_BASE = 'https://github.com/wess09/AzurPilotLauncher/releases/download/runtime'


def run(*args, cwd=None, env=None):
    return subprocess.check_output(args, cwd=cwd, env=env, text=True).strip()


def file_hash(path):
    with path.open('rb') as reader:
        return hashlib.file_digest(reader, 'sha256').hexdigest()


def archive(output, name, files):
    """固定时间戳并解引用 Python 内部相对链接，避免 ZIP 链接和 CI 路径泄漏。"""
    output.mkdir(parents=True, exist_ok=True)
    temporary = output / f'{name}.zip'
    with zipfile.ZipFile(temporary, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as target:
        for source, destination in sorted(files, key=lambda item: item[1]):
            info = zipfile.ZipInfo(destination, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (0o100000 | (source.stat().st_mode & 0o777)) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            with source.open('rb') as reader, target.open(info, 'w', force_zip64=True) as writer:
                shutil.copyfileobj(reader, writer, 1024 * 1024)
    digest = file_hash(temporary)
    filename = f'component-{name}-{digest}.zip'
    final = output / filename
    temporary.replace(final)
    return {'id': name, 'sha256': digest, 'size': final.stat().st_size,
            'urls': [f'{CNB_BASE}/{filename}', f'{GITHUB_BASE}/{filename}']}


def tree(root, prefix):
    for path in root.rglob('*'):
        if path.is_file() and '__pycache__' not in path.parts and path.suffix != '.pyc':
            yield path, f'{prefix}/{path.relative_to(root).as_posix()}'


def install_dependencies(interpreter, requirements):
    """沿用后端 uv 的锁定同步行为，不引入第二套 pip 解析或 wheel 发布流程。"""
    subprocess.run(['uv', 'pip', 'sync', '--python', str(interpreter), '--system',
                    '--break-system-packages', '--default-index', 'https://pypi.org/simple/',
                    str(requirements)], check=True)
    return run('uv', 'pip', 'freeze', '--python', str(interpreter)) + '\n'


def source_files(source):
    paths = subprocess.check_output(['git', 'ls-files', '-z'], cwd=source).decode().split('\0')
    for name in paths:
        if not name:
            continue
        first = name.split('/')[0]
        if first.startswith('.') or first in {'log', 'cache', 'screenshots', 'downloads', 'bootstrap', 'tests', 'test', 'docs'}:
            continue
        if name.startswith('frontend/dist/') or name.startswith('frontend/node_modules/'):
            continue
        if first == 'config' and not name.startswith(('config/template', 'config/deploy.template')):
            continue
        if (source / name).is_file():
            yield source / name, name


def build(source, output, platform, python):
    os.environ['PYTHONNOUSERSITE'] = '1'
    revision = run('git', 'rev-parse', 'HEAD', cwd=source)
    if 'AZURPILOT_PREBUILT' not in (source / 'deploy/frontend.py').read_text(encoding='utf-8'):
        raise RuntimeError('后端尚未包含预构建组件协议；请先提交并推送配套后端修改')
    version = run(python, '-c', 'import platform; print(platform.python_version())')
    if not re.fullmatch(r'3\.14\.\d+', version):
        raise RuntimeError(f'Expected stable Python 3.14.x, got {version}')
    home = Path(run(python, '-c', 'import sys; print(sys.base_prefix)'))
    windows = platform.startswith('windows-')
    uv_executable = Path(shutil.which('uv'))
    uv_record = archive(output, 'uv', [(uv_executable, 'uv.exe' if windows else 'uv')])
    with tempfile.TemporaryDirectory(prefix='azurpilot-components-') as temporary:
        stage = Path(temporary)
        runtime = stage / '.venv/runtime'
        shutil.copytree(home, runtime, symlinks=False)
        interpreter = runtime / ('python.exe' if windows else 'bin/python3.14')
        packages = runtime / ('Lib/site-packages' if windows else 'lib/python3.14/site-packages')
        # Python 包只含解释器和标准库，第三方依赖由独立组件提供。
        if packages.exists():
            shutil.rmtree(packages)
        packages.mkdir(parents=True)
        python_files = [(p, n) for p, n in tree(runtime, '.venv/runtime') if '/site-packages/' not in n]
        records = [archive(output, 'python', python_files)]
        requirements = stage / 'requirements.txt'
        requirements.write_text(run('uv', 'export', '--frozen', '--no-dev', '--no-emit-project', '--no-hashes', cwd=source), encoding='utf-8')
        pinned = install_dependencies(interpreter, requirements)
        subprocess.run([str(interpreter), '-c',
                        'import cv2, numpy, scipy, psutil, yaml, pydantic, starlette, uvicorn, requests, ncnn, rapidocr'], check=True)
        # 含绝对路径的 console scripts 不参与发布，后端始终由 Python 入口启动。
        dependencies = archive(output, 'dependencies', tree(packages, packages.relative_to(stage).as_posix()))
        dependencies.update(requirements=pinned)
        records.append(dependencies)
        adb_zip = stage / 'adb.zip'
        adb_os = {'windows': 'windows', 'linux': 'linux', 'macos': 'darwin'}[platform.split('-')[0]]
        urllib.request.urlretrieve(f'https://dl.google.com/android/repository/platform-tools-latest-{adb_os}.zip', adb_zip)
        adb = stage / 'platform-tools'
        with zipfile.ZipFile(adb_zip) as package:
            for info in package.infolist():
                try:
                    relative = Path(info.filename).relative_to('platform-tools')
                except ValueError:
                    continue
                basename = relative.name
                if info.is_dir() or relative.is_absolute() or '..' in relative.parts:
                    continue
                if basename in ('adb', 'adb.exe', 'NOTICE.txt') or relative.suffix == '.dll' or relative.parts[0] == 'lib64':
                    destination = adb / relative
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.write_bytes(package.read(info))
                    destination.chmod(0o755 if basename in ('adb', 'adb.exe') else 0o644)
        if not (adb / ('adb.exe' if windows else 'adb')).is_file():
            raise RuntimeError('Android platform-tools archive has no adb executable')
        subprocess.run([str(adb / ('adb.exe' if windows else 'adb')), 'version'], check=True)
        records.append(archive(output, 'adb', tree(adb, '.venv/Scripts' if windows else '.venv/bin')))
        # 与后端的实际指纹算法一致，启动时无需 npm 或 Node.js。
        spec = importlib.util.spec_from_file_location('frontend_builder', source / 'deploy/frontend.py')
        frontend = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(frontend)
        (source / 'frontend/dist/.source-fingerprint').write_text(frontend.source_fingerprint(source / 'frontend'), encoding='utf-8')
        records.append(archive(output, 'frontend', tree(source / 'frontend/dist', 'frontend/dist')))
        build_manifest = stage / 'BUILD_MANIFEST'
        build_manifest.write_text(json.dumps({'azurpilot_commit': revision, 'python': version, 'managed_by': 'launcher'}), encoding='utf-8')
        source_entries = list(source_files(source))
        records.append(archive(output, 'resources', [(p, n) for p, n in source_entries if n.startswith(('assets/', 'bin/'))]))
        records.append(archive(output, 'source', [*((p, n) for p, n in source_entries if not n.startswith(('assets/', 'bin/'))), (build_manifest, 'BUILD_MANIFEST')]))
        # 在不同路径重新组装，直接验证可重定位解释器和预构建前端。
        relocated = stage / 'relocated'
        relocated.mkdir()
        for record in records:
            filename = Path(record['urls'][0]).name
            with zipfile.ZipFile(output / filename) as package:
                package.extractall(relocated)
                for entry in package.infolist():
                    (relocated / entry.filename).chmod((entry.external_attr >> 16) & 0o777)
        moved_python = relocated / interpreter.relative_to(stage)
        env = dict(os.environ, AZURPILOT_PREBUILT='1', AZURPILOT_NO_UV_BOOTSTRAP='1')
        env.pop('PYTHONHOME', None)
        env.pop('PYTHONPATH', None)
        subprocess.run([str(moved_python), '-c',
                        'import cv2, numpy, ncnn, rapidocr, uvicorn; from deploy.frontend import ensure_frontend; ensure_frontend()'],
                       cwd=relocated, env=env, check=True)
    fragment = {'schema': 1, 'revision': revision, 'python': version, 'platforms': {platform: records}, 'installers': {platform: uv_record}}
    (output / f'{platform}.json').write_text(json.dumps(fragment, indent=2), encoding='utf-8')


def merge(directory, output):
    merged = None
    for file in sorted(directory.glob('*.json')):
        fragment = json.loads(file.read_text(encoding='utf-8'))
        if merged is None:
            merged = dict(fragment, platforms={}, installers={})
        if any(fragment[key] != merged[key] for key in ('schema', 'revision', 'python')):
            raise RuntimeError('同一批次的源码提交或 Python 补丁版本不一致，拒绝发布')
        for platform, components in fragment['platforms'].items():
            if platform in merged['platforms'] or {c['id'] for c in components} != set(IDS):
                raise RuntimeError('重复平台或缺少组件')
            for component in components:
                verify_artifact(directory, component)
            merged['platforms'][platform] = components
            installer = fragment.get('installers', {}).get(platform)
            if installer is None:
                raise RuntimeError('Missing optional mirror installer')
            verify_artifact(directory, installer)
            merged['installers'][platform] = installer
    required_platforms = {'linux-x86_64', 'windows-x86_64', 'macos-aarch64'}
    if merged is None or set(merged['platforms']) != required_platforms:
        raise RuntimeError('Incomplete platform matrix; refusing to publish')
    output.write_text(json.dumps(merged, indent=2), encoding='utf-8')


def verify_artifact(directory, artifact):
    archive_path = directory / Path(artifact['urls'][0]).name
    if archive_path.stat().st_size != artifact['size'] or file_hash(archive_path) != artifact['sha256']:
        raise RuntimeError(f'组件损坏: {archive_path}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--platform')
    parser.add_argument('--python')
    parser.add_argument('--merge', type=Path)
    args = parser.parse_args()
    if args.merge:
        merge(args.merge, args.output)
    else:
        if not all((args.source, args.platform, args.python)):
            parser.error('--source, --platform and --python are required for build')
        build(args.source.resolve(), args.output.resolve(), args.platform, args.python)
