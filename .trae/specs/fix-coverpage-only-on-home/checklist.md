# Checklist

## custom.css 封面规则
- [x] `section.cover` 的 `display` 改为 `none`（默认隐藏）
- [x] `section.cover` 的 `position` 改为 `fixed`（覆盖层定位，不占文档流）
- [x] `section.cover` 补充 `top: 0; left: 0; width: 100%; height: 100%; z-index: 10;`
- [x] 新增 `section.cover.show { display: flex; }` 规则
- [x] 封面其它样式（background / min-height / align-items / justify-content / padding / overflow）保持不变
- [x] `.cover-main` 及其子元素样式不受影响

## index.html 配置
- [x] `window.$docsify` 新增 `onlyCover: true`
- [x] `onlyCover: true` 位于 `coverpage: true` 之后
- [x] 其它配置项不变

## 验证
- [x] 主页（`#/`）显示封面，不显示 README 正文（`onlyCover: true` + `section.cover.show { display: flex; }`）
- [x] 点击侧边栏栏目（如"快速开始"）后，封面完全隐藏，正文直接可见，无需滚动（`section.cover` 默认 `display: none`，docsify 移除 `show` 类）
- [x] 直接访问非主页路由（如 `#/api-functions`）不显示封面（同上机制）
- [x] 从文档页面点击 logo / 站点名返回主页时，封面重新显示（docsify 重新添加 `show` 类）
- [x] 主题切换按钮、移动端汉堡菜单在封面可见时仍可点击（z-index 50 高于封面 z-index 10）
- [x] `_coverpage.md` 的 CTA 按钮（"🚀 快速开始"等）hash 路由正确（`href="#/quickstart"` / `href="#/api-functions"`）
