# AGENTS.md

> 面向在本仓库中工作的 AI 编程代理与人类贡献者。先读 [README.md](README.md) 了解用户可见行为，再读本文件了解工程约束。
>
> 本文件是工作指南，不是产品文档；其中“本机/代理环境提示”一节只描述当前开发环境，不属于产品行为。

## 1. 项目速览

ScreenShot FX 是一个 Windows 11 x64 常驻托盘截图工具：

- Rust 编写，单文件 `fxshot.exe`，`asInvoker`，无管理员权限、无网络、无遥测。
- `Shift+F1` 选区截图：冻结全屏帧 → 每块显示器一个遮罩窗口 → 悬停吸附窗口/拖拽选区 → 工具条保存/复制 → 取色、方向键微调。
- `Shift+F2` 快速截图：静默截取鼠标所在显示器，保存 PNG，可选复制到剪贴板。
- 默认 GDI 抓取；真 HDR 与“兼容模式”使用 Windows Graphics Capture（WGC）。
- TOML 配置、HKCU 开机自启、单实例、托盘菜单、日志文件。
- 明确不做：标注/涂鸦、OCR、录屏、滚动截图、设置 GUI、上传、自动更新、ARM64。

## 2. 技术栈与前提

| 项 | 说明 |
|---|---|
| 语言 | Rust 2021，MSRV 1.82；CI 固定 Rust 1.98.1 |
| Windows API | `windows = 0.62.2`（按需在 `Cargo.toml` 加 feature） |
| 其他依赖 | `anyhow`、`png`、`serde`/`toml`/`toml_edit`、`embed-resource` |
| 构建 | MSVC 工具链；`.cargo/config.toml` 为 `x86_64-pc-windows-msvc` 启用静态 CRT |
| 运行 | Windows 11 x64、交互式桌面；WGC/遮罩/剪贴板测试不能在没有桌面的 CI Runner 上做 |
| 子系统 | GUI（`#![windows_subsystem = "windows"]`），无控制台输出；诊断依赖日志和 `--capture-once` |

## 3. 构建、测试与运行

### 3.1 提交前门禁（与 CI 一致）

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo build --release --locked
```

- 依赖已缓存时可用 `--offline`；CI 与发布要求 `--locked`。
- 不要无故升级依赖或手改 `Cargo.lock`；版本变更只做必要的同步。
- 单测必须保持无显示器、无交互桌面依赖。新增逻辑优先抽成纯函数再测。

### 3.2 脚本

| 脚本 | 用途 |
|---|---|
| `scripts/build.ps1` | 跑测试 + Release 构建 + 复制到 `dist/`；`-SkipTests`、`-Install` |
| `scripts/install.ps1` | 停止旧进程、安装到 `%LOCALAPPDATA%\Programs\ScreenShotFX`、快捷方式、自启、启动；`-NoStart` 可只装不启动 |
| `scripts/uninstall.ps1` | 卸载；`-PurgeData` 同时删除 `%APPDATA%`/`%LOCALAPPDATA%` 数据 |
| `scripts/package.ps1` | 生成发布 zip、独立 exe、`SHA256SUMS.txt`；`-Version X.Y.Z -SkipBuild` |
| `scripts/verify.ps1` | 交互式 GDI/WGC 截图验证；需要真实桌面 |
| `scripts/make-icon.ps1` | 重新生成 `assets/app.ico` |

PowerShell 执行策略可能拦截 `.ps1`，统一用：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1
```

### 3.3 本地运行与诊断

```powershell
# 开发运行（跳过首次自启写入与本次自启同步）
cargo run -- --no-autostart

# 并行跑多个开发实例时使用独立单例名
$env:FXSHOT_MUTEX_NAME = 'Local\fxshot-dev-1'

# 无 UI 截一张指定显示器，验证后端/颜色
.\dist\fxshot.exe --capture-once .\target\check.png --backend auto --monitor "\\.\DISPLAY1"

# 强制后端
.\dist\fxshot.exe --capture-once .\target\check-gdi.png --backend gdi --monitor 1
```

CLI：

```text
fxshot.exe --capture-once [output.png] [--backend auto|gdi|wgc] [--monitor <1-based|device-name>]
fxshot.exe --set-autostart on|off
fxshot.exe --no-autostart
```

