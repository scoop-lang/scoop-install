# scoop-install

Scoop 工具链、target 与可选组件的独立安装和版本管理程序。源码仓库为 [scoop-lang/scoop-install](https://github.com/scoop-lang/scoop-install)，不依赖 Scoop 编译器、LLVM 或 Python 运行。

```sh
scoop-install toolchain install latest
scoop-install toolchain install 0.0.0-dev.20261111 --target x86_64-apple-darwin
scoop-install target add aarch64-apple-darwin
scoop-install component add gui --target aarch64-apple-darwin
scoop-install component add vendor
scoop-install default 0.0.0-dev.20261111
scoop-install run 0.0.0-dev.20261111 scoop --version
scoop-install update
scoop-install show
```

不指定 target 时只装宿主默认目标；显式 `--target` 列表取代默认列表。默认包含宿主工具、所选目标 base、sources 和所需 native，GUI／vendor 按需安装。Windows 主驱动代理名为 `sco.exe`。将 `~/.scoop/bin`（Windows 的 `%USERPROFILE%\.scoop\bin`）加入 PATH 后，通过代理运行实际工具链。

版本选择优先级为命令行 `+<version>`、`SCOOP_TOOLCHAIN`、最近的 `scoop-toolchain.toml`、全局 default。缺少版本或项目要求的 target 时显示安装命令，不隐式下载。项目配置示例：

```toml
[toolchain]
version = "0.0.0-dev.20261111"
targets = ["aarch64-apple-darwin"]
```

`target list --installed`、`component list --installed` 查询所选版本；`--toolchain <version>` 显式选择。`target remove <target>` 一起删除该目标的 GUI 和 native；`component remove gui --target <target>` 只删除 GUI；`component remove vendor` 删除上游源码。删除当前默认 toolchain 前先选择另一版本。sources 目前还提供必需的 FFI 头，不能单独删除。

首次安装可用 `--skip-docs` 跳过 API 文档，后续用 `scoop-install docs --toolchain <version>` 重试。文档失败保留已装工具链并显示重试命令；下载、摘要检查或解包失败不会激活不完整工具链。更新一次准备所选 targets、GUI 和 vendor，成功后切换 default，保留旧版本供显式删除。

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
