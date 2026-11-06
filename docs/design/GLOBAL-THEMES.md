# 全局主题（Global Themes）设计记录

对应入口：设置 → 外观配置 → **全局主题**。持久化字段：`appearance.uiStyle`
（`classic`（默认）| `frosted` | `polar` | `twilight`），与 `theme`（light/dark/system）、
字体、ANSI 预设、背景选择、布局全部正交。

## 互联网案例与结论

- **Microsoft Mica**：材质不是壁纸——取桌面壁纸一次采样作为窗口底衬，内容分层
  叠加；低配硬件、省电、窗口非激活或不支持透明时**必须回退实色**。
  （learn.microsoft.com/windows/apps/design/style/mica）
- **MDN backdrop-filter**：2024 年起为 Baseline（Chromium 76 / Safari 9 /
  Firefox 103 / Edge 17）；需要半透明底衬；祖先带 `filter`、`opacity<1`、
  `mask/clip-path`、`mix-blend-mode` 或另一个 `backdrop-filter` 会形成
  backdrop root 限制取样范围；会话栏不施加模糊，也避免给固定定位的窗口
  控件建立包含块。
- **玻璃拟态通行做法**（glassmorphism）：低饱和底色 + 薄亮边 + 内高光 +
  克制的投影；文字不模糊、只模糊底衬。
- **Nord**：低饱和“极地夜/雪暴”十色体系，深浅两模式同一色族。
- **Tokyo Night**：深蓝紫夜色 + 蓝紫强调，另有官方日间变体。

## 四套样式

| 样式 | 浅色 | 深色 | 强调 | 模糊 |
| --- | --- | --- | --- | --- |
| 经典 | 现状不变 | 现状不变 | 现状 | 无 |
| 液态玻璃 | 经典配色 · 白玻璃 72% | 经典配色 · 暗玻璃 70% | 经典强调色 | 侧栏 26px、面板 22px、浮层 30px |
| 极地 | 雪 #f6f9fc | 极夜板 #343c4a | 冰蓝 #3f6d94 / #88c0d0 | 无 |
| 夜幕 | 日间 #eceef2 | 夜色板 #1f2133 | 蓝 #3763bd / #7aa2f7 | 无 |

- 材质只重绘 CSS 变量（`--surface/--surface-inset/--surface-hover/--surface-selected/
  --text*/--accent/--border/--shadow`）与少量组件规则（顶栏伪元素、连续侧栏、
  终端面板、浮层），不新增布局、不改控件几何；经典完全不受影响。
- **终端阅读底**：液态玻璃沿用经典配色——终端底即经典终端底加透明度（浅
  #223746/82%，深 #0f161d/80%），文字色与经典一致，自定义字体颜色优先；
  极地、夜幕用各自色板的深色终端底（极地 #2b323e/浅 #3b4351；
  夜幕 #16161e/浅 #2a2f4a），保证 ANSI 语义色可读。
- 液态玻璃区别于平铺磨砂：面板更透明（saturate 约 1.2，底层光斑色相取经典
  强调色同族的蓝，不引入经典之外的颜色），四团光斑缓慢漂移（`liquid-drift`，
  `prefers-reduced-motion` 下静止），面板叠加镜面光泽层（`--glass-sheen`）与
  边缘聚光内阴影（`--glass-edge`），配长弥散投影；顶部工具栏融入背景，
  监控面板从工具栏下方开始，Shell 与 SFTP 通过面板间距轻微分隔；监控信息、目录与文件列表融入各自面板，
  主要通过留白和标题区分内容，减少嵌套边框。
- 液态玻璃不使用背景图（保存的 `backgroundId` 原样保留，切回经典即恢复）；
  极地/夜幕照常使用背景。
- 回退：`@supports not (backdrop-filter)` 或 `prefers-reduced-transparency` →
  全部收敛到同色实底（--glass-solid / --glass-terminal-solid）；
  `forced-colors:active` → Canvas/CanvasText + 系统描边；原生窗口保持不透明，
  材质全部画在 WebViews 内（`WindowIsTranslucent:false`，桌面启动底色按样式+明暗选择）。

## 状态与兼容

- `uiStyle` 走外观补丁（`case "uiStyle"`），保存端枚举校验，旧配置缺字段时
  UnmarshalJSON 从 `defaultAppearance()` 回退 classic；boot 注入 `DEVSHELL.uiStyle`，
  `theme.js` 在样式加载前写 `html[data-ui-style]`，`setStyle()` 供运行时切换。
- 切换不重建终端、不中断 PTY，保留字体/字号/自定义颜色/ANSI/布局/连接状态；
  保存失败回滚并提示。
- 与「主题管理」（用户保存的整套外观快照）互不重叠：快照包含 `uiStyle`
  所在的外观字段但布局与缩放不入快照。

## 预览

`npm run preview:themes` 生成 `docs/design/global-themes-preview.html`：
复用产品标记与 CSS + 固定演示数据，无真实服务器、不写产品偏好。
参数：`?style=classic|frosted|polar|twilight`、`?mode=light|dark|system`、
`?blur=off`（预览实色回退）、`?panel=themes`（打开对话框）、
`?panel=settings-appearance`（外观配置子菜单）。
截图见 `docs/design/previews/`（Edge 无头）。

## 验证范围

- Go：`uiStyle` 补丁往返、四枚举校验、非法值整笔拒绝、空值回退 classic。
- 前端：`npm run check`；`test:theme` 增加 boot 样式、`setStyle` 白名单回退、
  setStyle 不直接持久化的断言。
- 视觉：Edge 无头截图四样式×明暗 + 对话框（`docs/design/previews/`）。
- 真机待办：macOS 原生窗口与 GPU 性能；液态玻璃在真实 WebView2 下的采样成本。
