use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorIcon, WindowAttributes},
};

use crate::render::text::TextRenderer;
use crate::render::{ColorF, MAX_RECTS, RectClip, Renderer, layout_to_clip};

/// Chrome layout constants.
const TAB_BAR_WIDTH: u32 = 200;
const ADDRESS_BAR_HEIGHT: u32 = 40;
const TAB_BUTTON_HEIGHT: f32 = 45.0;
const TAB_BUTTON_SPACING: f32 = 4.0;
const GROUP_HEADER_HEIGHT: f32 = 30.0;
const TAB_BUTTON_X: f32 = 10.0;
const TAB_BUTTON_RIGHT_MARGIN: f32 = 20.0;
const TAB_GROUP_COLOR_STRIP_WIDTH: f32 = 4.0;
const LOADING_BAR_HEIGHT: f32 = 3.0;
const LOADING_BAR_COLOR_R: f32 = 80.0 / 255.0;
const LOADING_BAR_COLOR_G: f32 = 140.0 / 255.0;
const LOADING_BAR_COLOR_B: f32 = 230.0 / 255.0;

/// Main application struct implementing winit's ApplicationHandler trait.
pub struct MistilteinnApp {
    renderer: Option<Renderer>,
    pub start_url: Option<String>,
    tab_manager: crate::browser::tab::TabManager,
    group_manager: crate::browser::tab_group::GroupManager,
    tokio_handle: Option<tokio::runtime::Handle>,
    /// Tracks the current cursor position for hit-testing chrome elements.
    cursor_pos: (f32, f32),
    /// Whether Ctrl key is currently pressed (for keyboard shortcuts).
    ctrl_pressed: bool,
    /// The tab id currently under the cursor (for hover highlight).
    hovered_tab_id: Option<crate::browser::tab::TabId>,
    /// Whether the address bar is currently under the cursor.
    hovered_address_bar: bool,
    /// The deepest DOM node ID in page content that was last determined to be under the cursor.
    /// Used to detect when :hover needs to be recomputed.
    prev_hovered_dom_id: Option<u32>,
}

/// Hit-test result for tab bar clicks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum HitTestResult {
    /// Clicked on a group header (toggle collapse)
    GroupHeader(crate::browser::tab_group::GroupId),
    /// Clicked on a tab button
    TabButton(crate::browser::tab::TabId),
    /// Clicked on empty space in tab bar
    Empty,
}

impl HitTestResult {
    fn into_tab_id(self) -> Option<crate::browser::tab::TabId> {
        match self {
            HitTestResult::TabButton(id) => Some(id),
            _ => None,
        }
    }
}

/// Cursor kinds for interactive elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorKind {
    /// Default arrow cursor.
    Default,
    /// Pointer (hand) cursor for links.
    Pointer,
    /// I-beam cursor for text inputs.
    IBeam,
}

