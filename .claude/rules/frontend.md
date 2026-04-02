---
description: Solid.js frontend conventions — accessibility, Tauri IPC, minimal bundle
globs: ["src/**/*.tsx", "src/**/*.ts"]
---

# Frontend Rules

1. **Solid.js reactive primitives.** Use `createSignal`, `createEffect`, `For`, `Show`. No external state management libraries.

2. **Full ARIA support.** Landmarks (nav, complementary, main), aria-activedescendant for list navigation, aria-live for announcements, heading hierarchy (h1/h2/h3), focus-visible indicators.

3. **Keyboard-first.** j/k for articles, arrows for feeds, Enter to open, Esc to go back, Tab between panes, h for home, n/p for unread navigation.

4. **Tauri IPC via `invoke()`.** All data comes from `@tauri-apps/api/core`. The frontend never fetches feeds directly.

5. **Minimal bundle.** Currently ~20KB JS + ~5KB CSS. Avoid adding heavy dependencies.
