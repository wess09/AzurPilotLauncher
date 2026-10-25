"""在 CI 打包 git/adb 二进制组件并生成 deploy 引导下载清单。用户机器不运行此脚本。

组件只含外部二进制：git（Windows 取 git-for-windows 最新 MinGit，
macOS/Linux 取 runner 发行包布局）与 adb（Google platform-tools）。
源码、Python、依赖与前端不经 CI：启动器经 deploy 脚本（master raw
下载引导，git 拉取仓库）与内嵌 uv 在启动阶段自行完成。
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import urllib.request
import zipfile
from pathlib import Path

IDS = ('git', 'adb')
SCHEMA = 2
CNB_BASE = 'https://cnb.cool/AzurPilot/AzurPilotLauncher/-/releases/download/1.0.0'
GITHUB_BASE = 'https://github.com/wess09/AzurPilotLauncher/releases/download/runtime'
# deploy 引导脚本直接从仓库 master raw 下载，跟随最新提交，不锁定版本。
DEPLOY_RAW_BASES = [
    'https://cnb.cool/AzurPilot/AzurPilot/-/git/raw/master',
    'https://raw.githubusercontent.com/wess09/AzurPilot/master',
]
DEPLOY_TEMPLATES = (
    'config/deploy.template.yaml',
    'config/deploy.template-cn.yaml',
    'config/deploy.template-linux.yaml',
    'config/deploy.template-linux-cn.yaml',
)


def run(*args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, encoding='utf-8').strip()


def file_hash(path):
    digest = hashlib.sha256()
    with path.open('rb') as reader:
        for block in iter(lambda: reader.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def archive(output, name, files):
    """固定时间戳打包为内容寻址 ZIP，保留可执行位且不泄漏 CI 路径。"""
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


def github_api(url):
    """请求 GitHub API；CI 环境携带 GITHUB_TOKEN，规避 runner 共享
    IP 触发的匿名限流（每小时 60 次，定时构建常撞 403）。"""
    headers = {'Accept': 'application/vnd.github+json'}
    token = os.environ.get('GITHUB_TOKEN')
    if token:
        headers['Authorization'] = f'Bearer {token}'
    return urllib.request.urlopen(urllib.request.Request(url, headers=headers))


def git_component_files(stage, platform):
    """收集当前平台的 git 二进制布局，返回归档文件表。

    Windows 从 git-for-windows 最新 release 提取 MinGit（busybox 64-bit）；
    macOS/Linux 复制 CI runner 的发行包布局：git 主程序、`libexec/git-core`
    与初始化模板。
    """
    if platform.startswith('windows-'):
        release = json.load(github_api(
            'https://api.github.com/repos/git-for-windows/git/releases/latest'))
        asset = next(
            asset for asset in release['assets']
            if re.fullmatch(r'MinGit-[0-9.]+-busybox-64-bit\.zip', asset['name'])
        )
        archive_path = stage / 'mingit.zip'
        urllib.request.urlretrieve(asset['browser_download_url'], archive_path)
        extracted = stage / 'mingit'
        with zipfile.ZipFile(archive_path) as package:
            for info in package.infolist():
                relative = Path(info.filename)
                if info.is_dir() or relative.is_absolute() or '..' in relative.parts:
                    continue
                destination = extracted / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(package.read(info))
        return list(tree(extracted, '.azurpilot/tools/git'))

    exec_path = Path(run('git', '--exec-path'))
    tool_root = exec_path.parent.parent
    git_bin = tool_root / 'bin' / 'git'
    if not git_bin.is_file():
        git_bin = Path(shutil.which('git'))
    files = [(git_bin, '.azurpilot/tools/git/bin/git')]
    files.extend(tree(exec_path, '.azurpilot/tools/git/libexec/git-core'))
    templates = tool_root / 'share' / 'git-core' / 'templates'
    if not templates.is_dir():
        templates = Path('/usr/share/git-core/templates')
    if templates.is_dir():
        files.extend(tree(templates, '.azurpilot/tools/git/share/git-core/templates'))
    return files


def adb_component_files(stage, platform):
    """下载 Google platform-tools 并提取 adb 及其运行库。"""
    adb_os = {'windows': 'windows', 'linux': 'linux', 'macos': 'darwin'}[platform.split('-')[0]]
    windows = platform.startswith('windows-')
    archive_path = stage / 'adb.zip'
    urllib.request.urlretrieve(
        f'https://dl.google.com/android/repository/platform-tools-latest-{adb_os}.zip', archive_path)
    adb = stage / 'platform-tools'
    with zipfile.ZipFile(archive_path) as package:
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
    return list(tree(adb, '.azurpilot/tools/adb'))


def deploy_files(source):
    """列出 deploy 部署脚本与分系统模板；config 下的 json 一律不列。"""
    names = run('git', 'ls-files', '-z', cwd=source).split('\0')
    files = [
        name for name in names
        if name.startswith('deploy/')
        and '__pycache__' not in Path(name).parts
        and not name.endswith('.pyc')
    ]
    files.extend(template for template in DEPLOY_TEMPLATES if (source / template).is_file())
    return sorted(files)


def verify_relocated(output, platform):
    """把两组件解压到空目录，验证 git/adb 可独立执行。"""
    relocated = output / 'relocated'
    relocated.mkdir(exist_ok=True)
    for path in output.glob('component-*.zip'):
        with zipfile.ZipFile(path) as package:
            for entry in package.infolist():
                destination = relocated / entry.filename
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(package.read(entry))
                destination.chmod((entry.external_attr >> 16) & 0o777 or 0o644)
    env = dict(os.environ)
    env.pop('GIT_EXEC_PATH', None)
    if platform.startswith('windows-'):
        git = relocated / '.azurpilot/tools/git/cmd/git.exe'
        adb = relocated / '.azurpilot/tools/adb/adb.exe'
    else:
        git = relocated / '.azurpilot/tools/git/bin/git'
        adb = relocated / '.azurpilot/tools/adb/adb'
        env['GIT_EXEC_PATH'] = str(relocated / '.azurpilot/tools/git/libexec/git-core')
        env['GIT_TEMPLATE_DIR'] = str(relocated / '.azurpilot/tools/git/share/git-core/templates')
    subprocess.run([str(git), '--version'], check=True, env=env)
    subprocess.run([str(adb), 'version'], check=True, env=env)


def build(source, output, platform):
    revision = run('git', 'rev-parse', 'HEAD', cwd=source)
    files = deploy_files(source)
    if 'deploy/installer.py' not in files:
        raise RuntimeError('后端缺少 deploy/installer.py')
    if not any(name in files for name in DEPLOY_TEMPLATES):
        raise RuntimeError('后端缺少 config/deploy.template*.yaml 模板')
    with tempfile.TemporaryDirectory(prefix='azurpilot-components-') as temporary:
        stage = Path(temporary)
        records = [
            archive(output, 'git', git_component_files(stage, platform)),
            archive(output, 'adb', adb_component_files(stage, platform)),
        ]
        verify_relocated(output, platform)
    fragment = {
        'schema': SCHEMA,
        'revision': revision,
        'platforms': {platform: records},
        'deploy': {'bases': list(DEPLOY_RAW_BASES), 'files': files},
    }
    (output / f'{platform}.json').write_text(json.dumps(fragment, indent=2), encoding='utf-8')


def merge(directory, output):
    merged = None
    for file in sorted(directory.glob('*.json')):
        fragment = json.loads(file.read_text(encoding='utf-8'))
        if merged is None:
            merged = dict(fragment, platforms={})
        if fragment.get('schema') != SCHEMA or any(
                fragment[key] != merged[key] for key in ('schema', 'revision', 'deploy')):
            raise RuntimeError('同一批次的源码提交或 deploy 清单不一致，拒绝发布')
        for platform, components in fragment['platforms'].items():
            if platform in merged['platforms'] or {c['id'] for c in components} != set(IDS):
                raise RuntimeError('重复平台或缺少组件')
            for component in components:
                verify_artifact(directory, component)
            merged['platforms'][platform] = components
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
    parser.add_argument('--merge', type=Path)
    args = parser.parse_args()
    if args.merge:
        merge(args.merge, args.output)
    else:
        if not all((args.source, args.platform)):
            parser.error('--source and --platform are required for build')
        build(args.source.resolve(), args.output.resolve(), args.platform)