impl MistilteinnApp {
    /// Build chrome UI rectangles (tab bar background + per-tab buttons + address bar).
    fn build_chrome_rects(
        &self,
        window_width: u32,
        window_height: u32,
    ) -> (Vec<RectClip>, Vec<ColorF>) {
        let mut rects: Vec<RectClip> = Vec::new();
        let mut colors: Vec<ColorF> = Vec::new();

        let active_id = self.tab_manager.active_tab_id();

        // Tab bar background — dark gray full-height strip on left
        rects.push(layout_to_clip(
            0.0,
            0.0,
            TAB_BAR_WIDTH as f32,
            window_height as f32,
            window_width as f32,
            window_height as f32,
        ));
        colors.push(ColorF {
            r: 42.0 / 255.0,
            g: 42.0 / 255.0,
            b: 47.0 / 255.0,
            a: 255.0,
        });

        // Per-tab button rects — organized by groups
        let mut y = ADDRESS_BAR_HEIGHT as f32 + TAB_BUTTON_SPACING;

        // Render each group's tabs (in insertion order)
        for group in self.group_manager.all_groups() {
            // Group header bar — colored background
            let (hdr_r, hdr_g, hdr_b) = group.color.to_dark_rgb();

            rects.push(layout_to_clip(
                TAB_BUTTON_X,
                y,
                TAB_BAR_WIDTH as f32 - TAB_BUTTON_X - TAB_BUTTON_RIGHT_MARGIN,
                GROUP_HEADER_HEIGHT,
                window_width as f32,
                window_height as f32,
            ));
            colors.push(ColorF {
                r: hdr_r,
                g: hdr_g,
                b: hdr_b,
                a: 255.0,
            });

            // Colored left strip on group header — indicates group color
            let (strip_r, strip_g, strip_b) = group.color.to_rgb();

            rects.push(layout_to_clip(
                TAB_BUTTON_X,
                y,
                TAB_GROUP_COLOR_STRIP_WIDTH,
                GROUP_HEADER_HEIGHT,
                window_width as f32,
                window_height as f32,
            ));
            colors.push(ColorF {
                r: strip_r,
                g: strip_g,
                b: strip_b,
                a: 255.0,
            });

            y += GROUP_HEADER_HEIGHT + TAB_BUTTON_SPACING;

            // Render tabs in this group (if not collapsed)
            let (strip_r, strip_g, strip_b) = group.color.to_rgb();
            if !group.collapsed {
                for tab_id in &group.tab_ids {
                    if let Some(tab) = self.tab_manager.get_tab(*tab_id) {
                        Self::push_tab_button_rects(
                            &mut rects,
                            &mut colors,
                            tab,
                            active_id,
                            self.hovered_tab_id,
                            y,
                            window_width,
                            window_height,
                            Some((strip_r, strip_g, strip_b)),
                        );
                        y += TAB_BUTTON_HEIGHT + TAB_BUTTON_SPACING;
                    }
                }
            }

            y += TAB_BUTTON_SPACING; // extra spacing after each group
        }

        // Render ungrouped tabs at bottom
        for tab in self.tab_manager.all_tabs() {
            if tab.group_id.is_none() {
                Self::push_tab_button_rects(
                    &mut rects,
                    &mut colors,
                    tab,
                    active_id,
                    self.hovered_tab_id,
                    y,
                    window_width,
                    window_height,
                    None, // no group color strip
                );
                y += TAB_BUTTON_HEIGHT + TAB_BUTTON_SPACING;
            }
        }

        // Address bar background — right of tab bar, top
        rects.push(layout_to_clip(
            TAB_BAR_WIDTH as f32,
            0.0,
            window_width as f32 - TAB_BAR_WIDTH as f32,
            ADDRESS_BAR_HEIGHT as f32,
            window_width as f32,
            window_height as f32,
        ));
        let (addr_r, addr_g, addr_b) = if self.hovered_address_bar {
            (65.0 / 255.0, 65.0 / 255.0, 75.0 / 255.0)
        } else {
            (55.0 / 255.0, 55.0 / 255.0, 60.0 / 255.0)
        };
        colors.push(ColorF {
            r: addr_r,
            g: addr_g,
            b: addr_b,
            a: 255.0,
        });

        // Loading progress bar — appears below address bar when active tab is loading
        if self.tab_manager.is_active_tab_loading() {
            rects.push(layout_to_clip(
                TAB_BAR_WIDTH as f32,
                ADDRESS_BAR_HEIGHT as f32,
                window_width as f32 - TAB_BAR_WIDTH as f32,
                LOADING_BAR_HEIGHT,
                window_width as f32,
                window_height as f32,
            ));
            colors.push(ColorF {
                r: LOADING_BAR_COLOR_R,
                g: LOADING_BAR_COLOR_G,
                b: LOADING_BAR_COLOR_B,
                a: 255.0,
            });
        }

        (rects, colors)
    }

