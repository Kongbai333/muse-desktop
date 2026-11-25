# Windows 代理结果交付：实现与运行证据

本记录接续保留的 A `81ef39137ed6dc3e4fba9e481063d803f32b2604` 与 B `758e4af156de282de0a389b1e8038bf95e031257`，不重写历史研究结果。本轮按用户追加要求收敛 Windows-only，平台清理与候选版本/打包分别提交。

## 已落地

SOCKS5 客户端嵌入 fast-socks5 **1.0.0**，保留现有 Hyper 1 的 HTTP/CONNECT 和 rustls 证书验证。目标域名仍交上游解析，代理服务器本身通过 Tokio 异步解析/连接；保留含特殊字符、无认证及 token-only 空密码映射的兼容性，错误固定映射不输出第三方错误中的用户名。

账号桥接使用 tokio-util **0.7.20 CancellationToken/TaskTracker** 管理连接、HTTP driver、upgrade 和 tunnel，取消覆盖认证/建连等待，显式 shutdown 等待全部所属任务退出；最后一个句柄释放启动同一路径。listener 异常关闭同时取消子任务，BrowserPool 拒绝复用已停止句柄。原 96 连接/96 隧道、15 秒 deadline、回环随机认证、force_proxy、绑定与冻结迁移 v1 不变。

无外部代理 EXE、Go 工具链/子进程协议或额外更新器文件。新增组件原始许可随包位于 LICENSES/，现有 Muse.exe + LICENSES 打包/更新路径可继续使用。

## 实际通过

- 任务外一次性产品驱动：认证 SOCKS5 CONNECT、15,360 字节双向传输、FIN 后尾数据、域名无本地解析、IPv6、普通 HTTP 转发、无认证和 token-only、错认证返回 502 且目标未被直连。
- 活动隧道和认证等待中的取消均正常完成 shutdown + wait，监听端口关闭，合成上游连接全部回收；随机本机认证和跨账号认证隔离通过。
- 现有定向 TLS 检查仅运行一个 case：已验证证书的 HTTPS 上游普通 HTTP 与 CONNECT 成功，不信任证书拒绝。退出 0，1 passed；未跑全套测试或旧矩阵，未新增永久用例/框架。
- 隐藏的真实 GPUI/WebView2 产品驱动：UTF-8 BOM/CRLF TXT → 实际导入子屏预览（3 有效/2 重复/2 错误）→ 取消不变 → 确认 → 受保护保存/重载，逐项认证去重、密码冒号/末尾空格和 IPv6 保留；原 ID/绑定/generation/指纹未改，保存失败回滚通过。
- 同一原生账号两标签共用桥接、两个账号共用池配置却使用独立随机端口/认证、移除一个账号关闭其 listener 且另一个不受影响通过；应用正常退出后合成上游 active=0，无合成秘密落日志，无剪贴板或前台输入操作。

## 证据位置与边界

协调者指定报告根的 `result/` 保留驱动源码、原始构建日志、`proxy-contract.json`、`native-result.json`、`tls-contract.log` 及退出码。失败夹具记录原样保留：Windows 同步 mock 接受 socket 需恢复 blocking；匿名账号须通过产品创建路径使用本地 home；预览夹具从实际已保存 draft 派生重复行，显式构造非法百分号。断言未删除，产品认证/TLS/路由 guard 未放宽。

这些驱动只暂时编译进验证版，候选源和最终 EXE 不含驱动入口。没有读取真实 Cookie/profile/vault 或改用户安装。

真实当前线路没有被宣称已修复：唯一运行安装未提供可唯一确认的绝对 `--data-dir`，只读安全选择器在加载凭据前停止，没有扫描其他根或发起真实代理/网站请求。该事实不阻断合成契约已通过的 Windows 候选产物交付，也不等同确认提供方可达。
