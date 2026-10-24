/* Bloom std::ui runtime — a small, safe component tree and design system.
 *
 * Architecture:
 *   - A UiNode is a managed handle on an internal tree of elements and text.
 *     There is no public raw-HTML entry: every text is escaped on render,
 *     every attribute value is escaped and validated, and every URL goes
 *     through an allowlist (http, https, root-relative, relative path,
 *     fragment). Tags and attribute names are only created by the runtime,
 *     never from user input.
 *   - Components append children with `ui::append`; rendering walks the tree
 *     iteratively bounded (depth <= 64, <= 20000 nodes, output <= 8 MiB).
 *   - The stylesheet is embedded (version string at the top), served through
 *     `ui::stylesheet()` or `ui::send_stylesheet(conn)`.
 *   - Themes are an attribute on <html> and pure CSS: light, dark, system
 *     (`prefers-color-scheme`). No JavaScript is required.
 */

#define BLOOM_UI_MAX_DEPTH 64
#define BLOOM_UI_MAX_NODES 20000
#define BLOOM_UI_MAX_OUTPUT (8 * 1024 * 1024)
#define BLOOM_UI_MAX_ATTRS 24
#define BLOOM_UI_MAX_ATTR_VALUE 4096
#define BLOOM_UI_MAX_TEXT (1024 * 1024)
#define BLOOM_UI_MAX_URL 2048
#define BLOOM_UI_MAX_CHILDREN 4096
#define BLOOM_UI_MAX_ID 128
#define BLOOM_UI_MAX_CLASS 512

