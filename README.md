# Muse Desktop

面向 Muse.ai 的非官方多账号客户端，使用 Rust、GPUI 和 WebView2 构建。当前仅支持 Windows 10/11 x64。

## 界面预览

![Muse Desktop 界面：多账号侧栏、额度信息、标签页与 Muse 网页](assets/screenshots/desktop.png)

## 功能

- 多账号独立浏览器环境、账号切换、标签页和登录状态保存。
- 每账号固定的基础指纹隔离，可在账号设置中关闭并恢复原始像素读取。
- 账号套餐、周额度、邀请额度与头像同步。
- HTTP / HTTPS / SOCKS5 代理池，每账号独立路由，凭据由 Windows 加密保存。
- 代理池支持多行粘贴 / UTF-8 TXT 批量导入，逐行预览与凭据感知去重，确认后追加。
- 原生图片 / 视频创作工作室、参考图片、生成进度和本地媒体查看。
- 可调整的侧栏、窗口位置记忆及快捷键。
- 页面缩放记忆与白天 / 晚上主题切换，重启后恢复上次设置。
- 在新增账号时导入 Cookie 文本或浏览器 Cookie JSON，由 Muse 确认登录状态；账号浏览器环境独立。

## 下载与运行

在本仓库的 **Releases** 页面下载 `Muse-Desktop-<版本>-win-x64.zip`，解压后运行 `Muse.exe`。
需要 Windows 10/11 x64 和 Microsoft Edge WebView2 Evergreen Runtime，不需要安装 .NET。

默认数据目录为 `%LOCALAPPDATA%\MuseDesktopNext`。已有用户可用
`Muse.exe --data-dir "原数据目录"` 沿用账号和历史；升级时不要删除该目录。
安装包不包含开发者的账号、Cookie、代理凭据或聊天数据。

开发脚本默认使用项目下的 `.runtime/development`，诊断模式使用 `.runtime/diagnostics`，与上述默认目录不同。
从开发版或旧启动器切换前，请核对原启动参数中的 `--data-dir`；退出相关实例、备份并确认版本兼容后再沿用原目录。
不要覆盖现有账号目录或只复制 Cookie 数据库。

## 批量导入代理

打开「管理代理池」→「批量导入」，选择无协议行的默认协议，点击「粘贴多行」或「选择 TXT」。每行一条，支持完整 `http://`、`https://`、`socks5://` URL、`host:port`，或 `主机:端口:用户名:密码`；IPv6 使用 `[::1]:1080`。URL 内的特殊认证字符应使用百分号转义，四段式的用户名/密码按原字符保存，密码可以包含冒号。默认协议只作用于无 scheme 行。

每批最多 1 MiB、1000 个非空条目，文件须为 UTF-8（支持 BOM / CRLF）。预览显示行号、端点、重复与错误原因，始终隐藏密码；含密原文不写入日志。确认只追加有效且不重复的代理，重复判断包含协议、规范化地址/端口及用户名/密码，同一端点的不同认证会保留。错误项和重复项跳过，保存失败则本批不追加；返回或取消不保存。凭据仍使用系统保护存储，不自动测试代理或更改账号绑定。原有单条新增/编辑继续可用。

## 每账号语言与时区

在「编辑账号」中设置默认浏览器语言与时区，留空跟随系统/浏览器默认。支持的值列在输入框下方；不会根据代理推断位置。旧账号没有设置或保存了未知选项时仍使用默认值。

语言通过 WebView2 环境的原生 Language 设置（包括默认 Accept-Language），时区通过原生 CDP Emulation.setTimezoneOverride，在首次页面导航前核验完成。同一账号的标签页、弹窗转开的标签页及内部浏览器使用一致设置。修改后重开该账号的全部浏览器，等待原进程退出并复用原 profile，保留账号 ID、Cookie、代理绑定与保护存储。

所有账号（包括历史 basic 设置）均停止 Canvas/WebGL 读取改写，不修改 UA、client hints 或设备属性，也不清 Cookie、切换代理或自动登录。冻结 v1 迁移文件不变，不携带此本地可选设置；新导入账号默认跟随系统。服务端会话是否有效仍由网站决定。

代理池「测试隧道」仅核验代理连接/认证及到 muse.ai:443 的隧道建立，不代表目标 TLS、HTTP 页面或跨站登录重定向可用。浏览器导航失败会区分收到的 HTTP 状态、TLS/连接/认证/重定向错误；WebView2 0 表示没有明确原因，不等同于代理失效。不会因测试结果自动切换出口或回退直连。

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

跨端文件采用共同冻结的 v1 协议，Windows Rust 与 Java/JCA 已完成双向合成文件互解；协议保持跨端兼容，本桌面客户端当前仅提供 Windows x64。文件互解、Cookie 读回和本地假代理验证均不等于真实账号跨设备登录、真实代理出口或实体设备验收；发布状态与构建结果请以对应版本的 Release/Actions 为准。

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

```powershell
cargo fmt --all -- --check
node --test scripts/check-observer.cjs
node scripts/check-studio.cjs
node scripts/check-original-media.cjs
node scripts/check-reference-upload.cjs
.\scripts\check-package.ps1
```

## 自动构建与发布

- 提交到 `main`、提交 Pull Request 或在 Actions 中手动运行 **Desktop Windows x64**，会执行检查、测试、Windows 原生启动检查并上传 Windows x64 ZIP 与 SHA-256 校验文件。
- 普通构建产物位于该次 Actions 运行的 Artifacts，保留 14 天。
- 发布新版本时先修改根 `Cargo.toml` 的 `[workspace.package].version`，提交后推送同版本的 `v*` 标签。
- Windows x64 标签构建通过后，自动创建 GitHub Release，仅附带 Windows x64 客户端 ZIP 和 SHA-256 校验文件；标签与源码版本不符会拒绝发布。

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

### 应用内更新

本版起，启动后在后台检查 GitHub 官方仓库的稳定 Release；侧栏「检查更新」也可手动检查。
发现新版本后显示说明，点击下载才开始获取对应平台的更新包。下载可取消或重试，
SHA256、体积、架构及解压路径校验通过后，点击「重启并更新」完成安装。
更新不会导出 Muse Cookie、发送代理认证或修改账号数据；保留原 `--data-dir` 和启动参数。
Windows 仅替换包内应用文件，原应用备份保留在安装目录的 `.muse-update-backup-*`；
下载校验不等于独立的发布者签名。
首次升级仍需手动安装包含更新器的版本；旧版本不会因 GitHub 发版而自动获得该功能。

卡片刷新失败会保留上次成功的值与时间，并标记未更新/旧周期；明确退出登录单独处理。
邀请信息通过原账号 WebView 的只读 GET 自动读取已有邀请码与剩余使用次数，同时被动同步
网站已提供的 DOM/响应；不会自动生成邀请码、点击揭示、刮卡或领取按钮。账号尚无邀请码或
站点未提供的数据仍显示待同步；使用次数不会冒充已兑现好友数，额外额度也不会当作邀请次数。
