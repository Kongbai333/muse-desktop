# #42 Windows 系统 HTTP 代理前置候选

2026-11-26，本机 Windows x64。本地候选 `D:/orca-task-builds/windows-system-transport-20261126/artifacts/Muse-System-Transport.exe`，42,179,584 字节，SHA256 `4c708766c86bfe965c2e99a62f24fe7ee35cbf012ee6c536a18d9b0a643c93b1`。本次按任务优先交付已实测的开发构建（dev / unoptimized），未等待优化构建。

## 启动方式

```powershell
.\Muse-System-Transport.exe --proxy-via-system --proxy-trace
```

需要独立目录对照时加现有 `--data-dir "资料目录"`；没有系统代理地址、账号代理地址或密码参数。默认资料目录、版本号 0.1.5 和账号文件格式不变。同一 profile 不要同时交给两个进程。

**不加 --proxy-via-system 就保留上一个原生连接模式。** 开关只影响已绑定账号代理的账号。显式系统网络账号仍按原有 WebView2/Windows 网络行为运行；应用更新等其它网络也未改成新账号桥。

## 源码与保留边界

在相同 `Kongbai333/windows-native-proxy` 工作区继续。已将前一可运行原生候选保存成本地检查点 `e472029`，原起点仍为 v0.1.5 `d94d5729dfd09f8527dbe54ca26fd410f3764855`。前候选目录 `D:/orca-task-builds/windows-native-proxy-20261126` 没有覆盖；其中 Muse.exe SHA256 仍为 `4443eb6b0ca474bb0945d66af6482421fa61983d4d206ee2aaeea8b91d5587ec`。

源码提交编号、全量源码包及包哈希见交付 metadata / SOURCE-BASE.txt / SHA256SUMS.txt。未安装替换、未远端发布、未合并或新建远端分支。没有修改 CI，也没有新增项目测试用例；合成驱动和组件验收均在项目外 D 盘。

## 实现

`system_transport.rs` 只读调用 WinHttpGetIEProxyConfigForCurrentUser。只接受当前用户已启用的静态、单一、数值回环 HTTP 端点；支持裸 authority、http:// authority 或单条 http= authority，拒绝非回环地址、域名形式的系统代理、多条分号配置、认证 URI、路径/query/fragment，以及 PAC URL。不会调用自动发现或更改 Windows 网络设置；自动检测不能替代必须存在的静态端点。

配置在本次启动内不可变地存于内存。读取失败、未启用、配置不支持或非回环会缓存为失败状态，绑定代理的账号不能悄悄继续原生或直连。系统网络账号不受这个账号前置状态限制。修改 Windows 系统代理配置后须重新启动候选。

`muse-proxy/src/transport.rs` 是原 ProxyBridge 与兼容探测的共同拨号入口。开关关闭时仍直接拨保存的账号代理；开关开启时只连接 Windows 的数值回环端点，并向它发送目标为**该账号已保存代理端点**的 CONNECT。外层 CONNECT 只有 CONNECT/Host，不携带账号 Proxy-Authorization、Authorization 或 Cookie。它不接受网站目标作为外层目的地，也不自动响应外层认证、跟随重定向或选择其它出口。

外层隧道成立后，原 HTTP 代理仍使用原来的账号认证/内层 CONNECT；HTTPS 账号代理先在隧道中执行原来的 Rust TLS/证书校验，再执行其 HTTP 流程；SOCKS5 仍走原协商、认证及 CONNECT。HTTP/SOCKS 的账号认证是按原协议在隧道内传输，这里验证的是**外层 CONNECT 请求未携带账号认证**，并不声称本机透明转发服务不能观察隧道字节。

开启前置时，账号路由明确成为 system_http_bridge、system_https_bridge、system_socks5_bridge。没有改写静态绑定、受保护凭据格式或 profile 路径，仍按整账号回收/重建并保持语言/时区；force_proxy 的 loopback 规则保留。字面回环同端口和本机可解析的回环别名同端口被拒绝；需要别名核验但本机无法解析时也失败关闭。

`connect_response.rs` 复用已存在于依赖锁的 httparse 1.10.1，统一解析内外两层 CONNECT。整个响应头序列合计至多 32 KiB，每次响应至多 128 个头，临时响应至多 8 次；101、无效状态/头、EOF 和超限均拒绝。仅 2xx 成为隧道，407/其它拒绝不会尝试其它出口。Prefixed IO 保留一次读取超过外层头末尾的内层字节。两层拨号、TLS/协商共用原来的 15 秒总建立预算，没有再叠一份前置预算。

## 日志与错误

继续使用 --proxy-trace 的匿名编号、固定枚举和容量上限。新增 transport=system_loopback_http、route_path=system_*_bridge，以及 system_transport_config、system_proxy_tcp、system_proxy_connect_write/read/parse、system_proxy_connect。记录状态码/耗时/系统错误号，不记录原始系统地址、账号代理地址、凭据或 URL。普通模式标记 direct_to_account_proxy。未把经外层的上游连接伪装为 Rust 直接 provider_tcp。

未启用静态系统代理、非回环、连接不可用、CONNECT 拒绝、解析失败、超时及回环各返回固定明确错误；未发送账号认证去回答外层 407。CLI --check-upstream 增加 transport_mode，并保持 page_access_verified=false / browser_page_verification=account_window_only，隧道通过不等于网页通过。

## 已完成的最小验收

