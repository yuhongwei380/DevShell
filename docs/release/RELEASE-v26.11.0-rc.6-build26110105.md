# DevShell v26.11.0-rc.6 · 构建 2026110105

修复：应用图标白色外框背景去除——Windows exe / 安装器 / 任务栏图标与 macOS icns 此前均带白色底，
  现按原版 SVG 标志透明底渲染（resvg 重绘 png、16–256 共 7 档 ico 与 exe 内嵌资源同步再生），主图形与配色不变。
