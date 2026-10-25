"""仅打包启动器；Python、代码与工具在首次部署时独立下载。"""
import os
import platform
import shutil
import tarfile
from pathlib import Path

root = Path(__file__).resolve().parents[1]
os_name = {'Windows': 'Windows', 'Darwin': 'macOS', 'Linux': 'Linux'}[platform.system()]
arch = platform.machine().lower().replace('amd64', 'x86_64')
package = root / 'package-root/AzurLaneAutoScript'
package.mkdir(parents=True, exist_ok=True)
executable = root / ('target/release/azurpilot.exe' if os_name == 'Windows' else 'target/release/azurpilot')
if os_name == 'macOS':
    bundle = root / 'AzurPilot.app/Contents'
    (bundle / 'MacOS').mkdir(parents=True, exist_ok=True)
    (bundle / 'Resources').mkdir(exist_ok=True)
    shutil.copy2(executable, bundle / 'MacOS/AzurPilot')
    shutil.copy2(root / 'icons/icon.icns', bundle / 'Resources/icon.icns')
    shutil.copy2(root / 'Info.plist', bundle / 'Info.plist')
    target, prefix = bundle.parent, 'AP'
else:
    shutil.copy2(executable, package / ('AzurPilot.exe' if os_name == 'Windows' else 'AzurPilot'))
    target, prefix = package, 'AzurLaneAutoScript'
filename = root / f'{prefix}-{os_name}-{arch}.tar.xz'
with tarfile.open(filename, 'w:xz') as archive:
    archive.add(target, arcname=target.name)
shutil.copy2(filename, root / f'{prefix}-CN-{os_name}-{arch}.tar.xz')