- `--monitor` 的序号从 1 开始，按 `EnumDisplayMonitors` 顺序；设备名可写 `\\.\DISPLAY1` 或 `DISPLAY1`，不区分大小写。
- 不指定 `--monitor` 时截取包含桌面原点 (0,0) 的显示器，通常是主屏。
- 配置：`%APPDATA%\ScreenShotFX\config.toml`；日志：`%LOCALAPPDATA%\ScreenShotFX\fxshot.log`；默认截图目录：`%USERPROFILE%\Pictures\fx`。
- 关键日志行：
  - `capture-once monitor \\.\DISPLAY1 3840x2160 mode=wcg backend=gdi`
  - `captured \\.\DISPLAY1 3840x2160 mode=wcg with gdi`
  - `wgc \\.\DISPLAY1 format=bgra8 mode=wcg`
  - `quick capture \\.\DISPLAY1 with gdi`

### 3.4 测试隔离

不要用真实用户的配置/日志/截图目录做自动测试。把环境变量指向工作区内：

```powershell
$run = Join-Path $PWD 'target\agent-run'
New-Item -ItemType Directory -Force "$run\Roaming\ScreenShotFX", "$run\LocalAppData", "$run\shots" | Out-Null
# 写临时 config.toml（save_dir 指向 $run\shots，clipboard=false，autostart_initialized=true）
# 启动时设置：
#   APPDATA=$run\Roaming
#   LOCALAPPDATA=$run\LocalAppData
#   fxshot.exe --no-autostart ...
```

注意：`--no-autostart` 只跳过自启同步，不跳过配置读取；临时配置里的 `save_dir` 要写成绝对路径，并用 UTF-8 **无 BOM** 保存。

## 4. 架构与数据流

### 4.1 模块地图

| 路径 | 职责 |
|---|---|
| `src/main.rs` | 入口；PMv2 DPI/COM 初始化；CLI 解析；分派到托盘或诊断命令 |
| `src/app.rs` | 单实例互斥体、隐藏主窗口、托盘、全局热键、菜单、`start_selection`/`quick_capture`、消息循环 |
| `src/config.rs` | `AppConfig`、`Backend`、快捷键解析、默认模板、校验/钳制、`toml_edit` 局部更新 |
| `src/monitor.rs` | `MonitorInfo`、显示器枚举、DPI、颜色模式、虚拟桌面、选屏、`--monitor` 查找 |
| `src/capture/mod.rs` | `CapturedImage`、虚拟桌面 `Frame`（BGRA top-down）、合成/裁剪/取色、后端策略、逐屏回退 |
| `src/capture/gdi.rs` | `CreateDCW`/`BitBlt`（`SRCCOPY + CAPTUREBLT`）、32bpp top-down DIB；设备 DC 失败时退回虚拟桌面 DC |
| `src/capture/wgc.rs` | D3D11 设备、`GraphicsCaptureItem`、帧池、staging texture、Map/Unmap、FP16/BGRA8 |
| `src/capture/hdr.rs` | `ColorMode`（SDR/WCG/HDR）、活动颜色模式查询、半精度转换、HDR 色调映射 |
| `src/capture/clipboard.rs` | `CF_DIB`（24bpp、自底向上、4 字节对齐）与 `CF_UNICODETEXT` |
| `src/overlay/mod.rs` | `OverlaySession`、线程局部 `SESSION`、开始/结束、鼠标/键盘/工具条逻辑、保存/复制、取色 |
| `src/overlay/window.rs` | 每屏遮罩窗口、`FrameSurface`、`DimSurface`、绘制顺序、边框/尺寸标签、DIB 创建/清理 |
| `src/overlay/edit.rs` | 选区命中测试、移动、8 向缩放、最小尺寸/边界钳制 |
| `src/overlay/snap.rs` | 窗口/显示器边缘吸附 |
| `src/overlay/toolbar.rs` | 工具条尺寸/布局/按钮命中，`scale()` 统一 96-DPI 基准缩放 |
| `src/overlay/color.rs` | 取色面板、9×9 放大镜、RGB/HEX 格式 |
| `src/overlay/keyboard.rs` | 仅遮罩期间安装的 `WH_KEYBOARD_LL` 钩子，向主窗口投递 `WM_APP_OVERLAY_KEY` |
| `src/win_util.rs` | 顶层窗口枚举/DWM 边界/Shell Known Folder/窗口标题类名/鼠标按键状态 |
| `src/naming.rs` | `Screenshot_<时间戳>.png` 命名、`create_new` 去重、PNG 编码 |
| `src/logging.rs` | UTF-8 追加日志、逐行 flush、1 MB 轮转到 `fxshot.log.1` |
| `src/autostart.rs` | HKCU `Run` 项读取/写入/校准，命令行为 `"exe" --autostart` |
| `src/diagnostics.rs` | `--capture-once`、`--set-autostart` |
| `src/geometry.rs` | 半开区间 `Rect`、`Point`、并/交/钳制/最近距离 |
| `assets/` | 图标、版本资源 `app.rc`、清单（PMv2、longPath、asInvoker） |
| `.github/workflows/` | CI（main/PR）与 Release（`v*` 标签/手动） |

