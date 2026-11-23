# Muse Desktop

面向 Muse.ai 的非官方多账号客户端，使用 Rust 和 GPUI 构建。Windows 使用 WebView2，macOS 使用 WKWebView。

## 界面预览

![Muse Desktop 界面：多账号侧栏、额度信息、标签页与 Muse 网页](assets/screenshots/desktop.png)

## 功能

- 多账号独立浏览器环境、账号切换、标签页和登录状态保存。
- 账号套餐、周额度、邀请额度与头像同步。
- HTTP / HTTPS / SOCKS5 代理池，每账号独立路由，凭据由 Windows 加密保存。
- 原生图片 / 视频创作工作室、参考图片、生成进度和本地媒体查看。
- 可调整的侧栏、窗口位置记忆及快捷键。
- 页面缩放记忆与白天 / 晚上主题切换，重启后恢复上次设置。
- 在新增账号时导入 Cookie 文本或浏览器 Cookie JSON，由 Muse 确认登录状态；账号浏览器环境独立。

## 下载与运行

在本仓库的 **Releases** 页面下载 `Muse-Desktop-<版本>-win-x64.zip`，解压后运行 `Muse.exe`。
需要 Windows 10/11 x64 和 Microsoft Edge WebView2 Evergreen Runtime，不需要安装 .NET。

macOS 版要求 macOS 14 或更新版本：Apple 芯片选择 `macos-arm64.app.zip`，Intel 选择 `macos-x64.app.zip`。解压后将 `Muse.app` 放入「应用程序」。当前 Mac 包为临时签名的开发构建，尚未进行 Apple 开发者签名与公证。

默认数据目录为 `%LOCALAPPDATA%\MuseDesktopNext`。已有用户可用
`Muse.exe --data-dir "原数据目录"` 沿用账号和历史；升级时不要删除该目录。
安装包不包含开发者的账号、Cookie、代理凭据或聊天数据。

开发脚本默认使用项目下的 `.runtime/development`，诊断模式使用 `.runtime/diagnostics`，与上述默认目录不同。
从开发版或旧启动器切换前，请核对原启动参数中的 `--data-dir`；退出相关实例、备份并确认版本兼容后再沿用原目录。
不要覆盖现有账号目录或只复制 Cookie 数据库。

macOS 的配置目录是 `~/Library/Application Support/MuseDesktopNext`；每个账号使用独立的 WebKit 持久化存储，代理凭据保存到登录钥匙串。Windows 的浏览器目录与 DPAPI 凭据不能直接复制为 Mac 登录环境；应使用下文的加密迁移入口，或在 Mac 上重新登录和配置代理。Mac 暂不提供 WebView2 的后台页面休眠能力。

## 导入 Cookie 登录

1. 点击「新增账号」，在「Cookie 登录（可选）」中粘贴单账号的 Cookie 请求头文本，或浏览器导出的 Cookie JSON 数组。也支持包含 `cookies` 数组的 JSON。
2. 填写账号名称，并在创建前选择网络连接。**Cookie 文本不包含代理配置**；原账号使用代理时，请先选择同一条静态代理配置。
3. 点击「创建账号」。程序先在独立浏览器环境内写入并核验 Cookie，再打开 Muse 确认会话；不会把“写入成功”直接当作“已登录”。

Cookie 输入默认掩码显示，取消或创建成功后清空，不写入账号元数据。一次只导入一个账号，不覆盖已有账号。缺少有效 `hatch_sess`、格式不符或已过期时会提示；网站拒绝会话时仍需重新登录后导出。

浏览器 JSON 会保留提供的有效期和安全标记；纯 Cookie 请求头没有这些完整属性，有效期缺失时按会话 Cookie 导入，不会延长服务端会话。原有「提取 Cookie」与 muse2api 文本格式保持兼容；此入口尚不是包含代理的跨端账号迁移包。

## 加密账号与静态代理迁移

1. 在账号菜单选择「导出账号与静态代理」，勾选账号，输入并确认迁移密码，保存 `.muse-account` 文件。
2. 在另一设备选择「导入加密账号包」，选择文件并输入原密码；解密后核对账号、代理地址和路由/Cookie 差异，勾选确认后导入。
3. 程序为账号、代理、浏览器 profile 和本地凭据生成新身份；同包共享代理保持共享，不覆盖已有账号。全部路由与 Cookie 核验完成后，才保存并打开网页确认会话。