static const char BLOOM_UI_CSS[] =
    "/* Bloom UI 1 - default design system. Generated from the Bloom std::ui runtime. */\n"
    ":root {\n"
    "  --ui-font-sans: system-ui, -apple-system, \"Segoe UI\", Roboto, \"Helvetica Neue\", Arial, sans-serif;\n"
    "  --ui-font-mono: ui-monospace, SFMono-Regular, Consolas, \"Liberation Mono\", monospace;\n"
    "  --ui-bg: #f6f7f9;\n"
    "  --ui-surface: #ffffff;\n"
    "  --ui-surface-2: #eef1f5;\n"
    "  --ui-text: #17202b;\n"
    "  --ui-muted: #5a6675;\n"
    "  --ui-border: #d7dde5;\n"
    "  --ui-primary: #2456d6;\n"
    "  --ui-primary-contrast: #ffffff;\n"
    "  --ui-secondary: #4a5666;\n"
    "  --ui-success: #1c7a4a;\n"
    "  --ui-warning: #8a5a00;\n"
    "  --ui-danger: #b3261e;\n"
    "  --ui-info: #2456d6;\n"
    "  --ui-success-bg: #e6f4ec;\n"
    "  --ui-warning-bg: #fdf3dc;\n"
    "  --ui-danger-bg: #fdeceb;\n"
    "  --ui-info-bg: #e9eefc;\n"
    "  --ui-radius-sm: 4px;\n"
    "  --ui-radius: 8px;\n"
    "  --ui-radius-lg: 14px;\n"
    "  --ui-space-1: 4px;\n"
    "  --ui-space-2: 8px;\n"
    "  --ui-space-3: 12px;\n"
    "  --ui-space-4: 16px;\n"
    "  --ui-space-5: 24px;\n"
    "  --ui-space-6: 32px;\n"
    "  --ui-shadow-sm: 0 1px 2px rgba(16, 24, 40, 0.06);\n"
    "  --ui-shadow: 0 2px 8px rgba(16, 24, 40, 0.08);\n"
    "  --ui-focus: 0 0 0 3px rgba(36, 86, 214, 0.35);\n"
    "  --ui-max-width: 72rem;\n"
    "  --ui-line-height: 1.55;\n"
    "  --ui-font-size-sm: 0.875rem;\n"
    "  --ui-font-size: 1rem;\n"
    "  --ui-font-size-lg: 1.125rem;\n"
    "  --ui-transition: 130ms ease;\n"
    "}\n"
    "\n"
    "[data-theme=\"dark\"] {\n"
    "  --ui-bg: #0f141a;\n"
    "  --ui-surface: #171d25;\n"
    "  --ui-surface-2: #202833;\n"
    "  --ui-text: #e6ebf1;\n"
    "  --ui-muted: #9aa7b6;\n"
    "  --ui-border: #2b3542;\n"
    "  --ui-primary: #7da2ff;\n"
    "  --ui-primary-contrast: #0b1220;\n"
    "  --ui-secondary: #a5b1c0;\n"
    "  --ui-success: #5fd39b;\n"
    "  --ui-warning: #e5b75c;\n"
    "  --ui-danger: #f28b84;\n"
    "  --ui-info: #7da2ff;\n"
    "  --ui-success-bg: #123024;\n"
    "  --ui-warning-bg: #2f2611;\n"
    "  --ui-danger-bg: #331a19;\n"
    "  --ui-info-bg: #15203a;\n"
    "  --ui-shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.4);\n"
    "  --ui-shadow: 0 2px 10px rgba(0, 0, 0, 0.45);\n"
    "  --ui-focus: 0 0 0 3px rgba(125, 162, 255, 0.45);\n"
    "}\n"
    "\n"
    "@media (prefers-color-scheme: dark) {\n"
    "  [data-theme=\"system\"] {\n"
    "    --ui-bg: #0f141a;\n"
    "    --ui-surface: #171d25;\n"
    "    --ui-surface-2: #202833;\n"
    "    --ui-text: #e6ebf1;\n"
    "    --ui-muted: #9aa7b6;\n"
    "    --ui-border: #2b3542;\n"
    "    --ui-primary: #7da2ff;\n"
    "    --ui-primary-contrast: #0b1220;\n"
    "    --ui-secondary: #a5b1c0;\n"
    "    --ui-success: #5fd39b;\n"
    "    --ui-warning: #e5b75c;\n"
    "    --ui-danger: #f28b84;\n"
    "    --ui-info: #7da2ff;\n"
    "    --ui-success-bg: #123024;\n"
    "    --ui-warning-bg: #2f2611;\n"
    "    --ui-danger-bg: #331a19;\n"
    "    --ui-info-bg: #15203a;\n"
    "    --ui-shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.4);\n"
    "    --ui-shadow: 0 2px 10px rgba(0, 0, 0, 0.45);\n"
    "    --ui-focus: 0 0 0 3px rgba(125, 162, 255, 0.45);\n"
    "  }\n"
    "}\n"
    "\n"
    "*,\n"
    "*::before,\n"
    "*::after {\n"
    "  box-sizing: border-box;\n"
    "}\n"
    "\n"
    "html {\n"
    "  -webkit-text-size-adjust: 100%;\n"
    "}\n"
    "\n"
    "body {\n"
    "  margin: 0;\n"
    "  background: var(--ui-bg);\n"
    "  color: var(--ui-text);\n"
    "  font-family: var(--ui-font-sans);\n"
    "  font-size: var(--ui-font-size);\n"
    "  line-height: var(--ui-line-height);\n"
    "}\n"
    "\n"
    "h1,\n"
    "h2,\n"
    "h3,\n"
    "h4,\n"
    "h5,\n"
    "h6,\n"
    ".ui-heading {\n"
    "  margin: 0 0 var(--ui-space-3);\n"
    "  line-height: 1.25;\n"
    "  font-weight: 650;\n"
    "  letter-spacing: -0.01em;\n"
    "}\n"
    "\n"
    ".ui-heading[data-level=\"1\"] { font-size: 2rem; }\n"
    ".ui-heading[data-level=\"2\"] { font-size: 1.5rem; }\n"
    ".ui-heading[data-level=\"3\"] { font-size: 1.25rem; }\n"
    ".ui-heading[data-level=\"4\"] { font-size: 1.0625rem; }\n"
    ".ui-heading[data-level=\"5\"],\n"
    ".ui-heading[data-level=\"6\"] { font-size: 1rem; text-transform: uppercase; letter-spacing: 0.04em; }\n"
    "\n"
    "p {\n"
    "  margin: 0 0 var(--ui-space-3);\n"
    "}\n"
    "\n"
    ".ui-text { color: var(--ui-text); }\n"
    ".ui-muted { color: var(--ui-muted); }\n"
    "\n"
    "a,\n"
    ".ui-link {\n"
    "  color: var(--ui-primary);\n"
    "  text-decoration: none;\n"
    "  text-underline-offset: 2px;\n"
    "}\n"
    "\n"
    "a:hover,\n"
    ".ui-link:hover {\n"
    "  text-decoration: underline;\n"
    "}\n"
    "\n"
    ".ui-container {\n"
    "  width: 100%;\n"
    "  max-width: var(--ui-max-width);\n"
    "  margin-inline: auto;\n"
    "  padding-inline: var(--ui-space-4);\n"
    "}\n"
    "\n"
    ".ui-container[data-width=\"sm\"] { max-width: 36rem; }\n"
    ".ui-container[data-width=\"md\"] { max-width: 52rem; }\n"
    ".ui-container[data-width=\"lg\"] { max-width: var(--ui-max-width); }\n"
    ".ui-container[data-width=\"full\"] { max-width: none; }\n"
    "\n"
    ".ui-stack { display: flex; flex-direction: column; gap: var(--ui-space-3); }\n"
    ".ui-row { display: flex; flex-direction: row; flex-wrap: wrap; align-items: center; gap: var(--ui-space-3); }\n"
    ".ui-row[data-wrap=\"false\"] { flex-wrap: nowrap; }\n"
    ".ui-stack[data-gap=\"xs\"],\n"
    ".ui-row[data-gap=\"xs\"] { gap: var(--ui-space-1); }\n"
    ".ui-stack[data-gap=\"sm\"],\n"
    ".ui-row[data-gap=\"sm\"] { gap: var(--ui-space-2); }\n"
    ".ui-stack[data-gap=\"md\"],\n"
    ".ui-row[data-gap=\"md\"] { gap: var(--ui-space-3); }\n"
    ".ui-stack[data-gap=\"lg\"],\n"
    ".ui-row[data-gap=\"lg\"] { gap: var(--ui-space-5); }\n"
    ".ui-stack[data-gap=\"xl\"],\n"
    ".ui-row[data-gap=\"xl\"] { gap: var(--ui-space-6); }\n"
    "\n"
    ".ui-grid {\n"
    "  display: grid;\n"
    "  grid-template-columns: repeat(1, minmax(0, 1fr));\n"
    "  gap: var(--ui-space-3);\n"
    "}\n"
    "\n"
    ".ui-grid[data-columns=\"2\"] { grid-template-columns: repeat(2, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"3\"] { grid-template-columns: repeat(3, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"4\"] { grid-template-columns: repeat(4, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"5\"] { grid-template-columns: repeat(5, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"6\"] { grid-template-columns: repeat(6, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"7\"] { grid-template-columns: repeat(7, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"8\"] { grid-template-columns: repeat(8, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"9\"] { grid-template-columns: repeat(9, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"10\"] { grid-template-columns: repeat(10, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"11\"] { grid-template-columns: repeat(11, minmax(0, 1fr)); }\n"
    ".ui-grid[data-columns=\"12\"] { grid-template-columns: repeat(12, minmax(0, 1fr)); }\n"
    "\n"
    ".ui-section { margin-block: var(--ui-space-6); }\n"
    "\n"
    ".ui-divider {\n"
    "  border: 0;\n"
    "  border-top: 1px solid var(--ui-border);\n"
    "  margin: var(--ui-space-4) 0;\n"
    "}\n"
    "\n"
    ".ui-badge {\n"
    "  display: inline-flex;\n"
    "  align-items: center;\n"
    "  gap: var(--ui-space-1);\n"
    "  padding: 2px 10px;\n"
    "  border-radius: 999px;\n"
    "  border: 1px solid var(--ui-border);\n"
    "  background: var(--ui-surface-2);\n"
    "  color: var(--ui-text);\n"
    "  font-size: var(--ui-font-size-sm);\n"
    "  white-space: nowrap;\n"
    "}\n"
    "\n"
    ".ui-badge[data-variant=\"success\"] { background: var(--ui-success-bg); color: var(--ui-success); border-color: transparent; }\n"
    ".ui-badge[data-variant=\"warning\"] { background: var(--ui-warning-bg); color: var(--ui-warning); border-color: transparent; }\n"
    ".ui-badge[data-variant=\"danger\"] { background: var(--ui-danger-bg); color: var(--ui-danger); border-color: transparent; }\n"
    ".ui-badge[data-variant=\"info\"] { background: var(--ui-info-bg); color: var(--ui-info); border-color: transparent; }\n"
    "\n"
    ".ui-card {\n"
    "  background: var(--ui-surface);\n"
    "  border: 1px solid var(--ui-border);\n"
    "  border-radius: var(--ui-radius);\n"
    "  padding: var(--ui-space-5);\n"
    "  box-shadow: var(--ui-shadow-sm);\n"
    "}\n"
    "\n"
    ".ui-card > h3 { margin-bottom: var(--ui-space-2); }\n"
    "\n"
    ".ui-alert {\n"
    "  display: block;\n"
    "  padding: var(--ui-space-3) var(--ui-space-4);\n"
    "  border-radius: var(--ui-radius);\n"
    "  border: 1px solid var(--ui-border);\n"
    "  background: var(--ui-surface-2);\n"
    "  margin-bottom: var(--ui-space-3);\n"
    "}\n"
    "\n"
    ".ui-alert[data-variant=\"success\"] { background: var(--ui-success-bg); border-color: transparent; color: var(--ui-success); }\n"
    ".ui-alert[data-variant=\"warning\"] { background: var(--ui-warning-bg); border-color: transparent; color: var(--ui-warning); }\n"
    ".ui-alert[data-variant=\"danger\"] { background: var(--ui-danger-bg); border-color: transparent; color: var(--ui-danger); }\n"
    ".ui-alert[data-variant=\"info\"] { background: var(--ui-info-bg); border-color: transparent; color: var(--ui-info); }\n"
    "\n"
    ".ui-btn {\n"
    "  display: inline-flex;\n"
    "  align-items: center;\n"
    "  justify-content: center;\n"
    "  gap: var(--ui-space-2);\n"
    "  min-height: 2.5rem;\n"
    "  padding: 0 var(--ui-space-4);\n"
    "  border-radius: var(--ui-radius);\n"
    "  border: 1px solid var(--ui-border);\n"
    "  background: var(--ui-surface);\n"
    "  color: var(--ui-text);\n"
    "  font: inherit;\n"
    "  font-weight: 600;\n"
    "  line-height: 1;\n"
    "  cursor: pointer;\n"
    "  transition: background var(--ui-transition), border-color var(--ui-transition), color var(--ui-transition);\n"
    "}\n"
    "\n"
    ".ui-btn:hover { background: var(--ui-surface-2); }\n"
    "\n"
    ".ui-btn[data-variant=\"primary\"] {\n"
    "  background: var(--ui-primary);\n"
    "  border-color: var(--ui-primary);\n"
    "  color: var(--ui-primary-contrast);\n"
    "}\n"
    "\n"
    ".ui-btn[data-variant=\"primary\"]:hover { filter: brightness(1.06); }\n"
    "\n"
    ".ui-btn[data-variant=\"secondary\"] {\n"
    "  background: var(--ui-surface-2);\n"
    "  border-color: transparent;\n"
    "  color: var(--ui-text);\n"
    "}\n"
    "\n"
    ".ui-btn[data-variant=\"ghost\"] {\n"
    "  background: transparent;\n"
    "  border-color: transparent;\n"
    "  color: var(--ui-primary);\n"
    "}\n"
    "\n"
    ".ui-btn[data-variant=\"danger\"] {\n"
    "  background: var(--ui-danger);\n"
    "  border-color: var(--ui-danger);\n"
    "  color: #fff;\n"
    "}\n"
    "\n"
    ".ui-btn[data-size=\"sm\"] { min-height: 2rem; padding: 0 var(--ui-space-3); font-size: var(--ui-font-size-sm); }\n"
    ".ui-btn[data-size=\"lg\"] { min-height: 2.875rem; padding: 0 var(--ui-space-5); font-size: var(--ui-font-size-lg); }\n"
    "\n"
    ".ui-btn:disabled,\n"
    ".ui-btn[disabled] {\n"
    "  opacity: 0.55;\n"
    "  cursor: not-allowed;\n"
    "  filter: none;\n"
    "}\n"
    "\n"
    ".ui-form { display: grid; gap: var(--ui-space-4); max-width: 36rem; }\n"
    "\n"
    ".ui-field { display: grid; gap: var(--ui-space-2); }\n"
    "\n"
    ".ui-label {\n"
    "  font-weight: 600;\n"
    "  font-size: var(--ui-font-size-sm);\n"
    "  color: var(--ui-text);\n"
    "}\n"
    "\n"
    ".ui-label[data-required=\"true\"]::after {\n"
    "  content: \" *\";\n"
    "  color: var(--ui-danger);\n"
    "}\n"
    "\n"
    ".ui-input,\n"
    ".ui-textarea,\n"
    ".ui-select {\n"
    "  width: 100%;\n"
    "  min-height: 2.5rem;\n"
    "  padding: var(--ui-space-2) var(--ui-space-3);\n"
    "  border: 1px solid var(--ui-border);\n"
    "  border-radius: var(--ui-radius-sm);\n"
    "  background: var(--ui-surface);\n"
    "  color: var(--ui-text);\n"
    "  font: inherit;\n"
    "  transition: border-color var(--ui-transition), box-shadow var(--ui-transition);\n"
    "}\n"
    "\n"
    ".ui-textarea { min-height: 6rem; resize: vertical; }\n"
    "\n"
    ".ui-input:focus,\n"
    ".ui-textarea:focus,\n"
    ".ui-select:focus {\n"
    "  outline: none;\n"
    "  border-color: var(--ui-primary);\n"
    "  box-shadow: var(--ui-focus);\n"
    "}\n"
    "\n"
    ".ui-input[aria-invalid=\"true\"],\n"
    ".ui-textarea[aria-invalid=\"true\"],\n"
    ".ui-select[aria-invalid=\"true\"] { border-color: var(--ui-danger); }\n"
    "\n"
    ".ui-input:disabled,\n"
    ".ui-textarea:disabled,\n"
    ".ui-select:disabled { background: var(--ui-surface-2); opacity: 0.7; cursor: not-allowed; }\n"
    "\n"
    ".ui-check { display: inline-flex; align-items: center; gap: var(--ui-space-2); font-size: var(--ui-font-size-sm); }\n"
    ".ui-check input { inline-size: 1.05rem; block-size: 1.05rem; accent-color: var(--ui-primary); }\n"
    "\n"
    ".ui-help { color: var(--ui-muted); font-size: var(--ui-font-size-sm); }\n"
    "\n"
    ".ui-error {\n"
    "  color: var(--ui-danger);\n"
    "  font-size: var(--ui-font-size-sm);\n"
    "  font-weight: 600;\n"
    "}\n"
    "\n"
    ".ui-table-wrap { width: 100%; overflow-x: auto; border: 1px solid var(--ui-border); border-radius: var(--ui-radius); background: var(--ui-surface); }\n"
    "\n"
    ".ui-table { width: 100%; border-collapse: collapse; font-size: var(--ui-font-size-sm); }\n"
    ".ui-table caption { text-align: left; padding: var(--ui-space-3) var(--ui-space-4); color: var(--ui-muted); }\n"
    ".ui-table th,\n"
    ".ui-table td { padding: var(--ui-space-2) var(--ui-space-3); border-bottom: 1px solid var(--ui-border); text-align: left; vertical-align: top; }\n"
    ".ui-table th { background: var(--ui-surface-2); font-weight: 650; white-space: nowrap; }\n"
    ".ui-table tbody tr:last-child td { border-bottom: 0; }\n"
    "\n"
    ".ui-nav { display: flex; flex-wrap: wrap; align-items: center; gap: var(--ui-space-2); margin-bottom: var(--ui-space-4); }\n"
    "\n"
    ".ui-nav-link {\n"
    "  padding: var(--ui-space-2) var(--ui-space-3);\n"
    "  border-radius: var(--ui-radius-sm);\n"
    "  color: var(--ui-secondary);\n"
    "  font-weight: 600;\n"
    "  font-size: var(--ui-font-size-sm);\n"
    "}\n"
    "\n"
    ".ui-nav-link:hover { background: var(--ui-surface-2); text-decoration: none; }\n"
    ".ui-nav-link[aria-current=\"page\"] { color: var(--ui-primary); background: var(--ui-info-bg); }\n"
    "\n"
    ".ui-breadcrumbs { display: flex; flex-wrap: wrap; gap: var(--ui-space-2); font-size: var(--ui-font-size-sm); color: var(--ui-muted); margin-bottom: var(--ui-space-3); }\n"
    ".ui-crumb { color: var(--ui-muted); }\n"
    ".ui-crumb[aria-current=\"page\"] { color: var(--ui-text); font-weight: 600; }\n"
    ".ui-crumb + .ui-crumb::before { content: \"/\"; margin-right: var(--ui-space-2); color: var(--ui-border); }\n"
    "\n"
    ".ui-empty {\n"
    "  padding: var(--ui-space-6);\n"
    "  text-align: center;\n"
    "  border: 1px dashed var(--ui-border);\n"
    "  border-radius: var(--ui-radius);\n"
    "  color: var(--ui-muted);\n"
    "  background: var(--ui-surface);\n"
    "}\n"
    "\n"
    ".ui-avatar { border-radius: 999px; object-fit: cover; background: var(--ui-surface-2); }\n"
    ".ui-avatar[data-size=\"sm\"] { inline-size: 1.75rem; block-size: 1.75rem; }\n"
    ".ui-avatar[data-size=\"md\"] { inline-size: 2.5rem; block-size: 2.5rem; }\n"
    ".ui-avatar[data-size=\"lg\"] { inline-size: 3.5rem; block-size: 3.5rem; }\n"
    "\n"
    ".ui-progress { width: 100%; block-size: 0.6rem; accent-color: var(--ui-primary); }\n"
    "\n"
    ".ui-skeleton { display: grid; gap: var(--ui-space-2); }\n"
    ".ui-skeleton span {\n"
    "  display: block;\n"
    "  block-size: 0.75rem;\n"
    "  border-radius: var(--ui-radius-sm);\n"
    "  background: var(--ui-surface-2);\n"
    "  animation: ui-skeleton-pulse 1.4s ease-in-out infinite;\n"
    "}\n"
    ".ui-skeleton span:last-child { inline-size: 60%; }\n"
    "\n"
    "@keyframes ui-skeleton-pulse {\n"
    "  0%, 100% { opacity: 1; }\n"
    "  50% { opacity: 0.55; }\n"
    "}\n"
    "\n"
    ".ui-pagination { display: flex; flex-wrap: wrap; align-items: center; gap: var(--ui-space-1); margin-block: var(--ui-space-4); }\n"
    ".ui-page {\n"
    "  min-inline-size: 2.25rem;\n"
    "  padding: var(--ui-space-1) var(--ui-space-2);\n"
    "  border: 1px solid var(--ui-border);\n"
    "  border-radius: var(--ui-radius-sm);\n"
    "  text-align: center;\n"
    "  font-size: var(--ui-font-size-sm);\n"
    "}\n"
    ".ui-page[aria-current=\"page\"] { background: var(--ui-primary); border-color: var(--ui-primary); color: var(--ui-primary-contrast); font-weight: 650; }\n"
    "\n"
    ".ui-page:focus-visible,\n"
    ".ui-btn:focus-visible,\n"
    ".ui-nav-link:focus-visible,\n"
    ".ui-link:focus-visible,\n"
    ".ui-crumb:focus-visible,\n"
    "a:focus-visible {\n"
    "  outline: none;\n"
    "  box-shadow: var(--ui-focus);\n"
    "  border-radius: var(--ui-radius-sm);\n"
    "}\n"
    "\n"
    "@media (max-width: 720px) {\n"
    "  .ui-grid[data-columns] { grid-template-columns: repeat(1, minmax(0, 1fr)); }\n"
    "  .ui-row { align-items: flex-start; }\n"
    "  .ui-heading[data-level=\"1\"] { font-size: 1.65rem; }\n"
    "  .ui-card { padding: var(--ui-space-4); }\n"
    "}\n"
    "\n"
    "@media (prefers-reduced-motion: reduce) {\n"
    "  *,\n"
    "  *::before,\n"
    "  *::after {\n"
    "    animation-duration: 0.001ms !important;\n"
    "    animation-iteration-count: 1 !important;\n"
    "    transition-duration: 0.001ms !important;\n"
    "  }\n"
    "}\n"
    "\n"
    ;


