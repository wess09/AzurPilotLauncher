"""准备已签名的 WinDivert；构建前下载并校验，不安装或加载驱动。"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def prepare(data, destination, manifest):
    if hashlib.sha256(data).hexdigest() != manifest['sha256']:
        raise ValueError('WinDivert archive digest mismatch')
    verified = {}
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        for name, entry in manifest['files'].items():
            if Path(name).name != name or '/' in name or '\\' in name:
                raise ValueError('Invalid WinDivert output filename')
            content = archive.read(entry['member'])
            if hashlib.sha256(content).hexdigest() != entry['sha256']:
                raise ValueError('WinDivert file digest mismatch')
            verified[name] = content
    destination.mkdir(parents=True, exist_ok=True)
    for name, content in verified.items():
        temporary = destination / (name + '.tmp')
        temporary.write_bytes(content)
        temporary.replace(destination / name)
    return destination.resolve()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/bootstrap/windivert/prepared')
    args = parser.parse_args()
    manifest = json.loads((ROOT / 'tools/windivert.json').read_text(encoding='utf-8'))
    try:
        if args.archive:
            data = args.archive.read_bytes()
        else:
            with urllib.request.urlopen(manifest['url'], timeout=30) as response:
                data = response.read(2 * 1024 * 1024)
        destination = prepare(data, args.output, manifest)
    except Exception as error:
        # 不把下载 URL 或 traceback 写入 CI 日志。
        raise SystemExit('WinDivert preparation failed: ' + type(error).__name__) from None
    if os.environ.get('GITHUB_ENV'):
        with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as environment:
            environment.write(f'AZURPILOT_WINDIVERT_DIR={destination}\n')
            environment.write('AZURPILOT_REQUIRE_WINDIVERT=1\n')
    print('Prepared verified WinDivert ' + manifest['version'])
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
