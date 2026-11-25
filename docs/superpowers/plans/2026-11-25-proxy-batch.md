# Proxy Batch Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline; only this worker edits. User AGENTS overrides test creation/full-suite gates.

**Goal:** 多行/TXT 安全预览、凭据感知去重、确认原子追加。

**Architecture:** 独立纯解析模块复用 ProxyDraft；proxy_pool 原子批次提交；现有 AccountDialog 抽屉增加一个子屏。

**Tech Stack:** 当前 Rust/GPUI/url/ring/Zeroizing，无新增依赖。

**Spec:** docs/superpowers/specs/2026-11-25-proxy-batch-design.md

## Global Constraints
- 仅隔离工作区源码；版本 0.1.3 保持；不改 CI、冻结 v1 或保护存储。
- 不访问真实账号、代理、profile、Cookie；不操作前台/剪贴板；无远端写入。
- 不新增测试用例/框架，不跑全套/native 矩阵；最小编译验证与自审。

## Review Focus
- 密码含冒号、百分号、空格以及 IPv6/URL 默认端口的规范化。
- 相同端点不同认证保留，批内/已保存重复排除，凭据读取失败关闭。
- 凭据或元数据失败的回滚；既有 ID/账号 generation/绑定不变。
- 默认协议变化和异步文件返回不得确认旧预览或覆盖已关闭的新对话框。
- BOM/CRLF/错误行/超限/取消均无含密日志或隐式保存。

### Task 1: 解析与原子追加
- [x] 新增 proxy_batch.rs：安全解析、有限文件读取、预览及规范键。
- [x] 扩展 proxy_pool.rs：现有凭据键、确认时重新去重、暂存新 revision、一次 commit、失败回滚。

### Task 2: 代理池子屏
- [x] 新增 proxy_batch_dialog.rs，扩展 AccountDialog/PoolScreen 与按钮、导航。
- [x] 粘贴/TXT 立即预览，默认协议重解析，确认后显示数量；关闭时释放原文。
- [x] README 补充格式与安全边界。

### Task 3: 交付
- [x] 格式化/差异自审；cargo build --locked -p muse-desktop（独立 target）通过，保留原始日志及退出码。
- [x] 无批量入口的既有安全后台原生验证，记录 UI/行为未测，不新增运行入口/测试。
- [x] A 单独本地提交，完整 HEAD、基线及产物记外部 A/report.md；阶段回报后才做 B。
- [ ] B 结束后清理本任务 target/临时目录，保留证据并报告空间（外部报告记录，避免重写 A）。

## Execution ledger
- 基线 7ac49491e8f28152ca94679d3c4381813df44438；隔离分支干净；本地 origin/main 相同；2026-11-25 git ls-remote 读取远端 main 也相同，不代表之后无更新。
- Ruling: 用户明确禁止新增测试与另派 worker，因此只做构建和作者自审，不执行技能默认的 TDD/独立 reviewer/全量测试。
- Ruling: 复用既有点击粘贴与系统选择器，预览承担逐行展示；不新增框架/编辑器，源文本仅保留于 Zeroizing 内存。
- Final review: 作者自审，未派 reviewer（任务明确唯一 worker）。逐项检查格式/认证去重/事务/异步返回隔离/取消路径；未发现阻塞问题。这不是运行验证。
- Build: 2026-11-25 Windows x64，首轮独立 target 构建 1m30s、退出 0；最终来源一致性增量确认 0.57s、退出 0。9 条既有未使用代码警告；fmt --check、diff --check 退出 0。无新增测试/依赖/CI/版本修改。
