# B：成熟代理方案选型与最小适配依据

日期：2026-11-25。A 已先独立交付于 `81ef39137ed6dc3e4fba9e481063d803f32b2604`，本研究不修改或重写该提交，也不代表真实代理故障已修复。

## 结论

**推荐后续路线是 GOST v3 的独立引擎适配，而不是把 Go 嵌入 Rust、直接换一个 SOCKS 客户端库，或立即整体替换现有桥接。当前准入状态为“暂缓接入”：官方 v3.3.3 的本机 PoC 证明认证链路可行，但半关闭尾数据检查失败，Windows 正常退出也未验证。** 必须先把这些缺口与打包/许可证清单交协调者核定；本轮产品继续使用既有 Rust 桥接。

这是明确的技术路线建议，不是“stock EXE 可以直接上线”的结论。GOST 的 JSON 配置/stdin 通道、独立 HTTP handler 与认证 SOCKS5 connector、三个原生发布目标使它最接近所需完整转发层；3proxy 是可行备选，但配置转义、Mac 两架构自建及 SSL 构建许可成本更高。fast-socks5 是有历史的可嵌入组件，只能替换 SOCKS 握手/流部分，不能自行消除现有 HTTP CONNECT、生命周期与取消逻辑。[G1–G8, T1–T5, R1–R4]

## 当前项目事实

- `muse-proxy/src/lib.rs` 已使用 Hyper 1 / Tokio、tokio-socks 0.5、tokio-rustls；已有回环随机端口/随机认证、双向复制、15s 建连 deadline、96 连接及 96 隧道许可、停止 watch，错误不自动回落直连。不是从零自写全部 HTTP/SOCKS 协议。
- `BrowserPool` 以 **AccountId** 缓存 `Arc<ProxyBridge>`，同账号标签共享，同一池条目被不同账号使用时仍有各自桥接。`browser.rs` 负责载入 ProtectedStore、Windows WebView2 回环认证、macOS 安装代理；不能把共享池 ID 当隔离边界。
- 凭据在 Windows DPAPI / macOS Keychain；批量导入只追加静态配置，不改变这些边界或冻结 v1 迁移。
- Windows `updater_download::windows_app_file()` 当前仅允许 Muse.exe、许可证/说明及 LICENSES 下文件，**直接往 ZIP 加 gost.exe 会被现有更新器拒绝**。Mac 包当前复制一个主程序并 ad-hoc 签名，安装器校验主程序架构、Info.plist 与 deep/strict 签名；加入 helper 需要真正调整构建、签名、更新与回滚语义。

项目证据链接固定到公开基线 `7ac49491e8f28152ca94679d3c4381813df44438`，所引文件在 A 提交中均未改变：[P1–P5]。此处是源码核对，未运行真实账号/网站/代理。

## 三个候选

