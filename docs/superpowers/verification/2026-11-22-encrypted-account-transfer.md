# PC 加密账号迁移验证记录

日期：2026-11-22 至 2026-11-23。工作区：`D:/AI/muse-desktop`。分支：`feat/encrypted-account-transfer`。

## 源码与本地提交

- 原 Cookie 导入基线：`1dd9c724aac84d1021fb9e0fccd94ff4846a1b18`，保留在 `feat/cookie-login`。
- `f2638b9`：严格加密 codec、合成向量及实施计划。
- `ef89f60`：候选账号/代理图、vault 分阶段写入、冻结规范原字节同步。
- `c606c54`：原生 Cookie 快照、分阶段激活、异步导入导出 UI 与合成检查脚本。
- `8b1dd4f1f054b74414cfc17d251d257f95a04602`：导航 ID 防旧回调、导入名称保留、本机地址提示与回归修复。
- **当前 ready 受测源码提交：`7beacc783f5435cfb31a9510d737ff44493b9dcf`**：CI 显示标识与现有 Mac smoke 的新适配器检查接入。下列受影响检查已在该源码上重新执行；最后的文档提交不再改变产品或脚本。

相对基线未更改 workspace 版本、签名或发布流程。未推送、PR、合并或发布。工作区及本地分支保留。

### 后续授权与构建追踪补充

本地实现验证结束后，用户追加了“双端完成后推送并编译”的授权。当前先向协调者提交 ready，只有其收到双端证据并明确放行后才普通推送功能分支。尚未推送或 dispatch；不合并 main、不 force、不建 tag、不做正式 Release。

`.github/workflows/build.yml` 仅新增可选 string 输入 `build_request_id`（空默认值）及显示用 `run-name`。输入不进入 shell、env 或任何 job；现有触发、权限、并发、构建、签名和发布条件均保持原值。协调者后续针对精确远端 HEAD 统一 dispatch 一次，实际 run/下载入口待该事件产生后记录，不猜测“最新 run”。

追加验证：`py -3 .superpowers/check-workflow-trace.py`，四项 YAML/AST 合约检查 exit 0（新增前两项缺失断言 RED，新字段加入后全 GREEN），使用本机已有 PyYAML 6.0.3；完整比较除两项授权显示元数据外的 workflow 树与 `56a3a6d` 完全一致。原始日志见 `.superpowers/sdd/2026-11-22-encrypted-account-transfer/workflow-trace-{red,green}.log`。

为让两种 Mac CI 实际触及新的原生适配器，现有 `check-macos-smoke.py` 在直连/认证代理路径加入 `--transfer-smoke-test`，并要求 7 项迁移报告标志全部为 true；独立诊断探针在 Release 中可用，默认运行不会开启，完整加密 UI 驱动仍仅在 debug 中编译。新增两项 Python 行为合约 RED→GREEN 后，现有 4 项打包合约加这 2 项共 6 项全部通过。workflow 的任何 job、触发器、权限、签名/发布条件均未改变。随后又在 `7beacc7` 上重跑下列 Rust/Windows 原生与脚本检查，不能把它称为已经在 Mac 运行成功。

## 协议与真实实现互解

唯一规范：`docs/protocols/account-transfer-v1.md`，从 Android owner 原字节复制。

- 冻结规范 SHA256：`b1aa1e6f476233ebdb31551315ef47ce85537eb80f22c22c6d52e556e86e0596`。
- Rust 随机 Unicode 回包 SHA256：`55a9307d8df12f5d3100308f6e322ce60fe975dae6fba0db4e6e5b810e74be34`。
- Java 随机 Unicode 包 SHA256：`2995893253a793eabcc17b813fe35139ff7c7c3a446010b41d949991811b699d`。
- Java 随机 ASCII 包 SHA256：`0b3b7a25dd482ccde7f57a5e0f2c8547e644d659d3ab90773d5b0931c6242e59`。
- 精确合成明文 SHA256：`20eb681e8cf8ed5a9252700550f0ce605e76ccc61a9e4ab26d2e9e83db9604b0`。

Windows Rust/ring 0.17.14 实际消费 Java 固定及随机文件，解密明文与 `payload.json` 逐字节一致；固定 salt/nonce 的测试重现也逐字节一致。Android owner 已以 JBR21/JCA 消费 Rust 新随机包并 `Arrays.equals` 比较，通过 35 项 JCA 与 37 项 schema 检查，exit 0。对端原始记录见 `checks/fixtures/account-transfer-v1/interoperability.json`。这些是 host codec 证据，不是 Android 设备 provider 或真实登录证据。