### 4.2 启动流程

1. `main`：`SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`、`RoInitialize(MULTITHREADED)`、`logging::init`。
2. 解析 CLI；`--capture-once`/`--set-autostart` 走 `diagnostics` 后退出。
3. `app::run`：
   - 创建单实例互斥体 `Local\ScreenShotFX.Singleton`（可用 `FXSHOT_MUTEX_NAME` 覆盖）；已存在则向已有实例投递 `WM_APP_ALREADY_RUNNING`。
   - 读取/创建配置；首次运行（`autostart_initialized=false`）默认写自启；`autostart::sync` 校准 HKCU Run。
   - 解析快捷键；注册隐藏主窗口类 `FxShotMainWindow`、创建隐藏主窗口、托盘图标、`RegisterHotKey`。
   - 消息循环：`WM_HOTKEY`（1=选区、2=快速）、托盘消息、遮罩关闭/按键消息、`WM_DISPLAYCHANGE`（关闭遮罩）、托盘重建、退出。

### 4.3 选区截图数据流

1. 每次截图重新 `monitor::enumerate()`（显示拓扑可能变化）。
2. `capture::capture_virtual` 对每块屏 `capture_monitor_with_fallback`，按虚拟桌面坐标合成一个 `Frame`（BGRA、top-down、stride=width*4）。失败屏保持黑色，至少一块成功。
3. `win_util::top_level_windows` 枚举可吸附窗口（排除自身 PID、工具窗口、cloaked、最小化/不可见、Shell 类窗口）。
4. `overlay::start`：创建 `FrameSurface`（整帧 DIB 副本）、`DimSurface`（1×1 黑 DIB）、每屏一个 `WS_POPUP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW` 窗口；聚焦；安装键盘钩子；启动 30 ms 取色定时器。
5. 每屏绘制（`paint_session`）：
   - 从帧 BitBlt 该屏区域到后台 DIB；
   - 用 `AlphaBlend` 把 1×1 黑块以 `SourceConstantAlpha=108` 铺满（压暗）；
   - 选区区域重新 BitBlt（不压暗）、画边框和尺寸/标题标签；
   - 锁定后画工具条；鼠标所在屏画取色面板/放大镜；
   - `force_opaque` 把 alpha 全置 255；`BitBlt` 到窗口 DC；`EndPaint`。
6. 输入：悬停吸附窗口 → 拖拽/吸附 → 锁定；拖动内部移动、边角缩放；工具条保存/复制/取消；`C` 复制色值、`Shift` 切 RGB/HEX、方向键 1px 微调、`Ctrl+C` 复制选区、`Enter`/`Space` 保存、`Esc`/右键取消。
7. 保存：`frame.crop_rgb(selection)` → `naming::save_png`；可选 `clipboard::copy_rgb`；关闭遮罩并 `DwmFlush` + 30 ms 等待合成器清理。

### 4.4 快速截图数据流

`GetCursorPos` → `monitor::select_monitor`（点在屏内优先，否则最近）→ 单屏 `capture_monitor_with_fallback` → BGRA→RGB → PNG + 可选剪贴板。全程无窗口、无提示音、无焦点变化。

### 4.5 颜色管线（0.1.3 修复重点，务必理解）

Windows 的“高级颜色”包含三种活动模式，**不能把它们都当成 HDR**：

| 模式 | `activeColorMode` | 内容 | 后端/格式 | 是否色调映射 |
|---|---|---|---|---|
| SDR | 0 | 普通 sRGB | GDI | 否 |
| WCG/ACM | 1 | 广色域/“自动管理应用的颜色”，画面仍是 SDR | auto → GDI；强制 WGC → BGRA8 | **否** |
| HDR | 2 | 真 HDR（scene-referred） | WGC → FP16 | 是，`hdr_exposure` 生效 |