|候选与锁定材料|认证及接口|Windows x64 / Mac x64 / Mac arm64|维护及许可核实|判断|
|---|---|---|---|---|
|**GOST v3.3.3**；gost commit `3d514c57230bfa524aa89087da5580b314f1cd36`，依赖 core v0.6.1、x v0.28.1（`006b432a7d0472d49ec3ca77a37a133c71e8c0f3`）|HTTP handler 的 auth 与 chain，SOCKS5 connector 独立上游 auth；HTTP CONNECT 与普通 HTTP 都有实现；`-C -` 从 stdin 读取 JSON/YAML|v3.3.3 发布资产及 `.goreleaser.yaml` 均明确列出 windows_amd64、darwin_amd64、darwin_arm64；仅 Windows 实跑|2026-11-25 公开 `/releases/latest` 重定向 v3.3.3，发布时间 **2026-11-02 13:20:15 UTC**；repo 公开元数据 pushed 2026-11-11。核心 LICENSE 是 MIT，x 亦有 LICENSE；完整传递依赖通知尚未审计|首选**后续适配路线**；stock PoC 部分失败，不能直接接入|
|**3proxy 1.0.4**；commit `e28d86d7e415057186c913705150d5a04c55a4fb`|`auth strong`/users 入站，HTTP `proxy` 服务，`parent 1000 socks5+ host port user secret`；`+` 表示把域名交给上游；省略配置文件可从 stdin 读|官方 x64 ZIP；Darwin CMake 分支和 macos-15 CI 使用 Makefile.FreeBSD，**没有显式 x64+arm64 双架构矩阵或本次 Mac 资产**，需两架构自建验证；本轮未构建/运行|`/releases/latest` 确认 1.0.4，**2026-11-21 09:14:48 UTC**；CHANGELOG 实际有认证复用/parent 失败等修复。直接读 `copying`：BSD 风格保留声明、二进制再分发通知及不背书条件，另给可选 Apache/GPL/LGPL 路径；不是 GitHub NOASSERTION 就无许可|第二选择；核心可用，但跨端构建/配置转义/SSL 依赖清单不如 GOST 简单|
|**fast-socks5 1.0.0**；crate VCS commit `bae2fc05f993d3a247b5435ba7da221f3fd80a3c`|Tokio `Socks5Stream` 客户端，用户名/密码、IPv4/IPv6/domain，AsyncRead/AsyncWrite 和底层 shutdown 委托；不提供本项目所需 HTTP 入站层|源码为 Rust/Tokio/socket2，无该 crate 自身 build script；三个目标有源码可行性，**不能把 README cross-platform 当作三目标已验收**；本次未原生编译/运行|crates.io 官方 metadata 与 CHANGELOG 一致：1.0.0 **2026-01-20**，另有 2025 rc/beta、2024 0.10.0 历史，非新建一次性库。LICENSE 原文 MIT；crate 下载校验和与 registry metadata 一致|适合未来限定替换 `connect_socks` 的备选；收益窄，保留大量现有桥接责任，不作为“换成熟内核”的替代说法|

本轮不按 star 数评判。GOST v2 的 `ginuerzh/gost` 没有与 v3 文档/配置混用。seeds 中 shoes v0.3.8（2026-11-16）的公开 Release 已读取，但缺本任务 Windows 成品/原生依据，因此未提升为前三目标主推荐，也未为它安装新工具链。[S1]

## 安全和工程差异

**回环认证与隔离。** GOST/3proxy 都能配置 HTTP 入站认证，但本项目要自行生成每账号的随机认证、只绑定 `127.0.0.1`、保持同账号标签共享和不同账号独立。不能启用系统代理、TUN、透明转发、公开管理 API 或多账号共用一个无隔离端口。GOST 的基本认证方法协商与已配置密码不是“强制只接受一种上游 auth 方法”的同义词；本次 mock 确认选择了 SOCKS5 用户名/密码，其他协商策略未验证。[G3, G4]

**秘密传递。** GOST v3 源码确有 `-C -`，不是臆测。PoC 使用匿名 stdin 管道；命令行仅 `-C -`，随机合成秘密未写配置文件，日志 0 字节。产品适配继续由 ProtectedStore 解密后从私有管道交给子进程；不使用带密码的 `-L/-F`、inline JSON argv、环境变量、临时明文文件或 `-O` 导出，不启用 debug/trace/recorders/sniffing/MITM/API。父进程 Zeroizing 不意味着 Go 堆里的副本会可靠清零，需在凭据轮换/账号退出时终止对应进程，并核对崩溃转储策略。3proxy 也支持 stdin，但其行式 tokenizer 对空格、引号、冒号、特殊字符必须另做精确映射；未凭一段 README 假设任意凭据都能无损传递。[G3, G5, T3]

