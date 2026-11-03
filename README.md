# xr-dock

独立的 Rust 空间任务栏。它通过唯一 `xr-shell-protocols` SDK 与 Shell 通信，使用 `xr-spatial-kit` 场景和控件，不创建 OpenXR session、不执行应用的 Exec 命令，也不控制硬件。

## 功能与状态权威

- 从 XDG `.desktop` 清单读取实际应用，支持目录优先级、Hidden/NoDisplay、OnlyShowIn/NotShowIn、中文名称和图标标识。默认固定已发现的文件管理器、终端和浏览器，可用 `--pin` 选择。
- “应用”展开分页清单；固定应用和清单按钮发送 `WindowIntent launch`，由 Shell/窗口服务执行真实启动。Dock 仅发送 app ID，不把文件名或命令当 shell 字符串执行。
- 从 Shell Snapshot/连续 StateChanged 构建任务和四个工作区。仅 toplevel 列为任务，popup、subsurface 和输入法候选不作为新任务。工作区窗口数、焦点、关闭完成都以服务端状态为准。
- 任务按钮发送 focus，关闭按钮发送 close，工作区按钮发送 WorkspaceIntent；不会乐观删除窗口或伪造启动成功。任务和应用都支持分页。
- Kit 控件按同一 capture/interaction 的 press/release 激活，Cancel 清理状态。连续状态 revision 缺号时请求快照；scene revision 等 Shell ack 后再推进。
- SDK 断开时清除场景缓存、capture 和任务缓存，重连后重新握手和取快照。SIGINT/SIGTERM 停止连接，Shell 根据连接生命周期退役本组件场景。

显示由 Shell 的真实渲染/字形后端完成；本仓只发送几何、文本和样式标识。图标 ID 是主题资源名，绝对图标路径不作为场景资源发送。

## 构建

兄弟仓默认布局：

```text
xr-shell/protocols/  xr-shell-protocols 0.2.0
xr-spatial-kit/      xr-spatial-kit 0.1.0
xr-dock/
```

```sh
cargo build --release --locked --offline
# 独立源码包或已安装SDK：
XR_SHELL_PROTOCOLS_DIR=/work/sdk/xr-shell-protocols \
XR_SPATIAL_KIT_DIR=/work/sdk/xr-spatial-kit python3 tools/build.py
```

`tools/build.py` 在私有 build 目录生成 manifest，原仓库 manifest/lock 不变，不复制协议源码。未指定时使用兄弟仓，再查找 `/usr/share/cargo/registry/<package>-<version>`。Cargo.lock、SDK/Kit 来源提交与产物摘要应由组合 manifest 固定；path 依赖本身不能代替源码提交锁。构建前准备校验后的离线 Cargo 缓存。

`dpkg-buildpackage -us -uc -b` 构建独立包，Build-Depends 包括两个源码 SDK。只用源码路径构建的本地包与已在干净发行版验证的依赖闭包是不同状态。安装包不自动启动 Dock 或 XR。

## 运行

```sh
xr-dock --socket "$XDG_RUNTIME_DIR/xr-desktop/shell.sock"
xr-dock --seconds 60 --pin peony.desktop --pin qterminal.desktop
xr-dock --list-apps
```

默认 socket 为 `$XDG_RUNTIME_DIR/xr-desktop/shell.sock`。重复 `--apps-dir DIR` 可替换正常 XDG 清单目录；它读取真实桌面文件，实际 launch 仍由窗口服务按 app ID 解析。所需能力为 core/scene/snapshot/input/window/workspace v1，缺少能力时明确报告并等待可用会话。

stdout 记录真实握手、快照 revision、scene ack、输入、意图 request ID 和退出；服务错误在状态栏可见。真实启动/焦点/关闭/工作区及断线回收须在已选 Shell/窗口服务组合中验收，编译和应用清单读取不等于这些行为已通过。
