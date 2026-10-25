"""合并同一份 stable.json：启动器载荷和运行组件互相保留，禁止发布丢字段的清单。"""
import argparse
import json
import urllib.request
from pathlib import Path


def fetch_manifest(url, *, timeout=30):
    request = urllib.request.Request(url, headers={
        'Cache-Control': 'no-cache',
        'User-Agent': 'AzurPilot-CI/1',
    })
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.load(response)


def merge(current, *, launcher=None, runtime=None):
    result = dict(current)
    if launcher is not None:
        if not launcher.get('version') or not launcher.get('platforms'):
            raise ValueError('Launcher manifest is incomplete')
        result.update({key: value for key, value in launcher.items() if key != 'runtime'})
    if runtime is not None:
        if runtime.get('schema') != 2 or not runtime.get('platforms'):
            raise ValueError('Runtime manifest is incomplete')
        result['runtime'] = runtime
    if not result.get('version') or not result.get('platforms'):
        raise ValueError('Current stable.json has no launcher version/platforms; refusing to replace it')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--current-url', default='https://launcher-api.azurpilot.com/updata/stable.json')
    parser.add_argument('--launcher', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--expected-revision')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    runtime = json.loads(args.runtime.read_text(encoding='utf-8')) if args.runtime else None
    if runtime and args.expected_revision and runtime.get('revision') != args.expected_revision:
        raise SystemExit('Backend advanced while building; refusing to publish a stale runtime batch')
    current = fetch_manifest(args.current_url)
    result = merge(current,
                   launcher=json.loads(args.launcher.read_text(encoding='utf-8')) if args.launcher else None,
                   runtime=runtime)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