**TLS 不能照搬默认值。** x v0.28.1 的 `LoadClientConfig()` 设 `InsecureSkipVerify = !config.Secure`；HTTPS 上游必须显式 `tls.secure=true` 并设正确 serverName/系统根或指定 CA，不能降低当前 Rust 的证书验证。普通 SOCKS5 保持 TCP，并显式 `connector.metadata.notls=true` 关闭 GOST 的非标准 TLS 扩展；不要误把该扩展当标准 SOCKS5 TLS。3proxy 文档同样把 `ssl_client_no_verify` 标成默认，需显式验证与 CA/SNI；官方 Windows 构建 workflow 还使用静态 wolfSSL/PCRE2，不能只看核心 `copying` 就认定整包统一许可。两者 TLS 证书路径本轮都未运行验证。[G4, G6, T4, T5]

**失败关闭、取消、半关闭。** 只有明确单一强制上游链可被选入适配；禁用直连 bypass、故障 direct fallback、替代出口和自动轮换；启动失败/引擎退出先阻断对应账号浏览器，不清代理后重试。GOST 源码有 context/双向 Pipe/CloseWrite 等路径，仍不能代替实测，本次尾数据失败就是反例。fast-socks5 委托底层 shutdown，但取消 deadline、Hyper upgrade 生命周期、并发限额、HTTP hop header 清理仍由 Muse 负责；错误中可包含 username，必须继续静态错误映射，不能直接把库错误写 UI/日志。[G7, G8, R3, P1]

**体积与随包成本。** 实际下载的 GOST Windows x64 ZIP 为 **17,527,304 字节（16.72 MiB）**，EXE 为 **48,732,160 字节（46.47 MiB）**；不是零成本依赖。每账号独立进程还会增加内存/句柄，未测多账号 RSS。官方 Mac 两资产存在但本轮未下载/执行，不能给出伪造体积/签名结论。3proxy 从源码可裁剪，但 SSL/插件依赖与自建产物大小未测。Rust 组件无第二 EXE，最终增量体积也未测。[G1, G2, T5, R1]

## 纯回环 PoC：链路可行，完整契约未通过

文件：`gost_loopback_poc.py`、`poc-run.log`、`poc-result.json`、`poc.exit.txt`、`poc-engine.log`、`gost-buildinfo.txt`。这是报告目录中的独立合成 spike，不是产品新测试用例/框架，也没有改产品内核。

- 官方 v3.3.3 Windows 普通 amd64 ZIP 的 SHA-256 **b00441624a66ebed94a2ddcc01ab2832069f053e6ca0abf19d7104b84fad1f78** 与该 Release checksums.txt 一致。`go version -m` 的 VCS revision 与源码 archive comment 相同，x/core 版本也一致。校验和同源一致证明完整性，不额外声称已做发布者独立签名验证。
- 已有 Python 标准库充当唯一 `127.0.0.1` SOCKS5 上游；目标为 `127.0.0.1:443`，上游仅作合成 echo，**不连接该目标**，无 DNS/网站/真实代理。子进程 CREATE_NO_WINDOW，不占用户窗口/键盘/剪贴板。
- 启动 **167 ms**；未认证 CONNECT 为 **407**；本机随机认证通过后 CONNECT **200**；上游实际收到一次正确用户名/密码认证与一次 CONNECT；**13,824 字节** 双向 echo 一致。
- **半关闭检查失败**：mock 确认收到 FIN，但客户端没有收到 mock 在 FIN 后发送的尾数据；结果 `half_close_tail_received=false`。这只描述该固定二进制和夹具的观察，不据此宣布 GOST 所有场景有 bug，更不把它等同昨天真实上游的 10054/timeout。
- stdin EOF 后引擎仍存活。仅停止自己创建的 PID：Windows TerminateProcess 后 **3 ms** 退出并被 wait 回收，监听端口关闭；退出码 **1**。这不是正常/graceful Stop 已通过，父进程异常退出的子进程回收也未验证。
- 引擎日志 **0 字节**，未包含合成用户名/密码。夹具 errors=0，child_reaped=true。**PoC 总退出码 1，passed=false，原样保留失败证据，不把局部成功写成全绿。**

