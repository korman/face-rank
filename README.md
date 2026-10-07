# Face Rank

Face Rank 是一个使用 Tauri 2 构建的本地照片两两评选工具。每轮展示两张照片，用户选择更偏好的一张，程序使用 OpenSkill 的 Plackett-Luce 模型持续更新评分，并按稳健分生成排行榜。

所有原图、缩略图、评分和比较记录都保存在本机，不会上传到网络。

> 请只使用来源合法且已获得必要授权的照片。排行榜反映的是当前用户的主观选择和有限样本下的模型估计，不代表对现实人物价值的判断。

## 第一版功能

- 使用系统目录选择器导入照片，并记住上次选择的目录
- 递归读取目录中的 PNG、JPG 和 JPEG，扩展名不区分大小写
- 自动应用 JPEG 的 EXIF 方向信息；跳过损坏、无法读取、无法解码或超过 256 MiB 安全限制的图片，不跟随符号链接
- 在系统应用缓存目录生成最长边 1280 px 的 JPEG 缩略图，前端不直接读取原图
- 通过 OpenSkill 0.0.1 默认 Plackett-Luce 模型更新 `mu` 和 `sigma`
- 优先安排比较次数少、尚未配对且预测胜率接近的照片
- 随机交换左右位置，支持选择、跳过和撤销最近一次比较
- 使用 SQLite 在本机保存照片、评分、比较事件和所选目录
- 排行按 `mu - 3 × sigma` 降序，显示评分、不确定度、比较次数、胜负和胜率
- 比较少于 5 次的照片会显示“样本少”标记

## 使用方法

1. 打开“评选”页，点击“选择文件夹”。
2. 选择包含照片的目录；程序会递归扫描其子目录。
3. 点击更偏好的照片，或使用键盘 `A` 选择左图、`D` 选择右图、空格键跳过。
4. 在“排行”页查看当前结果；需要更新文件列表时点击“重新扫描”。
5. 误选后可点击“撤销”，评分会根据剩余比较记录重新计算。

重新扫描同一路径时，已有照片通过规范化路径识别，原有评分会被保留。少于两张有效照片时无法开始评选。

## 窗口与界面

- 桌面窗口默认以最大化状态启动，保留系统标题栏和窗口控制按钮
- 宽屏桌面会同步放大内容区、标题、按钮、导航、图片和排行榜表格，充分利用可用空间
- 较小窗口会自动切换为紧凑布局，评选图片始终保持完整显示
- 当前不提供点击图片后的单图放大预览；评选页会直接在当前窗口内展示两张完整缩略图

## 本地数据

- SQLite 数据库：系统应用数据目录下的 `face-rank.sqlite`
- 缩略图：系统应用缓存目录下的 `face-rank/thumbnails/`
- 原图：始终保留在用户选择的目录中，程序不会移动或修改原图

仓库根目录的 `resources/` 用于本地测试素材，已加入 `.gitignore`。程序所需的 `public/`、`src/assets/` 和 Tauri 图标仍由 Git 跟踪。

## 开发环境

- Node.js LTS
- Rust stable 与 Cargo
- 对应平台的 [Tauri 2 系统依赖](https://tauri.app/start/prerequisites/)

安装依赖：

```bash
npm install
```

启动桌面开发环境：

```bash
npm run tauri dev
```

构建前端：

```bash
npm run build
```

运行 Rust 测试：

```bash
cd src-tauri
cargo test --all-targets
```

构建桌面安装包：

```bash
npm run tauri build
```

## 技术栈

- Tauri 2
- React 19、TypeScript、Vite
- Rust、SQLite（rusqlite）
- OpenSkill 0.0.1
- image、walkdir

## 项目结构

```text
.
├── src/                  # React 界面与前端交互
├── src-tauri/src/        # 扫描、配对、评分、数据库与 Tauri commands
├── src-tauri/capabilities/
├── public/               # 前端静态资源
├── resources/            # 本地测试照片，不提交到 Git
├── package.json
└── README.md
```

## 当前范围

第一版面向单用户、本地单库使用，暂不包含账号、云同步、多人比较、平局、导出或排名趋势图。

## 许可证

项目许可证尚未确定。
