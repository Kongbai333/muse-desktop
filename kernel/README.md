# Muse Windows x64 代理内核

`mihomo.exe` 是 Muse 修改构建的 **Mihomo v1.19.34-muse.0.1.11**，不是官方二进制。SHA-256 为 `b3db115379b866eeb1db722be0f31bf6bfa1610dd54a019fc748ecf9a733ab9d`。仅用于固定账号代理及手动读取订阅，不提供 Windows 系统 TUN，也不查找、下载或安装 Wintun。

## 对应源码与构建

`mihomo-source.zip` 是本次 Windows x64 实际编译的 Go/汇编源码、嵌入资源、依赖清单及许可通告，可在无网络时复建。SHA-256 为 `7f9b86d236164ec2c9745d7e41782e584ce5e0771c153538bab1f84938feea80`。编译器为 [Go 1.27.4 Windows amd64](https://go.dev/dl/go1.27.4.windows-amd64.zip)，官方 ZIP SHA-256 `178ab98950ac69ab25069307dc6ade79f3816e414cce95b4da166a115cde1e7e`。Windows x64 上用 PowerShell 7 在本目录执行：

```powershell
./build.ps1 -OutputDirectory "$PWD/rebuilt"
```

该命令只在重建时下载并验证编译器；已有该版本时，可加 `-GoExecutable 'X:/path/to/go/bin/go.exe'` 完全离线构建。源码包不依赖 Git 检出路径或宿主代理配置。手工构建可在空目录完整解压源码，用上述 Go 版本，设置 `GOTOOLCHAIN=local`、`CGO_ENABLED=0`、`GOOS=windows`、`GOARCH=amd64`、`GOAMD64=v1`、`GOWORK=off` 并运行：

```text
go build -buildvcs=false -mod=vendor -tags with_gvisor -trimpath -ldflags "-s -w -buildid= -X github.com/metacubex/mihomo/constant.Version=v1.19.34-muse.0.1.11" -o mihomo.exe .
```

## 来源及变更

- 主工程来自 [MetaCubeX/mihomo v1.19.34](https://github.com/MetaCubeX/mihomo/tree/0455d0d28c4526937380505153e65a843d8612e8)，GPLv3，全文为 `mihomo-source.zip` 根目录 `LICENSE`。依赖基线是该版本[官方发布](https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.34)的 `vendor.tar.gz`（SHA-256 `a0a27ab5917d4744a652c68a90cc3ca196a0d1c7f1b66cd3fa3b2933ff6c3dc0`），该原始归档**不随本项目分发**。版内包含的是实际选用的源码，而非完整未裁剪 vendor。各模块著作权及许可保留在其对应目录，Go BSD 许可及专利授权见 `licenses/go/`。
- `changes.patch` 是相对上述源码和 vendor 的文本差异。Muse 增加受限的 `muse-provider` 原生订阅读取、快照绑定、证书与出站策略、错误状态及本版本请求标识。Windows 配置显式拒绝 `tun.enable`，TUN 入站和 `sing-tun` 创建设备亦明确返回不支持；移除 Windows 驱动实现、gVisor **设备**适配和整个 `vendor/github.com/metacubex/sing-tun/internal/wintun/`（删除的二进制不写入补丁）。保留 `with_gvisor` 的用户态协议栈和其余账号订阅协议。内嵌公共 CA 集在 `component/ca/ca-certificates.crt`，SHA-256 `185d96cba669be5e215d2b73c15e289930779717163f9d5c6ea3c0a55f2183cc`。它不是用户证书或私钥。
- Go 模块身份与版本固定在 `go.mod`、`go.sum`、`vendor/modules.txt`。ChaCha v0.1.5 的模块顶层 MIT 通告取自[下一提交](https://github.com/MetaCubeX/chacha/commit/7b981d42c3baca5a004e7b0ecfb9b67ee20e583a)，该提交只增加许可证。go-lzo 的上游说明为 [GPL-2.0-or-later](https://github.com/rasky/go-lzo/commit/ad21c35a111113348567708a4db74f998be1a4c3)。
- [EasyTier Go wrapper](https://github.com/EasyTier/EasyTier/tree/2abb1676b475947e46c527b57acc5f296cb48482/easytier-go) 使用 LGPLv3。其 WASM 已从[公开 Rust 源码](https://github.com/EasyTier/EasyTier/tree/b661721ad00182af7c53f9c0661a50abec762de1)独立重建，与随包资源逐字节相同：6,221,381 字节，SHA-256 `de332a01eb4141e2e86ecee89500c40cc7d07a822a381868f95bb231bfc3739c`。这里标的是实际复现的公开提交，不是声称找回了 Go wrapper 记载的旧提交。

### 重建 EasyTier WASM

在一次性 Linux x86_64 环境，将上述公开提交检出至 `/source`（树 `3cbb7dc6e4a45d60ee9722ecba93c6db5a72df8d`，未变的 `Cargo.lock` SHA-256 `006dc75ed326b71206f04fcf6ed33c44644c92c530beae511bb582381302b68e`）。使用官方 `rustc`/Cargo 1.95.0、主机及 `wasm32-wasip1` 标准库、`rust-src`，安装位置 `/home/admin/.rustup/toolchains/1.95-x86_64-unknown-linux-gnu`，使用该工具链的 `bin`。`rust-src-1.95.0.tar.xz` SHA-256 `67b09138c8db96afc4bbfc69ea771ac9a091fd777698acb43f6dfd9fb7dea363`。提供 `protoc 3.21.12` 及其头文件；上游构建脚本固定 Binaryen 131，Linux x64 包摘要 `b5bf1f0eaf17c63ee588ff7a5954dc8f6ce2c26989051c66f24dfe9ece3e46db`。这些路径是复现调试信息所需的隔离构建布局，不是 Muse 的运行依赖。

```sh
cd /source
export HOME=/home/admin CARGO_HOME=/home/admin/.cargo
export CARGO_TARGET_DIR=/data/project/EasyTier/target SOURCE_DATE_EPOCH=1793029352
export RUSTFLAGS='' CARGO_ENCODED_RUSTFLAGS='--remap-path-prefix=/source=/workspace/EasyTier'
cargo fetch --locked --target wasm32-wasip1 --target x86_64-unknown-linux-gnu
bash script/build-wasi-core.sh
sha256sum "$CARGO_TARGET_DIR/wasm32-wasip1/release/easytier_core_go_host.wasm"
```

使用者可修改、重建、替换源码中的 WASM 再编译内核；Muse 校验的是本次发布内核，使用自行修改的内核时需一并从 [Muse 源码](https://github.com/Kongbai333/muse-desktop)调整摘要并重建客户端。
