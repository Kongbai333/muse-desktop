# 本轮 Windows-only 收敛

用户在结果任务中明确要求去掉 Mac 兼容，覆盖此前保留 Mac/不改 CI 的限制。本提交只处理该平台范围，独立于代理实现与候选版本提交。

- 应用显式仅构建 Windows x64；删除 WKWebView、Mac 原生迁移适配、Keychain、Mac host/caption、持久化 store namespace 等专用实现及直接依赖。Windows WebView2、DPAPI、profile 目录核验、readiness、账号/指纹/代理绑定保持。
- 删除 Mac 打包、smoke 脚本与其专用打包测试；去掉 Mac 证书/Bundle/Finder 更新交接、Mac 下载选择和当前支持说明，更新器仅接受 Windows x64 ZIP，保留 PE 架构、路径、哈希、文件白名单、应用文件原子替换/回滚。
- CI 删除 Mac 双架构任务及发布依赖，下载/发布资产限定 Windows ZIP/SHA256；Windows 格式化、浏览器适配检查、Rust 测试、打包检查及构建保持，使用既有应用 `--smoke-test` 增加 Windows 原生启动/本地浏览器检查，没有绕过 Windows guard 或执行远端 workflow。
- 不修改通用 core 模型、冻结迁移 v1、Android、历史报告或 Git 历史；不操作已有远端 Mac tag/Release 资产或任何用户安装与资料。Cargo.lock/第三方仍可保留传递依赖的其他平台支持，不 fork 通用依赖。

本机直接验证：Windows 应用 `cargo check` 退出 0；仅执行与更新平台选择有关的既有 `updater::` 三项测试，3 passed / 117 filtered，退出 0；未跑全套/旧矩阵，未新增永久用例或框架。最终候选仍以 result 目录的最终构建、原生结果和产物校验为准。
