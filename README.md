# xr-dock

独立 Rust 空间桌面客户端，负责三岛任务栏的左信息岛与中央应用岛。右状态岛由 xr-system-ui 负责，几何来自唯一 `xr-spatial-kit::taskbar` 合同；材质、字形、图标与命中渲染由 Shell 负责。本仓不创建 OpenXR session，不执行应用 Exec，不接管 Wayland 或系统服务。

外观以 Kit 中先行提交的 `design/qingkong-controls/control-library.png` 为准：晴空浅雾银、墨蓝文字、通透圆角胶囊。信息岛和应用岛分别有自己的背景，中间保留真实空间间隔。

## 行为与真实状态

- 左信息岛显示真实本地 HH:MM、日期和星期，使用系统时区；仅在分钟变化时触发时钟 scene 更新。没有天气提供者时不展示温度或天气图标。
- 中央启动器始终可用。默认固定应用按文件管理器、浏览器、文档编辑器、视频、终端、设置的顺序，从真实可见 `.desktop` 文件匹配，最多六项；缺失角色不占假图标。支持 Peony、Firefox、Pluma、QTerminal、UKUI Control Center 等真实 ID 与分类回退。
- 已匹配角色使用 Kit 的 `ukui-xr-files/browser/document/media/terminal/settings` 图稿语义资源；自选 pin 和其他运行应用继续使用实际主题图标。中央按钮无文字，图标尺寸由 `dock.app-icon` 样式限定；焦点使用 `dock.active-icon`，独立蓝点只表示真实运行状态。
- Shell Snapshot/连续 StateChanged 是窗口和工作区的唯一权威。仅 toplevel 聚合为应用图标，popup、subsurface 和输入法候选不成为任务。聚合匹配真实 desktop ID、StartupWMClass 或唯一名称后缀；不构造虚假的 launch ID。
- 固定应用始终可见，额外运行应用分页。单窗口点击直接聚焦；多实例按需展开窗口选择面板，每个真实窗口均有分页入口，任务栏不默认展示大窗口标题或逐应用关闭 X。窗口关闭由 Shell 的窗口 Orb 处理，任务图标随实际快照回收。
- 四个紧凑工作区点保留真实 WorkspaceIntent。选择其他工作区的窗口时，先发送工作区请求，待权威快照确认切换后再发送该 generation/surface 的 focus；不会乐观修改焦点或工作区。
- 应用库是按需展开的独立 Group，保留固定应用和全应用分页，发送真实 `WindowIntent launch`。收到 `ComponentCommand {role:"dock",panel:"launcher",intent:"show"|"hide"|"toggle"}` 也可打开/收起。启动和聚焦结果以实际窗口状态为准。
- Kit 控件按同一 capture 的 press/release 激活，Cancel 清理；revision 缺号请求快照，scene 等待 ack。收到 SessionEnding 正常退出；未预告的连接或协议错误清理后退出 1，由会话管理者决定是否重启。

## 构建

相邻源码布局：

```text
xr-shell/protocols/  xr-shell-protocols 0.2.0 Rust crate
xr-spatial-kit/      xr-spatial-kit 0.2.2
xr-dock/
```

```sh
cargo build --release --locked --offline
# 显式选择源码 SDK 或已安装的开发源码包：
XR_SHELL_PROTOCOLS_DIR=/work/sdk/xr-shell-protocols \
XR_SPATIAL_KIT_DIR=/work/sdk/xr-spatial-kit python3 tools/build.py
```

`tools/build.py` 在私有 build 目录生成 manifest，不修改原 manifest/lock、不复制协议源码。Cargo.lock、SDK/Kit 提交与产物摘要由集成锁固定；path 本身不替代提交锁。构建前准备经过校验的离线 registry 缓存。

`dpkg-buildpackage -us -uc -b` 生成独立包。使用显式源码 SDK 构建本地包、干净发行版依赖闭包、目标安装和原生交互是独立验收状态。安装不会启动桌面或服务。

## 运行

```sh
xr-dock --socket "$XDG_RUNTIME_DIR/xr-desktop/shell.sock"
xr-dock --seconds 60 --pin peony.desktop --pin qterminal.desktop
xr-dock --list-apps
xr-dock --list-pins
```

默认 socket 为 `$XDG_RUNTIME_DIR/xr-desktop/shell.sock`。重复 `--apps-dir DIR` 替换默认 XDG 清单目录，重复 `--pin` 显式选择真实应用及顺序；`--list-pins` 只打印匹配结果，不连接 XR。所需能力为 core/scene/snapshot/input/window/workspace/system v1，缺少时明确失败。

stdout 记录握手、快照/scene revision、输入、意图 request ID 与退出。真实启动、多实例与跨工作区焦点、分钟更新和三岛画面须在选定 Shell/窗口服务组合验证；本地构建和目录读取不等于原生渲染或操作验收通过。