    /// Rebuild the render artifacts from the current page and upload to GPU.
    fn recompose(&mut self) {
        // Gather window dimensions without holding a mutable borrow on renderer
        let (win_w, win_h) = self
            .renderer
            .as_ref()
            .map(|r| {
                (
                    r.window().inner_size().width,
                    r.window().inner_size().height,
                )
            })
            .unwrap_or((1280, 800));

        // Build chrome rects (tab bar + address bar)
        let (chrome_rects, chrome_colors) = self.build_chrome_rects(win_w, win_h);
        let _chrome_count = chrome_rects.len();

        // Read scroll offset as a value before borrowing page
        let scroll_offset = self
            .tab_manager
            .get_active_tab_scroll_mut()
            .map(|s| *s)
            .unwrap_or((0.0, 0.0));

        let Some(ref page) = self.tab_manager.get_active_tab_page() else {
            // Upload chrome only if no page content
            if let Some(ref mut renderer) = self.renderer {
                renderer.set_rects(&chrome_rects, &chrome_colors);
            }
            return;
        };

        // Use effective view dimensions (subtract chrome area offsets for layout purposes)
        let view_w = page.view_width;
        let view_h = page.view_height;

        // Collect render rectangles from layout tree and convert to clip space
        let rects = page.collect_rects();

        // Shift page content by chrome offset and apply scroll, then convert to clip space
        let mut page_clip_rects: Vec<(RectClip, Option<[u8; 4]>)> = Vec::new();
        for (mut r, c) in rects.into_iter() {
            // Apply scroll offset by shifting rect position
            r.x -= scroll_offset.0;
            r.y -= scroll_offset.1;

            // Shift to account for tab bar width and address bar height
            let final_x = r.x + TAB_BAR_WIDTH as f32;
            let final_y = r.y + ADDRESS_BAR_HEIGHT as f32;

            if r.width > 0.0 && r.height > 0.0 {
                page_clip_rects.push((
                    layout_to_clip(
                        final_x,
                        final_y,
                        r.width,
                        r.height,
                        win_w as f32,
                        win_h as f32,
                    ),
                    c,
                ));
            }
        }

        // Merge chrome rects + page rects into single buffer for GPU upload
        let mut all_rects: Vec<RectClip> = chrome_rects;
        let mut all_colors: Vec<ColorF> = chrome_colors;

        for (rect, color) in page_clip_rects {
            if all_rects.len() < MAX_RECTS {
                all_rects.push(rect);
                all_colors.push(if let Some(col) = color {
                    crate::render::color_u8_to_f32(col)
                } else {
                    ColorF {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }
                });
            }
        }

        // Now borrow renderer mutably for GPU upload
        let Some(ref mut renderer) = self.renderer else {
            return;
        };
        renderer.set_rects(&all_rects, &all_colors);

        // Collect text nodes from layout tree and rasterize into a bitmap
        let text_nodes = crate::layout::collect_text_nodes(&page.layout_root);

        // Collect image nodes from layout tree (sync recompose does NOT fetch images)
        let image_nodes = crate::layout::collect_image_nodes(&page.layout_root);

        if !text_nodes.is_empty() || !image_nodes.is_empty() {
            let view_width = page.view_width as u32;
            let view_height = page.view_height as u32;

            // Allocate RGBA buffer (transparent background)
            let mut composite_buffer = vec![0u8; (view_width * view_height * 4) as usize];

            // Rasterize text nodes into the composite buffer
            if !text_nodes.is_empty() {
                let mut text_renderer = TextRenderer::new();

                for text_info in &text_nodes {
                    let color_f32: [f32; 4] = [
                        text_info.color[0] as f32 / 255.0,
                        text_info.color[1] as f32 / 255.0,
                        text_info.color[2] as f32 / 255.0,
                        text_info.color[3] as f32 / 255.0,
                    ];

                    // Apply scroll offset to text position too
                    let text_x = text_info.x - scroll_offset.0;
                    let text_y = text_info.y - scroll_offset.1;

                    text_renderer.rasterize_to_bitmap(
                        &text_info.text,
                        text_info.font_size,
                        "sans-serif",
                        color_f32,
                        text_x,
                        text_y,
                        text_info.width,
                        &mut composite_buffer,
                        view_width,
                        view_height,
                    );
                }

                log::info!(
                    "Rasterized {} text nodes at {}x{}",
                    text_nodes.len(),
                    view_width,
                    view_height
                );
            }

            // Composite cached images from page cache (scrolling, no network fetch)
            for img_info in &image_nodes {
                let resolved_src = if !page.page_url.is_empty() {
                    crate::network::resolve_url(&page.page_url, &img_info.src)
                } else {
                    img_info.src.clone()
                };

                if let Some(cached) = page.image_cache.get(&resolved_src) {
                    let img_x = img_info.x - scroll_offset.0;
                    let img_y = img_info.y - scroll_offset.1;
                    crate::render::composite_image(
                        &cached.rgba,
                        cached.width,
                        cached.height,
                        &mut composite_buffer,
                        view_width as u32,
                        view_height as u32,
                        img_x,
                        img_y,
                    );
                }
            }

            // Upload the composite bitmap to GPU
            renderer.set_text_bitmap(view_width, view_height, &composite_buffer);
            log::info!(
                "Composite overlay uploaded: {} text + {} images at {}x{}",
                text_nodes.len(),
                image_nodes.len(),
                view_width,
                view_height
            );
        }
    }

    /// Load a page from HTML and CSS source strings.
    pub fn load_page(&mut self, html_source: &str, css_source: &str) {
        let w = self.window_width() as f32 - TAB_BAR_WIDTH as f32;
        let h = self.window_height() as f32 - ADDRESS_BAR_HEIGHT as f32;

        let new_page = crate::page::Page::new(html_source, css_source, w, h);
        self.tab_manager.set_active_tab_page(new_page);
        self.recompose();

        if let Some(ref mut renderer) = self.renderer {
            if let Err(e) = renderer.render() {
                log::error!("Render after load_page failed: {:?}", e);
            } else {
                log::info!("Page loaded and rendered");
            }
        }

        // Memory profiling (only when memprof feature is enabled)
        #[cfg(feature = "memprof")]
        {
            if let Some(ref page) = self.tab_manager.get_active_tab_page() {
                let comp_size = (page.view_width as usize)
                    .saturating_mul(page.view_height as usize)
                    .saturating_mul(4);
                let profile = page.profile(comp_size);
                log::info!("{}", profile.summary());
            }
        }
    }

    /// Load a page from a URL by fetching it over the network.
    pub fn load_url(&mut self, url: &str) {
        let handle = match &self.tokio_handle {
            Some(h) => h.clone(),
            None => {
                log::error!("No tokio handle available for loading URL");
                return;
            }
        };
        // Delegate to async version to avoid blocking the event loop synchronously.
        handle.block_on(self.load_url_async(url));
    }