typedef struct {
    bloom_str name;
    bloom_str value; /* value.data == NULL marks a boolean attribute */
} bloom_ui_attr;

typedef struct bloom_ui_node {
    size_t rc;
    int is_document;
    int kind; /* 0 = element, 1 = text */
    bloom_str tag;
    bloom_str text;
    bloom_ui_attr *attrs;
    size_t attr_count;
    size_t attr_cap;
    struct bloom_ui_node **kids;
    size_t kid_count;
    size_t kid_cap;
    struct bloom_ui_node *thead; /* tables only, borrowed */
    struct bloom_ui_node *tbody;
    struct bloom_ui_node *control; /* checkbox/radio wrapper, borrowed */
} bloom_ui_node;

static bloom_str bloom_ui_lit(const char *s) {
    bloom_str v;
    v.rc = BLOOM_STR_IMMORTAL;
    v.data = (char *)s;
    v.len = strlen(s);
    return v;
}

static bloom_str bloom_ui_msg(const char *op, const char *detail) {
    size_t opn = strlen(op);
    size_t dn = strlen(detail);
    char *buf = (char *)malloc(opn + 2 + dn + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(buf, op, opn);
    buf[opn] = ':';
    buf[opn + 1] = ' ';
    memcpy(buf + opn + 2, detail, dn);
    bloom_str s = bloom_str_from_bytes(buf, opn + 2 + dn);
    free(buf);
    return s;
}

static bloom_enum_Result_std_ui_UiNode_str bloom_ui_ok_node(bloom_ui_node *n);
static bloom_enum_Result_std_ui_UiNode_str bloom_ui_err_node(bloom_str m);

/* ---- node lifetime ----------------------------------------------------- */

static bloom_ui_node *bloom_ui_new(bloom_str tag, int kind) {
    bloom_ui_node *n = (bloom_ui_node *)calloc(1, sizeof(bloom_ui_node));
    if (n == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    n->rc = 1;
    n->kind = kind;
    n->tag = tag;
    bloom_live_allocs++;
    return n;
}

static bloom_ui_node *bloom_ui_new_element(const char *tag) {
    return bloom_ui_new(bloom_str_from_bytes(tag, strlen(tag)), 0);
}

static bloom_ui_node *bloom_ui_new_text(bloom_str value) {
    bloom_ui_node *n = bloom_ui_new(bloom_ui_lit("#text"), 1);
    n->text = bloom_str_clone(value);
    return n;
}

static bloom_ui_node *bloom_ui_new_document(void) {
    bloom_ui_node *n = bloom_ui_new_element("html");
    n->is_document = 1;
    return n;
}

static bloom_struct_std_ui_UiNode bloom_ui_wrap(bloom_ui_node *n) {
    bloom_struct_std_ui_UiNode v;
    v.f_id = (bloom_ui_handle)(intptr_t)n;
    return v;
}

static bloom_ui_node *bloom_ui_unwrap(bloom_struct_std_ui_UiNode v) {
    return (bloom_ui_node *)(intptr_t)v.f_id;
}

/* checkbox/radio return a layout wrapper; control setters target the input. */
static bloom_ui_node *bloom_ui_control_target(bloom_ui_node *n) {
    if (n != NULL && n->control != NULL) {
        return n->control;
    }
    return n;
}

bloom_ui_handle bloom_ui_retain(bloom_ui_handle h) {
    bloom_ui_node *n = (bloom_ui_node *)(intptr_t)h;
    if (n != NULL) {
        n->rc++;
    }
    return h;
}

void bloom_ui_release(bloom_ui_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_ui_node *n = (bloom_ui_node *)(intptr_t)*h;
    *h = 0;
    if (n->rc > 0) {
        n->rc--;
    }
    if (n->rc != 0) {
        return;
    }
    for (size_t i = 0; i < n->attr_count; i++) {
        bloom_str_drop(&n->attrs[i].name);
        bloom_str_drop(&n->attrs[i].value);
    }
    free(n->attrs);
    for (size_t i = 0; i < n->kid_count; i++) {
        bloom_ui_handle kh = (bloom_ui_handle)(intptr_t)n->kids[i];
        bloom_ui_release(&kh);
    }
    free(n->kids);
    bloom_str_drop(&n->text);
    bloom_str_drop(&n->tag);
    free(n);
    bloom_live_allocs--;
}

/* ---- validation -------------------------------------------------------- */

static bool bloom_ui_ascii_alnum(char c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
        || (c >= '0' && c <= '9');
}

static bool bloom_ui_token_ok(bloom_str s, size_t max) {
    if (s.len == 0 || s.len > max) {
        return false;
    }
    if (!bloom_ui_ascii_alnum(s.data[0]) && s.data[0] != '_' && s.data[0] != '-') {
        return false;
    }
    for (size_t i = 0; i < s.len; i++) {
        char c = s.data[i];
        if (!bloom_ui_ascii_alnum(c) && c != '_' && c != '-' && c != '.') {
            return false;
        }
    }
    return true;
}

static bool bloom_ui_id_ok(bloom_str s) {
    if (s.len == 0 || s.len > BLOOM_UI_MAX_ID) {
        return false;
    }
    if (!bloom_ui_ascii_alnum(s.data[0])) {
        return false;
    }
    for (size_t i = 0; i < s.len; i++) {
        char c = s.data[i];
        if (!bloom_ui_ascii_alnum(c) && c != '_' && c != '-') {
            return false;
        }
    }
    return true;
}

static bool bloom_ui_class_ok(bloom_str s) {
    if (s.len > BLOOM_UI_MAX_CLASS) {
        return false;
    }
    size_t i = 0;
    while (i < s.len) {
        while (i < s.len && s.data[i] == ' ') {
            i++;
        }
        size_t start = i;
        while (i < s.len && s.data[i] != ' ') {
            i++;
        }
        if (i == start) {
            continue;
        }
        bloom_str piece = bloom_str_from_bytes(s.data + start, i - start);
        bool ok = bloom_ui_token_ok(piece, 64);
        bloom_str_drop(&piece);
        if (!ok) {
            return false;
        }
    }
    return true;
}

static bool bloom_ui_enum_ok(bloom_str v, const char *const *allowed, size_t count) {
    for (size_t i = 0; i < count; i++) {
        if (strlen(allowed[i]) == v.len
            && memcmp(allowed[i], v.data, v.len) == 0) {
            return true;
        }
    }
    return false;
}

static bool bloom_ui_has_ctl(bloom_str s) {
    for (size_t i = 0; i < s.len; i++) {
        unsigned char c = (unsigned char)s.data[i];
        if (c < 0x20 || c == 0x7F) {
            return true;
        }
    }
    return false;
}

static bool bloom_ui_attr_value_ok(bloom_str s) {
    return s.len <= BLOOM_UI_MAX_ATTR_VALUE && !bloom_ui_has_ctl(s);
}

/* Allow-list: http(s) absolute, root-relative, relative paths, fragments.
 * Rejects control bytes, whitespace, backslashes and every other scheme. */
static bool bloom_ui_url_ok(bloom_str url) {
    if (url.len == 0 || url.len > BLOOM_UI_MAX_URL) {
        return false;
    }
    for (size_t i = 0; i < url.len; i++) {
        unsigned char c = (unsigned char)url.data[i];
        if (c <= 0x20 || c == 0x7F || c == '\\') {
            return false;
        }
    }
    if (url.data[0] == '#') {
        return true;
    }
    if (url.data[0] == '/') {
        return url.len == 1 || url.data[1] != '/';
    }
    /* Detect a scheme before any path/query/fragment separator. */
    size_t colon = url.len;
    for (size_t i = 0; i < url.len; i++) {
        char c = url.data[i];
        if (c == '/' || c == '?' || c == '#') {
            break;
        }
        if (c == ':') {
            colon = i;
            break;
        }
    }
    if (colon == url.len) {
        return true; /* relative path without a scheme */
    }
    if (colon == 0) {
        return false;
    }
    size_t n = colon;
    if (n != 4 && n != 5) {
        return false;
    }
    if (colon + 2 >= url.len || url.data[colon + 1] != '/'
        || url.data[colon + 2] != '/') {
        return false;
    }
    char buf[6];
    memcpy(buf, url.data, n);
    buf[n] = '\0';
    for (size_t i = 0; i < n; i++) {
        char c = buf[i];
        if (c >= 'A' && c <= 'Z') {
            buf[i] = (char)(c + 32);
        }
    }
    return strcmp(buf, "http") == 0 || strcmp(buf, "https") == 0;
}

static size_t bloom_ui_text_len(bloom_str s) {
    return s.len;
}

/* ---- attributes and children ------------------------------------------ */

static bool bloom_ui_void_tag(bloom_str tag) {
    static const char *const voids[] = {
        "area", "base", "br", "col", "embed", "hr", "img", "input",
        "link", "meta", "source", "track", "wbr"
    };
    for (size_t i = 0; i < sizeof voids / sizeof *voids; i++) {
        if (strlen(voids[i]) == tag.len && memcmp(voids[i], tag.data, tag.len) == 0) {
            return true;
        }
    }
    return false;
}

/* Stores a validated value; overwrites an attribute with the same name. */
static void bloom_ui_set_attr(bloom_ui_node *n, const char *name, bloom_str value) {
    bloom_str key = bloom_ui_lit(name);
    for (size_t i = 0; i < n->attr_count; i++) {
        if (n->attrs[i].name.len == key.len
            && memcmp(n->attrs[i].name.data, key.data, key.len) == 0) {
            bloom_str_drop(&n->attrs[i].value);
            n->attrs[i].value = value;
            return;
        }
    }
    if (n->attr_count >= BLOOM_UI_MAX_ATTRS) {
        bloom_str_drop(&value);
        return;
    }
    if (n->attr_count == n->attr_cap) {
        size_t cap = n->attr_cap ? n->attr_cap * 2 : 4;
        bloom_ui_attr *na = (bloom_ui_attr *)realloc(n->attrs, cap * sizeof *na);
        if (na == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        n->attrs = na;
        n->attr_cap = cap;
    }
    n->attrs[n->attr_count].name = key;
    n->attrs[n->attr_count].value = value;
    n->attr_count++;
}

static void bloom_ui_set_attr_text(bloom_ui_node *n, const char *name, bloom_str value) {
    bloom_ui_set_attr(n, name, bloom_str_clone(value));
}

static void bloom_ui_set_attr_lit(bloom_ui_node *n, const char *name, const char *value) {
    bloom_ui_set_attr(n, name, bloom_ui_lit(value));
}

static void bloom_ui_set_bool(bloom_ui_node *n, const char *name, bool on) {
    if (!on) {
        return;
    }
    bloom_str empty;
    empty.rc = BLOOM_STR_IMMORTAL;
    empty.data = NULL;
    empty.len = 0;
    bloom_ui_set_attr(n, name, empty);
}

/* Moves `value` into the attribute (no clone). */
static void bloom_ui_set_attr_owned(bloom_ui_node *n, const char *name, bloom_str value) {
    bloom_ui_set_attr(n, name, value);
}

static bool bloom_ui_append_child(bloom_ui_node *parent, bloom_ui_node *child) {
    if (parent == NULL || child == NULL || parent->kind != 0) {
        return false;
    }
    if (bloom_ui_void_tag(parent->tag)) {
        return false;
    }
    if (parent->kid_count >= BLOOM_UI_MAX_CHILDREN) {
        return false;
    }
    if (parent->kid_count == parent->kid_cap) {
        size_t cap = parent->kid_cap ? parent->kid_cap * 2 : 4;
        bloom_ui_node **nk = (bloom_ui_node **)realloc(
            parent->kids,
            cap * sizeof *nk
        );
        if (nk == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        parent->kids = nk;
        parent->kid_cap = cap;
    }
    bloom_ui_retain((bloom_ui_handle)(intptr_t)child);
    parent->kids[parent->kid_count++] = child;
    return true;
}

/* ---- small constructors ------------------------------------------------ */

static bloom_ui_node *bloom_ui_text_node(bloom_str value) {
    return bloom_ui_new_text(value);
}

static bloom_ui_node *bloom_ui_classed(const char *tag, const char *cls) {
    bloom_ui_node *n = bloom_ui_new_element(tag);
    bloom_ui_set_attr_lit(n, "class", cls);
    return n;
}

static void bloom_ui_add_text(bloom_ui_node *parent, bloom_str text) {
    bloom_ui_node *t = bloom_ui_text_node(text);
    bloom_ui_append_child(parent, t);
    bloom_ui_handle h = (bloom_ui_handle)(intptr_t)t;
    bloom_ui_release(&h);
}

/* ---- public: composition ---------------------------------------------- */

bool bloom_ui_append(bloom_struct_std_ui_UiNode parent, bloom_struct_std_ui_UiNode child) {
    return bloom_ui_append_child(bloom_ui_unwrap(parent), bloom_ui_unwrap(child));
}

bool bloom_ui_set_id(bloom_struct_std_ui_UiNode node, bloom_str id) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0 || !bloom_ui_id_ok(id)) {
        return false;
    }
    n = bloom_ui_control_target(n);
    bloom_ui_set_attr_text(n, "id", id);
    return true;
}

bool bloom_ui_placeholder(bloom_struct_std_ui_UiNode node, bloom_str text) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0 || !bloom_ui_attr_value_ok(text)) {
        return false;
    }
    n = bloom_ui_control_target(n);
    bloom_ui_set_attr_text(n, "placeholder", text);
    return true;
}

