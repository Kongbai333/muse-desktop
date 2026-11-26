# #42 Windows 浏览器原生连接层本地候选

2026-11-26，本地 Windows x64，版本保持 0.1.5。最终优化版 EXE：`D:/orca-task-builds/windows-native-proxy-20261126/artifacts/Muse.exe`，22,779,904 字节，SHA256 `4443eb6b0ca474bb0945d66af6482421fa61983d4d206ee2aaeea8b91d5587ec`。这是独立本地候选，没有替换现用安装，没有远端发布或合并。

## 基线与源码

新 Orca 工作区 `Kongbai333/windows-native-proxy` 的起点和旧 `proxy-batch-and-engine` 的 HEAD 均为 `d94d5729dfd09f8527dbe54ca26fd410f3764855`。交接的 `connect-stage/product-tracked.diff` 先通过 git apply --check，再应用；6 个跟踪文件及两个补充诊断模块逐文件 SHA256 与旧工作区一致，见 baseline.json。交付前复核旧工作区这 8 个文件和 HEAD 均未改变，见 old-workspace-preserved.json；旧诊断 EXE、分支、原始现场证据及安装均保留。

源码：`artifacts/Muse-0.1.5-native-proxy-source.zip`（完整跟踪源码加 3 个连接/诊断新模块与实施记录）；跟踪文件差异：product-tracked-final.diff。没有变更 Cargo.lock、账号序列化/加密格式、版本号、CI 或安装目录。已有合成夹具仅补齐标准 407 挑战，以及用实际路由/夹具流量替代旧的“必须存在桥计数”判断；未新增项目测试用例。

## 实际连接入口

| 配置 | 实际入口 | 凭据范围 |
| --- | --- | --- |
| HTTP，无认证 | native_http | 无上游凭据进入回调 |
| HTTP，Basic 认证 | native_http | WebView2 407 URI 精确匹配协议、主机、端口、根路径，且无 userinfo/query/fragment |
| HTTPS，Basic 认证 | native_https | 同上；代理 TLS 由 WebView2 校验 |
| HTTPS，无认证 | native_https | 同一个原生 TLS 入口，未另列无认证 HTTPS 的完整合成轮次 |
| SOCKS5，无认证 | native_socks5 | Chromium 的 SOCKS5 入口，无本机桥 |
| SOCKS5，认证 | socks5_auth_bridge | 保留原本随机本机认证到 Rust SOCKS5 的兼容入口 |
| 认证 HTTP/HTTPS，旧或未知运行时 | http_auth_compatibility_bridge / https_auth_compatibility_bridge | 保守沿用已存在的兼容桥；本机只有新运行时，未另装旧 Runtime 做端到端试测 |

原生认证保守要求 Runtime ≥ 122.0.2365.46，创建后再次读实际版本，并要求 ICoreWebView2_22 接口。版本门槛或范围接口不满足时绝不猜测站点身份。未通过接口安装的浏览器不会加载账号页面，也不会回退直连。原生认证当前仅响应 Basic；其它认证挑战取消。

原生路径没有创建 ProxyBridge；账号持有不可变 AccountRoute，同账号普通页、重复页、弹窗重新开的标签以及内部读者共用同一入口与 WebView2 环境。静态代理 ID、DPAPI 受保护存储、实际 profile 目录核验、整账号回收与重建、时区和语言逻辑保留。代理口令只留在受保护文件和进程内存，不进入参数、URL、普通日志或站点认证响应。

认证入口另用 AddWebResourceRequestedFilterWithRequestSourceKinds(ALL) 阻止把配置的代理端点自身当作普通网页源，包括 iframe 与 Service Worker。此限制是为了消除代理端点根 URI 自身返回站点 401 的歧义；因此该端点本身不能再作为普通网站浏览。

force_proxy 继续移除 Chromium 的隐式 loopback 绕过；未强制时保留原有规则。固定代理参数没有 DIRECT/PAC 后备。受保护凭据丢失时未创建 WebView；代理停止或密码错误时账号页面失败，配置不改为直连。

## WebView2 合成验收

本机 Runtime 158.0.4370.33。复用交接的 driver_entry.rs、ui_driver.rs、hook_driver.py、run_native.py/run_http.py，副本与修改后的夹具只在本次 D 盘 evidence 目录；未使用真实 Cookie、账号登录、用户 profile、公开目标、hosts 修改或系统证书库修改。

