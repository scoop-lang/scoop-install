# scoop-install

Scoop 工具链的安装与版本管理程序，使用独立 Git 仓库维护。

当前阶段已经完成设计文档，尚未实现可执行程序。

- [安装程序设计](docs/DESIGN.md)：命令、版本与 target 选择、组件、安装布局、更新、项目与编辑器集成。
- [Scoop 发行包与下载目录设计](../scoop/docs/distribution/PACKAGING-DESIGN.md)：sources／vendor 拆包及打包脚本的外部依赖下载复用。

安装程序的名称固定为 `scoop-install`，Windows 为 `scoop-install.exe`。它独立消费 Scoop 的发行索引和组件归档，Scoop 编译器仓库负责构建与发布这些输入。

本机源码仓库位于 `/Volumes/Data/home/chenxu/repos/scoop-install-src`。同级的 `scoop-install` 已是 `0.0.0-dev.20261110` 工具链安装目录，因此保留该目录，源码仓库另用 `-src` 后缀。这里的跨仓库文档链接以当前相邻检出布局为基准。