    /// Load a page from a URL asynchronously with external CSS fetching.
    pub async fn load_url_async(&mut self, url: &str) {
        // Set loading state and repaint so the indicator appears before the async fetch
        self.tab_manager.set_active_tab_loading(true);
        self.recompose();
        if let Some(ref renderer) = self.renderer {
            renderer.window().request_redraw();
        }

        // Fetch HTML
        let fetch_result = match crate::network::fetch(url).await {
            Ok(r) => r,
            Err(e) => {
                log::error!("Failed to fetch {}: {:?}", url, e);
                self.tab_manager.set_active_tab_loading(false);
                return;
            }
        };

        let final_url = fetch_result.final_url;
        let html = fetch_result.content;

        // Log redirect if the final URL differs from the requested URL
        if final_url != url {
            log::info!("Redirected: {} -> {}", url, final_url);
        }

        // Push resolved URL to history
        if let Some(tab) = self.tab_manager.active_tab_mut() {
            tab.push_history(&final_url);
            tab.url = final_url.clone();
        }

        // Fetch all CSS (inline + external stylesheets) concurrently
        let css = crate::network::fetch_external_css(&final_url, &html)
            .await
            .unwrap_or_else(|e| {
                log::warn!("Failed to fetch external CSS: {:?}", e);
                crate::network::extract_css(&html)
            });

        // Fallback default CSS if nothing extracted
        let final_css = if css.is_empty() {
            "* { margin: 0; padding: 0; } body { display: block; }".to_string()
        } else {
            css
        };

        self.load_page_async(&html, &final_css, Some(&final_url))
            .await;
        self.tab_manager.set_active_tab_loading(false);
    }