冻结后未更改 KDF/AEAD 参数、AAD、编码、tag 布局或 wire 字段。逻辑校验覆盖未知/重复/缺失字段、引用完整性、Cookie 作用域、安全标记与会话期限；缺失域且明确要求 domain-cookie 的歧义组合、空白名称按对端规则拒绝。

## 命令与结果

本机 Cargo 缓存使用 `C:/Users/lizhen/.cargo`。Release shader 检查使用已安装 SDK 的 `GPUI_FXC_PATH=C:/Program Files (x86)/Windows Kits/10/bin/10.0.22621.0/x64/fxc.exe`，仅设置当前进程环境；未安装系统依赖或更改系统环境。

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo test --workspace --locked` | exit 0；144 tests：core 单元 3、contracts 16、display 5、desktop 106、proxy 单元 1、bridge 13；无失败/忽略 |
| `cargo build -p muse-desktop --locked` | exit 0；Windows debug 实际构建 |
| `cargo check --release -p muse-desktop --locked` | exit 0；Windows release 模式检查，不能称 release 打包或发布 |
| `node --test scripts/check-observer.cjs tests/diagnostics-marker.test.cjs` | exit 0；21 tests |
| `node scripts/check-studio.cjs` | exit 0 |
| `node scripts/check-original-media.cjs` | exit 0 |
| `node scripts/check-reference-upload.cjs` | exit 0 |
| `py -3 -m unittest discover -s tests -p test_macos_package.py` | exit 0；4 packaging + 2 native smoke wiring/report contract tests，非 Mac 构建/运行 |
| `./scripts/check-package.ps1` | exit 0；Windows packaging contract，非正式安装包 |
| `py -3 -m py_compile scripts/check-account-transfer-native.py` | exit 0 |

原 Cookie 的 muse2api header/labelled-batch 兼容测试随 workspace suite 执行，未改变旧格式冒充加密包。构建仍有 unused/dead_code 警告，不能称无警告构建。首次 release 检查未找到 SDK 的 fxc；定位现有 22621 SDK 并设置上述进程变量后已通过。

## Windows 原生合成验证

最终 debug binary 对应上述受测产品源码；仅使用临时 profile、合成 Cookie、测试生成认证和本地 fixture。

| 路径 | 命令/证据 | 结果 |
| --- | --- | --- |
| 新迁移 UI，日间直连 | `py -3 scripts/check-account-transfer-native.py`；`.runtime/encrypted-transfer-native-8o8ui97b/smoke-result.json` | exit 0 |
| 新迁移 UI，夜间认证假代理 | `py -3 scripts/check-account-transfer-native.py --proxy --dark`；`.runtime/encrypted-transfer-native-0bn9wkw5/smoke-result.json` | exit 0 |
| 原 Cookie 登录直连回归 | `py -3 .superpowers/run-cookie-native.py`；`.runtime/cookie-native-vglrc5y9/smoke-result.json` | exit 0 |
| 原 Cookie 登录认证假代理回归 | `py -3 .superpowers/run-cookie-native.py proxy`；`.runtime/cookie-native-izvhmcmd/smoke-result.json` | exit 0 |

后两项使用本机会话原有的 ignored Windows owned-window harness，调用 debug 程序的 `--cookie-import-smoke-test --transfer-smoke-test` 及对应的 `--smoke-test` / `--proxy-smoke-test`，每次显式指定临时 `--data-dir`。新迁移公共 runner 已提交到 `scripts/`，不依赖真实用户 profile。

新 UI 检查实际经过后台解密、预览、vault staging、原生 profile/Cookie 写入与读回、后台 metadata commit、激活及加密再导出，验证：

- 密码精确确认、错误密码不建账号、取消清空输入，取消中的 vault 任务不激活候选。
- 预览前无 profile，未勾选确认不得 staging；预览期间已有账号被改名，最终保存保留该最新修改。
- 新 ID/profile 隔离，同包共享静态代理只映射一次；native Cookie scope/HttpOnly/SameSite/expiry 读回。
- 所有候选仍存在后才提交，元数据成功后才激活；认证假代理的各账号 bridge 收到实际本地诊断请求。
- 账号元数据不含 Cookie；旧 header 登录首次诊断导航携带测试 Cookie，原账号恢复及像素合成检查通过。
- 已人工查看最终日间、夜间 `transfer-preview.png`：内容、路由/属性说明、确认与取消按钮在窗口内，文字及对比度可读。

系统文件选择结果由合成驱动注入，未验证操作系统选择对话框的人工交互。`real_service_identity_verified=false` 为预期结果，不把有 Cookie 或本地页面就绪说成真实 Muse 身份成功。

## 失败调查与修复证据

- 固定密码学向量的 2030 有效期在原生写入后读回不一致，导入按设计拒绝。原生成功 fixture 另用短有效期，未修改规范向量或在产品中静默改期限。早期失败报告仍在 `.runtime/cookie-native-5bskqlpo/`。
- 多账号候选不在 workspace 中，旧 `retain_registered` 会提前删除其 view/bridge。现在保留 `cookie_only` 候选，显式取消/失败才 discard，并在 metadata commit 前再次检查整批 prepared view 完整性。
- 新账号都加入待确认观察链；Windows native 旧导航失败按 NavigationId 过滤。单元测试覆盖新 ID 已登记后的旧失败晚到；另一个准确边界用例明确“新 ID 登记前的交错”仍依赖 STA handler 顺序，不声称纯状态模型已证明所有原生时序。
- 后续修复均执行了对应 RED→GREEN：长名称编辑不被旧 40/48 字符默认规则截断、localhost 尾点/LAN/mapped-loopback 提示且不改写地址、歧义域和空白名称校验。最终上述完整 suite 和两组迁移、两组旧 Cookie 原生路径均通过。

原协调者完成限定只读静态复核，未确认阻断缺陷；这不计作其独立重跑测试，也不代替 Mac 原生验收。

## 明确未验证与能力边界

- 本轮没有在 Mac 构建或运行。`browser_transfer_macos.rs` 的 Foundation 字典、SameSite、session、host-only 读回和代理旁路转换仍需 Mac 原生合成验收；不能从 Windows 或包装脚本测试推导通过。
- Mac 当前只能声明 platform 策略。来源 strict 需要预览确认转换，再次导出为实际策略；来源 bool 仅作 provenance，不是当前执行策略。
- 原生 API 完成/读回不等于独立的跨进程磁盘 fsync 证明。session Cookie 不增加期限；取消/失败仅请求清理本批候选，不宣称所有平台磁盘 profile/Cookie 已彻底擦除。
- native host_only 缺失保留 unknown；快照记录返回的 Muse URL 作用域，不保证完整 jar、分区键、localStorage 或 IndexedDB。
- 仅按用户确定的静态代理假设迁移配置与不透明认证；不做轮换、动态识别或出口 IP 获取/比较/保证。
- 没有读取真实账号数据库、用户浏览器 profile、真实 Cookie/代理凭据；没有真实 Muse/代理身份验收；没有远端操作。

## 2026-11-23 首轮远端 CI 与诊断补充

以上“没有 Mac 构建/远端操作”为初次本地 ready 的历史边界。获双端放行后，已普通推送 `009e2cec0c023c5543864ba7334df6223dad5c04`；协调者唯一触发 Desktop clients run `45044303264`。该 run 的 Windows job 成功，Mac arm64 job `137275413602` 与 x64 job `137275413427` 都已完成 release 编译与打包，但原生 smoke 失败，不能称 Mac 验收完成。

两份精确 job 日志均只有 `cookie_import_error="Cookie import did not create exactly one new account"`，没有原 Cookie 表单保存的错误上下文。源码追踪到旧入口将 native stage/parse/save 错误存于表单 `message`，而 smoke 随后的账号计数断言丢弃该信息。因此先补仅合成 smoke 的诊断：保留全部失败断言，输出既有表单错误、退出阶段、save_enabled 和数量/切换布尔值，不输出 Cookie、账号 ID、密码或路由认证。Mac runner 也在旧入口断言失败前打印独立 transfer probe 报告，避免临时目录清理后丢失先行检查证据。此提交用于下一轮 Mac 定位，尚未确定或修复 Mac 导入根因；未猜改 Foundation/SameSite，也未修改冻结 v1。

此诊断源码在 Windows 的新鲜验证：

- `py -3 -m unittest discover -s tests -p test_macos_package.py`：新增“后续旧入口失败仍保留先行 transfer 报告”的回归先 RED，修改后 7 项 GREEN，exit 0。
- `cargo test --workspace --locked`：144 项通过，exit 0；`cargo build -p muse-desktop --locked`、`cargo fmt --all -- --check` 与 `git diff --check` 均 exit 0，保留既有 unused/dead_code 警告。
- `py -3 .superpowers/run-cookie-native.py`：exit 0，合成证据 `.runtime/cookie-native-e5hyo6hp/`。
- `py -3 .superpowers/run-cookie-native.py proxy`：exit 0，合成证据 `.runtime/cookie-native-mya8qf4n/`。

原始日志为 `.superpowers/sdd/2026-11-22-encrypted-account-transfer/ci-cookie-diagnostics-{red,green,workspace,build,native-direct,native-proxy}.log`，另保存 `ci-first-macos-x64.log`。Windows 合成通过不代替 Mac 运行；后续仍由协调者唯一 dispatch，不执行 rerun/dispatch 或真实账号请求。
