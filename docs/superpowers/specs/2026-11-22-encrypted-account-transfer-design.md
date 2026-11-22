# PC 加密账号与静态代理迁移设计

## 已批准范围

依据协调者 `encrypted-account-transfer-approved-design.md` 和用户“加密吧”的确认，在原 Cookie 导入提交 `1dd9c72` 上增量实现 PC/Android 双向账号迁移。原 Cookie header 入口和 muse2api 剪贴板导出保持兼容。代理统一按静态配置处理。

账号管理中选择账号后设置、确认迁移密码，导出 `.muse-account` 加密文件。导入先选择文件、输入密码，认证及完整校验后预览账号、代理和路由差异，确认后新建隔离账号。账号名、Cookie、实际上游代理凭据和绑定关系全部位于加密负载。文件及 KDF 操作在后台执行，密码不进入配置、日志、命令行或明文临时文件。

## 协议 authority 与冻结门

Android 原 worker 负责 `docs/protocols/account-transfer-v1.md` 和 `checks/fixtures/account-transfer-v1/`。PC 只复制经两端复核的相同规范，不自行改参数。当前候选为 PBKDF2-HMAC-SHA256 600000 次、16-byte salt、32-byte key；AES-256-GCM 12-byte nonce、16-byte tag 追加在密文后；固定 AAD、规范 Base64 和外层顺序严格按协议。密码 UTF-8 原字节，不 trim/正规化。先通过 Java 固定/随机包，再用 Rust 随机包反向解密，双方确认后冻结。

## 边界与文件地图

- `account_transfer.rs`：严格 envelope、认证加密、typed payload/字段引用校验、密文文件读写；维护库 `ring` 复用锁定版本。敏感 owned buffer 尽早清零，不派生 Debug。
- `transfer_plan.rs`：包内引用到新本地 UUID 的候选图、代理 vault 分阶段写入/清理、目标旁路策略、预览信息、提交前冲突校验。
- `transfer_ui.rs`：账号选择/密码确认/文件对话框/导入预览和确认；后台任务用 generation + 弱 entity 拒绝过时结果。
- `browser.rs` / `browser_macos.rs`：原生本地 Cookie 快照读取，候选 about:blank 路由及 Cookie staging/验证、失败清理；不为导出导航 Muse。
- `muse-core/src/lib.rs`：只添加必要、向后兼容的迁移来源记录；来源标记不是实际路由。
- `input.rs`：迁移密码使用精确粘贴，不沿用普通输入框换行替换。
- `ui.rs` / `account_cards.rs`：现有账号菜单接入和 overlay 生命周期。

## 一致性与激活

每个包内代理 ref 生成一个新 UUID/vault revision；共享 ref 保留，不按地址或名称合并已有节点。新账号、新 profile、新 vault 身份。禁止导出 bridge、设备 vault blob/引用、profile 路径、revision。

prepare graph → vault → route/profile about:blank → stage Cookie/readback → final candidate metadata commit → activate。候选不得进入 workspace 前台/额度/studio 调度。提交时基于最新 workspace 合并仅新对象，禁止旧快照覆盖并发变化；提交期间通过明确的短提交屏障避免持久化竞态。任何错误/取消/过时结果清除候选资源，不改包外账号；指定代理失败不直连。提交后网页身份仍须另行确认，Cookie 存在不代表身份成功。

Windows strict 复用 force_proxy=true；platform 复用 false。Mac 当前只可声明 platform，不能称 failover=false 等于 strict。跨策略转换、系统网络、localhost/私网设备地址和 header/未知 Cookie 属性均在预览说明，并经确认。无法可靠恢复的属性/路由拒绝，不静默降级。再次导出写当前执行策略，来源值只作 provenance。

## 验收与权限

合成向量、Unicode/特殊字符、错误密码、篡改/格式/成本/重复字段/深度上限、引用/共享/新 ID、写入失败/取消/隔离、密文文件原子写入均有测试。Windows 原生 smoke 只用临时 profile 和本地 fixture；禁止真实 Muse/代理请求和真实用户凭据。Mac 未运行的构建/原生行为如实标为未验证。

仅本地分支和必要本地提交；不推送、PR、合并、发布、变更版本或签名。保留原任务提交。无额外 PC worker。
