# scoop-install 安装与版本管理设计

日期：2026-11-11。状态：安装、组件操作、更新、代理、项目版本选择、缓存和编辑器接入已实现；平台验证范围与发行产物见 [Scoop 实施记录](https://github.com/scoop-lang/scoop/blob/codex/scoop-install/docs/distribution/IMPLEMENTATION.md)。

## 1. 已确定的范围

安装程序命名为 **`scoop-install`**，Windows 可执行文件名为 `scoop-install.exe`。源码在独立仓库维护，不放进 Scoop 编译器的 Cargo workspace 或 `tooling/`。

公开仓库为 `scoop-lang/scoop-install`。本机检出目录使用 `scoop-install-src`，保留相邻的既有工具链目录。程序和 Cargo package 名称均为 `scoop-install`。

程序负责安装、查询、增删和选择 Scoop 工具链版本及 target，支持按目标追加 GUI 组件。Scoop 仓库负责编译器、系统库、上游依赖的维护构建、组件组装和发布索引；官网仓库提供 shell／PowerShell bootstrap。

源码分为两种全局组件：`sources` 包含 Scoop 自己的 Cone 及胶水，默认安装以支持 LSP 源码导航；`vendor` 包含上游源码及构建补丁，按需安装。包内容和生产侧下载目录的详细合同见 [Scoop 打包设计](https://github.com/scoop-lang/scoop/blob/codex/scoop-install/docs/distribution/PACKAGING-DESIGN.md)。

## 2. 版本、宿主、target 与组件

一套 toolchain 由发行版本和宿主确定。target 指生成程序所用的完整目标身份，component 指实际下载和安装的资源包。

以 2026-11-10 的发行作为参考，当前组合如下；未来可安装列表以具体 release 的 manifest 为准。

| 宿主 bin | 默认 target | 同宿主其他发行 target |
| --- | --- | --- |
| `aarch64-apple-darwin` | `aarch64-apple-darwin` | `x86_64-apple-darwin` |
| `x86_64-unknown-linux-gnu` | `x86_64-unknown-linux-gnu` | `x86_64-unknown-linux-musl` |
| `x86_64-pc-windows-gnullvm` | `x86_64-pc-windows-msvc-ucrt` | `x86_64-w64-windows-llvm-mingw-ucrt`、`x86_64-w64-windows-llvm-mingw-msvcrt` |

Windows 三个 target 共用一个宿主 bin，宿主 triple 不替代默认 target。安装 target 后仍用 `scoop build --target <target>` 选择编译目标；安装操作不改变编译器的默认目标。

| 组件 | 选择规则 |
| --- | --- |
| `bin-<host>` | 必装，包含 Scoop 命令及其 LLVM／LLDB／Python 运行资源 |
| `lib-base-<target>` | 每个所选 target 必装，包含预编译系统库、native 链接库及 runtime |
| `lib-gui-<target>` | 按需，依赖同版本、同目标的 base；当前 musl 没有此组件 |
| `native-<target>` | 选择 MinGW target 时自动安装对应 SDK 数据 |
| `sources` | 默认安装，保留内置 Cone 源码、清单、胶水、头文件和资源 |
| `vendor` | 显式选择，上游源码和构建补丁 |

不指定 target 时仅装宿主默认 target；显式 target 列表取代默认列表。默认组合为 bin、所选目标 base、sources，以及目标必需的 native；GUI 与 vendor 分别按需追加。

首版保留 sources，因为它还提供应用 native 编译所用的公开 FFI 头。后续需要不装源码的 minimal 组合时，先由生产侧把必需公开头放入必装组件，再允许省略 sources。LSP 的依赖语义来自 `.slib`，sources 提供源码导航，不要求 vendor 或重建上游库。

## 3. 程序和安装布局

使用独立 Rust 程序实现选择、HTTP 下载、归档解包、安装状态及命令代理。安装程序自身不依赖 Scoop、LLVM 或 LLDB 的 Python。优先使用成熟库实现 HTTP、TOML、tar／XZ、文件锁和临时目录，依赖选型在实现前检查维护状态。

XZ 库静态链接；Windows 的 MSVC C runtime 也静态链接，bootstrap 可直接启动安装器，不要求用户事先安装 Visual C++ Redistributable。

官网 `install.sh` 和 `install.ps1` 只检测宿主、下载相应的 `scoop-install` 并传递参数。安装管理逻辑由同一份 Rust 代码处理。

```text
~/.scoop/
  bin/
    scoop-install
    scoop                 # Windows: sco.exe
    scoopc
    scoop-link
    scoop-lsp
    scoop-pm
    ...
  settings.toml
  toolchains/
    <version>-<host>/
      bin/
      commands/
      lib/<target>/
      include/
      src/
      native/
      vendor/
      libexec/
      share/
      VERSION
      ...
  packages/
```

未选的资源目录可以不存在。顶层 bin 中除安装程序外是工具代理；toolchain 内保留发行归档的现有布局。每套工具链的安装状态记录宿主、精确版本、已装组件和正常卸载所需的文件归属，与编译器使用的发行说明文件分开。

安装根优先采用 `--home`／`SCOOP_HOME`；已安装在 `<home>/bin` 的管理器和代理按相邻的 `settings.toml` 定位该 home，其余入口默认使用用户目录下的 `.scoop`。因此自定义安装目录加入 PATH 后可以直接使用，无需在每个终端再次设置环境变量。

代理启动所选版本中的实际命令。当前 Scoop 会根据真实执行文件定位相邻 sysroot 和配套工具，这条路径可以沿用。命令保留参数、cwd、标准输入输出、退出码与信号行为，并将同一次工具链选择传给子进程。用户显式传入的 `--sysroot`／`SCOOP_SYSROOT` 继续按已有规则覆盖资源位置。

Windows 使用原生 `.exe` 代理，主驱动保持 `sco.exe`。LLVM／LLDB 的公开工具也必须随同一选择切换。

## 4. 命令与项目选择

安装与组件命令：

```sh
scoop-install toolchain install 0.0.0-dev.20261111
scoop-install toolchain install 0.0.0-dev.20261111 --target x86_64-apple-darwin
scoop-install default 0.0.0-dev.20261111
scoop-install run 0.0.0-dev.20261111 scoop --version
scoop-install toolchain list
scoop-install toolchain remove 0.0.0-dev.20261111
scoop-install target add x86_64-apple-darwin --toolchain 0.0.0-dev.20261111
scoop-install target remove x86_64-apple-darwin --toolchain 0.0.0-dev.20261111
scoop-install target list --installed
scoop-install component add gui --target aarch64-apple-darwin
scoop-install component add vendor
scoop-install component list --installed
scoop-install show
```

目标相关示例适用于 macOS 宿主。`gui` 的增删明确绑定 target；全局 sources／vendor 属于所选工具链。删除当前默认版本或删除其他已装组件必需的组件时，给出明确错误及需要先完成的选择操作，不留下无法解析的默认配置。

default／run 和项目级版本选择采用以下优先级：

1. 命令行 `+<version>`，例如 `scoop +0.0.0-dev.20261111 build`。
2. `SCOOP_TOOLCHAIN` 环境变量。
3. 向上查找最近的 `scoop-toolchain.toml`。
4. 全局 default。

项目配置固定精确版本。缺少所需工具链或 target 时，代理给出对应安装命令，下载由显式安装操作完成。仅有实际发布渠道时才增加相应渠道名；当前提供精确版本和 latest 查找入口即可。latest 解析到精确版本，项目固定版本不随之改变。

## 5. 发行索引合同

使用结构化 TOML：每个固定 release 发布 `manifest.toml`，可变的 `latest.toml` 指向一个完整 release。生产侧由本次成功组装的组件生成索引，安装器不从文件名或 bucket 目录猜测 target 与组件依赖。

| 内容 | 必要字段 |
| --- | --- |
| 发行身份 | schema、release、version、source revision |
| 宿主选择 | host、默认 target、可安装 targets、bin 包引用 |
| 目标资源 | base 包引用、存在时的 GUI／native 包引用 |
| 全局资源 | sources／vendor 包引用 |
| 每个归档 | URL 或相对路径、压缩字节数、SHA-256 |
| 平台要求 | 发行已有的 OS／libc／SDK 基线 |

manifest 只表达当前组件的明确关系；同一安装使用同一精确 release，不求解组件之间的任意版本范围。安装器版本独立于 Scoop 版本，bootstrap 下载其可运行的宿主程序。

发布顺序为组件齐全、固定 manifest、最后 latest。固定 release 对应稳定的一组归档；重打包使用新的 release/build 标识。归档摘要在下载边界检查一次，正常命令启动不重读归档或扫描已安装文件验证内容。

## 6. 安装、追加、更新和文档

安装前形成完整计划，显示目标、组件、下载量及发行声明的外部要求。已经安装的组件直接复用。追加 target 或 GUI 时读取所选工具链固定版本的 manifest，不从 latest 取包。

新版本下载到临时位置，完整解包成功后发布到对应版本目录，再更新默认选择。失败不会先删除当前可用版本。更新延续用户已选组件和 targets；新版本不提供所选组合时，明确报告并保持旧选择。

组件修改使用每套工具链的普通文件锁。解包先准备新增内容，再提交文件及安装状态；目录可以合并，归档中文件归属必须明确。卸载只处理所属文件和相应生成文档。Windows bin 中的 DLL 硬链接、Unix 正常软链接及 executable 权限需要正确保留。

post-install 只生成离线 API 文档，接收实际已安装目标列表；只装非默认目标时必须显式传 target。提供跳过文档的安装选项，文档失败可以单独重试。未完成必需步骤时不切换默认版本。

首次采用版本目录时，保留顶层 `packages/`、`commands/` 和其他用户文件；只将本次 manifest 的组件登记为新版本，公开命令切为代理，不沿用旧脚本删除顶层发行目录的方式。共享自定义命令继续由 Scoop 的外部命令查找消费。安装程序自身的 Windows 替换与自更新另行实现，不作为首版组件选择的前置。

## 7. 编译与编辑器集成

代理将所选工具链的发行、版本、源码 revision 和宿主组合为 `SCOOP_TOOLCHAIN_ID`，传给同一次调用的子进程。Scoop 使用该值的摘要划分缓存目录；直接调用工具链内的命令时使用内嵌发行身份。默认缓存、`SCOOP_CACHE_DIR` 和显式 `--cache-dir` 都保留各版本的独立结果，切回原版本可直接复用，无需先 clean。身份来自发行／调用配置，更新判断不扫描或哈希工具文件。

VS Code 从 `SCOOP_HOME/bin` 或默认安装目录查找代理，并以各工作区目录为 cwd 启动配套 LSP、编译器与 debugger。每个工作区有独立语言服务；祖先目录的 `scoop-toolchain.toml` 或安装目录的 `settings.toml` 改变后重启相应服务，多根工作区可以分别选择版本。外部命令先查所选 toolchain 的 `commands/`，再查安装 home 的共享 `commands/`。

安装 target 只提供该发行的资源。Darwin SDK、MSVC Build Tools／Windows SDK、Linux libc／开发环境、musl 工具和 GUI 系统运行库仍遵守 Scoop 的现有平台合同。首版采用已发行宿主／target 矩阵，不附带扩展跨 OS SDK 分发。

## 8. 分工、实施和验收

| 仓库 | 所属工作 |
| --- | --- |
| `scoop-install` 独立仓库 | 安装器、版本状态、组件操作、命令代理及自身发行 |
| Scoop 编译器仓库 | sources／vendor 拆分、外部依赖下载目录、组件与 manifest 生产、缓存配置和编辑器接入 |
| 官网仓库 | bootstrap、版本入口与使用文档 |

实施顺序：

1. Scoop 侧先修订发行相关规范，完成源码拆包和下载目录；产出明确的组件 manifest。
2. 本仓库实现独立安装程序，完成版本目录、default／run、target／GUI／vendor 增删与查询，并验证三宿主实际安装。
3. 两仓库接通项目固定版本、缓存隔离和编辑器工具选择，完成所选版本的编译、语言服务与调试闭环。可省略 sources 的 minimal、安装器自更新和本地工具链 link 不属于本次实现范围。

验收覆盖所选归档请求、MinGW native 自动补齐、sources 的 LSP 导航而不请求 vendor、按版本追加目标、更新保留选择、失败不激活半成品、卸载文件归属、三宿主路径和链接语义、代理的真实版本与退出行为。项目选择阶段增加编译缓存和 LSP 版本一致性覆盖。

测试保持原源码、工具和产物存在，以真实调用与输出确认复用；不靠删除或隐藏这些输入证明隔离。原始日志与安装产物不提交到 Git；本轮本机临时文件统一放在仓库 `tmp/`，文档只保留结果摘要。

## 9. 依据

- [Scoop 发行调查](https://github.com/scoop-lang/scoop/blob/codex/scoop-install/docs/distribution/INSTALLER-RESEARCH.md)：2026-11-10 的真实包结构和体积。
- [Scoop 打包设计](https://github.com/scoop-lang/scoop/blob/codex/scoop-install/docs/distribution/PACKAGING-DESIGN.md)：源码组件与 `--download-dir`。
- [Scoop 实现规范](https://github.com/scoop-lang/scoop/blob/codex/scoop-install/docs/specs/SCOOP-IMPL-SPEC.md)：发行、工具路径、缓存和 LSP 合同。
- rustup 官方文档：[Toolchains](https://rust-lang.github.io/rustup/concepts/toolchains.html)、[Components](https://rust-lang.github.io/rustup/concepts/components.html)、[Overrides](https://rust-lang.github.io/rustup/overrides.html)。
