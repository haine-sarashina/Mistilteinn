# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working in this repository.

## Project Overview

**Mistilteinn** is a lightweight desktop browser application built as a pure Rust Cargo project. It does **not** use Tauri, WebView, or any system web rendering backend. Instead, it implements its **own HTML parser, CSS parser, layout engine, and renderer** — rendering web content from scratch.

### Design Goals
- Minimal memory footprint
- Fast rendering performance
- Self-contained HTML/CSS parsing and rendering (no WebView/Webview2)

### Current Milestone

**Japan Amazon Top Page (amazon.co.jp) renders correctly.**

This is the near-term target. The engine must handle the HTML structure, CSS layout, images, and interactive elements found on `https://www.amazon.co.jp/` without visual or functional issues. Use this page as the primary integration test benchmark.

## Tech Stack

| Layer | Crate |
|-------|-------|
| Window Management | `winit` |
| GPU Rendering | `wgpu` |
| HTML Parsing | `html5ever` |
| CSS Parsing | `cssparser` + `cssparser-color` |
| HTTP Requests | `reqwest` (async) |
| Image Loading | `image` |
| Font/Text | `rusttype` or `parley` |
| Testing | `cargo test` (built-in, with `#[cfg(test)]`) |
| Linting | `cargo clippy` |
| Formatting | `cargo fmt` |

## Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│                         Application                               │
│                        (winit event loop)                          │
│                                                                   │
│  ┌──────────┐                                                   │
│  │ Browser  │  ← vertical tab bar (left side) + tab groups       │
│  │ Chrome   │  ← address bar, nav buttons                        │
│  └───┬──────┘                                                   │
│      │                                                          │
│  ┌───▼───────┐    ┌──────────────┐    ┌────────────────┐       │
│  │ HTML      │ →  │ CSS          │ →  │ Layout         │       │
│  │ Parser    │    │ Parser       │    │ Engine         │       │
│  │           │    │              │    │  (custom)      │       │
│  └─────┬─────┘    └──────────────┘    └───────┬───────┘       │
│        │                                     │                │
│        │         ┌───────────────────┐        │                │
│        └──────── │ Style             │ <──────┘                │
│                  │ Computation       │                          │
│                  └───────┬──────────┘                          │
│                          │                                     │
│                 ┌───────▼───────────┐    ┌────────────────┐    │
│                 │ Render Tree       │ →  │ GPU Renderer   │    │
│                 │ (custom)          │    │  (wgpu)        │    │
│                 └───────────────────┘    └────────────────┘    │
└────────────────────────────────────────────────────────────────┘
```

### Vertical Tab + Tab Group Design

Tabs are displayed **vertically on the left side** of the window, not horizontally across the top.
- Each tab holds a browsing context (DOM, layout tree, render state, scroll position)
- Tabs can be organized into **groups**, each with a name and visual indicator
- The tab bar supports: create / close / reorder / switch tabs, and create / rename / collapse groups
- Rendering area occupies the remaining window space to the right of the tab bar

### Key Modules (`src/`)
- `html/` — HTML parsing using `html5ever`, DOM tree construction
- `css/` — CSS parsing using `cssparser`, style computation & cascade
- `layout/` — Layout engine (box model, flexbox, inline layout, render tree build)
- `render/` — GPU rendering pipeline using `wgpu`
- `network/` — HTTP client for fetching web resources
- `browser/` — Browser chrome (vertical tabs, tab groups, address bar, navigation)
- `app/` — Application state, event loop integration with `winit`

## Essential Commands

```bash
cargo check           # Check without full build
cargo clippy          # Run Clippy linter
cargo fmt             # Format code
cargo test            # Run all tests
cargo test layout     # Run tests matching "layout" (module or test name)
cargo test html::parser::tests::test_empty_tag   # Run a single test
cargo build           # Build release-ready binary
cargo run             # Run the application
```

## Important Notes

- **Pure Rust** — no Tauri, no frontend framework. Everything is in Cargo.
- **No WebView** — the rendering engine is custom-built. Do not delegate content rendering to system WebView/Webview2.
- **Memory efficiency is a core requirement** — profile memory usage when adding new features. Prefer arenas, slab allocation, and cache-friendly data structures.
- **Test coverage is mandatory** for parsing, layout, and rendering logic.