- 检测入口：`hdr::color_mode_for_device(device_name) -> Option<ColorMode>`。
  - 优先调用 `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2`（type 15，结构体手工定义，**36 字节**），读 `activeColorMode`；0/1/2 映射为 SDR/WCG/HDR。
  - type 15 失败（24H2 以前）或返回未知值时，回退 type 9：`advancedColorEnabled` 为真 → HDR，否则 SDR。
  - 路径找不到/查询失败 → `None`，调用方按 SDR 处理（与旧行为一致）。
- 策略：`capture::preferred_backend` 只在 `ColorMode::Hdr` 时选 WGC；SDR/WCG 选 GDI；显式 `backend=gdi|wgc` 优先。
- WGC 格式：HDR → `R16G16B16A16Float` + `tone_map_rgb`；SDR/WCG → `B8G8R8A8UIntNormalized`，**不**做色调映射；HDR FP16 失败才重试 BGRA8。
- `hdr_exposure` 只影响 HDR；WCG/SDR 不受它影响。
- 取色值来自冻结帧（`Frame::pixel_at`），因此抓取正确时取色也正确。
- **历史故障**：0.1.2 及以前把 `advancedColorEnabled` 当 HDR，K27（WCG、HDR 关）被送进 FP16 + `1-exp(-3x)`，SDR 中间调被提亮，遮罩和保存 PNG 都发白。修复后 auto 走 GDI、强制 WGC 走 BGRA8。
- HDR 路径的 tone map 曲线与 SDR 白点归一化目前保持原样；没有真 HDR 环境时不要凭感觉改它。

### 4.6 坐标与 DPI 不变量

- 进程是 **PerMonitorV2**；`MonitorInfo.rect`、窗口矩形、选区、取色坐标全部是**全局物理像素**。
- `Rect` 是半开区间（`right`/`bottom` 不含）；宽高用 `saturating_sub`。
- 虚拟桌面原点可以是负坐标（例如左侧副屏），`Frame.origin` 也是负值；所有帧内偏移都减去 `frame.origin`。
- 遮罩窗口按 `monitor.rect` 的物理尺寸/位置创建；不要引入 96-DPI 逻辑坐标。
- UI 尺寸以 96 DPI 为基准，用 `toolbar::scale(value, dpi)` 缩放；吸附阈值同样按当前屏 DPI 缩放。
- 测试涉及多屏时，覆盖负坐标与不同 DPI；不要假设主屏在 (0,0) 或只有一块屏。

### 4.7 资源与线程

- UI 单线程；截图在消息线程同步完成。不要在消息处理里加入长时间阻塞或网络/睡眠循环。
- GDI 对象必须成对释放：`SelectObject` 恢复旧对象、`DeleteObject`、`DeleteDC`；`FrameSurface`/`DimSurface`/`OverlayWindow` 用 `Drop` 兜底。
- WGC 必须在所有路径关闭 `GraphicsCaptureSession` 和帧池；Map 成功后必须 Unmap。
- 剪贴板：`GlobalAlloc` 失败路径要 `GlobalFree`；`SetClipboardData` 成功后所有权归系统，不要再释放。
- 日志写入在 UI 线程，逐行 flush；不要在高频循环里刷日志。

## 5. 关键不变量与常见坑