    /// Load a page asynchronously (fetches and composites images).
    pub async fn load_page_async(
        &mut self,
        html_source: &str,
        css_source: &str,
        base_url: Option<&str>,
    ) {
        let w = self.window_width() as f32 - TAB_BAR_WIDTH as f32;
        let h = self.window_height() as f32 - ADDRESS_BAR_HEIGHT as f32;

        let mut new_page = crate::page::Page::new(html_source, css_source, w, h);
        if let Some(url) = &base_url {
            new_page.page_url = url.to_string();
        }
        self.tab_manager.set_active_tab_page(new_page);

        // Read scroll offset before borrowing page
        let scroll_offset = self
            .tab_manager
            .get_active_tab_scroll_mut()
            .map(|s| *s)
            .unwrap_or((0.0, 0.0));

        // Borrow the page we just stored for composition
        let Some(ref page) = self.tab_manager.get_active_tab_page() else {
            return;
        };
        let Some(ref mut renderer) = self.renderer else {
            return;
        };
        let view_w = page.view_width;
        let view_h = page.view_height;

        // Collect render rectangles from layout tree and convert to clip space
        let rects = page.collect_rects();
        let clip_rects: Vec<_> = rects
            .into_iter()
            .take(MAX_RECTS)
            .filter_map(|(mut r, c)| {
                // Apply scroll offset by shifting rect position
                r.x -= scroll_offset.0;
                r.y -= scroll_offset.1;
                if r.width > 0.0 && r.height > 0.0 {
                    Some((
                        layout_to_clip(r.x, r.y, r.width, r.height, view_w, view_h),
                        c,
                    ))
                } else {
                    None
                }
            })
            .collect();

        let render_rects: Vec<_> = clip_rects.iter().map(|(r, _)| *r).collect();
        let render_colors: Vec<_> = clip_rects
            .iter()
            .map(|(_, c)| {
                if let Some(col) = c {
                    crate::render::color_u8_to_f32(*col)
                } else {
                    ColorF {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }
                }
            })
            .collect();

        renderer.set_rects(&render_rects, &render_colors);

        // Collect text and image nodes from layout tree
        let text_nodes = crate::layout::collect_text_nodes(&page.layout_root);
        let image_nodes = crate::layout::collect_image_nodes(&page.layout_root);

        if !text_nodes.is_empty() || !image_nodes.is_empty() {
            let view_width = page.view_width as u32;
            let view_height = page.view_height as u32;

            // Allocate RGBA buffer (transparent background)
            let mut composite_buffer = vec![0u8; (view_width * view_height * 4) as usize];

            // Rasterize text nodes into the composite buffer
            if !text_nodes.is_empty() {
                let mut text_renderer = TextRenderer::new();

                for text_info in &text_nodes {
                    let color_f32: [f32; 4] = [
                        text_info.color[0] as f32 / 255.0,
                        text_info.color[1] as f32 / 255.0,
                        text_info.color[2] as f32 / 255.0,
                        text_info.color[3] as f32 / 255.0,
                    ];

                    // Apply scroll offset to text position too
                    let text_x = text_info.x - scroll_offset.0;
                    let text_y = text_info.y - scroll_offset.1;

                    text_renderer.rasterize_to_bitmap(
                        &text_info.text,
                        text_info.font_size,
                        "sans-serif",
                        color_f32,
                        text_x,
                        text_y,
                        text_info.width,
                        &mut composite_buffer,
                        view_width,
                        view_height,
                    );
                }

                log::info!(
                    "Rasterized {} text nodes at {}x{}",
                    text_nodes.len(),
                    view_width,
                    view_height
                );
            }

            // Resolve image URLs against base_url before concurrent fetching
            let resolved_srcs: Vec<String> = image_nodes
                .iter()
                .map(|img| {
                    if let Some(base) = base_url {
                        crate::network::resolve_url(base, &img.src)
                    } else {
                        img.src.clone()
                    }
                })
                .collect();

            // Build concurrent fetch futures
            let fetch_futures =
                resolved_srcs
                    .iter()
                    .zip(image_nodes.iter())
                    .map(|(src, img_info)| {
                        let src_clone = src.clone();
                        let img_x = img_info.x - scroll_offset.0;
                        let img_y = img_info.y - scroll_offset.1;
                        async move {
                            match crate::network::fetch_image(&src_clone).await {
                                Ok(bytes) => {
                                    if let Ok(img) = image::load_from_memory(&bytes) {
                                        let rgba = img.to_rgba8();
                                        let (iw, ih) = rgba.dimensions();
                                        log::info!("Decoded image: {} ({}x{})", src_clone, iw, ih);
                                        Some((src_clone, rgba.into_raw(), iw, ih, img_x, img_y))
                                    } else {
                                        log::warn!("Failed to decode image: {}", src_clone);
                                        None
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Failed to fetch image {}: {:?}", src_clone, e);
                                    None
                                }
                            }
                        }
                    });

            let results = futures::future::join_all(fetch_futures).await;

            // Composite all successful images and collect for caching
            let mut items_to_cache = Vec::new();
            for result in results.into_iter().flatten() {
                let (src, pixels, iw, ih, ix, iy) = result;
                crate::render::composite_image(
                    &pixels,
                    iw,
                    ih,
                    &mut composite_buffer,
                    view_width,
                    view_height,
                    ix,
                    iy,
                );
                items_to_cache.push((src, pixels, iw, ih));
            }

            // Write to page image cache
            if let Some(tab) = self.tab_manager.active_tab_mut() {
                if let Some(ref mut page) = tab.page {
                    for (src, rgba, width, height) in items_to_cache {
                        page.image_cache.insert(
                            src,
                            crate::page::CachedImage {
                                rgba,
                                width,
                                height,
                            },
                        );
                    }
                }
            }

            // Upload the composite bitmap to GPU
            renderer.set_text_bitmap(view_width, view_height, &composite_buffer);
            log::info!(
                "Composite overlay uploaded: {} text + {} images at {}x{}",
                text_nodes.len(),
                image_nodes.len(),
                view_width,
                view_height
            );
        }

        if let Err(e) = renderer.render() {
            log::error!("Render after load_page_async failed: {:?}", e);
        } else {
            log::info!("Page loaded (async) and rendered");
        }

        // Memory profiling (only when memprof feature is enabled)
        #[cfg(feature = "memprof")]
        {
            if let Some(ref page) = self.tab_manager.get_active_tab_page() {
                let comp_size = (page.view_width as usize)
                    .saturating_mul(page.view_height as usize)
                    .saturating_mul(4);
                let profile = page.profile(comp_size);
                log::info!("{}", profile.summary());
            }
        }
    }

    /// Get the current window width, or default 1280.
    fn window_width(&self) -> u32 {
        self.renderer
            .as_ref()
            .map(|r| r.window().inner_size().width)
            .unwrap_or(1280)
    }

    /// Get the current window height, or default 800.
    fn window_height(&self) -> u32 {
        self.renderer
            .as_ref()
            .map(|r| r.window().inner_size().height)
            .unwrap_or(800)
    }

    /// Pushes rectangle(s) for a single tab button into the rects/colors vectors.
    fn push_tab_button_rects(
        rects: &mut Vec<RectClip>,
        colors: &mut Vec<ColorF>,
        tab: &crate::browser::tab::Tab,
        active_id: Option<crate::browser::tab::TabId>,
        hovered_id: Option<crate::browser::tab::TabId>,
        y: f32,
        window_width: u32,
        window_height: u32,
        group_color: Option<(f32, f32, f32)>,
    ) {
        let is_active = active_id == Some(tab.id);
        let is_hovered = hovered_id == Some(tab.id);
        // Color priority: active > hovered > inactive
        let (r, g, b) = if is_active {
            (100.0 / 255.0, 100.0 / 255.0, 150.0 / 255.0)
        } else if is_hovered {
            (80.0 / 255.0, 80.0 / 255.0, 120.0 / 255.0)
        } else {
            (60.0 / 255.0, 60.0 / 255.0, 65.0 / 255.0)
        };

        rects.push(layout_to_clip(
            TAB_BUTTON_X,
            y,
            TAB_BAR_WIDTH as f32 - TAB_BUTTON_X - TAB_BUTTON_RIGHT_MARGIN,
            TAB_BUTTON_HEIGHT,
            window_width as f32,
            window_height as f32,
        ));
        colors.push(ColorF { r, g, b, a: 255.0 });

        // Colored strip on left for grouped tabs
        if let Some((sr, sg, sb)) = group_color {
            rects.push(layout_to_clip(
                TAB_BUTTON_X,
                y,
                TAB_GROUP_COLOR_STRIP_WIDTH,
                TAB_BUTTON_HEIGHT,
                window_width as f32,
                window_height as f32,
            ));
            colors.push(ColorF {
                r: sr,
                g: sg,
                b: sb,
                a: 255.0,
            });
        }
    }

    /// Hit-test the tab bar area accounting for group headers and collapsed groups.
    fn hit_test_tab_bar(&self, x: f32, y: f32) -> HitTestResult {
        let mut button_y = ADDRESS_BAR_HEIGHT as f32 + TAB_BUTTON_SPACING;

        // Check group headers first, then visible tabs
        for group in self.group_manager.all_groups() {
            // Check if click is on the group header
            if x >= TAB_BUTTON_X
                && x <= (TAB_BAR_WIDTH as f32 - TAB_BUTTON_RIGHT_MARGIN)
                && y >= button_y
                && y <= button_y + GROUP_HEADER_HEIGHT
            {
                return HitTestResult::GroupHeader(group.id);
            }
            button_y += GROUP_HEADER_HEIGHT + TAB_BUTTON_SPACING;

            // Check tabs in this group (if not collapsed)
            if !group.collapsed {
                for tab_id in &group.tab_ids {
                    if let Some(tab) = self.tab_manager.get_tab(*tab_id) {
                        if x >= TAB_BUTTON_X
                            && x <= (TAB_BAR_WIDTH as f32 - TAB_BUTTON_RIGHT_MARGIN)
                            && y >= button_y
                            && y <= button_y + TAB_BUTTON_HEIGHT
                        {
                            return HitTestResult::TabButton(tab.id);
                        }
                        button_y += TAB_BUTTON_HEIGHT + TAB_BUTTON_SPACING;
                    }
                }
            }

            button_y += TAB_BUTTON_SPACING; // extra spacing after each group
        }

        // Check ungrouped tabs
        for tab in self.tab_manager.all_tabs() {
            if tab.group_id.is_none() {
                if x >= TAB_BUTTON_X
                    && x <= (TAB_BAR_WIDTH as f32 - TAB_BUTTON_RIGHT_MARGIN)
                    && y >= button_y
                    && y <= button_y + TAB_BUTTON_HEIGHT
                {
                    return HitTestResult::TabButton(tab.id);
                }
                button_y += TAB_BUTTON_HEIGHT + TAB_BUTTON_SPACING;
            }
        }

        HitTestResult::Empty
    }

    /// Check if a position falls within the page content area (not on chrome).
    fn is_in_content_area(&self, x: f32, y: f32) -> bool {
        x >= TAB_BAR_WIDTH as f32 && y >= ADDRESS_BAR_HEIGHT as f32
    }

    /// Create a default tab group and assign the active tab to it.
    fn create_group_for_active_tab(&mut self) {
        if let Some(active_id) = self.tab_manager.active_tab_id() {
            use crate::browser::tab_group::GroupColor;
            // Cycle through colors based on existing group count
            let color_count = GroupColor::variants().len();
            let color_idx = self.group_manager.all_groups().count() % color_count;
            let color = GroupColor::variants()[color_idx];

            let group_name = format!("Group {}", self.group_manager.all_groups().count() + 1);
            let group_id = self.group_manager.create_group(group_name.clone(), color);

            // Assign active tab to this group
            self.tab_manager.assign_to_group(active_id, group_id);
            self.group_manager.add_tab_to_group(group_id, active_id);

            log::info!(
                "Created group '{}' with color {:?} for active tab",
                group_name,
                color
            );
            self.recompose();
        }
    }

    /// Sets the winit window cursor icon from a CursorKind value.
    #[allow(deprecated)]
    fn set_winit_cursor(renderer: Option<&Renderer>, kind: CursorKind) {
        if let Some(r) = renderer {
            r.window().set_cursor_icon(match kind {
                CursorKind::Default => CursorIcon::Default,
                CursorKind::Pointer => CursorIcon::Pointer,
                CursorKind::IBeam => CursorIcon::Text,
            });
        }
    }
}

impl ApplicationHandler for MistilteinnApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_none() {
            // Initialize tab manager and create the first tab
            self.tab_manager = crate::browser::tab::TabManager::new();
            self.group_manager = crate::browser::tab_group::GroupManager::new();
            self.cursor_pos = (0.0, 0.0);
            self.ctrl_pressed = false;
            self.hovered_tab_id = None;
            self.hovered_address_bar = false;
            self.prev_hovered_dom_id = None;
            self.tab_manager.create_tab();

