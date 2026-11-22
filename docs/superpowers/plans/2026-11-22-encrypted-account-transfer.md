# Encrypted Account Transfer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** PC 通过密码加密文件与 Android 双向迁移隔离账号 Cookie 和绑定静态代理。

**Architecture:** 在原 Cookie stage/activate API 上增加独立协议层、候选图和原生 UI。加密文件先认证和校验，再准备新 vault/路由/profile；仅最终提交后调度账号。Android owner 与 Rust 双向互解后共同冻结 v1。

**Tech Stack:** Rust, serde, ring 0.17.14（已有 Cargo.lock）, zeroize, GPUI, Wry/WebView2/WKWebView.

**Spec:** `docs/superpowers/specs/2026-11-22-encrypted-account-transfer-design.md`

## Global Constraints

- Wire 以 Android owner 的 `docs/protocols/account-transfer-v1.md` 为唯一 authority；互解前不宣称冻结。
- 保留旧 header 入口；不导出 bridge/vault blob/profile/revision，不读取真实数据。
- 所有代理统一按静态配置；认证字段作为不透明文本原样迁移；指定代理失败不得 Direct。
- 本地分支从 1dd9c72 增量，无远端操作，无版本/签名变动。
- KDF/文件后台执行，敏感数据无日志、无明文临时文件。

## Review Focus

- 合法 JSON 的重排/额外/重复 envelope 字段须在 KDF 前拒绝（Task 1）。
- Unicode 密码、控制字符与密码粘贴不得隐式变换（Task 1/4）。
- 同地址不同 ref 不合并，共享 ref 只建一个本地节点（Task 2）。
- 导入期间旧账号变化不能被快照回写覆盖，取消/失败不得激活（Task 2/4）。
- Mac strict 无法原样执行，预览须确认转换并按实际策略再次导出（Task 2/3/4）。

---

### Task 1: 严格协议与 Rust/Java 互解

**Files:** Create `crates/muse-desktop/src/account_transfer.rs`; Modify `main.rs`, `Cargo.toml`; Copy owner protocol and synthetic fixtures into `docs/protocols/`, `checks/fixtures/account-transfer-v1/`.

**Interfaces:** Produces `Payload`, `Account`, `Network`, `Proxy`, `CookieSet`, `Cookie`, `TransferError`; `decrypt(&[u8], &str) -> Result<Payload, TransferError>`, `encrypt(&Payload, &str) -> Result<Vec<u8>, TransferError>`, `validate(&Payload, i64)`, bounded `read_file` / atomic ciphertext `write_file`.

- [x] Write tests consuming fixed ASCII/Unicode and Java random files; assert exact payload and deterministic test-only reproduction. Add format, password/auth, duplicates/depth, tamper, randomness and schema failures.
- [x] Run `cargo test -p muse-desktop account_transfer`; verify RED because implementation missing.
- [x] Implement with ring PBKDF2/AES-GCM and system random; strict canonical envelope, no arbitrary KDF costs; typed deny-unknown payload and duplicate/depth validation. No production deterministic randomness API.
- [x] Run same tests GREEN; generate new Rust Unicode random fixture, send hash/path to owner for Java consumption, agree freeze before product integration.
- [x] Local commit protocol/code/fixtures; only explicit docs paths force-added.

### Task 2: 候选图、vault 和提交原子性

**Files:** Create `transfer_plan.rs`; Modify `muse-core/src/lib.rs` only for migration provenance.

**Interfaces:** Consumes Task 1 Payload; produces `ImportPlan::prepare`, new IDs/bindings, preview lines, `stage_vault`、RAII 清理、 `merge_into(&Workspace) -> Workspace`; snapshot export resolves actual upstream credentials from ProtectedStore.

- [x] Write failing tests for shared refs/new IDs/same-endpoint distinct refs, strict/platform conversion, legacy routes, failure cleanup and merge preserving latest unrelated changes.
- [x] Run targeted tests RED; implement candidate graph and provenance without publishing accounts or per-proxy metadata saves.
- [x] Run targeted tests GREEN, including synthetic DPAPI vault and failed workspace save; verify existing metadata unchanged.
- [x] Local commit candidate implementation.

### Task 3: Native Cookie snapshot 与多账号 staging

**Files:** Modify `browser.rs`; Create `browser_transfer.rs`, `browser_transfer_macos.rs`, `transfer_cookies.rs`; extend synthetic smoke as needed.

**Interfaces:** Consumes plan account identities and protocol CookieSet; native snapshot reports URL-scoped coverage, unknown host-only honestly. Staging uses original stage_cookie_import and explicit capability checks before commit; no Muse navigation during export/stage.

- [x] Add failing tests for attribute conversion/expiry/scope duplicates/unsupported metadata and URL-scoped export fidelity; native smoke starts with disposable fixture profiles only.
- [x] Run RED; implement native snapshots and conversion, readback/flush supported paths, precise rejection if unsupported. Retain old header output.
- [x] Run GREEN and native local direct/proxy smoke; do not call fixture identity a real login.
- [x] Local commit native integration.

### Task 4: 异步导入导出 UI

**Files:** Create `transfer_ui.rs`; Modify `input.rs`, `ui.rs`, `account_cards.rs`.

**Interfaces:** Consumes encrypt/decrypt, ImportPlan and native snapshots; produces select/password/file/preview/confirm/cancel flow with request generation and final activation barrier.

- [x] Add failing tests for exact password paste, invalid confirmation, cancellation/stale job behavior and menu/UI contract.
- [x] Run RED; implement export selection/password confirmation and file picker; import bounded background decode plus preview; confirm route/attribute mappings; stage/commit/activate candidate only once.
- [x] Run GREEN including failure/late-result/UI responsiveness assertions. Clear inputs and zero sensitive owned buffers when dialog exits.
- [x] Local commit UI feature.

### Task 5: 完整验证与本地交付

**Files:** Test scripts/README only where behavior requires; update this ledger and exact protocol freeze status.

**Interfaces:** Consumes all earlier tasks; produces tested local commits and interoperability report.

- [x] Run `cargo fmt --all -- --check`, `cargo test --workspace`, existing Node/Python packaging/contracts, native Windows transfer smoke.
- [x] Confirm Java→Rust and Rust→Java random-file hashes, all negative cases and exact-source evidence.
- [x] Review full diff for secret/logging, stale callbacks, activation order, scope and wire consistency; fix material issues with RED/GREEN.
- [x] Record actual Windows/Mac coverage, user acceptance boundary and local commit IDs. No remote action.
