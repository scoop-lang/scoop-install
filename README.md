# scoop-install

Scoop 工具链、target 与可选组件的独立安装和版本管理程序。源码仓库为 [scoop-lang/scoop-install](https://github.com/scoop-lang/scoop-install)，不依赖 Scoop 编译器、LLVM 或 Python 运行。

已发布的安装器 `0.1.0` 可从 R2 下载：

- [Apple Silicon macOS](https://release.scoop-lang.org/installers/0.1.0/scoop-install-aarch64-apple-darwin)
- [x86-64 GNU/Linux](https://release.scoop-lang.org/installers/0.1.0/scoop-install-x86_64-unknown-linux-gnu)
- [x64 Windows](https://release.scoop-lang.org/installers/0.1.0/scoop-install-x86_64-pc-windows-gnullvm.exe)

[SHA256SUMS](https://release.scoop-lang.org/installers/0.1.0/SHA256SUMS) 提供校验值。将文件命名为 `scoop-install`（Windows 为 `scoop-install.exe`），macOS／Linux 执行 `chmod +x scoop-install`。第一次可用 `./scoop-install toolchain install latest` 安装；之后将管理目录的 `bin` 加入 PATH。安装器版本与 Scoop 工具链版本独立管理。

以下目标选择示例适用于 macOS：

```sh
scoop-install toolchain install latest
scoop-install toolchain install 0.0.0-dev.20261111 --target x86_64-apple-darwin
scoop-install target add aarch64-apple-darwin
scoop-install component add gui --target aarch64-apple-darwin
scoop-install component add profiling --target aarch64-apple-darwin
scoop-install component add vendor
scoop-install default 0.0.0-dev.20261111
scoop-install run 0.0.0-dev.20261111 scoop --version
scoop-install update
scoop-install show
```

不指定 target 时只装宿主默认目标；显式 `--target` 列表取代默认列表。默认包含宿主工具、所选目标 base、sources 和所需 native，GUI／profiling／vendor 按需安装。Windows 主驱动代理名为 `sco.exe`。将 `~/.scoop/bin`（Windows 的 `%USERPROFILE%\.scoop\bin`）加入 PATH 后，通过代理运行实际工具链。

提供 profiling 的发行版本可在首次安装时使用 `--component profiling`，也可按上例单独追加；该 target 的 base/native 需要已安装。组件包含 Profiling/full 与 Release/full 两套系统库、训练映射及采集 runtime，GUI native 资源仍由 gui 提供。`component remove profiling --target <target>` 只移除专用采集目录，普通库保持可用；更新保留该选择，`target remove` 一并移除。安装器不会现场构建库。

版本选择优先级为命令行 `+<version>`、`SCOOP_TOOLCHAIN`、最近的 `scoop-toolchain.toml`、全局 default。缺少版本或项目要求的 target 时显示安装命令，不隐式下载。项目配置示例：

```toml
[toolchain]
version = "0.0.0-dev.20261111"
targets = ["aarch64-apple-darwin"]
```

`target list --installed`、`component list --installed` 查询所选版本；`--toolchain <version>` 显式选择。`target remove <target>` 一起删除该目标的 GUI、profiling 和 native；`component remove gui --target <target>` 只删除 GUI；`component remove vendor` 删除上游源码。删除当前默认 toolchain 前先选择另一版本。sources 目前还提供必需的 FFI 头，不能单独删除。

首次安装可用 `--skip-docs` 跳过 API 文档，后续用 `scoop-install docs --toolchain <version>` 重试。文档失败保留已装工具链并显示重试命令；下载、摘要检查或解包失败不会激活不完整工具链。更新一次准备所选 targets、GUI、profiling 和 vendor，成功后切换 default，保留旧版本供显式删除。

`--home`／`SCOOP_HOME` 选择管理目录；`--dist-server`／`SCOOP_DIST_SERVER` 选择镜像（默认 `https://release.scoop-lang.org`）。`toolchain install <version> --manifest <path-or-url>` 消费本地或 HTTP(S) 索引，便于发行验收。独立安装的版本存放在 `toolchains/<version>-<host>`；已有的 `packages/`、`commands/` 和其他用户文件保留。

构建需要 Rust 1.89+ 和 C 编译器，XZ 静态链接。CLI 测试需要 Python 3.11+；测试使用本地 HTTP 服务与真实子进程，不请求线上发行。

```sh
mkdir -p tmp
export TMPDIR="$PWD/tmp" TMP="$PWD/tmp" TEMP="$PWD/tmp"
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo build
python3 -m unittest discover -s tests
cargo build --release
```

依赖通知嵌入程序并随安装写入 `share/licenses/scoop-install/`；更新 Cargo.lock 后执行 `python3 tools/update_notices.py`。自身二进制发行使用本仓库的 workflow；Scoop 仓库负责工具链组件和 manifest，官网负责 bootstrap。

[完整设计](docs/DESIGN.md) · [Scoop 打包设计](https://github.com/scoop-lang/scoop/blob/codex/scoop-install/docs/distribution/PACKAGING-DESIGN.md) · [索引 schema 1](https://github.com/scoop-lang/scoop/blob/codex/scoop-install/docs/distribution/MANIFEST.md)
