# ScreenShot FX

> Windows 11 常驻截图工具——`Shift+F1` 冻结屏幕、自由选区；**`Shift+F2` 在游戏里静默截取整屏**，不弹窗、不遮罩、不打断操作。

[![CI](https://github.com/kamisato-funeki/fx-screen-shot/actions/workflows/ci.yml/badge.svg)](https://github.com/kamisato-funeki/fx-screen-shot/actions/workflows/ci.yml)
[![Release](https://github.com/kamisato-funeki/fx-screen-shot/actions/workflows/release.yml/badge.svg)](https://github.com/kamisato-funeki/fx-screen-shot/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/kamisato-funeki/fx-screen-shot?display_name=tag)](https://github.com/kamisato-funeki/fx-screen-shot/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows%2011%20x64-0078D4)

ScreenShot FX 是一个用 Rust 和原生 Win32 编写的轻量截图工具。它常驻系统托盘，**没有主窗口、没有控制台、不需要管理员权限**，也不会在后台弹广告或联网。

- 想精细截取窗口或区域：按 `Shift+F1`，屏幕冻结后悬停吸附窗口、拖拽框选，松开鼠标出现工具条。
- **想在游戏里快速留一张图：按 `Shift+F2`**，直接截取鼠标所在显示器并保存，成功时屏幕上不会出现任何东西。
- 默认同时复制到剪贴板，方便粘贴到聊天窗口、文档或作图软件。

---

## 游戏快速截图：`Shift+F2`

这是本项目最推荐在游戏中使用的功能。按下快捷键后，程序只做三件事：**抓取屏幕 → 编码 PNG → 写入磁盘**；不会打开遮罩、工具条、编辑器或通知窗口。

### 在游戏里怎么用

1. 启动 `fxshot.exe`，确认托盘区出现蓝色截图图标。
2. 正常进入游戏。
3. 按 `Shift+F2`。
4. 截图已经保存到 `图片\fx`；如果开启了剪贴板，可以直接 `Ctrl+V` 粘贴。

成功时**没有任何视觉或声音反馈**，这是有意设计的：游戏画面不会被遮挡，游戏也不会被切到后台。只有失败时（例如目录不可写、截图 API 报错），托盘才会弹出提示气泡，同时写入日志。

### 为什么它适合游戏

| 特性 | 说明 |
|---|---|
| 无遮罩 | 不创建全屏覆盖窗口，不冻结画面，不显示工具条 |
| 无提示音 | 静默保存；成功路径不弹气泡 |
| 不抢焦点 | 不激活任何窗口，不把游戏切到后台 |
| 整屏截取 | 截取鼠标指针所在的那一块显示器，适合无边框全屏游戏 |
| 系统级热键 | 使用 `RegisterHotKey` 注册全局快捷键，游戏有焦点时也能触发 |
| 不注入游戏 | 不加载 DLL、不 Hook DirectX/图形管线、不读写游戏内存 |
| 本地处理 | 不联网、不上传截图、不收集遥测 |
| 支持多屏 | 每次按快捷键时重新枚举显示器，按鼠标位置选屏 |
| 自动处理 HDR | HDR 显示器自动切到 WGC 后端并做色调映射 |

> 截图不包含鼠标指针。`Shift+F2` 截取的是**整块显示器**；如果游戏运行在窗口中，窗口外的桌面也会进入图片。只想截游戏窗口时，请使用 `Shift+F1` 单击窗口。

### `Shift+F1` 与 `Shift+F2` 怎么选

| | `Shift+F1` 选区截图 | `Shift+F2` 快速截图 |
|---|---|---|
| 交互 | 冻结全屏，悬停吸附窗口，拖拽选区，显示工具条 | 完全静默，无任何 UI |
| 截图范围 | 自由选区、整窗、跨屏区域 | 鼠标所在显示器的完整画面 |
| 游戏内体验 | 遮罩会暂时打断游戏操作 | 不打断，推荐 |
| 典型场景 | 桌面、浏览器、聊天、教程配图 | 游戏、全屏视频、直播、快速留档 |
| 输出 | 选区 PNG，可选复制到剪贴板 | 整屏 PNG，可选复制到剪贴板 |
| 成功反馈 | 工具条与选区边框 | 无反馈，文件直接落盘 |

### 游戏截图实战建议

- **显示模式**：优先使用“无边框全屏 / 窗口化全屏”。这种模式在 Windows 11 上合成方式最稳定，GDI 和 WGC 后端都容易成功。
- **独占全屏**：部分游戏的独占全屏会让 GDI 截到黑屏。可以在托盘菜单勾选“兼容模式（WGC，有黄框）”后重试；仍失败时改用无边框全屏。
- **HDR 游戏**：保持 `backend = "auto"` 即可。程序检测到 HDR 显示器会自动使用 WGC，并在截图时做 SDR 色调映射；如果颜色偏亮/偏暗，调整 `hdr_exposure`（0.25–4.0）后重新加载配置。
- **多显示器**：先把鼠标移到游戏所在屏幕，再按 `Shift+F2`。程序选择鼠标指针所在的显示器；鼠标落在屏幕之间的空隙时，选最近的一块。
- **4K / 高刷**：截图和 PNG 编码在消息线程中同步完成，耗时与分辨率有关，通常为几十到几百毫秒。它不会暂停游戏，但极短暂的帧时间波动仍有可能出现。
- **快捷键冲突**：如果 `Shift+F2` 被游戏或其他软件占用，日志会记录注册失败并弹出一次提示。把 `quick_hotkey` 改成 `Ctrl+Shift+F2` 等组合，然后在托盘菜单选择“重新加载配置”。
- **反作弊提醒**：本工具不注入、不 Hook、不读内存、不联网，只使用 Windows 公开的截图 API 与全局热键。但任何常驻后台程序在带反作弊的在线游戏中都存在被误判的可能，建议先在人机/单机模式验证，并自行评估竞技场景下的风险。

### 怎么确认截图成功了

成功时没有提示，可以这样检查：

```powershell
# 打开保存目录（默认）
explorer "$env:USERPROFILE\Pictures\fx"

# 查看最新截图
Get-ChildItem "$env:USERPROFILE\Pictures\fx" -Filter *.png |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 5 Name, Length, LastWriteTime

# 查看最近的快速截图日志
Get-Content "$env:LOCALAPPDATA\ScreenShotFX\fxshot.log" -Tail 30
```

日志中成功时会出现类似内容：

```text
INFO  quick capture \\.\DISPLAY1 with gdi
INFO  quick screenshot saved C:\Users\<你>\Pictures\fx\Screenshot_2026-10-19_20-30-12.png
```

---

## 功能特性

- **两张截图方式**：`Shift+F1` 精选取区，`Shift+F2` 游戏静默整屏。
- **托盘常驻**：无主窗口、无控制台、无管理员权限；单实例运行，重复启动只会通知已有实例。
- **窗口吸附**：`Shift+F1` 悬停时高亮窗口并显示标题与尺寸，单击锁定整窗，拖拽自由选区，靠近窗口/屏幕边缘自动吸附。
- **锁定后调整**：松开鼠标后仍可拖动选区内部移动、拖动边/角缩放，并实时更新工具条与尺寸标签。
- **PNG 输出**：保存到系统“图片”目录下的 `fx` 子目录，文件名带时间戳，同秒重复自动追加序号。
- **剪贴板**：默认保存的同时复制到剪贴板；工具条的“复制”按钮可只复制不保存。
- **开机自启**：首次运行自动写入当前用户的开机启动项，托盘菜单随时开关。
- **多显示器 / 混合 DPI**：所有坐标按物理像素处理，支持负坐标、不同缩放比例和上下排列。
- **HDR 支持**：自动检测高级颜色（HDR/WCG）显示器，使用 Windows Graphics Capture 捕获并色调映射。
- **兼容模式**：遇到全屏游戏、录屏软件冲突或 GDI 黑屏时，可强制使用 WGC。
- **配置热加载**：修改 `config.toml` 后在托盘菜单选择“重新加载配置”，无需重启。
- **本地隐私**：不联网、不收集信息、不上传截图；仅在选区截图期间临时安装键盘钩子。

---

## 安装

系统要求：**Windows 11 x64**。程序使用静态 CRT，目标机器不需要额外安装 VC++ 运行库。

### 方式一：下载 Release（推荐）

1. 打开 [最新 Release](https://github.com/kamisato-funeki/fx-screen-shot/releases/latest)。
2. 下载 `fxshot-<版本>-windows-x64.zip`。
3. 解压到任意目录。
4. 在解压目录打开 PowerShell，运行安装脚本：

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1
   ```

安装脚本会：

1. 停止正在运行的旧实例；
2. 复制 `fxshot.exe` 到 `%LOCALAPPDATA%\Programs\ScreenShotFX`；
3. 创建开始菜单快捷方式；
4. 写入当前用户的开机自启项；
5. 启动程序。

安装完成后，托盘出现图标，按 `Shift+F2` 即可开始游戏截图。

> 也可以不安装，直接双击解压目录里的 `fxshot.exe` 绿色运行。首次运行同样会写入开机自启；如不想自启，建议先用 `fxshot.exe --set-autostart off` 永久关闭。`--no-autostart` 只跳过本次启动的自启写入与同步，之后正常启动仍会按配置启用。

### 方式二：从源码构建

环境：

- Windows 11 x64
- Rust 1.82+（MSVC 工具链）
- Visual Studio C++ 生成工具 + Windows SDK（用于资源编译）

```powershell
git clone https://github.com/kamisato-funeki/fx-screen-shot.git
cd fx-screen-shot

# 运行测试
cargo test --locked

# 构建 Release
cargo build --release --locked
```

产物：`target\release\fxshot.exe`。

也可以使用仓库脚本：

```powershell
# 测试 + 构建 + 复制到 dist\fxshot.exe
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1

# 构建并生成发布 zip + SHA256SUMS.txt
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package.ps1
```

### 方式三：使用安装脚本从源码安装

先执行 `scripts\build.ps1`，再执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1
```

若 `dist\fxshot.exe` 不存在，安装脚本会自动尝试 `target\release\fxshot.exe`。

### 卸载

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1
```

卸载脚本会停止进程、移除开机自启、删除开始菜单快捷方式和安装目录。默认**保留配置与日志**；使用 `-PurgeData` 一并删除：

```powershell
.\scripts\uninstall.ps1 -PurgeData
```

卸载不会删除 `图片\fx` 里的截图。

### 下载校验

Release 页面提供 `SHA256SUMS.txt`。下载后可这样核对：

```powershell
Get-FileHash .\fxshot-0.1.0-windows-x64.zip -Algorithm SHA256
Get-Content .\SHA256SUMS.txt
```

两个 SHA-256 值一致即可。由于 exe 未做代码签名，从网络下载后首次运行可能触发 SmartScreen：选择“更多信息 → 仍要运行”，或直接从源码构建。

---

## 使用

### 托盘菜单

左键单击托盘图标：直接进入选区截图。  
右键单击托盘图标：打开菜单。

菜单项：

- 选区截图（显示当前快捷键）
- 快速截图（显示当前快捷键）
- 打开保存目录
- 打开配置目录
- 开机自启动（勾选）
- 兼容模式（WGC，有黄框）（勾选）
- 重新加载配置
- 关于
- 退出

托盘图标被 Explorer 重启影响时，程序会每 2 秒自动重试，并在任务栏重建消息后恢复。

### `Shift+F1` 选区截图

1. 按下 `Shift+F1`，当前整个虚拟桌面被冻结成一张底图，出现半透明遮罩。
2. 鼠标悬停在窗口上时自动高亮该窗口，并显示标题与尺寸。
3. 单击窗口可锁定整窗选区；按住拖拽可自由框选；靠近窗口边或屏幕边时自动吸附。
4. 松开鼠标后锁定选区，出现**取消 / 保存 / 复制**工具条。
5. 此时仍可：
   - 拖动选区内部移动整个选区；
   - 拖动边或角调整大小（按像素精确调整，不再吸附）；
   - 点击选区外清空选区重新选择。
6. 按 `Enter` / `Space` 保存，`Ctrl+C` 只复制，`Esc` / 鼠标右键取消。

遮罩打开时再按一次 `Shift+F1` 会取消本次截图。

### `Shift+F2` 快速截图

1. 按 `Shift+F2`。
2. 程序读取鼠标位置，选择所在显示器。
3. 使用当前后端抓取该显示器。
4. 写入 PNG；如果 `clipboard = true`，同时复制到剪贴板。
5. 成功时无任何 UI 反馈；失败时托盘气泡提示并写日志。

如果当前正在选区截图中按 `Shift+F2`，程序会先关闭选区遮罩，再执行快速截图。

### 快捷键总表

| 操作 | 说明 |
|---|---|
| `Shift+F1` | 进入选区截图；遮罩打开时再按一次取消 |
| `Shift+F2` | 快速截图鼠标所在显示器；成功时完全静默 |
| 鼠标悬停 | 高亮光标下的窗口，并显示标题与尺寸 |
| 单击窗口 | 锁定整窗选区，随后出现工具条 |
| 按住拖拽 | 自由选区；靠近窗口边/屏幕边时自动吸附 |
| 鼠标松开 | 锁定选区，显示“取消 / 保存 / 复制”工具条 |
| 锁定后拖动选区内部 | 移动选区，工具条和尺寸标签实时跟随 |
| 锁定后拖动边或角 | 调整选区大小；不吸附，按像素精确调整 |
| `Enter` / `Space` | 保存当前选区（并复制，若启用剪贴板） |
| `Ctrl+C` | 只复制当前选区，不保存文件 |
| `Esc` / 鼠标右键 | 取消本次截图 |
| 点击选区外 | 清空当前选区，重新选择；不会保存 |

### 工具条

从左到右：**取消、保存、复制**。

- **取消**：关闭遮罩，不保存也不复制。
- **保存**：裁切并保存 PNG；若 `clipboard = true`，同时复制。
- **复制**：只复制到剪贴板，不保存文件。

保存和复制始终作用于当前调整后的选区。

### 鼠标光标

- 新框选阶段（含窗口悬停和拖拽框选）：十字。
- 锁定后：选区内部为移动光标，边/角为缩放光标，工具条按钮为手形，其余区域为普通箭头。
- 截图中不包含鼠标指针。

### 多显示器与 DPI

- 每次截图动态枚举显示器，不缓存布局。
- 所有坐标按物理像素处理，支持不同缩放比例、负坐标和上下排列。
- 选区截图允许跨屏；屏幕之间的空隙在 PNG 中显示为黑色。
- 快速截图选择鼠标指针所在显示器；鼠标落在空隙时取最近显示器。

---

## 配置

### 配置文件位置

```text
%APPDATA%\ScreenShotFX\config.toml
```

首次运行会自动生成带注释的配置；也可以参考 [config.example.toml](config.example.toml)。

修改后，在托盘菜单选择“重新加载配置”即可生效，无需重启。若新快捷键注册失败，程序会保留旧快捷键并提示。

### 完整示例

```toml
# 保存目录，支持 %USERPROFILE% 等环境变量
# TOML 双引号字符串里的反斜杠需要写成 \\
save_dir = "%USERPROFILE%\\Pictures\\fx"

# 全局快捷键，必须至少包含一个修饰键
select_hotkey = "Shift+F1"
quick_hotkey = "Shift+F2"

# 截图后端：auto | gdi | wgc
backend = "auto"

# 截图后是否复制到剪贴板
clipboard = true

# 窗口/屏幕边缘吸附距离（96 DPI 基准像素，按显示器 DPI 缩放）
snap_distance = 10

# WGC HDR 色调映射曝光，范围 0.25 - 4.0
hdr_exposure = 1.0

# 开机自启
autostart = true

# 内部首次配置标记，一般不要手动修改
autostart_initialized = false
```

### 字段说明

| 字段 | 默认值 | 说明 |
|---|---|---|
| `save_dir` | `%USERPROFILE%\Pictures\fx` | 截图保存目录；支持 `%USERPROFILE%` 等环境变量，不存在会自动创建 |
| `select_hotkey` | `Shift+F1` | 选区截图快捷键 |
| `quick_hotkey` | `Shift+F2` | 快速截图快捷键；游戏内静默截图用的就是它 |
| `backend` | `auto` | `auto`：普通屏 GDI、HDR 屏 WGC；`gdi`：始终 GDI；`wgc`：兼容模式，始终 WGC |
| `clipboard` | `true` | 保存后是否同时复制；工具条的“复制”按钮不受此开关影响 |
| `snap_distance` | `10` | 吸附距离，96 DPI 基准像素，实际按显示器 DPI 缩放，范围 0–50 |
| `hdr_exposure` | `1.0` | WGC HDR 色调映射曝光，范围 0.25–4.0 |
| `autostart` | `true` | 开机自启；托盘菜单可切换 |
| `autostart_initialized` | `false` | 内部首次配置标记，一般不要手动修改 |

### 快捷键语法

- 必须至少包含一个修饰键：`Shift`、`Ctrl`、`Alt`、`Win`。
- 主键支持：
  - `F1`–`F24`
  - `A`–`Z`
  - `0`–`9`
  - `Space`、`Tab`、`Enter`、`Esc`
  - `Insert`、`Delete`、`Home`、`End`、`PageUp`、`PageDown`
  - `PrintScreen`、`Pause`
- 修饰键顺序不限，例如 `Ctrl+Alt+Shift+Win+X`。
- 两个快捷键不能相同；相同时快速截图会回退为默认的 `Shift+F2`。

### 后端与兼容模式

- `auto`（默认）：普通 SDR 显示器用 GDI，HDR 显示器自动用 WGC。
- `gdi`：始终使用 GDI。静默、无捕获边框，适合大多数桌面与无边框游戏；不支持部分独占全屏和 DRM 内容。
- `wgc`：始终使用 Windows Graphics Capture，也就是托盘菜单里的“兼容模式”。对全屏游戏、录屏软件更友好，但 Windows 会在被截显示器边缘显示黄色捕获边框。

托盘菜单的“兼容模式”会在 `auto` 与 `wgc` 之间切换（若当前为 `gdi`，则先切到 `wgc`）；要强制 GDI，请直接编辑配置文件。

---

## 命令行

```text
fxshot.exe
fxshot.exe --help
fxshot.exe --capture-once [output.png] [--backend auto|gdi|wgc]
fxshot.exe --set-autostart on|off
fxshot.exe --no-autostart
```

| 参数 | 说明 |
|---|---|
| 无参数 | 以托盘常驻模式启动 |
| `--capture-once [output.png] [--backend ...]` | 截取主显示器后退出；不创建托盘、不占用单实例、不写自启。未指定路径时写入 `%TEMP%\fxshot-capture-<pid>.png`，适合自动化验证 |
| `--set-autostart on\|off` | 更新配置与注册表自启项后退出；安装/卸载脚本使用 |
| `--no-autostart` | 开发运行，跳过首次自启写入与本次同步 |
| `--help` / `-h` / `/?` | 弹出帮助窗口 |

示例：

```powershell
# 用 GDI 截一张主屏，验证当前桌面
.\fxshot.exe --capture-once .\check.png --backend gdi

# 临时关闭开机自启
.\fxshot.exe --set-autostart off
```

---

## 文件与目录

| 内容 | 路径 |
|---|---|
| 配置 | `%APPDATA%\ScreenShotFX\config.toml` |
| 日志 | `%LOCALAPPDATA%\ScreenShotFX\fxshot.log` |
| 安装目录 | `%LOCALAPPDATA%\Programs\ScreenShotFX` |
| 默认截图目录 | `%USERPROFILE%\Pictures\fx` |
| 开始菜单快捷方式 | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\ScreenShot FX.lnk` |
| 开机自启项 | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 下的 `ScreenShotFX` |

日志超过 1 MB 时轮转为 `fxshot.log.1`。

---

## 截图后端与兼容性

### GDI（默认，静默）

- 通过桌面 DC 逐显示器 `BitBlt`，无任何系统级捕获指示。
- 适合桌面、窗口化游戏、无边框全屏、全屏视频。
- 不支持 DRM 保护内容、部分独占全屏游戏；HDR 内容可能偏色。
- 受 UAC 安全桌面/锁屏限制，无法截取。

### Windows Graphics Capture（HDR / 兼容模式）

- 用于 HDR 显示器自动模式和用户手动开启的兼容模式。
- Windows 会在被截显示器边缘显示黄色隐私边框；未打包的普通 Win32 应用无法可靠关闭它，因此默认只对 HDR 使用。
- HDR 使用 FP16 捕获并色调映射为 8-bit sRGB PNG；PNG 本身不保留 HDR 高动态范围。
- 某些受保护内容、独占全屏游戏仍可能失败；失败时会回退 GDI 并写入日志。

### HDR

- 自动检测 Windows 高级颜色（HDR/WCG）是否开启。
- `auto` 后端下，HDR 显示器自动使用 WGC，避免 GDI 截出灰暗/偏色画面。
- 曝光不合适时调整 `hdr_exposure`，重新加载配置后再试。

### 受保护内容与独占全屏

- UAC 安全桌面、锁屏、DRM 视频、部分受保护的游戏画面无法截取，这是 Windows 的安全边界。
- 少量独占全屏游戏在两种后端下都可能黑屏；改用无边框全屏通常可解决。

---

## 常见问题（FAQ）

### 1. 按 `Shift+F2` 没反应

- 确认程序正在运行，托盘有图标。
- 查看日志是否出现 `hotkeys registered: Shift+F1 and Shift+F2`；若提示注册失败，说明快捷键被其他程序占用。修改 `quick_hotkey` 后重新加载配置。
- `Shift+F1` / `Shift+F2` 是系统级全局快捷键：注册成功后按键会被本程序接收，不会再传给游戏；如果游戏或其他软件先注册了同一组合，本程序会注册失败。遇到冲突时修改 `config.toml` 中的快捷键。
- 以管理员权限运行的游戏通常不会阻止系统级热键，但某些安全软件可能拦截全局热键。

### 2. 截图文件在哪里

默认在 `%USERPROFILE%\Pictures\fx`。文件名形如：

```text
Screenshot_2026-10-19_20-30-12.png
Screenshot_2026-10-19_20-30-12_2.png   # 同一秒内第二张
```

也可以在托盘菜单选择“打开保存目录”。

### 3. 游戏截出来是黑屏

- 先在托盘菜单开启“兼容模式（WGC，有黄框）”重试。
- 把游戏改为“无边框全屏 / 窗口化全屏”。
- 检查是否属于 DRM 保护内容或 UAC 安全桌面。
- 查看日志中的 `backend ... failed` 与 `fallback` 记录。

### 4. HDR 颜色不对，或者边缘有黄框

- 黄框是 Windows Graphics Capture 的隐私指示器，未打包 Win32 应用无法可靠关闭；只在 HDR 和兼容模式下出现。
- 颜色偏亮/偏暗：调整 `hdr_exposure`（0.25–4.0），重新加载配置。
- HDR 截图输出为 SDR PNG，不保留 16-bit 高动态范围。

### 5. 剪贴板里没有图

- 确认 `clipboard = true`，并已重新加载配置。
- 快速截图成功后会同步复制；如果目标程序以管理员身份运行，而本工具不是管理员，Windows 会阻止跨完整性级别粘贴。
- 查看日志中的 `clipboard copy failed`。

### 6. 开机没有自启

- 检查 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 是否存在 `ScreenShotFX`。
- Windows 11 设置中的“启动应用”可能被手动禁用。
- 如果 exe 被移动，下次正常启动会自动更新自启路径。
- 安装脚本在受限环境中写注册表失败时会给出警告；在正常登录的桌面运行即可。

### 7. 托盘图标不见了

- 程序会每 2 秒自动重试，并在 Explorer 重启后通过 `TaskbarCreated` 恢复。
- 检查 Windows 设置中的“任务栏角落溢出”是否隐藏了图标。
- 某些受限/自动化会话不允许托盘和注册表操作；请在正常登录的桌面中运行。

### 8. 双屏截错了屏幕

`Shift+F2` 截取鼠标指针所在显示器。先把鼠标移到游戏画面内再按快捷键。鼠标处于屏幕之间的空隙时，会选择最近的一块。

### 9. 能截到鼠标指针吗

不能。当前版本不包含鼠标指针。

### 10. 会触发杀毒软件或 SmartScreen 吗

- 程序未签名，首次从网络下载运行时可能触发 SmartScreen。
- 程序不联网、不注入进程、不写系统目录；安装目录和注册表项都在当前用户范围。
- 可以使用 Release 中的 `SHA256SUMS.txt` 校验下载文件，或从源码自行构建。

### 11. 支持 Windows 10 / ARM64 吗

- 面向 **Windows 11 x64** 开发与验证。
- Windows 10 21H2+ 理论上具备所需 API，但未做完整验证。
- 暂未提供 ARM64 构建。

### 12. 玩游戏会被封号吗

本工具不注入游戏、不 Hook 图形 API、不读写游戏内存、不联网，只使用 Windows 公开截图 API 和全局热键。但任何常驻后台程序在带反作弊的在线游戏中都存在被误判的可能，建议先在人机/单机模式验证，并自行评估竞技场景风险。

---

## 隐私与安全

- 不联网、不收集遥测、不上传截图。
- 截图只写入配置的保存目录；剪贴板内容由用户自行处理。
- 仅在选区截图期间临时安装键盘钩子，用于捕获 `Esc`、`Enter`、`Space`、`Ctrl+C`；遮罩关闭后立即卸载，非截图状态不监听键盘。
- 全局热键通过 `RegisterHotKey` 注册，不安装常驻键盘钩子。
- 以当前用户权限运行，不请求管理员权限；因此无法截取 UAC 安全桌面、锁屏或更高完整性级别的窗口。

---

## 已知限制

- 不提供标注、涂鸦、文字、马赛克、滚动截图、录屏、OCR、上传、自动更新或设置 GUI。
- 截图不包含鼠标指针。
- HDR 输出为 SDR PNG，不保留 16-bit HDR。
- WGC 在未打包 Win32 应用中无法可靠关闭黄色捕获边框；因此默认只对 HDR 使用。
- 暂未针对 ARM64 构建。
- 未签名 exe 在从网络下载后可能触发 SmartScreen；本地构建/安装不会带 Mark-of-the-Web。
- 当前开发环境只有一块 2560×1600 @150% 显示器；双屏、HDR 和重启自启需要在实际外接显示器上做最终人工验收。

---

## 项目结构

```text
.
├── .github/workflows/     CI 与 Release 工作流
├── assets/                app.ico、app.rc、app.manifest
├── scripts/
│   ├── build.ps1          测试 + Release 构建 + 复制到 dist
│   ├── package.ps1        生成发布 zip、fxshot.exe、SHA256SUMS.txt
│   ├── install.ps1        安装到用户目录、快捷方式、自启
│   ├── uninstall.ps1      卸载
│   ├── verify.ps1         GDI/WGC 截图验证
│   └── make-icon.ps1      生成多尺寸图标
├── src/
│   ├── main.rs            入口、CLI、DPI/COM 初始化
│   ├── app.rs             托盘、热键、单实例、菜单命令、快速截图
│   ├── config.rs          TOML 配置、快捷键解析
│   ├── autostart.rs       HKCU Run 读写与校准
│   ├── logging.rs         文件日志与轮转
│   ├── monitor.rs         显示器枚举、DPI、HDR、选屏
│   ├── win_util.rs        窗口枚举、DWM 边界、Known Folder、Shell 帮助
│   ├── naming.rs          PNG 命名、去重、编码
│   ├── diagnostics.rs     --capture-once / --set-autostart
│   ├── capture/           GDI、WGC、HDR、剪贴板、虚拟桌面合成
│   └── overlay/           选区遮罩、吸附、工具条、键盘钩子
├── Cargo.toml
├── Cargo.lock
├── config.example.toml
├── LICENSE
└── README.md
```

---

## 开发

### 环境

- Windows 11 x64
- Rust 1.82+（MSVC 工具链）
- Visual Studio C++ 生成工具 + Windows SDK
- PowerShell 7 或 Windows PowerShell 5.1（脚本）

### 常用命令

```powershell
# 格式化
cargo fmt --all

# 静态检查（CI 使用同样的严格级别）
cargo clippy --all-targets --locked -- -D warnings

# 单元测试
cargo test --all-targets --locked

# Release 构建
cargo build --release --locked
```

依赖版本固定并提交 `Cargo.lock`；`.cargo/config.toml` 为 MSVC 目标启用静态 CRT。

### 本地验证截图

`scripts\verify.ps1` 会分别用 GDI 和 WGC 执行 `--capture-once`，检查 PNG 是否存在、尺寸是否一致：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify.ps1
```

WGC 步骤可能短暂显示黄色捕获边框。该脚本需要**交互式桌面会话**，不能在无桌面的 CI Runner 中运行。

### 代码结构提示

- 截图后端在 `src/capture/`：GDI 负责静默路径，WGC 负责 HDR 和兼容模式，失败会自动回退并写日志。
- 选区 UI 在 `src/overlay/`：遮罩窗口、吸附、编辑手柄、工具条和键盘钩子相互独立。
- 所有坐标使用物理像素；涉及 DPI 的尺寸（如吸附距离、工具条）在显示前按显示器 DPI 缩放。

---

## CI/CD 与发布

### CI（`.github/workflows/ci.yml`）

触发条件：推送到 `main`、Pull Request、手动触发。

在 `windows-latest` 上依次执行：

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets --locked -- -D warnings`
3. `cargo test --all-targets --locked`
4. `cargo build --release --locked`
5. 上传 `fxshot.exe`、`config.example.toml`、`README.md` 作为工作流产物

### Release（`.github/workflows/release.yml`）

触发条件：推送 `v*` 标签，或在 Actions 中手动触发并填写标签。

工作流会：

1. 校验标签格式，并要求标签版本与 `Cargo.toml`、`assets/app.rc` 中的版本一致；
2. 运行全部单元测试；
3. 构建 Release；
4. 调用 `scripts/package.ps1` 生成发布 zip、独立 exe 和 `SHA256SUMS.txt`；
5. 上传工作流产物；
6. 创建 GitHub Release，自动生成变更说明，并附加上述文件。

### 发布新版本

1. 同步修改版本号：
   - `Cargo.toml` 的 `version`
   - `assets/app.rc` 的 `FILEVERSION` / `PRODUCTVERSION` / `FileVersion` / `ProductVersion`
2. 提交并推送：

   ```powershell
   git add -A
   git commit -m "release: v0.1.1"
   git push origin main
   ```

3. 打标签并推送，触发 Release 工作流：

   ```powershell
   git tag v0.1.1
   git push origin v0.1.1
   ```

4. 用 GitHub CLI 查看结果：

   ```powershell
   gh run list --workflow release.yml
   gh release view v0.1.1
   gh release download v0.1.1
   ```

也可以手动触发：

```powershell
gh workflow run release.yml -f tag=v0.1.1
```

### Release 产物

```text
fxshot.exe                          单文件绿色版
fxshot-<版本>-windows-x64.zip       发布包
SHA256SUMS.txt                      ZIP 与 EXE 的 SHA-256
```

发布包内的目录结构：

```text
fxshot-<版本>-windows-x64/
├── fxshot.exe
├── config.example.toml
├── README.md
├── LICENSE
├── VERSION.txt
└── scripts/
    ├── install.ps1
    └── uninstall.ps1
```

---

## 贡献

欢迎提交 Issue 和 Pull Request。提交前请确保：

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

全部通过。涉及 UI 或截图行为的改动，请同时说明测试所用的 Windows 版本、显示器配置（分辨率/缩放/HDR）和游戏显示模式。

---

## 许可证

[MIT](LICENSE)，Copyright (c) 2026 kamisato-funeki。

仅覆盖本工具自身代码；第三方依赖按其各自许可证分发（`anyhow`、`png`、`serde`、`toml`、`toml_edit`、`windows`、`embed-resource` 等）。