            let window = event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title("Mistilteinn")
                        .with_inner_size(winit::dpi::PhysicalSize::new(1280, 800)),
                )
                .expect("Failed to create window");

            log::info!("Window created (1280x800)");

            // Use tokio runtime to run the async wgpu initialization.
            // wgpu requires an async runtime for adapter/device requests.
            let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
            self.tokio_handle = Some(rt.handle().clone());
            let renderer = rt.block_on(async {
                match Renderer::new(window).await {
                    Ok(renderer) => Some(renderer),
                    Err(e) => {
                        log::error!("Failed to initialize renderer: {}", e);
                        None
                    }
                }
            });
            self.renderer = renderer;

            // Load startup URL or default demo page through the pipeline
            if self.renderer.is_some() {
                if let Some(url) = self.start_url.take() {
                    self.load_url(&url);
                    log::info!("Loaded startup URL from MISTILTEIN_URL: {}", url);
                } else {
                    self.load_page(
                        r#"<html><body>
                            <div id="header" class="header">Header</div>
                            <div class="content">
                                <p class="box green">Green box</p>
                                <p class="box red">Red box</p>
                            </div>
                            <div id="footer" class="footer">Footer</div>
                          </body></html>"#,
                        r#".header { display: block; background-color: blue; padding: 20px; }
                           .content { display: block; }
                           .box { display: block; padding: 15px; }
                           .green { background-color: green; }
                           .red { background-color: red; }
                           .footer { display: block; background-color: orange; padding: 10px; }"#,
                    );
                    log::info!("First frame rendered — pipeline output (HTML→CSS→Layout→Render)");
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("Window close requested, exiting");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return; // Ignore spurious resize events
                }
                if let Some(ref mut renderer) = self.renderer {
                    renderer.resize(size.width, size.height);
                }
                // Trigger recompose with new view dimensions
                if let Some(tab) = self.tab_manager.active_tab_mut() {
                    if let Some(ref mut page) = tab.page {
                        page.view_width = size.width as f32 - TAB_BAR_WIDTH as f32;
                        page.view_height = size.height as f32 - ADDRESS_BAR_HEIGHT as f32;
                        // Rebuild layout positions with new view width
                        let mut text_renderer = TextRenderer::new();
                        crate::layout::compute_layout(
                            &mut page.layout_root,
                            page.view_width,
                            &mut text_renderer,
                        );
                    }
                }
                self.recompose();
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(scroll) = self.tab_manager.get_active_tab_scroll_mut() {
                    match delta {
                        winit::event::MouseScrollDelta::LineDelta(_dx, dy) => {
                            *scroll = (scroll.0, (scroll.1 + (dy * 30.0)).max(0.0));
                        }
                        winit::event::MouseScrollDelta::PixelDelta(pos) => {
                            *scroll = (scroll.0, (scroll.1 + pos.y as f32).max(0.0));
                        }
                    }
                }
                self.recompose();
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let (cx, cy) = self.cursor_pos;

                match button {
                    MouseButton::Left => {
                        if state == ElementState::Pressed {
                            // Hit-test the tab bar first
                            if cx < TAB_BAR_WIDTH as f32 && cy > 0.0 {
                                match self.hit_test_tab_bar(cx, cy) {
                                    HitTestResult::GroupHeader(group_id) => {
                                        // Toggle group collapse
                                        if let Some(is_now_collapsed) =
                                            self.group_manager.toggle_collapse(group_id)
                                        {
                                            log::info!(
                                                "Toggled group {:?} collapsed={}",
                                                group_id,
                                                is_now_collapsed
                                            );
                                            self.recompose();
                                            if let Some(ref renderer) = self.renderer {
                                                renderer.window().request_redraw();
                                            }
                                            return;
                                        }
                                    }
                                    HitTestResult::TabButton(tab_id) => {
                                        self.tab_manager.activate_tab(tab_id);
                                        self.recompose();
                                        log::info!("Activated tab {:?}", tab_id);
                                    }
                                    HitTestResult::Empty => {}
                                }
                            }
                        }
                    }
                    MouseButton::Right => {
                        // Right-click on a tab closes it
                        if state == ElementState::Pressed && cx < TAB_BAR_WIDTH as f32 {
                            if let Some(clicked_tab) = self.hit_test_tab_bar(cx, cy).into_tab_id() {
                                if self.tab_manager.active_tab_id() != Some(clicked_tab) {
                                    // Remove from group if assigned
                                    if let Some(tab) = self.tab_manager.get_tab(clicked_tab) {
                                        if let Some(gid) = tab.group_id {
                                            self.group_manager
                                                .remove_tab_from_group(gid, clicked_tab);
                                        }
                                    }
                                    self.tab_manager.close_tab(clicked_tab);
                                    self.recompose();
                                    log::info!("Closed tab {:?}", clicked_tab);
                                }
                            }
                        }
                    }
                    _ => {}
                }

                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cx = position.x as f32;
                let cy = position.y as f32;
                self.cursor_pos = (cx, cy);

                // Hit-test tab bar for hover highlight
                let new_hovered_tab = if cx < TAB_BAR_WIDTH as f32 && cy > 0.0 {
                    match self.hit_test_tab_bar(cx, cy) {
                        HitTestResult::TabButton(id) => Some(id),
                        _ => None,
                    }
                } else {
                    None
                };

                let tab_changed = self.hovered_tab_id != new_hovered_tab;
                self.hovered_tab_id = new_hovered_tab;

                // Hit-test address bar for hover highlight
                let new_hovered_addr = cx >= TAB_BAR_WIDTH as f32 && cy < ADDRESS_BAR_HEIGHT as f32;
                let addr_changed = self.hovered_address_bar != new_hovered_addr;
                self.hovered_address_bar = new_hovered_addr;

                if tab_changed || addr_changed {
                    self.recompose();
                    if let Some(ref renderer) = self.renderer {
                        renderer.window().request_redraw();
                    }
                }

                // Hit-test content area for interactive elements (links, inputs) to change cursor
                // and compute :hover state. Extract all info first to avoid borrow conflicts.
                let (content_cursor, hovered_dom_path) = if self.is_in_content_area(cx, cy) {
                    if let Some(ref page) = self.tab_manager.get_active_tab_page() {
                        let scroll_offset = self
                            .tab_manager
                            .get_active_tab_scroll()
                            .unwrap_or((0.0, 0.0));
                        // Adjust coordinates: remove chrome offset, add scroll offset
                        let content_x = cx - TAB_BAR_WIDTH as f32 + scroll_offset.0;
                        let content_y = cy - ADDRESS_BAR_HEIGHT as f32 + scroll_offset.1;

                        let cursor_kind = crate::layout::hit_test_interactive(
                            &page.layout_root,
                            content_x,
                            content_y,
                        )
                        .map(|interaction| match interaction {
                            crate::layout::InteractionType::Link => CursorKind::Pointer,
                            crate::layout::InteractionType::Input => CursorKind::IBeam,
                            _ => CursorKind::Default,
                        })
                        .unwrap_or(CursorKind::Default);

                        // :hover — hit-test for DOM node path (includes ancestors)
                        let dom_path = crate::layout::hit_test_dom_path(
                            &page.layout_root,
                            content_x,
                            content_y,
                        );

                        (Some(cursor_kind), Some(dom_path))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };

                // Apply cursor change
                if let Some(kind) = content_cursor {
                    Self::set_winit_cursor(self.renderer.as_ref(), kind);
                } else {
                    Self::set_winit_cursor(self.renderer.as_ref(), CursorKind::Default);
                }

                // Recompute :hover styles if the hovered DOM node changed
                if let Some(hovered_dom_path) = hovered_dom_path {
                    let new_hovered_id = hovered_dom_path.last().copied();
                    let hover_changed = new_hovered_id != self.prev_hovered_dom_id;
                    self.prev_hovered_dom_id = new_hovered_id;

                    if hover_changed {
                        if let Some(tab) = self.tab_manager.active_tab_mut() {
                            if let Some(ref mut pg) = tab.page {
                                pg.recompute_with_hover(&hovered_dom_path);
                            }
                        }
                        self.recompose();
                        if let Some(ref renderer) = self.renderer {
                            renderer.window().request_redraw();
                        }
                    }
                } else {
                    // Outside content area: clear hover state if it was set
                    if self.prev_hovered_dom_id.is_some() {
                        self.prev_hovered_dom_id = None;
                        if let Some(tab) = self.tab_manager.active_tab_mut() {
                            if let Some(ref mut pg) = tab.page {
                                pg.recompute_with_hover(&[]);
                            }
                        }
                        self.recompose();
                        if let Some(ref renderer) = self.renderer {
                            renderer.window().request_redraw();
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(ref mut renderer) = self.renderer
                    && let Err(e) = renderer.render()
                {
                    log::error!("Render error: {:?}", e);
                }
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::ControlLeft)
                    || event.physical_key == PhysicalKey::Code(KeyCode::ControlRight)
                {
                    self.ctrl_pressed = event.state == ElementState::Pressed;
                    return;
                }
                if event.state == ElementState::Pressed && self.ctrl_pressed {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Tab) => {
                            // Ctrl+Tab: switch to next tab
                            let tabs: Vec<_> = self.tab_manager.all_tabs().map(|t| t.id).collect();
                            if let Some(current) = self.tab_manager.active_tab_id() {
                                let idx = tabs.iter().position(|&id| id == current).unwrap_or(0);
                                let next_idx = (idx + 1) % tabs.len();
                                self.tab_manager.activate_tab(tabs[next_idx]);
                                self.recompose();
                                log::info!("Switched to tab {:?}", tabs[next_idx]);
                            }
                        }
                        PhysicalKey::Code(KeyCode::KeyG) => {
                            // Ctrl+G: create group for active tab
                            self.create_group_for_active_tab();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

/// Application entry point.
pub fn run(start_url: Option<String>) {
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = MistilteinnApp {
        renderer: None,
        start_url,
        tab_manager: crate::browser::tab::TabManager::new(),
        group_manager: crate::browser::tab_group::GroupManager::new(),
        tokio_handle: None,
        cursor_pos: (0.0, 0.0),
        ctrl_pressed: false,
        hovered_tab_id: None,
        hovered_address_bar: false,
        prev_hovered_dom_id: None,
    };
    event_loop.run_app(&mut app).expect("Event loop failed");
}
