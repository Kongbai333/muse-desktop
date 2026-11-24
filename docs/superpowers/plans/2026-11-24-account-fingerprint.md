# Account Fingerprint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement inline. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为各账号提供可持久化、稳定、可关闭且覆盖明确的基础Canvas/WebGL读取指纹。

**Architecture:** core保存模式；desktop从账号ID派生域分离种子并通过Wry在主窗口/iframe首次脚本前安装读取适配器。账号设置复用既有事务和profile，不改代理/迁移wire。

**Tech Stack:** Rust/serde/ring、原Wry/GPUI、无新增依赖；Node定向行为测试及本机真实WebView合成验证。

**Spec:** docs/superpowers/specs/2026-11-24-account-fingerprint-design.md

## Global Constraints

- 版本保持0.1.2；冻结v1字节不改；保留544c3e1及当前feature分支。
- 不访问真实账号/profile/vault/备份，不对真实代理发探测；不操作用户窗口。
- 不另派worker，不推送/main/tag/dispatch/Release。
- 明示worker/音频/字体/TLS等未覆盖，不修改UA/Client Hints与设备型号。

## Review Focus

- 旧workspace缺字段、非法枚举：旧值可读，非法值失败关闭。
- 同账号重建/跨标签：固定种子，不引用tab/generation，跨账号不同。
- 原生Canvas错误、透明/越界/大尺寸：保留原生安全边界和可用性。
- 图像导出和读取：一致坐标扰动，不二次扰动，不修改绘制缓冲。
- 自定义WebGL帧缓冲：失败或成功都保留原生输出及错误状态，不读取getError。
- 账号设置失败/取消：模式与其余字段原子提交；不丢登录数据。

---

### Task 1: 持久化模式与稳定派生

Files: muse-core/src/lib.rs、muse-core/tests/fingerprint.rs；desktop/src/browser_fingerprint.rs。

Interfaces: core FingerprintMode(Basic/Original)及Workspace::set_fingerprint_mode(id,mode)->Result<bool>；desktop initialization(account_id,mode)->Option<String>。

- [ ] 先写旧数据默认/模式持久化、同值不换generation、变值只更新本账号、溢出原子性的RED并运行。
- [ ] 加入最小模型和派生；测试不同账号与generation独立性，不输出UUID。
- [ ] 运行cargo test -p muse-core --locked --test fingerprint及desktop目标测试，记结果。

### Task 2: Canvas/WebGL读取适配与真实原生验证

Files: desktop/src/browser_fingerprint.js、tests/browser-fingerprint.test.cjs、browser.rs及BrowserRequest所有构造点。

Interfaces: JS仅接受8个u32的派生种子；Basic返回初始化脚本，Original无脚本；不暴露全局seed对象。

- [ ] 写定向JS RED：读值稳定/跨账号不同、像素不变、export与read一致、透明和异常、默认帧缓冲RGBA8/WebGL packing边界；隐藏原生GL2探针确认多采样/整数附件失败读回不改目标数组，错误状态保留。
- [ ] 实现稀疏确定扰动与有限克隆导出，并在Wry全frame初始化入口接入。
- [ ] 用真实隐藏WebView/合成profile检查上述主要读回与iframe、重建；不以mock替代原生。

### Task 3: 账号设置与最终验证

Files: account_dialog.rs、proxy_pool.rs、必要文档/验证记录。

Interfaces: 原子save_account_with_fingerprint(...,mode)，保持SavedAccount::browser_changed语义。

- [ ] 写事务失败/只改模式/不改模式RED，运行，再接入开关及范围文案。
- [ ] 复用既有checkbox/主题；以合成账号检查取消/保存、昼夜布局，记录截图。
- [ ] 一次必要cargo test --workspace --locked、Node相关测试、fmt/diff check及本地debug/release构建；精确保留Mac未运行边界。
- [ ] 检查无诊断/秘密/真实资料入git，做主题本地提交，向协调者给完整HEAD/日志/限制。
