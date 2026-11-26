# Windows 系统回环代理前置实施记录

前候选本地检查点：e472029；原交付目录 D:/orca-task-builds/windows-native-proxy-20261126 保持不变。新资源目录 D:/orca-task-builds/windows-system-transport-20261126。

用户已指定方案与执行方式：显式 --proxy-via-system 从 Windows 只读获取当前启用的静态单回环 HTTP 代理，只有绑定代理的账号改走本机桥；外层 CONNECT 只到该账号保存的代理端点，不提供账号认证，然后复用原 HTTP CONNECT / HTTPS TLS / SOCKS5。普通入口与显式系统网络账号不变。

- [x] 保存前候选历史，核对静态配置形态（仅脱敏结果）。
- [x] 在 muse-proxy 统一账号代理拨号入口，加入只读、启动期不可变的前置配置与明确失败关闭；httparse 标准解析器限制总响应头 32 KiB、128 个头和 8 次临时响应，保护两层响应与过读字节。
- [x] Windows 启动通过 WinHttpGetIEProxyConfigForCurrentUser 只读配置，账号路由/兼容探测共用，日志使用固定枚举与匿名路由 ID。
- [x] 项目外 7 个真实 WebView 场景、18 个拨号/解析边界场景和 13 种静态配置形态通过；未增项目测试或扩大 CI。
- [x] 已移除全部驱动并逐文件哈希核对还原；最终开发构建 EXE 经普通入口分别开启前置、关闭前置验证页面就绪，真实 Windows 静态代理只读配置与同一合成账号资料均通过；清理 target/runtime/tmp 释放 4,703,698,584 字节（4.381 GiB），源码和交付证据保留。

不改 Windows 网络、不改账号绑定/存储格式、不自动切出口、不发布/安装/合并、不增项目测试或修改 CI。优先复用已有依赖缓存并交付开发构建；构建和运行证据均在新的 D 盘目录。