| 轮次 | 结果证据 | 验证结果 |
| --- | --- | --- |
| 认证 HTTP | http-auth-result.json | native_http；407 精确代理 URI；同 realm 两站点 401 均未收到代理凭据；阻止代理端点页面/路径/iframe/Service Worker |
| 认证 HTTPS | https-auth-result.json | native_https；相同认证范围验证通过；TLS 合成证书仅由外置驱动临时 SPKI 放行 |
| 无认证 HTTP | http-result.json | native_http；跨主机 HTTP→HTTPS 跳转就绪，重复加载/弹窗/代理停启成功 |
| 无认证 SOCKS5 | socks-result.json | native_socks5；合成域名经 SOCKS 传递，浏览器路径就绪与恢复成功 |
| 认证 SOCKS5 | socks-auth-result.json | socks5_auth_bridge；沿用桥的页面、跳转、重复页、启停和恢复通过 |
| 原生 HTTPS，无证书信任 | https-untrusted-auth-result.json | 不放行 SPKI，TLS 失败；没有代理认证事件，没有站点请求，没有直连 |
| 错误认证 | http-bad-auth-result.json | 代理拒绝密码后页面失败；没有站点请求，没有直连 |
| 受保护凭据缺失 | http-missing-auth-result.json | 保存的静态绑定保留，但未创建账号 WebView，没有连接 |

各正向轮次均包含同一账号两个代理端点/不同凭据与相同 realm 的切换、原 profile canary 保留、显式 en-US/Asia/Shanghai 保留、两次连续 reload、跨主机跳转、弹窗、重复页面、代理真实停止/重启。正向轮次站点 Authorization/Proxy-Authorization 泄漏均为 0，跨端点错误凭据为 0，停止代理后可直接访问的 loopback canary 命中为 0。原生 HTTP/HTTPS 均记录 document 来源 1 与 Service Worker 来源 4 的端点拦截。所有这些轮次的自有浏览器进程已退出。

早期夹具将 HTTPS 自签名代理的 Rust 兼容探测也要求成功，以及将桥停止返回的 502 错当成必须为 0 的原生状态；修正的是夹具验收口径。保留 routes-run2.log/routes-run3.log 的原始记录；以各协议最终 *-result.json 的 passed 为准。兼容探测拒绝该自签名证书恰好说明它与页面的原生入口是不同检测层。

## 最终交付 EXE 验证

移除所有临时钩子后，以 `cargo build --release --locked --offline -p muse-desktop` 构建成功；GPUI_FXC_PATH 指向本机已有 Windows SDK 10.0.22621.0/x64/fxc.exe。首轮优化构建未找到 fxc 的环境错误保留在 build-release-environment-failure.log；未修改依赖、CI 或安装工具。最终 release 与 debug 构建退出码均为 0，分别只有现存的 10/7 条 unused/dead_code 警告；cargo fmt --all -- --check 和 git diff --check 均通过。

normal-entry.json：最终 EXE 仅用普通参数 `--proxy-trace --update-background --data-dir <新合成目录>` 启动，窗口与工作区创建，没有抢前台，没有自动导航，WM_CLOSE 正常退出 0，trace_start/trace_stop 完整。

final-page-result.json：同一个 SHA256 的最终 EXE 再通过普通入口打开已保存的合成账号、相同 DPAPI 代理绑定和原 profile 目录，使用默认语言/时区分支；原 canary 未变，页面就绪 1 次，原生代理认证 1 次，桥事件 0 次，站点实际收到 2 次请求，退出 0，全部自有浏览器子进程退出。该次没有驱动入口、测试环境、证书放行或合成命令行参数；夹具本身是外部回环服务。

最终 PE 检查为 AMD64 0x8664、Windows GUI 子系统。源码及最终二进制均未包含临时 MUSE_TRACE_NATIVE_DRIVER/MUSE_FIXTURE_SPKI、测试 marker、原始认证 URI 写入器或 ignore-certificate-errors-spki-list。正常 profile/参数路径与现有版本保持兼容。

## 检测边界与剩余工作

检测按钮和 --check-upstream 明确标记为 Rust 兼容连接层的 muse.ai:443 隧道探测；成功不代表目标 TLS、跳转目标或页面完成。CLI 增加 connection_layer=rust_compatibility_probe 与 browser_page_verification=account_window_only。实际入口在账号窗口和 route_selected/route_path 中确认；原有阶段诊断保留，原生传输没有伪造 Rust TCP/CONNECT 事件。

本次证明新连接架构在本地受控浏览器路径可用，尚未据此判定用户现场故障已解决。协调者仍需用同一原静态代理、同一 HTTPS 目标和 Windows 本机隔离 profile 做原版/候选 A/B；旧现场 CONNECT 读取复位与 SOCKS5 超时的已知结果继续有效。独立网关或代理端日志的来源判断也未由本次合成验收取代。

本次清理释放 3.978 GiB；删除本任务独立 target、runtime 和 tmp，打包前保留量 22.3 MiB，最终交付目录也未超过 5 GiB。清理前核实本任务程序/编译器/浏览器进程均不占用目录，旧工作区及其构建产物未清理，见 cleanup.json。

官方 API 依据由协调者交接：BasicAuthenticationRequestedEventArgs.Uri 的代理 407 语义，以及 ICoreWebView2_22 的全来源资源过滤接口；实际回环事件 URI 和来源位见对应结果中的 auth_events/blocked_resource_events。本机只验证了上述 Runtime 和 Basic 认证；不推定其它运行时、其它认证机制、所有第三方代理或真实会话登录均可用。