所有合成目标、假代理、随机测试凭据和数据目录均由本任务控制。没有使用真实 Cookie、登录状态或用户保存账号去做合成验收，也没有修改 Windows 设置、hosts、系统信任库或安装。浏览器夹具的自签名目标证书仅在带外驱动构建中临时使用 SPKI；该参数与驱动已从交付 EXE 移除。

| 真实 WebView 场景 | 关键结果 |
| --- | --- |
| HTTP 账号代理 + 合成外层 | 8 次外层 CONNECT 200；正确账号认证 8 次；内层 CONNECT 5 次；跨主机 HTTP→HTTPS 页面就绪及 reload 通过；同路径池探测通过 |
| 认证 SOCKS5 + 合成外层 | 外层 CONNECT 200、SOCKS 认证/CONNECT、页面就绪、跨主机跳转、reload 和同路径池探测通过 |
| 外层返回 403 | 页面失败、外层收到 2 次同端点请求；内层连接与站点请求均为 0，没有直连 |
| 非回环配置 | 没有创建账号 WebView，没有内层或站点请求 |
| 同端点回环配置 | 没有创建账号 WebView，没有外层/内层连接或站点请求 |
| 显式系统网络账号 + 无效前置配置 | 原有系统网络账号页面仍就绪，未创建账号桥，外层/内层连接均为 0 |
| 不带开关普通入口 | native_http；桥事件 0，外层连接 0，跨主机页面和原生认证通过 |

以上 7 个场景全部 passed；外层 CONNECT 的认证/错误目标计数均为 0，站点认证泄漏计数为 0，自有浏览器进程全部退出。源记录为 browser-result.json 及各场景 *-result.json / *-trace.ndjson。

另有项目外 13 种静态配置形态与 18 个组件拨号场景通过，见 connect-bounds-result.json：分段 103→200、保留过读的内层 100 响应、认证 SOCKS5、403/407、无效头/状态、32 KiB 超限、129 个头、9 次临时响应、101、截断 EOF、不可用端口、同端点及 localhost 别名、非回环配置、普通直接拨号回归，以及实际约 15 秒的前置读取超时。拒绝/异常场景的账号代理连接均为 0，没有直连后备。

## 最终不带钩子 EXE

5 个临时钩子文件逐文件核对还原（hooks-restored.json 全 true）；最终 `cargo build --locked --offline -p muse-desktop` 退出 0，仅有 7 条既有 dead_code 警告。cargo fmt --all -- --check、git diff --check 通过，PE 为 AMD64 0x8664 / Windows GUI 子系统。用本任务独立 D 盘 target 并复制复用已有依赖缓存；未在旧任务目录构建。提供的 Windows SDK fxc 路径保留为可用构建环境，本次最终按开发构建交付。

最终字节 SHA256 与本报告一致；产品 crates 源码和最终二进制都已检查不含 MUSE_FIXTURE_SYSTEM_PROXY、MUSE_FIXTURE_SPKI、MUSE_TRACE_NATIVE_DRIVER、测试 marker、trace_driver 或证书放行参数。

final-entry-result.json 是同一个最终 EXE 的普通入口证据，没有驱动参数/环境或配置替身：

- 开启 --proxy-via-system：实际读取当前 Windows 静态系统代理；连接本任务自己的合成账号代理，system_http_bridge，外层 CONNECT 200 两次，页面就绪一次，正常关闭 0。
- 不带开关：在同一合成账号/同一 profile 下恢复 native_http，桥事件与前置 CONNECT 均为 0，原生代理认证与页面就绪通过，正常关闭 0。
- --check-upstream --proxy-via-system 也经真实当前系统代理到合成账号代理完成探测，transport_mode 正确；muse.ai:443 在该合成代理中仅返回隧道 ACK，没有据此声称访问公开 Muse 或 TLS 已验证。
- 绑定、DPAPI 文件字节、语言时区和 profile canary 均未改变，所有自有 WebView2 子进程退出。

最终入口夹具早期两次修正原始记录保留：一次是 Python 读取保存的代理 URI 时漏做百分号解码；一次是在本机桥 407 自动认证重试完成前过早关闭窗口。实际原生 200 已随重试到达；最终改为等待 dom_ready，未为此改动产品源码。

## 限制与交接

当前验收覆盖 HTTP 真实 WebView、认证 SOCKS5 的真实页面、共用探测及解析边界。HTTPS 账号代理复用同一拨号与原 TLS，但本轮未另做完整 HTTPS 账号代理的站点端到端验收。当前支持外层无认证静态 HTTP 单回环端点，不支持 PAC、多端点、外层认证、远程前置或外层 HTTPS/SOCKS；IPv6 回环字面量只做了配置解析检查，没有另开 IPv6 网络轮次。

本轮证明显式前置连接方式可在当前 Windows 系统代理与受控浏览器目标上运行，**没有据此判定原故障 HTTPS 目标已解决**。协调者继续用原静态账号代理、相同 HTTPS 目标、独立无登录资料做真实地址和网页验收，保留原版/前候选的失败证据。可使用本 evidence 目录的 replay_system_candidate.py：它只是在原协调者脚本副本中增加开关和前置阶段摘要，本 worker 没有运行它去访问真实账号或目标。

清理前已确认本任务编译器、应用和浏览器均不占用目标。已删除本任务 target/runtime/tmp，释放 4,703,698,584 字节（4.381 GiB）；旧候选、旧工作区及共享缓存来源没有清理。最终保留 EXE、便携包、源码、证据和哈希清单，目录未超过 5 GiB。详见 cleanup.json。