未做真实 Muse 登录/网站/代理探测、TLS/域名/IPv6/大流量/并发压力、Mac 原生或应用更新实测；不重跑旧 #19 诊断，也未安装任何工具链。此次研究已足够说明接口与接入缺口，未继续扩大试验。

## 下一步的具体工作（尚未实施）

详细接口设计见 `docs/superpowers/specs/2026-11-25-proxy-engine-adapter.md`。建议协调者先审阅这四项，再决定是否授权结构改动：

1. 把半关闭失败缩小到夹具/包装连接/上游实现的确切层次，并修正或选择已验证版本；不能通过删掉失败断言来宣称适配完成。
2. 选择受控 runner/IPC 生命周期，完成随机端口真实 ready 回执、私有配置传递、Windows Job Object + 正常 stop/wait、Mac SIGTERM 超时退出及账号关闭时阻断。
3. 核定核心和传递依赖 LICENSE/NOTICE 清单、三目标可复现产物/版本哈希，批准安装包体积与两个 Mac 架构签名成本；现有签名仍为 ad-hoc，开发者签名/公证不是本轮已完成项。
4. 同一授权变更内调整 Windows 文件白名单、受限路径、引擎架构/哈希核验、整体原子更新/回滚，Mac 先签 helper 后签 Bundle，保留 v1 迁移和账号资料边界。

**真实故障边界：** 上游 socket read 10054 表示连接被重置的观察，后续 TCP timeout 是另一阶段的不可达/超时观察；单凭这两者不能归因 Muse 桥接。成熟组件仍连接同一提供方、网络和目的站，可能遭遇相同故障；本研究没有证明替换能根治。

## 可复核来源（官方原文/固定版本）

公开 Release HTML、expanded-assets HTML、官方源码 archive、crate 与元数据已保留于 `B/sources/`，manifest 记录 URL/时间/字节/哈希。GitHub 匿名 API 因共享额度返回 403，Exa MCP 也额度耗尽，均未索取 token 或换账号绕过；后续改用普通公开发布页、codeload、crates.io 和上游源码。需要认证读取/远端操作时仍需用户自行 `gh auth login`；本轮没有认证或远端写操作。

