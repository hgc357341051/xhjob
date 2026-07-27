# 修复封面页仅在主页显示 Spec

> change-id: `fix-coverpage-only-on-home`
> 前序：`docsify-static-site`（已完成的文档站点搭建）

## Why

当前 docs 文档站点的封面页（`_coverpage.md` 渲染的 `section.cover`）在**所有页面导航时都可见**，而非仅在主页显示。用户点击侧边栏任意栏目后，会先看到占据整个视口（100vh）的封面页，必须向下滚动才能看到实际文档内容。

**根因**：`docs/assets/css/custom.css` 第 480 行对 `section.cover` 无条件设置了 `display: flex`，覆盖了 docsify 主题（vue.css）默认的 show/hide 机制。docsify 通过在 `<section class="cover">` 上添加/移除 `show` 类来控制封面显示——主页路由 `#/` 时加 `show`，其它路由移除 `show`。但自定义 CSS 的 `display: flex !important` 风格（虽未加 `!important` 但因选择器特异性覆盖了默认规则）导致封面始终以 flex 布局渲染，且 `position: relative` 使其处于文档流中，把正文挤到 100vh 以下。

## What Changes

### MODIFIED: `section.cover` 的显示/隐藏逻辑

**`docs/assets/css/custom.css`** 第 476-487 行：

将 `section.cover` 的无条件 `display: flex` 改为默认隐藏（`display: none`），并通过 `section.cover.show` 仅在 docsify 添加 `show` 类时显示（`display: flex`）。同时将 `position: relative` 改回 docsify 默认的 `position: fixed`，使封面以覆盖层形式渲染而非占据文档流空间。

**修改前**：
```css
section.cover {
  background: linear-gradient(135deg, ...) !important;
  min-height: 100vh;
  display: flex;              /* 始终 flex，导致所有页面都显示 */
  align-items: center;
  justify-content: center;
  text-align: center;
  padding: 40px 24px;
  position: relative;          /* 在文档流中，挤占正文空间 */
  overflow: hidden;
}
```

**修改后**：
```css
section.cover {
  background: linear-gradient(135deg, ...) !important;
  min-height: 100vh;
  display: none;               /* 默认隐藏，由 docsify 的 show 类控制 */
  align-items: center;
  justify-content: center;
  text-align: center;
  padding: 40px 24px;
  position: fixed;             /* 覆盖层定位，不挤占正文空间 */
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  z-index: 10;
  overflow: hidden;
}

/* 仅在主页路由时 docsify 添加 show 类，此时显示封面 */
section.cover.show {
  display: flex;
}
```

### ADDED: `onlyCover` 配置

**`docs/index.html`** 的 `window.$docsify` 配置新增 `onlyCover: true`：

```javascript
window.$docsify = {
  name: 'Xhjob',
  logo: 'assets/img/logo.svg',
  repo: 'https://github.com/hgc357341051/xhjob',
  themeColor: '#3B82F6',
  loadSidebar: true,
  coverpage: true,
  onlyCover: true,             // 新增：主页仅显示封面，隐藏 README 正文
  auto2top: true,
  subMaxLevel: 3,
  maxLevel: 4,
  ...
};
```

`onlyCover: true` 让主页（`#/`）**只显示封面**，不渲染 `README.md` 的正文内容。用户通过封面上的 CTA 按钮（"快速开始"、"API 参考"、"GitHub"）进入具体文档页面。这样主页更简洁，且避免封面下方还有一段 README 内容需要滚动。

### MODIFIED: 封面 CTA 按钮确保导航到具体页面

**`docs/_coverpage.md`** 的 CTA 按钮已使用 `href="#/quickstart"` 等 hash 路由，无需修改。仅确认点击后导航到 `#/quickstart` 等非主页路由，此时 docsify 移除 `show` 类，封面隐藏，正文直接显示。

## Impact

- **Affected specs**：`docsify-static-site`（前序，已完成的站点搭建）。本 spec 仅修复封面显示 bug，不改变站点结构。
- **Affected code**：
  - `docs/assets/css/custom.css`：修改 `section.cover` 的 `display` / `position` 规则，新增 `section.cover.show` 规则
  - `docs/index.html`：`window.$docsify` 新增 `onlyCover: true`
- **BREAKING**：无。用户在主页仍能看到封面，点击 CTA 或侧边栏进入文档页面时封面正确隐藏。行为符合 docsify 标准模式。

## ADDED Requirements

### Requirement: 封面页仅在主页路由显示
`section.cover` 元素 SHALL 默认隐藏（`display: none`），仅当 docsify 在主页路由（`#/`）为其添加 `show` 类时才显示（`display: flex`）。在其它任何路由（如 `#/quickstart`、`#/api-functions`），封面 SHALL 完全隐藏，不占据文档流空间。

#### Scenario: 首次打开主页
- **GIVEN** 用户首次访问文档站点（URL 为 `/` 或 `/#/`）
- **WHEN** 页面加载完成
- **THEN** 封面页可见，显示 logo / 标题 / 徽章 / CTA 按钮
- **AND** 封面占据整个视口（100vh）
- **AND** 主页不显示 README.md 正文内容（`onlyCover: true`）

#### Scenario: 从主页导航到文档页面
- **GIVEN** 用户在主页（`#/`），封面可见
- **WHEN** 用户点击侧边栏"快速开始"或封面 CTA 按钮"🚀 快速开始"
- **THEN** URL 变为 `#/quickstart`
- **AND** 封面页完全隐藏（`display: none`），不占据任何空间
- **AND** "快速开始"文档内容直接显示在视口顶部，无需向下滚动

#### Scenario: 直接访问非主页路由
- **GIVEN** 用户直接访问 `/#/api-functions`
- **WHEN** 页面加载完成
- **THEN** 封面页不显示
- **AND** API 函数参考文档内容直接可见

#### Scenario: 从文档页面返回主页
- **GIVEN** 用户在 `#/quickstart` 页面
- **WHEN** 用户点击侧边栏的站点名"Xhjob"或 logo（链接到 `#/`）
- **THEN** URL 变为 `#/`
- **AND** 封面页重新显示

### Requirement: 封面以覆盖层定位
`section.cover` SHALL 使用 `position: fixed` 定位，以覆盖层形式渲染而非占据文档流空间。封面 `z-index` SHALL 高于正文内容但低于侧边栏和导航控件，确保封面可见时不被遮挡，隐藏时不影响正文布局。

#### Scenario: 封面可见时不遮挡侧边栏
- **GIVEN** 用户在主页，封面可见
- **WHEN** 查看页面布局
- **THEN** 封面覆盖主内容区
- **AND** 侧边栏仍可在封面左侧可见（或被封面覆盖，取决于 z-index 层级）
- **AND** 主题切换按钮、移动端汉堡菜单按钮仍可点击（z-index 高于封面）

## MODIFIED Requirements

### Requirement: 封面页 CSS 规则
`docs/assets/css/custom.css` 中 `section.cover` 的样式规则 SHALL 遵循 docsify 的 show/hide 协议：默认 `display: none`，`.show` 类触发 `display: flex`。`position` SHALL 为 `fixed`（docsify 默认）而非 `relative`，避免封面在文档流中挤占正文空间。

## REMOVED Requirements

无。
