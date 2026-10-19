# 打包与签名

## 本地出包

### Windows

MSVC 不在 PATH 时先初始化：

```bat
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cd /d app
pnpm tauri build --bundles nsis,msi
```

产物：

```
app/src-tauri/target/release/bundle/nsis/做T决策台_0.1.0_x64-setup.exe
app/src-tauri/target/release/bundle/msi/做T决策台_0.1.0_x64_en-US.msi
```

### macOS

```bash
cd app
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm tauri build --target universal-apple-darwin --bundles dmg
```

产物：

```
app/src-tauri/target/universal-apple-darwin/release/bundle/dmg/做T决策台_0.1.0_universal.dmg
```

通用二进制同时覆盖 Apple Silicon 与 Intel。

### 图标

图标源是 `app/src-tauri/icons/icon.png`（1024×1024）。重生成：

```bash
cd app
pnpm tauri icon src-tauri/icons/icon.png
```

会生成 `icon.icns`、`icon.ico` 与各尺寸 PNG。

## 签名与公证

### Windows

Tauri 支持两种方式：

1. 证书指纹（推荐，证书已在当前用户证书存储）：

```json
"bundle": {
  "windows": {
    "certificateThumbprint": "AB12...",
    "digestAlgorithm": "sha256",
    "timestampUrl": "http://timestamp.digicert.com"
  }
}
```

2. 自定义 `signCommand`，对接云签名服务。

没有证书时构建出的安装包是可安装但未签名的，Windows SmartScreen 会提示未知发布者。

### macOS

设置以下环境变量后，Tauri 会签名并自动公证：

```
APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
APPLE_ID="you@example.com"
APPLE_PASSWORD="app-specific-password"
APPLE_TEAM_ID="TEAMID"
```

也可以改用 App Store Connect API Key：`APPLE_API_ISSUER`、`APPLE_API_KEY`、`APPLE_API_KEY_PATH`。

未设置这些变量时，CI 走未签名构建；产物可以打开，但 Gatekeeper 会提示未验证开发者。

## CI

`.github/workflows/release.yml`：

- `workflow_dispatch` 或推送 `v*` tag 触发
- Windows：NSIS + MSI
- macOS：universal dmg
- 未配置 Apple secrets 时自动走未签名分支
- 产物通过 `actions/upload-artifact` 上传

需要配置的仓库 secrets：

| Secret | 用途 |
|---|---|
| `APPLE_SIGNING_IDENTITY` | Developer ID 证书名 |
| `APPLE_ID` | Apple 账号 |
| `APPLE_PASSWORD` | App 专用密码 |
| `APPLE_TEAM_ID` | 团队 ID |

Windows 证书暂未接入 CI；如需签名，在 `tauri.conf.json` 填指纹并确保证书已导入 runner，或改用 `signCommand`。

## 当前边界

- Windows 安装包已在本机构建验证；macOS dmg 仅由 CI 构建，未在真机安装验证
- 没有 Apple 开发者证书，公证流程未实际执行
- 没有 Windows 代码签名证书，安装包未签名
- 未接入自动更新（updater）