- [G1] GOST stable Release（latest 重定向已核对）：https://github.com/go-gost/gost/releases/tag/v3.3.3 ；资产原文：https://github.com/go-gost/gost/releases/expanded_assets/v3.3.3
- [G2] 三目标构建和 MIT 原文：https://github.com/go-gost/gost/blob/3d514c57230bfa524aa89087da5580b314f1cd36/.goreleaser.yaml ；https://github.com/go-gost/gost/blob/3d514c57230bfa524aa89087da5580b314f1cd36/LICENSE ；https://github.com/go-gost/gost/blob/3d514c57230bfa524aa89087da5580b314f1cd36/go.mod
- [G3] stdin 配置解析：https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/config/parsing/parser/parser.go#L255-L289
- [G4] SOCKS5 认证/扩展/地址接口：https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/connector/socks/v5/connector.go#L42-L165 ；https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/connector/socks/v5/metadata.go
- [G5] 配置结构：https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/config/config.go ；HTTP 入站认证：https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/handler/http/auth.go
- [G6] TLS 默认和 serverName：https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/internal/util/tls/tls.go#L174-L211 ；https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/config/parsing/node/parse.go#L350-L416
- [G7] CONNECT 与半关闭路径：https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/handler/http/connect.go#L32-L99 ；https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/internal/net/pipe.go
- [G8] 服务关闭与 runner：https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/service/service.go#L398-L424 ；https://github.com/go-gost/gost/blob/3d514c57230bfa524aa89087da5580b314f1cd36/cmd/gost/program.go#L170-L198 ；x 许可证：https://github.com/go-gost/x/blob/006b432a7d0472d49ec3ca77a37a133c71e8c0f3/LICENSE
- [T1] 3proxy 1.0.4 Release 和实际维护记录：https://github.com/3proxy/3proxy/releases/tag/1.0.4 ；https://github.com/3proxy/3proxy/blob/e28d86d7e415057186c913705150d5a04c55a4fb/CHANGELOG
- [T2] 核心许可原文：https://github.com/3proxy/3proxy/blob/e28d86d7e415057186c913705150d5a04c55a4fb/copying
- [T3] auth/parent/socks5+/stdin：https://github.com/3proxy/3proxy/blob/e28d86d7e415057186c913705150d5a04c55a4fb/man/3proxy.cfg.5#L640-L680 ；同文件 #L887-L1076；https://github.com/3proxy/3proxy/blob/e28d86d7e415057186c913705150d5a04c55a4fb/src/3proxy.c#L530-L555
- [T4] TLS 验证默认值：https://github.com/3proxy/3proxy/blob/e28d86d7e415057186c913705150d5a04c55a4fb/man/3proxy.cfg.5#L1526-L1566
- [T5] Windows SSL 依赖构建、Mac 构建与 Darwin 分支：https://github.com/3proxy/3proxy/blob/e28d86d7e415057186c913705150d5a04c55a4fb/.github/workflows/build-win64.yml ；https://github.com/3proxy/3proxy/blob/e28d86d7e415057186c913705150d5a04c55a4fb/.github/workflows/c-cpp-MacOS.yml ；https://github.com/3proxy/3proxy/blob/e28d86d7e415057186c913705150d5a04c55a4fb/CMakeLists.txt#L219-L237
- [R1] 官方 crate 版本/下载记录：https://crates.io/crates/fast-socks5/versions ；原始元数据：https://crates.io/api/v1/crates/fast-socks5
- [R2] 1.0.0 维护记录与 MIT：https://github.com/dizda/fast-socks5/blob/bae2fc05f993d3a247b5435ba7da221f3fd80a3c/CHANGELOG.md ；https://github.com/dizda/fast-socks5/blob/bae2fc05f993d3a247b5435ba7da221f3fd80a3c/LICENSE
- [R3] 固定源码认证/错误/流 shutdown：https://github.com/dizda/fast-socks5/blob/bae2fc05f993d3a247b5435ba7da221f3fd80a3c/src/client.rs#L65-L96 ；同文件 #L204-L243、#L656-L680
- [R4] 固定 API 文档：https://docs.rs/fast-socks5/1.0.0/fast_socks5/client/struct.Socks5Stream.html ；依赖：https://github.com/dizda/fast-socks5/blob/bae2fc05f993d3a247b5435ba7da221f3fd80a3c/Cargo.toml
- [S1] shoes 辅助初筛：https://github.com/cfal/shoes/releases/tag/v0.3.8
- [P1] 当前桥接：https://github.com/Kongbai333/muse-desktop/blob/7ac49491e8f28152ca94679d3c4381813df44438/crates/muse-proxy/src/lib.rs
- [P2] 账号路由/桥接生命周期：https://github.com/Kongbai333/muse-desktop/blob/7ac49491e8f28152ca94679d3c4381813df44438/crates/muse-desktop/src/browser.rs
- [P3] Windows 更新白名单：https://github.com/Kongbai333/muse-desktop/blob/7ac49491e8f28152ca94679d3c4381813df44438/crates/muse-desktop/src/updater_download.rs#L362-L367
- [P4] Mac 签名/架构：https://github.com/Kongbai333/muse-desktop/blob/7ac49491e8f28152ca94679d3c4381813df44438/scripts/package-macos.py ；https://github.com/Kongbai333/muse-desktop/blob/7ac49491e8f28152ca94679d3c4381813df44438/crates/muse-desktop/src/updater_install.rs#L221-L252
- [P5] 保护存储/迁移：同提交 `crates/muse-desktop/src/secrets.rs`、`secrets_macos.rs`、`account_transfer.rs`。