bool bloom_ui_require(bloom_struct_std_ui_UiNode node) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0) {
        return false;
    }
    n = bloom_ui_control_target(n);
    bloom_ui_set_bool(n, "required", true);
    return true;
}

bool bloom_ui_disable(bloom_struct_std_ui_UiNode node) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0) {
        return false;
    }
    n = bloom_ui_control_target(n);
    bloom_ui_set_bool(n, "disabled", true);
    return true;
}

bool bloom_ui_readonly(bloom_struct_std_ui_UiNode node) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0) {
        return false;
    }
    n = bloom_ui_control_target(n);
    bloom_ui_set_bool(n, "readonly", true);
    return true;
}

bool bloom_ui_set_value(bloom_struct_std_ui_UiNode node, bloom_str value) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0 || !bloom_ui_attr_value_ok(value)) {
        return false;
    }
    n = bloom_ui_control_target(n);
    bloom_ui_set_attr_text(n, "value", value);
    return true;
}

bool bloom_ui_check(bloom_struct_std_ui_UiNode node) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0) {
        return false;
    }
    n = bloom_ui_control_target(n);
    bloom_ui_set_bool(n, "checked", true);
    return true;
}

bool bloom_ui_select_option(bloom_struct_std_ui_UiNode node) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0) {
        return false;
    }
    n = bloom_ui_control_target(n);
    bloom_ui_set_bool(n, "selected", true);
    return true;
}

bool bloom_ui_set_aria_label(bloom_struct_std_ui_UiNode node, bloom_str label) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0 || !bloom_ui_attr_value_ok(label)) {
        return false;
    }
    n = bloom_ui_control_target(n);
    bloom_ui_set_attr_text(n, "aria-label", label);
    return true;
}

static bloom_enum_Result_bool_str bloom_ui_false(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_bool_str bloom_ui_true(void) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = true;
    return r;
}

bloom_enum_Result_bool_str bloom_ui_set_class(
    bloom_struct_std_ui_UiNode node,
    bloom_str tokens
) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL || n->kind != 0) {
        return bloom_ui_false(bloom_ui_msg("set_class", "invalid element"));
    }
    if (!bloom_ui_class_ok(tokens)) {
        return bloom_ui_false(bloom_ui_msg(
            "set_class",
            "invalid class names (tokens of a-z 0-9 _ -, separated by spaces)"
        ));
    }
    bloom_str existing = {0, NULL, 0};
    for (size_t i = 0; i < n->attr_count; i++) {
        if (n->attrs[i].name.len == 5
            && memcmp(n->attrs[i].name.data, "class", 5) == 0) {
            existing = n->attrs[i].value;
            break;
        }
    }
    size_t total = existing.len + (existing.len > 0 ? 1 : 0) + tokens.len;
    if (total > BLOOM_UI_MAX_CLASS) {
        return bloom_ui_false(bloom_ui_msg("set_class", "combined class is too long"));
    }
    char *buf = (char *)malloc(total + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t at = 0;
    if (existing.len > 0) {
        memcpy(buf, existing.data, existing.len);
        at = existing.len;
        buf[at++] = ' ';
    }
    memcpy(buf + at, tokens.data, tokens.len);
    at += tokens.len;
    bloom_str combined = bloom_str_from_bytes(buf, at);
    free(buf);
    bloom_ui_set_attr_owned(n, "class", combined);
    return bloom_ui_true();
}

/* ---- public: layout ---------------------------------------------------- */

static bloom_enum_Result_std_ui_UiNode_str bloom_ui_ok_node(bloom_ui_node *n) {
    bloom_enum_Result_std_ui_UiNode_str r;
    r.tag = 0;
    r.data.m_Ok._0 = bloom_ui_wrap(n);
    return r;
}