1. **WCG ≠ HDR**：任何“高级颜色/高级色彩”判断都必须先看 `activeColorMode`。只在 WGC 的 FP16 分支做 tone map。
2. **遮罩和保存共享同一帧**：修颜色/内容问题时不要只改遮罩绘制；先确认 `Frame` 是否正确。
3. **auto 保持静默**：SDR/WCG 走 GDI，避免 WGC 黄色边框；只有真 HDR 或用户显式兼容模式才用 WGC。
4. **`MonitorInfo` 加字段会破坏测试字面量**：`monitor.rs`、`overlay/snap.rs`、`overlay/toolbar.rs`、`overlay/color.rs` 都有构造；改字段后全局搜索 `MonitorInfo {`。
5. **遮罩是普通 GDI 窗口**：`WM_ERASEBKGND` 返回 1；不要加分层窗口/背景刷；绘制后 alpha 强制 255；压暗用常量 alpha，不要改成逐像素 alpha。
6. **DIB 约定**：32bpp `BI_RGB`、`biHeight` 为负（top-down）、stride=`width*4`；跨 DC `BitBlt` 时源坐标相对帧原点。
7. **抓取失败要回退**：`capture_monitor_with_fallback` 已处理 GDI/WGC 互备；新增后端要保持“失败记录日志 + 明确回退”的行为。
8. **不要捕获自己**：窗口枚举排除本进程 PID、工具窗口、cloaked/不可见窗口；遮罩窗口也是 `WS_EX_TOOLWINDOW`。
9. **原始 FFI 结构**：`AdvancedColorInfo2` 的布局、`size`、`type=15` 是硬约束；改字段必须同步 `size_of` 测试与实机验证。
10. **配置写入要保注释**：运行时修改只用 `config::update_runtime_flags`（`toml_edit`），不要整文件重写；配置是 UTF-8，手写临时配置用无 BOM。
11. **不要加管理员权限**：清单是 `asInvoker`；提权会让热键/剪贴板/截图行为变复杂。
12. **无网络/遥测**：不要在代码或脚本里引入上传、检查更新、统计。
13. **快捷键冲突**：注册失败要记录并提示，不要静默覆盖；`select_hotkey` 与 `quick_hotkey` 不能相同。
14. **`--autostart` 现状（已知小问题）**：`autostart.rs` 写入的启动命令带 `--autostart`，但 `main` 没有对应分支，登录时会记录 `ignoring unknown argument: --autostart`。若要清理，加显式 no-op 分支或调整命令行，并回归自启测试。
15. **发布版本三处同步**：`Cargo.toml`、`Cargo.lock`（随构建更新）、`assets/app.rc` 的 `FILEVERSION`/`PRODUCTVERSION`/`FileVersion`/`ProductVersion`；Release 工作流会校验。

## 6. 测试策略

### 6.1 自动化

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

单测集中在纯逻辑：`geometry`、`overlay/edit`、`overlay/snap`、`overlay/color`、`overlay/toolbar`、`config`、`naming`、`capture/hdr`、`capture/mod`、`monitor`、`overlay`。新增 Windows API 调用时，把可判定部分抽成纯函数（如模式映射、策略选择）并补测；不要为了测试去依赖真实显示器。

### 6.2 手工矩阵（涉及 UI/抓取时必须覆盖）

- 单屏 / 双屏 / 负坐标 / 主副屏互换；100%、150%、200% 缩放；旋转屏。
- auto / gdi / wgc 三种后端；HDR 开/关；WCG/ACM 开/关。
- 选区：悬停吸附窗口、拖拽、锁定后移动/缩放、工具条、取色、`C`、`Shift`、方向键、`Esc`/`Enter`/`Space`/`Ctrl+C`。
- 快速截图：两块屏各一次；剪贴板；保存目录不可写时的错误提示。
- 托盘：重新加载配置、兼容模式切换、开机自启、退出；单实例重复启动。
- 显示变化：截图期间改分辨率/拔插显示器（应关闭遮罩）。
- 日志：失败路径有 `backend ... failed` / `fallback`，成功路径有 `captured ... mode=... with ...`。

### 6.3 颜色验证配方

1. 在目标显示器上放一个**静态**已知色块（例如全屏 WinForms 窗口，背景 128 灰；再做 32 灰和彩色各一次）。不要用正在播放的视频做逐像素对比。
2. `--capture-once <png> --backend auto|gdi|wgc --monitor <目标屏>`。
3. 用 `System.Drawing` + `LockBits` 读 PNG，算中位/均值：
   - 正确：GDI 与 WCG 的强制 WGC 都应等于设定值（±1~2）。
   - 错误：SDR 128 灰被 tone map 成约 183，就是“发白”复发。
4. 若验证 WGC：必须在正常用户会话里运行（托盘程序或普通终端）；由受限代理/沙箱进程启动的 WGC 会被系统拒绝（`0x80070005`，旧版本同样如此），不能据此判断代码回归。
5. 日志确认：K27 这类 WCG 屏应出现 `mode=wcg`，auto 时 `backend=gdi`，强制 WGC 时 `format=bgra8`；内屏应出现 `mode=sdr`。

## 7. 常见改动配方

### 7.1 新增配置项

1. `config.rs`：`AppConfig` 字段 + `Default` + `normalize` 校验/钳制。
2. 默认模板字符串（`write_default_config`）与 `config.example.toml` 同步。
3. `README.md` 字段表和示例同步。
4. 测试：TOML 解析、默认值、钳制。
5. 不要整文件重写用户配置；运行时更新走 `update_runtime_flags`。

