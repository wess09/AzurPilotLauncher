# Windows 代码签名

发布工作流在打包前对 Windows x64/ARM64 的 `azurpilot.exe` 添加 Authenticode 签名，归档、安装包内的 `AzurPilot.exe` 和自更新载荷使用同一份签名后的文件。Inno Setup 通过同一脚本签署安装包、卸载程序及安装时使用的临时副本。签名或验签失败会中止该平台发布。

GitHub Actions 需要以下配置：

| 类型 | 名称 | 内容 |
| --- | --- | --- |
| Secret | `WINDOWS_SIGNING_PFX_B64` | 含代码签名私钥的加密 PFX 的 Base64 |
| Secret | `WINDOWS_SIGNING_PASSWORD` | PFX 密码 |
| Variable | `WINDOWS_SIGNING_ALLOW_SELF_SIGNED` | 使用自签名证书时显式设为 `true`；默认 `false` |

`tools/ci/sign-windows.ps1` 只在内存中加载私钥，不写临时 PFX、不通过进程参数传递密码，也不导入 Windows 证书存储。文件摘要使用 SHA-256，Windows SDK SignTool 追加 RFC 3161 / SHA-256 时间戳。验签检查文件完整性、签名证书与配置是否一致、时间戳是否存在；使用公开受信任证书时还要求 SignTool 的 Authenticode 策略验签成功。

自签名模式只允许 Windows 验签返回成功，或精确的 `CERT_E_UNTRUSTEDROOT`（`0x800B0109`）。摘要不符、签名错误等其他结果均失败。CI 不会把自签名证书安装为受信任根证书，启动器和安装包也不会替用户安装证书。

本地生成的 AzurPilot 自签名证书位于用户目录 `.azurpilot/code-signing/`，包含加密的 `AzurPilot-CodeSigning.pfx`、公开的 `AzurPilot-CodeSigning.cer` 和单独保存的 `password.txt`；目录仅当前用户与 SYSTEM 有访问权限。这些文件不属于仓库，私钥和密码不能提交或放入构建产物。请备份此目录，后续版本复用同一证书。

自签名仅证明文件由持有同一私钥的人签署并可检测篡改，不能建立公开可信的发布者身份。Windows 仍可能显示未知发布者、SmartScreen 或杀毒软件提示。需要公开信任时，应替换为可信代码签名证书或接入已验证的云签名服务；现有更新通道的 mTLS 证书不能用于替代代码签名。

参考：[Microsoft SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool)、[Microsoft 代码签名说明](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/code-signing-for-smart-app-control)、[Inno Setup 签名配置](https://jrsoftware.org/ishelp/topic_setup_signtool.htm)。
