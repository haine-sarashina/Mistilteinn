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
const TAB_BUTTON_HEIGHT: f32 = 40.0;
const TAB_BUTTON_SPACING: f32 = 4.0;
const NEW_TAB_BUTTON_HEIGHT: f32 = 28.0;
const CLOSE_BUTTON_SIZE: f32 = 18.0;
const NAV_BUTTON_WIDTH: f32 = 36.0;
const GROUP_HEADER_HEIGHT: f32 = 28.0;
const TAB_BUTTON_X: f32 = 8.0;
const TAB_BUTTON_RIGHT_MARGIN: f32 = 16.0;
const TAB_GROUP_COLOR_STRIP_WIDTH: f32 = 4.0;
const LOADING_BAR_HEIGHT: f32 = 3.0;
const LOADING_BAR_COLOR_R: f32 = 80.0 / 255.0;
const LOADING_BAR_COLOR_G: f32 = 140.0 / 255.0;
const LOADING_BAR_COLOR_B: f32 = 230.0 / 255.0;
const SCROLLBAR_WIDTH: f32 = 10.0;
const SCROLLBAR_MIN_THUMB_HEIGHT: f32 = 30.0;

/// Main application struct implementing winit's ApplicationHandler trait.
pub struct MistilteinnApp {
    renderer: Option<Renderer>,
    pub start_url: Option<String>,
    tab_manager: crate::browser::tab::TabManager,
    group_manager: crate::browser::tab_group::GroupManager,
    tokio_rt: Option<tokio::runtime::Runtime>,
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
    /// The current URL in the address bar.
    address_input: String,
    /// Whether the address bar is focused.
    is_address_focused: bool,
    /// The cursor position in the address input.
    address_cursor: usize,
    /// The DOM node ID of the focused page <input> / <textarea>, if any.
    focused_page_input: Option<u32>,
    /// The DOM node ID of the `<select>` whose option list is open, if any.
    open_select: Option<u32>,
    /// Whether the user is actively dragging the scrollbar thumb.
    is_dragging_scrollbar: bool,
    /// The cursor Y position when scrollbar dragging started.
    scrollbar_drag_start_y: f32,
    /// The scroll Y offset when scrollbar dragging started.
    scrollbar_drag_start_scroll_y: f32,
    /// Whether the scrollbar is currently hovered by cursor.
    hovered_scrollbar: bool,
}

/// Hit-test result for tab bar clicks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum HitTestResult {
    /// Clicked on a group header (toggle collapse)
    GroupHeader(crate::browser::tab_group::GroupId),
    /// Clicked on a tab button
    TabButton(crate::browser::tab::TabId),
    /// Clicked on close button ('×') of a tab
    CloseTabButton(crate::browser::tab::TabId),
    /// Clicked on '+ New Tab' button
    NewTabButton,
    /// Clicked on empty space in tab bar
    Empty,
    /// Clicked on Address Bar
    AddressBar,
    /// Clicked on Back Button
    BackButton,
    /// Clicked on Forward Button
    ForwardButton,
    /// Clicked on Reload Button
    ReloadButton,
    /// Clicked on Scrollbar Thumb
    ScrollbarThumb,
    /// Clicked on Scrollbar Track
    ScrollbarTrack,
}

impl HitTestResult {
    fn into_tab_id(self) -> Option<crate::browser::tab::TabId> {
        match self {
            HitTestResult::TabButton(id) => Some(id),
            _ => None,
        }
    }
}

/// Height of one row in an open `<select>` list.
const SELECT_OPTION_HEIGHT: f32 = 20.0;

/// The most rows an open `<select>` shows before it stops growing.
const SELECT_MAX_VISIBLE_OPTIONS: usize = 12;

/// Where an open `<select>`'s option list is drawn, in screen coordinates.
///
/// The list hangs below the control and is at least as wide as it. A long list
/// is capped rather than running off the window; the options past the cap are
/// simply not reachable yet, which is better than drawing over the whole page.
fn select_popup_geometry(
    select_x: f32,
    select_y: f32,
    select_width: f32,
    select_height: f32,
    option_count: usize,
) -> crate::layout::Rect {
    let visible = option_count.min(SELECT_MAX_VISIBLE_OPTIONS);
    crate::layout::Rect::new(
        select_x,
        select_y + select_height,
        select_width.max(64.0),
        visible as f32 * SELECT_OPTION_HEIGHT + 2.0,
    )
}

/// Which option row a click at `y` lands on, if any.
fn select_option_at(
    popup: &crate::layout::Rect,
    x: f32,
    y: f32,
    option_count: usize,
) -> Option<usize> {
    if x < popup.x || x > popup.right() || y < popup.y || y > popup.bottom() {
        return None;
    }
    let index = ((y - popup.y - 1.0) / SELECT_OPTION_HEIGHT).floor();
    if index < 0.0 {
        return None;
    }
    let index = index as usize;
    if index < option_count.min(SELECT_MAX_VISIBLE_OPTIONS) {
        Some(index)
    } else {
        None
    }
}