账号名、Cookie、实际上游代理认证和绑定关系位于同一加密负载；迁移密码不保存到配置或日志，迁移过程不生成临时明文文件。密码保留 UTF-8 原字符、大小写、空格和换行；请自行保存，通过不同渠道传递文件与密码。所有代理统一按静态配置处理，不提供出口 IP 检测或锁定。

系统网络、本机/局域网节点和不同平台的本机地址旁路规则需要在预览中确认。源端未知的 Cookie 属性会明确说明目标映射；原生浏览器若未保留有效期或安全属性，整批账号不会激活。成功写入不代表真实身份有效，网站仍可能要求重新登录。

跨端文件采用共同冻结的 v1 协议，Windows Rust 与 Java/JCA 已完成双向合成文件互解，Windows 与 macOS 两种架构已有原生合成回归验证。文件互解、Cookie 读回和本地假代理验证均不等于真实账号跨设备登录、真实代理出口或实体设备验收；发布状态与构建结果请以对应版本的 Release/Actions 为准。

Windows 合成检查：`cargo test --workspace --locked`，构建 debug 程序后运行 `py -3 scripts/check-account-transfer-native.py` 和 `py -3 scripts/check-account-transfer-native.py --proxy --dark`。检查只创建临时诊断账号和本地假代理，系统文件选择结果由测试注入，不访问真实 Muse 或真实代理。

## 从源码构建

安装 Rust / rustup、Visual Studio 2022 C++ 构建工具（含 Windows SDK）和 Node.js 22 及以上。
仓库的 `rust-toolchain.toml` 固定 Rust 1.95.0，Cargo.lock 固定依赖版本。

```powershell
cargo test --workspace --locked
cargo build --release --locked -p muse-desktop
.\scripts\package-windows.ps1
```

构建结果在 `target/release/muse-desktop.exe`，便携 ZIP 和 SHA-256 校验文件在 `dist/`。
开发运行：`.\scripts\run.ps1`；浏览器账号数据、测试数据与缓存均被 Git 忽略。

macOS 源码构建需要 Xcode 命令行工具、Rust 1.95.0、Node.js 22+ 和 Python 3：

```bash
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo test --workspace --locked
cargo build --release --locked -p muse-desktop
python3 scripts/check-macos-smoke.py
python3 scripts/package-macos.py --arch arm64  # Intel Mac 改为 x64
```

```powershell
cargo fmt --all -- --check
node --test scripts/check-observer.cjs
node scripts/check-studio.cjs
node scripts/check-original-media.cjs
node scripts/check-reference-upload.cjs
.\scripts\check-package.ps1
```

## 自动构建与发布

- 提交到 `main`、提交 Pull Request 或在 Actions 中手动运行 **Desktop clients**，会执行检查、测试、编译并上传 Windows ZIP 与两个架构的 Mac 应用包。
- 普通构建产物位于该次 Actions 运行的 Artifacts，保留 14 天。
- 发布新版本时先修改根 `Cargo.toml` 的 `[workspace.package].version`，提交后推送同版本的 `v*` 标签。
- Windows、macOS ARM64 与 macOS Intel 标签构建全部通过后，自动创建 GitHub Release，附带三个客户端包和 SHA-256 校验文件；标签与源码版本不符会拒绝发布。

```powershell
git tag v0.1.1
git push origin v0.1.1
```

手机端是独立的 `muse-android` 仓库。两个客户端各自保存账号和历史，不自动同步。

## 社区致谢

感谢 [LINUX DO](https://linux.do/) 社区的交流与分享。本项目认可并支持 LINUX DO 社区的开源交流与互助。

## 许可证

原创代码采用 MIT。适配自 GPUI 的输入组件保留 Apache-2.0 许可，Tabler 图标和
MUSE2API 相关适配保留相应 MIT 说明；详见 `NOTICE`、`THIRD_PARTY_NOTICES.md` 和 `LICENSES/`。