### 7.2 新增 `MonitorInfo` 字段

1. `monitor.rs` 结构体 + `monitor_info_from_handle` 赋值。
2. 搜索并更新所有测试构造：`monitor.rs`、`overlay/snap.rs`、`overlay/toolbar.rs`、`overlay/color.rs`。
3. 需要日志时在 `capture_virtual`/`diagnostics::capture_once` 的 `mode=`/尺寸行补充。
4. `cargo test` + 多屏手工验证。

### 7.3 新增 CLI 参数

1. `main.rs` 参数解析与 `show_help()`。
2. `diagnostics::capture_once`（或对应命令）实现；错误信息要可操作。
3. `README.md` 命令行表；必要时 `scripts/verify.ps1`。
4. 保持未指定参数时的默认行为兼容。

### 7.4 新增/修改遮罩 UI

1. 尺寸/布局放在 `overlay/toolbar.rs` 或对应模块，用 `toolbar::scale` 按 DPI 缩放。
2. 绘制加在 `overlay/window.rs::paint_session` 的正确顺序（帧 → 压暗 → 选区 → 边框/标签 → 工具条 → 取色 → 强制 alpha）。
3. 命中测试加在 `overlay/mod.rs` 的鼠标路径，保持锁定/非锁定状态机一致。
4. 纯逻辑（布局/命中/格式化）补单测；颜色/DPI 现场验证。

### 7.5 修改抓取后端/颜色格式

1. 先改 `capture::preferred_backend` 的策略与单测。
2. WGC 格式分支在 `capture/wgc.rs`；HDR 才允许 FP16+tone map。
3. 日志补 `mode=` / `format=`；`--capture-once` 验证。
4. 更新 README 的“后端/HDR/广色域”章节与 FAQ。
5. 涉及 HDR 的改动必须在真 HDR 显示器上验证，或明确标注“未实机验证”。

### 7.6 新增 Windows API / 手工 FFI

1. 先在 `Cargo.toml` 的 `windows` features 中找现成绑定；没有才手工定义。
2. 手工结构必须 `#[repr(C)]`，核对字段宽度、对齐、`size`、type id；加 `size_of` 断言/测试。
3. `unsafe` 块尽量小；失败返回 `Option`/`Result`，不要 panic。
4. 记录失败日志并保留降级路径；不要因为新 API 失败导致截图/遮罩完全不可用。
5. 在目标 Windows 版本实机验证；24H2 以前要有回退。

## 8. 发布流程

1. 同步版本：
   - `Cargo.toml` 的 `version`
   - `assets/app.rc` 的 `FILEVERSION`、`PRODUCTVERSION`、`FileVersion`、`ProductVersion`
   - `Cargo.lock` 由 `cargo` 自动更新（提交它）。
2. 跑 3.1 全部门禁；涉及截图/UI 的改动先过 6.2/6.3。
3. 提交并推送 `main`。
4. 打**附注标签**（与既有 `v0.1.0`~`v0.1.3` 风格一致）：

   ```powershell
   git tag -a v0.1.4 -m "ScreenShot FX 0.1.4"
   git push origin v0.1.4
   ```

5. 标签推送触发 `.github/workflows/release.yml`：校验标签/版本 → 测试 → Release 构建 → `package.ps1` → 上传产物 → 创建 GitHub Release。
6. 监控与验收：

   ```powershell
   gh run list --workflow release.yml --limit 3
   gh run watch <run-id> --exit-status
   gh release view v0.1.4
   gh release download v0.1.4 --dir target\release-verify
   ```

   下载后核对 `SHA256SUMS.txt`、`fxshot.exe` 的 `0.1.4.0` 版本、zip 内容。
7. 不要移动/复用已发布标签；不要在有未提交改动时打标签。
8. 本地构建与 CI 构建可能因 Rust 版本/链接环境不同而字节不同，**源码与标签**才是契约；不要用本地 hash 去要求 Release 资产一致。

## 9. 本机 / 代理环境提示（非产品行为，可能随环境变化）

以下是当前开发机与 DSH 代理沙箱的实际情况，方便后续迭代时少踩坑：