/// Map a computed CSS `cursor` onto a winit cursor icon.
///
/// `Auto` and `None` have no icon of their own — the caller resolves `Auto`
/// from the element's role first, and winit hides the cursor separately, so
/// both land on the plain arrow here.
fn cursor_icon_for(cursor: crate::css::Cursor) -> CursorIcon {
    use crate::css::Cursor as C;
    match cursor {
        C::Auto | C::Default | C::None => CursorIcon::Default,
        C::Pointer => CursorIcon::Pointer,
        C::Text => CursorIcon::Text,
        C::Crosshair => CursorIcon::Crosshair,
        C::Move => CursorIcon::Move,
        C::Grab => CursorIcon::Grab,
        C::Grabbing => CursorIcon::Grabbing,
        C::NotAllowed => CursorIcon::NotAllowed,
        C::Progress => CursorIcon::Progress,
        C::Wait => CursorIcon::Wait,
        C::Help => CursorIcon::Help,
        C::ColResize => CursorIcon::ColResize,
        C::RowResize => CursorIcon::RowResize,
        C::NResize => CursorIcon::NResize,
        C::EResize => CursorIcon::EResize,
        C::SResize => CursorIcon::SResize,
        C::WResize => CursorIcon::WResize,
        C::NeResize => CursorIcon::NeResize,
        C::NwResize => CursorIcon::NwResize,
        C::SeResize => CursorIcon::SeResize,
        C::SwResize => CursorIcon::SwResize,
        C::EwResize => CursorIcon::EwResize,
        C::NsResize => CursorIcon::NsResize,
        C::ZoomIn => CursorIcon::ZoomIn,
        C::ZoomOut => CursorIcon::ZoomOut,
    }
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
            a: 1.0,
        });

        // + New Tab button at top of Tab Bar
        let new_tab_btn_y = 6.0;
        rects.push(layout_to_clip(
            TAB_BUTTON_X,
            new_tab_btn_y,
            TAB_BAR_WIDTH as f32 - TAB_BUTTON_X - TAB_BUTTON_RIGHT_MARGIN,
            NEW_TAB_BUTTON_HEIGHT,
            window_width as f32,
            window_height as f32,
        ));
        colors.push(ColorF {
            r: 55.0 / 255.0,
            g: 55.0 / 255.0,
            b: 65.0 / 255.0,
            a: 1.0,
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
                a: 1.0,
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
                a: 1.0,
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

        // Address bar area background — right of tab bar, top
        rects.push(layout_to_clip(
            TAB_BAR_WIDTH as f32,
            0.0,
            window_width as f32 - TAB_BAR_WIDTH as f32,
            ADDRESS_BAR_HEIGHT as f32,
            window_width as f32,
            window_height as f32,
        ));
        colors.push(ColorF {
            r: 45.0 / 255.0,
            g: 45.0 / 255.0,
            b: 50.0 / 255.0,
            a: 1.0,
        });

        // Navigation buttons (Back, Forward, Reload)
        let mut curr_x = TAB_BAR_WIDTH as f32;

        // Back button
        rects.push(layout_to_clip(
            curr_x,
            0.0,
            NAV_BUTTON_WIDTH,
            ADDRESS_BAR_HEIGHT as f32,
            window_width as f32,
            window_height as f32,
        ));
        colors.push(ColorF {
            r: 55.0 / 255.0,
            g: 55.0 / 255.0,
            b: 60.0 / 255.0,
            a: 1.0,
        });
        curr_x += NAV_BUTTON_WIDTH;

        // Forward button
        rects.push(layout_to_clip(
            curr_x,
            0.0,
            NAV_BUTTON_WIDTH,
            ADDRESS_BAR_HEIGHT as f32,
            window_width as f32,
            window_height as f32,
        ));
        colors.push(ColorF {
            r: 55.0 / 255.0,
            g: 55.0 / 255.0,
            b: 60.0 / 255.0,
            a: 1.0,
        });
        curr_x += NAV_BUTTON_WIDTH;

        // Reload button
        rects.push(layout_to_clip(
            curr_x,
            0.0,
            NAV_BUTTON_WIDTH,
            ADDRESS_BAR_HEIGHT as f32,
            window_width as f32,
            window_height as f32,
        ));
        colors.push(ColorF {
            r: 55.0 / 255.0,
            g: 55.0 / 255.0,
            b: 60.0 / 255.0,
            a: 1.0,
        });
        curr_x += NAV_BUTTON_WIDTH;

        // Inner URL Input box
        let addr_box_margin = 6.0;
        let addr_box_x = curr_x + addr_box_margin;
        let addr_box_w = window_width as f32 - addr_box_x - addr_box_margin;
        rects.push(layout_to_clip(
            addr_box_x,
            addr_box_margin,
            addr_box_w,
            ADDRESS_BAR_HEIGHT as f32 - addr_box_margin * 2.0,
            window_width as f32,
            window_height as f32,
        ));
        let (addr_r, addr_g, addr_b) = if self.is_address_focused {
            (1.0, 1.0, 1.0) // White when focused
        } else if self.hovered_address_bar {
            (75.0 / 255.0, 75.0 / 255.0, 85.0 / 255.0)
        } else {
            (65.0 / 255.0, 65.0 / 255.0, 70.0 / 255.0)
        };
        colors.push(ColorF {
            r: addr_r,
            g: addr_g,
            b: addr_b,
            a: 1.0,
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
                a: 1.0,
            });
        }

        (rects, colors)
    }

    fn draw_chrome_text(
        &self,
        text_renderer: &mut TextRenderer,
        buffer: &mut [u8],
        win_w: u32,
        win_h: u32,
    ) {
        let text_color = [0.9, 0.9, 0.9, 1.0]; // Light gray/white

        // Draw + New Tab button text
        text_renderer.rasterize_to_bitmap(
            "+ New Tab",
            13.0,
            "sans-serif",
            [0.85, 0.85, 0.9, 1.0],
            TAB_BUTTON_X + 45.0,
            12.0,
            TAB_BAR_WIDTH as f32 - 60.0,
            buffer,
            win_w,
            win_h,
        );

        // Draw tab titles and close '×' buttons
        let mut y = ADDRESS_BAR_HEIGHT as f32 + TAB_BUTTON_SPACING;
        for group in self.group_manager.all_groups() {
            y += GROUP_HEADER_HEIGHT + TAB_BUTTON_SPACING;
            if !group.collapsed {
                for tab_id in &group.tab_ids {
                    if let Some(tab) = self.tab_manager.get_tab(*tab_id) {
                        let title = if tab.title.is_empty() {
                            "New Tab"
                        } else {
                            &tab.title
                        };
                        text_renderer.rasterize_to_bitmap(
                            title,
                            13.0,
                            "sans-serif",
                            text_color,
                            TAB_BUTTON_X + 12.0,
                            y + 11.0,
                            TAB_BAR_WIDTH as f32 - TAB_BUTTON_X - TAB_BUTTON_RIGHT_MARGIN - 30.0,
                            buffer,
                            win_w,
                            win_h,
                        );
                        // Close '×' button
                        text_renderer.rasterize_to_bitmap(
                            "×",
                            15.0,
                            "sans-serif",
                            [0.7, 0.7, 0.75, 1.0],
                            TAB_BAR_WIDTH as f32 - TAB_BUTTON_RIGHT_MARGIN - 16.0,
                            y + 9.0,
                            20.0,
                            buffer,
                            win_w,
                            win_h,
                        );
                        y += TAB_BUTTON_HEIGHT + TAB_BUTTON_SPACING;
                    }
                }
            }
            y += TAB_BUTTON_SPACING;
        }
        for tab in self.tab_manager.all_tabs() {
            if tab.group_id.is_none() {
                let title = if tab.title.is_empty() {
                    "New Tab"
                } else {
                    &tab.title
                };
                text_renderer.rasterize_to_bitmap(
                    title,
                    13.0,
                    "sans-serif",
                    text_color,
                    TAB_BUTTON_X + 12.0,
                    y + 11.0,
                    TAB_BAR_WIDTH as f32 - TAB_BUTTON_X - TAB_BUTTON_RIGHT_MARGIN - 30.0,
                    buffer,
                    win_w,
                    win_h,
                );
                // Close '×' button
                text_renderer.rasterize_to_bitmap(
                    "×",
                    15.0,
                    "sans-serif",
                    [0.7, 0.7, 0.75, 1.0],
                    TAB_BAR_WIDTH as f32 - TAB_BUTTON_RIGHT_MARGIN - 16.0,
                    y + 9.0,
                    20.0,
                    buffer,
                    win_w,
                    win_h,
                );
                y += TAB_BUTTON_HEIGHT + TAB_BUTTON_SPACING;
            }
        }

        // Draw Navigation buttons (Back, Forward, Reload)
        let mut curr_x = TAB_BAR_WIDTH as f32;

        // Back button
        text_renderer.rasterize_to_bitmap(
            "◀",
            18.0,
            "sans-serif",
            text_color,
            curr_x + 9.0,
            9.0,
            30.0,
            buffer,
            win_w,
            win_h,
        );
        curr_x += NAV_BUTTON_WIDTH;

        // Forward button
        text_renderer.rasterize_to_bitmap(
            "▶",
            18.0,
            "sans-serif",
            text_color,
            curr_x + 9.0,
            9.0,
            30.0,
            buffer,
            win_w,
            win_h,
        );
        curr_x += NAV_BUTTON_WIDTH;

        // Reload button
        text_renderer.rasterize_to_bitmap(
            "↻",
            18.0,
            "sans-serif",
            text_color,
            curr_x + 9.0,
            8.0,
            30.0,
            buffer,
            win_w,
            win_h,
        );
        curr_x += NAV_BUTTON_WIDTH;

        // Draw Address input
        let addr_box_margin = 6.0;
        let addr_box_x = curr_x + addr_box_margin;

        // Show cursor if focused
        let display_text = if self.is_address_focused {
            let mut t = self.address_input.clone();
            if self.address_cursor <= t.len() {
                t.insert(self.address_cursor, '|');
            } else {
                t.push('|');
            }
            t
        } else {
            if self.address_input.is_empty() {
                self.tab_manager
                    .get_active_tab_page()
                    .map(|p| p.page_url.clone())
                    .unwrap_or_default()
            } else {
                self.address_input.clone()
            }
        };

        let addr_color = if self.is_address_focused {
            [0.1, 0.1, 0.1, 1.0]
        } else {
            [0.8, 0.8, 0.8, 1.0]
        };

        text_renderer.rasterize_to_bitmap(
            &display_text,
            16.0,
            "sans-serif",
            addr_color,
            addr_box_x + 10.0,
            10.0,
            win_w as f32 - addr_box_x - 20.0,
            buffer,
            win_w,
            win_h,
        );
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
            // No active page: render blank white canvas with chrome overlay
            let mut all_rects: Vec<RectClip> = chrome_rects;
            let mut all_colors: Vec<ColorF> = chrome_colors;

            // Add white background for content area
            all_rects.push(layout_to_clip(
                TAB_BAR_WIDTH as f32,
                ADDRESS_BAR_HEIGHT as f32,
                win_w as f32 - TAB_BAR_WIDTH as f32,
                win_h as f32 - ADDRESS_BAR_HEIGHT as f32,
                win_w as f32,
                win_h as f32,
            ));
            all_colors.push(ColorF {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            });

            if let Some(ref mut renderer) = self.renderer {
                renderer.set_rects(&all_rects, &all_colors);
            }

            let mut composite_buffer = vec![0u8; (win_w * win_h * 4) as usize];
            let mut text_renderer = TextRenderer::new();
            self.draw_chrome_text(&mut text_renderer, &mut composite_buffer, win_w, win_h);
            if let Some(ref mut renderer) = self.renderer {
                renderer.set_text_bitmap(win_w, win_h, &composite_buffer);
            }
            return;
        };

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
        if let Some(ref mut renderer) = self.renderer {
            renderer.set_rects(&all_rects, &all_colors);
        }

        // Build the page's paint order once, then walk it in order. Painting
        // in three passes (all backgrounds, then all text, then all images)
        // put every positioned overlay underneath the text it should cover.
        let display_list = crate::layout::build_display_list(&page.layout_root);

        // Allocate full-window RGBA buffer (transparent background)
        let mut composite_buffer = vec![0u8; (win_w * win_h * 4) as usize];

        let page_base_url = page.base_url();
        let resolve_src = |src: &str| -> String {
            if page_base_url.is_empty() {
                src.to_string()
            } else {
                crate::network::resolve_url(&page_base_url, src)
            }
        };
        let lookup_image = |src: &str| {
            page.image_cache
                .get(&resolve_src(src))
                .or_else(|| page.image_cache.get(src))
        };

        let mut text_renderer = TextRenderer::new();

        for item in &display_list {
            match item {
                crate::layout::PaintItem::Decoration(deco) => {
                    let dx = deco.x - scroll_offset.0 + TAB_BAR_WIDTH as f32;
                    let dy = deco.y - scroll_offset.1 + ADDRESS_BAR_HEIGHT as f32;

                    if let Some(bg) = deco.background_color {
                        if deco.border_radius > 0.0 {
                            crate::render::draw_rounded_rect_fill(
                                &mut composite_buffer,
                                win_w,
                                win_h,
                                dx,
                                dy,
                                deco.width,
                                deco.height,
                                deco.border_radius,
                                bg,
                            );
                        } else {
                            crate::render::draw_solid_rect(
                                &mut composite_buffer,
                                win_w,
                                win_h,
                                dx,
                                dy,
                                deco.width,
                                deco.height,
                                bg,
                            );
                        }
                    }

                    // The image paints over the background colour and under the border.
                    if let Some(ref src) = deco.background_image {
                        if let Some(cached) = lookup_image(src) {
                            crate::render::draw_background_image(
                                &cached.rgba,
                                cached.width,
                                cached.height,
                                &mut composite_buffer,
                                win_w,
                                win_h,
                                dx,
                                dy,
                                deco.width,
                                deco.height,
                                deco.background_size,
                                deco.background_position,
                                deco.background_repeat,
                            );
                        }
                    }

                    crate::render::draw_rect_borders(
                        &mut composite_buffer,
                        win_w,
                        win_h,
                        dx,
                        dy,
                        deco.width,
                        deco.height,
                        deco.border_width,
                        deco.border_color,
                        deco.border_style,
                    );
                }

                crate::layout::PaintItem::Text(text_info) => {
                    let color_f32: [f32; 4] = [
                        text_info.color[0] as f32 / 255.0,
                        text_info.color[1] as f32 / 255.0,
                        text_info.color[2] as f32 / 255.0,
                        text_info.color[3] as f32 / 255.0,
                    ];

                    // Apply scroll offset and shift by chrome dimensions
                    let text_x = text_info.x - scroll_offset.0 + TAB_BAR_WIDTH as f32;
                    let text_y = text_info.y - scroll_offset.1 + ADDRESS_BAR_HEIGHT as f32;

                    text_renderer.rasterize_to_bitmap_styled(
                        &text_info.text,
                        text_info.font_size,
                        &text_info.font_family,
                        color_f32,
                        text_x,
                        text_y,
                        text_info.width,
                        text_info.text_style,
                        &mut composite_buffer,
                        win_w,
                        win_h,
                    );
                }

                crate::layout::PaintItem::Image(img_info) => {
                    let Some(cached) = lookup_image(&img_info.src) else {
                        continue;
                    };
                    // Skip unpositioned or collapsed small icons at (0, 0)
                    if img_info.x <= 0.0
                        && img_info.y <= 0.0
                        && img_info.width < 32.0
                        && img_info.height < 32.0
                    {
                        continue;
                    }
                    let img_x = img_info.x - scroll_offset.0 + TAB_BAR_WIDTH as f32;
                    let img_y = img_info.y - scroll_offset.1 + ADDRESS_BAR_HEIGHT as f32;
                    let target_w = if img_info.width >= 4.0 {
                        img_info.width
                    } else {
                        cached.width as f32
                    };
                    let target_h = if img_info.height >= 4.0 {
                        img_info.height
                    } else {
                        cached.height as f32
                    };
                    if target_w >= 4.0 && target_h >= 4.0 {
                        crate::render::composite_image_scaled(
                            &cached.rgba,
                            cached.width,
                            cached.height,
                            &mut composite_buffer,
                            win_w,
                            win_h,
                            img_x,
                            img_y,
                            target_w,
                            target_h,
                        );
                    }
                }
            }
        }

        // Highlight focused page input with blue outline
        if let Some(focused_dom_id) = self.focused_page_input {
            if let Some(rect) = find_layout_rect_by_dom_id(&page.layout_root, focused_dom_id) {
                let fx = rect.x - scroll_offset.0 + TAB_BAR_WIDTH as f32;
                let fy = rect.y - scroll_offset.1 + ADDRESS_BAR_HEIGHT as f32;
                crate::render::draw_rect_borders(
                    &mut composite_buffer,
                    win_w,
                    win_h,
                    fx - 1.0,
                    fy - 1.0,
                    rect.width + 2.0,
                    rect.height + 2.0,
                    [2.0, 2.0, 2.0, 2.0],
                    [[66, 133, 244, 255]; 4],
                    [crate::css::BorderStyle::Solid; 4],
                );
            }
        }

        // Draw the drop arrow on every <select>, and the option list of the one
        // that is open. Both go on top of the page, which is where a dropdown
        // belongs regardless of what the page's own stacking order says.
        let to_screen = |x: f32, y: f32| {
            (
                x - scroll_offset.0 + TAB_BAR_WIDTH as f32,
                y - scroll_offset.1 + ADDRESS_BAR_HEIGHT as f32,
            )
        };
        for select in crate::layout::collect_select_boxes(&page.layout_root) {
            let (sx, sy) = to_screen(select.x, select.y);
            crate::render::draw_select_arrow(
                &mut composite_buffer,
                win_w,
                win_h,
                sx,
                sy,
                select.width,
                select.height,
            );
        }

        if let Some(open_id) = self.open_select {
            if let Some(rect) = find_layout_rect_by_dom_id(&page.layout_root, open_id) {
                let options = page.select_options(open_id);
                let (sx, sy) = to_screen(rect.x, rect.y);
                let popup = select_popup_geometry(sx, sy, rect.width, rect.height, options.len());

                crate::render::draw_solid_rect(
                    &mut composite_buffer,
                    win_w,
                    win_h,
                    popup.x,
                    popup.y,
                    popup.width,
                    popup.height,
                    [255, 255, 255, 255],
                );
                crate::render::draw_rect_borders(
                    &mut composite_buffer,
                    win_w,
                    win_h,
                    popup.x,
                    popup.y,
                    popup.width,
                    popup.height,
                    [1.0; 4],
                    [[118, 118, 118, 255]; 4],
                    [crate::css::BorderStyle::Solid; 4],
                );

                for (i, (_, label, selected)) in options.iter().enumerate() {
                    let row_y = popup.y + 1.0 + i as f32 * SELECT_OPTION_HEIGHT;
                    if *selected {
                        crate::render::draw_solid_rect(
                            &mut composite_buffer,
                            win_w,
                            win_h,
                            popup.x + 1.0,
                            row_y,
                            popup.width - 2.0,
                            SELECT_OPTION_HEIGHT,
                            [0, 120, 215, 255],
                        );
                    }
                    let color = if *selected {
                        [1.0, 1.0, 1.0, 1.0]
                    } else {
                        [0.0, 0.0, 0.0, 1.0]
                    };
                    text_renderer.rasterize_to_bitmap_styled(
                        label,
                        13.0,
                        crate::css::DEFAULT_FONT_FAMILY,
                        color,
                        popup.x + 8.0,
                        row_y + 3.0,
                        popup.width - 16.0,
                        crate::css::TextStyleFlags::default(),
                        &mut composite_buffer,
                        win_w,
                        win_h,
                    );
                }
            }
        }

        // CRITICAL CLIPPING: Clear any page content (text/images) that overflowed into
        // the left tab bar (x < TAB_BAR_WIDTH) or top address bar (y < ADDRESS_BAR_HEIGHT)
        for py in 0..win_h {
            for px in 0..win_w {
                if px < TAB_BAR_WIDTH || py < ADDRESS_BAR_HEIGHT {
                    let idx = ((py * win_w + px) * 4) as usize;
                    if idx + 3 < composite_buffer.len() {
                        composite_buffer[idx] = 0;
                        composite_buffer[idx + 1] = 0;
                        composite_buffer[idx + 2] = 0;
                        composite_buffer[idx + 3] = 0;
                    }
                }
            }
        }

        self.draw_chrome_text(&mut text_renderer, &mut composite_buffer, win_w, win_h);

        // Draw Scrollbar (track and thumb)
        if let Some((track_x, track_y, track_w, track_h, thumb_x, thumb_y, thumb_w, thumb_h, _)) =
            self.get_scrollbar_metrics(win_w, win_h)
        {
            // Track background (subtle light gray)
            crate::render::draw_solid_rect(
                &mut composite_buffer,
                win_w,
                win_h,
                track_x,
                track_y,
                track_w,
                track_h,
                [230, 230, 230, 80],
            );

            // Thumb (rounded pill, styled with hover / drag states)
            let thumb_color = if self.is_dragging_scrollbar {
                [70, 70, 70, 230]
            } else if self.hovered_scrollbar {
                [100, 100, 100, 200]
            } else {
                [140, 140, 140, 160]
            };

            crate::render::draw_rounded_rect_fill(
                &mut composite_buffer,
                win_w,
                win_h,
                thumb_x,
                thumb_y,
                thumb_w,
                thumb_h,
                3.0,
                thumb_color,
            );
        }

        // Upload the composite bitmap to GPU
        if let Some(ref mut renderer) = self.renderer {
            renderer.set_text_bitmap(win_w, win_h, &composite_buffer);
        }
    }

    /// Load a page from HTML and CSS source strings.
    pub fn load_page(&mut self, html_source: &str, css_source: &str) {
        let w = self.window_width() as f32 - TAB_BAR_WIDTH as f32;
        let h = self.window_height() as f32 - ADDRESS_BAR_HEIGHT as f32;

        let new_page = crate::page::Page::new(html_source, css_source, w, h);
        if let Some(tab) = self.tab_manager.active_tab_mut() {
            tab.title = new_page.title.clone();
        }
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

    /// Load a page from a URL by fetching it over the network (adds to history).
    pub fn load_url(&mut self, url: &str) {
        self.load_url_internal(url, true);
    }

    /// Load a page without adding a new history entry (used for back/forward navigation).
    pub fn load_url_no_history(&mut self, url: &str) {
        self.load_url_internal(url, false);
    }

    /// Internal URL loading logic.
    fn load_url_internal(&mut self, url: &str, push_history: bool) {
        let trimmed = url.trim();
        let full_url = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else if trimmed.contains(' ') || !trimmed.contains('.') {
            format!(
                "https://www.google.com/search?q={}",
                trimmed.replace(' ', "+")
            )
        } else {
            format!("https://{}", trimmed)
        };

        let handle = self.tokio_rt.as_ref().map(|rt| rt.handle().clone());
        if let Some(handle) = handle {
            handle.block_on(self.load_url_async(&full_url, push_history));
        } else {
            log::error!("No tokio runtime available for loading URL");
        }
    }

    /// Load a page from a URL asynchronously with external CSS fetching.
    pub async fn load_url_async(&mut self, url: &str, push_history: bool) {
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
                let err_str = format!("{:?}", e);
                // An empty body is a distinct failure from a transport error:
                // the server answered fine, it just sent nothing to render.
                // Reporting it as "connection failed" would send the user
                // chasing their network instead of the real cause.
                let (err_code, reason) = match &e {
                    crate::network::NetworkError::EmptyResponse(detail)
                        if detail.contains("waf-action") =>
                    {
                        (
                            "ERR_BLOCKED_BY_BOT_PROTECTION",
                            "はボット対策により空の応答を返しました。通過には JavaScript の実行と Cookie の保存が必要です。",
                        )
                    }
                    crate::network::NetworkError::EmptyResponse(_) => (
                        "ERR_EMPTY_RESPONSE",
                        "は空の応答を返しました。表示できる内容がありません。",
                    ),
                    _ if err_str.contains("TimedOut") || err_str.contains("Timeout") => {
                        ("ERR_CONNECTION_TIMED_OUT", "からの応答時間が長すぎます。")
                    }
                    _ if err_str.contains("Connect") || err_str.contains("dns") => (
                        "ERR_NAME_NOT_RESOLVED",
                        "のサーバーの IP アドレスが見つかりませんでした。",
                    ),
                    _ => ("ERR_CONNECTION_FAILED", "への接続中に問題が発生しました。"),
                };

                let host = url
                    .strip_prefix("https://")
                    .or_else(|| url.strip_prefix("http://"))
                    .unwrap_or(url)
                    .split('/')
                    .next()
                    .unwrap_or(url);

                let search_query = host.replace('.', " ");
                let search_url = format!("https://www.google.com/search?q={}", search_query);

                let error_html = format!(
                    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<title>このサイトにアクセスできません</title>
<style>
  body {{
    background-color: #202124;
    color: #e8eaed;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
    margin: 0;
    padding: 80px 48px;
    line-height: 1.6;
  }}
  .container {{
    max-width: 600px;
    margin: 0 auto;
  }}
  .icon {{
    font-size: 56px;
    margin-bottom: 24px;
    color: #9aa0a6;
  }}
  h1 {{
    font-size: 24px;
    font-weight: 500;
    margin: 0 0 16px 0;
    color: #e8eaed;
  }}
  p {{
    font-size: 15px;
    color: #9aa0a6;
    margin: 0 0 16px 0;
  }}
  .host {{
    color: #e8eaed;
    font-weight: 600;
  }}
  ul {{
    margin: 8px 0 24px 20px;
    padding: 0;
    color: #9aa0a6;
    font-size: 14px;
  }}
  li {{
    margin-bottom: 6px;
  }}
  .error-code {{
    font-family: monospace;
    font-size: 12px;
    color: #5f6368;
    margin-top: 24px;
    margin-bottom: 32px;
  }}
  .btn-wrap {{
    margin-top: 32px;
  }}
  .btn {{
    display: inline-block;
    padding: 8px 24px;
    font-size: 14px;
    font-weight: 500;
    text-decoration: none;
    border-radius: 100px;
    background-color: #8ab4f8;
    color: #202124;
    text-align: center;
    margin-right: 12px;
  }}
  .btn-search {{
    display: inline-block;
    padding: 7px 23px;
    font-size: 14px;
    font-weight: 500;
    text-decoration: none;
    border-radius: 100px;
    background-color: transparent;
    color: #8ab4f8;
    border: 1px solid #5f6368;
    text-align: center;
  }}
</style>
</head>
<body>
  <div class="container">
    <div class="icon">📄</div>
    <h1>このサイトにアクセスできません</h1>
    <p><span class="host">{}</span> {}</p>
    <p>次をお試しください</p>
    <ul>
      <li>接続を確認する</li>
      <li>URL の綴りを確認する</li>
      <li>検索エンジンで正しいアドレスを調べる</li>
    </ul>
    <div class="error-code">{}</div>
    <div class="btn-wrap">
      <a href="{}" class="btn">再読み込み</a>
      <a href="{}" class="btn-search">Google で検索</a>
    </div>
  </div>
</body>
</html>"#,
                    host, reason, err_code, url, search_url
                );
                let error_css = "body { margin: 0; padding: 60px 48px; background-color: #202124; color: #e8eaed; }";
                self.load_page_async(&error_html, error_css, Some(url))
                    .await;
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

        // Update tab URL and history
        if let Some(tab) = self.tab_manager.active_tab_mut() {
            if push_history {
                tab.push_history(&final_url);
            }
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

    /// Download and register the fonts a page's `@font-face` rules declare.
    ///
    /// Each rule's sources are tried in order — that is what the list is for:
    /// the first entry the shaper can actually load wins. `local(...)` entries
    /// need no download, so they end the search only if the system already has
    /// that family; otherwise the search moves on to the next source.
    async fn load_web_fonts(
        font_faces: &[crate::css::parser::FontFaceRule],
        resolve: &impl Fn(&str) -> String,
    ) {
        use crate::css::parser::FontFaceSource;
        use crate::render::font_data;

        for face in font_faces {
            for source in &face.sources {
                let FontFaceSource::Url { url, format } = source else {
                    // A local() source is only usable if the system already has
                    // the family, in which case the normal font stack finds it.
                    continue;
                };
                if !font_data::is_supported_format(format.as_deref()) {
                    log::info!(
                        "skipping @font-face source for '{}': format {:?} is not supported",
                        face.family,
                        format
                    );
                    continue;
                }

                let resolved = resolve(url);
                let Ok(bytes) = crate::network::fetch_image(&resolved).await else {
                    log::warn!("failed to fetch web font {resolved}");
                    continue;
                };

                match font_data::to_sfnt(bytes) {
                    Ok(sfnt) => {
                        log::info!("registered web font '{}' from {resolved}", face.family);
                        crate::render::text::register_web_font(&face.family, sfnt);
                        break;
                    }
                    Err(e) => log::info!("web font {resolved} unusable: {e}"),
                }
            }
        }
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

        let image_nodes = crate::layout::collect_image_nodes(&new_page.layout_root);
        // Resolve against <base href> when the document declares one; it is
        // only knowable now that the HTML has been parsed.
        let effective_base = new_page.base_url();
        let resolve = |src: &str| -> String {
            if effective_base.is_empty() {
                src.to_string()
            } else {
                crate::network::resolve_url(&effective_base, src)
            }
        };

        let mut resolved_images: Vec<(String, f32, f32)> = image_nodes
            .iter()
            .map(|img| (resolve(&img.src), img.width, img.height))
            .collect();

        // CSS background images go through the same fetch and cache as <img>.
        // The requested size is the box they fill, which is what an SVG needs
        // to rasterize at a sensible resolution.
        for deco in crate::layout::collect_decorations(&new_page.layout_root) {
            if let Some(ref src) = deco.background_image {
                let url = resolve(src);
                if !resolved_images
                    .iter()
                    .any(|(existing, _, _)| existing == &url)
                {
                    resolved_images.push((url, deco.width, deco.height));
                }
            }
        }

        // Download the page's @font-face files before anything is measured
        // with them. The previous page's fonts go first — they are registered
        // process-wide, so leaving them would let one page's fonts leak into
        // the next.
        crate::render::text::clear_web_fonts();
        Self::load_web_fonts(&new_page.stylesheet.font_faces, &resolve).await;

        // Fetch all images concurrently with throttling
        use futures::StreamExt;
        let results = futures::stream::iter(resolved_images.into_iter().map(
            |(src, req_w, req_h)| async move {
                match crate::network::fetch_image(&src).await {
                    Ok(bytes) => {
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let rgba = img.to_rgba8();
                            let (iw, ih) = rgba.dimensions();
                            log::info!("Decoded image: {} ({}x{})", src, iw, ih);
                            Some((src, rgba.into_raw(), iw, ih))
                        } else if let Ok(svg_str) = std::str::from_utf8(&bytes) {
                            if let Some((rgba, iw, ih)) =
                                crate::render::render_svg_to_rgba(svg_str, req_w, req_h)
                            {
                                log::info!("Rendered SVG: {} ({}x{})", src, iw, ih);
                                Some((src, rgba, iw, ih))
                            } else {
                                log::warn!("Failed to render SVG: {}", src);
                                None
                            }
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            },
        ))
        .buffer_unordered(6)
        .collect::<Vec<_>>()
        .await;

        for result in results.into_iter().flatten() {
            let (src, rgba, width, height) = result;
            new_page.image_cache.insert(
                src,
                crate::page::CachedImage {
                    rgba,
                    width,
                    height,
                },
            );
        }

        // The first layout measured with system fonts because the web fonts had
        // not arrived yet. Re-measure now that they have, or every box sized
        // from text keeps the wrong width.
        if crate::render::text::web_font_count() > 0 {
            new_page.recompute_with_hover(&[]);
        }

        if let Some(tab) = self.tab_manager.active_tab_mut() {
            tab.title = new_page.title.clone();
        }
        self.tab_manager.set_active_tab_page(new_page);
        self.recompose();

        if let Some(ref mut renderer) = self.renderer {
            if let Err(e) = renderer.render() {
                log::error!("Render after load_page_async failed: {:?}", e);
            } else {
                log::info!("Page loaded (async) and rendered");
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
}

/// Recursively searches for an element with the given `id` or `name` in the layout tree and returns its Y position.
fn find_element_y_by_id(
    node: &crate::layout::LayoutNode,
    arena: &crate::html::DomArena,
    target_id: &str,
) -> Option<f32> {
    if let Some(dom_id) = node.dom_node_id {
        if let Some(dom_node) = arena.get(crate::html::DomHandle(crate::html::NodeId::from_raw(
            dom_id,
        ))) {
            if dom_node.get_attribute("id") == Some(target_id)
                || dom_node.get_attribute("name") == Some(target_id)
            {
                return Some(node.rect.y);
            }
        }
    }
    for child in &node.children {
        if let Some(y) = find_element_y_by_id(child, arena, target_id) {
            return Some(y);
        }
    }
    None
}

/// Searches from a starting DOM node up through its ancestors or down through its descendants
/// to find the closest `<input>` / `<textarea>` or `<a href="...">`.
fn find_link_or_input_at_dom_id(
    arena: &crate::html::DomArena,
    dom_id: u32,
) -> (Option<u32>, Option<String>) {
    let mut curr = Some(crate::html::NodeId::from_raw(dom_id));
    let mut clicked_input = None;
    let mut clicked_href = None;

    while let Some(nid) = curr {
        if let Some(node) = arena.get(crate::html::DomHandle(nid)) {
            let tag = node
                .tag_name()
                .map(|t| t.to_string())
                .unwrap_or_default()
                .to_lowercase();
            if tag == "input" || tag == "textarea" {
                if clicked_input.is_none() {
                    clicked_input = Some(nid.index() as u32);
                }
            }
            if tag == "a" {
                if clicked_href.is_none() {
                    if let Some(href) = node.get_attribute("href") {
                        if !href.trim().is_empty() {
                            clicked_href = Some(href.to_string());
                        }
                    }
                }
            }
            if clicked_input.is_some() || clicked_href.is_some() {
                break;
            }
            // If we are at a form or container, check its descendants for an input/textarea
            if (tag == "form" || tag == "div" || tag == "center") && clicked_input.is_none() {
                if let Some(desc_inp) = find_first_input_in_subtree(arena, nid) {
                    clicked_input = Some(desc_inp);
                }
            }
            curr = node.parent;
        } else {
            break;
        }
    }
    (clicked_input, clicked_href)
}

/// Walk up from `dom_id` looking for an enclosing `<select>`.
///
/// The hit test lands on whichever box holds the label text, which is the
/// select itself for a closed control — but going up costs nothing and covers
/// markup that wraps the label.
fn find_select_at_dom_id(arena: &crate::html::DomArena, dom_id: u32) -> Option<u32> {
    let mut curr = Some(crate::html::NodeId::from_raw(dom_id));
    while let Some(nid) = curr {
        let node = arena.get(crate::html::DomHandle(nid))?;
        let tag = node
            .tag_name()
            .map(|t| t.to_string())
            .unwrap_or_default()
            .to_lowercase();
        if tag == "select" {
            return Some(nid.index() as u32);
        }
        curr = node.parent;
    }
    None
}

/// Recursively finds the first `<input>` or `<textarea>` in a DOM subtree.
fn find_first_input_in_subtree(
    arena: &crate::html::DomArena,
    root: crate::html::NodeId,
) -> Option<u32> {
    if let Some(node) = arena.get(crate::html::DomHandle(root)) {
        let tag = node
            .tag_name()
            .map(|t| t.to_string())
            .unwrap_or_default()
            .to_lowercase();
        if tag == "input" || tag == "textarea" {
            let input_type = node
                .get_attribute("type")
                .unwrap_or_default()
                .to_lowercase();
            if input_type != "hidden" && input_type != "submit" && input_type != "button" {
                return Some(root.index() as u32);
            }
        }
        for &child in &node.children {
            if let Some(found) = find_first_input_in_subtree(arena, child) {
                return Some(found);
            }
        }
    }
    None
}

/// Searches the layout tree for the topmost interactive input/textarea containing or nearest to (x, y).
fn find_input_layout_at_pos(node: &crate::layout::LayoutNode, x: f32, y: f32) -> Option<u32> {
    if node.rect.contains(x, y) {
        if node.interaction_type == crate::layout::InteractionType::Input {
            if let Some(id) = node.dom_node_id {
                return Some(id);
            }
        }
        for child in node.children.iter().rev() {
            if let Some(id) = find_input_layout_at_pos(child, x, y) {
                return Some(id);
            }
        }
        for abs_child in node.absolute_children.iter().rev() {
            if let Some(id) = find_input_layout_at_pos(abs_child, x, y) {
                return Some(id);
            }
        }
    }
    None
}

/// Recursively searches for the layout rect of a given DOM node ID.
fn find_layout_rect_by_dom_id(
    node: &crate::layout::LayoutNode,
    dom_id: u32,
) -> Option<crate::layout::Rect> {
    if node.dom_node_id == Some(dom_id) {
        return Some(node.rect);
    }
    for child in &node.children {
        if let Some(r) = find_layout_rect_by_dom_id(child, dom_id) {
            return Some(r);
        }
    }
    None
}

/// Constructs the appropriate search/form submit URL given a form input and query string.
fn get_form_submit_url(page: &crate::page::Page, input_dom_id: u32, query_val: &str) -> String {
    let handle = crate::html::DomHandle(crate::html::NodeId::from_raw(input_dom_id));
    let mut param_name = "search".to_string();
    let mut form_action = String::new();

    if let Some(input_node) = page.arena.get(handle) {
        if let Some(name) = input_node.get_attribute("name") {
            if !name.is_empty() {
                param_name = name.to_string();
            }
        }

        // Traverse ancestors to find <form>
        let mut curr_id = input_node.parent;
        while let Some(pid) = curr_id {
            if let Some(pnode) = page.arena.get(crate::html::DomHandle(pid)) {
                let tag = pnode
                    .tag_name()
                    .map(|t| t.to_string())
                    .unwrap_or_default()
                    .to_lowercase();
                if tag == "form" {
                    if let Some(action) = pnode.get_attribute("action") {
                        form_action = action.to_string();
                    }
                    break;
                }
                curr_id = pnode.parent;
            } else {
                break;
            }
        }
    }

    if form_action.is_empty() {
        if page.page_url.contains("wikipedia.org") {
            format!(
                "https://ja.wikipedia.org/wiki/Special:Search?search={}",
                query_val
            )
        } else {
            format!("https://www.google.com/search?q={}", query_val)
        }
    } else {
        let base_resolved = crate::network::resolve_url(&page.base_url(), &form_action);
        let delimiter = if base_resolved.contains('?') {
            '&'
        } else {
            '?'
        };
        format!("{}{}{}={}", base_resolved, delimiter, param_name, query_val)
    }
}

impl MistilteinnApp {
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
        colors.push(ColorF { r, g, b, a: 1.0 });

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
                a: 1.0,
            });
        }
    }

    /// Get current inner window size or fallback.
    fn window_size(&self) -> (u32, u32) {
        self.renderer
            .as_ref()
            .map(|r| {
                (
                    r.window().inner_size().width,
                    r.window().inner_size().height,
                )
            })
            .unwrap_or((1280, 800))
    }

    /// Calculate scrollbar layout metrics:
    /// Returns `(track_x, track_y, track_w, track_h, thumb_x, thumb_y, thumb_w, thumb_h, max_scroll)` if scrollable.
    fn get_scrollbar_metrics(
        &self,
        win_w: u32,
        win_h: u32,
    ) -> Option<(f32, f32, f32, f32, f32, f32, f32, f32, f32)> {
        let page = self.tab_manager.get_active_tab_page()?;
        let content_h = page.layout_root.rect.height;
        let viewport_h = (win_h as f32 - ADDRESS_BAR_HEIGHT as f32).max(1.0);

        if content_h <= viewport_h {
            return None;
        }

        let scroll_y = self
            .tab_manager
            .get_active_tab_scroll()
            .map(|s| s.1)
            .unwrap_or(0.0);
        let max_scroll = (content_h - viewport_h).max(0.0);
        let thumb_h = (viewport_h / content_h * viewport_h)
            .max(SCROLLBAR_MIN_THUMB_HEIGHT)
            .min(viewport_h);
        let track_w = SCROLLBAR_WIDTH;
        let track_x = win_w as f32 - track_w;
        let track_y = ADDRESS_BAR_HEIGHT as f32;
        let track_h = viewport_h;

        let thumb_travel = (track_h - thumb_h).max(1.0);
        let thumb_y = track_y
            + if max_scroll > 0.0 {
                (scroll_y / max_scroll * thumb_travel).clamp(0.0, thumb_travel)
            } else {
                0.0
            };

        let thumb_w = track_w - 4.0;
        let thumb_x = track_x + 2.0;

        Some((
            track_x, track_y, track_w, track_h, thumb_x, thumb_y, thumb_w, thumb_h, max_scroll,
        ))
    }

    /// Hit-test the chrome area (tab bar, address bar, scrollbar).
    fn hit_test_chrome(&self, x: f32, y: f32) -> HitTestResult {
        // Check Scrollbar area (right edge)
        let (win_w, win_h) = self.window_size();
        if let Some((
            track_x,
            track_y,
            track_w,
            track_h,
            _thumb_x,
            thumb_y,
            _thumb_w,
            thumb_h,
            _max_scroll,
        )) = self.get_scrollbar_metrics(win_w, win_h)
        {
            if x >= track_x && x <= track_x + track_w && y >= track_y && y <= track_y + track_h {
                if y >= thumb_y && y <= thumb_y + thumb_h {
                    return HitTestResult::ScrollbarThumb;
                } else {
                    return HitTestResult::ScrollbarTrack;
                }
            }
        }

        // Check Address bar area (top)
        if y < ADDRESS_BAR_HEIGHT as f32 {
            if x >= TAB_BAR_WIDTH as f32 {
                let mut curr_x = TAB_BAR_WIDTH as f32;
                // Back button
                if x >= curr_x && x < curr_x + NAV_BUTTON_WIDTH {
                    return HitTestResult::BackButton;
                }
                curr_x += NAV_BUTTON_WIDTH;
                // Forward button
                if x >= curr_x && x < curr_x + NAV_BUTTON_WIDTH {
                    return HitTestResult::ForwardButton;
                }
                curr_x += NAV_BUTTON_WIDTH;
                // Reload button
                if x >= curr_x && x < curr_x + NAV_BUTTON_WIDTH {
                    return HitTestResult::ReloadButton;
                }
                curr_x += NAV_BUTTON_WIDTH;
                // Address bar input box
                if x >= curr_x {
                    return HitTestResult::AddressBar;
                }
            }
        }

        // Check Tab bar area (left)
        if x > TAB_BAR_WIDTH as f32 {
            return HitTestResult::Empty;
        }

        // Check + New Tab button at top
        if x >= TAB_BUTTON_X
            && x <= (TAB_BAR_WIDTH as f32 - TAB_BUTTON_RIGHT_MARGIN)
            && y >= 6.0
            && y <= 6.0 + NEW_TAB_BUTTON_HEIGHT
        {
            return HitTestResult::NewTabButton;
        }

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
                            // Check if click is on the '×' close button
                            if x >= TAB_BAR_WIDTH as f32
                                - TAB_BUTTON_RIGHT_MARGIN
                                - CLOSE_BUTTON_SIZE
                                - 4.0
                            {
                                return HitTestResult::CloseTabButton(tab.id);
                            }
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
                    // Check if click is on the '×' close button
                    if x >= TAB_BAR_WIDTH as f32 - TAB_BUTTON_RIGHT_MARGIN - CLOSE_BUTTON_SIZE - 4.0
                    {
                        return HitTestResult::CloseTabButton(tab.id);
                    }
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

    /// Creates a new blank tab, activates it, and focuses the address bar.
    pub fn create_new_tab(&mut self) -> crate::browser::tab::TabId {
        let id = self.tab_manager.create_tab();
        self.tab_manager.activate_tab(id);
        self.address_input.clear();
        self.address_cursor = 0;
        self.is_address_focused = true;
        self.focused_page_input = None;
        self.recompose();
        log::info!("Created and activated new tab {:?}", id);
        id
    }

    /// Closes a specified tab and updates active tab state.
    pub fn close_tab(&mut self, tab_id: crate::browser::tab::TabId) {
        self.group_manager.remove_tab_from_any_group(tab_id);
        self.tab_manager.close_tab(tab_id);
        if self.tab_manager.active_tab_id().is_none() {
            let new_id = self.tab_manager.create_tab();
            self.tab_manager.activate_tab(new_id);
        }
        if let Some(tab) = self.tab_manager.active_tab() {
            self.address_input = tab.url.clone();
        } else {
            self.address_input.clear();
        }
        self.address_cursor = self.address_input.len();
        self.is_address_focused = false;
        self.focused_page_input = None;
        self.recompose();
        log::info!("Closed tab {:?}", tab_id);
    }

    /// Reloads the currently active tab if it has a valid URL.
    pub fn reload_active_tab(&mut self) {
        if let Some(tab) = self.tab_manager.active_tab() {
            let url = tab.url.clone();
            if !url.is_empty() {
                log::info!("Reloading tab {:?}", tab.id);
                self.load_url_no_history(&url);
            }
        }
    }

    /// Handle a click while a `<select>`'s option list is open.
    ///
    /// Returns whether the click was consumed. An open list swallows the next
    /// click wherever it lands: inside it to choose, outside it to dismiss —
    /// which is what a dropdown does everywhere else.
    fn handle_open_select_click(&mut self, cx: f32, cy: f32) -> bool {
        let Some(open_id) = self.open_select else {
            return false;
        };
        self.open_select = None;

        // Resolve what was hit before borrowing the page mutably.
        let scroll = self
            .tab_manager
            .get_active_tab_scroll()
            .unwrap_or((0.0, 0.0));
        let hit = self.tab_manager.get_active_tab_page().and_then(|page| {
            let rect = find_layout_rect_by_dom_id(&page.layout_root, open_id)?;
            let options = page.select_options(open_id);
            let popup = select_popup_geometry(
                rect.x - scroll.0 + TAB_BAR_WIDTH as f32,
                rect.y - scroll.1 + ADDRESS_BAR_HEIGHT as f32,
                rect.width,
                rect.height,
                options.len(),
            );
            let index = select_option_at(&popup, cx, cy, options.len())?;
            options.get(index).map(|(id, _, _)| *id)
        });

        if let Some(option_id) = hit {
            if let Some(tab) = self.tab_manager.active_tab_mut() {
                if let Some(ref mut page) = tab.page {
                    page.select_option_and_recompute(open_id, option_id);
                }
            }
        }

        self.recompose();
        if let Some(ref renderer) = self.renderer {
            renderer.window().request_redraw();
        }
        true
    }

    /// Sets the winit window cursor icon from a computed CSS `cursor`.
    #[allow(deprecated)]
    fn set_winit_cursor(renderer: Option<&Renderer>, cursor: crate::css::Cursor) {
        if let Some(r) = renderer {
            // `cursor: none` hides the pointer rather than picking an icon.
            r.window()
                .set_cursor_visible(cursor != crate::css::Cursor::None);
            r.window().set_cursor_icon(cursor_icon_for(cursor));
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

            // Load the window icon
            let icon_bytes = include_bytes!("../assets/icon.jpg");
            let icon = image::load_from_memory(icon_bytes)
                .ok()
                .map(|img| img.into_rgba8())
                .and_then(|rgba| {
                    let (width, height) = rgba.dimensions();
                    winit::window::Icon::from_rgba(rgba.into_raw(), width, height).ok()
                });

            let mut window_attributes = WindowAttributes::default()
                .with_title(format!("Mistilteinn v{}", env!("CARGO_PKG_VERSION")))
                .with_inner_size(winit::dpi::PhysicalSize::new(1280, 800));

            if let Some(icon) = icon {
                window_attributes = window_attributes.with_window_icon(Some(icon));
            }

            let window = event_loop
                .create_window(window_attributes)
                .expect("Failed to create window");

            log::info!("Window created (1280x800)");

            // Use tokio runtime to run the async wgpu initialization.
            // wgpu requires an async runtime for adapter/device requests.
            let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
            let renderer = rt.block_on(async {
                match Renderer::new(window).await {
                    Ok(renderer) => Some(renderer),
                    Err(e) => {
                        log::error!("Failed to initialize renderer: {}", e);
                        None
                    }
                }
            });
            self.tokio_rt = Some(rt);
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
                let (win_w, win_h) = self.window_size();
                let max_scroll = self
                    .get_scrollbar_metrics(win_w, win_h)
                    .map(|m| m.8)
                    .unwrap_or(0.0);

                if let Some(scroll) = self.tab_manager.get_active_tab_scroll_mut() {
                    match delta {
                        winit::event::MouseScrollDelta::LineDelta(_dx, dy) => {
                            *scroll = (scroll.0, (scroll.1 - (dy * 40.0)).clamp(0.0, max_scroll));
                        }
                        winit::event::MouseScrollDelta::PixelDelta(pos) => {
                            *scroll = (scroll.0, (scroll.1 - pos.y as f32).clamp(0.0, max_scroll));
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
                            match self.hit_test_chrome(cx, cy) {
                                HitTestResult::ScrollbarThumb => {
                                    self.is_address_focused = false;
                                    self.is_dragging_scrollbar = true;
                                    self.scrollbar_drag_start_y = cy;
                                    self.scrollbar_drag_start_scroll_y = self
                                        .tab_manager
                                        .get_active_tab_scroll()
                                        .map(|s| s.1)
                                        .unwrap_or(0.0);
                                    self.recompose();
                                }
                                HitTestResult::ScrollbarTrack => {
                                    self.is_address_focused = false;
                                    let (win_w, win_h) = self.window_size();
                                    if let Some((
                                        _tx,
                                        track_y,
                                        _tw,
                                        track_h,
                                        _thumb_x,
                                        _thumb_y,
                                        _thumb_w,
                                        thumb_h,
                                        max_scroll,
                                    )) = self.get_scrollbar_metrics(win_w, win_h)
                                    {
                                        let thumb_travel = (track_h - thumb_h).max(1.0);
                                        let click_offset =
                                            (cy - track_y - thumb_h * 0.5).clamp(0.0, thumb_travel);
                                        let new_scroll = (click_offset / thumb_travel) * max_scroll;
                                        if let Some(scroll) =
                                            self.tab_manager.get_active_tab_scroll_mut()
                                        {
                                            scroll.1 = new_scroll.clamp(0.0, max_scroll);
                                        }
                                        self.is_dragging_scrollbar = true;
                                        self.scrollbar_drag_start_y = cy;
                                        self.scrollbar_drag_start_scroll_y = new_scroll;
                                        self.recompose();
                                    }
                                }
                                HitTestResult::GroupHeader(group_id) => {
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
                                HitTestResult::NewTabButton => {
                                    self.create_new_tab();
                                }
                                HitTestResult::CloseTabButton(tab_id) => {
                                    self.close_tab(tab_id);
                                }
                                HitTestResult::ReloadButton => {
                                    self.is_address_focused = false;
                                    self.reload_active_tab();
                                }
                                HitTestResult::TabButton(tab_id) => {
                                    self.tab_manager.activate_tab(tab_id);
                                    if let Some(tab) = self.tab_manager.active_tab() {
                                        self.address_input = tab.url.clone();
                                    }
                                    self.address_cursor = self.address_input.len();
                                    self.is_address_focused = false;
                                    self.recompose();
                                    log::info!("Activated tab {:?}", tab_id);
                                }
                                HitTestResult::AddressBar => {
                                    self.is_address_focused = true;
                                    self.address_cursor = self.address_input.len();
                                    self.recompose();
                                }
                                HitTestResult::BackButton => {
                                    self.is_address_focused = false;
                                    if let Some(tab) = self.tab_manager.active_tab_mut() {
                                        if let Some(url) = tab.go_back() {
                                            let url_clone = url.clone();
                                            self.address_input = url_clone.clone();
                                            self.load_url_no_history(&url_clone);
                                        }
                                    }
                                    self.recompose();
                                }
                                HitTestResult::ForwardButton => {
                                    self.is_address_focused = false;
                                    if let Some(tab) = self.tab_manager.active_tab_mut() {
                                        if let Some(url) = tab.go_forward() {
                                            let url_clone = url.clone();
                                            self.address_input = url_clone.clone();
                                            self.load_url_no_history(&url_clone);
                                        }
                                    }
                                    self.recompose();
                                }
                                HitTestResult::Empty => {
                                    if self.is_address_focused {
                                        self.is_address_focused = false;
                                    }

                                    // An open <select> list takes the click first — it
                                    // paints above the page, so it must hit-test above it.
                                    if self.handle_open_select_click(cx, cy) {
                                        return;
                                    }

                                    // Check page content area for input focus or link click
                                    if self.is_in_content_area(cx, cy) {
                                        let mut link_to_navigate: Option<String> = None;
                                        let mut anchor_jump_target: Option<String> = None;

                                        if let Some(tab) = self.tab_manager.active_tab_mut() {
                                            if let Some(ref mut page) = tab.page {
                                                let scroll_offset = tab.scroll_offset;
                                                let content_x =
                                                    cx - TAB_BAR_WIDTH as f32 + scroll_offset.0;
                                                let content_y = cy - ADDRESS_BAR_HEIGHT as f32
                                                    + scroll_offset.1;

                                                let dom_path = crate::layout::hit_test_dom_path(
                                                    &page.layout_root,
                                                    content_x,
                                                    content_y,
                                                );

                                                // A <select> swallows the click: it opens
                                                // its list rather than focusing anything.
                                                let clicked_select =
                                                    dom_path.iter().rev().find_map(|&node_id| {
                                                        find_select_at_dom_id(&page.arena, node_id)
                                                    });

                                                let mut clicked_input = None;
                                                let mut clicked_href: Option<String> = None;

                                                for &node_id in dom_path.iter().rev() {
                                                    let (inp, href) = find_link_or_input_at_dom_id(
                                                        &page.arena,
                                                        node_id,
                                                    );
                                                    if clicked_input.is_none() && inp.is_some() {
                                                        clicked_input = inp;
                                                    }
                                                    if clicked_href.is_none() && href.is_some() {
                                                        clicked_href = href;
                                                    }
                                                    if clicked_input.is_some()
                                                        || clicked_href.is_some()
                                                    {
                                                        break;
                                                    }
                                                }
                                                if clicked_select.is_some() {
                                                    clicked_input = None;
                                                    clicked_href = None;
                                                } else if clicked_input.is_none() {
                                                    clicked_input = find_input_layout_at_pos(
                                                        &page.layout_root,
                                                        content_x,
                                                        content_y,
                                                    );
                                                }
                                                self.focused_page_input = clicked_input;
                                                self.open_select = clicked_select;

                                                if let Some(href) = clicked_href {
                                                    let href_trimmed = href.trim();
                                                    if href_trimmed.starts_with('#') {
                                                        let target_id =
                                                            href_trimmed.trim_start_matches('#');
                                                        if !target_id.is_empty() {
                                                            anchor_jump_target =
                                                                Some(target_id.to_string());
                                                        }
                                                    } else if !href_trimmed.is_empty() {
                                                        let current_url = &page.page_url;
                                                        let resolved = crate::network::resolve_url(
                                                            current_url,
                                                            href_trimmed,
                                                        );
                                                        link_to_navigate = Some(resolved);
                                                    }
                                                }
                                            }
                                        }

                                        // Execute in-page anchor jump or URL navigation
                                        if let Some(target_id) = anchor_jump_target {
                                            let target_y = self
                                                .tab_manager
                                                .get_active_tab_page()
                                                .and_then(|page| {
                                                    find_element_y_by_id(
                                                        &page.layout_root,
                                                        &page.arena,
                                                        &target_id,
                                                    )
                                                });

                                            if let Some(target_y) = target_y {
                                                let (win_w, win_h) = self.window_size();
                                                let max_scroll = self
                                                    .get_scrollbar_metrics(win_w, win_h)
                                                    .map(|m| m.8)
                                                    .unwrap_or(0.0);
                                                if let Some(tab) = self.tab_manager.active_tab_mut()
                                                {
                                                    tab.scroll_offset.1 =
                                                        target_y.clamp(0.0, max_scroll);
                                                    log::info!(
                                                        "Jumped to anchor #{}: y={}",
                                                        target_id,
                                                        tab.scroll_offset.1
                                                    );
                                                }
                                            }
                                        } else if let Some(target_url) = link_to_navigate {
                                            log::info!(
                                                "Clicked link, navigating to: {}",
                                                target_url
                                            );
                                            self.address_input = target_url.clone();
                                            self.load_url(&target_url);
                                            return;
                                        }
                                    } else {
                                        self.focused_page_input = None;
                                    }
                                    self.recompose();
                                }
                            }
                        } else if state == ElementState::Released {
                            if self.is_dragging_scrollbar {
                                self.is_dragging_scrollbar = false;
                                self.recompose();
                            }
                        }
                    }
                    MouseButton::Right => {
                        // Right-click on a tab closes it
                        if state == ElementState::Pressed && cx < TAB_BAR_WIDTH as f32 {
                            if let Some(clicked_tab) = self.hit_test_chrome(cx, cy).into_tab_id() {
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

                // Handle scrollbar dragging
                if self.is_dragging_scrollbar {
                    let (win_w, win_h) = self.window_size();
                    if let Some((
                        _tx,
                        _ty,
                        _tw,
                        track_h,
                        _thumb_x,
                        _thumb_y,
                        _thumb_w,
                        thumb_h,
                        max_scroll,
                    )) = self.get_scrollbar_metrics(win_w, win_h)
                    {
                        let delta_y = cy - self.scrollbar_drag_start_y;
                        let thumb_travel = (track_h - thumb_h).max(1.0);
                        let new_scroll = self.scrollbar_drag_start_scroll_y
                            + (delta_y / thumb_travel) * max_scroll;
                        if let Some(scroll) = self.tab_manager.get_active_tab_scroll_mut() {
                            scroll.1 = new_scroll.clamp(0.0, max_scroll);
                        }
                        self.recompose();
                        if let Some(ref renderer) = self.renderer {
                            renderer.window().request_redraw();
                        }
                    }
                }

                // Hit-test tab bar for hover highlight
                let new_hovered_tab = if cx < TAB_BAR_WIDTH as f32 && cy > 0.0 {
                    match self.hit_test_chrome(cx, cy) {
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

                // Hit-test scrollbar for hover highlight
                let (win_w, win_h) = self.window_size();
                let is_over_scrollbar =
                    if let Some((track_x, track_y, track_w, track_h, _, _, _, _, _)) =
                        self.get_scrollbar_metrics(win_w, win_h)
                    {
                        cx >= track_x
                            && cx <= track_x + track_w
                            && cy >= track_y
                            && cy <= track_y + track_h
                    } else {
                        false
                    };
                let scrollbar_hover_changed = self.hovered_scrollbar != is_over_scrollbar;
                self.hovered_scrollbar = is_over_scrollbar;

                if tab_changed || addr_changed || scrollbar_hover_changed {
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

                        // An explicit CSS `cursor` wins outright; `auto` falls
                        // back to what the element under the pointer is.
                        let mut cursor_kind =
                            crate::layout::hit_test_cursor(&page.layout_root, content_x, content_y);

                        if cursor_kind == crate::css::Cursor::Auto {
                            cursor_kind = crate::layout::hit_test_interactive(
                                &page.layout_root,
                                content_x,
                                content_y,
                            )
                            .map(|interaction| match interaction {
                                crate::layout::InteractionType::Link => crate::css::Cursor::Pointer,
                                crate::layout::InteractionType::Input => crate::css::Cursor::Text,
                                // A select is not a text field; browsers keep
                                // the arrow over it.
                                crate::layout::InteractionType::Select => {
                                    crate::css::Cursor::Default
                                }
                                _ => crate::css::Cursor::Auto,
                            })
                            .unwrap_or(crate::css::Cursor::Auto);
                        }

                        // :hover — hit-test for DOM node path (includes ancestors)
                        let dom_path = crate::layout::hit_test_dom_path(
                            &page.layout_root,
                            content_x,
                            content_y,
                        );

                        if cursor_kind == crate::css::Cursor::Auto {
                            for &node_id in dom_path.iter().rev() {
                                let (inp, href) =
                                    find_link_or_input_at_dom_id(&page.arena, node_id);
                                if inp.is_some() {
                                    cursor_kind = crate::css::Cursor::Text;
                                    break;
                                }
                                if href.is_some() {
                                    cursor_kind = crate::css::Cursor::Pointer;
                                    break;
                                }
                            }
                        }

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
                    Self::set_winit_cursor(self.renderer.as_ref(), crate::css::Cursor::Default);
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

                if event.state == ElementState::Pressed {
                    if self.ctrl_pressed {
                        match event.physical_key {
                            PhysicalKey::Code(KeyCode::KeyT) => {
                                // Ctrl+T: New Tab
                                self.create_new_tab();
                            }
                            PhysicalKey::Code(KeyCode::KeyW) => {
                                // Ctrl+W: Close active Tab
                                if let Some(active_id) = self.tab_manager.active_tab_id() {
                                    self.close_tab(active_id);
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyR) => {
                                // Ctrl+R: Reload active Tab
                                self.reload_active_tab();
                            }
                            PhysicalKey::Code(KeyCode::KeyL) => {
                                // Ctrl+L: Focus address bar
                                self.is_address_focused = true;
                                self.address_cursor = self.address_input.len();
                                self.focused_page_input = None;
                                self.recompose();
                            }
                            PhysicalKey::Code(KeyCode::Tab) => {
                                // Ctrl+Tab: switch to next tab
                                let tabs: Vec<_> =
                                    self.tab_manager.all_tabs().map(|t| t.id).collect();
                                if let Some(current) = self.tab_manager.active_tab_id() {
                                    let idx =
                                        tabs.iter().position(|&id| id == current).unwrap_or(0);
                                    let next_idx = (idx + 1) % tabs.len();
                                    self.tab_manager.activate_tab(tabs[next_idx]);
                                    if let Some(tab) = self.tab_manager.active_tab() {
                                        self.address_input = tab.url.clone();
                                    }
                                    self.address_cursor = self.address_input.len();
                                    self.recompose();
                                    log::info!("Switched to tab {:?}", tabs[next_idx]);
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyG) => {
                                // Ctrl+G: create group for active tab
                                self.create_group_for_active_tab();
                            }
                            PhysicalKey::Code(KeyCode::KeyV) => {
                                // Ctrl+V: paste into address bar
                                if self.is_address_focused {
                                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                        if let Ok(text) = clipboard.get_text() {
                                            let cleaned_text = text.trim();
                                            self.address_input
                                                .insert_str(self.address_cursor, cleaned_text);
                                            self.address_cursor += cleaned_text.len();
                                            self.recompose();
                                            if let Some(ref renderer) = self.renderer {
                                                renderer.window().request_redraw();
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    } else if event.physical_key == PhysicalKey::Code(KeyCode::F5) {
                        // F5: Reload active tab
                        self.reload_active_tab();
                    } else if self.is_address_focused {
                        use winit::keyboard::{Key, NamedKey};
                        let mut changed = false;
                        match &event.logical_key {
                            Key::Named(NamedKey::Backspace) => {
                                if self.address_cursor > 0 {
                                    self.address_cursor -= 1;
                                    self.address_input.remove(self.address_cursor);
                                    changed = true;
                                }
                            }
                            Key::Named(NamedKey::Delete) => {
                                if self.address_cursor < self.address_input.len() {
                                    self.address_input.remove(self.address_cursor);
                                    changed = true;
                                }
                            }
                            Key::Named(NamedKey::ArrowLeft) => {
                                if self.address_cursor > 0 {
                                    self.address_cursor -= 1;
                                    changed = true;
                                }
                            }
                            Key::Named(NamedKey::ArrowRight) => {
                                if self.address_cursor < self.address_input.len() {
                                    self.address_cursor += 1;
                                    changed = true;
                                }
                            }
                            Key::Named(NamedKey::Enter) => {
                                let url = self.address_input.trim().to_string();
                                self.load_url(&url);
                                self.is_address_focused = false;
                                changed = true;
                            }
                            Key::Character(c) => {
                                if c.chars().count() == 1 {
                                    let ch = c.chars().next().unwrap();
                                    if !ch.is_control() {
                                        self.address_input.insert(self.address_cursor, ch);
                                        self.address_cursor += ch.len_utf8();
                                        changed = true;
                                    }
                                }
                            }
                            _ => {}
                        }

                        if changed {
                            self.recompose();
                            if let Some(ref renderer) = self.renderer {
                                renderer.window().request_redraw();
                            }
                        }
                    } else if let Some(input_node_id) = self.focused_page_input {
                        use winit::keyboard::{Key, NamedKey};
                        let mut changed = false;
                        let mut submit_url = None;

                        if let Some(tab) = self.tab_manager.active_tab_mut() {
                            if let Some(ref mut page) = tab.page {
                                let mut cur_val = page
                                    .arena
                                    .get_attribute(input_node_id, "value")
                                    .unwrap_or_default();
                                match &event.logical_key {
                                    Key::Named(NamedKey::Backspace) => {
                                        if !cur_val.is_empty() {
                                            cur_val.pop();
                                            page.set_input_value_and_recompute(
                                                input_node_id,
                                                &cur_val,
                                            );
                                            changed = true;
                                        }
                                    }
                                    Key::Named(NamedKey::Enter) => {
                                        if !cur_val.trim().is_empty() {
                                            submit_url = Some(get_form_submit_url(
                                                page,
                                                input_node_id,
                                                cur_val.trim(),
                                            ));
                                        }
                                    }
                                    Key::Character(c) => {
                                        for ch in c.chars() {
                                            if !ch.is_control() {
                                                cur_val.push(ch);
                                                changed = true;
                                            }
                                        }
                                        if changed {
                                            page.set_input_value_and_recompute(
                                                input_node_id,
                                                &cur_val,
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        if let Some(url) = submit_url {
                            log::info!("Submitting page form to: {}", url);
                            self.address_input = url.clone();
                            self.load_url(&url);
                        } else if changed {
                            self.recompose();
                            if let Some(ref renderer) = self.renderer {
                                renderer.window().request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                if self.is_address_focused {
                    for ch in text.chars() {
                        self.address_input.insert(self.address_cursor, ch);
                        self.address_cursor += ch.len_utf8();
                    }
                    self.recompose();
                    if let Some(ref renderer) = self.renderer {
                        renderer.window().request_redraw();
                    }
                } else if let Some(input_node_id) = self.focused_page_input {
                    if let Some(tab) = self.tab_manager.active_tab_mut() {
                        if let Some(ref mut page) = tab.page {
                            let mut cur_val = page
                                .arena
                                .get_attribute(input_node_id, "value")
                                .unwrap_or_default();
                            cur_val.push_str(&text);
                            page.set_input_value_and_recompute(input_node_id, &cur_val);
                            self.recompose();
                            if let Some(ref renderer) = self.renderer {
                                renderer.window().request_redraw();
                            }
                        }
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
        tokio_rt: None,
        cursor_pos: (0.0, 0.0),
        ctrl_pressed: false,
        hovered_tab_id: None,
        hovered_address_bar: false,
        prev_hovered_dom_id: None,
        address_input: String::new(),
        is_address_focused: false,
        address_cursor: 0,
        focused_page_input: None,
        open_select: None,
        is_dragging_scrollbar: false,
        scrollbar_drag_start_y: 0.0,
        scrollbar_drag_start_scroll_y: 0.0,
        hovered_scrollbar: false,
    };
    event_loop.run_app(&mut app).expect("Event loop failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrollbar_metrics_no_page() {
        let app = MistilteinnApp {
            renderer: None,
            start_url: None,
            tab_manager: crate::browser::tab::TabManager::new(),
            group_manager: crate::browser::tab_group::GroupManager::new(),
            tokio_rt: None,
            cursor_pos: (0.0, 0.0),
            ctrl_pressed: false,
            hovered_tab_id: None,
            hovered_address_bar: false,
            prev_hovered_dom_id: None,
            address_input: String::new(),
            is_address_focused: false,
            address_cursor: 0,
            focused_page_input: None,
            open_select: None,
            is_dragging_scrollbar: false,
            scrollbar_drag_start_y: 0.0,
            scrollbar_drag_start_scroll_y: 0.0,
            hovered_scrollbar: false,
        };

        assert!(app.get_scrollbar_metrics(1280, 800).is_none());
    }

    #[test]
    fn test_scrollbar_metrics_with_long_page() {
        let mut app = MistilteinnApp {
            renderer: None,
            start_url: None,
            tab_manager: crate::browser::tab::TabManager::new(),
            group_manager: crate::browser::tab_group::GroupManager::new(),
            tokio_rt: None,
            cursor_pos: (0.0, 0.0),
            ctrl_pressed: false,
            hovered_tab_id: None,
            hovered_address_bar: false,
            prev_hovered_dom_id: None,
            address_input: String::new(),
            is_address_focused: false,
            address_cursor: 0,
            focused_page_input: None,
            open_select: None,
            is_dragging_scrollbar: false,
            scrollbar_drag_start_y: 0.0,
            scrollbar_drag_start_scroll_y: 0.0,
            hovered_scrollbar: false,
        };

        let tab_id = app.tab_manager.create_tab();
        app.tab_manager.activate_tab(tab_id);

        let mut page = crate::page::Page::new(
            "<html><body><div style='height: 2000px;'>Long Content</div></body></html>",
            "",
            1080.0,
            760.0,
        );
        page.layout_root.rect.height = 2000.0;
        app.tab_manager.set_active_tab_page(page);

        let metrics = app.get_scrollbar_metrics(1280, 800);
        assert!(
            metrics.is_some(),
            "Long page should produce scrollbar metrics"
        );

        let (track_x, track_y, track_w, track_h, thumb_x, thumb_y, thumb_w, thumb_h, max_scroll) =
            metrics.unwrap();
        assert_eq!(track_x, 1280.0 - SCROLLBAR_WIDTH);
        assert_eq!(track_y, ADDRESS_BAR_HEIGHT as f32);
        assert_eq!(track_w, SCROLLBAR_WIDTH);
        assert_eq!(track_h, 800.0 - ADDRESS_BAR_HEIGHT as f32);
        assert!(thumb_w < track_w);
        assert_eq!(thumb_x, track_x + 2.0);
        assert_eq!(thumb_y, track_y);
        assert!(thumb_h >= SCROLLBAR_MIN_THUMB_HEIGHT);
        assert_eq!(max_scroll, 2000.0 - (800.0 - ADDRESS_BAR_HEIGHT as f32));
    }

    #[test]
    fn test_find_element_y_by_id() {
        let page = crate::page::Page::new(
            r#"<html><body>
                <div id="top" style="height: 100px;">Top</div>
                <div id="target" style="height: 200px;">Target Section</div>
              </body></html>"#,
            "",
            800.0,
            600.0,
        );

        let y = find_element_y_by_id(&page.layout_root, &page.arena, "target");
        assert!(y.is_some(), "Element with id='target' should be found");
        assert_eq!(
            find_element_y_by_id(&page.layout_root, &page.arena, "nonexistent"),
            None
        );
    }

    #[test]
    fn a_select_list_hangs_below_the_control() {
        let popup = select_popup_geometry(100.0, 50.0, 120.0, 24.0, 3);
        assert_eq!(popup.x, 100.0);
        assert_eq!(popup.y, 74.0, "directly under the control");
        assert_eq!(popup.width, 120.0, "at least as wide as the control");
        assert_eq!(popup.height, 3.0 * SELECT_OPTION_HEIGHT + 2.0);
    }

    #[test]
    fn a_long_select_list_stops_growing() {
        // A hundred options must not paint over the whole window.
        let popup = select_popup_geometry(0.0, 0.0, 100.0, 24.0, 100);
        assert_eq!(
            popup.height,
            SELECT_MAX_VISIBLE_OPTIONS as f32 * SELECT_OPTION_HEIGHT + 2.0
        );
    }

    #[test]
    fn clicks_map_to_the_option_row_under_them() {
        let popup = select_popup_geometry(100.0, 50.0, 120.0, 24.0, 3);

        assert_eq!(select_option_at(&popup, 110.0, popup.y + 2.0, 3), Some(0));
        assert_eq!(
            select_option_at(&popup, 110.0, popup.y + 1.0 + SELECT_OPTION_HEIGHT + 2.0, 3),
            Some(1)
        );
        assert_eq!(
            select_option_at(&popup, 110.0, popup.y + popup.height - 2.0, 3),
            Some(2)
        );
    }

    #[test]
    fn clicks_outside_the_list_select_nothing() {
        let popup = select_popup_geometry(100.0, 50.0, 120.0, 24.0, 3);
        assert_eq!(select_option_at(&popup, 50.0, popup.y + 2.0, 3), None);
        assert_eq!(select_option_at(&popup, 110.0, popup.y - 5.0, 3), None);
        assert_eq!(
            select_option_at(&popup, 110.0, popup.y + popup.height + 5.0, 3),
            None
        );
    }

    #[test]
    fn test_get_form_submit_url() {
        let mut page = crate::page::Page::new(
            r#"<html><body>
                <form action="/search" method="get">
                    <input name="q" value="rust" />
                </form>
              </body></html>"#,
            "",
            800.0,
            600.0,
        );
        page.page_url = "https://example.com/index.html".to_string();

        fn find_input(
            node: &crate::layout::LayoutNode,
            arena: &crate::html::DomArena,
        ) -> Option<u32> {
            if let Some(dom_id) = node.dom_node_id {
                if let Some(dom_node) = arena.get(crate::html::DomHandle(
                    crate::html::NodeId::from_raw(dom_id),
                )) {
                    if dom_node
                        .tag_name()
                        .map(|t| t.to_string())
                        .unwrap_or_default()
                        .to_lowercase()
                        == "input"
                    {
                        return Some(dom_id);
                    }
                }
            }
            for child in &node.children {
                if let Some(id) = find_input(child, arena) {
                    return Some(id);
                }
            }
            None
        }

        let input_id = find_input(&page.layout_root, &page.arena).expect("input node should exist");
        let submit_url = get_form_submit_url(&page, input_id, "rust browser");
        assert_eq!(submit_url, "https://example.com/search?q=rust browser");
    }
}
