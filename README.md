# Face Rank

一个基于 Tauri 2 的本地图片对比与排行工具。

项目的交互灵感来自电影《社交网络》中通过两两比较为人物建立排名的场景：每次展示两张图片，由用户选择更喜欢的一张，系统根据比较结果持续更新排名。评分模型计划使用 [OpenSkill](https://openskill.me/)，以便同时维护选手的评分和不确定性，比单一的 Elo 分数更适合冷启动和少量比较的场景。

> 本项目仅用于自愿、合法且尊重隐私的图片比较。请确保素材来源合法，并取得相关人员的授权；排行结果只代表当前比较数据下的模型结果，不代表对现实人物价值的判断。

## 项目状态

当前仓库处于早期开发阶段，已经完成 Tauri 2、React、TypeScript 和 Rust 的基础工程初始化。核心的图片管理、两两对比、OpenSkill 计算、历史记录和排行页面仍在开发中。

## 计划中的功能

- 导入本地图片并维护图片集合
- 通过两两对比收集偏好数据
- 使用 OpenSkill 更新评分、置信区间和排序
- 查看总榜、近期变化和比较次数
- 支持撤销最近一次比较和重新计算排行
- 数据默认保存在本机，提供导出与清空能力
- 在桌面端离线运行，不依赖在线服务

## 技术栈

- **桌面应用**：[Tauri 2](https://tauri.app/)
- **前端**：React 19、TypeScript、Vite
- **原生层**：Rust
- **评分模型**：[OpenSkill](https://openskill.me/)
- **构建工具**：Tauri CLI、Cargo

## 开始开发

### 环境要求

- Node.js LTS（建议使用当前维护中的 LTS 版本）
- Rust stable 与 Cargo
- Tauri 2 所需的系统依赖。请参考 [Tauri 环境准备文档](https://tauri.app/start/prerequisites/)

### 安装依赖

```bash
npm install
```

### 启动开发环境

```bash
npm run tauri dev
```

这会启动 Vite 开发服务器，并打开 Tauri 桌面窗口。

### 构建前端

```bash
npm run build
```

### 构建桌面安装包

```bash
npm run tauri build
```

构建产物会由 Tauri 输出到 `src-tauri/target/release/bundle/`。

## 使用流程（目标体验）

1. 创建一个排行项目并导入图片。
2. 在对比界面中选择两张图片里更符合个人偏好的一张。
3. 重复比较，OpenSkill 会在每次选择后更新评分和不确定性。
4. 在排行页查看当前结果，并根据需要继续比较或导出数据。

## 目录结构

```text
.
├── src/                  # React 前端
├── src-tauri/            # Tauri 配置与 Rust 原生层
├── public/               # 静态资源
├── index.html
├── package.json
└── vite.config.ts
```

## 开发约定

- UI 和交互逻辑放在 `src/`。
- 文件访问、持久化和评分计算等需要系统能力的逻辑放在 `src-tauri/`。
- 前后端之间通过 Tauri command 传递结构化数据。
- 提交功能时同时补充对应的类型、错误处理和测试。

## 许可证

项目许可证尚未确定，正式发布前会在此处补充。