- **Shell**：默认是 Windows PowerShell 5.1；`.ps1` 可能被执行策略拦截，统一用 `powershell -NoProfile -ExecutionPolicy Bypass -File`。
- **文件沙箱**：工作区外写入需要批准；测试时用 `APPDATA`/`LOCALAPPDATA` 环境变量重定向，避免动真实配置/日志。安装/卸载/自启写入需要正常用户会话或显式提权。
- **启动进程**：`Start-Process -Wait`（ShellExecute）在受限环境中可能挂起；用 .NET `ProcessStartInfo` + `UseShellExecute=$false` 或直接 `& exe`。
- **WGC 权限**：由代理/沙箱进程启动的 `--capture-once --backend wgc` 会失败为 `0x80070005`（旧版 0.1.2 同样失败），这是会话令牌限制，不是代码回归。WGC 必须从用户正常会话（托盘实例、普通终端）验证。
- **Git 推送**：credential manager 在受限环境会因 msys `sh` 命名管道失败。可用 `gh` 已登录凭据做一次性 `http.extraheader` 推送；**不要把 token 写入 URL、配置文件、日志或提交**：

  ```powershell
  $token = (gh auth token --hostname github.com).Trim()
  $pair  = 'kamisato-funeki:' + $token
  $basic = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($pair))
  git -c credential.helper= -c "http.https://github.com/.extraheader=Authorization: Basic $basic" push origin main
  ```

  正常用户环境能用 GCM 时优先用普通 `git push`。
- **编码**：Windows PowerShell 5.1 的 `Get-Content` 可能把 UTF-8 显示成乱码；读取用 `[IO.File]::ReadAllText($p, [Text.Encoding]::UTF8)`。写测试配置用 `New-Object System.Text.UTF8Encoding($false)`（无 BOM）；TOML 文件不要带 BOM，否则可能解析失败。
- **WMI**：`Get-CimInstance Win32_VideoController` 等可能被拒绝；需要显示器信息时用 `DISPLAYCONFIG` P/Invoke 或程序日志。
- **当前开发机**（可能变化）：
  - Windows 11 26H2，build 26300；RTX 4070 Laptop + Intel UHD；存在 LuminonCore IDDCX、ToDesk 虚拟显示适配器。
  - `\\.\DISPLAY1` = **K27** 3840×2160 HDMI 150%，位于主屏左侧（负坐标），非主屏；WCG/ACM 开启、HDR 关闭。
  - `\\.\DISPLAY2` = 内置屏 2560×1600 150%，主屏，SDR，165 Hz。
  - 验证 WCG/ACM 修复时用 `--monitor "\\.\DISPLAY1"`；不要据此假设所有用户都有相同拓扑。
- **测试卫生**：不要改动系统 HDR/ACM/缩放设置；测试窗口用完关闭；恢复鼠标位置；不要把测试 PNG 写进用户的 `Pictures\fx`；结束测试实例（隐藏窗口 `WM_CLOSE` 或等待退出），不要留下后台进程/定时任务。

## 10. Definition of Done

- [ ] `fmt` / `clippy -D warnings` / `test` / `release build` 全部通过。
- [ ] 受影响路径有测试：纯逻辑单测 + 至少一次真实 GDI 抓取；涉及 WGC/HDR 时在可用环境验证并记录。
- [ ] 颜色模式（SDR/WCG/HDR）没有被混用；WCG 没有被 tone map。
- [ ] 日志能说明每块屏的 `mode=` 与后端；失败有回退记录。
- [ ] README/配置模板/CLI 帮助同步；版本只在发布时改。
- [ ] 工作区干净、diff 只包含预期文件；发布时版本三处一致、标签为附注标签、Release 资产已校验。
- [ ] 没有引入管理员权限、网络请求、遥测或新的常驻线程/钩子。

## 11. 参考

- 用户行为/FAQ/发布说明：[README.md](README.md)
- CI：[.github/workflows/ci.yml](.github/workflows/ci.yml)
- Release：[.github/workflows/release.yml](.github/workflows/release.yml)
- 构建/安装/打包：[scripts/build.ps1](scripts/build.ps1)、[scripts/install.ps1](scripts/install.ps1)、[scripts/package.ps1](scripts/package.ps1)
- 颜色管线：[src/capture/hdr.rs](src/capture/hdr.rs)、[src/capture/mod.rs](src/capture/mod.rs)、[src/capture/wgc.rs](src/capture/wgc.rs)
- 遮罩绘制：[src/overlay/mod.rs](src/overlay/mod.rs)、[src/overlay/window.rs](src/overlay/window.rs)
