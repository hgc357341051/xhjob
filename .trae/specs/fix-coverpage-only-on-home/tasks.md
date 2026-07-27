# Tasks

- [x] Task 1: 修改 `docs/assets/css/custom.css` 的 `section.cover` 规则
  - [x] SubTask 1.1: 将 `section.cover` 的 `display: flex` 改为 `display: none`（默认隐藏）
  - [x] SubTask 1.2: 将 `section.cover` 的 `position: relative` 改为 `position: fixed`，并补充 `top: 0; left: 0; width: 100%; height: 100%; z-index: 10;`
  - [x] SubTask 1.3: 新增 `section.cover.show { display: flex; }` 规则，仅当 docsify 添加 `show` 类时显示

- [x] Task 2: 修改 `docs/index.html` 的 `window.$docsify` 配置
  - [x] SubTask 2.1: 在 `coverpage: true` 之后新增 `onlyCover: true`

- [x] Task 3: 静态验证
  - [x] SubTask 3.1: 确认 `custom.css` 中 `section.cover` 默认 `display: none`，`section.cover.show` 为 `display: flex`
  - [x] SubTask 3.2: 确认 `index.html` 中 `onlyCover: true` 已添加
  - [x] SubTask 3.3: 确认 `_coverpage.md` 的 CTA 按钮使用 `#/quickstart` 等 hash 路由（无需修改，已验证）
  - [x] SubTask 3.4: 启动本地静态服务器，验证资源可访问（index.html / custom.css / _coverpage.md / quickstart.md 均 HTTP 200）

# Task Dependencies
- Task 1 和 Task 2 无依赖，可并行
- Task 3 依赖 Task 1 和 Task 2 完成
