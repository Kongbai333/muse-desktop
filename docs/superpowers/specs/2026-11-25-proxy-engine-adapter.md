# 独立代理引擎最小适配草图（研究交付，未实施）

相关选型/证据：`../verification/2026-11-25-proxy-engine-selection.md`。A 功能提交固定为 `81ef39137ed6dc3e4fba9e481063d803f32b2604`，本草图不授权替换内核、添加引擎依赖或发布。

## 路线与准入状态

下一阶段优先评估 **GOST v3 独立进程**，保留既有 Rust 原生桥接作为已存在实现；不做 Go c-shared/FFI，不因引入一个 Rust SOCKS 客户端就声称转发层已整体成熟化。本轮 GOST v3.3.3 单次纯回环 PoC 的 HTTP/SOCKS5 认证和 echo 成功，但半关闭尾数据失败，Windows 正常退出未验证，因此当前 **不准入产品**。

## 局部接口，不改存储协议

在 `muse-proxy` 定义账号所有的句柄接口，`BrowserPool` 继续以 AccountId 缓存 Arc，同账号标签共享；不要以代理池 ID 复用跨账号子进程。接口示意（不是已存在 API）：

```rust
trait AccountProxyHandle {
    fn address(&self) -> SocketAddr;
    fn local_auth(&self) -> Arc<LocalAuth>;
    fn health(&self) -> EngineHealth;
    // request_count for external engines must be Option, never a made-up zero.
    fn request_count(&self) -> Option<u64>;
    // Explicit async close + wait; Drop only provides an owned-child failsafe.
}
```

原生实现包裹 ProxyBridge；外部实现持有 Child、私有 IPC、账号/实例 nonce、实际监听地址、健康通道、退出回收任务和启动所用 upstream revision。`LocalAuth::new` 当前私有，需在此边界生成并提供受限构造接口；Windows `install_local_auth` 与 Mac `install_proxy` 改为只依赖地址/认证契约，而非具体 ProxyBridge。浏览器 tab/profile、网络 force_proxy、账号 generation、保护存储、迁移 v1 不变。

## 一条明确的进程协议

1. 校验随包引擎路径、三目标原生架构、固定版本/哈希和签名；不用 PATH 搜索或 shell 拼接，不允许账号提供任意引擎路径/参数。
2. 每账号一个进程，配置由 ProtectedStore 在父进程内解密，经只继承指定句柄的匿名 stdin 管道一次传入；argv、环境变量、stdout/stderr 和临时明文文件不承载秘密。stock GOST 支持 `-C -`，但没有本项目所需完整 ready/stop 协议。
3. 建议薄 Go runner 使用固定 core/x 版本，先在 `127.0.0.1:0` bind，再通过独立 IPC 回传 `{instance_nonce, actual_addr, ready}`；父进程校验 nonce 和回环地址后才创建 WebView。不要关掉预占端口后假定另一个进程能抢回同端口，不解析人类日志当稳定 API。
4. 配置固定 HTTP handler + 独立随机本机 auth + 单一强制 upstream chain。普通 SOCKS5 为 connector=socks5、dialer=tcp、notls=true；HTTP 为 http/tcp；HTTPS 为 http/tls、secure=true、正确 SNI/根。密码按 JSON 原字符传递，不手工拼 URL 或双重解码。维持旧空密码处理的兼容性需要单独核对，不能静默改变。
5. 禁用 admin/API/metrics/recorders、debug/trace、sniffing/MITM、direct fallback、全局 bypass、自动轮换与远端配置加载；并发/建连/读写上界显式对齐现有契约。启动 readiness 只能本地鉴权检查，不自动触发真实上游测试。
6. runner 经控制管道接收 Stop，取消所有该账号连接、关闭 listener、等待 handler，再回传 Stopped。Windows 父进程用 Job Object 的 kill-on-close 防孤儿、正常退出后 wait；到 deadline 才终止自己持有的 Child。Mac 独立进程组先 SIGTERM、限时 wait，必要时杀自己的进程组。不能通过停止其他 Muse 实例或清端口解决冲突。
7. 引擎退出/IPC 丢失/版本不符 => 标记该账号失败、暂停/隐藏关联 WebView 并失效句柄；不切系统网络、不换出口、不沿用其他账号句柄。凭据变化生成新实例，旧连接明确失效。

## 明确需要先补齐的证据

- 解释并解决 PoC 半关闭尾数据失败，补齐取消与现有长连接契约；源码有 CloseWrite 不能代替结果。未通过前不接入。
- 正常 stop/父进程异常退出/启动失败的回收、active CONNECT 取消、端口释放；本轮只证明终止自己的 stock 子进程后已 wait 且端口关闭。
- TLS 正确/错误 CA、域名远端解析与 IPv6、不同账号不可串用、本机随机认证不可省略；这些是下一次授权的最小适配验证，不回到旧真实账号诊断。
- Windows x64 与 Mac x64/arm64 的固定构建/签名，Go 配置副本生命周期、内存/句柄成本及全部传递 LICENSE/NOTICE 清单。

## 包装和更新不可省略

- Windows 可放应用专用 `engines/` 子目录，但目前 `windows_app_file` 会拒绝此文件；必须显式白名单受限路径并核验架构/哈希，同时纳入 staging/备份/整体回滚。旧 updater 对新包的兼容升级路径要设计，不能先发一个会被旧版拒绝的包。
- Mac 每架构单独原生 helper，放在 Bundle 的标准嵌套代码位置（候选 `Contents/Helpers/`，需最终签名验证）。先签 helper 再签外层 App，并验证 deep/strict；未来 Developer ID/hardened runtime/公证需另行授权，当前 ad-hoc 状态不能冒称正式签名。
- 关闭并回收所有引擎后才开始更新，主程序与引擎版本一起替换/回滚；更新失败保留可启动的同一组合。不得把引擎生命周期文件、秘密或用户 profile 放进安装包/迁移 v1。

上述均是可审阅的后续工作边界。本轮没有创建 runner、加入引擎、改打包/更新/CI，或执行远端步骤。