static bloom_enum_Result_std_ui_UiNode_str bloom_ui_err_node(bloom_str m) {
    bloom_enum_Result_std_ui_UiNode_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_std_ui_UiNode_str bloom_ui_bad_node(
    const char *op,
    const char *detail
) {
    return bloom_ui_err_node(bloom_ui_msg(op, detail));
}

static bool bloom_ui_gap_ok(bloom_str gap) {
    static const char *const values[] = {"", "xs", "sm", "md", "lg", "xl"};
    return bloom_ui_enum_ok(gap, values, sizeof values / sizeof *values);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_container(bloom_str width) {
    static const char *const values[] = {"", "sm", "md", "lg", "xl", "full"};
    if (!bloom_ui_enum_ok(width, values, sizeof values / sizeof *values)) {
        return bloom_ui_bad_node(
            "container",
            "width must be one of sm, md, lg, xl, full or empty"
        );
    }
    bloom_ui_node *n = bloom_ui_classed("div", "ui-container");
    if (width.len > 0) {
        bloom_ui_set_attr_text(n, "data-width", width);
    }
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_stack(bloom_str gap) {
    if (!bloom_ui_gap_ok(gap)) {
        return bloom_ui_bad_node("stack", "gap must be xs, sm, md, lg, xl or empty");
    }
    bloom_ui_node *n = bloom_ui_classed("div", "ui-stack");
    if (gap.len > 0) {
        bloom_ui_set_attr_text(n, "data-gap", gap);
    }
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_row(bloom_str gap, bool wrap) {
    if (!bloom_ui_gap_ok(gap)) {
        return bloom_ui_bad_node("row", "gap must be xs, sm, md, lg, xl or empty");
    }
    bloom_ui_node *n = bloom_ui_classed("div", "ui-row");
    if (gap.len > 0) {
        bloom_ui_set_attr_text(n, "data-gap", gap);
    }
    if (!wrap) {
        bloom_ui_set_attr_lit(n, "data-wrap", "false");
    }
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_grid(int64_t columns, bloom_str gap) {
    if (columns < 1 || columns > 12) {
        return bloom_ui_bad_node("grid", "columns must be between 1 and 12");
    }
    if (!bloom_ui_gap_ok(gap)) {
        return bloom_ui_bad_node("grid", "gap must be xs, sm, md, lg, xl or empty");
    }
    bloom_ui_node *n = bloom_ui_classed("div", "ui-grid");
    char buf[8];
    int len = snprintf(buf, sizeof buf, "%d", (int)columns);
    bloom_ui_set_attr(n, "data-columns", bloom_str_from_bytes(buf, (size_t)len));
    if (gap.len > 0) {
        bloom_ui_set_attr_text(n, "data-gap", gap);
    }
    return bloom_ui_ok_node(n);
}

bloom_struct_std_ui_UiNode bloom_ui_section(bloom_str title) {
    bloom_ui_node *n = bloom_ui_classed("section", "ui-section");
    if (title.len > 0) {
        bloom_ui_node *h = bloom_ui_new_element("h2");
        bloom_ui_set_attr_lit(h, "class", "ui-heading");
        bloom_ui_set_attr_lit(h, "data-level", "2");
        bloom_ui_add_text(h, title);
        bloom_ui_append_child(n, h);
        bloom_ui_handle hh = (bloom_ui_handle)(intptr_t)h;
        bloom_ui_release(&hh);
    }
    return bloom_ui_wrap(n);
}

bloom_struct_std_ui_UiNode bloom_ui_divider(void) {
    bloom_ui_node *n = bloom_ui_new_element("hr");
    bloom_ui_set_attr_lit(n, "class", "ui-divider");
    return bloom_ui_wrap(n);
}

/* ---- public: text and display ----------------------------------------- */

bloom_enum_Result_std_ui_UiNode_str bloom_ui_heading(int64_t level, bloom_str text) {
    if (level < 1 || level > 6) {
        return bloom_ui_bad_node("heading", "level must be between 1 and 6");
    }
    if (text.len > BLOOM_UI_MAX_TEXT) {
        return bloom_ui_bad_node("heading", "text exceeds 1048576 bytes");
    }
    char tag[3];
    snprintf(tag, sizeof tag, "h%d", (int)level);
    char levelbuf[4];
    int n = snprintf(levelbuf, sizeof levelbuf, "%d", (int)level);
    bloom_ui_node *el = bloom_ui_classed(tag, "ui-heading");
    bloom_ui_set_attr(el, "data-level", bloom_str_from_bytes(levelbuf, (size_t)n));
    bloom_ui_add_text(el, text);
    return bloom_ui_ok_node(el);
}

bloom_struct_std_ui_UiNode bloom_ui_text(bloom_str content) {
    bloom_ui_node *n = bloom_ui_classed("p", "ui-text");
    bloom_ui_add_text(n, content);
    return bloom_ui_wrap(n);
}

bloom_struct_std_ui_UiNode bloom_ui_muted_text(bloom_str content) {
    bloom_ui_node *n = bloom_ui_classed("p", "ui-text ui-muted");
    bloom_ui_add_text(n, content);
    return bloom_ui_wrap(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_link(bloom_str href, bloom_str label) {
    if (!bloom_ui_url_ok(href)) {
        return bloom_ui_bad_node(
            "link",
            "href must be http(s), a single-slash path, a relative path or a fragment"
        );
    }
    bloom_ui_node *n = bloom_ui_classed("a", "ui-link");
    bloom_ui_set_attr_text(n, "href", href);
    bloom_ui_add_text(n, label);
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_badge(bloom_str label, bloom_str variant) {
    static const char *const values[] = {
        "", "default", "success", "warning", "danger", "info"
    };
    if (!bloom_ui_enum_ok(variant, values, sizeof values / sizeof *values)) {
        return bloom_ui_bad_node(
            "badge",
            "variant must be default, success, warning, danger, info or empty"
        );
    }
    bloom_ui_node *n = bloom_ui_classed("span", "ui-badge");
    if (variant.len > 0) {
        bloom_ui_set_attr_text(n, "data-variant", variant);
    }
    bloom_ui_add_text(n, label);
    return bloom_ui_ok_node(n);
}

bloom_struct_std_ui_UiNode bloom_ui_card(bloom_str title) {
    bloom_ui_node *n = bloom_ui_classed("article", "ui-card");
    if (title.len > 0) {
        bloom_ui_node *h = bloom_ui_new_element("h3");
        bloom_ui_set_attr_lit(h, "class", "ui-heading");
        bloom_ui_set_attr_lit(h, "data-level", "3");
        bloom_ui_add_text(h, title);
        bloom_ui_append_child(n, h);
        bloom_ui_handle hh = (bloom_ui_handle)(intptr_t)h;
        bloom_ui_release(&hh);
    }
    return bloom_ui_wrap(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_alert(bloom_str message, bloom_str variant) {
    static const char *const values[] = {"", "info", "success", "warning", "danger"};
    if (!bloom_ui_enum_ok(variant, values, sizeof values / sizeof *values)) {
        return bloom_ui_bad_node(
            "alert",
            "variant must be info, success, warning, danger or empty"
        );
    }
    const char *v = variant.len > 0 ? variant.data : "info";
    size_t vlen = variant.len > 0 ? variant.len : 4;
    bool danger = vlen == 6 && memcmp(v, "danger", 6) == 0;
    bloom_ui_node *n = bloom_ui_classed("div", "ui-alert");
    bloom_ui_set_attr(n, "data-variant", bloom_str_from_bytes(v, vlen));
    bloom_ui_set_attr_lit(n, "role", danger ? "alert" : "status");
    bloom_ui_add_text(n, message);
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_button(
    bloom_str label,
    bloom_struct_std_ui_ButtonOptions options
) {
    static const char *const variants[] = {
        "", "primary", "secondary", "ghost", "danger"
    };
    static const char *const sizes[] = {"", "sm", "md", "lg"};
    static const char *const types[] = {"", "button", "submit", "reset"};
    if (!bloom_ui_enum_ok(options.f_variant, variants, sizeof variants / sizeof *variants)) {
        return bloom_ui_bad_node(
            "button",
            "variant must be primary, secondary, ghost, danger or empty"
        );
    }
    if (!bloom_ui_enum_ok(options.f_size, sizes, sizeof sizes / sizeof *sizes)) {
        return bloom_ui_bad_node("button", "size must be sm, md, lg or empty");
    }
    if (!bloom_ui_enum_ok(options.f_type_name, types, sizeof types / sizeof *types)) {
        return bloom_ui_bad_node("button", "type must be button, submit, reset or empty");
    }
    if (options.f_name.len > 0 && !bloom_ui_token_ok(options.f_name, 128)) {
        return bloom_ui_bad_node("button", "name must be a form field name");
    }
    if (!bloom_ui_attr_value_ok(options.f_value)) {
        return bloom_ui_bad_node("button", "value exceeds 4096 bytes or contains control bytes");
    }
    const char *variant = options.f_variant.len > 0 ? options.f_variant.data : "secondary";
    size_t variant_len = options.f_variant.len > 0 ? options.f_variant.len : 9;
    bloom_ui_node *n = bloom_ui_classed("button", "ui-btn");
    bloom_ui_set_attr(n, "data-variant", bloom_str_from_bytes(variant, variant_len));
    if (options.f_size.len > 0) {
        bloom_ui_set_attr_text(n, "data-size", options.f_size);
    }
    bloom_ui_set_attr_text(
        n,
        "type",
        options.f_type_name.len > 0 ? options.f_type_name : bloom_ui_lit("button")
    );
    if (options.f_name.len > 0) {
        bloom_ui_set_attr_text(n, "name", options.f_name);
        if (options.f_value.len > 0 || options.f_name.len > 0) {
            bloom_ui_set_attr_text(n, "value", options.f_value);
        }
    }
    bloom_ui_set_bool(n, "disabled", options.f_disabled);
    bloom_ui_add_text(n, label);
    return bloom_ui_ok_node(n);
}

/* ---- public: forms ----------------------------------------------------- */

bloom_enum_Result_std_ui_UiNode_str bloom_ui_form(bloom_struct_std_ui_FormOptions options) {
    static const char *const methods[] = {"", "get", "post"};
    if (options.f_action.len > 0 && !bloom_ui_url_ok(options.f_action)) {
        return bloom_ui_bad_node(
            "form",
            "action must be http(s), a single-slash path, a relative path or a fragment"
        );
    }
    if (!bloom_ui_enum_ok(options.f_method, methods, sizeof methods / sizeof *methods)) {
        return bloom_ui_bad_node("form", "method must be get, post or empty");
    }
    bloom_ui_node *n = bloom_ui_classed("form", "ui-form");
    if (options.f_action.len > 0) {
        bloom_ui_set_attr_text(n, "action", options.f_action);
    }
    if (options.f_method.len > 0) {
        bloom_ui_set_attr_text(n, "method", options.f_method);
    }
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_label(bloom_str text, bloom_str for_id) {
    if (!bloom_ui_id_ok(for_id)) {
        return bloom_ui_bad_node("label", "for_id must be a valid id");
    }
    bloom_ui_node *n = bloom_ui_classed("label", "ui-label");
    bloom_ui_set_attr_text(n, "for", for_id);
    bloom_ui_add_text(n, text);
    return bloom_ui_ok_node(n);
}

static bool bloom_ui_input_type_ok(bloom_str type_name) {
    static const char *const types[] = {
        "text", "email", "password", "search", "url", "tel", "number",
        "date", "time", "datetime-local", "color", "file", "hidden",
        "checkbox", "radio", "range", "month", "week"
    };
    return bloom_ui_enum_ok(type_name, types, sizeof types / sizeof *types);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_input(bloom_struct_std_ui_InputOptions options) {
    if (!bloom_ui_input_type_ok(options.f_type_name.len > 0 ? options.f_type_name : bloom_ui_lit("text"))) {
        return bloom_ui_bad_node("input", "unsupported input type");
    }
    if (!bloom_ui_token_ok(options.f_name, 128)) {
        return bloom_ui_bad_node("input", "name must be a valid field name");
    }
    if (!bloom_ui_attr_value_ok(options.f_value)) {
        return bloom_ui_bad_node("input", "value exceeds 4096 bytes or contains control bytes");
    }
    if (!bloom_ui_attr_value_ok(options.f_placeholder)) {
        return bloom_ui_bad_node("input", "placeholder exceeds 4096 bytes or contains control bytes");
    }
    if (options.f_id.len > 0 && !bloom_ui_id_ok(options.f_id)) {
        return bloom_ui_bad_node("input", "id must be a valid id");
    }
    bloom_ui_node *n = bloom_ui_new_element("input");
    bloom_ui_set_attr_lit(n, "class", "ui-input");
    bloom_ui_set_attr_text(
        n,
        "type",
        options.f_type_name.len > 0 ? options.f_type_name : bloom_ui_lit("text")
    );
    bloom_ui_set_attr_text(n, "name", options.f_name);
    if (options.f_id.len > 0) {
        bloom_ui_set_attr_text(n, "id", options.f_id);
    }
    if (options.f_value.len > 0) {
        bloom_ui_set_attr_text(n, "value", options.f_value);
    }
    if (options.f_placeholder.len > 0) {
        bloom_ui_set_attr_text(n, "placeholder", options.f_placeholder);
    }
    bloom_ui_set_bool(n, "required", options.f_required);
    bloom_ui_set_bool(n, "disabled", options.f_disabled);
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_textarea(
    bloom_str name,
    bloom_str value,
    int64_t rows,
    bloom_str id
) {
    if (!bloom_ui_token_ok(name, 128)) {
        return bloom_ui_bad_node("textarea", "name must be a valid field name");
    }
    if (rows < 1 || rows > 40) {
        return bloom_ui_bad_node("textarea", "rows must be between 1 and 40");
    }
    if (!bloom_ui_attr_value_ok(value)) {
        return bloom_ui_bad_node("textarea", "value exceeds 4096 bytes or contains control bytes");
    }
    if (id.len > 0 && !bloom_ui_id_ok(id)) {
        return bloom_ui_bad_node("textarea", "id must be a valid id");
    }
    bloom_ui_node *n = bloom_ui_classed("textarea", "ui-textarea");
    bloom_ui_set_attr_text(n, "name", name);
    if (id.len > 0) {
        bloom_ui_set_attr_text(n, "id", id);
    }
    char buf[8];
    int len = snprintf(buf, sizeof buf, "%d", (int)rows);
    bloom_ui_set_attr(n, "rows", bloom_str_from_bytes(buf, (size_t)len));
    if (value.len > 0) {
        bloom_ui_add_text(n, value);
    }
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_select(
    bloom_str name,
    bloom_str id,
    bool multiple
) {
    if (!bloom_ui_token_ok(name, 128)) {
        return bloom_ui_bad_node("select", "name must be a valid field name");
    }
    if (id.len > 0 && !bloom_ui_id_ok(id)) {
        return bloom_ui_bad_node("select", "id must be a valid id");
    }
    bloom_ui_node *n = bloom_ui_classed("select", "ui-select");
    bloom_ui_set_attr_text(n, "name", name);
    if (id.len > 0) {
        bloom_ui_set_attr_text(n, "id", id);
    }
    bloom_ui_set_bool(n, "multiple", multiple);
    return bloom_ui_ok_node(n);
}

bloom_struct_std_ui_UiNode bloom_ui_option(bloom_str value, bloom_str label) {
    bloom_ui_node *n = bloom_ui_new_element("option");
    if (bloom_ui_attr_value_ok(value)) {
        bloom_ui_set_attr_text(n, "value", value);
    }
    bloom_ui_add_text(n, label);
    return bloom_ui_wrap(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_checkbox(
    bloom_str name,
    bloom_str value,
    bloom_str id
) {
    if (!bloom_ui_token_ok(name, 128)) {
        return bloom_ui_bad_node("checkbox", "name must be a valid field name");
    }
    if (!bloom_ui_attr_value_ok(value)) {
        return bloom_ui_bad_node("checkbox", "value exceeds 4096 bytes or contains control bytes");
    }
    if (id.len > 0 && !bloom_ui_id_ok(id)) {
        return bloom_ui_bad_node("checkbox", "id must be a valid id");
    }
    bloom_ui_node *wrap = bloom_ui_classed("span", "ui-check");
    bloom_ui_node *n = bloom_ui_new_element("input");
    bloom_ui_set_attr_lit(n, "type", "checkbox");
    bloom_ui_set_attr_text(n, "name", name);
    if (value.len > 0) {
        bloom_ui_set_attr_text(n, "value", value);
    }
    if (id.len > 0) {
        bloom_ui_set_attr_text(n, "id", id);
    }
    bloom_ui_append_child(wrap, n);
    wrap->control = n;
    bloom_ui_handle nh = (bloom_ui_handle)(intptr_t)n;
    bloom_ui_release(&nh);
    return bloom_ui_ok_node(wrap);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_radio(
    bloom_str name,
    bloom_str value,
    bloom_str id
) {
    if (!bloom_ui_token_ok(name, 128)) {
        return bloom_ui_bad_node("radio", "name must be a valid field name");
    }
    if (!bloom_ui_attr_value_ok(value)) {
        return bloom_ui_bad_node("radio", "value exceeds 4096 bytes or contains control bytes");
    }
    if (id.len > 0 && !bloom_ui_id_ok(id)) {
        return bloom_ui_bad_node("radio", "id must be a valid id");
    }
    bloom_ui_node *wrap = bloom_ui_classed("span", "ui-check");
    bloom_ui_node *n = bloom_ui_new_element("input");
    bloom_ui_set_attr_lit(n, "type", "radio");
    bloom_ui_set_attr_text(n, "name", name);
    if (value.len > 0) {
        bloom_ui_set_attr_text(n, "value", value);
    }
    if (id.len > 0) {
        bloom_ui_set_attr_text(n, "id", id);
    }
    bloom_ui_append_child(wrap, n);
    wrap->control = n;
    bloom_ui_handle nh = (bloom_ui_handle)(intptr_t)n;
    bloom_ui_release(&nh);
    return bloom_ui_ok_node(wrap);
}

/* aria-describedby is assembled from the generated help/error ids. */
static void bloom_ui_set_lit_attr_describedby(
    bloom_ui_node *n,
    bloom_str id,
    bool with_help,
    bool with_error
) {
    char buf[340];
    if (with_help && with_error) {
        snprintf(
            buf,
            sizeof buf,
            "%.*s-help %.*s-error",
            (int)id.len,
            id.data,
            (int)id.len,
            id.data
        );
    } else if (with_help) {
        snprintf(buf, sizeof buf, "%.*s-help", (int)id.len, id.data);
    } else {
        snprintf(buf, sizeof buf, "%.*s-error", (int)id.len, id.data);
    }
    bloom_ui_set_attr(n, "aria-describedby", bloom_str_from_bytes(buf, strlen(buf)));
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_field(
    bloom_struct_std_ui_UiNode control,
    bloom_str id,
    bloom_str label,
    bloom_str help,
    bloom_str error
) {
    bloom_ui_node *c = bloom_ui_unwrap(control);
    if (!bloom_ui_id_ok(id)) {
        return bloom_ui_bad_node("field", "id must be a valid id");
    }
    if (c == NULL || c->kind != 0) {
        return bloom_ui_bad_node("field", "control must be an element");
    }
    if (!bloom_ui_attr_value_ok(help) || !bloom_ui_attr_value_ok(error)) {
        return bloom_ui_bad_node("field", "help/error text exceeds 4096 bytes or contains control bytes");
    }
    bloom_ui_node *wrap = bloom_ui_classed("div", "ui-field");
    bloom_ui_node *target = bloom_ui_control_target(c);
    bloom_ui_set_attr_text(target, "id", id);
    bloom_ui_node *lab = bloom_ui_classed("label", "ui-label");
    bloom_ui_set_attr_text(lab, "for", id);
    bloom_ui_add_text(lab, label);
    bloom_ui_append_child(wrap, lab);
    bloom_ui_handle lh = (bloom_ui_handle)(intptr_t)lab;
    bloom_ui_release(&lh);
    bloom_ui_append_child(wrap, c);
    bool has_error = error.len > 0;
    bool has_help = help.len > 0;
    if (has_help || has_error) {
        bloom_ui_set_lit_attr_describedby(target, id, has_help, has_error);
    }
    if (has_error) {
        bloom_ui_set_attr_lit(target, "aria-invalid", "true");
    }
    if (has_help) {
        char help_id[160];
        snprintf(help_id, sizeof help_id, "%.*s-help", (int)id.len, id.data);
        bloom_ui_node *hp = bloom_ui_classed("p", "ui-help");
        bloom_str hid = bloom_str_from_bytes(help_id, strlen(help_id));
        bloom_ui_set_attr_text(hp, "id", hid);
        bloom_str_drop(&hid);
        bloom_ui_add_text(hp, help);
        bloom_ui_append_child(wrap, hp);
        bloom_ui_handle hh = (bloom_ui_handle)(intptr_t)hp;
        bloom_ui_release(&hh);
    }
    if (has_error) {
        char err_id[160];
        snprintf(err_id, sizeof err_id, "%.*s-error", (int)id.len, id.data);
        bloom_ui_node *ep = bloom_ui_classed("p", "ui-error");
        bloom_str eid = bloom_str_from_bytes(err_id, strlen(err_id));
        bloom_ui_set_attr_text(ep, "id", eid);
        bloom_str_drop(&eid);
        bloom_ui_set_attr_lit(ep, "role", "alert");
        bloom_ui_add_text(ep, error);
        bloom_ui_append_child(wrap, ep);
        bloom_ui_handle eh = (bloom_ui_handle)(intptr_t)ep;
        bloom_ui_release(&eh);
    }
    return bloom_ui_ok_node(wrap);
}

bloom_struct_std_ui_UiNode bloom_ui_help_text(bloom_str text) {
    bloom_ui_node *n = bloom_ui_classed("p", "ui-help");
    bloom_ui_add_text(n, text);
    return bloom_ui_wrap(n);
}

bloom_struct_std_ui_UiNode bloom_ui_form_error(bloom_str text) {
    bloom_ui_node *n = bloom_ui_classed("p", "ui-error");
    bloom_ui_set_attr_lit(n, "role", "alert");
    bloom_ui_add_text(n, text);
    return bloom_ui_wrap(n);
}

bloom_struct_std_ui_UiNode bloom_ui_submit_button(bloom_str label) {
    bloom_ui_node *n = bloom_ui_classed("button", "ui-btn");
    bloom_ui_set_attr_lit(n, "type", "submit");
    bloom_ui_set_attr_lit(n, "data-variant", "primary");
    bloom_ui_add_text(n, label);
    return bloom_ui_wrap(n);
}

/* ---- public: navigation ------------------------------------------------ */

bloom_struct_std_ui_UiNode bloom_ui_nav(bloom_str label) {
    bloom_ui_node *n = bloom_ui_classed("nav", "ui-nav");
    bloom_ui_set_attr_text(
        n,
        "aria-label",
        label.len > 0 ? label : bloom_ui_lit("Main navigation")
    );
    return bloom_ui_wrap(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_nav_link(
    bloom_str href,
    bloom_str label,
    bool active
) {
    if (!bloom_ui_url_ok(href)) {
        return bloom_ui_bad_node(
            "nav_link",
            "href must be http(s), a single-slash path, a relative path or a fragment"
        );
    }
    bloom_ui_node *n = bloom_ui_classed("a", "ui-nav-link");
    bloom_ui_set_attr_text(n, "href", href);
    if (active) {
        bloom_ui_set_attr_lit(n, "aria-current", "page");
    }
    bloom_ui_add_text(n, label);
    return bloom_ui_ok_node(n);
}

bloom_struct_std_ui_UiNode bloom_ui_breadcrumbs(bloom_str label) {
    bloom_ui_node *n = bloom_ui_classed("nav", "ui-breadcrumbs");
    bloom_ui_set_attr_text(
        n,
        "aria-label",
        label.len > 0 ? label : bloom_ui_lit("Breadcrumb")
    );
    return bloom_ui_wrap(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_breadcrumb(bloom_str href, bloom_str label) {
    if (href.len == 0) {
        bloom_ui_node *n = bloom_ui_classed("span", "ui-crumb");
        bloom_ui_set_attr_lit(n, "aria-current", "page");
        bloom_ui_add_text(n, label);
        return bloom_ui_ok_node(n);
    }
    if (!bloom_ui_url_ok(href)) {
        return bloom_ui_bad_node(
            "breadcrumb",
            "href must be http(s), a single-slash path, a relative path or a fragment"
        );
    }
    bloom_ui_node *n = bloom_ui_classed("a", "ui-crumb");
    bloom_ui_set_attr_text(n, "href", href);
    bloom_ui_add_text(n, label);
    return bloom_ui_ok_node(n);
}

/* ---- public: tables ---------------------------------------------------- */

bloom_enum_Result_std_ui_UiNode_str bloom_ui_table(bloom_struct_std_ui_TableOptions options) {
    bloom_ui_node *wrap = bloom_ui_classed("div", "ui-table-wrap");
    bloom_ui_node *table = bloom_ui_classed("table", "ui-table");
    if (options.f_caption.len > 0) {
        bloom_ui_node *cap = bloom_ui_new_element("caption");
        bloom_ui_add_text(cap, options.f_caption);
        bloom_ui_append_child(table, cap);
        bloom_ui_handle ch = (bloom_ui_handle)(intptr_t)cap;
        bloom_ui_release(&ch);
    }
    bloom_ui_node *thead = bloom_ui_new_element("thead");
    bloom_ui_node *tbody = bloom_ui_new_element("tbody");
    bloom_ui_append_child(table, thead);
    bloom_ui_append_child(table, tbody);
    table->thead = thead;
    table->tbody = tbody;
    bloom_ui_handle th = (bloom_ui_handle)(intptr_t)thead;
    bloom_ui_release(&th);
    bloom_ui_handle tb = (bloom_ui_handle)(intptr_t)tbody;
    bloom_ui_release(&tb);
    bloom_ui_append_child(wrap, table);
    bloom_ui_handle tah = (bloom_ui_handle)(intptr_t)table;
    bloom_ui_release(&tah);
    return bloom_ui_ok_node(wrap);
}

static bloom_ui_node *bloom_ui_table_root(bloom_ui_node *wrap) {
    if (wrap == NULL || wrap->kind != 0 || wrap->kid_count == 0) {
        return NULL;
    }
    bloom_ui_node *t = wrap->kids[0];
    if (t->tag.len != 5 || memcmp(t->tag.data, "table", 5) != 0) {
        return NULL;
    }
    return t;
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_table_row(
    bloom_struct_std_ui_UiNode table,
    bool header
) {
    bloom_ui_node *root = bloom_ui_table_root(bloom_ui_unwrap(table));
    if (root == NULL || root->tbody == NULL) {
        return bloom_ui_bad_node("table_row", "invalid table");
    }
    bloom_ui_node *parent = header ? root->thead : root->tbody;
    bloom_ui_node *row = bloom_ui_new_element("tr");
    bloom_ui_append_child(parent, row);
    /* The parent holds one reference, the caller receives the other. */
    return bloom_ui_ok_node(row);
}

static bool bloom_ui_is_row(bloom_ui_node *n) {
    return n != NULL && n->kind == 0 && n->tag.len == 2
        && memcmp(n->tag.data, "tr", 2) == 0;
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_table_cell(
    bloom_struct_std_ui_UiNode row,
    bloom_str text
) {
    bloom_ui_node *r = bloom_ui_unwrap(row);
    if (!bloom_ui_is_row(r)) {
        return bloom_ui_bad_node("table_cell", "invalid table row");
    }
    bloom_ui_node *cell = bloom_ui_new_element("td");
    bloom_ui_add_text(cell, text);
    bloom_ui_append_child(r, cell);
    return bloom_ui_ok_node(cell);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_table_header_cell(
    bloom_struct_std_ui_UiNode row,
    bloom_str text
) {
    bloom_ui_node *r = bloom_ui_unwrap(row);
    if (!bloom_ui_is_row(r)) {
        return bloom_ui_bad_node("table_header_cell", "invalid table row");
    }
    bloom_ui_node *cell = bloom_ui_new_element("th");
    bloom_ui_set_attr_lit(cell, "scope", "col");
    bloom_ui_add_text(cell, text);
    bloom_ui_append_child(r, cell);
    return bloom_ui_ok_node(cell);
}

bloom_struct_std_ui_UiNode bloom_ui_empty_state(bloom_str title, bloom_str text) {
    bloom_ui_node *n = bloom_ui_classed("div", "ui-empty");
    if (title.len > 0) {
        bloom_ui_node *h = bloom_ui_classed("h3", "ui-heading");
        bloom_ui_set_attr_lit(h, "data-level", "3");
        bloom_ui_add_text(h, title);
        bloom_ui_append_child(n, h);
        bloom_ui_handle hh = (bloom_ui_handle)(intptr_t)h;
        bloom_ui_release(&hh);
    }
    if (text.len > 0) {
        bloom_ui_node *p = bloom_ui_new_element("p");
        bloom_ui_add_text(p, text);
        bloom_ui_append_child(n, p);
        bloom_ui_handle ph = (bloom_ui_handle)(intptr_t)p;
        bloom_ui_release(&ph);
    }
    return bloom_ui_wrap(n);
}

/* ---- public: extras ---------------------------------------------------- */

bloom_enum_Result_std_ui_UiNode_str bloom_ui_avatar(
    bloom_str src,
    bloom_str alt,
    bloom_str size
) {
    static const char *const sizes[] = {"", "sm", "md", "lg"};
    if (!bloom_ui_url_ok(src)) {
        return bloom_ui_bad_node(
            "avatar",
            "src must be http(s), a single-slash path, a relative path or a fragment"
        );
    }
    if (!bloom_ui_attr_value_ok(alt)) {
        return bloom_ui_bad_node("avatar", "alt exceeds 4096 bytes or contains control bytes");
    }
    if (!bloom_ui_enum_ok(size, sizes, sizeof sizes / sizeof *sizes)) {
        return bloom_ui_bad_node("avatar", "size must be sm, md, lg or empty");
    }
    bloom_ui_node *n = bloom_ui_new_element("img");
    bloom_ui_set_attr_lit(n, "class", "ui-avatar");
    bloom_ui_set_attr_text(n, "src", src);
    bloom_ui_set_attr_text(n, "alt", alt);
    if (size.len > 0) {
        bloom_ui_set_attr_text(n, "data-size", size);
    }
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_progress(
    int64_t value,
    int64_t max,
    bloom_str label
) {
    if (max < 1 || max > 1000000) {
        return bloom_ui_bad_node("progress", "max must be between 1 and 1000000");
    }
    if (value < 0 || value > max) {
        return bloom_ui_bad_node("progress", "value must be between 0 and max");
    }
    if (!bloom_ui_attr_value_ok(label)) {
        return bloom_ui_bad_node("progress", "label exceeds 4096 bytes or contains control bytes");
    }
    bloom_ui_node *n = bloom_ui_classed("progress", "ui-progress");
    char buf[24];
    int len = snprintf(buf, sizeof buf, "%d", (int)max);
    bloom_ui_set_attr(n, "max", bloom_str_from_bytes(buf, (size_t)len));
    len = snprintf(buf, sizeof buf, "%d", (int)value);
    bloom_ui_set_attr(n, "value", bloom_str_from_bytes(buf, (size_t)len));
    if (label.len > 0) {
        bloom_ui_set_attr_text(n, "aria-label", label);
    }
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_skeleton(int64_t lines) {
    if (lines < 1 || lines > 20) {
        return bloom_ui_bad_node("skeleton", "lines must be between 1 and 20");
    }
    bloom_ui_node *n = bloom_ui_classed("div", "ui-skeleton");
    bloom_ui_set_attr_lit(n, "aria-hidden", "true");
    for (int64_t i = 0; i < lines; i++) {
        bloom_ui_node *line = bloom_ui_new_element("span");
        bloom_ui_append_child(n, line);
        bloom_ui_handle lh = (bloom_ui_handle)(intptr_t)line;
        bloom_ui_release(&lh);
    }
    return bloom_ui_ok_node(n);
}

bloom_enum_Result_std_ui_UiNode_str bloom_ui_pagination(
    int64_t current,
    int64_t total,
    bloom_str base_href
) {
    if (total < 1 || total > 10000) {
        return bloom_ui_bad_node("pagination", "total must be between 1 and 10000");
    }
    if (current < 1 || current > total) {
        return bloom_ui_bad_node("pagination", "current must be between 1 and total");
    }
    if (!bloom_ui_url_ok(base_href)) {
        return bloom_ui_bad_node(
            "pagination",
            "base_href must be http(s), a single-slash path or a relative path"
        );
    }
    bloom_ui_node *n = bloom_ui_classed("nav", "ui-pagination");
    bloom_ui_set_attr_lit(n, "aria-label", "Pagination");
    bool has_query = false;
    for (size_t i = 0; i < base_href.len; i++) {
        if (base_href.data[i] == '?') {
            has_query = true;
            break;
        }
    }
    for (int64_t page = 1; page <= total; page++) {
        char href[2200];
        snprintf(
            href,
            sizeof href,
            "%.*s%cpage=%d",
            (int)base_href.len,
            base_href.data,
            has_query ? '&' : '?',
            (int)page
        );
        bloom_ui_node *link = bloom_ui_classed("a", "ui-page");
        bloom_ui_set_attr(link, "href", bloom_str_from_bytes(href, strlen(href)));
        if (page == current) {
            bloom_ui_set_attr_lit(link, "aria-current", "page");
        }
        char num[24];
        int len = snprintf(num, sizeof num, "%d", (int)page);
        bloom_str num_text = bloom_str_from_bytes(num, (size_t)len);
        bloom_ui_add_text(link, num_text);
        bloom_str_drop(&num_text);
        bloom_ui_append_child(n, link);
        bloom_ui_handle lh = (bloom_ui_handle)(intptr_t)link;
        bloom_ui_release(&lh);
    }
    return bloom_ui_ok_node(n);
}

/* ---- public: document and rendering ----------------------------------- */

bloom_enum_Result_std_ui_UiNode_str bloom_ui_document(
    bloom_struct_std_ui_PageOptions options
) {
    static const char *const themes[] = {"", "light", "dark", "system"};
    if (!bloom_ui_enum_ok(options.f_theme, themes, sizeof themes / sizeof *themes)) {
        return bloom_ui_bad_node(
            "document",
            "theme must be light, dark, system or empty"
        );
    }
    if (options.f_lang.len > 0) {
        bool lang_ok = options.f_lang.len >= 2 && options.f_lang.len <= 16;
        for (size_t i = 0; lang_ok && i < options.f_lang.len; i++) {
            char c = options.f_lang.data[i];
            if (!bloom_ui_ascii_alnum(c) && c != '-') {
                lang_ok = false;
            }
        }
        if (!lang_ok) {
            return bloom_ui_bad_node("document", "lang must look like de or en-US");
        }
    }
    if (!bloom_ui_attr_value_ok(options.f_title)) {
        return bloom_ui_bad_node("document", "title exceeds 4096 bytes or contains control bytes");
    }
    if (!bloom_ui_attr_value_ok(options.f_description)) {
        return bloom_ui_bad_node("document", "description exceeds 4096 bytes or contains control bytes");
    }
    bloom_str css_href = bloom_ui_lit("/ui.css");
    if (options.f_stylesheet_href.len > 0) {
        if (!bloom_ui_url_ok(options.f_stylesheet_href)) {
            return bloom_ui_bad_node(
                "document",
                "stylesheet_href must be http(s), a single-slash path or a relative path"
            );
        }
        css_href = options.f_stylesheet_href;
    }
    bloom_ui_node *doc = bloom_ui_new_document();
    if (options.f_lang.len > 0) {
        bloom_ui_set_attr_text(doc, "lang", options.f_lang);
    }
    bloom_ui_set_attr_text(
        doc,
        "data-theme",
        options.f_theme.len > 0 ? options.f_theme : bloom_ui_lit("light")
    );
    bloom_ui_node *head = bloom_ui_new_element("head");
    bloom_ui_node *charset = bloom_ui_new_element("meta");
    bloom_ui_set_attr_lit(charset, "charset", "utf-8");
    bloom_ui_append_child(head, charset);
    bloom_ui_node *viewport = bloom_ui_new_element("meta");
    bloom_ui_set_attr_lit(viewport, "name", "viewport");
    bloom_ui_set_attr_lit(viewport, "content", "width=device-width, initial-scale=1");
    bloom_ui_append_child(head, viewport);
    bloom_ui_node *title = bloom_ui_new_element("title");
    bloom_ui_add_text(title, options.f_title);
    bloom_ui_append_child(head, title);
    if (options.f_description.len > 0) {
        bloom_ui_node *desc = bloom_ui_new_element("meta");
        bloom_ui_set_attr_lit(desc, "name", "description");
        bloom_ui_set_attr_text(desc, "content", options.f_description);
        bloom_ui_append_child(head, desc);
        bloom_ui_handle de = (bloom_ui_handle)(intptr_t)desc;
        bloom_ui_release(&de);
    }
    bloom_ui_node *css = bloom_ui_new_element("link");
    bloom_ui_set_attr_lit(css, "rel", "stylesheet");
    bloom_ui_set_attr_text(css, "href", css_href);
    bloom_ui_append_child(head, css);
    bloom_ui_node *body = bloom_ui_classed("body", "ui-body");
    bloom_ui_append_child(doc, head);
    bloom_ui_append_child(doc, body);
    bloom_ui_handle hh = (bloom_ui_handle)(intptr_t)head;
    bloom_ui_release(&hh);
    bloom_ui_handle bh = (bloom_ui_handle)(intptr_t)body;
    bloom_ui_release(&bh);
    bloom_ui_handle ch = (bloom_ui_handle)(intptr_t)charset;
    bloom_ui_release(&ch);
    bloom_ui_handle vh = (bloom_ui_handle)(intptr_t)viewport;
    bloom_ui_release(&vh);
    bloom_ui_handle th = (bloom_ui_handle)(intptr_t)title;
    bloom_ui_release(&th);
    bloom_ui_handle dh = (bloom_ui_handle)(intptr_t)css;
    bloom_ui_release(&dh);
    return bloom_ui_ok_node(doc);
}

bloom_struct_std_ui_UiNode bloom_ui_body(bloom_struct_std_ui_UiNode document) {
    bloom_ui_node *doc = bloom_ui_unwrap(document);
    if (doc == NULL || !doc->is_document || doc->kid_count == 0) {
        return bloom_ui_wrap(NULL);
    }
    bloom_ui_retain((bloom_ui_handle)(intptr_t)doc->kids[doc->kid_count - 1]);
    return bloom_ui_wrap(doc->kids[doc->kid_count - 1]);
}

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} bloom_ui_out;

static void bloom_ui_out_init(bloom_ui_out *o) {
    o->data = NULL;
    o->len = 0;
    o->cap = 0;
}

static bool bloom_ui_out_append(bloom_ui_out *o, const char *p, size_t n) {
    if (n == 0) {
        return true;
    }
    if (o->len > BLOOM_UI_MAX_OUTPUT || n > BLOOM_UI_MAX_OUTPUT - o->len) {
        return false;
    }
    size_t need = o->len + n + 1;
    if (need > o->cap) {
        size_t cap = o->cap ? o->cap * 2 : 4096;
        while (cap < need) {
            cap *= 2;
        }
        char *nd = (char *)realloc(o->data, cap);
        if (nd == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        o->data = nd;
        o->cap = cap;
    }
    memcpy(o->data + o->len, p, n);
    o->len += n;
    o->data[o->len] = '\0';
    return true;
}

static bool bloom_ui_out_append_str(bloom_ui_out *o, bloom_str s) {
    return bloom_ui_out_append(o, s.data != NULL ? s.data : "", s.len);
}

static bool bloom_ui_render_node(
    bloom_ui_out *o,
    bloom_ui_node *n,
    size_t depth,
    size_t *nodes,
    bloom_str *err
);

static bool bloom_ui_render_element(
    bloom_ui_out *o,
    bloom_ui_node *n,
    size_t depth,
    size_t *nodes,
    bloom_str *err
) {
    if (!bloom_ui_out_append(o, "<", 1)
        || !bloom_ui_out_append_str(o, n->tag)) {
        *err = bloom_ui_msg("render", "output exceeds 8388608 bytes");
        return false;
    }
    for (size_t i = 0; i < n->attr_count; i++) {
        if (!bloom_ui_out_append(o, " ", 1)
            || !bloom_ui_out_append_str(o, n->attrs[i].name)) {
            *err = bloom_ui_msg("render", "output exceeds 8388608 bytes");
            return false;
        }
        if (n->attrs[i].value.data == NULL) {
            continue;
        }
        bloom_str escaped = bloom_html_escape(n->attrs[i].value);
        bool ok = bloom_ui_out_append(o, "=\"", 2)
            && bloom_ui_out_append_str(o, escaped)
            && bloom_ui_out_append(o, "\"", 1);
        bloom_str_drop(&escaped);
        if (!ok) {
            *err = bloom_ui_msg("render", "output exceeds 8388608 bytes");
            return false;
        }
    }
    bool is_void = bloom_ui_void_tag(n->tag);
    if (is_void) {
        if (n->kid_count > 0) {
            *err = bloom_ui_msg("render", "void element cannot have children");
            return false;
        }
        return bloom_ui_out_append(o, ">", 1);
    }
    if (!bloom_ui_out_append(o, ">", 1)) {
        *err = bloom_ui_msg("render", "output exceeds 8388608 bytes");
        return false;
    }
    if (depth >= BLOOM_UI_MAX_DEPTH) {
        *err = bloom_ui_msg("render", "nesting deeper than 64 levels");
        return false;
    }
    for (size_t i = 0; i < n->kid_count; i++) {
        if (!bloom_ui_render_node(o, n->kids[i], depth + 1, nodes, err)) {
            return false;
        }
    }
    if (!bloom_ui_out_append(o, "</", 2)
        || !bloom_ui_out_append_str(o, n->tag)
        || !bloom_ui_out_append(o, ">", 1)) {
        *err = bloom_ui_msg("render", "output exceeds 8388608 bytes");
        return false;
    }
    return true;
}

static bool bloom_ui_render_node(
    bloom_ui_out *o,
    bloom_ui_node *n,
    size_t depth,
    size_t *nodes,
    bloom_str *err
) {
    if (n == NULL) {
        *err = bloom_ui_msg("render", "invalid node");
        return false;
    }
    (*nodes)++;
    if (*nodes > BLOOM_UI_MAX_NODES) {
        *err = bloom_ui_msg("render", "page exceeds 20000 nodes");
        return false;
    }
    if (n->kind == 1) {
        if (n->text.len > BLOOM_UI_MAX_TEXT || memchr(n->text.data, '\0', n->text.len) != NULL) {
            *err = bloom_ui_msg(
                "render",
                "text exceeds 1048576 bytes or contains a NUL byte"
            );
            return false;
        }
        bloom_str escaped = bloom_html_escape(n->text);
        bool ok = bloom_ui_out_append_str(o, escaped);
        bloom_str_drop(&escaped);
        if (!ok) {
            *err = bloom_ui_msg("render", "output exceeds 8388608 bytes");
            return false;
        }
        return true;
    }
    if (n->is_document) {
        if (!bloom_ui_out_append(o, "<!doctype html>", 15)) {
            *err = bloom_ui_msg("render", "output exceeds 8388608 bytes");
            return false;
        }
    }
    return bloom_ui_render_element(o, n, depth, nodes, err);
}

bloom_enum_Result_str_str bloom_ui_render(bloom_struct_std_ui_UiNode node) {
    bloom_ui_node *n = bloom_ui_unwrap(node);
    if (n == NULL) {
        bloom_enum_Result_str_str r;
        r.tag = 1;
        r.data.m_Err._0 = bloom_ui_msg("render", "invalid node");
        return r;
    }
    bloom_ui_out out;
    bloom_ui_out_init(&out);
    size_t nodes = 0;
    bloom_str err = {0, NULL, 0};
    if (!bloom_ui_render_node(&out, n, 0, &nodes, &err)) {
        free(out.data);
        bloom_enum_Result_str_str r;
        r.tag = 1;
        r.data.m_Err._0 = err;
        return r;
    }
    bloom_str html = bloom_str_from_bytes(out.data != NULL ? out.data : "", out.len);
    free(out.data);
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = html;
    return r;
}

bloom_enum_Result_bool_str bloom_ui_send_page(
    int64_t connection,
    int64_t status,
    bloom_struct_std_ui_UiNode document,
    bloom_str headers
) {
    bloom_enum_Result_str_str html = bloom_ui_render(document);
    if (html.tag == 1) {
        bloom_enum_Result_bool_str r;
        r.tag = 1;
        r.data.m_Err._0 = html.data.m_Err._0;
        return r;
    }
    bloom_str content_type = bloom_ui_lit("text/html; charset=utf-8");
    bloom_enum_Result_bool_str written = bloom_http_write_response_full(
        connection,
        status,
        content_type,
        headers,
        html.data.m_Ok._0
    );
    bloom_str_drop(&html.data.m_Ok._0);
    return written;
}

bloom_str bloom_ui_stylesheet(void) {
    return bloom_str_from_bytes(BLOOM_UI_CSS, sizeof BLOOM_UI_CSS - 1);
}

bloom_enum_Result_bool_str bloom_ui_send_stylesheet(int64_t connection) {
    bloom_str css = bloom_ui_stylesheet();
    bloom_str content_type = bloom_ui_lit("text/css; charset=utf-8");
    bloom_str headers = bloom_ui_lit("Cache-Control: public, max-age=86400");
    bloom_enum_Result_bool_str written = bloom_http_write_response_full(
        connection,
        200,
        content_type,
        headers,
        css
    );
    bloom_str_drop(&css);
    return written;
}
