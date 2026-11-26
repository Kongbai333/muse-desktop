# Windows 原生代理连接层实施记录

目标：从 v0.1.5 d94d5729dfd09f8527dbe54ca26fd410f3764855 交付独立 Windows x64 本地候选，保留账号格式、静态代理绑定、资料路径、时区和语言。

用户已指定 GPUI/WebView2 原生接入方案和执行方式，由当前 worker 自行实施。已排除全内核迁移；不替换安装、不发布、不合并、不改 CI。复用交接的有界合成验收驱动，不增加项目测试用例。

- [x] 核对新旧 HEAD 与诊断交接差异：8 文件逐文件 SHA256 一致，证据在 D:/orca-task-builds/windows-native-proxy-20261126/evidence/baseline.json。
- [x] 新建 browser_route.rs，账号持有不可变路由；HTTP/HTTPS/无认证 SOCKS5 原生，认证 SOCKS5 独立兼容桥；认证原生入口以运行时版本和代理端点 URI 核验作门槛。
- [x] browser.rs 使用账号路由并保持整账号销毁/重建；诊断记录实际路由，不把隧道探测当页面成功。
- [x] 复用外置驱动做真实 WebView2 回环实验：407 与同 realm 401、不同端点、普通页与弹窗、跳转、重复页、启停、资料复用、无直连与无凭据泄漏；HTTPS 与 SOCKS 分支按适用性验证。WebView2 158.0.4370.33，五条正向路径与三个额外负向入口通过；源代码测试/CI 均未新增。
- [x] 去掉临时驱动后构建最终 EXE，普通入口启动/退出并记录哈希；最终优化 EXE 也通过普通入口恢复已保存合成资料并加载原生 HTTP 页面就绪。清理独立 target/runtime/tmp 共释放 4,271,286,204 字节（3.978 GiB），保留本地 EXE、源码和证据。

安全裁定：WebView2 BasicAuthenticationRequested.Uri 在新运行时的 407 是代理服务 URI。任何无法精确归属到配置端点的挑战都不提供上游凭据；禁止把代理端点本身作为网页资源使用，避免同源 401 混淆。认证实验未通过则对应协议回到已验证桥接入口，并在路由标记中明示。

主要改动：muse-proxy/src/lib.rs 提供只读认证访问；muse-proxy/src/trace.rs 与 muse-desktop/src/proxy_trace.rs 扩展固定枚举路由标记；muse-desktop/src/browser_route.rs 管理实际连接入口；browser.rs 保持生命周期；代理检测界面说明页面在账号窗口验证。
